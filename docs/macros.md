# Program Macros

Quasar provides three proc macros that generate the program entrypoint, instruction dispatch, error handling, and off-chain client module: `#[program]`, `#[instruction]`, and `#[error_code]`. Together with the `dispatch!`, `no_alloc!`, and `heap_alloc!` macros from `quasar-core`, they form the complete code generation layer.

## `#[program]`

Wraps a module to generate the entrypoint, instruction dispatch, self-CPI event handler, and off-chain client module.

```rust
declare_id!("22222222222222222222222222222222222222222222");

#[program]
mod my_program {
    use super::*;

    #[instruction(discriminator = 0)]
    pub fn initialize(ctx: Ctx<Initialize>) -> Result<(), ProgramError> {
        // ...
    }

    #[instruction(discriminator = 1)]
    pub fn update(ctx: Ctx<Update>, value: u64) -> Result<(), ProgramError> {
        // ...
    }
}
```

### Generated Code

The `#[program]` macro generates the following outside the module:

1. **Program type** (`MyProgramProgram`): An account wrapper that validates the account is executable and matches the declared program ID. Implements `Program` trait. Provides `emit_event()` for self-CPI events.

2. **`EventAuthority` struct**: A PDA account type with const-evaluated address from seeds `["__event_authority"]`. Used for self-CPI event validation.

3. **Panic handler**: On SBF targets, installs a panic handler that logs "PANIC" and loops.

4. **Allocator**: Either `no_alloc!()` (panics on heap allocation) or `heap_alloc!()` (bump allocator), selected by the `alloc` feature flag.

5. **`extern crate alloc`**: Imported for both off-chain (always) and on-chain (when `alloc` feature is enabled).

Inside the module, three functions are appended:

1. **`__handle_event`**: Self-CPI event handler. Validates the `EventAuthority` PDA signer, then emits the payload via `log_data`.

2. **`__dispatch`**: Routes instruction data. Checks if the first byte is `0xFF` (event self-CPI) and routes to `__handle_event`, otherwise delegates to the `dispatch!` macro with all instruction discriminators.

3. **`entrypoint`**: The `#[no_mangle] extern "C"` SVM entrypoint. Optionally initializes the bump allocator cursor, reconstructs the instruction data slice from the SVM buffer layout, and calls `__dispatch`.

4. **`client` module**: Off-chain instruction builders (only compiled on non-SBF targets). Each instruction gets a struct with account fields and data fields, generated via macro bridge from `#[derive(Accounts)]`.

### Compile-Time Validations

The `#[program]` macro enforces at compile time:

- **Discriminator length consistency**: All instruction discriminators must have the same byte length. Mixing 1-byte and 2-byte discriminators is a compile error.
- **No duplicate discriminators**: Two instructions with the same discriminator value produce a compile error naming both functions.
- **`0xFF` prefix reserved**: Any instruction discriminator starting with `0xFF` is rejected because that prefix is reserved for self-CPI event routing.

### Entrypoint Details

The generated `entrypoint` function:

```rust
pub unsafe extern "C" fn entrypoint(ptr: *mut u8, instruction_data: *const u8) -> u64 {
    // 1. Initialize bump allocator cursor (if alloc feature)
    // 2. Reconstruct instruction data slice from SVM buffer
    // 3. Call __dispatch
    // 4. Return 0 on Ok, error code on Err
}
```

The instruction data length is read from the `u64` at offset `-8` from the data pointer — this is the SVM's input buffer layout convention. The read is technically misaligned in the Rust abstract machine, but the SVM buffer is 8-byte aligned and SBF handles unaligned access natively.

When the `alloc` feature is enabled, the first thing the entrypoint does is set the bump allocator cursor to `HEAP_START + 8`, skipping past the cursor slot itself. This eliminates a per-allocation zero-check branch.

## `#[instruction]`

Marks a function as a program instruction with an explicit discriminator. Generates the discriminator check, account parsing, argument deserialization, and optional return data handling.

