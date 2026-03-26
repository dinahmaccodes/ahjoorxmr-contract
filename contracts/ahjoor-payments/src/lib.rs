#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, String, Vec};

// ---------------------------------------------------------------------------
// Reflector-compatible oracle interface.
// lastprice(base, quote) returns Option<PriceData> where price is scaled by
// 10^decimals(). We call it via a generated client.
// ---------------------------------------------------------------------------
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PriceData {
    /// Price scaled by 10^7 (Reflector standard precision)
    pub price: i128,
    /// Ledger timestamp of the price update
    pub timestamp: u64,
}

/// Minimal oracle client — only the method we need.
mod oracle {
    use crate::PriceData;
    use soroban_sdk::{contractclient, Address, Env};

    #[contractclient(name = "OracleClient")]
    pub trait OracleInterface {
        fn lastprice(env: Env, base: Address, quote: Address) -> Option<PriceData>;
    }
}

// --- Storage TTL Constants ---
// Instance storage: counters and config (shared TTL with contract instance)
const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;

// Persistent storage: per-record data (Payment, Dispute, CustomerPayments)
// Individual TTL — survives beyond instance TTL, extended on each access.
const PERSISTENT_LIFETIME_THRESHOLD: u32 = 100_000;
const PERSISTENT_BUMP_AMOUNT: u32 = 120_000;

// Temporary storage: in-progress dispute state
// Short-lived; expires automatically if not extended.
const TEMP_LIFETIME_THRESHOLD: u32 = 10_000;
const TEMP_BUMP_AMOUNT: u32 = 15_000;

const DEFAULT_MAX_BATCH_SIZE: u32 = 20;
const DEFAULT_DISPUTE_TIMEOUT: u64 = 7 * 24 * 60 * 60; // 7 days in seconds
/// Reflector oracle price precision: prices are scaled by 10^7
const ORACLE_PRICE_PRECISION: i128 = 10_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
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
    /// Ledger timestamp after which the payment can be expired. 0 = no expiry.
    pub expires_at: u64,
    /// Cumulative amount refunded via partial refunds.
    pub refunded_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRequest {
    pub merchant: Address,
    pub amount: i128,
    pub token: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub payment_id: u32,
    pub reason: String,
    pub created_at: u64,
    pub resolved: bool,
}

/// Default payment timeout: 7 days in seconds.
const DEFAULT_PAYMENT_TIMEOUT: u64 = 7 * 24 * 60 * 60;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subscription {
    pub id: u32,
    pub subscriber: Address,
    pub merchant: Address,
    pub amount: i128,
    pub token: Address,
    pub interval_seconds: u64,
    pub last_charged_at: u64,
    pub max_charges: u32,
    pub charges_count: u32,
    pub active: bool,
}

/// Storage key classification:
/// - Instance:    Admin, PaymentCounter, MaxBatchSize, DisputeTimeout,
///                OracleAddress, UsdcToken
///                (config/counters — bounded, shared TTL with contract)
/// - Persistent:  Payment(u32), CustomerPayments(Address)
///                (per-record data — unbounded, individual TTL)
/// - Temporary:   Dispute(u32)
///                (in-progress dispute state — short-lived, auto-expires)
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    // --- Instance ---
    Admin,
    PaymentCounter,
    MaxBatchSize,
    DisputeTimeout,
    /// Reflector oracle contract address for token/USDC price feeds
    OracleAddress,
    /// USDC token contract address — canonical settlement currency
    UsdcToken,
    /// Maximum age (seconds) an oracle price may be before rejection
    MaxOracleAge,
    /// Proposed new admin address (pending acceptance)
    ProposedAdmin,
    /// Global emergency stop flag
    Paused,
    /// Human-readable pause reason
    PauseReason,
    /// Global payment timeout in seconds (default: 7 days)
    PaymentTimeout,
    /// When true, merchant allowlist is bypassed (open mode)
    MerchantOpenMode,
    /// Subscription counter
    SubscriptionCounter,
    // --- Persistent ---
    Payment(u32),
    CustomerPayments(Address),
    /// Merchant approval status (true = approved)
    MerchantApproved(Address),
    /// Subscription record
    Subscription(u32),
    // --- Temporary ---
    Dispute(u32),
}

