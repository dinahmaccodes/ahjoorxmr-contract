#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String, Vec,
};

const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;
const DEFAULT_MAX_BATCH_SIZE: u32 = 20;
const DEFAULT_DISPUTE_TIMEOUT: u64 = 7 * 24 * 60 * 60; // 7 days in seconds

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum PaymentStatus {
    Pending = 0,
    Completed = 1,
    Refunded = 2,
    Disputed = 3,
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

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    PaymentCounter,
    Payment(u32),
    CustomerPayments(Address),
    MaxBatchSize,
    Dispute(u32),
    DisputeTimeout,
}

mod events;

#[contract]
pub struct AhjoorPaymentsContract;

#[contractimpl]
impl AhjoorPaymentsContract {
    /// One-time contract initialization.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

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

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Create a single payment: transfer tokens from customer to contract (escrow).
    /// Payment starts as Pending. Call complete_payment to release to merchant.
    /// Returns the new payment ID.
    pub fn create_payment(
        env: Env,
        customer: Address,
        merchant: Address,
        amount: i128,
        token: Address,
    ) -> u32 {
        customer.require_auth();

        if amount <= 0 {
            panic!("Payment amount must be positive");
        }

        // Transfer tokens from customer to contract (escrow)
        let client = token::Client::new(&env, &token);
        client.transfer(&customer, &env.current_contract_address(), &amount);

        // Store the payment record as Pending
        let payment_id = Self::next_payment_id(&env);
        let payment = Payment {
            id: payment_id,
            customer: customer.clone(),
            merchant: merchant.clone(),
            amount,
            token: token.clone(),
            status: PaymentStatus::Pending,
            created_at: env.ledger().timestamp(),
        };

        env.storage()
            .instance()
            .set(&DataKey::Payment(payment_id), &payment);

        // Track payment under customer
        Self::add_customer_payment(&env, &customer, payment_id);

        events::emit_payment_created(&env, payment_id, customer, merchant, amount, token);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        payment_id
    }

