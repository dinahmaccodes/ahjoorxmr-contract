use soroban_sdk::{contracttype, Address, Map, String, Vec};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum PayoutStrategy {
    RoundRobin = 0,
    AdminAssigned = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum DistributionType {
    Equal = 0,
    Proportional = 1,
    Weighted = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoscaConfig {
    pub strategy: PayoutStrategy,
    pub custom_order: Option<Vec<Address>>,
    pub penalty_amount: i128,
    pub exit_penalty_bps: u32,
    pub collective_goal: Option<i128>,
    pub member_goals: Option<Map<Address, i128>>,
    /// Protocol fee in basis points (e.g., 100 = 1%, 500 = 5%). Max 500 bps.
    pub fee_bps: u32,
    /// Address that receives protocol fees
    pub fee_recipient: Option<Address>,
    /// Number of consecutive missed rounds before suspension (default: 3)
    pub max_defaults: u32,
    pub use_timestamp_schedule: bool,
    pub round_duration_seconds: u64,
    pub max_members: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupInfo {
    pub members: Vec<Address>,
    pub contribution_amount: i128,
    pub token: Address,
    pub current_round: u32,
    pub total_rounds: u32,
    pub paid_members: Vec<Address>,
    pub next_recipient: Address,
    /// Timestamp (seconds) by which all contributions for the current round must be received.
    pub round_deadline: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayoutRecord {
    pub recipient: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitRequest {
    pub member: Address,
    pub rounds_contributed: u32,
    /// Computed dynamically in `approve_exit` from rounds_contributed, payout history, and
    /// exit_penalty_bps; not stored at request time.
    pub refund_amount: i128,
    pub approved: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemberStatus {
    pub is_member: bool,
    pub is_suspended: bool,
    pub is_exited: bool,
    pub contributions_this_round: i128,
    pub has_paid_this_round: bool,
    pub default_count: u32,
    pub lifetime_contributions: i128,
    pub claimable_rewards: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum ProposalType {
    PenaltyAppeal = 0,
    RuleChange = 1,
    MemberRemoval = 2,
    MaxMembersUpdate = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum ProposalStatus {
    Pending = 0,
    Approved = 1,
    Rejected = 2,
    Executed = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub proposal_type: ProposalType,
    pub creator: Address,
    pub description: String,
    pub target_member: Address,
    pub votes_for: u32,
    pub votes_against: u32,
    pub created_at: u64,
    pub deadline: u64,
    pub status: ProposalStatus,
    pub execution_data: Option<i128>,
}

/// Storage key classification:
///
/// INSTANCE (config + active round state — bounded, shared TTL):
///   Admin, Members, PayoutOrder, Strategy, ContributionAmt, Token,
///   CurrentRound, PaidMembers, RoundDuration, RoundDeadline, Defaulters,
///   PenaltyAmount, DefaultCount, SuspendedMembers, ApprovedTokens,
///   RewardPool, TotalParticipations, MemberParticipation, ClaimedRewards,
///   RewardWeights, RewardDistType, ExitedMembers, ExitPenaltyBps,
///   IsPaused, PauseReason, PauseTimestamp, CollectiveGoal, TotalCollected,
///   MemberGoals, MemberCollected, MilestonesReached, ExchangeRates,
///   TokenLimits, ProposalCounter, Proposals, ProposalVotes,
///   VotingDeadline, QuorumPercentage, MemberContributions
///
/// PERSISTENT (unbounded growth — individual TTL per key):
///   RoundHistory — appended every round; must outlive instance TTL
///
/// TEMPORARY (short-lived in-progress state — auto-expires):
///   ExitRequests — pending admin approval; no long-term retention needed
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    // --- Instance ---
    Admin,                   // Address
    Members,                 // Vec<Address>
    PayoutOrder,             // Vec<Address>
    Strategy,                // PayoutStrategy
    ContributionAmt,         // i128
    Token,                   // Address
    CurrentRound,            // u32
    PaidMembers,             // Vec<Address>
    RoundDuration,           // u64
    RoundDeadline,           // u64
    Defaulters,              // Vec<Address>
    PenaltyAmount,           // i128
    DefaultCount,            // Map<Address, u32>
    SuspendedMembers,        // Vec<Address>
    ApprovedTokens,          // Vec<Address>
    RewardPool,              // i128
    TotalParticipations,     // u32
    MemberParticipation,     // Map<Address, u32>
    ClaimedRewards,          // Map<Address, i128>
    RewardWeights,           // Map<Address, u32>
    RewardDistType,          // DistributionType
    ExitedMembers,           // Vec<Address>
    ExitPenaltyBps,          // u32 (basis points, e.g. 1000 = 10%)
    Paused,                  // bool (global pause alias)
    IsPaused,                // bool
    PauseReason,             // String
    PauseTimestamp,          // u64
    CollectiveGoal,          // i128
    TotalCollected,          // i128
    MemberGoals,             // Map<Address, i128>
    MemberCollected,         // Map<Address, i128>
    MilestonesReached,       // Vec<u32> (e.g. 25, 50, 75, 100)
    ExchangeRates,           // Map<Address, i128>
    TokenLimits,             // Map<Address, i128>
    ProposalCounter,         // u32
    Proposals,               // Map<u32, Proposal>
    ProposalVotes,           // Map<u32, Map<Address, bool>>
    VotingDeadline,          // u64
    QuorumPercentage,        // u32 (e.g., 51 for 51%)
    MemberContributions,     // Map<Address, i128> cumulative per round
    ProposedAdmin,           // Address — proposed new admin (pending acceptance)
    ContractVersion,         // u32
    FeeBps,                  // u32 — protocol fee in basis points
    FeeRecipient,            // Address — receives protocol fees
    MaxDefaults,             // u32 — suspension threshold (consecutive missed rounds)
    UseTimestampSchedule,   // bool
    RoundDurationSeconds,   // u64
    RoundDeadlineTimestamp, // u64
    MaxMembers,             // u32
    // --- Persistent ---
    RoundHistory, // Vec<PayoutRecord> — grows every round
    // --- Temporary ---
    ExitRequests, // Map<Address, ExitRequest> — pending admin action
}
