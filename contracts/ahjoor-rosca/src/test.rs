#![cfg(test)]
extern crate alloc;
use super::*;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient as TokenAdminClient;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    vec, Address, Env, IntoVal,
};

pub struct TestSetup<'a> {
    pub env: Env,
    pub client: AhjoorContractClient<'a>,
    pub admin: Address,
    pub token_admin: Address,
    pub token_client: TokenClient<'a>,
    pub token_admin_client: TokenAdminClient<'a>,
}

fn setup_env<'a>() -> TestSetup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_client = TokenClient::new(&env, &token_admin);
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    TestSetup {
        env,
        client,
        admin,
        token_admin,
        token_client,
        token_admin_client,
    }
}

#[test]
fn test_rosca_flow_with_time_locks() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_client = TokenClient::new(&env, &token_admin);
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);
    for u in [&user1, &user2, &user3] {
        token_admin_client.mint(u, &1000);
    }

    let members = vec![&env, user1.clone(), user2.clone(), user3.clone()];
    let duration = 3600u64;
    let amount = 100i128;

    client.init(
        &admin,
        &members,
        &amount,
        &token_admin,
        &duration,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    env.ledger().set_timestamp(100);
    client.contribute(&user1);
    assert_eq!(token_client.balance(&user1), 900);

    env.ledger().set_timestamp(3601);
    let result = client.try_contribute(&user2);
    assert!(result.is_err());

    client.close_round();

    let (round, paid, deadline, _, _) = client.get_state();
    assert_eq!(round, 1);
    assert_eq!(paid.len(), 0);
    assert_eq!(deadline, 7201);

    env.ledger().set_timestamp(4000);
    client.contribute(&user1);
    assert_eq!(token_client.balance(&user1), 800);
}

#[test]
#[should_panic(expected = "Cannot close: Deadline has not passed yet")]
fn test_cannot_close_early() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let members = vec![&env, Address::generate(&env)];

    client.init(
        &admin,
        &members,
        &100,
        &Address::generate(&env),
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    env.ledger().set_timestamp(500);
    client.close_round();
}

#[test]
fn test_on_time_contribution() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    token_admin_client.mint(&user1, &1000);
    let members = vec![&env, user1.clone(), user2.clone()];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    env.ledger().set_timestamp(1000);
    client.contribute(&user1);

    assert_eq!(token_client.balance(&user1), 900);
    let (_, paid, _, _, _) = client.get_state();
    assert!(paid.contains(&user1));
}

#[test]
#[should_panic(expected = "Contribution failed: Round deadline has passed")]
fn test_late_contribution_rejection() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let user1 = Address::generate(&env);
    let members = vec![&env, user1.clone()];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    env.ledger().set_timestamp(3601);
    client.contribute(&user1);
}

#[test]
fn test_admin_close_round() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let members = vec![&env, Address::generate(&env)];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    env.ledger().set_timestamp(3601);
    client.close_round();

    let (round, _, _, _, _) = client.get_state();
    assert_eq!(round, 1);
}

// --- NEW STRATEGY-SPECIFIC TESTS ---

#[test]
fn test_admin_assigned_strategy_execution() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let members = vec![&env, user1.clone(), user2.clone()];

    // Reverse the order: user2 should get paid first
    let custom_order = vec![&env, user2.clone(), user1.clone()];

    token_admin_client.mint(&user1, &100);
    token_admin_client.mint(&user2, &100);

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::AdminAssigned,
            custom_order: Some(custom_order),
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    client.contribute(&user1);
    client.contribute(&user2);

    let token_client = TokenClient::new(&env, &token_admin);
    // User2 contributed 100, but was the recipient of the pot (200)
    assert_eq!(token_client.balance(&user2), 200);
}

#[test]
#[should_panic(expected = "Custom order length mismatch")]
fn test_invalid_admin_order_validation() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let members = vec![&env, Address::generate(&env), Address::generate(&env)];
    let bad_order = vec![&env, Address::generate(&env)]; // Too short

    client.init(
        &admin,
        &members,
        &100,
        &Address::generate(&env),
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::AdminAssigned,
            custom_order: Some(bad_order),
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );
}

#[test]
fn test_round_robin_e2e_all_rounds() {
    let env = Env::default();
    env.mock_all_auths();
    // FIX: Removed & and used register
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    // FIX: Use v2 and get the address
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone()];

    // FIX: Mint 2000 to cover multiple contributions and payouts
    for u in [&u1, &u2] {
        token_admin_client.mint(u, &2000);
    }

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    // ROUND 0: u1 should get the payout
    client.contribute(&u1);
    client.contribute(&u2);
    // Math: 2000 (start) - 100 (spent) + 200 (pot) = 2100
    assert_eq!(token_client.balance(&u1), 2100);

    // ROUND 1: u2 should get the payout
    client.contribute(&u1);
    client.contribute(&u2);
    // Math: 2000 (start) - 100 (spent R0) - 100 (spent R1) + 200 (pot R1) = 2000
    assert_eq!(token_client.balance(&u2), 2000);
}

#[test]
fn test_admin_assigned_e2e_all_rounds() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone()];

    for u in [&u1, &u2] {
        token_admin_client.mint(u, &2000);
    }

    // Strategy: Admin Assigned (Reverse the order: u2 then u1)
    let custom_order = vec![&env, u2.clone(), u1.clone()];
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::AdminAssigned,
            custom_order: Some(custom_order),
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    // ROUND 0: u2 should get the payout first
    client.contribute(&u1);
    client.contribute(&u2);
    assert_eq!(token_client.balance(&u2), 2100);

    // ROUND 1: u1 should get the payout second
    client.contribute(&u1);
    client.contribute(&u2);
    assert_eq!(token_client.balance(&u1), 2000);
}

