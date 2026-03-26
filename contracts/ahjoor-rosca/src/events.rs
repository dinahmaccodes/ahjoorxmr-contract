use crate::DistributionType;
use soroban_sdk::{contractevent, Address, Env, Symbol, Vec};

/// Event: Rosca initialized
#[contractevent]
#[derive(Clone, Debug)]
pub struct RoscaInitialized {
    pub member_count: u32,
    pub contribution_amount: i128,
}

/// Event: Contribution received
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContributionReceived {
    pub contributor: Address,
    pub round: u32,
    pub token: Address,
    pub amount: i128,
}

/// Event: Savings milestone reached
#[contractevent]
#[derive(Clone, Debug)]
pub struct SavingsMilestoneReached {
    pub milestone: u32,
    pub total_collected: i128,
}

/// Event: Round closed (deadline passed, defaulters identified)
#[contractevent]
#[derive(Clone, Debug)]
pub struct RoundClosed {
    pub round: u32,
    pub defaulters: Vec<Address>,
}

/// Event: Member defaulted on a round
#[contractevent]
#[derive(Clone, Debug)]
pub struct MemberDefaulted {
    pub member: Address,
    pub round: u32,
    pub penalty_amount: i128,
    pub default_count: u32,
}

/// Event: Member suspended due to multiple defaults
#[contractevent]
#[derive(Clone, Debug)]
pub struct MemberSuspended {
    pub member: Address,
    pub default_count: u32,
}

/// Event: New member added
#[contractevent]
#[derive(Clone, Debug)]
pub struct MemberAdded {
    pub member: Address,
    pub member_count: u32,
}

/// Event: Member removed by admin
#[contractevent]
#[derive(Clone, Debug)]
pub struct MemberRemoved {
    pub member: Address,
    pub member_count: u32,
}

/// Event: Token approved for contributions
#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenApproved {
    pub token: Address,
}

/// Event: Token removed from approved list
#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenRemoved {
    pub token: Address,
}

/// Event: Exchange rate updated for a token
#[contractevent]
#[derive(Clone, Debug)]
pub struct ExchangeRateSet {
    pub token: Address,
    pub rate: i128,
}

/// Event: Contribution limit set for a token
#[contractevent]
#[derive(Clone, Debug)]
pub struct TokenLimitSet {
    pub token: Address,
    pub limit: i128,
}

/// Event: Rewards deposited into the pool
#[contractevent]
#[derive(Clone, Debug)]
pub struct RewardDeposited {
    pub depositor: Address,
    pub amount: i128,
}

/// Event: Reward distribution configuration updated
#[contractevent]
#[derive(Clone, Debug)]
pub struct RewardConfigUpdated {
    pub dist_type: DistributionType,
}

/// Event: Rewards claimed by a member
#[contractevent]
#[derive(Clone, Debug)]
pub struct RewardClaimed {
    pub member: Address,
    pub amount: i128,
}

/// Event: New governance proposal created
#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalCreated {
    pub proposal_id: u32,
    pub creator: Address,
    pub target_member: Address,
    pub created_at: u64,
    pub deadline: u64,
}

/// Event: Vote cast on a proposal
#[contractevent]
#[derive(Clone, Debug)]
pub struct VoteCast {
    pub proposal_id: u32,
    pub voter: Address,
    pub vote_for: bool,
}

/// Event: Proposal rejected
#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalRejected {
    pub proposal_id: u32,
    pub reason: Symbol,
    pub votes_for: u32,
    pub votes_against: u32,
}

/// Event: Proposal executed
#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalExecuted {
    pub proposal_id: u32,
    pub proposal_type: u32,
    pub target_member: Address,
}

/// Event: Penalty appeal successfully approved
#[contractevent]
#[derive(Clone, Debug)]
pub struct PenaltyAppealApproved {
    pub member: Address,
}

/// Event: Voting quorum percentage updated
#[contractevent]
#[derive(Clone, Debug)]
pub struct QuorumUpdated {
    pub new_quorum: i128,
}

/// Event: Member removed via proposal execution
#[contractevent]
#[derive(Clone, Debug)]
pub struct MemberRemovalExecuted {
    pub member: Address,
}

/// Event: Deadline reminder emitted
#[contractevent]
#[derive(Clone, Debug)]
pub struct DeadlineReminder {
    pub round: u32,
    pub time_remaining: u64,
    pub non_contributors: Vec<Address>,
    pub interval: Symbol,
}

/// Event: Contract paused
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractPaused {
    pub reason: soroban_sdk::String,
}

/// Event: Contract resumed
#[contractevent]
#[derive(Clone, Debug)]
pub struct ContractResumed {
    pub reason: soroban_sdk::String,
}

/// Event: Emergency exit requested
#[contractevent]
#[derive(Clone, Debug)]
pub struct ExitRequested {
    pub member: Address,
    pub round: u32,
    pub refund_amount: i128,
}

/// Event: Emergency exit approved
#[contractevent]
#[derive(Clone, Debug)]
pub struct ExitApproved {
    pub member: Address,
    pub refund_amount: i128,
}

/// Event: Emergency exit rejected
#[contractevent]
#[derive(Clone, Debug)]
pub struct ExitRejected {
    pub member: Address,
}

/// Event: Round payout completed
#[contractevent]
#[derive(Clone, Debug)]
pub struct RoundCompleted {
    pub round: u32,
    pub recipient: Address,
    pub payout_amount: i128,
}