```rust
#[instruction(discriminator = 0)]
pub fn make(ctx: Ctx<Make>, deposit: u64, receive: u64) -> Result<(), ProgramError> {
    // ...
}
```

### First Parameter

The first parameter must be `ctx: Ctx<T>` or `ctx: CtxWithRemaining<T>`, where `T` implements `Accounts`. The macro generates a `Context` parameter in the actual function signature and constructs the typed context via `Ctx::new(context)`.

### Argument Deserialization

Additional parameters after `ctx` are deserialized from instruction data via a generated zero-copy struct:

```rust
#[repr(C)]
#[derive(Copy, Clone)]
struct InstructionDataZc {
    deposit: PodU64,
    receive: PodU64,
}
```

A compile-time assertion ensures `align_of::<InstructionDataZc>() == 1`. The struct is pointer-cast from the instruction data bytes (after the discriminator):

```rust
let __zc = unsafe { &*(ctx.data.as_ptr() as *const InstructionDataZc) };
let deposit = __zc.deposit.get();  // PodU64 -> u64
let receive = __zc.receive.get();
```

Bounds check: the macro verifies `ctx.data.len() >= size_of::<InstructionDataZc>()` before the cast.

### Dynamic Instruction Arguments

Instructions can accept `String<N>` and `Vec<T, N>` arguments (no lifetime parameter, unlike account dynamic fields):

```rust
#[instruction(discriminator = 0)]
pub fn create_profile(ctx: Ctx<CreateProfile>, name: String<32>, tags: Vec<Address, 10>) -> Result<(), ProgramError> {
    // name: &str, tags: &[Address]
}
```

The generated `InstructionDataZc` struct uses `PodU16` descriptors for dynamic fields:

```rust
struct InstructionDataZc {
    name_len: PodU16,
    tags_count: PodU16,
}
```

Dynamic data is read from the variable tail after the ZC header:

- Length/count is read from the descriptor
- Bounds checked against the max (`N`)
- Slice is pointer-cast from the tail region
- Strings are validated as UTF-8 (on non-SBF targets; `from_utf8_unchecked` on SBF for CU savings)

### Return Data

Instructions that return a non-unit type automatically call `sol_set_return_data`:

```rust
#[instruction(discriminator = 3)]
pub fn query(ctx: Ctx<Query>) -> Result<PodU64, ProgramError> {
    Ok(PodU64::from(42))
}
```

The macro rewrites the function signature to return `Result<(), ProgramError>` and wraps the body in a closure. On `Ok`, the return value is serialized as raw bytes (pointer cast, same as events) and passed to `set_return_data`. A compile-time assertion enforces `align_of::<T>() == 1`.

### Generated Dispatch Integration

For each `#[instruction]` function, the `#[program]` macro generates a match arm in `__dispatch`:

```rust
match __disc {
    [0] => make(MakeAccounts),
    [1] => take(TakeAccounts),
    [2] => refund(RefundAccounts),
    _ => Err(ProgramError::InvalidInstructionData),
}
```

The accounts type is extracted from the `Ctx<T>` parameter. Account parsing (`parse_accounts`) happens inline in each match arm using a `MaybeUninit` buffer — accounts are only parsed for the matched instruction.

## `#[error_code]`

Generates error handling boilerplate for a program error enum.

```rust
#[error_code]
pub enum MyError {
    InsufficientFunds = 6000,
    InvalidAuthority,     // 6001
    AccountExpired,       // 6002
}
```

### Generated Code

1. **`#[repr(u32)]`** on the enum — each variant is a `u32` discriminant.

2. **`From<MyError> for ProgramError`**: Converts the error to `ProgramError::Custom(e as u32)`.

```rust
impl From<MyError> for ProgramError {
    fn from(e: MyError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
```

3. **`TryFrom<u32> for MyError`**: Reconstructs the error from a `u32` code. Returns `Err(ProgramError::InvalidArgument)` for unknown codes.