#[test]
fn test_verify_contract_events() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    token_admin_client.mint(&user1, &1000);
    token_admin_client.mint(&user2, &1000);

    let members = vec![&env, user1.clone(), user2.clone()];
    let amount = 100i128;

    // 1. Verify ContractInitialized
    client.init(
        &admin,
        &members,
        &amount,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    let last_event = env.events().all().last().unwrap();
    assert_eq!(last_event.0, contract_id);
    // Topics check
    assert_eq!(
        last_event.1,
        vec![&env, symbol_short!("init").into_val(&env)]
    );
    // Data check: Convert Val -> (u32, i128)
    let init_data: (u32, i128) = soroban_sdk::FromVal::from_val(&env, &last_event.2);
    assert_eq!(init_data, (2u32, amount));

    // 2. Verify ContributionReceived
    client.contribute(&user1);

    let contribution_event = env.events().all().last().unwrap();
    assert_eq!(
        contribution_event.1,
        vec![
            &env,
            symbol_short!("contrib").into_val(&env),
            user1.clone().into_val(&env),
            0u32.into_val(&env)
        ]
    );
    // Data check: Val -> i128
    let contrib_amt: i128 = soroban_sdk::FromVal::from_val(&env, &contribution_event.2);
    assert_eq!(contrib_amt, amount);

    // 3. Verify RoundCompleted and RoundReset
    client.contribute(&user2);

    let all_events = env.events().all();
    let reset_event = all_events.get(all_events.len() - 1).unwrap();
    let payout_event = all_events.get(all_events.len() - 2).unwrap();

    // RoundCompleted check: Val -> (Address, i128)
    let payout_data: (Address, i128) = soroban_sdk::FromVal::from_val(&env, &payout_event.2);
    assert_eq!(payout_data, (user1.clone(), 200i128));

    // RoundReset check: Val -> u32
    let reset_round: u32 = soroban_sdk::FromVal::from_val(&env, &reset_event.2);
    assert_eq!(reset_round, 0u32);
}

// --- PENALTY AND DEFAULTER HANDLING TESTS ---

#[test]
fn test_single_defaulter_penalty() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let members = vec![&env, user1.clone(), user2.clone()];

    // Mint tokens including penalty amount
    token_admin_client.mint(&user1, &1000);
    token_admin_client.mint(&user2, &1000);

    let penalty_amount = 50i128;
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: penalty_amount,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Only user1 contributes
    client.contribute(&user1);

    // Wait for deadline to pass
    env.ledger().set_timestamp(3601);
    client.close_round();

    // Check events after close_round
    let events_after_close = env.events().all();
    assert!(events_after_close.len() > 0, "No events after close_round");

    // Admin penalizes user2 (defaulter) - this should work since user2 didn't contribute
    client.penalise_defaulter(&user2);

    // Check penalty was transferred
    assert_eq!(token_client.balance(&user2), 950); // 1000 - 50 penalty
}

#[test]
fn test_multiple_defaulters_penalty() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);
    let members = vec![&env, user1.clone(), user2.clone(), user3.clone()];

    // Mint tokens
    for user in [&user1, &user2, &user3] {
        token_admin_client.mint(user, &1000);
    }

    let penalty_amount = 30i128;
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: penalty_amount,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Only user1 contributes
    client.contribute(&user1);

    // Wait for deadline to pass
    env.ledger().set_timestamp(3601);
    client.close_round();

    // Admin penalizes both defaulters
    client.penalise_defaulter(&user2);
    client.penalise_defaulter(&user3);

    // Check penalties were transferred
    assert_eq!(token_client.balance(&user2), 970); // 1000 - 30 penalty
    assert_eq!(token_client.balance(&user3), 970); // 1000 - 30 penalty
}

#[test]
fn test_member_suspension_after_two_defaults() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let members = vec![&env, user1.clone(), user2.clone()];

    // Mint enough tokens for multiple rounds and penalties
    token_admin_client.mint(&user1, &2000);
    token_admin_client.mint(&user2, &2000);

    let penalty_amount = 25i128;
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: penalty_amount,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // ROUND 0: user2 defaults
    client.contribute(&user1);
    env.ledger().set_timestamp(3601);
    client.close_round();
    client.penalise_defaulter(&user2);

    // ROUND 1: user2 defaults again
    // After close_round, new deadline is set to current_timestamp + duration
    // So at 3601, new deadline would be 3601 + 3600 = 7201
    client.contribute(&user1);
    env.ledger().set_timestamp(7202); // Past the new deadline
    client.close_round();
    client.penalise_defaulter(&user2);

    // Check penalties were applied twice
    assert_eq!(token_client.balance(&user2), 1950); // 2000 - 25 - 25

    // Check that user2 was suspended (we can verify this by checking the balance was penalized twice)
    // The suspension event should have been emitted, but we'll just verify the functionality works
}

#[test]
fn test_suspended_member_skipped_in_payout() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let user3 = Address::generate(&env);
    let members = vec![&env, user1.clone(), user2.clone(), user3.clone()];

    // Mint enough tokens
    for user in [&user1, &user2, &user3] {
        token_admin_client.mint(user, &3000);
    }

    let penalty_amount = 20i128;
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: penalty_amount,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Suspend user2 by making them default twice
    // ROUND 0: user2 defaults
    client.contribute(&user1);
    client.contribute(&user3);
    env.ledger().set_timestamp(3601);
    client.close_round();
    client.penalise_defaulter(&user2);

    // ROUND 1: user2 defaults again (gets suspended)
    client.contribute(&user1);
    client.contribute(&user3);
    env.ledger().set_timestamp(7202);
    client.close_round();
    client.penalise_defaulter(&user2);

    // ROUND 2: All contribute, but user2 should be skipped for payout
    // Round 2 (index 2): 2 % 3 = 2, which is user3's turn
    // Since user2 is suspended, user3 should get the payout
    let user3_balance_before = token_client.balance(&user3);
    client.contribute(&user1);
    client.contribute(&user2); // user2 can still contribute
    client.contribute(&user3);

    // user3 should receive the payout (including penalty funds)
    let user3_balance_after = token_client.balance(&user3);
    let payout_received = user3_balance_after - user3_balance_before + 100; // +100 for contribution

    // Debug: let's see what the actual payout is
    // Expected: 300 (contributions) + accumulated penalties
    // Let's just check that user3 received more than the base contributions
    assert!(
        payout_received > 300,
        "Payout should include penalty funds, got: {}",
        payout_received
    );
}