mod events;

#[contract]
pub struct AhjoorPaymentsContract;

#[contractimpl]
impl AhjoorPaymentsContract {
    /// One-time contract initialization.
    /// Admin, counters, and config go to instance storage.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        // Instance: config and counters
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::PaymentCounter, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::MaxBatchSize, &DEFAULT_MAX_BATCH_SIZE);
        env.storage()
            .instance()
            .set(&DataKey::DisputeTimeout, &DEFAULT_DISPUTE_TIMEOUT);
        env.storage().instance().set(&DataKey::Paused, &false);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Create a single payment: transfer tokens from customer to contract (escrow).
    /// Payment record stored in persistent storage with individual TTL.
    /// Rejects unapproved merchants unless open mode is enabled (#58).
    /// Sets expiry based on global payment timeout (#54).
    /// Returns the new payment ID.
    pub fn create_payment(
        env: Env,
        customer: Address,
        merchant: Address,
        amount: i128,
        token: Address,
    ) -> u32 {
        Self::require_not_paused(&env);
        customer.require_auth();

        if amount <= 0 {
            panic!("Payment amount must be positive");
        }

        // Merchant allowlist check (#58)
        Self::require_merchant_approved(&env, &merchant);

        let client = token::Client::new(&env, &token);
        client.transfer(&customer, &env.current_contract_address(), &amount);

        let timeout: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PaymentTimeout)
            .unwrap_or(DEFAULT_PAYMENT_TIMEOUT);
        let now = env.ledger().timestamp();

        let payment_id = Self::next_payment_id(&env);
        let payment = Payment {
            id: payment_id,
            customer: customer.clone(),
            merchant: merchant.clone(),
            amount,
            token: token.clone(),
            status: PaymentStatus::Pending,
            created_at: now,
            expires_at: now + timeout,
            refunded_amount: 0,
        };

        // Persistent: per-payment record with individual TTL
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::add_customer_payment(&env, &customer, payment_id);

        events::emit_payment_created(&env, payment_id, customer, merchant, amount, token);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        payment_id
    }

    /// Create multiple payments atomically. All payment records go to persistent storage.
    /// Returns a Vec of payment IDs.
    pub fn create_payments_batch(
        env: Env,
        customer: Address,
        payments: Vec<PaymentRequest>,
    ) -> Vec<u32> {
        Self::require_not_paused(&env);
        customer.require_auth();

        let batch_len = payments.len();
        if batch_len == 0 {
            panic!("Batch cannot be empty");
        }

        let max_batch_size: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MaxBatchSize)
            .unwrap_or(DEFAULT_MAX_BATCH_SIZE);

        if batch_len > max_batch_size {
            panic!("Batch size exceeds maximum allowed");
        }

        let mut payment_ids = Vec::new(&env);
        let mut total_amount: i128 = 0;

        let timeout: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PaymentTimeout)
            .unwrap_or(DEFAULT_PAYMENT_TIMEOUT);
        let now = env.ledger().timestamp();

        for request in payments.iter() {
            if request.amount <= 0 {
                panic!("Payment amount must be positive");
            }

            Self::require_merchant_approved(&env, &request.merchant);

            let client = token::Client::new(&env, &request.token);
            client.transfer(&customer, &env.current_contract_address(), &request.amount);

            let payment_id = Self::next_payment_id(&env);
            let payment = Payment {
                id: payment_id,
                customer: customer.clone(),
                merchant: request.merchant.clone(),
                amount: request.amount,
                token: request.token.clone(),
                status: PaymentStatus::Pending,
                created_at: now,
                expires_at: now + timeout,
                refunded_amount: 0,
            };

            // Persistent: per-payment record with individual TTL
            env.storage()
                .persistent()
                .set(&DataKey::Payment(payment_id), &payment);
            env.storage().persistent().extend_ttl(
                &DataKey::Payment(payment_id),
                PERSISTENT_LIFETIME_THRESHOLD,
                PERSISTENT_BUMP_AMOUNT,
            );

            Self::add_customer_payment(&env, &customer, payment_id);

            events::emit_payment_created(
                &env,
                payment_id,
                customer.clone(),
                request.merchant.clone(),
                request.amount,
                request.token.clone(),
            );

            payment_ids.push_back(payment_id);
            total_amount += request.amount;
        }

        events::emit_batch_payment_created(&env, customer, batch_len, total_amount);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        payment_ids
    }

    /// Admin releases escrowed funds to the merchant. Payment must be Pending.
    pub fn complete_payment(env: Env, payment_id: u32) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        let mut payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Pending {
            panic!("Payment is not pending");
        }

        // Reject if payment has expired (#54)
        if payment.expires_at > 0 && env.ledger().timestamp() >= payment.expires_at {
            panic!("Payment has expired");
        }

        let client = token::Client::new(&env, &payment.token);
        client.transfer(
            &env.current_contract_address(),
            &payment.merchant,
            &payment.amount,
        );

        let old_status = payment.status;
        payment.status = PaymentStatus::Completed;

        // Extend TTL on update so completed records survive long-term
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_payment_completed(&env, payment_id, payment.merchant, payment.amount);
        events::emit_payment_status_changed(&env, payment_id, old_status, PaymentStatus::Completed);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    // --- Dispute Methods ---

    /// Customer disputes a Pending payment. Dispute state stored in temporary storage
    /// (short-lived, in-progress — auto-expires once resolved or timed out).
    pub fn dispute_payment(env: Env, customer: Address, payment_id: u32, reason: String) {
        Self::require_not_paused(&env);
        customer.require_auth();

        let mut payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.customer != customer {
            panic!("Only the payment customer can dispute");
        }

        if payment.status != PaymentStatus::Pending {
            panic!("Only pending payments can be disputed");
        }

        let old_status = payment.status;
        payment.status = PaymentStatus::Disputed;

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        // Temporary: active dispute state — short-lived, expires if not resolved
        let dispute = Dispute {
            payment_id,
            reason: reason.clone(),
            created_at: env.ledger().timestamp(),
            resolved: false,
        };
        env.storage()
            .temporary()
            .set(&DataKey::Dispute(payment_id), &dispute);
        env.storage().temporary().extend_ttl(
            &DataKey::Dispute(payment_id),
            TEMP_LIFETIME_THRESHOLD,
            TEMP_BUMP_AMOUNT,
        );

        events::emit_payment_disputed(&env, payment_id, customer, reason);
        events::emit_payment_status_changed(&env, payment_id, old_status, PaymentStatus::Disputed);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Admin resolves a dispute. Clears temporary dispute state on resolution.
    pub fn resolve_dispute(env: Env, payment_id: u32, release_to_merchant: bool) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        let mut payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Disputed {
            panic!("Payment is not disputed");
        }

        let client = token::Client::new(&env, &payment.token);
        let old_status = payment.status;

        if release_to_merchant {
            client.transfer(
                &env.current_contract_address(),
                &payment.merchant,
                &payment.amount,
            );
            payment.status = PaymentStatus::Completed;
        } else {
            client.transfer(
                &env.current_contract_address(),
                &payment.customer,
                &payment.amount,
            );
            payment.status = PaymentStatus::Refunded;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        // Mark dispute resolved in temporary storage, then let it expire naturally
        if let Some(mut dispute) = env
            .storage()
            .temporary()
            .get::<DataKey, Dispute>(&DataKey::Dispute(payment_id))
        {
            dispute.resolved = true;
            env.storage()
                .temporary()
                .set(&DataKey::Dispute(payment_id), &dispute);
            // No TTL extension — resolved disputes can expire on their own
        }

        events::emit_dispute_resolved(&env, payment_id, release_to_merchant, admin);
        events::emit_payment_status_changed(&env, payment_id, old_status, payment.status);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Check if a dispute has exceeded the timeout window.
    pub fn check_escalation(env: Env, payment_id: u32) -> bool {
        let payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Disputed {
            return false;
        }

        let dispute: Dispute = env
            .storage()
            .temporary()
            .get(&DataKey::Dispute(payment_id))
            .expect("Dispute not found");

        if dispute.resolved {
            return false;
        }

        let timeout: u64 = env
            .storage()
            .instance()
            .get(&DataKey::DisputeTimeout)
            .unwrap_or(DEFAULT_DISPUTE_TIMEOUT);

        let elapsed = env.ledger().timestamp() - dispute.created_at;
        if elapsed > timeout {
            events::emit_dispute_escalated(&env, payment_id, elapsed);
            return true;
        }

        false
    }

    // --- Oracle / Multi-Token ---

    /// Admin sets the oracle contract address, USDC token address, and max
    /// oracle price age. Must be called before create_payment_multi_token.
    pub fn set_oracle(env: Env, oracle: Address, usdc_token: Address, max_oracle_age: u64) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        if max_oracle_age == 0 {
            panic!("max_oracle_age must be positive");
        }

        env.storage()
            .instance()
            .set(&DataKey::OracleAddress, &oracle);
        env.storage()
            .instance()
            .set(&DataKey::UsdcToken, &usdc_token);
        env.storage()
            .instance()
            .set(&DataKey::MaxOracleAge, &max_oracle_age);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Create a payment where the customer pays in any supported token.
    /// The oracle provides the token/USDC rate. The contract:
    ///   1. Queries the oracle for the current price of `payment_token` in USDC.
    ///   2. Validates price freshness against `max_oracle_age`.
    ///   3. Calculates `required_token_amount` from `amount_usdc` and the rate.
    ///   4. Applies slippage tolerance: rejects if effective rate deviates
    ///      more than `slippage_bps` basis points from the oracle rate.
    ///   5. Transfers `required_token_amount` of `payment_token` from customer
    ///      to contract (escrow).
    ///   6. Records the payment with `amount = amount_usdc` and `token = usdc_token`
    ///      so that complete_payment always releases USDC to the merchant.
    ///
    /// Fallback: if `payment_token == usdc_token`, behaves identically to
    /// create_payment (no oracle call, no conversion).
    ///
    /// Returns the new payment ID.
    pub fn create_payment_multi_token(
        env: Env,
        customer: Address,
        merchant: Address,
        amount_usdc: i128,
        payment_token: Address,
        slippage_bps: u32,
    ) -> u32 {
        Self::require_not_paused(&env);
        if amount_usdc <= 0 {
            panic!("Payment amount must be positive");
        }
        if slippage_bps > 10_000 {
            panic!("slippage_bps cannot exceed 10000");
        }

        let usdc_token: Address = env
            .storage()
            .instance()
            .get(&DataKey::UsdcToken)
            .expect("Oracle not configured");

        // --- Fallback: direct USDC payment, no oracle needed ---
        if payment_token == usdc_token {
            return Self::create_payment(env, customer, merchant, amount_usdc, payment_token);
        }

        customer.require_auth();

        let oracle_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::OracleAddress)
            .expect("Oracle not configured");
        let max_oracle_age: u64 = env
            .storage()
            .instance()
            .get(&DataKey::MaxOracleAge)
            .expect("Oracle not configured");

        // --- Query oracle: price of payment_token denominated in USDC ---
        // Oracle returns price scaled by ORACLE_PRICE_PRECISION (10^7).
        let oracle_client = oracle::OracleClient::new(&env, &oracle_addr);
        let price_data: PriceData = oracle_client
            .lastprice(&payment_token, &usdc_token)
            .expect("Oracle price unavailable");

        // --- Freshness check ---
        let current_ts = env.ledger().timestamp();
        let age = current_ts.saturating_sub(price_data.timestamp);
        if age > max_oracle_age {
            panic!("Oracle price is stale");
        }

        if price_data.price <= 0 {
            panic!("Invalid oracle price");
        }

        // --- Calculate required payment_token amount ---
        // price = payment_token per USDC, scaled by 10^7
        // required = amount_usdc * 10^7 / price
        let required_token_amount = (amount_usdc * ORACLE_PRICE_PRECISION) / price_data.price;
        if required_token_amount <= 0 {
            panic!("Computed token amount is zero");
        }

        // --- Slippage check ---
        // Effective USDC value of required_token_amount at oracle rate must be
        // within slippage_bps of amount_usdc.
        // effective_usdc = required_token_amount * price / 10^7
        // deviation_bps = abs(effective_usdc - amount_usdc) * 10000 / amount_usdc
        let effective_usdc = (required_token_amount * price_data.price) / ORACLE_PRICE_PRECISION;
        let deviation = if effective_usdc >= amount_usdc {
            effective_usdc - amount_usdc
        } else {
            amount_usdc - effective_usdc
        };
        let deviation_bps = (deviation * 10_000) / amount_usdc;
        if deviation_bps > slippage_bps as i128 {
            panic!("Slippage tolerance exceeded");
        }

        // --- Transfer payment_token from customer to contract (escrow) ---
        let pay_client = token::Client::new(&env, &payment_token);
        pay_client.transfer(
            &customer,
            &env.current_contract_address(),
            &required_token_amount,
        );

        // --- Record payment in USDC terms so complete_payment releases USDC ---
        let timeout: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PaymentTimeout)
            .unwrap_or(DEFAULT_PAYMENT_TIMEOUT);
        let now = env.ledger().timestamp();
        let payment_id = Self::next_payment_id(&env);
        let payment = Payment {
            id: payment_id,
            customer: customer.clone(),
            merchant: merchant.clone(),
            amount: amount_usdc,
            token: usdc_token.clone(),
            status: PaymentStatus::Pending,
            created_at: now,
            expires_at: now + timeout,
            refunded_amount: 0,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        Self::add_customer_payment(&env, &customer, payment_id);

        events::emit_multi_token_payment_created(
            &env,
            payment_id,
            customer,
            merchant,
            amount_usdc,
            payment_token,
            required_token_amount,
            price_data.price,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        payment_id
    }

    pub fn get_oracle_address(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::OracleAddress)
            .expect("Oracle not configured")
    }

    pub fn get_usdc_token(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::UsdcToken)
            .expect("Oracle not configured")
    }

    pub fn get_max_oracle_age(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::MaxOracleAge)
            .expect("Oracle not configured")
    }

    // --- Admin ---

    pub fn set_max_batch_size(env: Env, new_size: u32) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        if new_size == 0 {
            panic!("Max batch size must be at least 1");
        }

        env.storage()
            .instance()
            .set(&DataKey::MaxBatchSize, &new_size);
    }

    pub fn set_dispute_timeout(env: Env, timeout: u64) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        if timeout == 0 {
            panic!("Dispute timeout must be positive");
        }

        env.storage()
            .instance()
            .set(&DataKey::DisputeTimeout, &timeout);
    }

    /// Propose a new admin address. Only the current admin can propose.
    pub fn propose_admin_transfer(env: Env, proposed_admin: Address) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::ProposedAdmin, &proposed_admin);

        events::emit_admin_transfer_proposed(&env, admin, proposed_admin);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Accept the admin role. Only the proposed admin can accept.
    pub fn accept_admin_role(env: Env) {
        Self::require_not_paused(&env);
        let proposed_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::ProposedAdmin)
            .expect("No admin transfer proposed");
        proposed_admin.require_auth();

        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");

        env.storage()
            .instance()
            .set(&DataKey::Admin, &proposed_admin);
        env.storage().instance().remove(&DataKey::ProposedAdmin);

        events::emit_admin_transferred(&env, old_admin, proposed_admin);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized")
    }

    /// Get the proposed admin address, if any.
    pub fn get_proposed_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::ProposedAdmin)
    }

    // --- Read Interface ---

    pub fn get_payment(env: Env, payment_id: u32) -> Payment {
        env.storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found")
    }

    pub fn get_customer_payments(env: Env, customer: Address) -> Vec<u32> {
        env.storage()
            .persistent()
            .get(&DataKey::CustomerPayments(customer))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_payment_counter(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::PaymentCounter)
            .unwrap_or(0)
    }

    pub fn get_max_batch_size(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MaxBatchSize)
            .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
    }

    pub fn is_disputed(env: Env, payment_id: u32) -> bool {
        let payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");
        payment.status == PaymentStatus::Disputed
    }

    pub fn get_dispute(env: Env, payment_id: u32) -> Dispute {
        env.storage()
            .temporary()
            .get(&DataKey::Dispute(payment_id))
            .expect("No dispute found for this payment")
    }

    pub fn get_dispute_timeout(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::DisputeTimeout)
            .unwrap_or(DEFAULT_DISPUTE_TIMEOUT)
    }

    // --- Payment Expiry (#54) ---

    /// Admin sets the global payment timeout in seconds.
    pub fn set_payment_timeout(env: Env, timeout_seconds: u64) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        if timeout_seconds == 0 {
            panic!("Timeout must be positive");
        }
        env.storage()
            .instance()
            .set(&DataKey::PaymentTimeout, &timeout_seconds);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn get_payment_timeout(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::PaymentTimeout)
            .unwrap_or(DEFAULT_PAYMENT_TIMEOUT)
    }

    /// Expire a pending payment after its deadline. Callable by anyone.
    /// Returns funds to the customer and emits PaymentExpired event.
    pub fn expire_payment(env: Env, payment_id: u32) {
        Self::require_not_paused(&env);
        let mut payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Pending {
            panic!("Only pending payments can expire");
        }
        if payment.expires_at == 0 {
            panic!("Payment has no expiry set");
        }
        if env.ledger().timestamp() < payment.expires_at {
            panic!("Payment has not expired yet");
        }

        let client = token::Client::new(&env, &payment.token);
        client.transfer(
            &env.current_contract_address(),
            &payment.customer,
            &payment.amount,
        );

        let old_status = payment.status;
        payment.status = PaymentStatus::Expired;
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_payment_status_changed(&env, payment_id, old_status, PaymentStatus::Expired);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    // --- Partial Refund (#55) ---

    /// Process a partial refund on a disputed payment. Admin only.
    /// `refund_amount` must be <= (payment.amount - payment.refunded_amount).
    pub fn partial_refund(env: Env, payment_id: u32, refund_amount: i128) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        if refund_amount <= 0 {
            panic!("Refund amount must be positive");
        }

        let mut payment: Payment = env
            .storage()
            .persistent()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Disputed && payment.status != PaymentStatus::Pending {
            panic!("Payment must be pending or disputed for partial refund");
        }

        let remaining = payment.amount - payment.refunded_amount;
        if refund_amount > remaining {
            panic!("Refund amount exceeds remaining balance");
        }

        let client = token::Client::new(&env, &payment.token);
        client.transfer(
            &env.current_contract_address(),
            &payment.customer,
            &refund_amount,
        );

        payment.refunded_amount += refund_amount;

        // If fully refunded, mark as Refunded
        if payment.refunded_amount >= payment.amount {
            payment.status = PaymentStatus::Refunded;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment_id), &payment);
        env.storage().persistent().extend_ttl(
            &DataKey::Payment(payment_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    // --- Merchant Allowlist (#58) ---

    /// Admin approves a merchant address.
    pub fn approve_merchant(env: Env, merchant: Address) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::MerchantApproved(merchant), &true);
    }

    /// Admin revokes a merchant address.
    pub fn revoke_merchant(env: Env, merchant: Address) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::MerchantApproved(merchant), &false);
    }

    /// Check if a merchant is approved.
    pub fn is_merchant_approved(env: Env, merchant: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::MerchantApproved(merchant))
            .unwrap_or(false)
    }

    /// Admin toggles open mode (bypasses merchant allowlist).
    pub fn set_merchant_open_mode(env: Env, open: bool) {
        Self::require_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::MerchantOpenMode, &open);
    }

    pub fn is_merchant_open_mode(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::MerchantOpenMode)
            .unwrap_or(true)
    }

    // --- Subscriptions (#60) ---

    /// Subscriber creates a recurring payment. Signs once to authorize future charges.
    pub fn create_subscription(
        env: Env,
        subscriber: Address,
        merchant: Address,
        amount: i128,
        token: Address,
        interval_seconds: u64,
        max_charges: u32,
    ) -> u32 {
        Self::require_not_paused(&env);
        subscriber.require_auth();
        if amount <= 0 {
            panic!("Subscription amount must be positive");
        }
        if interval_seconds == 0 {
            panic!("Interval must be positive");
        }

        Self::require_merchant_approved(&env, &merchant);

        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::SubscriptionCounter)
            .unwrap_or(0);
        let sub_id = counter;
        counter += 1;
        env.storage()
            .instance()
            .set(&DataKey::SubscriptionCounter, &counter);

        let sub = Subscription {
            id: sub_id,
            subscriber,
            merchant,
            amount,
            token,
            interval_seconds,
            last_charged_at: 0,
            max_charges,
            charges_count: 0,
            active: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Subscription(sub_id), &sub);
        env.storage().persistent().extend_ttl(
            &DataKey::Subscription(sub_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        sub_id
    }

    /// Charge a subscription. Callable by anyone when the interval has elapsed.
    pub fn charge_subscription(env: Env, subscription_id: u32) {
        Self::require_not_paused(&env);
        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&DataKey::Subscription(subscription_id))
            .expect("Subscription not found");

        if !sub.active {
            panic!("Subscription is cancelled");
        }
        if sub.max_charges > 0 && sub.charges_count >= sub.max_charges {
            panic!("Max charges reached");
        }

        let now = env.ledger().timestamp();
        if sub.last_charged_at > 0 && now < sub.last_charged_at + sub.interval_seconds {
            panic!("Interval has not elapsed");
        }

        let client = token::Client::new(&env, &sub.token);
        client.transfer(
            &sub.subscriber,
            &env.current_contract_address(),
            &sub.amount,
        );
        client.transfer(&env.current_contract_address(), &sub.merchant, &sub.amount);

        sub.last_charged_at = now;
        sub.charges_count += 1;

        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id), &sub);
        env.storage().persistent().extend_ttl(
            &DataKey::Subscription(subscription_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Cancel a subscription. Subscriber or merchant can cancel.
    pub fn cancel_subscription(env: Env, caller: Address, subscription_id: u32) {
        Self::require_not_paused(&env);
        caller.require_auth();

        let mut sub: Subscription = env
            .storage()
            .persistent()
            .get(&DataKey::Subscription(subscription_id))
            .expect("Subscription not found");

        if caller != sub.subscriber && caller != sub.merchant {
            panic!("Only subscriber or merchant can cancel");
        }

        sub.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Subscription(subscription_id), &sub);
        env.storage().persistent().extend_ttl(
            &DataKey::Subscription(subscription_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }

    /// Read a subscription.
    pub fn get_subscription(env: Env, subscription_id: u32) -> Subscription {
        env.storage()
            .persistent()
            .get(&DataKey::Subscription(subscription_id))
            .expect("Subscription not found")
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
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    pub fn get_pause_reason(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::PauseReason)
            .unwrap_or(String::from_str(&env, ""))
    }

    // --- Internal Helpers ---

    fn require_not_paused(env: &Env) {
        if env.storage().instance().get(&DataKey::Paused).unwrap_or(false) {
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

    /// Validates merchant is approved or open mode is enabled.
    fn require_merchant_approved(env: &Env, merchant: &Address) {
        let open_mode: bool = env
            .storage()
            .instance()
            .get(&DataKey::MerchantOpenMode)
            .unwrap_or(true); // Default: open mode (no allowlist enforcement)
        if open_mode {
            return;
        }

        let approved: bool = env
            .storage()
            .persistent()
            .get(&DataKey::MerchantApproved(merchant.clone()))
            .unwrap_or(false);
        if !approved {
            panic!("Merchant not approved");
        }
    }

    fn next_payment_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PaymentCounter)
            .unwrap_or(0);
        let id = counter;
        counter += 1;
        // Counter stays in instance storage — bounded, config-like
        env.storage()
            .instance()
            .set(&DataKey::PaymentCounter, &counter);
        id
    }

    fn add_customer_payment(env: &Env, customer: &Address, payment_id: u32) {
        let key = DataKey::CustomerPayments(customer.clone());
        let mut customer_payments: Vec<u32> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(env));
        customer_payments.push_back(payment_id);
        // Persistent: customer index grows with payment volume
        env.storage().persistent().set(&key, &customer_payments);
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
}

#[cfg(test)]
mod test;

pub use events::*;
