#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String,
};

// --- Storage TTL Constants ---
const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;

const PERSISTENT_LIFETIME_THRESHOLD: u32 = 100_000;
const PERSISTENT_BUMP_AMOUNT: u32 = 120_000;

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
    pub customer: Address,
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
    RefundCounter,
    Refund(u32),
}

mod events;

#[contract]
pub struct AhjoorRefundContract;

#[contractimpl]
impl AhjoorRefundContract {
    /// Initialize the refund contract with an admin.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::RefundCounter, &0u32);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Request a refund. Customer must have tokens to be refunded.
    /// Returns the refund ID.
    pub fn request_refund(
        env: Env,
        customer: Address,
        amount: i128,
        token: Address,
        reason: String,
    ) -> u32 {
        customer.require_auth();

        if amount <= 0 {
            panic!("Refund amount must be positive");
        }

        let client = token::Client::new(&env, &token);
        client.transfer(&customer, &env.current_contract_address(), &amount);

        let refund_id = Self::next_refund_id(&env);
        let refund = Refund {
            id: refund_id,
            customer: customer.clone(),
            amount,
            token: token.clone(),
            status: RefundStatus::Requested,
            reason,
            requested_at: env.ledger().timestamp(),
            approved_at: None,
            processed_at: None,
        };

        env.storage().persistent().set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_refund_requested(
            &env,
            refund_id,
            customer,
            amount,
            token,
            refund.reason,
        );

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        refund_id
    }

    /// Approve a refund request. Only admin can call this.
    pub fn approve_refund(env: Env, admin: Address, refund_id: u32) {
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

        env.storage().persistent().set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_refund_approved(&env, refund_id, admin, refund.approved_at.unwrap());

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Reject a refund request. Only admin can call this.
    pub fn reject_refund(env: Env, admin: Address, refund_id: u32, rejection_reason: String) {
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

        env.storage().persistent().set(&DataKey::Refund(refund_id), &refund);
        env.storage().persistent().extend_ttl(
            &DataKey::Refund(refund_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_refund_rejected(&env, refund_id, admin, rejection_reason);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Process an approved refund. Transfers tokens to customer. Only admin can call this.
    pub fn process_refund(env: Env, admin: Address, refund_id: u32) {
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
        client.transfer(&env.current_contract_address(), &refund.customer, &refund.amount);

        refund.status = RefundStatus::Processed;
        refund.processed_at = Some(env.ledger().timestamp());

        env.storage().persistent().set(&DataKey::Refund(refund_id), &refund);
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

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get refund details
    pub fn get_refund(env: Env, refund_id: u32) -> Refund {
        env.storage()
            .persistent()
            .get(&DataKey::Refund(refund_id))
            .expect("Refund not found")
    }

    /// Get refund counter
    pub fn get_refund_counter(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::RefundCounter).unwrap_or(0)
    }

    /// Get admin address
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Not initialized")
    }

    // --- Internal Helpers ---

    fn next_refund_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RefundCounter)
            .unwrap_or(0);
        let id = counter;
        counter += 1;
        env.storage().instance().set(&DataKey::RefundCounter, &counter);
        id
    }
}

#[cfg(test)]
mod test;