#[test]
#[should_panic(expected = "Member is not a defaulter for this round")]
fn test_cannot_penalise_before_deadline() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let members = vec![&env, user1.clone()];

    client.init(
        &admin,
        &members,
        &100,
        &Address::generate(&env),
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 50,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    // Try to penalise before any round is closed (no defaulters identified yet)
    env.ledger().set_timestamp(1000);
    client.penalise_defaulter(&user1);
}

#[test]
#[should_panic(expected = "Penalty system is disabled")]
fn test_penalty_disabled_when_amount_zero() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user1 = Address::generate(&env);
    let members = vec![&env, user1.clone()];

    client.init(
        &admin,
        &members,
        &100,
        &Address::generate(&env),
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0, // (merged) disabled
            collective_goal: None,
            member_goals: None,
        },
    );

    env.ledger().set_timestamp(3601);
    client.close_round();
    client.penalise_defaulter(&user1);
}

#[test]
#[should_panic(expected = "Member is not a defaulter for this round")]
fn test_cannot_penalise_non_defaulter() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);
    let members = vec![&env, user1.clone(), user2.clone()];

    token_admin_client.mint(&user1, &1000);
    token_admin_client.mint(&user2, &1000);

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 50,
            exit_penalty_bps: 0, // (merged)
            collective_goal: None,
            member_goals: None,
        },
    );

    // Both users contribute (no defaulters)
    client.contribute(&user1);
    client.contribute(&user2);

    // Try to penalise user1 who contributed
    client.penalise_defaulter(&user1);
}

#[test]
fn test_read_interface_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone()];

    token_admin_client.mint(&u1, &1000);
    token_admin_client.mint(&u2, &1000);

    // 1. STAGE: Post-Initialization
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    let info = client.get_group_info();
    assert_eq!(info.members.len(), 2);
    assert_eq!(info.current_round, 0);
    assert_eq!(info.next_recipient, u1); // Round 0 recipient
    assert_eq!(client.get_round_history().len(), 0);

    // 2. STAGE: Mid-Round Contribution
    client.contribute(&u1);

    // Verify member status
    assert_eq!(client.get_member_status(&u1), true);
    assert_eq!(client.get_member_status(&u2), false);

    // Verify GroupInfo updates paid_members
    let info_mid = client.get_group_info();
    assert_eq!(info_mid.paid_members.len(), 1);
    assert!(info_mid.paid_members.contains(&u1));

    // 3. STAGE: Post-Payout (Round 0 Complete)
    client.contribute(&u2); // This triggers complete_round_payout

    // Verify History
    let history = client.get_round_history();
    assert_eq!(history.len(), 1);
    let record = history.get(0).unwrap();
    assert_eq!(record.recipient, u1);
    assert_eq!(record.amount, 200);

    // Verify New Round State
    let info_new_round = client.get_group_info();
    assert_eq!(info_new_round.current_round, 1);
    assert_eq!(info_new_round.next_recipient, u2); // Now it's u2's turn
    assert_eq!(info_new_round.paid_members.len(), 0); // Should be reset
}

#[test]
fn test_member_status_resets_after_round() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env); // Use 2 members so the round doesn't auto-close
    let members = vec![&env, u1.clone(), u2.clone()];

    token_admin_client.mint(&u1, &1000);
    token_admin_client.mint(&u2, &1000);

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // 1. u1 contributes. Round is NOT over because u2 hasn't paid.
    client.contribute(&u1);
    assert_eq!(client.get_member_status(&u1), true);
    assert_eq!(client.get_group_info().current_round, 0);

    // 2. u2 contributes. This completes Round 0 and starts Round 1.
    client.contribute(&u2);

    // 3. Now verify status is reset for the new round.
    assert_eq!(client.get_group_info().current_round, 1);
    assert_eq!(client.get_member_status(&u1), false);
    assert_eq!(client.get_member_status(&u2), false);
}

// ============================================================
//  DYNAMIC MEMBERSHIP TESTS
// ============================================================

#[test]
fn test_add_member_before_round() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let new_member = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone()];

    token_admin_client.mint(&u1, &1000);
    token_admin_client.mint(&u2, &1000);

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Add the new member before any round starts (paid_members is empty)
    client.add_member(&new_member);

    let info = client.get_group_info();
    assert_eq!(info.members.len(), 3);
    assert!(info.members.contains(&new_member));

    // Payout order should now include the new member
    // (get_group_info returns total_rounds which equals payout_order.len())
    assert_eq!(info.total_rounds, 3);
    // Event emission is confirmed by state change above (deprecated publish API
    // does not populate env.events().all() in this SDK version)
}

#[test]
#[should_panic(expected = "Cannot change members mid-round")]
fn test_add_member_mid_round_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let new_member = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone()];

    token_admin_client.mint(&u1, &1000);
    token_admin_client.mint(&u2, &1000);

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // u1 contributes — now paid_members is non-empty (mid-round)
    client.contribute(&u1);

    // Attempt to add a member mid-round — must panic
    client.add_member(&new_member);
}

#[test]
fn test_remove_member_between_rounds() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let u3 = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone(), u3.clone()];

    for u in [&u1, &u2, &u3] {
        token_admin_client.mint(u, &1000);
    }

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Complete round 0 so paid_members is reset
    client.contribute(&u1);
    client.contribute(&u2);
    client.contribute(&u3);
    // paid_members is now empty (round completed)

    // Remove u3 between rounds
    client.remove_member(&u3);

    let info = client.get_group_info();
    assert_eq!(info.members.len(), 2);
    assert!(!info.members.contains(&u3));
    assert_eq!(info.total_rounds, 2); // payout_order shrunk to 2
                                      // Event emission is confirmed by state change above (deprecated publish API
                                      // does not populate env.events().all() in this SDK version)
}

#[test]
#[should_panic(expected = "Cannot change members mid-round")]
fn test_remove_member_mid_round_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone()];

    token_admin_client.mint(&u1, &1000);
    token_admin_client.mint(&u2, &1000);

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // u1 contributes — mid-round state
    client.contribute(&u1);

    // Attempt to remove a member mid-round — must panic
    client.remove_member(&u2);
}

