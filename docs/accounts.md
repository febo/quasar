# Accounts

Quasar's account system provides zero-copy, type-safe access to on-chain data. Every field in a `#[derive(Accounts)]` struct uses a wrapper type that determines what validations run at parse time. After parsing, field access is a pointer cast to the account's raw data in the SVM input buffer -- no deserialization, no allocation.

## Account Types

### `Account<T>`

The primary wrapper for validated on-chain accounts. Checks owner and discriminator at parse time. After parsing, `Account<T>` dereferences to the zero-copy companion struct (the `ZeroCopyDeref::Target`), so field access is a pointer cast past the discriminator bytes.

```rust
pub escrow: &'info mut Account<EscrowAccount>,
pub vault: &'info Account<TokenAccount>,
```

The trait bounds on `T` determine what validations and capabilities are available:

| Bound | Validation | Capability |
|-------|-----------|------------|
| `T: CheckOwner` | Owner matches expected program(s) | -- |
| `T: AccountCheck` | Discriminator + data length | -- |
| `T: ZeroCopyDeref` | -- | `Deref`/`DerefMut` to ZC companion struct |
| `T: Owner` | -- | `close()` (direct lamport drain) |
| `T: InterfaceResolve` | -- | `resolve()` for polymorphic dispatch |
| `T: QuasarAccount` | -- | `get()`/`set()` for Borsh-style access |

The internal representation is `#[repr(transparent)]` over `AccountView`:

```rust
#[repr(transparent)]
pub struct Account<T> {
    view: AccountView,
    _marker: PhantomData<T>,
}
```

Construction goes through `from_account_view` (read-only) or `from_account_view_mut` (writable). The mutable variant additionally checks the `is_writable` flag on the account.

### `Initialize<T>`

For accounts that do not exist yet. Skips owner and discriminator validation -- the account will be created via `init()` or `init_signed()`. The mutable variant checks `is_writable`.

```rust
pub escrow: &'info mut Initialize<EscrowAccount>,
```

`Initialize<T>` is also `#[repr(transparent)]` over `AccountView`. It performs no validation in `from_account_view` -- the account data is expected to be uninitialized at this point.

### `Signer`

Checks that the account has the `is_signer` flag set. Used for transaction payers, authority accounts, and any account that must prove it authorized the transaction.

```rust
pub maker: &'info mut Signer,
```

Defined via the `define_account!` macro with a single check:

```rust
define_account!(pub struct Signer => [checks::Signer]);
```

### `UncheckedAccount`

No validation at all. Used for accounts where you handle validation yourself -- PDAs owned by other programs, lamport-only destinations, etc.

```rust
pub maker: &'info mut UncheckedAccount,
```

Defined with an empty check list:

```rust
define_account!(pub struct UncheckedAccount => []);
```

### `SystemAccount`

Validates that the account is owned by the system program (all-zero address). Useful for receiving lamport transfers.

```rust
define_account!(pub struct SystemAccount => [checks::Owner]);

impl Owner for SystemAccount {
    const OWNER: Address = Address::new_from_array([0u8; 32]);
}
```

### `Sysvar<T>`

Sysvar account access. Validates the account address matches `T::ID` on construction. Dereferences directly to the sysvar data type via `borrow_unchecked` -- no `RefCell` overhead since sysvars are always read-only.

```rust
pub rent: &'info Sysvar<Rent>,
pub clock: &'info Sysvar<Clock>,
```

Access sysvar fields through `Deref`:

```rust
let lamports = self.rent.minimum_balance_unchecked(data_len);
let slot = self.clock.slot;
```

Internally, `Sysvar<T>` validates the address in `from_account_view` and then uses `borrow_unchecked` to get a reference to the sysvar data without runtime borrow tracking:

```rust
pub fn get(&self) -> &T {
    unsafe { T::from_bytes_unchecked(self.view.borrow_unchecked()) }
}
```

### `SystemProgram` / `TokenProgram` / `TokenInterface`

