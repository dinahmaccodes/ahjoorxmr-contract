#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, Map, Vec,
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

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Admin,            // Address
    Members,          // Vec<Address>
    PayoutOrder,      // Vec<Address>
    Strategy,         // PayoutStrategy
    ContributionAmt,  // i128
    Token,            // Address
    CurrentRound,     // u32
    PaidMembers,      // Vec<Address>
    RoundDuration,    // u64
    RoundDeadline,    // u64
    Defaulters,       // Vec<Address>
    PenaltyAmount,    // i128
    DefaultCount,     // Map<Address, u32>
    SuspendedMembers, // Vec<Address>
    RoundHistory,     // Vec<PayoutRecord>
    ApprovedTokens,   // Vec<Address>
    RewardPool,         // i128
    TotalParticipations, // u32
    MemberParticipation, // Map<Address, u32>
    ClaimedRewards,      // Map<Address, i128>
    RewardWeights,       // Map<Address, u32>
    RewardDistType,      // DistributionType
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
        strategy: PayoutStrategy,
        custom_order: Option<Vec<Address>>,
        penalty_amount: i128,
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

        let resolved_order = match strategy {
            PayoutStrategy::RoundRobin => members.clone(),
            PayoutStrategy::AdminAssigned => {
                let order = custom_order.expect("AdminAssigned strategy requires a custom order");
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
        env.storage().instance().set(&DataKey::Strategy, &strategy);
        env.storage()
            .instance()
            .set(&DataKey::ContributionAmt, &contribution_amount);
        env.storage().instance().set(&DataKey::Token, &token);
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
            .set(&DataKey::PenaltyAmount, &penalty_amount);
        env.storage()
            .instance()
            .set(&DataKey::DefaultCount, &Map::<Address, u32>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RoundHistory, &Vec::<PayoutRecord>::new(&env));
        env.storage().instance().set(&DataKey::RewardPool, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TotalParticipations, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::MemberParticipation, &Map::<Address, u32>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::ClaimedRewards, &Map::<Address, i128>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RewardWeights, &Map::<Address, u32>::new(&env));
        env.storage()
            .instance()
            .set(&DataKey::RewardDistType, &DistributionType::Equal);

        env.events().publish(
            (symbol_short!("init"),),
            (member_count, contribution_amount),
        );

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn contribute(env: Env, contributor: Address) {
        contributor.require_auth();

        let deadline: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RoundDeadline)
            .expect("Deadline not set");
        if env.ledger().timestamp() > deadline {
            panic!("Contribution failed: Round deadline has passed");
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
            panic!("Already contributed for this round");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);
        let amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ContributionAmt)
            .unwrap();
        let current_round: u32 = env
            .storage()
            .instance()
            .get(&DataKey::CurrentRound)
            .unwrap_or(0);

        client.transfer(&contributor, &env.current_contract_address(), &amount);

        env.events().publish(
            (symbol_short!("contrib"), contributor.clone(), current_round),
            amount,
        );

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

        let current_participation = member_participation.get(contributor.clone()).unwrap_or(0);
        member_participation.set(contributor.clone(), current_participation + 1);
        total_participations += 1;

        env.storage()
            .instance()
            .set(&DataKey::TotalParticipations, &total_participations);
        env.storage()
            .instance()
            .set(&DataKey::MemberParticipation, &member_participation);

        if paid_members.len() == members.len() {
            Self::complete_round_payout(&env, &paid_members, amount, client);
        }

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn close_round(env: Env) {
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

        let mut defaulters = Vec::new(&env);
        for member in members.iter() {
            if !paid_members.contains(&member) {
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

        // Recalculate payout order: filter out the member (handles gracefully
        // if they already received their payout turn — they simply won't appear
        // in future rounds)
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

    pub fn bump_storage(env: Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    pub fn deposit_rewards(env: Env, depositor: Address, amount: i128) {
        depositor.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);

        client.transfer(&depositor, &env.current_contract_address(), &amount);

        let mut reward_pool: i128 = env.storage().instance().get(&DataKey::RewardPool).unwrap_or(0);
        reward_pool += amount;
        env.storage().instance().set(&DataKey::RewardPool, &reward_pool);

        env.events().publish(
            (symbol_short!("rew_dep"), depositor),
            amount,
        );
    }

    pub fn set_reward_dist_params(
        env: Env,
        dist_type: DistributionType,
        weights: Option<Map<Address, u32>>,
    ) {
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

        env.events().publish(
            (symbol_short!("rew_clm"), member),
            claimable,
        );
    }

    pub fn get_claimable_reward(env: Env, member: Address) -> i128 {
        let members: Vec<Address> = env.storage().instance().get(&DataKey::Members).expect("Not initialized");
        if !members.contains(&member) {
            return 0;
        }

        let reward_pool: i128 = env.storage().instance().get(&DataKey::RewardPool).unwrap_or(0);
        if reward_pool == 0 {
            return 0;
        }

        let dist_type: DistributionType = env
            .storage()
            .instance()
            .get(&DataKey::RewardDistType)
            .unwrap_or(DistributionType::Equal);

        let share = match dist_type {
            DistributionType::Equal => {
                reward_pool / (members.len() as i128)
            }
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

    // --- NEW READ INTERFACE FUNCTIONS ---

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

    // --- INTERNAL HELPERS ---

    fn complete_round_payout(
        env: &Env,
        _paid_members: &Vec<Address>,
        _amount: i128,
        client: token::Client,
    ) {
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

        let mut recipient_idx = current_round % payout_order.len();
        let mut attempts = 0;
        while attempts < payout_order.len() {
            let potential_recipient = payout_order.get(recipient_idx).unwrap();
            if !suspended_members.contains(&potential_recipient) {
                break;
            }
            recipient_idx = (recipient_idx + 1) % payout_order.len();
            attempts += 1;
        }

        if attempts >= payout_order.len() {
            panic!("All members are suspended");
        }

        let payout_recipient = payout_order.get(recipient_idx).unwrap();
        let reward_pool: i128 = env.storage().instance().get(&DataKey::RewardPool).unwrap_or(0);
        let total_balance = client.balance(&env.current_contract_address());
        let total_pot = total_balance - reward_pool;

        client.transfer(
            &env.current_contract_address(),
            &payout_recipient,
            &total_pot,
        );

        // Record history before resetting
        let mut history: Vec<PayoutRecord> = env
            .storage()
            .instance()
            .get(&DataKey::RoundHistory)
            .unwrap_or(Vec::new(env));
        history.push_back(PayoutRecord {
            recipient: payout_recipient.clone(),
            amount: total_pot,
        });
        env.storage()
            .instance()
            .set(&DataKey::RoundHistory, &history);

        env.events().publish(
            (symbol_short!("rd_done"), current_round),
            (payout_recipient, total_pot),
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
            &DataKey::RoundDeadline,
            &(env.ledger().timestamp() + duration),
        );
        env.events()
            .publish((symbol_short!("reset"),), current_round);
    }
}
mod test;