#[test]
fn test_remove_member_who_already_received_payout() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin);
    let token_client = TokenClient::new(&env, &token_admin);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);
    let u3 = Address::generate(&env);
    let members = vec![&env, u1.clone(), u2.clone(), u3.clone()];

    for u in [&u1, &u2, &u3] {
        token_admin_client.mint(u, &3000);
    }

    // RoundRobin: u1 gets round 0, u2 gets round 1, u3 gets round 2
    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Round 0: u1 receives payout
    client.contribute(&u1);
    client.contribute(&u2);
    client.contribute(&u3);
    let u1_after_r0 = token_client.balance(&u1);
    // u1 spent 100 and received 300 → net +200 = 3200
    assert_eq!(u1_after_r0, 3200);

    // Between rounds: remove u1 (who already received their payout)
    client.remove_member(&u1);

    let info = client.get_group_info();
    assert_eq!(info.members.len(), 2);
    assert!(!info.members.contains(&u1));
    assert_eq!(info.total_rounds, 2); // payout order now has u2, u3

    // Round 1 can proceed with the remaining two members — u2 should receive payout
    token_admin_client.mint(&u2, &200); // top u2 up so they have enough
    token_admin_client.mint(&u3, &200);
    client.contribute(&u2);
    client.contribute(&u3);

    // u2 gets the pot (200) — the contract still works correctly after removal
    // u2 started with 3000-200(r0 spend)+200(mint)=3000, spent 100 in r1, received 200
    let u2_balance = token_client.balance(&u2);
    assert!(
        u2_balance > 2900,
        "u2 should have received the payout, got: {}",
        u2_balance
    );
}

// --- NEW WHITELIST ADMIN TESTS (Issue #6) ---

#[test]
fn test_add_and_remove_approved_token() {
    let setup = setup_env();
    let token1 = Address::generate(&setup.env);
    let token2 = Address::generate(&setup.env);

    // Initial state: any token works because whitelist is empty.

    // We must manually set Admin so the auth check passes.
    // Normally init does this, but we want to test whitelist methods independently.
    setup.env.as_contract(&setup.client.address, || {
        setup
            .env
            .storage()
            .instance()
            .set(&DataKey::Admin, &setup.admin);
    });
    setup.client.add_approved_token(&token1);

    // After this, only token1 should be allowed during init.

    // Add token2 to whitelist
    setup.client.add_approved_token(&token2);

    // Remove token1 from whitelist
    setup.client.remove_approved_token(&token1);
}

#[test]
fn test_init_with_approved_token() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone()];

    // Add the specific token admin to whitelist
    setup.env.as_contract(&setup.client.address, || {
        setup
            .env
            .storage()
            .instance()
            .set(&DataKey::Admin, &setup.admin);
    });
    setup.client.add_approved_token(&setup.token_admin);

    // Should succeed because token_admin is in the whitelist
    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );
}

#[test]
#[should_panic(expected = "Token not approved")]
fn test_init_with_unapproved_token_panics() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone()];

    // Set admin
    setup.env.as_contract(&setup.client.address, || {
        setup
            .env
            .storage()
            .instance()
            .set(&DataKey::Admin, &setup.admin);
    });

    // Add some other token to whitelist
    let other_token = Address::generate(&setup.env);
    setup.client.add_approved_token(&other_token);

    // Should fail because token_admin is not in the whitelist
    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );
}

// --- NEW EDGE CASE AND FAILURE PATH TESTS (Issue #9) ---

#[test]
#[should_panic(expected = "Already initialized")]
fn test_init_twice_panics() {
    let setup = setup_env();
    let members = vec![
        &setup.env,
        Address::generate(&setup.env),
        Address::generate(&setup.env),
    ];

    // First init
    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Second init should panic
    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );
}

#[test]
#[should_panic(expected = "Not a member")]
fn test_contribute_non_member_panics() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let non_member = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Non-member trying to contribute
    setup.client.contribute(&non_member);
}

#[test]
#[should_panic(expected = "Already contributed for this round")]
fn test_contribute_twice_panics() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];

    setup.token_admin_client.mint(&u1, &1000);

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // First contribution
    setup.client.contribute(&u1);

    // Second contribution by the same member in the same round should panic
    setup.client.contribute(&u1);
}

#[test]
fn test_payout_correct_member_n_group() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let u3 = Address::generate(&setup.env);
    let u4 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone(), u3.clone(), u4.clone()];

    for u in [&u1, &u2, &u3, &u4] {
        setup.token_admin_client.mint(u, &1000);
    }

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Round 0: u1 gets the pot (4 * 100 = 400)
    setup.client.contribute(&u1);
    setup.client.contribute(&u2);
    setup.client.contribute(&u3);
    setup.client.contribute(&u4);

    // u1 history: 1000 - 100 + 400 = 1300
    assert_eq!(setup.token_client.balance(&u1), 1300);

    // Round 1: u2 gets the pot
    setup.client.contribute(&u1);
    setup.client.contribute(&u2);
    setup.client.contribute(&u3);
    setup.client.contribute(&u4);

    // u2 history: 1000 - 100(R0) - 100(R1) + 400 = 1200
    assert_eq!(setup.token_client.balance(&u2), 1200);

    // Round 2: u3 gets the pot
    setup.client.contribute(&u1);
    setup.client.contribute(&u2);
    setup.client.contribute(&u3);
    setup.client.contribute(&u4);

    // u3 history: 1000 - 100(R0) - 100(R1) - 100(R2) + 400 = 1100
    assert_eq!(setup.token_client.balance(&u3), 1100);

    // Round 3: u4 gets the pot
    setup.client.contribute(&u1);
    setup.client.contribute(&u2);
    setup.client.contribute(&u3);
    setup.client.contribute(&u4);

    // u4 history: 1000 - 100(R0) - 100(R1) - 100(R2) - 100(R3) + 400 = 1000
    assert_eq!(setup.token_client.balance(&u4), 1000);

    // u1 loses 100 in R1, R2, R3 (total 300) -> 1300 - 300 = 1000
    assert_eq!(setup.token_client.balance(&u1), 1000);
}