Program account wrappers that validate the account address matches the expected program ID and that the account is executable. Provide typed CPI methods.

```rust
pub system_program: &'info SystemProgram,
pub token_program: &'info TokenProgram,
pub token_program: &'info TokenInterface,  // accepts SPL Token or Token-2022
```

## Mutability

`&'info mut` references automatically assert the account's `is_writable` flag during parsing. `&'info` references are read-only -- no writable check.

```rust
pub payer: &'info mut Signer,        // writable + signer
pub config: &'info Account<Config>,   // read-only, owner + discriminator
```

The writable check happens inside `from_account_view_mut`:

```rust
pub fn from_account_view_mut(view: &AccountView) -> Result<&mut Self, ProgramError> {
    if !view.is_writable() {
        return Err(ProgramError::Immutable);
    }
    // ... owner + discriminator checks ...
}
```

## State Definition

### `#[account]`

Defines on-chain state. The macro generates:

1. A zero-copy companion struct (`EscrowAccountZc`) where `u64` becomes `PodU64` (alignment 1)
2. A `Deref` impl for direct field access through `Account<T>`
3. Discriminator validation via the `Discriminator` trait
4. Space calculation via the `Space` trait
5. Owner trait implementation (`Owner`)
6. `init()` and `init_signed()` methods with re-initialization protection

```rust
#[account(discriminator = 1)]
pub struct EscrowAccount {
    pub maker: Address,
    pub mint_a: Address,
    pub mint_b: Address,
    pub maker_ta_b: Address,
    pub receive: u64,
    pub bump: u8,
}
```

Discriminators are explicit integers, not sha256 hashes. All-zero discriminators are rejected at compile time because they are indistinguishable from uninitialized account data -- the `validate_discriminator_not_zero` check runs during macro expansion.

Multi-byte discriminators are supported:

```rust
#[account(discriminator = [1, 2])]
pub struct MyAccount { ... }
```

### Type Mapping

Account fields are stored as alignment-1 Pod types in the ZC companion struct:

| Rust type | ZC type | Size |
|-----------|---------|------|
| `u8` / `i8` | `u8` / `i8` | 1 |
| `u16` | `PodU16` | 2 |
| `u32` | `PodU32` | 4 |
| `u64` | `PodU64` | 8 |
| `u128` | `PodU128` | 16 |
| `i16` - `i128` | `PodI16` - `PodI128` | 2-16 |
| `bool` | `PodBool` | 1 |
| `Address` | `Address` | 32 |

Pod types use `#[repr(transparent)]` over `[u8; N]`. Arithmetic operators use wrapping semantics in release builds for CU efficiency. Use `checked_add`, `checked_sub`, `checked_mul`, `checked_div` when overflow detection matters.

### Zero-Copy Companion Struct

For a struct like `EscrowAccount`, the macro generates a `#[repr(C)]` companion struct `EscrowAccountZc` where every integer field becomes its Pod equivalent. The companion struct has a compile-time alignment assertion:

```rust
const _: () = assert!(core::mem::align_of::<EscrowAccountZc>() == 1);
```

This guarantees the ZC struct can be pointer-cast from any byte-aligned position in the account data buffer. The `ZeroCopyDeref` implementation performs the cast past the discriminator:

```rust
impl ZeroCopyDeref for EscrowAccount {
    type Target = EscrowAccountZc;

    fn deref_from(view: &AccountView) -> &Self::Target {
        unsafe { &*(view.data_ptr().add(DISC_LEN) as *const EscrowAccountZc) }
    }
}
```

Bounds safety relies on the `AccountCheck::check` implementation validating `data_len >= DISC_LEN + size_of::<ZcStruct>()` during `from_account_view`.

### Initialization

Create accounts with `init()` (for regular accounts) or `init_signed()` (for PDA-owned accounts):

```rust
EscrowAccount {
    maker: *self.maker.address(),
    mint_a: *self.mint_a.address(),
    receive,
    bump: bumps.escrow,
    // ...
}
.init_signed(
    self.escrow,
    self.maker.to_account_view(),
    Some(&**self.rent),
    &[quasar_core::cpi::Signer::from(&seeds)],
)?;
```

