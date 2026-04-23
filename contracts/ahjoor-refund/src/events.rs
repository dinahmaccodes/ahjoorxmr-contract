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
    pub rejected_at: u64,
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

/// Event: Contract WASM upgraded
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractUpgraded {
    pub old_version: u32,
    pub new_version: u32,
    pub by_admin: Address,
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

/// Event: Refund auto-approved after dispute window elapsed without merchant response
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundAutoApproved {
    pub refund_id: u32,
    pub customer: Address,
    pub amount: i128,
}

/// Event: Refund auto-approved via whitelist
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundAutoApprovedWhitelist {
    pub refund_id: u32,
    pub merchant: Address,
    pub amount: i128,
}

/// Event: Escrow refund registered
#[contractevent]
#[derive(Clone, Debug)]
pub struct EscrowRefundRegistered {
    pub refund_id: u32,
    pub escrow_id: u32,
    pub buyer: Address,
    pub amount: i128,
}

/// Event: Refund fee collected
#[contractevent]
#[derive(Clone, Debug)]
pub struct RefundFeeCollected {
    pub refund_id: u32,
    pub fee_amount: i128,
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
    rejected_at: u64,
) {
    RefundRejected {
        refund_id,
        rejected_by,
        rejection_reason,
        rejected_at,
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

pub fn emit_contract_upgraded(e: &Env, old_version: u32, new_version: u32, by_admin: Address) {
    ContractUpgraded {
        old_version,
        new_version,
        by_admin,
    }
    .publish(e);
}
pub fn emit_refund_auto_approved(e: &Env, refund_id: u32, customer: Address, amount: i128) {
    RefundAutoApproved {
        refund_id,
        customer,
        amount,
    }
    .publish(e);
}

pub fn emit_partial_refund_cap_applied(e: &Env, refund_id: u32, remaining_refundable: i128) {
    PartialRefundCapApplied {
        refund_id,
        remaining_refundable,
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

pub fn emit_refund_auto_approved_whitelist(e: &Env, refund_id: u32, merchant: Address, amount: i128) {
    RefundAutoApprovedWhitelist {
        refund_id,
        merchant,
        amount,
    }
    .publish(e);
}

pub fn emit_escrow_refund_registered(
    e: &Env,
    refund_id: u32,
    escrow_id: u32,
    buyer: Address,
    amount: i128,
) {
    EscrowRefundRegistered {
        refund_id,
        escrow_id,
        buyer,
        amount,
    }
    .publish(e);
}

pub fn emit_refund_fee_collected(e: &Env, refund_id: u32, fee_amount: i128) {
    RefundFeeCollected {
        refund_id,
        fee_amount,
    }
    .publish(e);
}
