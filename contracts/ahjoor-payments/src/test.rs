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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &250, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &250, &s.token_addr);
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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);
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
    s.client
        .create_payment(&customer, &merchant, &0, &s.token_addr);
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
        PaymentRequest {
            merchant: Address::generate(&s.env),
            amount: 10,
            token: s.token_addr.clone(),
        },
        PaymentRequest {
            merchant: Address::generate(&s.env),
            amount: 10,
            token: s.token_addr.clone(),
        },
        PaymentRequest {
            merchant: Address::generate(&s.env),
            amount: 10,
            token: s.token_addr.clone(),
        },
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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &500, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);
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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &300, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &300, &s.token_addr);
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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);
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
    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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
    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);
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

    s.client
        .create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);

    let requests = vec![
        &s.env,
        PaymentRequest {
            merchant: Address::generate(&s.env),
            amount: 200,
            token: s.token_addr.clone(),
        },
        PaymentRequest {
            merchant: Address::generate(&s.env),
            amount: 300,
            token: s.token_addr.clone(),
        },
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
    let pid = s
        .client
        .create_payment(&customer, &merchant, &500, &s.token_addr);
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

    s.client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

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

    s.client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

    let reason = String::from_str(&s.env, "Dispute reason");
    s.client.dispute_payment(&customer, &0, &reason);
    s.client.resolve_dispute(&0, &true);

    let events = s.env.events().all();
    assert!(
        events.len() >= 3,
        "Expected multiple events for full dispute lifecycle"
    );
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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);

    // Advance ledger sequence past instance TTL threshold
    s.env
        .ledger()
        .set_sequence_number(s.env.ledger().sequence() + 110_000);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &100, &s.token_addr);
    s.client.complete_payment(&payment_id);

    s.env
        .ledger()
        .set_sequence_number(s.env.ledger().sequence() + 110_000);

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

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &200, &s.token_addr);

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

    s.client
        .create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);
    s.client
        .create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);
    s.client
        .create_payment(&customer, &Address::generate(&s.env), &100, &s.token_addr);

    s.env
        .ledger()
        .set_sequence_number(s.env.ledger().sequence() + 110_000);

    // Customer index must still be accessible after ledger advancement
    let ids = s.client.get_customer_payments(&customer);
    assert_eq!(ids.len(), 3);
}

// ===========================================================================
//  Multi-Token Payment Tests
// ===========================================================================

/// Mock oracle contract used in tests.
/// Stores a single price (scaled by 10^7) and timestamp set by the test.
mod mock_oracle {
    use crate::PriceData;
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    enum OracleKey {
        Price,
        Ts,
    }

    #[contract]
    pub struct MockOracle;

    #[contractimpl]
    impl MockOracle {
        /// Test helper: set the price and timestamp the oracle will return.
        pub fn set_price(env: Env, price: i128, timestamp: u64) {
            env.storage().instance().set(&OracleKey::Price, &price);
            env.storage().instance().set(&OracleKey::Ts, &timestamp);
        }

        /// Reflector-compatible: returns the stored price regardless of base/quote.
        pub fn lastprice(env: Env, _base: Address, _quote: Address) -> Option<PriceData> {
            let price: i128 = env.storage().instance().get(&OracleKey::Price)?;
            let timestamp: u64 = env.storage().instance().get(&OracleKey::Ts)?;
            Some(PriceData { price, timestamp })
        }
    }
}

use mock_oracle::MockOracle;
use soroban_sdk::token::StellarAssetClient as SacClient;

struct MultiTokenSetup<'a> {
    env: Env,
    client: AhjoorPaymentsContractClient<'a>,
    admin: Address,
    /// USDC token (settlement currency)
    usdc_addr: Address,
    usdc_client: TokenClient<'a>,
    usdc_admin: TokenAdminClient<'a>,
    /// XLM-like payment token
    xlm_addr: Address,
    xlm_admin: TokenAdminClient<'a>,
    xlm_client: TokenClient<'a>,
    oracle_addr: Address,
}

fn setup_multi_token<'a>() -> MultiTokenSetup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(AhjoorPaymentsContract, ());
    let client = AhjoorPaymentsContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    // USDC token
    let usdc_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let usdc_client = TokenClient::new(&env, &usdc_addr);
    let usdc_admin = TokenAdminClient::new(&env, &usdc_addr);

    // XLM-like payment token
    let xlm_addr = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let xlm_admin = TokenAdminClient::new(&env, &xlm_addr);
    let xlm_client = TokenClient::new(&env, &xlm_addr);

    // Mock oracle
    let oracle_addr = env.register(MockOracle, ());

    client.initialize(&admin);
    // max_oracle_age = 300 seconds
    client.set_oracle(&oracle_addr, &usdc_addr, &300u64);

    MultiTokenSetup {
        env,
        client,
        admin,
        usdc_addr,
        usdc_client,
        usdc_admin,
        xlm_addr,
        xlm_admin,
        xlm_client,
        oracle_addr,
    }
}