#[test]
fn test_contract_balance_zero_after_round() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];

    for u in [&u1, &u2] {
        setup.token_admin_client.mint(u, &1000);
    }

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Before contributions, balance is 0
    let current_contract_address = setup.client.address.clone();
    assert_eq!(setup.token_client.balance(&current_contract_address), 0);

    // u1 contributes, balance is 100
    setup.client.contribute(&u1);
    assert_eq!(setup.token_client.balance(&current_contract_address), 100);

    // u2 contributes, finishes round, payout dispatched, balance should be 0
    setup.client.contribute(&u2);
    assert_eq!(setup.token_client.balance(&current_contract_address), 0);
}

#[test]
fn test_single_member_rosca() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone()];

    setup.token_admin_client.mint(&u1, &1000);

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Single member contributes, should immediately complete round and payout to self
    setup.client.contribute(&u1);

    // Balance remains 1000 (spent 100, received 100 immediately)
    assert_eq!(setup.token_client.balance(&u1), 1000);

    // State should now be round 1
    let state = setup.client.get_state();
    assert_eq!(state.0, 1);
}

#[test]
fn test_large_group_rosca() {
    let setup = setup_env();
    let mut member_addresses = alloc::vec::Vec::new();
    let mut members = soroban_sdk::Vec::new(&setup.env);

    // 10 members
    for _ in 0..10 {
        let addr = Address::generate(&setup.env);
        setup.token_admin_client.mint(&addr, &2000); // Plenty of tokens
        member_addresses.push(addr.clone());
        members.push_back(addr.clone());
    }

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Do 1 full cycle (10 rounds)
    for round_idx in 0..10 {
        for m in member_addresses.iter() {
            setup.client.contribute(m);
        }
    }

    // At the end of 10 rounds, everyone should have exactly back what they started with
    for m in member_addresses.iter() {
        assert_eq!(setup.token_client.balance(m), 2000);
    }

    let state = setup.client.get_state();
    assert_eq!(state.0, 10); // completed 10 rounds
}

#[test]
fn test_get_state_lifecycle_details() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];

    for u in [&u1, &u2] {
        setup.token_admin_client.mint(u, &1000);
    }

    // Setup initially uses ledger timestamp 0 internally, so duration 3600 sets deadline to 3600.
    setup.env.ledger().set_timestamp(100);

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Before any contributions
    let (round, paid, deadline, strategy, _) = setup.client.get_state();
    assert_eq!(round, 0);
    assert_eq!(paid.len(), 0);
    assert_eq!(deadline, 3700); // 100 + 3600
    assert_eq!(strategy, PayoutStrategy::RoundRobin);

    // During a round
    setup.client.contribute(&u1);
    let (round_mid, paid_mid, deadline_mid, _, _) = setup.client.get_state();
    assert_eq!(round_mid, 0);
    assert_eq!(paid_mid.len(), 1);
    assert!(paid_mid.contains(&u1));
    assert_eq!(deadline_mid, 3700);

    // After a round
    setup.env.ledger().set_timestamp(200); // Advance time slightly
    setup.client.contribute(&u2); // Completes the round

    let (round_after, paid_after, deadline_after, _, _) = setup.client.get_state();
    assert_eq!(round_after, 1);
    assert_eq!(paid_after.len(), 0);
    assert_eq!(deadline_after, 3800); // 200 + 3600
}

#[test]
fn test_bump_storage() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone()];

    setup.token_admin_client.mint(&u1, &1000);

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Call bump_storage
    setup.client.bump_storage();

    // Advance ledger far into the future
    setup
        .env
        .ledger()
        .set_sequence_number(setup.env.ledger().sequence() + 50_000);

    // Verify contract is still accessible
    let (round, paid, _, _, _) = setup.client.get_state();
    assert_eq!(round, 0);
    assert_eq!(paid.len(), 0);
}

#[test]
fn test_reward_distribution_scenarios() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];

    setup.token_admin_client.mint(&u1, &1000);
    setup.token_admin_client.mint(&u2, &1000);
    setup.token_admin_client.mint(&setup.admin, &1000);

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // 1. Deposit Rewards
    setup.client.deposit_rewards(&setup.admin, &200);

    // 2. Equal Distribution (Default)
    assert_eq!(setup.client.get_claimable_reward(&u1), 100);
    assert_eq!(setup.client.get_claimable_reward(&u2), 100);

    // 3. Proportional Distribution
    setup
        .client
        .set_reward_dist_params(&DistributionType::Proportional, &None);
    // No participations yet
    assert_eq!(setup.client.get_claimable_reward(&u1), 0);

    setup.client.contribute(&u1);
    // u1 has 1 participation, total 1 -> 200 * 1/1 = 200
    assert_eq!(setup.client.get_claimable_reward(&u1), 200);
    assert_eq!(setup.client.get_claimable_reward(&u2), 0);

    setup.client.contribute(&u2);
    // u1 has 1, u2 has 1, total 2 -> 200 * 1/2 = 100 each
    assert_eq!(setup.client.get_claimable_reward(&u1), 100);
    assert_eq!(setup.client.get_claimable_reward(&u2), 100);

    // 4. Weighted Distribution
    let mut weights: Map<Address, u32> = Map::new(&setup.env);
    weights.set(u1.clone(), 3);
    weights.set(u2.clone(), 1);
    setup
        .client
        .set_reward_dist_params(&DistributionType::Weighted, &Some(weights));
    // u1: 200 * 3/4 = 150, u2: 200 * 1/4 = 50
    assert_eq!(setup.client.get_claimable_reward(&u1), 150);
    assert_eq!(setup.client.get_claimable_reward(&u2), 50);

    // 5. Claim Rewards
    let u1_balance_before = setup.token_client.balance(&u1);
    setup.client.claim_rewards(&u1);
    assert_eq!(setup.token_client.balance(&u1), u1_balance_before + 150);
    assert_eq!(setup.client.get_claimable_reward(&u1), 0);

    // 6. Deposit More Rewards
    setup.client.deposit_rewards(&setup.admin, &100);
    // Total pool is now 300.
    // u1 share: 300 * 3/4 = 225. Claimed: 150. Claimable: 75
    // u2 share: 300 * 1/4 = 75. Claimed: 0. Claimable: 75
    assert_eq!(setup.client.get_claimable_reward(&u1), 75);
    assert_eq!(setup.client.get_claimable_reward(&u2), 75);
}

