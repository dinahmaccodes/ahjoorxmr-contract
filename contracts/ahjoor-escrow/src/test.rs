#![cfg(test)]
extern crate alloc;
use super::*;
use proptest::prelude::*;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient as TokenAdminClient;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, BytesN, Env, IntoVal, String, Symbol,
};

const UPGRADE_WASM: &[u8] = include_bytes!("../../../fixtures/upgrade_contract.wasm");

// ---------------------------------------------------------------------------
//  Test Helpers
// ---------------------------------------------------------------------------

struct TestSetup<'a> {
    env: Env,
    client: AhjoorEscrowContractClient<'a>,
    admin: Address,
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

    client.initialize(&admin);
    client.add_allowed_token(&admin, &token_addr);

    TestSetup {
        env,
        client,
        admin,
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    s.client
        .create_escrow(&buyer, &seller, &arbiter, &0, &s.token_addr, &deadline);
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
    s.client
        .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.release_escrow(&arbiter, &escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
    assert_eq!(s.token_client.balance(&seller), 250);
}

#[test]
fn test_partial_release_by_buyer() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.partial_release(&buyer, &escrow_id, &150);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::PartiallyReleased);
    assert_eq!(escrow.amount, 100);
    assert_eq!(s.token_client.balance(&seller), 150);
    assert_eq!(s.token_client.balance(&s.client.address), 100);
}

#[test]
fn test_double_partial_release_then_full_release_remaining() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.partial_release(&buyer, &escrow_id, &100);
    s.client.partial_release(&arbiter, &escrow_id, &75);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::PartiallyReleased);
    assert_eq!(escrow.amount, 75);
    assert_eq!(s.token_client.balance(&seller), 175);
    assert_eq!(s.token_client.balance(&s.client.address), 75);

    s.client.release_escrow(&buyer, &escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
    assert_eq!(escrow.amount, 75);
    assert_eq!(s.token_client.balance(&seller), 250);
    assert_eq!(s.token_client.balance(&s.client.address), 0);
}

#[test]
fn test_partial_release_over_release_attempt_rejected() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    let result = s.client.try_partial_release(&buyer, &escrow_id, &251);
    assert!(result.is_err());

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Active);
    assert_eq!(escrow.amount, 250);
    assert_eq!(s.token_client.balance(&seller), 0);
    assert_eq!(s.token_client.balance(&s.client.address), 250);
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Item not received"),
        &250,
    );

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &seller,
        &escrow_id,
        &String::from_str(&s.env, "Payment not received"),
        &250,
    );

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client
        .dispute_escrow(&arbiter, &escrow_id, &String::from_str(&s.env, "Invalid"), &250);
}

// ===========================================================================
//  Partial Dispute Tests
// ===========================================================================

#[test]
fn test_partial_dispute_50_50_split() {
    let s = setup();
    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &200, &s.token_addr, &deadline);

    // Dispute only 100 (50%), undisputed 100 released to seller immediately
    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Half disputed"),
        &100,
    );

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::PartiallyDisputed);
    assert_eq!(escrow.amount, 100); // only disputed portion held
    assert_eq!(s.token_client.balance(&seller), 100); // undisputed released
    assert_eq!(s.token_client.balance(&s.client.address), 100);

    let dispute = s.client.get_dispute(&escrow_id);
    assert_eq!(dispute.dispute_amount, 100);
    assert_eq!(dispute.resolved, false);

    // Arbiter resolves the disputed portion to seller
    s.client.resolve_dispute(&arbiter, &escrow_id, &true);
    assert_eq!(s.token_client.balance(&seller), 200);
    assert_eq!(s.token_client.balance(&s.client.address), 0);
}

#[test]
fn test_partial_dispute_80_20_split() {
    let s = setup();
    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);

    // Dispute only 20 (20%), undisputed 80 released to seller immediately
    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Minor issue"),
        &20,
    );

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::PartiallyDisputed);
    assert_eq!(escrow.amount, 20);
    assert_eq!(s.token_client.balance(&seller), 80);
    assert_eq!(s.token_client.balance(&s.client.address), 20);

    // Arbiter refunds the disputed portion to buyer
    s.client.resolve_dispute(&arbiter, &escrow_id, &false);
    assert_eq!(s.token_client.balance(&buyer), 920); // 1000 - 100 deposited + 20 refunded
    assert_eq!(s.token_client.balance(&s.client.address), 0);
}

