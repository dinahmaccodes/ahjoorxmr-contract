use crate::{errors::Error, events, DataKey, PayoutRecord};
use soroban_sdk::{panic_with_error, token, Address, Env, Map, Vec};

const PERSISTENT_LIFETIME_THRESHOLD: u32 = 100_000;
const PERSISTENT_BUMP_AMOUNT: u32 = 120_000;

/// Panics if the contract is currently paused.
pub(crate) fn check_not_paused(env: &Env) {
    let is_paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::IsPaused)
        .unwrap_or(false);
    if is_paused {
        panic_with_error!(env, Error::ContractPaused);
    }
}

/// Pays out the current round's pot to the next eligible recipient, records
/// the payout in history, and resets the round state for the next round.
pub(crate) fn complete_round_payout(env: &Env, _paid_members: &Vec<Address>) {
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
        panic_with_error!(env, Error::AllMembersSuspended);
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
            client.transfer(&env.current_contract_address(), &payout_recipient, &balance);
        }
    }

    // Persistent: RoundHistory — append new record and extend its individual TTL
    let mut history: Vec<PayoutRecord> = env
        .storage()
        .persistent()
        .get(&DataKey::RoundHistory)
        .unwrap_or(Vec::new(env));
    history.push_back(PayoutRecord {
        recipient: payout_recipient.clone(),
        amount: total_payout_history_amt,
    });
    env.storage()
        .persistent()
        .set(&DataKey::RoundHistory, &history);
    env.storage().persistent().extend_ttl(
        &DataKey::RoundHistory,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );

    events::emit_rd_done(
        env,
        current_round,
        payout_recipient,
        total_payout_history_amt,
    );
    reset_round_state(env, current_round);
}

/// Advances the round counter, clears paid-members and per-round contributions,
/// and sets a new deadline.
pub(crate) fn reset_round_state(env: &Env, current_round: u32) {
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
    events::emit_reset(env, current_round);
}

/// Resets a member's default count and removes them from the suspended list.
pub(crate) fn execute_penalty_appeal(env: &Env, member: &Address) {
    let mut default_count: Map<Address, u32> = env
        .storage()
        .instance()
        .get(&DataKey::DefaultCount)
        .unwrap_or(Map::new(env));

    default_count.set(member.clone(), 0);
    env.storage()
        .instance()
        .set(&DataKey::DefaultCount, &default_count);

    let suspended_members: Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::SuspendedMembers)
        .unwrap_or(Vec::new(env));
    let mut new_suspended = Vec::new(env);
    for m in suspended_members.iter() {
        if m != *member {
            new_suspended.push_back(m);
        }
    }
    env.storage()
        .instance()
        .set(&DataKey::SuspendedMembers, &new_suspended);

    events::emit_appeal_ok(env, member.clone());
}

/// Updates the quorum percentage if the value is within [1, 100].
pub(crate) fn execute_rule_change(env: &Env, new_quorum: Option<i128>) {
    if let Some(quorum) = new_quorum {
        if quorum >= 1 && quorum <= 100 {
            env.storage()
                .instance()
                .set(&DataKey::QuorumPercentage, &(quorum as u32));
            events::emit_rule_upd(env, quorum);
        }
    }
}

/// Removes a member from both the members list and the payout order.
pub(crate) fn execute_member_removal(env: &Env, member: &Address) {
    let old_members: Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::Members)
        .unwrap_or(Vec::new(env));
    let mut new_members: Vec<Address> = Vec::new(env);
    for m in old_members.iter() {
        if m != *member {
            new_members.push_back(m);
        }
    }
    env.storage()
        .instance()
        .set(&DataKey::Members, &new_members);

    let old_order: Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::PayoutOrder)
        .unwrap_or(Vec::new(env));
    let mut new_order: Vec<Address> = Vec::new(env);
    for m in old_order.iter() {
        if m != *member {
            new_order.push_back(m);
        }
    }
    env.storage()
        .instance()
        .set(&DataKey::PayoutOrder, &new_order);

    events::emit_mem_del(env, member.clone());
}
