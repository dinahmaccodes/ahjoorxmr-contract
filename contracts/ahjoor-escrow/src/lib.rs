#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, BytesN, Env, String, Vec};

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
    PartiallyReleased = 5,
    PartiallyDisputed = 6,
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
    pub dispute_amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeadlineProposal {
    pub proposer: Address,
    pub new_deadline: u64,
    pub proposed_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowTemplateConfig {
    pub arbiter: Address,
    pub token: Address,
    pub deadline_duration: u64, // seconds from escrow creation
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EscrowTemplate {
    pub id: u32,
    pub creator: Address,
    pub config: EscrowTemplateConfig,
    pub active: bool,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,
    ContractVersion,
    MigrationCompleted(u32),
    Paused,
    PauseReason,
    EscrowCounter,
    Escrow(u32),
    Dispute(u32),
    DeadlineProposal(u32),
    AllowedToken(Address),
    ProtocolFeeBps,
    FeeRecipient,
    TemplateCounter,
    Template(u32),
    ArbiterPool,
    NextArbiterIndex,
    ArbiterNeedsReplacement(u32),
}

const MAX_PROTOCOL_FEE_BPS: u32 = 200; // 2%

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
        env.storage()
            .instance()
            .set(&DataKey::ContractVersion, &1u32);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
        Self::require_not_paused(&env);
        buyer.require_auth();

        if amount <= 0 {
            panic!("Escrow amount must be positive");
        }

        if deadline <= env.ledger().timestamp() {
            panic!("Deadline must be in the future");
        }

        let is_allowed = env
            .storage()
            .instance()
            .get(&DataKey::AllowedToken(token.clone()))
            .unwrap_or(false);
        if !is_allowed {
            panic!("TokenNotAllowed");
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

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_created(
            &env, escrow_id, buyer, seller, arbiter, amount, token, deadline,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

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

        if !Self::is_open_escrow_status(escrow.status) {
            panic!("Escrow is not active");
        }

        if caller != escrow.buyer && caller != escrow.arbiter {
            panic!("Only buyer or arbiter can release escrow");
        }

        let client = token::Client::new(&env, &escrow.token);
        client.transfer(
            &env.current_contract_address(),
            &escrow.seller,
            &escrow.amount,
        );

        escrow.status = EscrowStatus::Released;

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_released(&env, escrow_id, escrow.seller, escrow.amount);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Release part of the escrowed funds to seller. Can be called by buyer or arbiter.
    pub fn partial_release(env: Env, caller: Address, escrow_id: u32, release_amount: i128) {
        Self::require_not_paused(&env);
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if !Self::is_open_escrow_status(escrow.status) {
            panic!("Escrow is not active");
        }

        if caller != escrow.buyer && caller != escrow.arbiter {
            panic!("Only buyer or arbiter can release escrow");
        }

        if release_amount <= 0 {
            panic!("Release amount must be positive");
        }

        if release_amount > escrow.amount {
            panic!("Release amount exceeds escrow balance");
        }

        let client = token::Client::new(&env, &escrow.token);
        client.transfer(
            &env.current_contract_address(),
            &escrow.seller,
            &release_amount,
        );

        escrow.amount -= release_amount;
        escrow.status = if escrow.amount == 0 {
            EscrowStatus::Released
        } else {
            EscrowStatus::PartiallyReleased
        };

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_partial_released(&env, escrow_id, release_amount, escrow.amount);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Dispute an escrow. Can be called by buyer or seller.
    /// Pass `dispute_amount` equal to the full escrow amount for a full dispute,
    /// or less for a partial dispute (undisputed portion is released to seller immediately).
    pub fn dispute_escrow(
        env: Env,
        caller: Address,
        escrow_id: u32,
        reason: String,
        dispute_amount: i128,
    ) {
        Self::require_not_paused(&env);
        caller.require_auth();

        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if !Self::is_open_escrow_status(escrow.status) {
            panic!("Escrow is not active");
        }

        if caller != escrow.buyer && caller != escrow.seller {
            panic!("Only buyer or seller can dispute escrow");
        }

        if dispute_amount <= 0 || dispute_amount > escrow.amount {
            panic!("dispute_amount must be > 0 and <= escrow amount");
        }

        let released_amount = escrow.amount - dispute_amount;

        // Release undisputed portion to seller immediately
        if released_amount > 0 {
            let client = token::Client::new(&env, &escrow.token);
            client.transfer(
                &env.current_contract_address(),
                &escrow.seller,
                &released_amount,
            );
        }

        escrow.amount = dispute_amount;
        escrow.status = if released_amount > 0 {
            EscrowStatus::PartiallyDisputed
        } else {
            EscrowStatus::Disputed
        };

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
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
            dispute_amount,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(escrow_id), &dispute);
        env.storage().persistent().extend_ttl(
            &DataKey::Dispute(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        if released_amount > 0 {
            events::emit_partial_dispute_raised(&env, escrow_id, dispute_amount, released_amount);
        } else {
            events::emit_escrow_disputed(&env, escrow_id, caller, reason);
        }

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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

        if escrow.status != EscrowStatus::Disputed
            && escrow.status != EscrowStatus::PartiallyDisputed
        {
            panic!("Escrow is not disputed");
        }

        if arbiter != escrow.arbiter {
            panic!("Only arbiter can resolve dispute");
        }

        let client = token::Client::new(&env, &escrow.token);

        // Compute and deduct protocol fee
        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ProtocolFeeBps)
            .unwrap_or(0);
        let protocol_fee = (escrow.amount * fee_bps as i128) / 10_000;

        if protocol_fee > 0 {
            let fee_recipient: Address = env
                .storage()
                .instance()
                .get(&DataKey::FeeRecipient)
                .expect("FeeRecipient not set");
            client.transfer(
                &env.current_contract_address(),
                &fee_recipient,
                &protocol_fee,
            );
            events::emit_protocol_fee_paid(&env, escrow_id, protocol_fee, fee_recipient);
        }

        let winner_amount = escrow.amount - protocol_fee;

        if release_to_seller {
            client.transfer(
                &env.current_contract_address(),
                &escrow.seller,
                &winner_amount,
            );
            escrow.status = EscrowStatus::Released;
        } else {
            client.transfer(
                &env.current_contract_address(),
                &escrow.buyer,
                &winner_amount,
            );
            escrow.status = EscrowStatus::Refunded;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
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
            env.storage()
                .persistent()
                .set(&DataKey::Dispute(escrow_id), &dispute);
        }

        events::emit_dispute_resolved(&env, escrow_id, release_to_seller, arbiter);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Set protocol fee in basis points and fee recipient. Admin only.
    /// Max fee is 200 bps (2%).
    pub fn update_protocol_fee(env: Env, admin: Address, fee_bps: u32, fee_recipient: Address) {
        Self::require_admin(&env, &admin);
        if fee_bps > MAX_PROTOCOL_FEE_BPS {
            panic!("Fee exceeds maximum of 200 bps");
        }
        env.storage()
            .instance()
            .set(&DataKey::ProtocolFeeBps, &fee_bps);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get current protocol fee bps and fee recipient.
    pub fn get_protocol_fee(env: Env) -> (u32, Option<Address>) {
        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ProtocolFeeBps)
            .unwrap_or(0);
        let fee_recipient: Option<Address> = env.storage().instance().get(&DataKey::FeeRecipient);
        (fee_bps, fee_recipient)
    }

    /// Auto-release expired escrow (past deadline, undisputed). Can be called by buyer.
    pub fn auto_release_expired(env: Env, escrow_id: u32) {
        Self::require_not_paused(&env);
        let mut escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if !Self::is_open_escrow_status(escrow.status) {
            panic!("Escrow is not active");
        }

        if env.ledger().timestamp() <= escrow.deadline {
            panic!("Escrow has not expired yet");
        }

        escrow.buyer.require_auth();

        let client = token::Client::new(&env, &escrow.token);
        client.transfer(
            &env.current_contract_address(),
            &escrow.buyer,
            &escrow.amount,
        );

        escrow.status = EscrowStatus::Refunded;

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_refunded(&env, escrow_id, escrow.buyer, escrow.amount);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Propose a new deadline for an active escrow.
    /// Only buyer or seller may propose and the proposal requires counterparty acceptance.
    pub fn propose_deadline_extension(
        env: Env,
        caller: Address,
        escrow_id: u32,
        new_deadline: u64,
    ) {
        caller.require_auth();

        let escrow: Escrow = env
            .storage()
            .persistent()
            .get(&DataKey::Escrow(escrow_id))
            .expect("Escrow not found");

        if caller != escrow.buyer && caller != escrow.seller {
            panic!("Only buyer or seller can propose deadline extension");
        }

        if escrow.status == EscrowStatus::Disputed || Self::is_terminal_escrow_status(escrow.status)
        {
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

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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

        if escrow.status == EscrowStatus::Disputed || Self::is_terminal_escrow_status(escrow.status)
        {
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

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
        env.storage().persistent().extend_ttl(
            &DataKey::Escrow(escrow_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
        env.storage()
            .persistent()
            .remove(&DataKey::DeadlineProposal(escrow_id));

        events::emit_deadline_extended(&env, escrow_id, old_deadline, escrow.deadline);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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
        env.storage()
            .instance()
            .get(&DataKey::EscrowCounter)
            .unwrap_or(0)
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

    /// Add a token to the allowlist. Admin only.
    pub fn add_allowed_token(env: Env, admin: Address, token: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::AllowedToken(token.clone()), &true);
        events::emit_token_allowlisted(&env, admin, token);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Remove a token from the allowlist. Admin only.
    pub fn remove_allowed_token(env: Env, admin: Address, token: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .remove(&DataKey::AllowedToken(token.clone()));
        events::emit_token_removed_from_allowlist(&env, admin, token);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Create a reusable escrow template. Returns the template ID.
    pub fn create_escrow_template(
        env: Env,
        creator: Address,
        config: EscrowTemplateConfig,
    ) -> u32 {
        creator.require_auth();

        let is_allowed = env
            .storage()
            .instance()
            .get(&DataKey::AllowedToken(config.token.clone()))
            .unwrap_or(false);
        if !is_allowed {
            panic!("TokenNotAllowed");
        }
        if config.deadline_duration == 0 {
            panic!("deadline_duration must be positive");
        }

        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TemplateCounter)
            .unwrap_or(0);
        let template_id = counter;
        counter += 1;
        env.storage()
            .instance()
            .set(&DataKey::TemplateCounter, &counter);

        let template = EscrowTemplate {
            id: template_id,
            creator: creator.clone(),
            config,
            active: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Template(template_id), &template);
        env.storage().persistent().extend_ttl(
            &DataKey::Template(template_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_template_created(&env, template_id, creator);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        template_id
    }

    /// Create an escrow from an existing template. Any caller may use any active template.
    pub fn create_escrow_from_template(
        env: Env,
        buyer: Address,
        seller: Address,
        template_id: u32,
        amount: i128,
    /// Add an arbiter to the pool. Admin only.
    pub fn add_arbiter(env: Env, admin: Address, arbiter: Address) {
        Self::require_admin(&env, &admin);
        let mut pool: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ArbiterPool)
            .unwrap_or(Vec::new(&env));
        for i in 0..pool.len() {
            if pool.get(i).unwrap() == arbiter {
                panic!("Arbiter already in pool");
            }
        }
        pool.push_back(arbiter.clone());
        env.storage()
            .instance()
            .set(&DataKey::ArbiterPool, &pool);
        events::emit_arbiter_pool_updated(&env, arbiter, true);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Remove an arbiter from the pool. Admin only.
    /// Active escrows with this arbiter are flagged via ArbiterNeedsReplacement.
    pub fn remove_arbiter(env: Env, admin: Address, arbiter: Address, escrow_ids: Vec<u32>) {
        Self::require_admin(&env, &admin);
        let mut pool: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ArbiterPool)
            .expect("Arbiter pool is empty");
        let mut found = false;
        let mut new_pool: Vec<Address> = Vec::new(&env);
        for i in 0..pool.len() {
            let a = pool.get(i).unwrap();
            if a == arbiter {
                found = true;
            } else {
                new_pool.push_back(a);
            }
        }
        if !found {
            panic!("Arbiter not in pool");
        }
        // Reset index if it would go out of bounds
        let next_idx: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextArbiterIndex)
            .unwrap_or(0);
        if new_pool.is_empty() || next_idx >= new_pool.len() {
            env.storage()
                .instance()
                .set(&DataKey::NextArbiterIndex, &0u32);
        }
        env.storage()
            .instance()
            .set(&DataKey::ArbiterPool, &new_pool);
        // Flag active escrows that used this arbiter
        for i in 0..escrow_ids.len() {
            let eid = escrow_ids.get(i).unwrap();
            if let Some(escrow) = env
                .storage()
                .persistent()
                .get::<DataKey, Escrow>(&DataKey::Escrow(eid))
            {
                if escrow.arbiter == arbiter && Self::is_open_escrow_status(escrow.status) {
                    env.storage()
                        .persistent()
                        .set(&DataKey::ArbiterNeedsReplacement(eid), &true);
                }
            }
        }
        events::emit_arbiter_pool_updated(&env, arbiter, false);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Create an escrow with the next arbiter from the pool (round-robin).
    pub fn create_escrow_with_pool_arbiter(
        env: Env,
        buyer: Address,
        seller: Address,
        amount: i128,
        token: Address,
        deadline: u64,
    ) -> u32 {
        Self::require_not_paused(&env);
        buyer.require_auth();

        let template: EscrowTemplate = env
            .storage()
            .persistent()
            .get(&DataKey::Template(template_id))
            .expect("Template not found");

        if !template.active {
            panic!("Template is deactivated");
        }
        if amount <= 0 {
            panic!("Escrow amount must be positive");
        }

        let deadline = env.ledger().timestamp() + template.config.deadline_duration;

        let client = token::Client::new(&env, &template.config.token);
        let pool: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ArbiterPool)
            .unwrap_or(Vec::new(&env));
        if pool.is_empty() {
            panic!("Arbiter pool is empty");
        }

        let idx: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextArbiterIndex)
            .unwrap_or(0);
        let arbiter = pool.get(idx % pool.len()).unwrap();
        let next_idx = (idx + 1) % pool.len();
        env.storage()
            .instance()
            .set(&DataKey::NextArbiterIndex, &next_idx);

        if amount <= 0 {
            panic!("Escrow amount must be positive");
        }
        if deadline <= env.ledger().timestamp() {
            panic!("Deadline must be in the future");
        }
        let is_allowed = env
            .storage()
            .instance()
            .get(&DataKey::AllowedToken(token.clone()))
            .unwrap_or(false);
        if !is_allowed {
            panic!("TokenNotAllowed");
        }

        let client = token::Client::new(&env, &token);
        client.transfer(&buyer, &env.current_contract_address(), &amount);

        let escrow_id = Self::next_escrow_id(&env);
        let escrow = Escrow {
            id: escrow_id,
            buyer: buyer.clone(),
            seller: seller.clone(),
            arbiter: template.config.arbiter.clone(),
            amount,
            token: template.config.token.clone(),
            arbiter: arbiter.clone(),
            amount,
            token: token.clone(),
            status: EscrowStatus::Active,
            created_at: env.ledger().timestamp(),
            deadline,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Escrow(escrow_id), &escrow);
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
            template.config.arbiter,
            amount,
            template.config.token,
            deadline,
        );
        events::emit_escrow_created_from_template(&env, escrow_id, template_id);
            &env, escrow_id, buyer, seller, arbiter.clone(), amount, token, deadline,
        );
        events::emit_arbiter_assigned(&env, escrow_id, arbiter);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        escrow_id
    }

    /// Update a template's config. Only the template creator can call this.
    pub fn update_escrow_template(
        env: Env,
        creator: Address,
        template_id: u32,
        new_config: EscrowTemplateConfig,
    ) {
        creator.require_auth();

        let mut template: EscrowTemplate = env
            .storage()
            .persistent()
            .get(&DataKey::Template(template_id))
            .expect("Template not found");

        if template.creator != creator {
            panic!("Only template creator can update");
        }
        if !template.active {
            panic!("Template is deactivated");
        }

        let is_allowed = env
            .storage()
            .instance()
            .get(&DataKey::AllowedToken(new_config.token.clone()))
            .unwrap_or(false);
        if !is_allowed {
            panic!("TokenNotAllowed");
        }
        if new_config.deadline_duration == 0 {
            panic!("deadline_duration must be positive");
        }

        template.config = new_config;
        env.storage()
            .persistent()
            .set(&DataKey::Template(template_id), &template);
        env.storage().persistent().extend_ttl(
            &DataKey::Template(template_id),
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );

        events::emit_escrow_template_updated(&env, template_id, creator);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Deactivate a template. Only the template creator can call this.
    pub fn deactivate_escrow_template(env: Env, creator: Address, template_id: u32) {
        creator.require_auth();

        let mut template: EscrowTemplate = env
            .storage()
            .persistent()
            .get(&DataKey::Template(template_id))
            .expect("Template not found");

        if template.creator != creator {
            panic!("Only template creator can deactivate");
        }
        if !template.active {
            panic!("Template already deactivated");
        }

        template.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Template(template_id), &template);

        events::emit_escrow_template_deactivated(&env, template_id, creator);

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Get template details.
    pub fn get_escrow_template(env: Env, template_id: u32) -> EscrowTemplate {
        env.storage()
            .persistent()
            .get(&DataKey::Template(template_id))
            .expect("Template not found")
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

    fn require_or_bootstrap_admin(env: &Env, admin: &Address) {
        admin.require_auth();
        if let Some(stored_admin) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::Admin)
        {
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

    fn is_open_escrow_status(status: EscrowStatus) -> bool {
        matches!(
            status,
            EscrowStatus::Active | EscrowStatus::PartiallyReleased
        )
    }

    fn is_terminal_escrow_status(status: EscrowStatus) -> bool {
        matches!(
            status,
            EscrowStatus::Released | EscrowStatus::Resolved | EscrowStatus::Refunded
        )
    }

    fn next_escrow_id(env: &Env) -> u32 {
        let mut counter: u32 = env
            .storage()
            .instance()
            .get(&DataKey::EscrowCounter)
            .unwrap_or(0);
        let id = counter;
        counter += 1;
        env.storage()
            .instance()
            .set(&DataKey::EscrowCounter, &counter);
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