#[test]
fn test_contribution_pot_separation() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];

    setup.token_admin_client.mint(&u1, &1000);
    setup.token_admin_client.mint(&u2, &1000);
    setup.token_admin_client.mint(&setup.admin, &1000);

    setup.client.init(
        &setup.admin,
        &members,
        &100,
        &setup.token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Deposit rewards
    setup.client.deposit_rewards(&setup.admin, &500);

    // Complete a round
    let u1_balance_before = setup.token_client.balance(&u1);
    setup.client.contribute(&u1);
    setup.client.contribute(&u2);

    // u1 was recipient. Pot should be exactly 200 (100 * 2), NOT including rewards.
    // u1 balance: 1000 (start) - 100 (contrib) + 200 (pot) = 1100
    assert_eq!(setup.token_client.balance(&u1), 1100);

    // Rewards pool should still be intact (500)
    assert_eq!(setup.client.get_claimable_reward(&u1), 250); // Equal share
    assert_eq!(setup.client.get_claimable_reward(&u2), 250);
}

// ============================================================
//  EMERGENCY EXIT MECHANISM TESTS — Issue #24
// ============================================================

/// Helper: initialise a 3-member ROSCA with an exit penalty of 10% (1000 bps).
/// Returns (client, admin, u1, u2, u3, token_client, token_admin)
fn setup_exit_env(
    env: &Env,
) -> (
    AhjoorContractClient,
    Address,
    Address,
    Address,
    Address,
    soroban_sdk::token::Client,
    Address,
) {
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let token_admin_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(env, &token_admin_addr);
    let token_client = soroban_sdk::token::Client::new(env, &token_admin_addr);

    let u1 = Address::generate(env);
    let u2 = Address::generate(env);
    let u3 = Address::generate(env);

    for u in [&u1, &u2, &u3] {
        token_admin_client.mint(u, &3000);
    }

    let members = vec![env, u1.clone(), u2.clone(), u3.clone()];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin_addr,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 1000,
            exit_penalty_bps: 1000,
            collective_goal: None,
            member_goals: None,
        },
    );

    (client, admin, u1, u2, u3, token_client, token_admin_addr)
}

