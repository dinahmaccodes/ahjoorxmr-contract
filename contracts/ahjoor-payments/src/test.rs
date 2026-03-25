#![cfg(test)]
extern crate alloc;
use super::*;
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::token::StellarAssetClient as TokenAdminClient;
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    vec, Address, Env, String,
};

// ---------------------------------------------------------------------------
//  Test Helpers
// ---------------------------------------------------------------------------

struct TestSetup<'a> {
    env: Env,
    client: AhjoorPaymentsContractClient<'a>,
    admin: Address,
    token_addr: Address,
    token_client: TokenClient<'a>,
    token_admin_client: TokenAdminClient<'a>,
}

fn setup<'a>() -> TestSetup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorPaymentsContract, ());
    let client = AhjoorPaymentsContractClient::new(&env, &contract_id);

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

    assert_eq!(s.client.get_payment_counter(), 0);
    assert_eq!(s.client.get_max_batch_size(), 20);
    assert_eq!(s.client.get_dispute_timeout(), 7 * 24 * 60 * 60);
}

#[test]
#[should_panic(expected = "Already initialized")]
fn test_initialize_twice_panics() {
    let s = setup();
    s.client.initialize(&s.admin);
    s.client.initialize(&s.admin);
}

// ===========================================================================
//  Single Payment (Escrow) Tests
// ===========================================================================

#[test]
fn test_create_single_payment_escrow() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(
        &customer,
        &merchant,
        &250,
        &s.token_addr,
    );

    assert_eq!(payment_id, 0);
    assert_eq!(s.token_client.balance(&customer), 750);
    assert_eq!(s.token_client.balance(&merchant), 0);
    assert_eq!(s.token_client.balance(&s.client.address), 250);

    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.amount, 250);
    assert_eq!(s.client.get_payment_counter(), 1);
}

#[test]
fn test_complete_payment_releases_to_merchant() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &250, &s.token_addr);
    s.client.complete_payment(&payment_id);

    assert_eq!(s.token_client.balance(&merchant), 250);
    assert_eq!(s.token_client.balance(&s.client.address), 0);

    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Completed);
}

#[test]
#[should_panic(expected = "Payment is not pending")]
fn test_complete_already_completed_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);
    s.client.complete_payment(&payment_id);
    s.client.complete_payment(&payment_id);
}

#[test]
#[should_panic(expected = "Payment amount must be positive")]
fn test_create_payment_zero_amount_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.client.create_payment(&customer, &merchant, &0, &s.token_addr);
}

// ===========================================================================
//  Batch Payment Tests (Escrow)
// ===========================================================================

#[test]
fn test_create_batch_payments_escrow() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant1 = Address::generate(&s.env);
    let merchant2 = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &5000);

    let requests = vec![
        &s.env,
        PaymentRequest {
            merchant: merchant1.clone(),
            amount: 100,
            token: s.token_addr.clone(),
        },
        PaymentRequest {
            merchant: merchant2.clone(),
            amount: 200,
            token: s.token_addr.clone(),
        },
    ];

    let ids = s.client.create_payments_batch(&customer, &requests);

    assert_eq!(ids.len(), 2);
    assert_eq!(s.token_client.balance(&customer), 4700);
    assert_eq!(s.token_client.balance(&merchant1), 0);
    assert_eq!(s.token_client.balance(&merchant2), 0);
    assert_eq!(s.token_client.balance(&s.client.address), 300);

    let p0 = s.client.get_payment(&ids.get(0).unwrap());
    let p1 = s.client.get_payment(&ids.get(1).unwrap());
    assert_eq!(p0.status, PaymentStatus::Pending);
    assert_eq!(p1.status, PaymentStatus::Pending);
}

