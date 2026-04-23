#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, BytesN, Env, String, Vec};

// --- Storage TTL Constants ---
const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;

const PERSISTENT_LIFETIME_THRESHOLD: u32 = 100_000;
const PERSISTENT_BUMP_AMOUNT: u32 = 120_000;

// ---------------------------------------------------------------------------
// Minimal payment contract client — only the fields we need from get_payment.
// ---------------------------------------------------------------------------
mod payment_contract {
    use soroban_sdk::{contractclient, contracttype, Address, Env, Map, String};

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub enum PaymentStatus {
        Pending = 0,
        Completed = 1,
        Refunded = 2,
        Disputed = 3,
        Expired = 4,
    }

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct Payment {
        pub id: u32,
        pub customer: Address,
        pub merchant: Address,
        pub amount: i128,
        pub token: Address,
        pub status: PaymentStatus,
        pub created_at: u64,
        pub expires_at: u64,
        pub refunded_amount: i128,
        pub reference: Option<String>,
        pub metadata: Option<Map<String, String>>,
    }

    #[allow(dead_code)]
    #[contractclient(name = "PaymentContractClient")]
    pub trait PaymentContractInterface {
        fn get_payment(env: Env, payment_id: u32) -> Payment;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum RefundStatus {
    Requested = 0,
    Approved = 1,
    Rejected = 2,
    Processed = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refund {
    pub id: u32,
    pub payment_id: u32,
    pub customer: Address,
    pub merchant: Address,
    pub amount: i128,
    pub token: Address,
    pub status: RefundStatus,
    pub reason: String,
    pub requested_at: u64,
    pub approved_at: Option<u64>,
    pub processed_at: Option<u64>,
    pub auto_approved_source: Option<String>, // "whitelist" or "dispute_window"
    pub escrow_id: Option<u32>,                // For cross-contract escrow refunds
    pub fee_amount: Option<i128>,              // Fee deducted on processing
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundStats {
    pub total_requested: u32,
    pub total_approved: u32,
    pub total_rejected: u32,
    pub total_processed: u32,
    pub total_amount_refunded: i128,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    Paused,
    PauseReason,
    RefundCounter,
    ContractVersion,
    MigrationCompleted(u32),
    Refund(u32),
    /// Address of the payment contract for cross-contract validation (#64).
    PaymentContractAddress,
    /// Index: customer → Vec<u32> of refund IDs
    CustomerRefunds(Address),
    /// Index: merchant → Vec<u32> of refund IDs
    MerchantRefunds(Address),
    /// Index: payment_id → Vec<u32> of refund IDs
    PaymentRefunds(u32),
    /// Dispute window in seconds; after this period a Requested refund can be auto-approved.
    DisputeWindow,
    /// Whitelist of auto-approved merchants (Issue #163)
    AutoApprovedMerchants,
    /// Escrow contract address for cross-contract refund registration (Issue #162)
    EscrowContractAddress,
    /// Global refund statistics (Issue #161)
    GlobalRefundStats,
    /// Per-merchant refund statistics (Issue #161)
    MerchantRefundStats(Address),
    /// Refund processing fee in basis points (Issue #160)
    RefundFeeBps,
    /// Fee recipient address (Issue #160)
    FeeRecipient,
}

mod events;

#[contract]
pub struct AhjoorRefundContract;

#[contractimpl]
impl AhjoorRefundContract {
    /// Initialize the refund contract with an admin, the payment contract address, a
    /// `dispute_window` (in seconds), optional escrow contract address, and refund fee parameters.
    pub fn initialize(
        env: Env,
        admin: Address,
        payment_contract: Address,
        dispute_window: u64,
        escrow_contract: Option<Address>,
        refund_fee_bps: u32,
        fee_recipient: Option<Address>,
    ) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        // Validate fee cap (max 200 bps = 2%)
        if refund_fee_bps > 200 {
            panic!("Refund fee cannot exceed 200 basis points (2%)");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::RefundCounter, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::ContractVersion, &1u32);
        env.storage()
            .instance()
            .set(&DataKey::PaymentContractAddress, &payment_contract);
        env.storage()
            .instance()
            .set(&DataKey::DisputeWindow, &dispute_window);

        // Issue #162: Store escrow contract address
        if let Some(escrow_addr) = escrow_contract {
            env.storage()
                .instance()
                .set(&DataKey::EscrowContractAddress, &escrow_addr);
        }

        // Issue #160: Store fee configuration
        env.storage()
            .instance()
            .set(&DataKey::RefundFeeBps, &refund_fee_bps);
        if let Some(recipient) = fee_recipient {
            env.storage()
                .instance()
                .set(&DataKey::FeeRecipient, &recipient);
        }

        // Issue #161: Initialize global stats
        let initial_stats = RefundStats {
            total_requested: 0,
            total_approved: 0,
            total_rejected: 0,
            total_processed: 0,
            total_amount_refunded: 0,
        };
        env.storage()
            .instance()
            .set(&DataKey::GlobalRefundStats, &initial_stats);

        // Issue #163: Initialize empty whitelist
        let empty_whitelist: Vec<Address> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&DataKey::AutoApprovedMerchants, &empty_whitelist);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Request a refund linked to an existing completed payment.
    /// Cross-contract validates: payment exists, status is Completed, merchant matches,
    /// and refund amount does not exceed the original payment amount (#64).
    /// If merchant is whitelisted, auto-approves immediately (Issue #163).
    /// Returns the refund ID.
    pub fn request_refund(
        env: Env,
        customer: Address,
        payment_id: u32,
        amount: i128,
        reason: String,
    ) -> u32 {
        Self::require_not_paused(&env);
        customer.require_auth();

        if amount <= 0 {
            panic!("Refund amount must be positive");
        }

        // --- Cross-contract validation (#64) ---
        let payment_contract_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::PaymentContractAddress)
            .expect("Payment contract not configured");

        let payment_client =
            payment_contract::PaymentContractClient::new(&env, &payment_contract_addr);
        let payment = payment_client
            .try_get_payment(&payment_id)
            .unwrap_or_else(|_| panic!("PaymentContractError: payment not found"))
            .unwrap_or_else(|_| panic!("PaymentContractError: payment not found"));

        // Validate payment status is Completed
        if payment.status != payment_contract::PaymentStatus::Completed {
            panic!("PaymentContractError: payment is not completed");
        }

        // Validate merchant matches the payment's merchant
        // (customer is the one requesting, merchant is cached for audit)
        let merchant = payment.merchant.clone();

        // Validate refund amount does not exceed original payment amount
        let remaining = payment.amount - payment.refunded_amount;
        if amount > remaining {
            panic!("PaymentAmountMismatch: refund amount exceeds remaining payment amount");
        }

        // Cache validated payment data — token comes from the payment record
        let token = payment.token.clone();

        // Escrow funds to this contract so approved refunds can be processed.
        let client = token::Client::new(&env, &token);
        client.transfer(&customer, &env.current_contract_address(), &amount);

        let refund_id = Self::next_refund_id(&env);

        let is_whitelisted = Self::is_merchant_auto_approved(&env, &merchant);

        let initial_status = if is_whitelisted {
            RefundStatus::Approved
        } else {
            RefundStatus::Requested
        };

        let now = env.ledger().timestamp();
        let refund = Refund {
            id: refund_id,
            payment_id,
            customer: customer.clone(),
            merchant: merchant.clone(),
            amount,
            token: token.clone(),
            status: initial_status,
            reason,
            requested_at: now,
            approved_at: if is_whitelisted { Some(now) } else { None },
            processed_at: None,
            auto_approved_source: if is_whitelisted {
                Some(String::from_str(&env, "whitelist"))
            } else {
                None
            },
            escrow_id: None,
            fee_amount: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::append_index(&env, &DataKey::CustomerRefunds(customer.clone()), refund_id);
        Self::append_index(&env, &DataKey::MerchantRefunds(merchant.clone()), refund_id);
        Self::append_index(&env, &DataKey::PaymentRefunds(payment_id), refund_id);

        Self::update_stats_on_request(&env, &merchant, amount);

        events::emit_refund_requested(&env, refund_id, customer, amount, token, refund.reason);

        if is_whitelisted {
            events::emit_refund_auto_approved_whitelist(&env, refund_id, merchant, amount);
        }

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        refund_id
    }

    /// Approve a refund request. Only admin can call this.
    pub fn approve_refund(env: Env, admin: Address, refund_id: u32) {
        Self::require_not_paused(&env);
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");

        if admin != stored_admin {
            panic!("Only admin can approve refunds");
        }

        let mut refund: Refund = env
            .storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .expect("Refund not found");

        if refund.status != RefundStatus::Requested {
            panic!("Refund is not in requested status");
        }

        refund.status = RefundStatus::Approved;
        refund.approved_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::update_stats_on_approve(&env, &refund.merchant);

        events::emit_refund_approved(&env, refund_id, admin, refund.approved_at.unwrap());

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Reject a refund request. Only admin can call this.
    pub fn reject_refund(env: Env, admin: Address, refund_id: u32, rejection_reason: String) {
        Self::require_not_paused(&env);
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");

        if admin != stored_admin {
            panic!("Only admin can reject refunds");
        }

        let mut refund: Refund = env
            .storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .expect("Refund not found");

        if refund.status != RefundStatus::Requested {
            panic!("Refund is not in requested status");
        }

        refund.status = RefundStatus::Rejected;

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::update_stats_on_reject(&env, &refund.merchant);

        events::emit_refund_rejected(
            &env,
            refund_id,
            admin,
            rejection_reason,
            env.ledger().timestamp(),
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn process_refund(env: Env, admin: Address, refund_id: u32) {
        Self::require_not_paused(&env);
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");

        if admin != stored_admin {
            panic!("Only admin can process refunds");
        }

        let mut refund: Refund = env
            .storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .expect("Refund not found");

        if refund.status != RefundStatus::Approved {
            panic!("Refund is not approved");
        }

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RefundFeeBps)
            .unwrap_or(0);

        let fee_amount = if fee_bps > 0 {
            (refund.amount as u128 * fee_bps as u128 / 10_000) as i128
        } else {
            0
        };

        let customer_amount = refund.amount - fee_amount;

        // Transfer tokens to customer
        let client = token::Client::new(&env, &refund.token);
        if customer_amount > 0 {
            client.transfer(
                &env.current_contract_address(),
                &refund.customer,
                &customer_amount,
            );
        }

        // Transfer fee to fee recipient if configured
        if fee_amount > 0 {
            if let Some(fee_recipient) = env
                .storage()
                .instance()
                .get::<DataKey, Address>(&DataKey::FeeRecipient)
            {
                client.transfer(
                    &env.current_contract_address(),
                    &fee_recipient,
                    &fee_amount,
                );
                events::emit_refund_fee_collected(&env, refund_id, fee_amount);
            }
        }

        refund.status = RefundStatus::Processed;
        refund.processed_at = Some(env.ledger().timestamp());
        refund.fee_amount = if fee_amount > 0 { Some(fee_amount) } else { None };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::update_stats_on_process(&env, &refund.merchant, refund.amount);

        events::emit_refund_processed(
            &env,
            refund_id,
            refund.customer,
            customer_amount,
            refund.processed_at.unwrap(),
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Auto-approve a refund once the dispute window has elapsed without merchant action.
    /// Callable by anyone. Panics if the merchant has already approved or rejected the refund,
    /// or if the dispute window has not yet elapsed.
    pub fn auto_approve_refund(env: Env, refund_id: u32) {
        Self::require_not_paused(&env);

        let mut refund: Refund = env
            .storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .expect("Refund not found");

        if refund.status != RefundStatus::Requested {
            panic!("Refund has already been acted on");
        }

        let dispute_window: u64 = env
            .storage()
            .instance()
            .get(&DataKey::DisputeWindow)
            .expect("Dispute window not configured");

        let now = env.ledger().timestamp();
        if now < refund.requested_at + dispute_window {
            panic!("Dispute window has not elapsed");
        }

        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RefundFeeBps)
            .unwrap_or(0);

        let fee_amount = if fee_bps > 0 {
            (refund.amount as u128 * fee_bps as u128 / 10_000) as i128
        } else {
            0
        };

        let customer_amount = refund.amount - fee_amount;

        let client = token::Client::new(&env, &refund.token);
        if customer_amount > 0 {
            client.transfer(
                &env.current_contract_address(),
                &refund.customer,
                &customer_amount,
            );
        }

        // Transfer fee to fee recipient if configured
        if fee_amount > 0 {
            if let Some(fee_recipient) = env
                .storage()
                .instance()
                .get::<DataKey, Address>(&DataKey::FeeRecipient)
            {
                client.transfer(
                    &env.current_contract_address(),
                    &fee_recipient,
                    &fee_amount,
                );
                events::emit_refund_fee_collected(&env, refund_id, fee_amount);
            }
        }

        refund.status = RefundStatus::Processed;
        refund.processed_at = Some(now);
        refund.auto_approved_source = Some(String::from_str(&env, "dispute_window"));
        refund.fee_amount = if fee_amount > 0 { Some(fee_amount) } else { None };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::update_stats_on_process(&env, &refund.merchant, refund.amount);

        events::emit_refund_auto_approved(&env, refund_id, refund.customer, refund.amount);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Add a merchant to the auto-approval whitelist. Admin only.
    pub fn add_to_auto_approve(env: Env, admin: Address, merchant: Address) {
        Self::require_admin(&env, &admin);

        let mut whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AutoApprovedMerchants)
            .unwrap_or(Vec::new(&env));

        // Check if already whitelisted
        for addr in whitelist.iter() {
            if addr == merchant {
                panic!("Merchant already whitelisted");
            }
        }

        whitelist.push_back(merchant);
        env.storage()
            .persistent()
            .set(&DataKey::AutoApprovedMerchants, &whitelist);
        env.storage().persistent().extend_ttl(
            &DataKey::AutoApprovedMerchants,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Remove a merchant from the auto-approval whitelist. Admin only.
    pub fn remove_from_auto_approve(env: Env, admin: Address, merchant: Address) {
        Self::require_admin(&env, &admin);

        let whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AutoApprovedMerchants)
            .unwrap_or(Vec::new(&env));

        let mut found = false;
        let mut new_whitelist = Vec::new(&env);
        for addr in whitelist.iter() {
            if addr != merchant {
                new_whitelist.push_back(addr);
            } else {
                found = true;
            }
        }

        if !found {
            panic!("Merchant not in whitelist");
        }

        env.storage()
            .persistent()
            .set(&DataKey::AutoApprovedMerchants, &new_whitelist);
        env.storage().persistent().extend_ttl(
            &DataKey::AutoApprovedMerchants,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get the auto-approval whitelist.
    pub fn get_auto_approved_merchants(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AutoApprovedMerchants)
            .unwrap_or(Vec::new(&env))
    }

    /// Register a refund from the escrow contract. Only callable by the configured escrow contract.
    /// Creates a refund record in Processed status (no approval needed).
    pub fn register_escrow_refund(
        env: Env,
        escrow_id: u32,
        buyer: Address,
        amount: i128,
        token: Address,
    ) -> u32 {
        Self::require_not_paused(&env);

        // Verify caller is the configured escrow contract
        let escrow_contract_addr: Option<Address> = env
            .storage()
            .instance()
            .get(&DataKey::EscrowContractAddress);

        if let Some(escrow_addr) = escrow_contract_addr {
            if env.current_contract_address() != escrow_addr {
                panic!("Only escrow contract can register escrow refunds");
            }
        } else {
            panic!("Escrow contract not configured");
        }

        if amount <= 0 {
            panic!("Refund amount must be positive");
        }

        let refund_id = Self::next_refund_id(&env);
        let now = env.ledger().timestamp();

        // Use buyer as merchant placeholder for escrow refunds
        let merchant = buyer.clone();

        let refund = Refund {
            id: refund_id,
            payment_id: 0, // No payment_id for escrow refunds
            customer: buyer.clone(),
            merchant: merchant.clone(),
            amount,
            token: token.clone(),
            status: RefundStatus::Processed,
            reason: String::from_str(&env, "escrow_refund"),
            requested_at: now,
            approved_at: Some(now),
            processed_at: Some(now),
            auto_approved_source: None,
            escrow_id: Some(escrow_id),
            fee_amount: None,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::append_index(&env, &DataKey::CustomerRefunds(buyer.clone()), refund_id);

        events::emit_escrow_refund_registered(&env, refund_id, escrow_id, buyer, amount);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        refund_id
    }

    /// Get global refund statistics.
    pub fn get_global_refund_stats(env: &Env) -> RefundStats {
        env.storage()
            .instance()
            .get(&DataKey::GlobalRefundStats)
            .unwrap_or(RefundStats {
                total_requested: 0,
                total_approved: 0,
                total_rejected: 0,
                total_processed: 0,
                total_amount_refunded: 0,
            })
    }

    /// Get per-merchant refund statistics.
    pub fn get_merchant_refund_stats(env: &Env, merchant: Address) -> RefundStats {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantRefundStats(merchant))
            .unwrap_or(RefundStats {
                total_requested: 0,
                total_approved: 0,
                total_rejected: 0,
                total_processed: 0,
                total_amount_refunded: 0,
            })
    }

    /// Update the refund fee in basis points. Admin only. Max 200 bps (2%).
    pub fn update_refund_fee(env: Env, admin: Address, new_fee_bps: u32) {
        Self::require_admin(&env, &admin);

        if new_fee_bps > 200 {
            panic!("Refund fee cannot exceed 200 basis points (2%)");
        }

        env.storage()
            .instance()
            .set(&DataKey::RefundFeeBps, &new_fee_bps);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get the current refund fee in basis points.
    pub fn get_refund_fee(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::RefundFeeBps)
            .unwrap_or(0)
    }

    /// Get the fee recipient address.
    pub fn get_fee_recipient(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::FeeRecipient)
    }

    /// Get the configured dispute window in seconds.
    pub fn get_dispute_window(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::DisputeWindow)
            .expect("Dispute window not configured")
    }

    /// Get refund details
    pub fn get_refund(env: Env, refund_id: u32) -> Refund {
        env.storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .expect("Refund not found")
    }

    /// Get refunds by customer with pagination.
    pub fn get_refunds_by_customer(
        env: Env,
        customer: Address,
        limit: u32,
        offset: u32,
    ) -> Vec<u32> {
        Self::paginate(&env, &DataKey::CustomerRefunds(customer), limit, offset)
    }

    /// Get refunds by merchant with pagination.
    pub fn get_refunds_by_merchant(
        env: Env,
        merchant: Address,
        limit: u32,
        offset: u32,
    ) -> Vec<u32> {
        Self::paginate(&env, &DataKey::MerchantRefunds(merchant), limit, offset)
    }

    /// Get all refund IDs for a given payment ID.
    pub fn get_refunds_by_payment(env: Env, payment_id: u32) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::PaymentRefunds(payment_id))
            .unwrap_or(Vec::new(&env))
    }

    /// Get refund counter
    pub fn get_refund_counter(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::RefundCounter)
            .unwrap_or(0)
    }

    /// Get admin address
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized")
    }

    /// Get the configured payment contract address (#64).
    pub fn get_payment_contract(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::PaymentContractAddress)
            .expect("Payment contract not configured")
    }

