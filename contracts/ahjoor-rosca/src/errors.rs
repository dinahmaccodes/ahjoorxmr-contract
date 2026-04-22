use soroban_sdk::contracterror;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyInitialized = 1,
    TokenNotApproved = 2,
    CustomOrderLengthMismatch = 3,
    CustomOrderNonMember = 4,
    AmountMustBePositive = 5,
    RoundDeadlinePassed = 6,
    MemberHasExited = 7,
    NotAMember = 8,
    AlreadyContributed = 9,
    InvalidExchangeRate = 10,
    ExceedsTokenLimit = 11,
    ExceedsRemainingContribution = 12,
    DeadlineNotPassed = 13,
    PenaltyDisabled = 14,
    NotADefaulter = 15,
    CannotChangeMidRound = 16,
    AlreadyAMember = 17,
    NoRewardsToClaim = 18,
    OnlyMembersAllowed = 19,
    ProposalNotFound = 20,
    VotingDeadlinePassed = 21,
    ProposalNotPending = 22,
    AlreadyVoted = 23,
    VotingNotEnded = 24,
    ContractPaused = 25,
    AllMembersSuspended = 26,
    AlreadyPaused = 27,
    NotPaused = 28,
    MemberAlreadyExited = 29,
    ExitRequestPending = 30,
    NoExitRequestFound = 31,
    ExitNotAllowedMidRound = 32,
    /// Contribution rejected because the round deadline has passed.
    ContributionWindowClosed = 33,
    /// Fee basis points exceeds maximum allowed (500 bps = 5%).
    FeeExceedsMaximum = 34,
    /// Max defaults must be at least 1.
    InvalidMaxDefaults = 35,
}