#[test]
#[should_panic(expected = "Batch size exceeds maximum allowed")]
fn test_batch_exceeds_max_size() {
    let s = setup();
    s.client.initialize(&s.admin);
    s.client.set_max_batch_size(&2);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &10000);

    let requests = vec![
        &s.env,
        PaymentRequest { merchant: Address::generate(&s.env), amount: 10, token: s.token_addr.clone() },
        PaymentRequest { merchant: Address::generate(&s.env), amount: 10, token: s.token_addr.clone() },
        PaymentRequest { merchant: Address::generate(&s.env), amount: 10, token: s.token_addr.clone() },
    ];
    s.client.create_payments_batch(&customer, &requests);
}

#[test]
fn test_batch_insufficient_funds_reverts_all() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant1 = Address::generate(&s.env);
    let merchant2 = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &150);

    let requests = vec![
        &s.env,
        PaymentRequest { merchant: merchant1.clone(), amount: 100, token: s.token_addr.clone() },
        PaymentRequest { merchant: merchant2.clone(), amount: 200, token: s.token_addr.clone() },
    ];

    let result = s.client.try_create_payments_batch(&customer, &requests);
    assert!(result.is_err());

    assert_eq!(s.token_client.balance(&customer), 150);
    assert_eq!(s.client.get_payment_counter(), 0);
}

// ===========================================================================
//  Dispute Lifecycle Tests
// ===========================================================================

#[test]
fn test_dispute_pending_payment() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &500, &s.token_addr);

    let reason = String::from_str(&s.env, "Wrong item delivered");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Disputed);
    assert!(s.client.is_disputed(&payment_id));

    let dispute = s.client.get_dispute(&payment_id);
    assert_eq!(dispute.payment_id, payment_id);
    assert!(!dispute.resolved);

    assert_eq!(s.token_client.balance(&s.client.address), 500);
}

#[test]
#[should_panic(expected = "Only pending payments can be disputed")]
fn test_dispute_completed_payment_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);
    s.client.complete_payment(&payment_id);

    let reason = String::from_str(&s.env, "Too late");
    s.client.dispute_payment(&customer, &payment_id, &reason);
}

#[test]
#[should_panic(expected = "Only pending payments can be disputed")]
fn test_dispute_already_disputed_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Issue 1");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    let reason2 = String::from_str(&s.env, "Issue 2");
    s.client.dispute_payment(&customer, &payment_id, &reason2);
}

#[test]
#[should_panic(expected = "Only the payment customer can dispute")]
fn test_dispute_non_customer_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    let stranger = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Not my payment");
    s.client.dispute_payment(&stranger, &payment_id, &reason);
}

#[test]
#[should_panic(expected = "Payment is not pending")]
fn test_complete_disputed_payment_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Dispute this");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    s.client.complete_payment(&payment_id);
}

// ===========================================================================
//  Dispute Resolution Tests
// ===========================================================================

#[test]
fn test_resolve_dispute_to_merchant() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &300, &s.token_addr);

    let reason = String::from_str(&s.env, "Quality issue");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    s.client.resolve_dispute(&payment_id, &true);

    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Completed);
    assert_eq!(s.token_client.balance(&merchant), 300);
    assert_eq!(s.token_client.balance(&s.client.address), 0);

    let dispute = s.client.get_dispute(&payment_id);
    assert!(dispute.resolved);
}

#[test]
fn test_resolve_dispute_to_customer() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &300, &s.token_addr);
    assert_eq!(s.token_client.balance(&customer), 700);

    let reason = String::from_str(&s.env, "Never received item");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    s.client.resolve_dispute(&payment_id, &false);

    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Refunded);
    assert_eq!(s.token_client.balance(&customer), 1000);
    assert_eq!(s.token_client.balance(&merchant), 0);
    assert_eq!(s.token_client.balance(&s.client.address), 0);

    let dispute = s.client.get_dispute(&payment_id);
    assert!(dispute.resolved);
}

#[test]
#[should_panic(expected = "Payment is not disputed")]
fn test_resolve_non_disputed_panics() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);
    s.client.resolve_dispute(&payment_id, &true);
}

