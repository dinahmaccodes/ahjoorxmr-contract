#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, BytesN, Env, String,
};

// --- Storage TTL Constants ---
const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;

const PERSISTENT_LIFETIME_THRESHOLD: u32 = 100_000;
const PERSISTENT_BUMP_AMOUNT: u32 = 120_000;

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

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    ContractVersion,
    MigrationCompleted(u32),
    EscrowCounter,
    Escrow(u32),
    Dispute(u32),
}

mod events;

#[contract]
pub struct AhjoorEscrowContract;

#[contractimpl]
impl AhjoorEscrowContract {
    /// Initialize upgrade admin and contract versioning state.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ContractVersion, &1u32);

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

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

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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

        env.storage().instance().extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Returns the current contract version.
    pub fn get_version(env: Env) -> u32 {
        Self::get_or_init_version(&env)
    }

    // --- Internal Helpers ---

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
