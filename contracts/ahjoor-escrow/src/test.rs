#![cfg(test)]
extern crate alloc;
use super::*;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient as TokenAdminClient;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, String,
};

// ---------------------------------------------------------------------------
//  Test Helpers
// ---------------------------------------------------------------------------

struct TestSetup<'a> {
    env: Env,
    client: AhjoorEscrowContractClient<'a>,
    token_addr: Address,
    token_client: TokenClient<'a>,
    token_admin_client: TokenAdminClient<'a>,
}

fn setup<'a>() -> TestSetup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorEscrowContract, ());
    let client = AhjoorEscrowContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_client = TokenClient::new(&env, &token_addr);
    let token_admin_client = TokenAdminClient::new(&env, &token_addr);

    TestSetup {
        env,
        client,
        token_addr,
        token_client,
        token_admin_client,
    }
}

// ===========================================================================
//  Create Escrow Tests
// ===========================================================================

#[test]
fn test_create_escrow() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    assert_eq!(escrow_id, 0);
    assert_eq!(s.token_client.balance(&buyer), 750);
    assert_eq!(s.token_client.balance(&s.client.address), 250);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Active);
    assert_eq!(escrow.amount, 250);
    assert_eq!(escrow.buyer, buyer);
    assert_eq!(escrow.seller, seller);
    assert_eq!(escrow.arbiter, arbiter);
}

#[test]
#[should_panic(expected = "Escrow amount must be positive")]
fn test_create_escrow_zero_amount_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    s.client.create_escrow(&buyer, &seller, &arbiter, &0, &s.token_addr, &deadline);
}

#[test]
#[should_panic(expected = "Deadline must be in the future")]
fn test_create_escrow_past_deadline_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp();
    s.client.create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);
}

// ===========================================================================
//  Release Escrow Tests
// ===========================================================================

#[test]
fn test_release_escrow_by_buyer() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.release_escrow(&buyer, &escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
    assert_eq!(s.token_client.balance(&seller), 250);
    assert_eq!(s.token_client.balance(&s.client.address), 0);
}

#[test]
fn test_release_escrow_by_arbiter() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.release_escrow(&arbiter, &escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
    assert_eq!(s.token_client.balance(&seller), 250);
}

#[test]
#[should_panic(expected = "Only buyer or arbiter can release escrow")]
fn test_release_escrow_by_seller_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.release_escrow(&seller, &escrow_id);
}

#[test]
#[should_panic(expected = "Escrow is not active")]
fn test_release_escrow_already_released_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.release_escrow(&buyer, &escrow_id);
    s.client.release_escrow(&buyer, &escrow_id); // Should panic
}

// ===========================================================================
//  Dispute Escrow Tests
// ===========================================================================

#[test]
fn test_dispute_escrow_by_buyer() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "Item not received"));

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Disputed);

    let dispute = s.client.get_dispute(&escrow_id);
    assert_eq!(dispute.resolved, false);
}

#[test]
fn test_dispute_escrow_by_seller() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&seller, &escrow_id, &String::from_str(&s.env, "Payment not received"));

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Disputed);
}

#[test]
#[should_panic(expected = "Only buyer or seller can dispute escrow")]
fn test_dispute_escrow_by_arbiter_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&arbiter, &escrow_id, &String::from_str(&s.env, "Invalid"));
}

// ===========================================================================
//  Resolve Dispute Tests
// ===========================================================================

#[test]
fn test_resolve_dispute_to_seller() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "Item not received"));
    s.client.resolve_dispute(&arbiter, &escrow_id, &true);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
    assert_eq!(s.token_client.balance(&seller), 250);
    assert_eq!(s.token_client.balance(&s.client.address), 0);

    let dispute = s.client.get_dispute(&escrow_id);
    assert_eq!(dispute.resolved, true);
}