// ===========================================================================
//  Escalation Tests
// ===========================================================================

#[test]
fn test_dispute_escalation_after_timeout() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.env.ledger().set_timestamp(1000);
    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    s.env.ledger().set_timestamp(2000);
    let reason = String::from_str(&s.env, "Test dispute");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    s.client.set_dispute_timeout(&3600);

    s.env.ledger().set_timestamp(3000);
    let escalated = s.client.check_escalation(&payment_id);
    assert!(!escalated);

    s.env.ledger().set_timestamp(6000);
    let escalated = s.client.check_escalation(&payment_id);
    assert!(escalated);
}

#[test]
fn test_no_escalation_for_non_disputed() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let escalated = s.client.check_escalation(&payment_id);
    assert!(!escalated);
}

#[test]
fn test_no_escalation_after_resolved() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.env.ledger().set_timestamp(1000);
    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Dispute");
    s.client.dispute_payment(&customer, &payment_id, &reason);
    s.client.resolve_dispute(&payment_id, &true);

    s.env.ledger().set_timestamp(1_000_000);
    let escalated = s.client.check_escalation(&payment_id);
    assert!(!escalated);
}

// ===========================================================================
//  Admin Config Tests
// ===========================================================================

#[test]
fn test_set_dispute_timeout() {
    let s = setup();
    s.client.initialize(&s.admin);

    assert_eq!(s.client.get_dispute_timeout(), 7 * 24 * 60 * 60);

    s.client.set_dispute_timeout(&86400);
    assert_eq!(s.client.get_dispute_timeout(), 86400);
}

#[test]
#[should_panic(expected = "Dispute timeout must be positive")]
fn test_set_dispute_timeout_zero_panics() {
    let s = setup();
    s.client.initialize(&s.admin);
    s.client.set_dispute_timeout(&0);
}

#[test]
fn test_set_max_batch_size() {
    let s = setup();
    s.client.initialize(&s.admin);

    s.client.set_max_batch_size(&50);
    assert_eq!(s.client.get_max_batch_size(), 50);
}

// ===========================================================================
//  Read Interface Tests
// ===========================================================================

#[test]
fn test_is_disputed() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);
    assert!(!s.client.is_disputed(&payment_id));

    let reason = String::from_str(&s.env, "Dispute");
    s.client.dispute_payment(&customer, &payment_id, &reason);
    assert!(s.client.is_disputed(&payment_id));

    s.client.resolve_dispute(&payment_id, &true);
    assert!(!s.client.is_disputed(&payment_id));
}

#[test]
fn test_customer_payment_tracking() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &10000);

    s.client.create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);

    let requests = vec![
        &s.env,
        PaymentRequest { merchant: Address::generate(&s.env), amount: 200, token: s.token_addr.clone() },
        PaymentRequest { merchant: Address::generate(&s.env), amount: 300, token: s.token_addr.clone() },
    ];
    s.client.create_payments_batch(&customer, &requests);

    let ids = s.client.get_customer_payments(&customer);
    assert_eq!(ids.len(), 3);
    assert_eq!(s.client.get_payment_counter(), 3);
}

#[test]
#[should_panic(expected = "Payment not found")]
fn test_get_nonexistent_payment_panics() {
    let s = setup();
    s.client.initialize(&s.admin);
    s.client.get_payment(&999);
}

// ===========================================================================
//  Full Dispute Lifecycle Test
// ===========================================================================