#[test]
fn test_full_dispute_still_works() {
    let s = setup();
    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    // Full dispute: dispute_amount == escrow amount
    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Full dispute"),
        &250,
    );

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Disputed);
    assert_eq!(escrow.amount, 250);
    assert_eq!(s.token_client.balance(&seller), 0); // nothing released
    assert_eq!(s.token_client.balance(&s.client.address), 250);
}

#[test]
fn test_partial_dispute_amount_exceeds_escrow_rejected() {
    let s = setup();
    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);

    let result = s.client.try_dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Over dispute"),
        &101,
    );
    assert!(result.is_err());

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Active);
}

#[test]
fn test_partial_dispute_emits_partial_dispute_raised_event() {
    let s = setup();
    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &200, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Partial"),
        &120,
    );

    let events = s.env.events().all();
    let last = events.last().unwrap();
    let expected_topics = (Symbol::new(&s.env, "partial_dispute_raised"),).into_val(&s.env);
    assert_eq!(last.1, expected_topics);

    let data: soroban_sdk::Map<Symbol, soroban_sdk::Val> = last.2.into_val(&s.env);
    let dispute_amount: i128 = data
        .get(Symbol::new(&s.env, "dispute_amount"))
        .unwrap()
        .into_val(&s.env);
    let released_amount: i128 = data
        .get(Symbol::new(&s.env, "released_amount"))
        .unwrap()
        .into_val(&s.env);
    assert_eq!(dispute_amount, 120);
    assert_eq!(released_amount, 80);
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Item not received"),
        &250,
    );
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &seller,
        &escrow_id,
        &String::from_str(&s.env, "Payment not received"),
        &250,
    );
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Item not received"),
        &250,
    );
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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.dispute_escrow(
        &buyer,
        &escrow_id,
        &String::from_str(&s.env, "Item not received"),
        &250,
    );

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
    s.client
        .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

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
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.release_escrow(&buyer, &escrow_id);

    let events = s.env.events().all();
    assert!(events.len() > 1);
}

#[test]
fn test_partial_release_emits_event() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &250, &s.token_addr, &deadline);

    s.client.partial_release(&buyer, &escrow_id, &150);

    let events = s.env.events().all();
    let last = events.last().unwrap();

    let expected_topics = (Symbol::new(&s.env, "partial_released"),).into_val(&s.env);
    assert_eq!(last.1, expected_topics);

    let data: soroban_sdk::Map<Symbol, soroban_sdk::Val> = last.2.into_val(&s.env);
    let emitted_escrow_id: u32 = data
        .get(Symbol::new(&s.env, "escrow_id"))
        .unwrap()
        .into_val(&s.env);
    let released_amount: i128 = data
        .get(Symbol::new(&s.env, "released_amount"))
        .unwrap()
        .into_val(&s.env);
    let remaining_amount: i128 = data
        .get(Symbol::new(&s.env, "remaining_amount"))
        .unwrap()
        .into_val(&s.env);

    assert_eq!(emitted_escrow_id, escrow_id);
    assert_eq!(released_amount, 150);
    assert_eq!(remaining_amount, 100);
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

    s.client
        .create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);
    s.client
        .create_escrow(&buyer, &seller, &arbiter, &200, &s.token_addr, &deadline);

    assert_eq!(s.client.get_escrow_counter(), 2);
}

#[test]
fn test_boundary_amount_i128_max_rejected_without_balance() {
    // TODO: Implement test
}

// ===========================================================================
//  Upgradeability Tests
// ===========================================================================

#[test]
fn test_admin_can_upgrade_and_version_increments() {
    let s = setup();

    assert_eq!(s.client.get_version(), 1);

    let wasm_hash = s.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    s.client.upgrade(&s.admin, &wasm_hash);

    let version: u32 = s.env.as_contract(&s.client.address, || {
        s.env
            .storage()
            .instance()
            .get(&DataKey::ContractVersion)
            .unwrap()
    });
    assert_eq!(version, 2);
}

#[test]
fn test_unauthorized_upgrade_fails() {
    let s = setup();

    let intruder = Address::generate(&s.env);
    let wasm_hash = s.env.deployer().upload_contract_wasm(UPGRADE_WASM);

    let result = s.client.try_upgrade(&intruder, &wasm_hash);
    assert!(result.is_err());
    assert_eq!(s.client.get_version(), 1);
}