/// Helper: set oracle price (scaled by 10^7) and ledger timestamp.
fn set_oracle_price(s: &MultiTokenSetup, price: i128, ts: u64) {
    use mock_oracle::MockOracleClient;
    let oc = MockOracleClient::new(&s.env, &s.oracle_addr);
    oc.set_price(&price, &ts);
    s.env.ledger().set_timestamp(ts);
}

// ---------------------------------------------------------------------------

/// Direct USDC payment (payment_token == usdc_token) bypasses oracle entirely.
#[test]
fn test_multi_token_usdc_fallback() {
    let s = setup_multi_token();

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.usdc_admin.mint(&customer, &1000);

    let pid = s
        .client
        .create_payment_multi_token(&customer, &merchant, &500, &s.usdc_addr, &50);

    // Funds escrowed in contract
    assert_eq!(s.usdc_client.balance(&customer), 500);
    assert_eq!(s.usdc_client.balance(&s.client.address), 500);

    let payment = s.client.get_payment(&pid);
    assert_eq!(payment.amount, 500);
    assert_eq!(payment.token, s.usdc_addr);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

/// XLM payment: oracle price = 0.10 USDC per XLM (price = 1_000_000 in 10^7).
/// Customer wants to pay 100 USDC → needs 1_000 XLM.
#[test]
fn test_multi_token_xlm_payment_correct_amount() {
    let s = setup_multi_token();

    // price = 0.10 USDC/XLM → 10^7 * 0.10 = 1_000_000
    set_oracle_price(&s, 1_000_000, 1000);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    // Customer needs 1000 XLM for 100 USDC
    s.xlm_admin.mint(&customer, &2000);

    let pid = s
        .client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);

    // required_token_amount = 100 * 10_000_000 / 1_000_000 = 1000
    assert_eq!(s.xlm_client.balance(&customer), 1000);
    assert_eq!(s.xlm_client.balance(&s.client.address), 1000);

    let payment = s.client.get_payment(&pid);
    // Payment recorded in USDC terms
    assert_eq!(payment.amount, 100);
    assert_eq!(payment.token, s.usdc_addr);
    assert_eq!(payment.status, PaymentStatus::Pending);
}

/// Oracle price = 0.50 USDC/XLM (price = 5_000_000).
/// Customer pays 50 USDC → needs 100 XLM.
#[test]
fn test_multi_token_different_rate() {
    let s = setup_multi_token();

    // 0.50 USDC/XLM → 5_000_000
    set_oracle_price(&s, 5_000_000, 500);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &500);

    s.client
        .create_payment_multi_token(&customer, &merchant, &50, &s.xlm_addr, &100);

    // required = 50 * 10_000_000 / 5_000_000 = 100
    assert_eq!(s.xlm_client.balance(&customer), 400);
    assert_eq!(s.xlm_client.balance(&s.client.address), 100);
}

/// Stale oracle price (age > max_oracle_age) must be rejected.
#[test]
#[should_panic(expected = "Oracle price is stale")]
fn test_multi_token_stale_oracle_rejected() {
    let s = setup_multi_token();

    // Price timestamp = 0, current ledger = 400 → age = 400 > max_oracle_age(300)
    use mock_oracle::MockOracleClient;
    let oc = MockOracleClient::new(&s.env, &s.oracle_addr);
    oc.set_price(&1_000_000i128, &0u64);
    s.env.ledger().set_timestamp(400);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &2000);

    s.client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);
}

/// Unavailable oracle (no price set) must be rejected.
#[test]
#[should_panic(expected = "Oracle price unavailable")]
fn test_multi_token_oracle_unavailable() {
    let s = setup_multi_token();
    // Oracle has no price set — lastprice returns None

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &2000);

    s.client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);
}

/// Zero slippage tolerance: exact integer division must not deviate.
/// price = 10_000_000 (1.0 USDC/XLM) → required = amount_usdc exactly.
#[test]
fn test_multi_token_zero_slippage_exact_rate() {
    let s = setup_multi_token();

    // 1.0 USDC/XLM → 10_000_000
    set_oracle_price(&s, 10_000_000, 100);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &500);

    s.client
        .create_payment_multi_token(&customer, &merchant, &200, &s.xlm_addr, &0);

    // required = 200 * 10_000_000 / 10_000_000 = 200 — no deviation
    assert_eq!(s.xlm_client.balance(&s.client.address), 200);
}