Re-initialization protection: `init()` checks that the discriminator region is all-zero before writing. Since all-zero discriminators are banned at compile time, uninitialized data (all zeros) can never match a valid account. An attacker cannot reinitialize an existing account because its discriminator bytes will be non-zero.

### Closing Accounts

```rust
self.escrow.close(self.maker.to_account_view())?;
```

The `close` method (available on `Account<T>` where `T: Owner`):

1. Zeroes the discriminator bytes (up to 8 bytes) to prevent account revival within the same transaction
2. Transfers all lamports to the destination
3. Reassigns the account to the system program
4. Resizes the account data to 0

```rust
pub fn close(&self, destination: &AccountView) -> Result<(), ProgramError> {
    let view = self.to_account_view();
    let zero_len = view.data_len().min(8);
    if zero_len > 0 {
        unsafe { core::ptr::write_bytes(view.data_ptr(), 0, zero_len); }
    }
    destination.set_lamports(destination.lamports() + view.lamports());
    view.set_lamports(0);
    unsafe { view.assign(&SYSTEM_PROGRAM_ID) };
    view.resize(0)?;
    Ok(())
}
```

For token/mint accounts owned by the SPL Token or Token-2022 programs, use the CPI-based `TokenClose` trait instead -- direct lamport drain would fail because the calling program does not own those accounts.

### Realloc

```rust
self.account.realloc(new_space, payer.to_account_view(), None)?;
```

Adjusts account size and transfers lamports to/from the payer to maintain rent exemption. Pass `Some(rent)` to use an already-fetched Rent sysvar instead of a syscall.

## Account Directives

Directives are specified in `#[account(...)]` attributes on fields of a `#[derive(Accounts)]` struct. The derive macro parses these into `AccountFieldAttrs` and generates the corresponding validation code.

### `seeds` + `bump`

PDA derivation. `seeds` accepts byte slices and account references (account references automatically resolve to their 32-byte address).

```rust
// find_program_address -- bump is auto-discovered and stored in the bumps struct
#[account(seeds = [b"escrow", maker], bump)]
pub escrow: &'info mut Initialize<EscrowAccount>,

// create_program_address -- cheaper when bump is already known
#[account(seeds = [b"escrow", maker], bump = escrow.bump)]
pub escrow: &'info mut Account<EscrowAccount>,
```

When `bump` has no value (`bump` alone), the macro generates a `find_program_address` call and stores the discovered bump in the bumps struct. When `bump = expr`, it uses the cheaper `create_program_address` with the provided bump value.

The bumps struct (e.g., `MakeBumps`, `TakeBumps`) captures account addresses at parse time and exposes `*_seeds()` methods that return fixed-size `[Seed; N]` arrays:

```rust
let seeds = bumps.escrow_seeds();
cpi_call.invoke_signed(&seeds)?;
```

PDA seeds are reconstructed from the captured addresses and bump -- no re-derivation needed.

### `has_one`

Cross-account validation. Checks that a field in the validated account matches the address of another account in the struct:

```rust
#[account(has_one = maker, has_one = maker_ta_b)]
pub escrow: &'info mut Account<EscrowAccount>,
pub maker: &'info mut Signer,
pub maker_ta_b: &'info mut Account<TokenAccount>,
```

Generates:

```rust
require!(escrow.maker == *maker.address(), QuasarError::HasOneMismatch);
require!(escrow.maker_ta_b == *maker_ta_b.address(), QuasarError::HasOneMismatch);
```

Custom errors can be specified with `@`:

```rust
#[account(has_one = maker @ MyError::WrongMaker)]
```

### `constraint`

Arbitrary boolean expression. Any valid Rust expression that evaluates to `bool`:

```rust
#[account(constraint = escrow.receive > 0)]
pub escrow: &'info mut Account<EscrowAccount>,
```

Generates:

```rust
require!(escrow.receive > 0, QuasarError::ConstraintViolation);
```

Custom errors:

```rust
#[account(constraint = escrow.receive > 0 @ MyError::ZeroReceive)]
```

### `address`

Validates the account's address matches an expected value:

```rust
#[account(address = EXPECTED_ADDRESS)]
pub config: &'info Account<Config>,
```

Custom errors:

```rust
#[account(address = EXPECTED_ADDRESS @ MyError::WrongConfig)]
```

## `#[derive(Accounts)]`

The derive macro generates:

1. A `Bumps` companion struct (e.g., `MakeBumps`) holding PDA bump seeds and captured addresses
2. A `ParseAccounts` implementation that validates all fields in order
3. An `AccountCount` implementation for compile-time account count
4. A `parse_accounts` method for the raw entrypoint buffer parsing (zero-copy from SVM input)
5. Seed reconstruction methods on the bumps struct

The parsing flow:

```
SVM input buffer
    |
    v
parse_accounts() -- walks raw pointers, builds AccountView array
    |
    v
ParseAccounts::parse() -- validates each AccountView
    |                      (owner, discriminator, seeds, has_one, constraint)
    v
(T, T::Bumps) -- typed struct + PDA bumps
```

Checks execute in this order: field construction (owner + discriminator), then mutation checks, then `has_one` checks, then `constraint` checks, then PDA checks. This ordering ensures that field references in `has_one` and `constraint` expressions are valid.

## Dynamic Data

For variable-length fields -- strings and arrays that can change size after initialization.

### Defining Dynamic Fields

Use `String<'a, N>` for variable-length UTF-8 strings and `Vec<'a, T, N>` for variable-length arrays. `N` is the maximum byte length (for String) or element count (for Vec). The struct must have a lifetime parameter.

```rust
#[account(discriminator = 5)]
pub struct Profile<'a> {
    pub owner: Address,         // fixed field
    pub score: u64,             // fixed field
    pub name: String<'a, 32>,   // up to 32 bytes
    pub tags: Vec<'a, Address, 10>,  // up to 10 addresses
}
```

`String` and `Vec` are marker types (`PhantomData`). The macro transforms them -- `String<'a, 32>` becomes `&'a str`, `Vec<'a, Address, 10>` becomes `&'a [Address]` in the generated code.

### Memory Layout

```
[discriminator][ZC header: fixed fields + PodU16 length descriptors][variable tail: packed data]
```

For the `Profile` example:

```
[disc: 1 byte][owner: 32 bytes][score: 8 bytes (PodU64)][name_len: 2 bytes (PodU16)][tags_count: 2 bytes (PodU16)][name bytes...][tag elements...]
```

The ZC header has a fixed size regardless of current field values. A `PodU16` descriptor per dynamic field stores the current length/count. The variable tail packs all dynamic data contiguously.

### Rules (Compile-Time Enforced)

1. **Fixed fields must precede all dynamic fields.** The macro checks field ordering and emits a compile error if a fixed field follows a dynamic field.
2. **Vec element types must be fixed-size, alignment-1 types.** No nested `String`/`Vec`. The macro validates this recursively.
3. **The struct must have a lifetime parameter.** Dynamic fields reference data in the account buffer, requiring a lifetime.

### Reading Dynamic Fields

Individual accessors -- each re-casts the ZC header:

```rust
let name: &str = account.name();
let tags: &[Address] = account.tags();
```

Batch accessor -- single ZC cast, one linear scan. O(N) instead of O(N per field):

```rust
let fields = account.dynamic_fields();
// fields: ProfileDynamicFields { name: &str, tags: &[Address] }
```

### Writing Dynamic Fields

Individual setters -- each triggers realloc + memmove for subsequent fields:

```rust
account.set_name(&payer, "alice")?;
account.set_tags(&payer, &[addr1, addr2])?;
```

Batch setter -- stack buffer, one realloc, zero memmove:

