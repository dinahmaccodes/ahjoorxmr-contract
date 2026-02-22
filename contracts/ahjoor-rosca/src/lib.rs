#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, Map, Symbol, Vec,
};

const INSTANCE_LIFETIME_THRESHOLD: u32 = 100_000;
const INSTANCE_BUMP_AMOUNT: u32 = 120_000;

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
    pub penalty_amount: i128,
    pub refund_amount: i128,
    pub approved: bool,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,               // Address
    Members,             // Vec<Address>
    PayoutOrder,         // Vec<Address>
    Strategy,            // PayoutStrategy
    ContributionAmt,     // i128
    Token,               // Address
    CurrentRound,        // u32
    PaidMembers,         // Vec<Address>
    RoundDuration,       // u64
    RoundDeadline,       // u64
    Defaulters,          // Vec<Address>
    PenaltyAmount,       // i128
    DefaultCount,        // Map<Address, u32>
    SuspendedMembers,    // Vec<Address>
    RoundHistory,        // Vec<PayoutRecord>
    ApprovedTokens,      // Vec<Address>
    RewardPool,          // i128
    TotalParticipations, // u32
    MemberParticipation, // Map<Address, u32>
    ClaimedRewards,      // Map<Address, i128>
    RewardWeights,       // Map<Address, u32>
    RewardDistType,      // DistributionType
    ExitRequests,        // Map<Address, ExitRequest>
    ExitedMembers,       // Vec<Address>
    ExitPenaltyBps,      // u32 (basis points, e.g. 1000 = 10%)
    IsPaused,            // bool
    PauseReason,         // String
    PauseTimestamp,      // u64
    CollectiveGoal,      // i128
    TotalCollected,      // i128
    MemberGoals,         // Map<Address, i128>
    MemberCollected,     // Map<Address, i128>
    MilestonesReached,   // Vec<u32> (e.g. 25, 50, 75, 100)
    ExchangeRates,       // Map<Address, i128>
    TokenLimits,         // Map<Address, i128>
    MemberContributions, // Map<Address, i128>  cumulative per round
}

#[contract]
pub struct AhjoorContract;

#[contractimpl]
impl AhjoorContract {
    pub fn init(
        env: Env,
        admin: Address,
        members: Vec<Address>,
        contribution_amount: i128,
        token: Address,
        round_duration: u64,
        config: RoscaConfig,
    ) {
        if env.storage().instance().has(&DataKey::Members) {
            panic!("Already initialized");
        }

        let approved_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(&env));

        if !approved_tokens.is_empty() && !approved_tokens.contains(&token) {
            panic!("Token not approved");
        }

        let resolved_order = match config.strategy {
            PayoutStrategy::RoundRobin => members.clone(),
            PayoutStrategy::AdminAssigned => {
                let order = config.custom_order.expect("AdminAssigned strategy requires a custom order");
                if order.len() != members.len() {
                    panic!("Custom order length mismatch");
                }
                for member in order.iter() {
                    if !members.contains(&member) {
                        panic!("Custom order contains non-member address");
                    }
                }
                order
            }
        };

