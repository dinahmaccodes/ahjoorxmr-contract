#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, String,
};

// --- Storage TTL Constants ---
const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;

const PERSISTENT_LIFETIME_THRESHOLD: u32 = 100_000;
const PERSISTENT_BUMP_AMOUNT: u32 = 120_000;
const DEADLINE_EXTENSION_PROPOSAL_WINDOW: u64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum EscrowStatus {
    Active = 0,
    Released = 1,
    Disputed = 2,
    Resolved = 3,
    Refunded = 4,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Escrow {
    pub id: u32,
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
    pub amount: i128,
    pub token: Address,
    pub status: EscrowStatus,
    pub created_at: u64,
    pub deadline: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub escrow_id: u32,
    pub reason: String,
    pub created_at: u64,
    pub resolved: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeadlineProposal {
    pub proposer: Address,
    pub new_deadline: u64,
    pub proposed_at: u64,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    Paused,
    PauseReason,
    EscrowCounter,
    Escrow(u32),
    Dispute(u32),
    DeadlineProposal(u32),
}

mod events;

#[contract]
pub struct AhjoorEscrowContract;

#[contractimpl]
impl AhjoorEscrowContract {
    /// Create a new escrow. Funds are transferred from buyer to contract.
    /// Returns the escrow ID.
    pub fn create_escrow(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        amount: i128,
        token: Address,
        deadline: u64,
    ) -> u32 {
        Self::require_not_paused(&env);
        buyer.require_auth();

        if amount <= 0 {
            panic!("Escrow amount must be positive");
        }

        if deadline <= env.ledger().timestamp() {
            panic!("Deadline must be in the future");
        }

        // Transfer tokens from buyer to contract (escrow)
        let client = token::Client::new(&env, &token);
        client.transfer(&buyer, &env.current_contract_address(), &amount);

        let escrow_id = Self::next_escrow_id(&env);
        let escrow = Escrow {
            id: escrow_id,
            buyer: buyer.clone(),
            seller: seller.clone(),
            arbiter: arbiter.clone(),
            amount,
            token: token.clone(),
            status: EscrowStatus::Active,
            created_at: env.ledger().timestamp(),
            deadline,
        };

        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_created(
            &env,
            escrow_id,
            buyer,
            seller,
            arbiter,
            amount,
            token,
            deadline,
        );

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        escrow_id
    }

    /// Release escrowed funds to seller. Can be called by buyer or arbiter.
    pub fn release_escrow(env: Env, caller: Address, escrow_id: u32) {
        Self::require_not_paused(&env);
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if escrow.status != EscrowStatus::Active {
            panic!("Escrow is not active");
        }

        if caller != escrow.buyer && caller != escrow.arbiter {
            panic!("Only buyer or arbiter can release escrow");
        }

        let client = token::Client::new(&env, &escrow.token);
        client.transfer(&env.current_contract_address(), &escrow.seller, &escrow.amount);

        escrow.status = EscrowStatus::Released;

        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_released(&env, escrow_id, escrow.seller, escrow.amount);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Dispute an escrow. Can be called by buyer or seller.
    pub fn dispute_escrow(env: Env, caller: Address, escrow_id: u32, reason: String) {
        Self::require_not_paused(&env);
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if escrow.status != EscrowStatus::Active {
            panic!("Escrow is not active");
        }

        if caller != escrow.buyer && caller != escrow.seller {
            panic!("Only buyer or seller can dispute escrow");
        }

        escrow.status = EscrowStatus::Disputed;

        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        let dispute = Dispute {
            escrow_id,
            reason: reason.clone(),
            created_at: env.ledger().timestamp(),
            resolved: false,
        };
        env.storage().persistent().set(&DataKey::Dispute(escrow_id), &dispute);
        env.storage().persistent().extend_ttl(
            &DataKey::Dispute(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_disputed(&env, escrow_id, caller, reason);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Resolve a dispute. Only arbiter can call this.
    pub fn resolve_dispute(env: Env, arbiter: Address, escrow_id: u32, release_to_seller: bool) {
        Self::require_not_paused(&env);
        arbiter.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if escrow.status != EscrowStatus::Disputed {
            panic!("Escrow is not disputed");
        }

        if arbiter != escrow.arbiter {
            panic!("Only arbiter can resolve dispute");
        }

        let client = token::Client::new(&env, &escrow.token);

        if release_to_seller {
            client.transfer(&env.current_contract_address(), &escrow.seller, &escrow.amount);
            escrow.status = EscrowStatus::Released;
        } else {
            client.transfer(&env.current_contract_address(), &escrow.buyer, &escrow.amount);
            escrow.status = EscrowStatus::Refunded;
        }

        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        // Mark dispute as resolved
        if let Some(mut dispute) = env
            .storage()
            .persistent()
            .get::<DataKey, Dispute>(&DataKey::Dispute(escrow_id))
        {
            dispute.resolved = true;
            env.storage().persistent().set(&DataKey::Dispute(escrow_id), &dispute);
        }

        events::emit_dispute_resolved(&env, escrow_id, release_to_seller, arbiter);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Auto-release expired escrow (past deadline, undisputed). Can be called by buyer.
    pub fn auto_release_expired(env: Env, escrow_id: u32) {
        Self::require_not_paused(&env);
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if escrow.status != EscrowStatus::Active {
            panic!("Escrow is not active");
        }

        if env.ledger().timestamp() <= escrow.deadline {
            panic!("Escrow has not expired yet");
        }

        escrow.buyer.require_auth();

        let client = token::Client::new(&env, &escrow.token);
        client.transfer(&env.current_contract_address(), &escrow.buyer, &escrow.amount);

        escrow.status = EscrowStatus::Refunded;

        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_refunded(&env, escrow_id, escrow.buyer, escrow.amount);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Propose a new deadline for an active escrow.
    /// Only buyer or seller may propose and the proposal requires counterparty acceptance.
    pub fn propose_deadline_extension(env: Env, caller: Address, escrow_id: u32, new_deadline: u64) {
        caller.require_auth();

        let escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if caller != escrow.buyer && caller != escrow.seller {
            panic!("Only buyer or seller can propose deadline extension");
        }

        if escrow.status == EscrowStatus::Disputed {
            panic!("Cannot extend deadline while escrow is disputed");
        }

        if new_deadline <= escrow.deadline {
            panic!("New deadline must be greater than current deadline");
        }

        let proposal = DeadlineProposal {
            proposer: caller.clone(),
            new_deadline,
            proposed_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::DeadlineProposal(escrow_id), &proposal);
        env.storage().persistent().extend_ttl(
            &DataKey::DeadlineProposal(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_deadline_extension_proposed(
            &env,
            escrow_id,
            caller,
            proposal.new_deadline,
            proposal.proposed_at,
        );

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Accept a pending deadline extension proposed by the counterparty.
    pub fn accept_deadline_extension(env: Env, caller: Address, escrow_id: u32) {
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if caller != escrow.buyer && caller != escrow.seller {
            panic!("Only buyer or seller can accept deadline extension");
        }

        if escrow.status == EscrowStatus::Disputed {
            panic!("Cannot extend deadline while escrow is disputed");
        }

        let proposal: DeadlineProposal = env
            .storage()
            .persistent()
            .get(&DataKey::DeadlineProposal(escrow_id))
            .expect("No deadline extension proposal found");

        if caller == proposal.proposer {
            panic!("Proposer cannot accept their own deadline extension");
        }

        let now = env.ledger().timestamp();
        if now > proposal.proposed_at + DEADLINE_EXTENSION_PROPOSAL_WINDOW {
            env.storage()
                .persistent()
                .remove(&DataKey::DeadlineProposal(escrow_id));
            panic!("Deadline extension proposal has expired");
        }

        if proposal.new_deadline <= escrow.deadline {
            env.storage()
                .persistent()
                .remove(&DataKey::DeadlineProposal(escrow_id));
            panic!("New deadline must be greater than current deadline");
        }

        let old_deadline = escrow.deadline;
        escrow.deadline = proposal.new_deadline;

        env.storage().persistent().set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
        env.storage()
            .persistent()
            .remove(&DataKey::DeadlineProposal(escrow_id));

        events::emit_deadline_extended(&env, escrow_id, old_deadline, escrow.deadline);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get escrow details
    pub fn get_escrow(env: Env, escrow_id: u32) -> Escrow {
        env.storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found")
    }

    /// Get dispute details
    pub fn get_dispute(env: Env, escrow_id: u32) -> Dispute {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(escrow_id))
            .expect("No dispute found for this escrow")
    }

    /// Get escrow counter
    pub fn get_escrow_counter(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::EscrowCounter).unwrap_or(0)
    }

    pub fn pause_contract(env: Env, admin: Address, reason: String) {
        Self::require_or_bootstrap_admin(&env, &admin);

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

    fn require_or_bootstrap_admin(env: &Env, admin: &Address) {
        admin.require_auth();
        if let Some(stored_admin) = env.storage().instance().get::<DataKey, Address>(&DataKey::Admin) {
            if stored_admin != *admin {
                panic!("Only admin can pause contract");
            }
        } else {
            env.storage().instance().set(&DataKey::Admin, admin);
        }
    }

    fn require_admin(env: &Env, admin: &Address) {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        if stored_admin != *admin {
            panic!("Only admin can resume contract");
        }
    }

    fn next_escrow_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::EscrowCounter)
            .unwrap_or(0);
        let id = counter;
        counter += 1;
        env.storage().instance().set(&DataKey::EscrowCounter, &counter);
        id
    }
}

#[cfg(test)]
mod test;