#[test]
fn test_resolve_dispute_to_buyer() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&seller, &escrow_id, &String::from_str(&s.env, "Payment not received"));
    s.client.resolve_dispute(&arbiter, &escrow_id, &false);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Refunded);
    assert_eq!(s.token_client.balance(&buyer), 1000);
    assert_eq!(s.token_client.balance(&s.client.address), 0);
}

#[test]
#[should_panic(expected = "Only arbiter can resolve dispute")]
fn test_resolve_dispute_by_buyer_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "Item not received"));
    s.client.resolve_dispute(&buyer, &escrow_id, &true);
}

// ===========================================================================
//  Auto-Release Expired Escrow Tests
// ===========================================================================

#[test]
fn test_auto_release_expired() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    // Advance time past deadline
    s.env.ledger().set_timestamp(deadline + 1);

    s.client.auto_release_expired(&escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Refunded);
    assert_eq!(s.token_client.balance(&buyer), 1000);
    assert_eq!(s.token_client.balance(&s.client.address), 0);
}

#[test]
#[should_panic(expected = "Escrow has not expired yet")]
fn test_auto_release_not_expired_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.auto_release_expired(&escrow_id);
}

#[test]
#[should_panic(expected = "Escrow is not active")]
fn test_auto_release_disputed_panics() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "Item not received"));

    // Advance time past deadline
    s.env.ledger().set_timestamp(deadline + 1);

    s.client.auto_release_expired(&escrow_id);
}

// ===========================================================================
//  Event Tests
// ===========================================================================

#[test]
fn test_escrow_created_emits_event() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    s.client.create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    let events = s.env.events().all();
    assert!(events.len() > 0);
}

#[test]
fn test_escrow_released_emits_event() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client.release_escrow(&buyer, &escrow_id);

    let events = s.env.events().all();
    assert!(events.len() > 1);
}

// ===========================================================================
//  Counter Tests
// ===========================================================================

#[test]
fn test_escrow_counter_increments() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;

    s.client.create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);
    s.client.create_escrow(&buyer, &seller, &arbiter, &200, &s.token_addr, &deadline);

    assert_eq!(s.client.get_escrow_counter(), 2);
}

// ===========================================================================
//  Pause Mechanism Tests
// ===========================================================================

#[test]
fn test_admin_can_pause_and_resume_contract() {
    let s = setup();

    let admin = Address::generate(&s.env);
    let reason = String::from_str(&s.env, "Emergency maintenance");

    s.client.pause_contract(&admin, &reason);
    assert_eq!(s.client.is_paused(), true);
    assert_eq!(s.client.get_pause_reason(), reason);

    s.client.resume_contract(&admin);
    assert_eq!(s.client.is_paused(), false);
    assert_eq!(s.client.get_pause_reason(), String::from_str(&s.env, ""));
}

#[test]
fn test_non_admin_cannot_resume_contract() {
    let s = setup();

    let admin = Address::generate(&s.env);
    let non_admin = Address::generate(&s.env);
    s.client
        .pause_contract(&admin, &String::from_str(&s.env, "Incident"));

    let res = s.client.try_resume_contract(&non_admin);
    assert!(res.is_err());
}

#[test]
fn test_write_functions_blocked_when_paused_reads_still_work() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    let admin = Address::generate(&s.env);

    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client
        .pause_contract(&admin, &String::from_str(&s.env, "Emergency"));

    let create_res = s
        .client
        .try_create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);
    assert!(create_res.is_err());

    let release_res = s.client.try_release_escrow(&buyer, &escrow_id);
    assert!(release_res.is_err());

    let dispute_res =
        s.client
            .try_dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "reason"));
    assert!(dispute_res.is_err());

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Active);
    assert_eq!(s.client.get_escrow_counter(), 1);
}

#[test]
fn test_recovery_after_resume() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    let admin = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &deadline,
    );

    s.client
        .pause_contract(&admin, &String::from_str(&s.env, "Emergency"));
    s.client.resume_contract(&admin);

    s.client.release_escrow(&buyer, &escrow_id);
    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
}
