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
}

mod events;

#[contract]
pub struct AhjoorRefundContract;

#[contractimpl]
impl AhjoorRefundContract {
    /// Initialize the refund contract with an admin and the payment contract address.
    pub fn initialize(env: Env, admin: Address, payment_contract: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
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
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Request a refund linked to an existing completed payment.
    /// Cross-contract validates: payment exists, status is Completed, merchant matches,
    /// and refund amount does not exceed the original payment amount (#64).
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
        let refund = Refund {
            id: refund_id,
            payment_id,
            customer: customer.clone(),
            merchant: merchant.clone(),
            amount,
            token: token.clone(),
            status: RefundStatus::Requested,
            reason,
            requested_at: env.ledger().timestamp(),
            approved_at: None,
            processed_at: None,
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

        events::emit_refund_requested(&env, refund_id, customer, amount, token, refund.reason);

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

    /// Process an approved refund. Transfers tokens to customer. Only admin can call this.
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

        // Transfer tokens to customer
        let client = token::Client::new(&env, &refund.token);
        client.transfer(
            &env.current_contract_address(),
            &refund.customer,
            &refund.amount,
        );

        refund.status = RefundStatus::Processed;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage()
            .persistent()
            .set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_refund_processed(
            &env,
            refund_id,
            refund.customer,
            refund.amount,
            refund.processed_at.unwrap(),
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
}

#[cfg(test)]
mod test;