/// set_oracle rejects max_oracle_age = 0.
#[test]
#[should_panic(expected = "max_oracle_age must be positive")]
fn test_set_oracle_zero_age_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(AhjoorPaymentsContract, ());
    let client = AhjoorPaymentsContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let oracle = Address::generate(&env);
    let usdc = Address::generate(&env);
    client.initialize(&admin);
    client.set_oracle(&oracle, &usdc, &0u64);
}

/// get_oracle_address / get_usdc_token / get_max_oracle_age return stored values.
#[test]
fn test_get_oracle_config() {
    let s = setup_multi_token();
    assert_eq!(s.client.get_oracle_address(), s.oracle_addr);
    assert_eq!(s.client.get_usdc_token(), s.usdc_addr);
    assert_eq!(s.client.get_max_oracle_age(), 300u64);
}

/// Multi-token payment emits MultiTokenPaymentCreated event.
#[test]
fn test_multi_token_emits_event() {
    let s = setup_multi_token();

    // 0.10 USDC/XLM
    set_oracle_price(&s, 1_000_000, 100);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &2000);

    s.client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);

    let events = s.env.events().all();
    assert!(events.len() > 0);
}

/// Multi-token payment counter increments correctly.
#[test]
fn test_multi_token_payment_counter() {
    let s = setup_multi_token();
    set_oracle_price(&s, 10_000_000, 100);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &5000);

    s.client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);
    s.client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);

    assert_eq!(s.client.get_payment_counter(), 2);
}

/// Multi-token payment is tracked in customer payment index.
#[test]
fn test_multi_token_customer_tracking() {
    let s = setup_multi_token();
    set_oracle_price(&s, 10_000_000, 100);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.xlm_admin.mint(&customer, &5000);

    s.client
        .create_payment_multi_token(&customer, &merchant, &100, &s.xlm_addr, &50);
    s.client
        .create_payment_multi_token(&customer, &merchant, &200, &s.xlm_addr, &50);

    let ids = s.client.get_customer_payments(&customer);
    assert_eq!(ids.len(), 2);
}

// ===========================================================================
//  Token Transfer Integration Tests
// ===========================================================================

/// Verify customer balance decreases on payment creation
#[test]
fn test_token_transfer_on_payment_creation() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let initial_balance = s.token_client.balance(&customer);
    assert_eq!(initial_balance, 1000);

    s.client
        .create_payment(&customer, &merchant, &250, &s.token_addr);

    let final_balance = s.token_client.balance(&customer);
    assert_eq!(final_balance, 750);
}

/// Verify contract holds escrowed funds
#[test]
fn test_contract_holds_escrow() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client
        .create_payment(&customer, &merchant, &250, &s.token_addr);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 250);
}

/// Verify merchant receives tokens on payment completion
#[test]
fn test_token_transfer_on_payment_completion() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &250, &s.token_addr);
    s.client.complete_payment(&payment_id);

    let merchant_balance = s.token_client.balance(&merchant);
    assert_eq!(merchant_balance, 250);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 0);
}

/// Verify customer receives tokens on refund
#[test]
fn test_token_transfer_on_refund() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &250, &s.token_addr);
    s.client.dispute_payment(
        &customer,
        &payment_id,
        &String::from_str(&s.env, "test refund"),
    );
    s.client.resolve_dispute(&payment_id, &false); // Release to customer

    let customer_balance = s.token_client.balance(&customer);
    assert_eq!(customer_balance, 1000);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 0);
}

/// Verify batch payment transfers total amount
#[test]
fn test_token_transfer_on_batch_payment() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant1 = Address::generate(&s.env);
    let merchant2 = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payments = soroban_sdk::vec![
        &s.env,
        PaymentRequest {
            merchant: merchant1,
            amount: 250,
            token: s.token_addr.clone(),
        },
        PaymentRequest {
            merchant: merchant2,
            amount: 350,
            token: s.token_addr.clone(),
        },
    ];

    s.client.create_payments_batch(&customer, &payments);

    let customer_balance = s.token_client.balance(&customer);
    assert_eq!(customer_balance, 400);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 600);
}

/// Verify dispute resolution transfers to merchant
#[test]
fn test_token_transfer_on_dispute_resolution_to_merchant() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &250, &s.token_addr);
    s.client.dispute_payment(
        &customer,
        &payment_id,
        &String::from_str(&s.env, "Item not received"),
    );
    s.client.resolve_dispute(&payment_id, &true); // Release to merchant

    let merchant_balance = s.token_client.balance(&merchant);
    assert_eq!(merchant_balance, 250);

    let customer_balance = s.token_client.balance(&customer);
    assert_eq!(customer_balance, 750);
}