    /// Upgrade this contract's WASM code. Admin only.
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        if admin != stored_admin {
            panic!("Only admin can upgrade contract");
        }

        let old_version = Self::get_or_init_version(&env);
        env.deployer().update_current_contract_wasm(new_wasm_hash);

        let new_version = old_version.checked_add(1).expect("Version overflow");
        env.storage()
            .instance()
            .set(&DataKey::ContractVersion, &new_version);

        events::emit_contract_upgraded(&env, old_version, new_version, admin);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Run one-time migration logic for the current version. Admin only.
    pub fn migrate(env: Env, admin: Address) {
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        if admin != stored_admin {
            panic!("Only admin can migrate contract");
        }

        let version = Self::get_or_init_version(&env);
        if env
            .storage()
            .instance()
            .get(&DataKey::MigrationCompleted(version))
            .unwrap_or(false)
        {
            panic!("Migration already completed for this version");
        }

        env.storage()
            .instance()
            .set(&DataKey::MigrationCompleted(version), &true);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Returns the current contract version.
    pub fn get_version(env: Env) -> u32 {
        Self::get_or_init_version(&env)
    }

    pub fn pause_contract(env: Env, admin: Address, reason: String) {
        Self::require_admin(&env, &admin);

        if Self::is_paused(env.clone()) {
            panic!("Contract already paused");
        }

        env.storage().instance().set(&DataKey::Paused, &true);
        env.storage().instance().set(&DataKey::PauseReason, &reason);

        events::emit_contract_paused(&env, admin, reason, env.ledger().timestamp());
    }

    pub fn resume_contract(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);

        if !Self::is_paused(env.clone()) {
            panic!("Contract is not paused");
        }

        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().remove(&DataKey::PauseReason);

        events::emit_contract_resumed(&env, admin, env.ledger().timestamp());
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    pub fn get_pause_reason(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::PauseReason)
            .unwrap_or(String::from_str(&env, ""))
    }