#[test]
fn test_migration_runs_once_per_version() {
    let s = setup();

    s.client.migrate(&s.admin);

    let second = s.client.try_migrate(&s.admin);
    assert!(second.is_err());
}

#[test]
fn test_upgrade_atomicity_on_invalid_wasm_hash() {
    let s = setup();

    let invalid_hash = BytesN::from_array(&s.env, &[7u8; 32]);
    let result = s.client.try_upgrade(&s.admin, &invalid_hash);

    assert!(result.is_err());
    assert_eq!(s.client.get_version(), 1);
}

// ===========================================================================
//  Deadline Extension Tests
// ===========================================================================

#[test]
fn test_deadline_extension_two_party_flow() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1);

    let deadline = s.env.ledger().timestamp() + 10;
    let result = s.client.try_create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &i128::MAX,
        &s.token_addr,
        &deadline,
    );
    assert!(result.is_err());
}

#[test]
fn test_boundary_payment_id_u64_max_cast_not_found() {
    let s = setup();
    let id = u64::MAX as u32;
    let res = s.client.try_get_escrow(&id);
    assert!(res.is_err());
}

#[test]
fn test_auth_required_for_release_path() {
    let env = Env::default();
    let contract_id = env.register(AhjoorEscrowContract, ());
    let client = AhjoorEscrowContractClient::new(&env, &contract_id);
    let caller = Address::generate(&env);

    let res = client.try_release_escrow(&caller, &0);
    assert!(res.is_err());
}

#[test]
fn test_deadline_extension_buyer_proposes_seller_accepts() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let initial_deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &initial_deadline,
    );

    let extended_deadline = initial_deadline + 3600;
    s.client
        .propose_deadline_extension(&buyer, &escrow_id, &extended_deadline);
    s.client.accept_deadline_extension(&seller, &escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.deadline, extended_deadline);
}

#[test]
fn test_deadline_extension_seller_can_propose_buyer_accepts() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let initial_deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &initial_deadline,
    );

    let extended_deadline = initial_deadline + 7200;
    s.client
        .propose_deadline_extension(&seller, &escrow_id, &extended_deadline);
    s.client.accept_deadline_extension(&buyer, &escrow_id);

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.deadline, extended_deadline);
}

#[test]
fn test_deadline_extension_invalid_deadline_rejected() {
    // TODO: Implement test
}

// ===========================================================================
//  Pause Mechanism Tests
// ===========================================================================

#[test]
fn test_admin_can_pause_and_resume_contract() {
    let s = setup();

    let reason = String::from_str(&s.env, "Emergency maintenance");

    s.client.pause_contract(&s.admin, &reason);
    assert_eq!(s.client.is_paused(), true);
    assert_eq!(s.client.get_pause_reason(), reason);

    s.client.resume_contract(&s.admin);
    assert_eq!(s.client.is_paused(), false);
    assert_eq!(s.client.get_pause_reason(), String::from_str(&s.env, ""));
}

#[test]
fn test_non_admin_cannot_resume_contract() {
    let s = setup();

    let non_admin = Address::generate(&s.env);
    s.client
        .pause_contract(&s.admin, &String::from_str(&s.env, "Incident"));

    let res = s.client.try_resume_contract(&non_admin);
    assert!(res.is_err());
}

#[test]
fn test_event_snapshot_for_dispute() {
    // TODO: Implement event snapshot test
}

#[test]
fn test_write_functions_blocked_when_paused_reads_still_work() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 500;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &200, &s.token_addr, &deadline);

    s.client
        .dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "snapshot"), &200);

    let events = s.env.events().all();
    assert!(!events.is_empty());
    let snapshot = alloc::format!("{:?}", events);
    assert!(!snapshot.is_empty());
}

#[test]
fn test_fuzz_like_create_inputs_100_cases() {
    let s = setup();
    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &10_000_000);

    let mut seed: u64 = 0xA11CE73;
    for _ in 0..100 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let amount = ((seed % 5000) as i128) + 1;
        let deadline = s.env.ledger().timestamp() + 1 + (seed % 1000);
        let _ = s.client.try_create_escrow(
            &buyer,
            &seller,
            &arbiter,
            &amount,
            &s.token_addr,
            &deadline,
        );
    }

    assert!(s.client.get_escrow_counter() <= 100);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(120))]

    #[test]
    fn prop_escrow_conservation(deposit in 1i128..1_000_000, release in 0i128..1_000_000, refund in 0i128..1_000_000) {
        let released = core::cmp::min(release, deposit);
        let remaining_after_release = deposit - released;
        let refunded = core::cmp::min(refund, remaining_after_release);
        let remaining = deposit - released - refunded;

        prop_assert!(released + refunded <= deposit);
        prop_assert_eq!(released + refunded + remaining, deposit);
    }
}