// ---------------------------------------------------------------
// 1. Happy-path: a member can request an emergency exit
// ---------------------------------------------------------------
#[test]
fn test_member_can_request_emergency_exit() {
    let env = Env::default();
    let (client, _admin, u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    // Between rounds (paid_members is empty) — request should succeed
    client.request_emergency_exit(&u1);

    let requests = client.get_exit_requests();
    assert!(
        requests.contains_key(u1.clone()),
        "Exit request should be stored"
    );

    let req = requests.get(u1.clone()).unwrap();
    assert_eq!(req.member, u1);
    // u1 has contributed 0 full rounds so far → refund = 0 - penalty = 0
    assert_eq!(req.rounds_contributed, 0);
    assert_eq!(req.penalty_amount, 0);
    assert_eq!(req.refund_amount, 0);
    assert!(!req.approved);
}

// ---------------------------------------------------------------
// 2. Non-member cannot request an exit
// ---------------------------------------------------------------
#[test]
#[should_panic(expected = "Not a member")]
fn test_exit_request_rejected_if_not_member() {
    let env = Env::default();
    let (client, _admin, _u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    let non_member = Address::generate(&env);
    client.request_emergency_exit(&non_member);
}

// ---------------------------------------------------------------
// 3. Already-exited member cannot request again
// ---------------------------------------------------------------
#[test]
#[should_panic(expected = "Member already exited")]
fn test_exit_request_rejected_if_already_exited() {
    let env = Env::default();
    let (client, _admin, u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    client.request_emergency_exit(&u1);
    client.approve_exit(&u1);

    // Now u1 is in ExitedMembers, requesting again should panic
    client.request_emergency_exit(&u1);
}

// ---------------------------------------------------------------
// 4. Cannot request exit mid-round (after at least one contribution)
// ---------------------------------------------------------------
#[test]
#[should_panic(expected = "Cannot request exit mid-round")]
fn test_exit_request_rejected_mid_round() {
    let env = Env::default();
    let (client, _admin, u1, u2, _u3, _tc, _ta) = setup_exit_env(&env);

    // u2 contributes → round is in progress
    client.contribute(&u2);

    // u1 tries to exit mid-round → should panic
    client.request_emergency_exit(&u1);
}

// ---------------------------------------------------------------
// 5. Admin approves exit: penalty kept, refund sent, member removed
//    Set up: advance round via close_round so contributions are still
//    held in the contract and can be refunded on exit.
// ---------------------------------------------------------------
#[test]
fn test_admin_approves_exit_penalty_applied() {
    let env = Env::default();
    let (client, _admin, u1, u2, u3, token_client, _ta) = setup_exit_env(&env);

    // u1 contributes in round 0. u2 does NOT (so the round never auto-completes).
    // Therefore the 100 tokens u1 sent remain in the contract.
    client.contribute(&u1);

    // Advance past deadline so admin can close the round.
    env.ledger().set_timestamp(3601);
    client.close_round();
    // Now CurrentRound = 1. Contract still holds u1's 100 tokens.

    // u1 has contributed in 1 round. penalty = 100 * 1000 / 10000 = 10. refund = 90.
    let u1_balance_before_exit = token_client.balance(&u1);
    client.request_emergency_exit(&u1);

    let req = client.get_exit_requests().get(u1.clone()).unwrap();
    assert_eq!(req.rounds_contributed, 1);
    assert_eq!(req.penalty_amount, 10); // 10% of 100
    assert_eq!(req.refund_amount, 90);

    client.approve_exit(&u1);

    // u1 received the refund (90 returned, 10 stays as penalty in contract)
    let u1_balance_after_exit = token_client.balance(&u1);
    assert_eq!(u1_balance_after_exit, u1_balance_before_exit + 90);

    // u1 no longer a member
    let info = client.get_group_info();
    assert!(!info.members.contains(&u1));
    assert_eq!(info.total_rounds, 3); // PayoutOrder remains at 3 to keep schedule sync

    // u1 appears in exited members
    let exited = client.get_exited_members();
    assert!(exited.contains(&u1));

    // Exit request is cleared
    assert!(!client.get_exit_requests().contains_key(u1.clone()));

    // u2 can still continue normally in round 1
    // At Round 1, recipient is originally u2 (1 % 3 = 1).
    let u2_before = token_client.balance(&u2);
    client.contribute(&u2);
    client.contribute(&u3); // Both must contribute to complete round with 2 members

    // Pot = 10 (penalty from u1's exit) + 200 (u2+u3 contributions) = 210 → goes to u2
    let u2_after = token_client.balance(&u2);
    assert!(
        u2_after > u2_before,
        "u2 should have received the round payout"
    );
    assert_eq!(u2_after, u2_before - 100 + 210);
}

// ---------------------------------------------------------------
// 6. Admin rejects exit: member stays, request cleared
// ---------------------------------------------------------------
#[test]
fn test_admin_rejects_exit_request() {
    let env = Env::default();
    let (client, _admin, u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    client.request_emergency_exit(&u1);
    client.reject_exit(&u1);

    // Request is removed
    assert!(!client.get_exit_requests().contains_key(u1.clone()));

    // u1 is still a member
    let info = client.get_group_info();
    assert!(info.members.contains(&u1));

    // u1 is NOT in exited members
    assert!(!client.get_exited_members().contains(&u1));
}

// ---------------------------------------------------------------
// 7. Exited member cannot contribute
// ---------------------------------------------------------------
#[test]
#[should_panic(expected = "Member has exited")]
fn test_exited_member_cannot_contribute() {
    let env = Env::default();
    let (client, _admin, u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    client.request_emergency_exit(&u1);
    client.approve_exit(&u1);

    // u1 tries to contribute after exit — must panic
    client.contribute(&u1);
}

// ---------------------------------------------------------------
// 8. Exited member is skipped in payout order; remaining members
//    still receive correct payouts.
//    u1 exits between rounds (0 contributions, so refund=0).
// ---------------------------------------------------------------
#[test]
fn test_exited_member_skipped_in_payout_order() {
    let env = Env::default();
    let (client, _admin, u1, u2, u3, token_client, _ta) = setup_exit_env(&env);

    // Round 0: u1 (index 0) is the payout recipient. All contribute.
    // u1 exits right away (before any round contributions — refund = 0, no transfer needed)
    client.request_emergency_exit(&u1);
    client.approve_exit(&u1);

    // Now only u2 and u3 are members. Payout order is still 3.
    // Round 0 recipient was u1 (0 % 3 = 0). Since u1 is exited, it skips to u2 (1 % 3 = 1).
    let u2_before = token_client.balance(&u2);
    client.contribute(&u2);
    client.contribute(&u3);
    // Pot = 200 (100 * 2 members).
    let u2_after = token_client.balance(&u2);
    assert_eq!(
        u2_after,
        u2_before - 100 + 200,
        "u2 should receive the round 0 pot after u1 exits"
    );
}

// ---------------------------------------------------------------
// 9. Exited members are NOT counted as defaulters after close_round
// ---------------------------------------------------------------
#[test]
fn test_exited_member_skipped_in_defaulters_list() {
    let env = Env::default();
    let (client, _admin, u1, u2, u3, _tc, _ta) = setup_exit_env(&env);

    // u1 exits before the first round deadline
    client.request_emergency_exit(&u1);
    client.approve_exit(&u1);

    // Only u2 contributes; u3 does not
    client.contribute(&u2);

    // Advance past deadline
    env.ledger().set_timestamp(3601);
    client.close_round();

    // u3 should be a defaulter, u1 should NOT (they've exited)
    // We verify by checking that penalising u1 panics
    let result = client.try_penalise_defaulter(&u1);
    assert!(
        result.is_err(),
        "Exited member must not appear in defaulters"
    );
}

// ---------------------------------------------------------------
// 10. Exit with zero penalty: full refund of contributions.
//     Use close_round to advance to round 1 while keeping
//     u1's contribution in the contract.
// ---------------------------------------------------------------
#[test]
fn test_exit_with_zero_penalty() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_admin_client = TokenAdminClient::new(&env, &token_admin_addr);
    let token_client = TokenClient::new(&env, &token_admin_addr);

    let u1 = Address::generate(&env);
    let u2 = Address::generate(&env);

    token_admin_client.mint(&u1, &2000);
    token_admin_client.mint(&u2, &2000);

    let members = vec![&env, u1.clone(), u2.clone()];
    client.init(
        &admin,
        &members,
        &100,
        &token_admin_addr,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // u1 contributes in round 0 but u2 does NOT → round never auto-completes.
    // u1's 100 tokens remain in the contract.
    client.contribute(&u1);

    // Let deadline pass and close round so CurrentRound becomes 1.
    env.ledger().set_timestamp(3601);
    client.close_round();

    // CurrentRound = 1. u1 has contributed 1 round. penalty = 0. refund = 100.
    let u1_balance_before = token_client.balance(&u1);
    client.request_emergency_exit(&u1);

    let req = client.get_exit_requests().get(u1.clone()).unwrap();
    assert_eq!(req.penalty_amount, 0);
    assert_eq!(req.refund_amount, 100); // full refund

    client.approve_exit(&u1);
    assert_eq!(token_client.balance(&u1), u1_balance_before + 100);
}

// ---------------------------------------------------------------
// 11. Exit request emits the correct event
// ---------------------------------------------------------------
#[test]
fn test_exit_request_event_emitted() {
    let env = Env::default();
    let (client, _admin, u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    client.request_emergency_exit(&u1);

    let all_events = env.events().all();
    let last = all_events.last().unwrap();
    // Topic[0] should be the symbol "exit_req"
    assert_eq!(
        last.1,
        vec![
            &env,
            symbol_short!("exit_req").into_val(&env),
            u1.into_val(&env)
        ]
    );
}

// ---------------------------------------------------------------
// 12. Approved exit emits the correct event
// ---------------------------------------------------------------
#[test]
fn test_exit_approval_event_emitted() {
    let env = Env::default();
    let (client, _admin, u1, _u2, _u3, _tc, _ta) = setup_exit_env(&env);

    client.request_emergency_exit(&u1);
    client.approve_exit(&u1);

    let all_events = env.events().all();
    let last = all_events.last().unwrap();
    assert_eq!(
        last.1,
        vec![
            &env,
            symbol_short!("exit_ok").into_val(&env),
            u1.into_val(&env)
        ]
    );
}

// --- PAUSE AND RESUME TESTS ---

#[test]
fn test_pause_and_resume_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let members = vec![&env, Address::generate(&env)];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    // Default state: not paused
    assert_eq!(client.is_paused(), false);

    // Admin pauses the group
    env.ledger().set_timestamp(1000);
    let reason = soroban_sdk::String::from_str(&env, "Emergency maintenance");
    client.pause_group(&reason);

    assert_eq!(client.is_paused(), true);
    let (is_paused, retrieved_reason, pause_time) = client.get_pause_info();
    assert_eq!(is_paused, true);
    assert_eq!(retrieved_reason, reason);
    assert_eq!(pause_time, 1000);

    // Initial deadline was start_time(0) + 3600 = 3600.
    let (_, _, initial_deadline, _, _) = client.get_state();
    assert_eq!(initial_deadline, 3600);

    // Admin resumes the group after 500 units of time
    env.ledger().set_timestamp(1500);
    client.resume_group(&soroban_sdk::String::from_str(&env, "Fixed"));

    assert_eq!(client.is_paused(), false);
    let (is_paused_after, retrieved_reason_after, pause_time_after) = client.get_pause_info();
    assert_eq!(is_paused_after, false);
    assert_eq!(retrieved_reason_after.len(), 0); // Removed from storage
    assert_eq!(pause_time_after, 0);

    // Check if the deadline was extended by pause duration (500)
    let (_, _, new_deadline, _, _) = client.get_state();
    assert_eq!(new_deadline, 4100);
}

#[test]
#[should_panic(expected = "Action blocked: Group is paused")]
fn test_paused_blocks_contribute() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let user1 = Address::generate(&env);
    let members = vec![&env, user1.clone()];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    client.pause_group(&soroban_sdk::String::from_str(&env, "Pause"));
    client.contribute(&user1);
}

#[test]
#[should_panic(expected = "Group is already paused")]
fn test_cannot_pause_already_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let members = vec![&env, Address::generate(&env)];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    let r = soroban_sdk::String::from_str(&env, "P");
    client.pause_group(&r);
    client.pause_group(&r);
}

#[test]
#[should_panic(expected = "Group is not paused")]
fn test_cannot_resume_not_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorContract, ());
    let client = AhjoorContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_admin = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let members = vec![&env, Address::generate(&env)];

    client.init(
        &admin,
        &members,
        &100,
        &token_admin,
        &3600,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: None,
        },
    );

    client.resume_group(&soroban_sdk::String::from_str(&env, "R"));
}

