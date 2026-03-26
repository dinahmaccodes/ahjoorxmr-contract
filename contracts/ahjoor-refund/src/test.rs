#![cfg(test)]
extern crate alloc;
use super::*;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient as TokenAdminClient;
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, BytesN, Env, String,
};

const UPGRADE_WASM: &[u8] = include_bytes!("../../../fixtures/upgrade_contract.wasm");

// ---------------------------------------------------------------------------
//  Test Helpers
// ---------------------------------------------------------------------------

struct TestSetup<'a> {
    env: Env,
    client: AhjoorRefundContractClient<'a>,
    admin: Address,
    token_addr: Address,
    token_client: TokenClient<'a>,
    token_admin_client: TokenAdminClient<'a>,
}

fn setup<'a>() -> TestSetup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorRefundContract, ());
    let client = AhjoorRefundContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_client = TokenClient::new(&env, &token_addr);
    let token_admin_client = TokenAdminClient::new(&env, &token_addr);

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
//  Initialize Tests
// ===========================================================================

#[test]
fn test_initialize() {
    let s = setup();
    s.client.initialize(&s.admin);

    assert_eq!(s.client.get_refund_counter(), 0);
    assert_eq!(s.client.get_admin(), s.admin);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_panics() {
    let s = setup();
    s.client.initialize(&s.admin);
    s.client.initialize(&s.admin);
}

// ===========================================================================
//  Request Refund Tests
// ===========================================================================

#[test]
fn test_request_refund() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    assert_eq!(refund_id, 0);
    assert_eq!(s.client.get_refund_counter(), 1);

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Requested);
    assert_eq!(refund.amount, 250);
    assert_eq!(refund.customer, customer);
}

#[test]
#[should_panic(expected = "Refund amount must be positive")]
fn test_request_refund_zero_amount_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.client.request_refund(
        &customer,
        &0,
        &s.token_addr,
        &String::from_str(&s.env, "Invalid"),
    );
}

// ===========================================================================
//  Approve Refund Tests
// ===========================================================================

#[test]
fn test_approve_refund() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Approved);
    assert!(refund.approved_at.is_some());
}

#[test]
#[should_panic(expected = "Only admin can approve refunds")]
fn test_approve_refund_by_non_admin_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let non_admin = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&non_admin, &refund_id);
}

#[test]
#[should_panic(expected = "Refund is not in requested status")]
fn test_approve_already_approved_refund_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);
    s.client.approve_refund(&s.admin, &refund_id); // Should panic
}

// ===========================================================================
//  Reject Refund Tests
// ===========================================================================

#[test]
fn test_reject_refund() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.reject_refund(
        &s.admin,
        &refund_id,
        &String::from_str(&s.env, "Invalid reason"),
    );

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Rejected);
}

#[test]
#[should_panic(expected = "Only admin can reject refunds")]
fn test_reject_refund_by_non_admin_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let non_admin = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.reject_refund(
        &non_admin,
        &refund_id,
        &String::from_str(&s.env, "Invalid reason"),
    );
}

#[test]
#[should_panic(expected = "Refund is not in requested status")]
fn test_reject_already_rejected_refund_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.reject_refund(
        &s.admin,
        &refund_id,
        &String::from_str(&s.env, "Invalid reason"),
    );
    s.client.reject_refund(
        &s.admin,
        &refund_id,
        &String::from_str(&s.env, "Already rejected"),
    ); // Should panic
}

// ===========================================================================
//  Process Refund Tests
// ===========================================================================

#[test]
fn test_process_refund() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);
    s.client.process_refund(&s.admin, &refund_id);

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Processed);
    assert!(refund.processed_at.is_some());
}

#[test]
#[should_panic(expected = "Only admin can process refunds")]
fn test_process_refund_by_non_admin_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let non_admin = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);
    s.client.process_refund(&non_admin, &refund_id);
}

#[test]
#[should_panic(expected = "Refund is not approved")]
fn test_process_unapproved_refund_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.process_refund(&s.admin, &refund_id);
}