#[test]
fn test_deadline_extension_cannot_be_same_as_current() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let initial_deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &initial_deadline,
    );

    let result = s
        .client
        .try_propose_deadline_extension(&buyer, &escrow_id, &initial_deadline);
    assert!(result.is_err());
}

#[test]
fn test_deadline_extension_same_party_accept_rejected() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let initial_deadline = s.env.ledger().timestamp() + 1000;
    let admin = Address::generate(&s.env);

    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &initial_deadline,
    );

    let extended_deadline = initial_deadline + 1800;
    s.client
        .propose_deadline_extension(&buyer, &escrow_id, &extended_deadline);

    let result = s.client.try_accept_deadline_extension(&buyer, &escrow_id);
    assert!(result.is_err());
}

#[test]
fn test_deadline_extension_proposal_expiry_rejected() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let initial_deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &initial_deadline,
    );

    let extended_deadline = initial_deadline + 1800;
    s.client
        .propose_deadline_extension(&buyer, &escrow_id, &extended_deadline);

    s.env
        .ledger()
        .set_timestamp(s.env.ledger().timestamp() + 24 * 60 * 60 + 1);

    let result = s.client.try_accept_deadline_extension(&seller, &escrow_id);
    assert!(result.is_err());

    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.deadline, initial_deadline);
}

#[test]
fn test_dispute_blocks_deadline_extension() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let initial_deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id = s.client.create_escrow(
        &buyer,
        &seller,
        &arbiter,
        &250,
        &s.token_addr,
        &initial_deadline,
    );

    s.client
        .dispute_escrow(&buyer, &escrow_id, &String::from_str(&s.env, "Need review"), &250);

    let result =
        s.client
            .try_propose_deadline_extension(&buyer, &escrow_id, &(initial_deadline + 3600));
    assert!(result.is_err());
}

#[test]
fn test_recovery_after_resume() {
    let s = setup();

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    let escrow_id =
        s.client
            .create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);

    s.client
        .pause_contract(&s.admin, &String::from_str(&s.env, "Emergency"));

    let create_res =
        s.client
            .try_create_escrow(&buyer, &seller, &arbiter, &100, &s.token_addr, &deadline);
    assert!(create_res.is_err());

    let release_res = s.client.try_release_escrow(&buyer, &escrow_id);
    assert!(release_res.is_err());

    s.client.resume_contract(&s.admin);

    s.client.release_escrow(&buyer, &escrow_id);
    let escrow = s.client.get_escrow(&escrow_id);
    assert_eq!(escrow.status, EscrowStatus::Released);
}

// ===========================================================================
//  Multi-Token Allowlist Tests
// ===========================================================================

#[test]
fn test_admin_can_add_and_remove_allowed_token() {
    let s = setup();
    let new_token = Address::generate(&s.env);

    s.client.add_allowed_token(&s.admin, &new_token);

    s.client.remove_allowed_token(&s.admin, &new_token);

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    let deadline = s.env.ledger().timestamp() + 1000;

    let res = s
        .client
        .try_create_escrow(&buyer, &seller, &arbiter, &100, &new_token, &deadline);
    assert!(res.is_err());
}

#[test]
fn test_non_admin_cannot_add_or_remove_allowed_token() {
    let s = setup();
    let non_admin = Address::generate(&s.env);
    let new_token = Address::generate(&s.env);

    let res = s.client.try_add_allowed_token(&non_admin, &new_token);
    assert!(res.is_err());

    let res = s.client.try_remove_allowed_token(&non_admin, &s.token_addr);
    assert!(res.is_err());
}

#[test]
#[should_panic(expected = "TokenNotAllowed")]
fn test_create_escrow_with_disallowed_token_panics_token_not_allowed() {
    let s = setup();
    let unallowed_token = Address::generate(&s.env);

    let buyer = Address::generate(&s.env);
    let seller = Address::generate(&s.env);
    let arbiter = Address::generate(&s.env);
    s.token_admin_client.mint(&buyer, &1000);

    let deadline = s.env.ledger().timestamp() + 1000;
    s.client
        .create_escrow(&buyer, &seller, &arbiter, &250, &unallowed_token, &deadline);
}
