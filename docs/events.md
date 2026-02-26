# Events

Quasar events provide two emission paths with different trust and cost tradeoffs: log-based emission via `sol_log_data` (~100 CU) and self-CPI emission (~1,000 CU) that is resistant to spoofing by other programs.

## Defining Events

The `#[event]` attribute macro turns a struct into an emittable event. Discriminators are explicit integers, not hashed.

```rust
#[event(discriminator = 0)]
pub struct MakeEvent {
    pub escrow: Address,
    pub maker: Address,
    pub deposit: u64,
    pub receive: u64,
}
```

The macro generates:

1. `#[repr(C)]` on the struct
2. A compile-time assertion that `size_of::<MakeEvent>()` equals the sum of field sizes (no padding allowed)
3. An `Event` trait implementation with `DISCRIMINATOR`, `DATA_SIZE`, `write_data()`, and `emit()`
4. An `emit_log()` convenience method

### Supported Field Types

Event fields must be one of these fixed-size types:

| Type | Size (bytes) |
|------|-------------|
| `u8`, `i8`, `bool` | 1 |
| `u16`, `i16` | 2 |
| `u32`, `i32` | 4 |
| `u64`, `i64` | 8 |
| `u128`, `i128` | 16 |
| `Address` | 32 |

Dynamic fields (`String`, `Vec`) are not supported in events.

### Multi-byte Discriminators

Like account and instruction discriminators, event discriminators can be multi-byte:

```rust
#[event(discriminator = [1, 2])]
pub struct TransferEvent {
    pub from: Address,
    pub to: Address,
    pub amount: u64,
}
```

## Event Discriminators

Quasar uses explicit integer discriminators rather than sha256 hashes. The discriminator is stored as a `&'static [u8]` constant on the `Event` trait implementation:

```rust
// For discriminator = 0 -> DISCRIMINATOR = &[0]
// For discriminator = [1, 2] -> DISCRIMINATOR = &[1, 2]
```

The `0xFF` byte prefix is reserved for self-CPI event payloads. The `#[program]` macro validates at compile time that no instruction discriminator starts with `0xFF`, preventing collisions between instruction dispatch and event handling.

## The Event Trait

Every `#[event]` struct implements:

```rust
pub trait Event {
    const DISCRIMINATOR: &'static [u8];
    const DATA_SIZE: usize;
    fn write_data(&self, buf: &mut [u8]);
    fn emit(&self, f: impl FnOnce(&[u8]) -> Result<(), ProgramError>) -> Result<(), ProgramError>;
}
```

### `write_data` Serialization

`write_data` uses `copy_nonoverlapping` (memcpy) from the `#[repr(C)]` struct directly into the output buffer. The compile-time no-padding assertion guarantees that the struct's memory layout matches the serialized format byte-for-byte:

```rust
fn write_data(&self, buf: &mut [u8]) {
    unsafe {
        core::ptr::copy_nonoverlapping(
            self as *const Self as *const u8,
            buf.as_mut_ptr(),
            DATA_SIZE,
        );
    }
}
```

No field-by-field serialization, no encoding overhead.

### `emit` Method

The `emit` method builds a self-CPI payload: a `MaybeUninit` stack buffer of size `1 + disc_len + data_size`, writes the `0xFF` prefix byte, copies the discriminator, calls `write_data` for the payload, then passes the buffer to the provided closure (which performs the actual CPI call).

```rust
fn emit(&self, f: impl FnOnce(&[u8]) -> Result<(), ProgramError>) -> Result<(), ProgramError> {
    // Stack buffer layout: [0xFF][discriminator bytes][event data]
    let mut buf = MaybeUninit::<[u8; 1 + DISC_LEN + DATA_SIZE]>::uninit();
    // ... write 0xFF prefix, discriminator, event data ...
    f(unsafe { buf.assume_init_ref() })
}
```

## Log-Based Emission (`emit!`)

The `emit!` macro calls `sol_log_data` with the discriminator and serialized event data concatenated into a single stack-allocated buffer. Cost: ~100 CU.

```rust
emit!(MakeEvent {
    escrow: *ctx.accounts.escrow.address(),
    maker: *ctx.accounts.maker.address(),
    deposit: 100,
    receive: 200,
});
```

Under the hood, `emit!` calls `emit_log()` on the event struct. `emit_log()` allocates a `[0u8; disc_len + data_size]` buffer on the stack, copies the discriminator and serialized data into it, and calls `log_data`:

```rust
pub fn emit_log(&self) {
    let mut buf = [0u8; TOTAL_BUF_SIZE];
    buf[..DISC_LEN].copy_from_slice(Self::DISCRIMINATOR);
    Self::write_data(self, &mut buf[DISC_LEN..]);
    quasar_core::log::log_data(&[&buf]);
}
```

`log_data` calls the `sol_log_data` syscall directly on SBF targets. On non-SBF targets (tests), it uses `black_box` to prevent optimization.

### When to Use Log-Based Events

- Indexing where trust in the emitting program is not required
- High-frequency events where CU budget is tight
- Events that do not need to be verified on-chain by other programs

### Limitation

Any program can call `sol_log_data` with arbitrary bytes. A malicious program invoked via CPI could emit fake events that look identical to yours. Log-based events are **spoofable**.

## Self-CPI Emission (`program.emit_event()`)

Self-CPI events are not spoofable because the event payload is delivered through a CPI call back into the emitting program, and the callee validates a PDA signer that only it can derive.

```rust
program.emit_event(&event, &event_authority)?;
```

Cost: ~1,000 CU.

### How It Works

1. The event struct's `emit()` method builds a `[0xFF][discriminator][data]` payload on the stack
2. `emit_event_cpi` constructs a CPI call back to the program itself, with the `EventAuthority` PDA as a signer
3. The program's entrypoint receives the CPI call, detects the `0xFF` prefix, and routes to `__handle_event`
4. `__handle_event` validates the `EventAuthority` PDA address and signer flag
5. If valid, the payload (minus the `0xFF` prefix) is emitted via `sol_log_data`

### Event Authority PDA

The `#[program]` macro generates an `EventAuthority` struct with a const-evaluated PDA:

```rust
impl EventAuthority {
    const __PDA: (Address, u8) = find_program_address_const(
        &[b"__event_authority"],
        &crate::ID,
    );
    pub const ADDRESS: Address = Self::__PDA.0;
    pub const BUMP: u8 = Self::__PDA.1;
}
```

Seeds: `["__event_authority"]` + bump. The PDA address and bump are computed at compile time (`find_program_address_const`), so the runtime handler validates the signer address via a direct 32-byte comparison with no derivation cost.

### Validation in `__handle_event`

The generated event handler performs these checks:

1. The event authority account is a signer (`is_signer != 0`)
2. The event authority address matches `EventAuthority::ADDRESS` (4x `u64` comparison for speed — ~20 CU less than `memcmp`)
3. The instruction data has more than 1 byte (prefix + at least discriminator)

If all checks pass, the data after the `0xFF` prefix is emitted via `log_data`.

### When to Use Self-CPI Events

- Events consumed by other on-chain programs that need to verify the source
- Financial events (transfers, liquidations) where spoofing would be a security issue
- Any event where the indexer must guarantee the emitting program's identity

### `EventAuthority` in Account Structs

To use self-CPI events, include the `EventAuthority` and program type in your accounts:

```rust
#[derive(Accounts)]
pub struct Make<'info> {
    // ... other accounts ...
    pub program: &'info MyProgramProgram,
    pub event_authority: &'info EventAuthority,
}
```

Then in the instruction handler:

```rust
let event = MakeEvent { /* ... */ };
ctx.accounts.program.emit_event(&event, &ctx.accounts.event_authority)?;
```

## Dispatch Integration

The `#[program]` macro generates a `__dispatch` function that checks the first byte of instruction data:

```rust
fn __dispatch(ptr: *mut u8, instruction_data: &[u8]) -> Result<(), ProgramError> {
    if !instruction_data.is_empty() && instruction_data[0] == 0xFF {
        return __handle_event(ptr, instruction_data);
    }
    // ... normal instruction dispatch ...
}
```

This is why instruction discriminators cannot start with `0xFF` — the byte is reserved as the event routing prefix. The `#[program]` macro enforces this at compile time.

## Summary: Log vs Self-CPI

| Property | `emit!()` | `program.emit_event()` |
|----------|-----------|----------------------|
| Cost | ~100 CU | ~1,000 CU |
| Spoof-resistant | No | Yes |
| Requires extra accounts | No | `EventAuthority` + program |
| Mechanism | `sol_log_data` syscall | Self-CPI with PDA signer |
| Payload prefix | None (discriminator + data) | `0xFF` + discriminator + data |