        let start_time = env.ledger().timestamp();
        let deadline = start_time + round_duration;
        let member_count = members.len();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Members, &members);
        env.storage()
            .instance()
            .set(&DataKey::PayoutOrder, &resolved_order);
        env.storage().instance().set(&DataKey::Strategy, &config.strategy);
        env.storage()
            .instance()
            .set(&DataKey::ContributionAmt, &contribution_amount);
        env.storage().instance().set(&DataKey::Token, &token);

        // Auto-approve the base token
        let mut approved_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(&env));
        if !approved_tokens.contains(&token) {
            approved_tokens.push_back(token.clone());
            env.storage()
                .instance()
                .set(&DataKey::ApprovedTokens, &approved_tokens);
        }

        env.storage().instance().set(&DataKey::CurrentRound, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::PaidMembers, &Vec::<Address>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RoundDuration, &round_duration);
        env.storage()
            .instance()
            .set(&DataKey::RoundDeadline, &deadline);
        env.storage()
            .instance()
            .set(&DataKey::Defaulters, &Vec::<Address>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::PenaltyAmount, &config.penalty_amount);
        env.storage()
            .instance()
            .set(&DataKey::DefaultCount, &Map::<Address, u32>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::SuspendedMembers, &Vec::<Address>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RoundHistory, &Vec::<PayoutRecord>::new(&env));
        env.storage().instance().set(&DataKey::RewardPool, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalParticipations, &0u32);
        env.storage().instance().set(
            &DataKey::MemberParticipation,
            &Map::<Address, u32>::new(&env),
        );
        env.storage()
            .instance()
            .set(&DataKey::ClaimedRewards, &Map::<Address, i128>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RewardWeights, &Map::<Address, u32>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RewardDistType, &DistributionType::Equal);

        env.storage()
            .instance()
            .set(&DataKey::ExitPenaltyBps, &config.exit_penalty_bps);
        env.storage().instance().set(
            &DataKey::ExitRequests,
            &Map::<Address, ExitRequest>::new(&env),
        );
        env.storage()
            .instance()
            .set(&DataKey::ExitedMembers, &Vec::<Address>::new(&env));
        env.storage().instance().set(&DataKey::IsPaused, &false);
        env.storage().instance().set(
            &DataKey::MemberContributions,
            &Map::<Address, i128>::new(&env),
        );

        // Savings Goal Initialization
        if let Some(goal) = config.collective_goal {
            env.storage().instance().set(&DataKey::CollectiveGoal, &goal);
        }
        if let Some(goals) = config.member_goals {
            env.storage().instance().set(&DataKey::MemberGoals, &goals);
        }
        env.storage().instance().set(&DataKey::TotalCollected, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::MemberCollected, &Map::<Address, i128>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::MilestonesReached, &Vec::<u32>::new(&env));

        env.events().publish(
            (symbol_short!("init"),),
            (member_count, contribution_amount),
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn contribute(env: Env, contributor: Address, token: Address, amount: i128) {
        Self::check_not_paused(&env);
        contributor.require_auth();

        if amount <= 0 {
            panic!("Contribution amount must be positive");
        }

        let deadline: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RoundDeadline)
            .expect("Deadline not set");
        if env.ledger().timestamp() > deadline {
            panic!("Contribution failed: Round deadline has passed");
        }

        let exited_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ExitedMembers)
            .unwrap_or(Vec::new(&env));
        if exited_members.contains(&contributor) {
            panic!("Member has exited");
        }

        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .expect("Not initialized");
        if !members.contains(&contributor) {
            panic!("Not a member");
        }

        let mut paid_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaidMembers)
            .expect("Not initialized");
        if paid_members.contains(&contributor) {
            panic!("Already contributed full amount for this round");
        }

        // Validate token
        let approved_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(&env));
        if !approved_tokens.contains(&token) {
            panic!("Token not approved");
        }

        let base_token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let base_amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ContributionAmt)
            .unwrap();

        let amount_to_transfer = if token == base_token {
            base_amount
        } else {
            let rates: Map<Address, i128> = env
                .storage()
                .instance()
                .get(&DataKey::ExchangeRates)
                .unwrap_or(Map::new(&env));
            let rate = rates.get(token.clone()).expect("Exchange rate not set");
            if rate <= 0 {
                panic!("Invalid exchange rate");
            }
            // Valuation logic: RequiredAmount = (BaseAmount * 10^7) / Rate
            // Rate is expected to be in 10^7 precision (e.g., 1.5 * 10^7 = 15,000,000)
            (base_amount * 10_000_000) / rate
        };

        // Check token-specific limits
        let limits: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::TokenLimits)
            .unwrap_or(Map::new(&env));
        if let Some(limit) = limits.get(token.clone()) {
            if amount_to_transfer > limit {
                panic!("Contribution exceeds token limit");
            }
        }

        let client = token::Client::new(&env, &token);
        client.transfer(&contributor, &env.current_contract_address(), &amount_to_transfer);

        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap_or(0);

        // Load (and update) cumulative contributions for this round
        let mut member_contributions: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::MemberContributions)
            .unwrap_or(Map::new(&env));
        let already_paid: i128 = member_contributions.get(contributor.clone()).unwrap_or(0);
        let remaining = base_amount - already_paid;

        if amount > remaining {
            panic!("Amount exceeds remaining contribution");
        }

        let new_total = already_paid + amount;
        member_contributions.set(contributor.clone(), new_total);
        env.storage()
            .instance()
            .set(&DataKey::MemberContributions, &member_contributions);


        env.events().publish(
            (symbol_short!("contrib"), contributor.clone(), current_round),
            (token, amount_to_transfer),
        );

        // Only mark as fully paid (and track participation) when target is reached
        if new_total == target {
            paid_members.push_back(contributor.clone());
            env.storage()
                .instance()
                .set(&DataKey::PaidMembers, &paid_members);

            // Track reward participation
            let mut total_participations: u32 = env
                .storage()
                .instance()
                .get(&DataKey::TotalParticipations)
                .unwrap_or(0);
            let mut member_participation: Map<Address, u32> = env
                .storage()
                .instance()
                .get(&DataKey::MemberParticipation)
                .unwrap_or(Map::new(&env));

            let current_participation =
                member_participation.get(contributor.clone()).unwrap_or(0);
            member_participation.set(contributor.clone(), current_participation + 1);
            total_participations += 1;

            env.storage()
                .instance()
                .set(&DataKey::TotalParticipations, &total_participations);
            env.storage()
                .instance()
                .set(&DataKey::MemberParticipation, &member_participation);

            // Only trigger payout when all members have fully contributed
            if new_total == base_amount && paid_members.len() == members.len() {
                Self::complete_round_payout(&env, &paid_members);
            }

            // Savings Goal Progress Tracking
            let mut total_collected: i128 = env
                .storage()
                .instance()
                .get(&DataKey::TotalCollected)
                .unwrap_or(0);
            total_collected += amount;
            env.storage()
                .instance()
                .set(&DataKey::TotalCollected, &total_collected);

            let mut member_collected: Map<Address, i128> = env
                .storage()
                .instance()
                .get(&DataKey::MemberCollected)
                .unwrap_or(Map::new(&env));
            let m_collected = member_collected.get(contributor.clone()).unwrap_or(0) + amount;
            member_collected.set(contributor.clone(), m_collected);
            env.storage()
                .instance()
                .set(&DataKey::MemberCollected, &member_collected);

            // Milestone Detection
            if let Some(collective_goal) = env
                .storage()
                .instance()
                .get::<_, i128>(&DataKey::CollectiveGoal)
            {
                let mut milestones_reached: Vec<u32> = env
                    .storage()
                    .instance()
                    .get(&DataKey::MilestonesReached)
                    .unwrap_or(Vec::new(&env));

                let progress_bps = (total_collected * 10000i128) / collective_goal;
                let thresholds: [u32; 4] = [2500u32, 5000u32, 7500u32, 10000u32];
                let milestone_names: [u32; 4] = [25u32, 50u32, 75u32, 100u32];

                for i in 0..4 {
                    let threshold = thresholds[i];
                    let milestone = milestone_names[i];
                    if progress_bps >= threshold as i128 && !milestones_reached.contains(&milestone) {
                        milestones_reached.push_back(milestone);
                        env.events().publish(
                            (symbol_short!("milestone"), milestone),
                            total_collected,
                        );
                    }
                }
                env.storage()
                    .instance()
                    .set(&DataKey::MilestonesReached, &milestones_reached);
        }

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn close_round(env: Env) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let deadline: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RoundDeadline)
            .unwrap();
        if env.ledger().timestamp() <= deadline {
            panic!("Cannot close: Deadline has not passed yet");
        }

        let members: Vec<Address> = env.storage().instance().get(&DataKey::Members).unwrap();
        let paid_members: Vec<Address> =
            env.storage().instance().get(&DataKey::PaidMembers).unwrap();
        let exited_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ExitedMembers)
            .unwrap_or(Vec::new(&env));

        let mut defaulters = Vec::new(&env);
        for member in members.iter() {
            if !paid_members.contains(&member) && !exited_members.contains(&member) {
                defaulters.push_back(member);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::Defaulters, &defaulters);

        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap();
        env.events()
            .publish((symbol_short!("closed"), current_round), defaulters);

        Self::reset_round_state(&env, current_round);
    }

    pub fn penalise_defaulter(env: Env, member: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let penalty_amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::PenaltyAmount)
            .unwrap_or(0);
        if penalty_amount == 0 {
            panic!("Penalty system is disabled");
        }

        let defaulters: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Defaulters)
            .unwrap_or(Vec::new(&env));
        if !defaulters.contains(&member) {
            panic!("Member is not a defaulter for this round");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);

        member.require_auth();
        client.transfer(&member, &env.current_contract_address(), &penalty_amount);

        let mut default_count: Map<Address, u32> = env
            .storage()
            .instance()
            .get(&DataKey::DefaultCount)
            .unwrap_or(Map::new(&env));
        let current_defaults = default_count.get(member.clone()).unwrap_or(0);
        let new_default_count = current_defaults + 1;
        default_count.set(member.clone(), new_default_count);
        env.storage()
            .instance()
            .set(&DataKey::DefaultCount, &default_count);

        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap();
        env.events().publish(
            (symbol_short!("defaulted"), member.clone(), current_round),
            (penalty_amount, new_default_count),
        );

        if new_default_count >= 2 {
            let mut suspended_members: Vec<Address> = env
                .storage()
                .instance()
                .get(&DataKey::SuspendedMembers)
                .unwrap_or(Vec::new(&env));
            if !suspended_members.contains(&member) {
                suspended_members.push_back(member.clone());
                env.storage()
                    .instance()
                    .set(&DataKey::SuspendedMembers, &suspended_members);
                env.events().publish(
                    (symbol_short!("suspended"), member.clone()),
                    new_default_count,
                );
            }
        }
    }

    pub fn add_member(env: Env, new_member: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        // Reject mid-round: paid_members must be empty
        let paid_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaidMembers)
            .unwrap_or(Vec::new(&env));
        if !paid_members.is_empty() {
            panic!("Cannot change members mid-round");
        }

        let mut members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .expect("Not initialized");
        if members.contains(&new_member) {
            panic!("Already a member");
        }
        members.push_back(new_member.clone());
        env.storage().instance().set(&DataKey::Members, &members);

        // Recalculate payout order: append new member to the end
        let mut payout_order: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PayoutOrder)
            .expect("Payout order not set");
        payout_order.push_back(new_member.clone());
        env.storage()
            .instance()
            .set(&DataKey::PayoutOrder, &payout_order);

        env.events()
            .publish((symbol_short!("mem_add"), new_member), members.len());
    }

    pub fn remove_member(env: Env, member: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        // Reject mid-round: paid_members must be empty
        let paid_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaidMembers)
            .unwrap_or(Vec::new(&env));
        if !paid_members.is_empty() {
            panic!("Cannot change members mid-round");
        }

        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .expect("Not initialized");
        if !members.contains(&member) {
            panic!("Not a member");
        }

        // Remove from members list
        let mut new_members: Vec<Address> = Vec::new(&env);
        for m in members.iter() {
            if m != member {
                new_members.push_back(m);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::Members, &new_members);

        // Recalculate payout order: filter out the member
        let old_order: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PayoutOrder)
            .expect("Payout order not set");
        let mut new_order: Vec<Address> = Vec::new(&env);
        for m in old_order.iter() {
            if m != member {
                new_order.push_back(m);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::PayoutOrder, &new_order);

        env.events()
            .publish((symbol_short!("mem_rmv"), member), new_members.len());
    }

    pub fn add_approved_token(env: Env, token: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let mut approved_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(&env));

        if !approved_tokens.contains(&token) {
            approved_tokens.push_back(token.clone());
            env.storage()
                .instance()
                .set(&DataKey::ApprovedTokens, &approved_tokens);
            env.events().publish((symbol_short!("tok_add"),), token);
        }
    }

    pub fn remove_approved_token(env: Env, token: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let approved_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(&env));

        if approved_tokens.contains(&token) {
            let mut new_approved_tokens: Vec<Address> = Vec::new(&env);
            for t in approved_tokens.iter() {
                if t != token {
                    new_approved_tokens.push_back(t);
                }
            }
            env.storage()
                .instance()
                .set(&DataKey::ApprovedTokens, &new_approved_tokens);
            env.events().publish((symbol_short!("tok_rmv"),), token);
        }
    }

    pub fn set_exchange_rate(env: Env, token: Address, rate: i128) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let mut rates: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::ExchangeRates)
            .unwrap_or(Map::new(&env));

        rates.set(token.clone(), rate);
        env.storage().instance().set(&DataKey::ExchangeRates, &rates);
        env.events().publish((symbol_short!("rate_set"),), (token, rate));
    }

    pub fn set_token_limit(env: Env, token: Address, limit: i128) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let mut limits: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::TokenLimits)
            .unwrap_or(Map::new(&env));

        limits.set(token.clone(), limit);
        env.storage().instance().set(&DataKey::TokenLimits, &limits);
        env.events().publish((symbol_short!("lim_set"),), (token, limit));
    }

    pub fn bump_storage(env: Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn deposit_rewards(env: Env, depositor: Address, amount: i128) {
        Self::check_not_paused(&env);
        depositor.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);

        client.transfer(&depositor, &env.current_contract_address(), &amount);

        let mut reward_pool: i128 = env
            .storage()
            .instance()
            .get(&DataKey::RewardPool)
            .unwrap_or(0);
        reward_pool += amount;
        env.storage()
            .instance()
            .set(&DataKey::RewardPool, &reward_pool);

        env.events()
            .publish((symbol_short!("rew_dep"), depositor), amount);
    }

    pub fn set_reward_dist_params(
        env: Env,
        dist_type: DistributionType,
        weights: Option<Map<Address, u32>>,
    ) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::RewardDistType, &dist_type);

        if let Some(w) = weights {
            env.storage().instance().set(&DataKey::RewardWeights, &w);
        }

        env.events().publish((symbol_short!("rew_cfg"),), dist_type);
    }

    pub fn claim_rewards(env: Env, member: Address) {
        Self::check_not_paused(&env);
        member.require_auth();

        let claimable = Self::get_claimable_reward(env.clone(), member.clone());
        if claimable <= 0 {
            panic!("No rewards to claim");
        }

        let mut claimed_rewards: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::ClaimedRewards)
            .unwrap_or(Map::new(&env));
        let total_claimed = claimed_rewards.get(member.clone()).unwrap_or(0);
        claimed_rewards.set(member.clone(), total_claimed + claimable);
        env.storage()
            .instance()
            .set(&DataKey::ClaimedRewards, &claimed_rewards);

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);

        client.transfer(&env.current_contract_address(), &member, &claimable);

        env.events()
            .publish((symbol_short!("rew_clm"), member), claimable);
    }

    pub fn get_claimable_reward(env: Env, member: Address) -> i128 {
        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .expect("Not initialized");
        if !members.contains(&member) {
            return 0;
        }

        let reward_pool: i128 = env
            .storage()
            .instance()
            .get(&DataKey::RewardPool)
            .unwrap_or(0);
        if reward_pool == 0 {
            return 0;
        }

        let dist_type: DistributionType = env
            .storage()
            .instance()
            .get(&DataKey::RewardDistType)
            .unwrap_or(DistributionType::Equal);

        let share = match dist_type {
            DistributionType::Equal => reward_pool / (members.len() as i128),
            DistributionType::Proportional => {
                let total_participations: u32 = env
                    .storage()
                    .instance()
                    .get(&DataKey::TotalParticipations)
                    .unwrap_or(0);
                if total_participations == 0 {
                    0
                } else {
                    let member_participation: Map<Address, u32> = env
                        .storage()
                        .instance()
                        .get(&DataKey::MemberParticipation)
                        .unwrap_or(Map::new(&env));
                    let count = member_participation.get(member.clone()).unwrap_or(0);
                    (reward_pool * (count as i128)) / (total_participations as i128)
                }
            }
            DistributionType::Weighted => {
                let weights: Map<Address, u32> = env
                    .storage()
                    .instance()
                    .get(&DataKey::RewardWeights)
                    .unwrap_or(Map::new(&env));
                let total_weight: u32 = {
                    let mut sum = 0u32;
                    for w in weights.values().iter() {
                        sum += w;
                    }
                    sum
                };
                if total_weight == 0 {
                    reward_pool / (members.len() as i128) // Fallback to equal
                } else {
                    let weight = weights.get(member.clone()).unwrap_or(0);
                    (reward_pool * (weight as i128)) / (total_weight as i128)
                }
            }
        };

        let claimed_rewards: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::ClaimedRewards)
            .unwrap_or(Map::new(&env));
        let already_claimed = claimed_rewards.get(member).unwrap_or(0);

        share - already_claimed
    }

    // --- READ INTERFACE ---

    pub fn get_group_info(env: Env) -> GroupInfo {
        let members: Vec<Address> = env.storage().instance().get(&DataKey::Members).unwrap();
        let payout_order: Vec<Address> =
            env.storage().instance().get(&DataKey::PayoutOrder).unwrap();
        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap_or(0);

        let recipient_idx = (current_round % payout_order.len()) as u32;
        let next_recipient = payout_order.get(recipient_idx).unwrap();

        GroupInfo {
            members,
            contribution_amount: env
                .storage()
                .instance()
                .get(&DataKey::ContributionAmt)
                .unwrap_or(0),
            token: env.storage().instance().get(&DataKey::Token).unwrap(),
            current_round,
            total_rounds: payout_order.len(),
            paid_members: env
                .storage()
                .instance()
                .get(&DataKey::PaidMembers)
                .unwrap_or(Vec::new(&env)),
            next_recipient,
        }
    }

    pub fn get_member_status(env: Env, member: Address) -> bool {
        let paid_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaidMembers)
            .unwrap_or(Vec::new(&env));
        paid_members.contains(&member)
    }

    /// Returns `(amount_contributed_so_far, amount_remaining)` for `member`
    /// in the current round.
    pub fn get_member_contribution_status(env: Env, member: Address) -> (i128, i128) {
        let target: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ContributionAmt)
            .unwrap_or(0);
        let member_contributions: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&DataKey::MemberContributions)
            .unwrap_or(Map::new(&env));
        let contributed = member_contributions.get(member).unwrap_or(0);
        let remaining = target - contributed;
        (contributed, remaining)
    }

    pub fn get_round_history(env: Env) -> Vec<PayoutRecord> {
        env.storage()
            .instance()
            .get(&DataKey::RoundHistory)
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_state(env: Env) -> (u32, Vec<Address>, u64, PayoutStrategy, Address) {
        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap_or(0);
        let paid_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaidMembers)
            .unwrap_or(Vec::new(&env));
        let deadline: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RoundDeadline)
            .unwrap_or(0);
        let strategy: PayoutStrategy = env
            .storage()
            .instance()
            .get(&DataKey::Strategy)
            .unwrap_or(PayoutStrategy::RoundRobin);
        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        (current_round, paid_members, deadline, strategy, token)
    }

    pub fn get_savings_progress(
        env: Env,
        member: Option<Address>,
    ) -> (i128, i128, i128, i128) {
        let total_collected = env
            .storage()
            .instance()
            .get(&DataKey::TotalCollected)
            .unwrap_or(0);
        let collective_goal = env
            .storage()
            .instance()
            .get(&DataKey::CollectiveGoal)
            .unwrap_or(0);

        let (member_collected, member_goal) = if let Some(m) = member {
            let m_collected = env
                .storage()
                .instance()
                .get::<_, Map<Address, i128>>(&DataKey::MemberCollected)
                .unwrap_or(Map::new(&env))
                .get(m.clone())
                .unwrap_or(0);
            let m_goal = env
                .storage()
                .instance()
                .get::<_, Map<Address, i128>>(&DataKey::MemberGoals)
                .unwrap_or(Map::new(&env))
                .get(m)
                .unwrap_or(0);
            (m_collected, m_goal)
        } else {
            (0, 0)
        };

        (total_collected, collective_goal, member_collected, member_goal)
    }

    pub fn get_exchange_rates(env: Env) -> Map<Address, i128> {
        env.storage()
            .instance()
            .get(&DataKey::ExchangeRates)
            .unwrap_or(Map::new(&env))
    }

    pub fn get_token_limits(env: Env) -> Map<Address, i128> {
        env.storage()
            .instance()
            .get(&DataKey::TokenLimits)
            .unwrap_or(Map::new(&env))
    }

    pub fn get_approved_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(&env))
    }

    // --- EMERGENCY EXIT ---

    pub fn pause_group(env: Env, reason: soroban_sdk::String) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        if Self::is_paused(env.clone()) {
            panic!("Group is already paused");
        }

        env.storage().instance().set(&DataKey::IsPaused, &true);
        env.storage().instance().set(&DataKey::PauseReason, &reason);
        env.storage()
            .instance()
            .set(&DataKey::PauseTimestamp, &env.ledger().timestamp());

        env.events().publish((symbol_short!("paused"),), reason);
    }

    pub fn resume_group(env: Env, reason: soroban_sdk::String) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        if !Self::is_paused(env.clone()) {
            panic!("Group is not paused");
        }

        let pause_timestamp: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PauseTimestamp)
            .unwrap();
        let current_timestamp = env.ledger().timestamp();
        let pause_duration = current_timestamp - pause_timestamp;

        // Extend the round deadline
        let current_deadline: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RoundDeadline)
            .unwrap_or(0);
        if current_deadline > 0 {
            env.storage().instance().set(
                &DataKey::RoundDeadline,
                &(current_deadline + pause_duration),
            );
        }

        env.storage().instance().set(&DataKey::IsPaused, &false);

        // Clean up Reason and Timestamp to save storage space
        env.storage().instance().remove(&DataKey::PauseReason);
        env.storage().instance().remove(&DataKey::PauseTimestamp);

        env.events().publish((symbol_short!("resumed"),), reason);
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::IsPaused)
            .unwrap_or(false)
    }

    pub fn get_pause_info(env: Env) -> (bool, soroban_sdk::String, u64) {
        let is_paused = Self::is_paused(env.clone());
        let reason: soroban_sdk::String = env
            .storage()
            .instance()
            .get(&DataKey::PauseReason)
            .unwrap_or(soroban_sdk::String::from_str(&env, ""));
        let timestamp: u64 = env
            .storage()
            .instance()
            .get(&DataKey::PauseTimestamp)
            .unwrap_or(0);
        (is_paused, reason, timestamp)
    }

    pub fn request_emergency_exit(env: Env, member: Address) {
        Self::check_not_paused(&env);
        member.require_auth();

        let exited_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ExitedMembers)
            .unwrap_or(Vec::new(&env));
        if exited_members.contains(&member) {
            panic!("Member already exited");
        }

        let members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .expect("Not initialized");
        if !members.contains(&member) {
            panic!("Not a member");
        }

        // Prevent exit mid-round
        let paid_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::PaidMembers)
            .unwrap_or(Vec::new(&env));
        if !paid_members.is_empty() {
            panic!("Cannot request exit mid-round");
        }

        // Check no existing pending request
        let mut requests: Map<Address, ExitRequest> = env
            .storage()
            .instance()
            .get(&DataKey::ExitRequests)
            .unwrap_or(Map::new(&env));
        if requests.contains_key(member.clone()) {
            panic!("Exit request already pending");
        }

        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap_or(0);
        let contribution_amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ContributionAmt)
            .unwrap_or(0);
        let exit_penalty_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::ExitPenaltyBps)
            .unwrap_or(0);

        let total_contributed = contribution_amount * (current_round as i128);
        let penalty_amount = total_contributed * (exit_penalty_bps as i128) / 10_000;
        let refund_amount = total_contributed - penalty_amount;

        let request = ExitRequest {
            member: member.clone(),
            rounds_contributed: current_round,
            penalty_amount,
            refund_amount,
            approved: false,
        };
        requests.set(member.clone(), request);
        env.storage()
            .instance()
            .set(&DataKey::ExitRequests, &requests);

        env.events().publish(
            (symbol_short!("exit_req"), member.clone()),
            (current_round, refund_amount),
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn approve_exit(env: Env, member: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let mut requests: Map<Address, ExitRequest> = env
            .storage()
            .instance()
            .get(&DataKey::ExitRequests)
            .unwrap_or(Map::new(&env));
        if !requests.contains_key(member.clone()) {
            panic!("No exit request found for member");
        }
        let request = requests.get(member.clone()).unwrap();

        if request.refund_amount > 0 {
            let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            let client = token::Client::new(&env, &token_addr);
            client.transfer(
                &env.current_contract_address(),
                &member,
                &request.refund_amount,
            );
        }

        // Remove from Members list
        let old_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Members)
            .unwrap_or(Vec::new(&env));
        let mut new_members: Vec<Address> = Vec::new(&env);
        for m in old_members.iter() {
            if m != member {
                new_members.push_back(m);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::Members, &new_members);

        // Add to ExitedMembers
        let mut exited_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ExitedMembers)
            .unwrap_or(Vec::new(&env));
        exited_members.push_back(member.clone());
        env.storage()
            .instance()
            .set(&DataKey::ExitedMembers, &exited_members);

        requests.remove(member.clone());
        env.storage()
            .instance()
            .set(&DataKey::ExitRequests, &requests);

        env.events().publish(
            (symbol_short!("exit_ok"), member.clone()),
            request.refund_amount,
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn reject_exit(env: Env, member: Address) {
        Self::check_not_paused(&env);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not set");
        admin.require_auth();

        let mut requests: Map<Address, ExitRequest> = env
            .storage()
            .instance()
            .get(&DataKey::ExitRequests)
            .unwrap_or(Map::new(&env));
        if !requests.contains_key(member.clone()) {
            panic!("No exit request found for member");
        }

        requests.remove(member.clone());
        env.storage()
            .instance()
            .set(&DataKey::ExitRequests, &requests);

        env.events()
            .publish((symbol_short!("exit_no"), member.clone()), ());

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn get_exit_requests(env: Env) -> Map<Address, ExitRequest> {
        env.storage()
            .instance()
            .get(&DataKey::ExitRequests)
            .unwrap_or(Map::new(&env))
    }

    pub fn get_exited_members(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::ExitedMembers)
            .unwrap_or(Vec::new(&env))
    }

    // --- INTERNAL HELPERS ---

    fn check_not_paused(env: &Env) {
        let is_paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::IsPaused)
            .unwrap_or(false);
        if is_paused {
            panic!("Action blocked: Group is paused");
        }
    }

    fn complete_round_payout(env: &Env, _paid_members: &Vec<Address>) {
        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap();
        let payout_order: Vec<Address> =
            env.storage().instance().get(&DataKey::PayoutOrder).unwrap();
        let suspended_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SuspendedMembers)
            .unwrap_or(Vec::new(env));
        let exited_members: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ExitedMembers)
            .unwrap_or(Vec::new(env));

        let mut recipient_idx = (current_round % payout_order.len()) as u32;
        let mut attempts = 0;
        while attempts < payout_order.len() {
            let potential_recipient = payout_order.get(recipient_idx).unwrap();
            if !suspended_members.contains(&potential_recipient)
                && !exited_members.contains(&potential_recipient)
            {
                break;
            }
            recipient_idx = (recipient_idx + 1) % payout_order.len();
            attempts += 1;
        }

        if attempts >= payout_order.len() {
            panic!("All members are suspended");
        }

        let payout_recipient = payout_order.get(recipient_idx).unwrap();
        let reward_pool: i128 = env
            .storage()
            .instance()
            .get(&DataKey::RewardPool)
            .unwrap_or(0);
        let base_token: Address = env.storage().instance().get(&DataKey::Token).unwrap();

        let approved_tokens: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ApprovedTokens)
            .unwrap_or(Vec::new(env));

        let mut total_payout_history_amt = 0i128;

        for token_addr in approved_tokens.iter() {
            let client = token::Client::new(env, &token_addr);
            let mut balance = client.balance(&env.current_contract_address());

            if token_addr == base_token {
                balance -= reward_pool;
                total_payout_history_amt = balance;
            }

            if balance > 0 {
                client.transfer(
                    &env.current_contract_address(),
                    &payout_recipient,
                    &balance,
                );
            }
        }

        let mut history: Vec<PayoutRecord> = env
            .storage()
            .instance()
            .get(&DataKey::RoundHistory)
            .unwrap_or(Vec::new(env));
        history.push_back(PayoutRecord {
            recipient: payout_recipient.clone(),
            amount: total_payout_history_amt,
        });
        env.storage()
            .instance()
            .set(&DataKey::RoundHistory, &history);

        env.events().publish(
            (symbol_short!("rd_done"), current_round),
            (payout_recipient, total_payout_history_amt),
        );
        Self::reset_round_state(env, current_round);
    }

    fn reset_round_state(env: &Env, current_round: u32) {
        let duration: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RoundDuration)
            .unwrap();
        env.storage()
            .instance()
            .set(&DataKey::CurrentRound, &(current_round + 1));
        env.storage()
            .instance()
            .set(&DataKey::PaidMembers, &Vec::<Address>::new(env));
        env.storage().instance().set(
            &DataKey::MemberContributions,
            &Map::<Address, i128>::new(env),
        );
        env.storage().instance().set(
            &DataKey::RoundDeadline,
            &(env.ledger().timestamp() + duration),
        );
        env.events()
            .publish((symbol_short!("reset"),), current_round);
    }
}
mod test;