#[test]
fn test_full_dispute_lifecycle() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.env.ledger().set_timestamp(100);
    let pid = s.client.create_payment(&customer, &merchant, &500, &s.token_addr);
    assert_eq!(s.client.get_payment(&pid).status, PaymentStatus::Pending);
    assert_eq!(s.token_client.balance(&s.client.address), 500);

    s.env.ledger().set_timestamp(200);
    let reason = String::from_str(&s.env, "Defective product");
    s.client.dispute_payment(&customer, &pid, &reason);
    assert_eq!(s.client.get_payment(&pid).status, PaymentStatus::Disputed);
    assert!(s.client.is_disputed(&pid));

    s.client.set_dispute_timeout(&1000);
    s.env.ledger().set_timestamp(500);
    assert!(!s.client.check_escalation(&pid));

    s.env.ledger().set_timestamp(1500);
    assert!(s.client.check_escalation(&pid));

    s.client.resolve_dispute(&pid, &false);
    assert_eq!(s.client.get_payment(&pid).status, PaymentStatus::Refunded);
    assert_eq!(s.token_client.balance(&customer), 1000);
    assert_eq!(s.token_client.balance(&merchant), 0);
    assert_eq!(s.token_client.balance(&s.client.address), 0);
    assert!(!s.client.is_disputed(&pid));

    let dispute = s.client.get_dispute(&pid);
    assert!(dispute.resolved);
}

// ===========================================================================
//  Event Emission Tests
// ===========================================================================

#[test]
fn test_dispute_emits_events() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Bad service");
    s.client.dispute_payment(&customer, &0, &reason);

    let events = s.env.events().all();
    assert!(events.len() > 0, "No events emitted");
}

#[test]
fn test_resolve_emits_events() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Dispute reason");
    s.client.dispute_payment(&customer, &0, &reason);
    s.client.resolve_dispute(&0, &true);

    let events = s.env.events().all();
    assert!(events.len() >= 3, "Expected multiple events for full dispute lifecycle");
}

// ===========================================================================
//  TTL Extension Behavior Tests
// ===========================================================================

/// Verify that a Payment record stored in persistent storage survives
/// well beyond the instance TTL threshold by checking it remains accessible
/// after advancing the ledger sequence past INSTANCE_LIFETIME_THRESHOLD.
#[test]
fn test_payment_persistent_ttl_extended_on_create() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);

    // Advance ledger sequence past instance TTL threshold
    s.env.ledger().set_sequence_number(
        s.env.ledger().sequence() + 110_000,
    );

    // Payment record must still be accessible (persistent storage, individual TTL)
    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.id, payment_id);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

/// Verify that completing a payment extends its persistent TTL so the
/// completed record remains accessible for auditing.
#[test]
fn test_payment_persistent_ttl_extended_on_complete() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &100, &s.token_addr);
    s.client.complete_payment(&payment_id);

    s.env.ledger().set_sequence_number(
        s.env.ledger().sequence() + 110_000,
    );

    let payment = s.client.get_payment(&payment_id);
    assert_eq!(payment.status, PaymentStatus::Completed);
}

/// Verify that a Dispute record in temporary storage is accessible immediately
/// after creation and that the resolved flag is updated correctly.
#[test]
fn test_dispute_temporary_storage_lifecycle() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s.client.create_payment(&customer, &merchant, &200, &s.token_addr);

    let reason = String::from_str(&s.env, "Item not received");
    s.client.dispute_payment(&customer, &payment_id, &reason);

    // Dispute is in temporary storage and accessible
    let dispute = s.client.get_dispute(&payment_id);
    assert!(!dispute.resolved);
    assert_eq!(dispute.payment_id, payment_id);

    // Resolve — dispute record updated in temporary storage
    s.client.resolve_dispute(&payment_id, &false);
    let resolved_dispute = s.client.get_dispute(&payment_id);
    assert!(resolved_dispute.resolved);
}

/// Verify that CustomerPayments index in persistent storage accumulates
/// correctly across multiple payments and survives ledger advancement.
#[test]
fn test_customer_payments_persistent_ttl() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &5000);

    s.client.create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);
    s.client.create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);
    s.client.create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);

    s.env.ledger().set_sequence_number(
        s.env.ledger().sequence() + 110_000,
    );

    // Customer index must still be accessible after ledger advancement
    let ids = s.client.get_customer_payments(&customer);
    assert_eq!(ids.len(), 3);
}
