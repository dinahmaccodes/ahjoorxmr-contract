# Payment Contract Feature Implementation

## Overview

This document describes the implementation of two new features for the Ahjoor Payment Contract:

1. **Fee Collection Mechanism** - Protocol fee deduction on payment completion
2. **Idempotency Key** - Prevention of duplicate payment submissions

## Feature 1: Fee Collection Mechanism

### Problem Statement

FacilPay operates as a payment infrastructure provider but the contracts currently forward the full payment amount with no protocol fee deduction. A configurable fee mechanism is needed to sustain the protocol.

### Implementation Details

#### Storage Changes

- Added `FeeBps` (u32) to instance storage - stores fee in basis points (1 bps = 0.01%)
- Added `FeeRecipient` (Address) to instance storage - address that receives protocol fees

#### Constants

- `MAX_FEE_BPS: u32 = 500` - Maximum allowed fee is 500 bps (5%)
- `DEFAULT_FEE_BPS: u32 = 0` - Default fee is 0 bps (no fee initially)

#### Modified Functions

**`initialize(env, admin, fee_recipient, fee_bps)`**

- Now requires `fee_recipient` and `fee_bps` parameters
- Validates that `fee_bps <= MAX_FEE_BPS`
- Stores both values in instance storage

**`complete_payment(env, payment_id)`**

- Calculates fee: `fee_amount = (payment.amount * fee_bps) / 10_000`
- Transfers fee to `fee_recipient` if `fee_amount > 0`
- Updates payment amount to net amount: `merchant_amount = payment.amount - fee_amount`
- Emits `FeeCollected` event with payment_id, fee_amount, fee_recipient, and token

#### New Admin Functions

**`update_fee(env, admin, new_fee_bps)`**

- Admin-only function to update the protocol fee
- Validates `new_fee_bps <= MAX_FEE_BPS` (max 5%)
- Updates `FeeBps` in instance storage

**`update_fee_recipient(env, admin, new_fee_recipient)`**

- Admin-only function to update the fee recipient address
- Updates `FeeRecipient` in instance storage

**`get_fee_bps(env) -> u32`**

- Returns the current protocol fee in basis points

**`get_fee_recipient(env) -> Address`**

- Returns the current fee recipient address

#### New Event

```rust
pub struct FeeCollected {
    pub payment_id: u32,
    pub fee_amount: i128,
    pub fee_recipient: Address,
    pub token: Address,
}
```

### Usage Example

```rust
// Initialize with 2.5% fee
contract.initialize(&admin, &fee_recipient, &250);

// Create payment for 1000 tokens
let payment_id = contract.create_payment(
    &customer, &merchant, &1000, &token, &None, &None, &None
);

// Complete payment - fee is automatically deducted
contract.complete_payment(&payment_id);
// Result: 25 tokens to fee_recipient, 975 tokens remain for merchant

// Update fee to 1%
contract.update_fee(&admin, &100);
```

### Fee Calculation Examples

| Payment Amount | Fee (bps)  | Fee Amount | Merchant Receives |
| -------------- | ---------- | ---------- | ----------------- |
| 1,000          | 250 (2.5%) | 25         | 975               |
| 10,000         | 100 (1%)   | 100        | 9,900             |
| 5,000          | 500 (5%)   | 250        | 4,750             |
| 1,000          | 0 (0%)     | 0          | 1,000             |

---

## Feature 2: Idempotency Key

### Problem Statement

The payment creation flow does not guard against duplicate submissions. If a client submits the same payment request twice (e.g., due to a network retry), two separate payment records are created and two token transfers are executed, leading to double-charging the customer.

### Implementation Details

#### Storage Changes

- Added `IdempotencyKey(BytesN<32>)` to temporary storage
- Maps idempotency key → payment_id
- Keys expire automatically after 24 hours (~17,280 ledgers at 5s/ledger)

#### Constants

- `IDEMPOTENCY_KEY_LIFETIME_THRESHOLD: u32 = 10_000` ledgers
- `IDEMPOTENCY_KEY_BUMP_AMOUNT: u32 = 17_280` ledgers (~24 hours)

#### Modified Functions

**`create_payment(env, customer, merchant, amount, token, reference, metadata, idempotency_key)`**

- Added optional `idempotency_key: Option<BytesN<32>>` parameter
- **Before any processing**: Checks if idempotency key exists in temporary storage
- If key exists: Returns the existing payment_id immediately (no new payment created, no token transfer)
- If key doesn't exist or is None: Proceeds with normal payment creation
- After successful payment creation: Stores key → payment_id mapping in temporary storage with 24h TTL

### Key Features

1. **Backwards Compatible**: The `idempotency_key` parameter is optional
   - Existing callers can pass `None` and behavior is unchanged
   - New callers can provide a key for duplicate prevention

2. **Automatic Expiration**: Keys expire after 24 hours
   - Uses temporary storage with automatic TTL management
   - No manual cleanup required

