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

pub fn emit_escrow_disputed(e: &Env, escrow_id: u32, disputer: Address, reason: String) {
    EscrowDisputed {
        escrow_id,
        disputer,
        reason,
    }
    .publish(e);
}

pub fn emit_dispute_resolved(e: &Env, escrow_id: u32, release_to_seller: bool, resolved_by: Address) {
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
