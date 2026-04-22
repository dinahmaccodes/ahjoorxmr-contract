use soroban_sdk::{contractevent, Address, Env, String};

/// Event: Escrow created
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowCreated {
    pub escrow_id: u32,
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
    pub amount: i128,
    pub token: Address,
    pub deadline: u64,
}

/// Event: Escrow released to seller
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowReleased {
    pub escrow_id: u32,
    pub seller: Address,
    pub amount: i128,
}

/// Event: Escrow partially released to seller
#[contractevent]
#[derive(Clone, Debug)]
pub struct PartialReleased {
    pub escrow_id: u32,
    pub released_amount: i128,
    pub remaining_amount: i128,
}

/// Event: Escrow disputed
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowDisputed {
    pub escrow_id: u32,
    pub disputer: Address,
    pub reason: String,
}

/// Event: Dispute resolved
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeResolved {
    pub escrow_id: u32,
    pub release_to_seller: bool,
    pub resolved_by: Address,
}

/// Event: Escrow refunded to buyer
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowRefunded {
    pub escrow_id: u32,
    pub buyer: Address,
    pub amount: i128,
}

/// Event: Contract WASM upgraded
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractUpgraded {
    pub old_version: u32,
    pub new_version: u32,
    pub by_admin: Address,
}

/// Event: Deadline extension proposed by a participant
#[contractevent]
#[derive(Clone, Debug)]
pub struct DeadlineExtensionProposed {
    pub escrow_id: u32,
    pub proposer: Address,
    pub new_deadline: u64,
    pub proposed_at: u64,
}

/// Event: Deadline updated after counterparty acceptance
#[contractevent]
#[derive(Clone, Debug)]
pub struct DeadlineExtended {
    pub escrow_id: u32,
    pub old_deadline: u64,
    pub new_deadline: u64,
}

/// Event: Contract paused
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractPaused {
    pub admin: Address,
    pub reason: String,
    pub timestamp: u64,
}

/// Event: Contract resumed
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractResumed {
    pub admin: Address,
    pub timestamp: u64,
}

/// Event: Token Allowlisted
#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenAllowlisted {
    pub admin: Address,
    pub token: Address,
}

/// Event: Token Removed From Allowlist
#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenRemovedFromAllowlist {
    pub admin: Address,
    pub token: Address,
}

// --- Helper Emission Functions ---

pub fn emit_escrow_created(
    e: &Env,
    escrow_id: u32,
    buyer: Address,
    seller: Address,
    arbiter: Address,
    amount: i128,
    token: Address,
    deadline: u64,
) {
    EscrowCreated {
        escrow_id,
        buyer,
        seller,
        arbiter,
        amount,
        token,
        deadline,
    }
    .publish(e);
}

pub fn emit_escrow_released(e: &Env, escrow_id: u32, seller: Address, amount: i128) {
    EscrowReleased {
        escrow_id,
        seller,
        amount,
    }
    .publish(e);
}

pub fn emit_partial_released(
    e: &Env,
    escrow_id: u32,
    released_amount: i128,
    remaining_amount: i128,
) {
    PartialReleased {
        escrow_id,
        released_amount,
        remaining_amount,
    }
    .publish(e);
}

pub fn emit_escrow_disputed(e: &Env, escrow_id: u32, disputer: Address, reason: String) {
    EscrowDisputed {
        escrow_id,
        disputer,
        reason,
    }
    .publish(e);
}

pub fn emit_dispute_resolved(
    e: &Env,
    escrow_id: u32,
    release_to_seller: bool,
    resolved_by: Address,
) {
    DisputeResolved {
        escrow_id,
        release_to_seller,
        resolved_by,
    }
    .publish(e);
}

pub fn emit_escrow_refunded(e: &Env, escrow_id: u32, buyer: Address, amount: i128) {
    EscrowRefunded {
        escrow_id,
        buyer,
        amount,
    }
    .publish(e);
}

pub fn emit_contract_upgraded(e: &Env, old_version: u32, new_version: u32, by_admin: Address) {
    ContractUpgraded {
        old_version,
        new_version,
        by_admin,
    }
    .publish(e);
}
pub fn emit_deadline_extension_proposed(
    e: &Env,
    escrow_id: u32,
    proposer: Address,
    new_deadline: u64,
    proposed_at: u64,
) {
    DeadlineExtensionProposed {
        escrow_id,
        proposer,
        new_deadline,
        proposed_at,
    }
    .publish(e);
}

pub fn emit_contract_paused(e: &Env, admin: Address, reason: String, timestamp: u64) {
    ContractPaused {
        admin,
        reason,
        timestamp,
    }
    .publish(e);
}

pub fn emit_deadline_extended(e: &Env, escrow_id: u32, old_deadline: u64, new_deadline: u64) {
    DeadlineExtended {
        escrow_id,
        old_deadline,
        new_deadline,
    }
    .publish(e);
}

pub fn emit_contract_resumed(e: &Env, admin: Address, timestamp: u64) {
    ContractResumed { admin, timestamp }.publish(e);
}

pub fn emit_token_allowlisted(e: &Env, admin: Address, token: Address) {
    TokenAllowlisted { admin, token }.publish(e);
}

pub fn emit_token_removed_from_allowlist(e: &Env, admin: Address, token: Address) {
    TokenRemovedFromAllowlist { admin, token }.publish(e);
}

/// Event: Escrow template created
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowTemplateCreated {
    pub template_id: u32,
    pub creator: Address,
}

/// Event: Escrow template config updated
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowTemplateUpdated {
    pub template_id: u32,
    pub creator: Address,
}

/// Event: Escrow template deactivated
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowTemplateDeactivated {
    pub template_id: u32,
    pub creator: Address,
}

/// Event: Escrow created from a template
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowCreatedFromTemplate {
    pub escrow_id: u32,
    pub template_id: u32,
}

pub fn emit_escrow_template_created(e: &Env, template_id: u32, creator: Address) {
    EscrowTemplateCreated { template_id, creator }.publish(e);
}

pub fn emit_escrow_template_updated(e: &Env, template_id: u32, creator: Address) {
    EscrowTemplateUpdated { template_id, creator }.publish(e);
}

pub fn emit_escrow_template_deactivated(e: &Env, template_id: u32, creator: Address) {
    EscrowTemplateDeactivated { template_id, creator }.publish(e);
}

pub fn emit_escrow_created_from_template(e: &Env, escrow_id: u32, template_id: u32) {
    EscrowCreatedFromTemplate { escrow_id, template_id }.publish(e);
}