3. **Global Scope**: Idempotency is global, not per-customer
   - Same key from different customers returns the same payment_id
   - This prevents cross-customer duplicate submissions

4. **Early Return**: Duplicate detection happens before:
   - Rate limit checks
   - Token transfers
   - Payment counter increments
   - Any state modifications

### Usage Example

```rust
// Generate a unique idempotency key (e.g., from request ID)
let key = BytesN::from_array(&env, &[1u8; 32]);

// First request - creates payment
let payment_id_1 = contract.create_payment(
    &customer, &merchant, &1000, &token,
    &None, &None, &Some(key.clone())
);
// Result: payment_id = 0, customer charged 1000 tokens

// Duplicate request (e.g., network retry) - returns existing payment
let payment_id_2 = contract.create_payment(
    &customer, &merchant, &1000, &token,
    &None, &None, &Some(key)
);
// Result: payment_id = 0 (same), customer NOT charged again

assert_eq!(payment_id_1, payment_id_2);
```

### Best Practices

1. **Key Generation**: Use a deterministic, unique identifier
   - Request ID from client
   - Hash of (customer + merchant + amount + timestamp)
   - UUID from external system

2. **Key Format**: 32-byte array (BytesN<32>)

   ```rust
   // Example: SHA256 hash of request data
   let key_data = format!("{}-{}-{}", customer, merchant, timestamp);
   let key = env.crypto().sha256(&Bytes::from_slice(&env, key_data.as_bytes()));
   ```

3. **Expiration Handling**: Keys expire after 24 hours
   - After expiration, the same key can be used for a new payment
   - Design clients to handle this gracefully

4. **Optional Usage**: Only use when needed
   - High-value transactions
   - Unreliable network conditions
   - User-facing payment flows

---

## Combined Usage Example

```rust
// Initialize contract with 2.5% fee
contract.initialize(&admin, &fee_recipient, &250);

// Create payment with idempotency protection
let idempotency_key = env.crypto().sha256(&request_id);
let payment_id = contract.create_payment(
    &customer,
    &merchant,
    &2000,
    &token,
    &None,
    &None,
    &Some(idempotency_key.clone())
);

// Complete payment - fee is deducted
contract.complete_payment(&payment_id);
// Result: 50 tokens to fee_recipient (2.5% of 2000)
//         1950 tokens remain for merchant settlement

// Retry with same key - returns existing payment, no double charge
let duplicate_id = contract.create_payment(
    &customer,
    &merchant,
    &2000,
    &token,
    &None,
    &None,
    &Some(idempotency_key)
);
assert_eq!(payment_id, duplicate_id);
```

---

## Testing

### Fee Collection Tests

- ✅ Initialize with valid fee (0-500 bps)
- ✅ Initialize with excessive fee (>500 bps) - should panic
- ✅ Fee collection on payment completion
- ✅ Zero fee handling (no transfer to fee_recipient)
- ✅ Update fee within valid range
- ✅ Update fee beyond max - should panic
- ✅ Update fee recipient
- ✅ Fee event emission
- ✅ Settlement with fee-deducted amount

### Idempotency Tests

- ✅ Duplicate prevention with same key
- ✅ Different keys create different payments
- ✅ Optional key (backwards compatibility)
- ✅ Cross-customer behavior with same key
- ✅ Idempotency with fee collection
- ✅ Full payment flow with both features

---

## Migration Notes

### Breaking Changes

- `initialize()` signature changed from `initialize(admin)` to `initialize(admin, fee_recipient, fee_bps)`
- `create_payment()` signature changed - added optional `idempotency_key` parameter

### Upgrade Path

1. Deploy new contract version
2. Call `initialize()` with desired fee configuration
3. Update client code to:
   - Pass fee parameters to `initialize()`
   - Optionally pass `None` for `idempotency_key` in `create_payment()` calls
   - Or implement idempotency key generation for duplicate prevention

### Backwards Compatibility

- Existing `create_payment()` callers can pass `None` for `idempotency_key`
- Fee can be set to 0 to maintain current behavior (no fee collection)

---

## Security Considerations

1. **Fee Cap**: Maximum fee is hardcoded at 500 bps (5%) to prevent excessive fees
2. **Admin-Only**: Fee updates require admin authentication
3. **Idempotency Scope**: Global scope prevents cross-customer exploits
4. **Temporary Storage**: Idempotency keys auto-expire, preventing storage bloat
5. **Early Validation**: Idempotency check happens before any state changes

---

## Performance Impact

1. **Fee Collection**: Minimal - one additional token transfer per completed payment
2. **Idempotency Check**: Minimal - one temporary storage lookup per payment creation
3. **Storage**: Idempotency keys use temporary storage with automatic cleanup

---

## Future Enhancements

1. **Per-Merchant Fees**: Different fee rates for different merchants
2. **Fee Tiers**: Volume-based fee discounts
3. **Fee Analytics**: Track total fees collected per token/merchant
4. **Idempotency Metrics**: Track duplicate prevention statistics
5. **Custom TTL**: Allow admin to configure idempotency key expiration time