    // --- Internal Helpers ---

    fn require_not_paused(env: &Env) {
        if env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
        {
            panic!("Contract is paused");
        }
    }

    fn require_admin(env: &Env, admin: &Address) {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        if stored_admin != *admin {
            panic!("Only admin can manage pause state");
        }
    }

    fn append_index(env: &Env, key: &DataKey, refund_id: u32) {
        let mut ids: Vec<u32> = env.storage().persistent().get(key).unwrap_or(Vec::new(env));
        ids.push_back(refund_id);
        env.storage().persistent().set(key, &ids);
        env.storage().persistent().extend_ttl(
            key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }

    fn paginate(env: &Env, key: &DataKey, limit: u32, offset: u32) -> Vec<u32> {
        let all: Vec<u32> = env.storage().persistent().get(key).unwrap_or(Vec::new(env));
        let total = all.len();
        let start = offset.min(total);
        let end = (start + limit).min(total);
        let mut page = Vec::new(env);
        for i in start..end {
            page.push_back(all.get(i).unwrap());
        }
        page
    }

    fn next_refund_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RefundCounter)
            .unwrap_or(0);
        let id = counter;
        counter += 1;
        env.storage()
            .instance()
            .set(&DataKey::RefundCounter, &counter);
        id
    }

    fn get_or_init_version(env: &Env) -> u32 {
        if let Some(version) = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::ContractVersion)
        {
            version
        } else {
            let initial_version = 1u32;
            env.storage()
                .instance()
                .set(&DataKey::ContractVersion, &initial_version);
            initial_version
        }
    }

    // --- Helper Functions for Stats and Whitelist ---

    fn is_merchant_auto_approved(env: &Env, merchant: &Address) -> bool {
        let whitelist: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AutoApprovedMerchants)
            .unwrap_or(Vec::new(env));

        for addr in whitelist.iter() {
            if addr == *merchant {
                return true;
            }
        }
        false
    }

    fn update_stats_on_request(env: &Env, merchant: &Address, _amount: i128) {
        // Update global stats
        let mut global_stats = Self::get_global_refund_stats(env);
        global_stats.total_requested += 1;
        env.storage()
            .instance()
            .set(&DataKey::GlobalRefundStats, &global_stats);

        // Update merchant stats
        let mut merchant_stats = Self::get_merchant_refund_stats(env, merchant.clone());
        merchant_stats.total_requested += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MerchantRefundStats(merchant.clone()), &merchant_stats);
        env.storage().persistent().extend_ttl(
            &DataKey::MerchantRefundStats(merchant.clone()),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }

    fn update_stats_on_approve(env: &Env, merchant: &Address) {
        // Update global stats
        let mut global_stats = Self::get_global_refund_stats(env);
        global_stats.total_approved += 1;
        env.storage()
            .instance()
            .set(&DataKey::GlobalRefundStats, &global_stats);

        // Update merchant stats
        let mut merchant_stats = Self::get_merchant_refund_stats(env, merchant.clone());
        merchant_stats.total_approved += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MerchantRefundStats(merchant.clone()), &merchant_stats);
        env.storage().persistent().extend_ttl(
            &DataKey::MerchantRefundStats(merchant.clone()),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }

    fn update_stats_on_reject(env: &Env, merchant: &Address) {
        // Update global stats
        let mut global_stats = Self::get_global_refund_stats(env);
        global_stats.total_rejected += 1;
        env.storage()
            .instance()
            .set(&DataKey::GlobalRefundStats, &global_stats);

        // Update merchant stats
        let mut merchant_stats = Self::get_merchant_refund_stats(env, merchant.clone());
        merchant_stats.total_rejected += 1;
        env.storage()
            .persistent()
            .set(&DataKey::MerchantRefundStats(merchant.clone()), &merchant_stats);
        env.storage().persistent().extend_ttl(
            &DataKey::MerchantRefundStats(merchant.clone()),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }

    fn update_stats_on_process(env: &Env, merchant: &Address, amount: i128) {
        // Update global stats
        let mut global_stats = Self::get_global_refund_stats(env);
        global_stats.total_processed += 1;
        global_stats.total_amount_refunded += amount;
        env.storage()
            .instance()
            .set(&DataKey::GlobalRefundStats, &global_stats);

        // Update merchant stats
        let mut merchant_stats = Self::get_merchant_refund_stats(env, merchant.clone());
        merchant_stats.total_processed += 1;
        merchant_stats.total_amount_refunded += amount;
        env.storage()
            .persistent()
            .set(&DataKey::MerchantRefundStats(merchant.clone()), &merchant_stats);
        env.storage().persistent().extend_ttl(
            &DataKey::MerchantRefundStats(merchant.clone()),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
}

#[cfg(test)]
mod test;