    /// Create multiple payments atomically within a single invocation.
    /// All transfers go to escrow (contract). Returns a Vec of payment IDs.
    pub fn create_payments_batch(
        env: Env,
        customer: Address,
        payments: Vec<PaymentRequest>,
    ) -> Vec<u32> {
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

        for request in payments.iter() {
            if request.amount <= 0 {
                panic!("Payment amount must be positive");
            }

            // Transfer tokens from customer to contract (escrow)
            let client = token::Client::new(&env, &request.token);
            client.transfer(&customer, &env.current_contract_address(), &request.amount);

            // Store the payment record as Pending
            let payment_id = Self::next_payment_id(&env);
            let payment = Payment {
                id: payment_id,
                customer: customer.clone(),
                merchant: request.merchant.clone(),
                amount: request.amount,
                token: request.token.clone(),
                status: PaymentStatus::Pending,
                created_at: env.ledger().timestamp(),
            };

            env.storage()
                .instance()
                .set(&DataKey::Payment(payment_id), &payment);

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
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        let mut payment: Payment = env
            .storage()
            .instance()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Pending {
            panic!("Payment is not pending");
        }

        // Release escrow: transfer from contract to merchant
        let client = token::Client::new(&env, &payment.token);
        client.transfer(
            &env.current_contract_address(),
            &payment.merchant,
            &payment.amount,
        );

        let old_status = payment.status;
        payment.status = PaymentStatus::Completed;
        env.storage()
            .instance()
            .set(&DataKey::Payment(payment_id), &payment);

        events::emit_payment_completed(&env, payment_id, payment.merchant, payment.amount);
        events::emit_payment_status_changed(&env, payment_id, old_status, PaymentStatus::Completed);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    // --- Dispute Methods ---

    /// Customer disputes a Pending payment. Transitions to Disputed status.
    pub fn dispute_payment(env: Env, customer: Address, payment_id: u32, reason: String) {
        customer.require_auth();

        let mut payment: Payment = env
            .storage()
            .instance()
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
            .instance()
            .set(&DataKey::Payment(payment_id), &payment);

        let dispute = Dispute {
            payment_id,
            reason: reason.clone(),
            created_at: env.ledger().timestamp(),
            resolved: false,
        };
        env.storage()
            .instance()
            .set(&DataKey::Dispute(payment_id), &dispute);

        events::emit_payment_disputed(&env, payment_id, customer, reason);
        events::emit_payment_status_changed(&env, payment_id, old_status, PaymentStatus::Disputed);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Admin resolves a dispute. If release_to_merchant is true, funds go to merchant;
    /// otherwise, customer is refunded.
    pub fn resolve_dispute(env: Env, payment_id: u32, release_to_merchant: bool) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        let mut payment: Payment = env
            .storage()
            .instance()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Disputed {
            panic!("Payment is not disputed");
        }

        let client = token::Client::new(&env, &payment.token);
        let old_status = payment.status;

        if release_to_merchant {
            // Release escrow to merchant
            client.transfer(
                &env.current_contract_address(),
                &payment.merchant,
                &payment.amount,
            );
            payment.status = PaymentStatus::Completed;
        } else {
            // Refund customer
            client.transfer(
                &env.current_contract_address(),
                &payment.customer,
                &payment.amount,
            );
            payment.status = PaymentStatus::Refunded;
        }

        env.storage()
            .instance()
            .set(&DataKey::Payment(payment_id), &payment);

        // Mark dispute as resolved
        let mut dispute: Dispute = env
            .storage()
            .instance()
            .get(&DataKey::Dispute(payment_id))
            .expect("Dispute not found");
        dispute.resolved = true;
        env.storage()
            .instance()
            .set(&DataKey::Dispute(payment_id), &dispute);

        events::emit_dispute_resolved(&env, payment_id, release_to_merchant, admin);
        events::emit_payment_status_changed(&env, payment_id, old_status, payment.status);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Check if a dispute has exceeded the timeout window. If so, emit DisputeEscalated.
    /// Returns true if the dispute was escalated.
    pub fn check_escalation(env: Env, payment_id: u32) -> bool {
        let payment: Payment = env
            .storage()
            .instance()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");

        if payment.status != PaymentStatus::Disputed {
            return false;
        }

        let dispute: Dispute = env
            .storage()
            .instance()
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

    // --- Admin ---

    /// Update the maximum batch size. Admin only.
    pub fn set_max_batch_size(env: Env, new_size: u32) {
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

    /// Update the dispute escalation timeout. Admin only.
    pub fn set_dispute_timeout(env: Env, timeout: u64) {
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

    // --- Read Interface ---

    /// Get a payment by its ID.
    pub fn get_payment(env: Env, payment_id: u32) -> Payment {
        env.storage()
            .instance()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found")
    }

    /// Get all payment IDs for a given customer.
    pub fn get_customer_payments(env: Env, customer: Address) -> Vec<u32> {
        env.storage()
            .instance()
            .get(&DataKey::CustomerPayments(customer))
            .unwrap_or(Vec::new(&env))
    }

    /// Get the current payment counter (total payments created).
    pub fn get_payment_counter(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::PaymentCounter)
            .unwrap_or(0)
    }

    /// Get the current maximum batch size.
    pub fn get_max_batch_size(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MaxBatchSize)
            .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
    }

    /// Check whether a payment is currently disputed.
    pub fn is_disputed(env: Env, payment_id: u32) -> bool {
        let payment: Payment = env
            .storage()
            .instance()
            .get(&DataKey::Payment(payment_id))
            .expect("Payment not found");
        payment.status == PaymentStatus::Disputed
    }

    /// Get the dispute record for a payment.
    pub fn get_dispute(env: Env, payment_id: u32) -> Dispute {
        env.storage()
            .instance()
            .get(&DataKey::Dispute(payment_id))
            .expect("No dispute found for this payment")
    }

    /// Get the current dispute timeout.
    pub fn get_dispute_timeout(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::DisputeTimeout)
            .unwrap_or(DEFAULT_DISPUTE_TIMEOUT)
    }

    // --- Internal Helpers ---

    /// Increment and return the next payment ID.
    fn next_payment_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PaymentCounter)
            .unwrap_or(0);
        let id = counter;
        counter += 1;
        env.storage()
            .instance()
            .set(&DataKey::PaymentCounter, &counter);
        id
    }

    /// Append a payment ID to a customer's payment list.
    fn add_customer_payment(env: &Env, customer: &Address, payment_id: u32) {
        let mut customer_payments: Vec<u32> = env
            .storage()
            .instance()
            .get(&DataKey::CustomerPayments(customer.clone()))
            .unwrap_or(Vec::new(env));
        customer_payments.push_back(payment_id);
        env.storage().instance().set(
            &DataKey::CustomerPayments(customer.clone()),
            &customer_payments,
        );
    }
}

#[cfg(test)]
mod test;

pub use events::*;