/// Verify multiple payments track balances correctly
#[test]
fn test_token_balance_tracking_multiple_payments() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    let payment1 = s
        .client
        .create_payment(&customer, &merchant, &200, &s.token_addr);
    let payment2 = s
        .client
        .create_payment(&customer, &merchant, &300, &s.token_addr);

    let customer_balance = s.token_client.balance(&customer);
    assert_eq!(customer_balance, 500);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 500);

    s.client.complete_payment(&payment1);

    let merchant_balance = s.token_client.balance(&merchant);
    assert_eq!(merchant_balance, 200);

    let contract_balance = s.token_client.balance(&s.client.address);
    assert_eq!(contract_balance, 300);
}

// ===========================================================================
//  Admin Transfer Tests
// ===========================================================================

#[test]
fn test_propose_admin_transfer() {
    let s = setup();
    s.client.initialize(&s.admin);

    let new_admin = Address::generate(&s.env);
    s.client.propose_admin_transfer(&new_admin);

    assert_eq!(s.client.get_admin(), s.admin);
    assert_eq!(s.client.get_proposed_admin(), Some(new_admin));
}

#[test]
fn test_accept_admin_role() {
    let s = setup();
    s.client.initialize(&s.admin);

    let new_admin = Address::generate(&s.env);
    s.client.propose_admin_transfer(&new_admin);
    s.client.accept_admin_role();

    assert_eq!(s.client.get_admin(), new_admin);
    assert_eq!(s.client.get_proposed_admin(), None);
}

#[test]
#[should_panic(expected = "No admin transfer proposed")]
fn test_accept_admin_role_without_proposal_panics() {
    let s = setup();
    s.client.initialize(&s.admin);
    s.client.accept_admin_role();
}

#[test]
fn test_admin_transfer_emits_events() {
    let s = setup();
    s.client.initialize(&s.admin);

    let new_admin = Address::generate(&s.env);
    s.client.propose_admin_transfer(&new_admin);

    let events = s.env.events().all();
    assert!(events.len() > 0);

    s.client.accept_admin_role();

    let events = s.env.events().all();
    assert!(events.len() > 0);
}

#[test]
fn test_get_admin_returns_current_admin() {
    let s = setup();
    s.client.initialize(&s.admin);

    assert_eq!(s.client.get_admin(), s.admin);
}

#[test]
fn test_get_proposed_admin_returns_none_when_no_proposal() {
    let s = setup();
    s.client.initialize(&s.admin);

    assert_eq!(s.client.get_proposed_admin(), None);
}

// ===========================================================================
//  Pause Mechanism Tests
// ===========================================================================

#[test]
fn test_admin_can_pause_and_resume_contract() {
    let s = setup();
    s.client.initialize(&s.admin);

    let reason = String::from_str(&s.env, "Emergency maintenance");
    s.client.pause_contract(&s.admin, &reason);

    assert_eq!(s.client.is_paused(), true);
    assert_eq!(s.client.get_pause_reason(), reason);

    s.client.resume_contract(&s.admin);
    assert_eq!(s.client.is_paused(), false);
    assert_eq!(s.client.get_pause_reason(), String::from_str(&s.env, ""));
}

#[test]
fn test_non_admin_cannot_pause_or_resume() {
    let s = setup();
    s.client.initialize(&s.admin);

    let attacker = Address::generate(&s.env);
    let pause_res = s
        .client
        .try_pause_contract(&attacker, &String::from_str(&s.env, "malicious"));
    assert!(pause_res.is_err());

    s.client
        .pause_contract(&s.admin, &String::from_str(&s.env, "incident"));

    let resume_res = s.client.try_resume_contract(&attacker);
    assert!(resume_res.is_err());
}

#[test]
fn test_write_functions_blocked_when_paused_reads_still_work() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client
        .pause_contract(&s.admin, &String::from_str(&s.env, "Emergency"));

    let create_res = s
        .client
        .try_create_payment(&customer, &merchant, &100, &s.token_addr);
    assert!(create_res.is_err());

    assert_eq!(s.client.get_payment_counter(), 0);
    assert_eq!(s.client.get_admin(), s.admin);
}

#[test]
fn test_recovery_after_resume() {
    let s = setup();
    s.client.initialize(&s.admin);

    let customer = Address::generate(&s.env);
    let merchant = Address::generate(&s.env);
    s.token_admin_client.mint(&customer, &1000);

    s.client
        .pause_contract(&s.admin, &String::from_str(&s.env, "Emergency"));
    s.client.resume_contract(&s.admin);

    let payment_id = s
        .client
        .create_payment(&customer, &merchant, &200, &s.token_addr);
    assert_eq!(payment_id, 0);
}
