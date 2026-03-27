use crate::PaymentStatus;
use soroban_sdk::{contractevent, Address, BytesN, Env, String};

/// Event: Payment receipt issued on completion (#65)
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentReceiptIssued {
    pub payment_id: u32,
    pub receipt_hash: BytesN<32>,
}

/// Event: Multi-token payment created (customer paid in non-USDC token)
#[contractevent]
#[derive(Clone, Debug)]
pub struct MultiTokenPaymentCreated {
    pub payment_id: u32,
    pub customer: Address,
    pub merchant: Address,
    pub amount_usdc: i128,
    pub payment_token: Address,
    pub token_amount: i128,
    /// Oracle price used (scaled by 10^7)
    pub oracle_price: i128,
}

/// Event: Individual payment created
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentCreated {
    pub payment_id: u32,
    pub customer: Address,
    pub merchant: Address,
    pub amount: i128,
    pub token: Address,
}

/// Event: Batch payment operation completed
#[contractevent]
#[derive(Clone, Debug)]
pub struct BatchPaymentCreated {
    pub customer: Address,
    pub payment_count: u32,
    pub total_amount: i128,
}

/// Event: Payment status changed
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentStatusChanged {
    pub payment_id: u32,
    pub old_status: PaymentStatus,
    pub new_status: PaymentStatus,
}

/// Event: Payment completed (released from escrow to merchant)
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentCompleted {
    pub payment_id: u32,
    pub merchant: Address,
    pub amount: i128,
    pub completed_at: u64,
}

/// Event: Payment expired — funds returned to customer
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentExpired {
    pub payment_id: u32,
    pub customer: Address,
    pub amount: i128,
    pub expired_at: u64,
}

/// Event: Partial refund issued on a pending/disputed payment
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentPartialRefund {
    pub payment_id: u32,
    pub customer: Address,
    pub refund_amount: i128,
    pub remaining: i128,
}

/// Event: Subscription charged
#[contractevent]
#[derive(Clone, Debug)]
pub struct SubscriptionCharged {
    pub subscription_id: u32,
    pub subscriber: Address,
    pub merchant: Address,
    pub amount: i128,
    pub charged_at: u64,
}

/// Event: Subscription cancelled
#[contractevent]
#[derive(Clone, Debug)]
pub struct SubscriptionCancelled {
    pub subscription_id: u32,
    pub cancelled_by: Address,
}

/// Event: Merchant settlement batch processed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BatchSettlementProcessed {
    pub merchant: Address,
    pub total_amount: i128,
    pub fee_collected: i128,
    pub payment_count: u32,
}

/// Event: Payment disputed by customer
#[contractevent]
#[derive(Clone, Debug)]
pub struct PaymentDisputed {
    pub payment_id: u32,
    pub customer: Address,
    pub reason: String,
}

/// Event: Dispute resolved by admin
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeResolved {
    pub payment_id: u32,
    pub release_to_merchant: bool,
    pub resolved_by: Address,
}

/// Event: Dispute auto-escalated after timeout
#[contractevent]
#[derive(Clone, Debug)]
pub struct DisputeEscalated {
    pub payment_id: u32,
    pub elapsed_seconds: u64,
}

/// Event: Admin transfer proposed
#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminTransferProposed {
    pub current_admin: Address,
    pub proposed_admin: Address,
}

/// Event: Admin transfer accepted
#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminTransferred {
    pub old_admin: Address,
    pub new_admin: Address,
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

// --- Helper Emission Functions ---

pub fn emit_payment_created(
    e: &Env,
    payment_id: u32,
    customer: Address,
    merchant: Address,
    amount: i128,
    token: Address,
) {
    PaymentCreated {
        payment_id,
        customer,
        merchant,
        amount,
        token,
    }
    .publish(e);
}

pub fn emit_batch_payment_created(
    e: &Env,
    customer: Address,
    payment_count: u32,
    total_amount: i128,
) {
    BatchPaymentCreated {
        customer,
        payment_count,
        total_amount,
    }
    .publish(e);
}