```rust
impl TryFrom<u32> for MyError {
    type Error = ProgramError;
    fn try_from(error: u32) -> Result<Self, Self::Error> {
        match error {
            6000 => Ok(MyError::InsufficientFunds),
            6001 => Ok(MyError::InvalidAuthority),
            6002 => Ok(MyError::AccountExpired),
            _ => Err(ProgramError::InvalidArgument),
        }
    }
}
```

### Discriminant Assignment

The first variant must have an explicit integer value. Subsequent variants auto-increment. Quasar's own framework errors (`QuasarError`) use the 3000 range. Program errors conventionally start at 6000 to avoid collisions.

### Usage with `require!`

```rust
require!(amount > 0, MyError::InsufficientFunds);
require_eq!(authority, expected, MyError::InvalidAuthority);
```

## `dispatch!`

The `dispatch!` macro (from `quasar-core`) is the low-level instruction router used by `__dispatch`. It is not typically used directly — the `#[program]` macro generates the call.

```rust
dispatch!(ptr, instruction_data, DISC_LEN, {
    [0] => make(MakeAccounts),
    [1] => take(TakeAccounts),
    [2] => refund(RefundAccounts),
});
```

What it does:

1. Extracts the program ID from the end of the instruction data slice (SVM convention: program ID is appended after instruction data)
2. Computes the accounts region start from the raw pointer
3. Reads `DISC_LEN` bytes as a fixed-size array from instruction data
4. Matches the discriminator against the provided arms
5. For the matched arm: allocates a `MaybeUninit<[AccountView; N]>` buffer, calls `parse_accounts` to populate it, constructs a `Context`, and calls the handler function

The `MaybeUninit` buffer avoids zero-initialization of the accounts array. `parse_accounts` returns a pointer past the last parsed account, which becomes `remaining_ptr` in the `Context` for `RemainingAccounts` support.

## `no_alloc!`

Installs a global allocator that panics on any heap allocation. Used when the `alloc` feature is disabled (the default).

```rust
no_alloc!();
```

This guarantees the entire dispatch-parse-execute-CPI path is zero-allocation. Any accidental heap allocation (e.g., a `Vec::new()` or `format!()`) immediately panics instead of silently degrading performance.

The `dealloc` implementation is a no-op (you cannot deallocate what was never allocated).

## `heap_alloc!`

Installs a bump allocator as the global allocator. Used when the `alloc` feature is enabled.

```rust
heap_alloc!();
```

The bump allocator is simple: a cursor pointer starts at `HEAP_START + 8` (set by the entrypoint) and advances on each allocation. Alignment is handled by rounding up to `layout.align()`. Deallocation is a no-op.

Constants:
- `HEAP_START_ADDRESS`: `0x300000000` (SVM convention)
- `MAX_HEAP_LENGTH`: `256 * 1024` (256 KiB)

An overflow guard rejects any single allocation larger than 256 KiB, preventing `allocation + layout.size()` from wrapping `usize`.

## `panic_handler!`

Installs a minimal panic handler on SBF targets that logs "PANIC" and enters an infinite loop. The `#[program]` macro generates this automatically, but it can also be used standalone:

```rust
panic_handler!();
```

## Discriminator Collision Detection

The `#[program]` macro maintains a list of all instruction discriminators during expansion. It checks for:

1. **Duplicate discriminators**: Two instructions with the same byte sequence produce a compile error:
   ```
   error: duplicate discriminator [0]: already used by `make`
   ```

2. **Length mismatch**: All discriminators must have the same byte length:
   ```
   error: all instruction discriminators must have the same length: expected 1 byte(s), found 2
   ```

3. **`0xFF` conflict**: Discriminators starting with `0xFF` conflict with the event self-CPI prefix:
   ```
   error: instruction `bad_fn` has a discriminator starting with 0xFF which is reserved for events
   ```

These are all compile-time errors — no runtime overhead.