/// Event: Round state reset for next round
#[contractevent]
#[derive(Clone, Debug)]
pub struct RoundReset {
    pub round: u32,
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

// --- Helper Emission Functions ---

pub fn emit_rosc_init(e: &Env, member_count: u32, contribution_amount: i128) {
    RoscaInitialized {
        member_count,
        contribution_amount,
    }
    .publish(e);
}

pub fn emit_contrib(e: &Env, contributor: Address, round: u32, token: Address, amount: i128) {
    ContributionReceived {
        contributor,
        round,
        token,
        amount,
    }
    .publish(e);
}

pub fn emit_milestone(e: &Env, milestone: u32, total_collected: i128) {
    SavingsMilestoneReached {
        milestone,
        total_collected,
    }
    .publish(e);
}

pub fn emit_closed(e: &Env, round: u32, defaulters: Vec<Address>) {
    RoundClosed { round, defaulters }.publish(e);
}

pub fn emit_defaulted(
    e: &Env,
    member: Address,
    round: u32,
    penalty_amount: i128,
    default_count: u32,
) {
    MemberDefaulted {
        member,
        round,
        penalty_amount,
        default_count,
    }
    .publish(e);
}

pub fn emit_suspended(e: &Env, member: Address, default_count: u32) {
    MemberSuspended {
        member,
        default_count,
    }
    .publish(e);
}

pub fn emit_mem_add(e: &Env, member: Address, member_count: u32) {
    MemberAdded {
        member,
        member_count,
    }
    .publish(e);
}

pub fn emit_mem_rmv(e: &Env, member: Address, member_count: u32) {
    MemberRemoved {
        member,
        member_count,
    }
    .publish(e);
}

pub fn emit_tok_add(e: &Env, token: Address) {
    TokenApproved { token }.publish(e);
}

pub fn emit_tok_rmv(e: &Env, token: Address) {
    TokenRemoved { token }.publish(e);
}

pub fn emit_rate_set(e: &Env, token: Address, rate: i128) {
    ExchangeRateSet { token, rate }.publish(e);
}

pub fn emit_lim_set(e: &Env, token: Address, limit: i128) {
    TokenLimitSet { token, limit }.publish(e);
}

pub fn emit_rew_dep(e: &Env, depositor: Address, amount: i128) {
    RewardDeposited { depositor, amount }.publish(e);
}

pub fn emit_rew_cfg(e: &Env, dist_type: DistributionType) {
    RewardConfigUpdated { dist_type }.publish(e);
}

pub fn emit_rew_clm(e: &Env, member: Address, amount: i128) {
    RewardClaimed { member, amount }.publish(e);
}

pub fn emit_prop_new(
    e: &Env,
    proposal_id: u32,
    creator: Address,
    target_member: Address,
    created_at: u64,
    deadline: u64,
) {
    ProposalCreated {
        proposal_id,
        creator,
        target_member,
        created_at,
        deadline,
    }
    .publish(e);
}

pub fn emit_voted(e: &Env, proposal_id: u32, voter: Address, vote_for: bool) {
    VoteCast {
        proposal_id,
        voter,
        vote_for,
    }
    .publish(e);
}

pub fn emit_prop_rej(
    e: &Env,
    proposal_id: u32,
    reason: Symbol,
    votes_for: u32,
    votes_against: u32,
) {
    ProposalRejected {
        proposal_id,
        reason,
        votes_for,
        votes_against,
    }
    .publish(e);
}

pub fn emit_prop_exec(e: &Env, proposal_id: u32, proposal_type: u32, target_member: Address) {
    ProposalExecuted {
        proposal_id,
        proposal_type,
        target_member,
    }
    .publish(e);
}

pub fn emit_appeal_ok(e: &Env, member: Address) {
    PenaltyAppealApproved { member }.publish(e);
}

pub fn emit_rule_upd(e: &Env, new_quorum: i128) {
    QuorumUpdated { new_quorum }.publish(e);
}

pub fn emit_mem_del(e: &Env, member: Address) {
    MemberRemovalExecuted { member }.publish(e);
}

pub fn emit_reminder(
    e: &Env,
    round: u32,
    time_remaining: u64,
    non_contributors: Vec<Address>,
    interval: Symbol,
) {
    DeadlineReminder {
        round,
        time_remaining,
        non_contributors,
        interval,
    }
    .publish(e);
}

pub fn emit_paused(e: &Env, reason: soroban_sdk::String) {
    ContractPaused { reason }.publish(e);
}

pub fn emit_resumed(e: &Env, reason: soroban_sdk::String) {
    ContractResumed { reason }.publish(e);
}

pub fn emit_exit_req(e: &Env, member: Address, round: u32, refund_amount: i128) {
    ExitRequested {
        member,
        round,
        refund_amount,
    }
    .publish(e);
}

pub fn emit_exit_ok(e: &Env, member: Address, refund_amount: i128) {
    ExitApproved {
        member,
        refund_amount,
    }
    .publish(e);
}

pub fn emit_exit_no(e: &Env, member: Address) {
    ExitRejected { member }.publish(e);
}

pub fn emit_rd_done(e: &Env, round: u32, recipient: Address, payout_amount: i128) {
    RoundCompleted {
        round,
        recipient,
        payout_amount,
    }
    .publish(e);
}

pub fn emit_reset(e: &Env, round: u32) {
    RoundReset { round }.publish(e);
}

pub fn emit_admin_transfer_proposed(e: &Env, current_admin: Address, proposed_admin: Address) {
    AdminTransferProposed {
        current_admin,
        proposed_admin,
    }
    .publish(e);
}

pub fn emit_admin_transferred(e: &Env, old_admin: Address, new_admin: Address) {
    AdminTransferred { old_admin, new_admin }.publish(e);
}

pub fn emit_contract_upgraded(e: &Env, old_version: u32, new_version: u32, by_admin: Address) {
    ContractUpgraded {
        old_version,
        new_version,
        by_admin,
    }
    .publish(e);
}
