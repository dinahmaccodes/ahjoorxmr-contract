use soroban_sdk::{contractevent, Address, Env, String};

/// Event: Refund requested
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundRequested {
    pub refund_id: u32,
    pub customer: Address,
    pub amount: i128,
    pub token: Address,
    pub reason: String,
}

/// Event: Refund approved
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundApproved {
    pub refund_id: u32,
    pub approved_by: Address,
    pub approved_at: u64,
}

/// Event: Refund rejected
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundRejected {
    pub refund_id: u32,
    pub rejected_by: Address,
    pub rejection_reason: String,
}

/// Event: Refund processed (tokens transferred)
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundProcessed {
    pub refund_id: u32,
    pub customer: Address,
    pub amount: i128,
    pub processed_at: u64,
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

// --- Helper Emission Functions ---

pub fn emit_refund_requested(
    e: &Env,
    refund_id: u32,
    customer: Address,
    amount: i128,
    token: Address,
    reason: String,
) {
    RefundRequested {
        refund_id,
        customer,
        amount,
        token,
        reason,
    }
    .publish(e);
}

pub fn emit_refund_approved(e: &Env, refund_id: u32, approved_by: Address, approved_at: u64) {
    RefundApproved {
        refund_id,
        approved_by,
        approved_at,
    }
    .publish(e);
}

pub fn emit_refund_rejected(
    e: &Env,
    refund_id: u32,
    rejected_by: Address,
    rejection_reason: String,
) {
    RefundRejected {
        refund_id,
        rejected_by,
        rejection_reason,
    }
    .publish(e);
}

pub fn emit_refund_processed(
    e: &Env,
    refund_id: u32,
    customer: Address,
    amount: i128,
    processed_at: u64,
) {
    RefundProcessed {
        refund_id,
        customer,
        amount,
        processed_at,
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

pub fn emit_contract_resumed(e: &Env, admin: Address, timestamp: u64) {
    ContractResumed { admin, timestamp }.publish(e);
}