pub fn emit_payment_status_changed(
    e: &Env,
    payment_id: u32,
    old_status: PaymentStatus,
    new_status: PaymentStatus,
) {
    PaymentStatusChanged {
        payment_id,
        old_status,
        new_status,
    }
    .publish(e);
}

pub fn emit_payment_completed(
    e: &Env,
    payment_id: u32,
    merchant: Address,
    amount: i128,
    completed_at: u64,
) {
    PaymentCompleted {
        payment_id,
        merchant,
        amount,
        completed_at,
    }
    .publish(e);
}

pub fn emit_payment_expired(
    e: &Env,
    payment_id: u32,
    customer: Address,
    amount: i128,
    expired_at: u64,
) {
    PaymentExpired {
        payment_id,
        customer,
        amount,
        expired_at,
    }
    .publish(e);
}

pub fn emit_payment_partial_refund(
    e: &Env,
    payment_id: u32,
    customer: Address,
    refund_amount: i128,
    remaining: i128,
) {
    PaymentPartialRefund {
        payment_id,
        customer,
        refund_amount,
        remaining,
    }
    .publish(e);
}

pub fn emit_subscription_charged(
    e: &Env,
    subscription_id: u32,
    subscriber: Address,
    merchant: Address,
    amount: i128,
    charged_at: u64,
) {
    SubscriptionCharged {
        subscription_id,
        subscriber,
        merchant,
        amount,
        charged_at,
    }
    .publish(e);
}

pub fn emit_subscription_cancelled(e: &Env, subscription_id: u32, cancelled_by: Address) {
    SubscriptionCancelled {
        subscription_id,
        cancelled_by,
    }
    .publish(e);
}

pub fn emit_batch_settlement_processed(
    e: &Env,
    merchant: Address,
    total_amount: i128,
    fee_collected: i128,
    payment_count: u32,
) {
    BatchSettlementProcessed {
        merchant,
        total_amount,
        fee_collected,
        payment_count,
    }
    .publish(e);
}

pub fn emit_payment_disputed(e: &Env, payment_id: u32, customer: Address, reason: String) {
    PaymentDisputed {
        payment_id,
        customer,
        reason,
    }
    .publish(e);
}

pub fn emit_dispute_resolved(
    e: &Env,
    payment_id: u32,
    release_to_merchant: bool,
    resolved_by: Address,
) {
    DisputeResolved {
        payment_id,
        release_to_merchant,
        resolved_by,
    }
    .publish(e);
}

pub fn emit_dispute_escalated(e: &Env, payment_id: u32, elapsed_seconds: u64) {
    DisputeEscalated {
        payment_id,
        elapsed_seconds,
    }
    .publish(e);
}

pub fn emit_admin_transfer_proposed(e: &Env, current_admin: Address, proposed_admin: Address) {
    AdminTransferProposed {
        current_admin,
        proposed_admin,
    }
    .publish(e);
}

pub fn emit_admin_transferred(e: &Env, old_admin: Address, new_admin: Address) {
    AdminTransferred {
        old_admin,
        new_admin,
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

pub fn emit_payment_receipt_issued(e: &Env, payment_id: u32, receipt_hash: BytesN<32>) {
    PaymentReceiptIssued {
        payment_id,
        receipt_hash,
    }
    .publish(e);
}

#[allow(clippy::too_many_arguments)]
pub fn emit_multi_token_payment_created(
    e: &Env,
    payment_id: u32,
    customer: Address,
    merchant: Address,
    amount_usdc: i128,
    payment_token: Address,
    token_amount: i128,
    oracle_price: i128,
) {
    MultiTokenPaymentCreated {
        payment_id,
        customer,
        merchant,
        amount_usdc,
        payment_token,
        token_amount,
        oracle_price,
    }
    .publish(e);
}