```rust
// Update name, keep existing tags
account.set_dynamic_fields(&payer, Some("alice"), None)?;

// Update both
account.set_dynamic_fields(&payer, Some("bob"), Some(&[addr1]))?;
```

The batch setter copies all field data (old for `None`, new for `Some`) into a `[0u8; MAX_TAIL]` stack buffer, does one realloc, and one `copy_from_slice` back. No memmove overlap issues.

### In-Place Mutation (Vec Only)

Mutate existing Vec elements without realloc (element count stays the same):

```rust
account.tags_mut()[0] = new_address;
```

### Dynamic Instruction Arguments

Instruction arguments support `String<N>` and `Vec<T, N>` (no lifetime -- instruction data is immutable):

```rust
#[instruction(discriminator = 0)]
pub fn create_profile(ctx: Ctx<CreateProfile>, name: String<32>, tags: Vec<Address, 10>) -> Result<(), ProgramError> {
    // name: &str, tags: &[Address] -- already parsed from instruction data
}
```

Instruction data layout: `[discriminator][ZC header with PodU16 descriptors][variable tail]`. Bounds and max-length checks are generated automatically. String data is validated as UTF-8.

## Remaining Accounts

Access accounts beyond those declared in the `#[derive(Accounts)]` struct. The `RemainingAccounts` struct is constructed lazily -- zero allocation in the dispatch hot path.

To use remaining accounts, the instruction must use `CtxWithRemaining` instead of `Ctx`:

```rust
#[instruction(discriminator = 0)]
pub fn process(ctx: CtxWithRemaining<Process>) -> Result<(), ProgramError> {
    let remaining = ctx.remaining_accounts();

    // Iterate sequentially (builds index for O(1) dup resolution)
    for account in remaining.iter() {
        // account: AccountView
    }

    // Random access by index (O(n) -- walks from start)
    let third = remaining.get(2);

    // Check if there are remaining accounts
    if remaining.is_empty() { ... }
}
```

### Implementation Details

`RemainingAccounts` uses a boundary pointer (end of accounts region in the SVM buffer) instead of a count. This is computed from the instruction data pointer: `ix_data_ptr - sizeof(u64)`.

The iterator (`RemainingIter`) maintains a `MaybeUninit<[AccountView; 64]>` cache for O(1) duplicate account resolution -- the same pattern used by the entrypoint's declared accounts parser. When a duplicate account is encountered (indicated by its `borrow_state` field), the iterator resolves it by looking up either the declared accounts slice (for declared account duplicates) or its own cache (for previously-yielded remaining accounts).

Random access via `get(index)` is O(n) because it walks from the start of the buffer each time. For sequential access, `iter()` is preferred.

## Interface Accounts (Multi-Owner)

When an account can be owned by multiple programs, use interface types instead of single-owner types.

### Built-In Interface Types

| Type | Accepts | Deref target |
|------|---------|-------------|
| `Account<TokenAccount>` | SPL Token only | `TokenAccountState` |
| `Account<InterfaceTokenAccount>` | SPL Token **or** Token-2022 | `TokenAccountState` |
| `Account<MintAccount>` | SPL Token only | `MintAccountState` |
| `Account<InterfaceMintAccount>` | SPL Token **or** Token-2022 | `MintAccountState` |

```rust
// Single-owner -- only accepts SPL Token accounts
pub vault: &'info Account<TokenAccount>,

// Interface -- accepts either SPL Token or Token-2022
pub vault: &'info Account<InterfaceTokenAccount>,
```

Both types deref to the same `TokenAccountState` -- field access is identical:

```rust
let mint = ctx.accounts.vault.mint();
let amount = ctx.accounts.vault.amount();
```

### Building Custom Interface Types

Interface accounts use two traits instead of `Owner`:

- **`CheckOwner`** -- validates the account is owned by one of the accepted programs
- **`ZeroCopyDeref`** -- pointer-casts account data to a `#[repr(C)]` struct

For single-owner types, `CheckOwner` is implemented automatically via a blanket impl on `Owner`. Interface types implement `CheckOwner` directly with explicit comparison chains:

```rust
pub struct InterfaceTokenAccount;

impl CheckOwner for InterfaceTokenAccount {
    fn check_owner(view: &AccountView) -> Result<(), ProgramError> {
        if !view.owned_by(&SPL_TOKEN_ID) && !view.owned_by(&TOKEN_2022_ID) {
            return Err(ProgramError::IllegalOwner);
        }
        Ok(())
    }
}

impl AccountCheck for InterfaceTokenAccount {
    fn check(view: &AccountView) -> Result<(), ProgramError> {
        if view.data_len() < TokenAccountState::LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        Ok(())
    }
}

impl ZeroCopyDeref for InterfaceTokenAccount {
    type Target = TokenAccountState;

    fn deref_from(view: &AccountView) -> &Self::Target {
        unsafe { &*(view.data_ptr() as *const TokenAccountState) }
    }

    fn deref_from_mut(view: &AccountView) -> &mut Self::Target {
        unsafe { &mut *(view.data_ptr() as *mut TokenAccountState) }
    }
}
```

This pattern works for any multi-owner account. For example, a custom oracle interface:

```rust
pub struct OracleInterface;

impl CheckOwner for OracleInterface {
    fn check_owner(view: &AccountView) -> Result<(), ProgramError> {
        if !view.owned_by(&PYTH_PROGRAM_ID) && !view.owned_by(&SWITCHBOARD_PROGRAM_ID) {
            return Err(ProgramError::IllegalOwner);
        }
        Ok(())
    }
}
```

### Polymorphic Dispatch with `resolve()`

When interface accounts have different layouts depending on which program owns them, implement `InterfaceResolve` to dispatch at runtime:

```rust
pub enum OraclePrice<'a> {
    Pyth(&'a PythPriceState),
    Switchboard(&'a SwitchboardState),
}

impl InterfaceResolve for OracleInterface {
    type Resolved<'a> = OraclePrice<'a>;

    fn resolve<'a>(view: &'a AccountView) -> Result<OraclePrice<'a>, ProgramError> {
        if view.owned_by(&PYTH_PROGRAM_ID) {
            Ok(OraclePrice::Pyth(unsafe {
                &*(view.data_ptr() as *const PythPriceState)
            }))
        } else {
            Ok(OraclePrice::Switchboard(unsafe {
                &*(view.data_ptr() as *const SwitchboardState)
            }))
        }
    }
}
```

Then in your instruction:

```rust
pub oracle: &'info Account<OracleInterface>,
```

```rust
match ctx.accounts.oracle.resolve()? {
    OraclePrice::Pyth(price) => { /* read Pyth fields */ }
    OraclePrice::Switchboard(price) => { /* read Switchboard fields */ }
}
```

The owner check runs once during account parsing. `resolve()` is a second pointer cast -- no re-validation, no allocation.

## Core Traits Reference

| Trait | Purpose | Implemented by |
|-------|---------|----------------|
| `Owner` | Declares expected owner address | `#[account]` macro |
| `CheckOwner` | Validates account owner (blanket impl for `Owner`) | Interface types implement directly |
| `AccountCheck` | Discriminator + data length validation | `#[account]` macro, `define_account!` |
| `Discriminator` | Byte-level discriminator prefix | `#[account]`, `#[instruction]` macros |
| `Space` | Account data size (excl. discriminator) | `#[account]` macro |
| `ZeroCopyDeref` | Pointer cast to ZC companion struct | `#[account]` macro |
| `InterfaceResolve` | Polymorphic dispatch based on owner | Manual impl |
| `QuasarAccount` | Borsh-style serialize/deserialize | Manual impl |
| `ParseAccounts` | Parse + validate from `AccountView` slice | `#[derive(Accounts)]` |
| `AccountCount` | Compile-time account count | `#[derive(Accounts)]` |
| `AsAccountView` | Access underlying `AccountView` | All account types |
| `FromAccountView` | Construct from raw `AccountView` | All account types |