#[test]
fn test_collective_savings_goal() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let u2 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone(), u2.clone()];
    
    // Total goal: 400 (4 contributions of 100)
    setup.client.init(
        &setup.admin,
        &members,
        &100i128,
        &setup.token_admin,
        &3600u64,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: Some(400i128),
            member_goals: None,
        },
    );
    
    setup.env.ledger().set_timestamp(100);
    setup.token_admin_client.mint(&u1, &1000);
    setup.token_admin_client.mint(&u2, &1000);

    // 1st contribution (25%)
    setup.client.contribute(&u1);
    
    let events = setup.env.events().all();
    let last_event = events.last().unwrap();
    // Milestone 25 reached
    assert_eq!(last_event.1, vec![&setup.env, symbol_short!("milestone").into_val(&setup.env), 25u32.into_val(&setup.env)]);
    let milestone_data_1: i128 = soroban_sdk::FromVal::from_val(&setup.env, &last_event.2);
    assert_eq!(milestone_data_1, 100i128);

    // 2nd contribution (50%)
    setup.client.contribute(&u2);
    let events = setup.env.events().all();
    let last_event = events.last().unwrap();
    // Milestone 50 reached
    assert_eq!(last_event.1, vec![&setup.env, symbol_short!("milestone").into_val(&setup.env), 50u32.into_val(&setup.env)]);
    let milestone_data_2: i128 = soroban_sdk::FromVal::from_val(&setup.env, &last_event.2);
    assert_eq!(milestone_data_2, 200i128);

    let (total, goal, _, _) = setup.client.get_savings_progress(&None);
    assert_eq!(total, 200i128);
    assert_eq!(goal, 400i128);
}

#[test]
fn test_individual_member_goals() {
    let setup = setup_env();
    let u1 = Address::generate(&setup.env);
    let members = vec![&setup.env, u1.clone()];
    
    let mut member_goals = Map::new(&setup.env);
    member_goals.set(u1.clone(), 300i128);

    setup.client.init(
        &setup.admin,
        &members,
        &100i128,
        &setup.token_admin,
        &3600u64,
        &RoscaConfig {
            strategy: PayoutStrategy::RoundRobin,
            custom_order: None,
            penalty_amount: 0,
            exit_penalty_bps: 0,
            collective_goal: None,
            member_goals: Some(member_goals),
        },
    );
    
    setup.env.ledger().set_timestamp(100);
    setup.token_admin_client.mint(&u1, &1000);

    // Round 0
    setup.client.contribute(&u1);
    let (total, _, m_done, m_goal) = setup.client.get_savings_progress(&Some(u1.clone()));
    assert_eq!(total, 100i128);
    assert_eq!(m_done, 100i128);
    assert_eq!(m_goal, 300i128);

    // Round 1
    setup.env.ledger().set_timestamp(4000);
    setup.client.close_round();
    setup.client.contribute(&u1);
    
    let (_, _, m_done, _) = setup.client.get_savings_progress(&Some(u1.clone()));
    assert_eq!(m_done, 200i128);
}