#[test]
#[should_panic(expected = "Refund is not approved")]
fn test_process_rejected_refund_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.reject_refund(
        &s.admin,
        &refund_id,
        &String::from_str(&s.env, "Invalid reason"),
    );
    s.client.process_refund(&s.admin, &refund_id);
}

// ===========================================================================
//  Token Transfer Tests
// ===========================================================================

#[test]
fn test_token_transfer_on_process_refund() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let initial_balance = s.token_client.balance(&customer);
    assert_eq!(initial_balance, 1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);
    s.client.process_refund(&s.admin, &refund_id);

    let final_balance = s.token_client.balance(&customer);
    assert_eq!(final_balance, 1000);
}

#[test]
fn test_contract_holds_no_balance_after_process() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);
    s.client.process_refund(&s.admin, &refund_id);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 0);
}

// ===========================================================================
//  Lifecycle Tests
// ===========================================================================

#[test]
fn test_full_refund_lifecycle_approved() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    // Request
    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Requested);

    // Approve
    s.client.approve_refund(&s.admin, &refund_id);

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Approved);

    // Process
    s.client.process_refund(&s.admin, &refund_id);

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Processed);
    assert!(refund.processed_at.is_some());
}

#[test]
fn test_full_refund_lifecycle_rejected() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    // Request
    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Requested);

    // Reject
    s.client.reject_refund(
        &s.admin,
        &refund_id,
        &String::from_str(&s.env, "Invalid reason"),
    );

    let refund = s.client.get_refund(&refund_id);
    assert_eq!(refund.status, RefundStatus::Rejected);
}

// ===========================================================================
//  Event Tests
// ===========================================================================

#[test]
fn test_refund_requested_emits_event() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    let events = s.env.events().all();
    assert!(events.len() > 0);
}

#[test]
fn test_refund_approved_emits_event() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);

    let events = s.env.events().all();
    assert!(events.len() > 0);
}

#[test]
fn test_refund_processed_emits_event() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let refund_id = s.client.request_refund(
        &customer,
        &250,
        &s.token_addr,
        &String::from_str(&s.env, "Item not received"),
    );

    s.client.approve_refund(&s.admin, &refund_id);
    s.client.process_refund(&s.admin, &refund_id);

    let events = s.env.events().all();
    assert!(events.len() > 0);
}

// ===========================================================================
//  Counter Tests
// ===========================================================================

#[test]
fn test_refund_counter_increments() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client.request_refund(
        &customer,
        &100,
        &s.token_addr,
        &String::from_str(&s.env, "Reason 1"),
    );
    s.client.request_refund(
        &customer,
        &200,
        &s.token_addr,
        &String::from_str(&s.env, "Reason 2"),
    );

    assert_eq!(s.client.get_refund_counter(), 2);
}

#[test]
fn test_admin_upgrade_increments_version() {
    let s = setup();
    s.client.initialize(&s.admin);

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
fn test_upgrade_by_non_admin_fails() {
    let s = setup();
    s.client.initialize(&s.admin);

    let intruder = Address::generate(&s.env);
    let wasm_hash = s.env.deployer().upload_contract_wasm(UPGRADE_WASM);
    let result = s.client.try_upgrade(&intruder, &wasm_hash);

    assert!(result.is_err());
    assert_eq!(s.client.get_version(), 1);
}

#[test]
fn test_migration_runs_once_per_version() {
    let s = setup();
    s.client.initialize(&s.admin);

    s.client.migrate(&s.admin);
    let second = s.client.try_migrate(&s.admin);

    assert!(second.is_err());
}

#[test]
fn test_upgrade_atomicity_with_invalid_hash() {
    let s = setup();
    s.client.initialize(&s.admin);

    let invalid_hash = BytesN::from_array(&s.env, &[9u8; 32]);
    let result = s.client.try_upgrade(&s.admin, &invalid_hash);

    assert!(result.is_err());
    assert_eq!(s.client.get_version(), 1);
}
