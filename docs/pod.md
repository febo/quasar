# Pod Types

`quasar-pod` provides alignment-1 integer types for zero-copy Solana account access. Every type wraps a native integer in a `[u8; N]` little-endian byte array, guaranteeing `align_of::<T>() == 1`. This is the foundation that makes pointer casts from raw account data safe without alignment violations.

The crate is `no_std` and has no dependencies on the rest of the Quasar framework. It can be used standalone in any Solana program or off-chain tooling.

```toml
[dependencies]
quasar-pod = "0.1"
```

## Why Alignment 1

Solana account data arrives as a `&[u8]` slice. Casting `&[u8]` to `&u64` requires 8-byte alignment, but the SVM makes no alignment guarantees for account data offsets within the input buffer. A misaligned cast is undefined behavior.

Pod types solve this by storing integers as `[u8; N]` with `#[repr(transparent)]`:

```rust
#[repr(transparent)]
pub struct PodU64([u8; 8]);
```

Since `[u8; 8]` has alignment 1, a `#[repr(C)]` struct composed entirely of Pod types also has alignment 1. This means the entire zero-copy companion struct can be pointer-cast from any byte offset in account data.

Quasar enforces this invariant with compile-time assertions on every Pod type:

```rust
const _: () = assert!(core::mem::align_of::<PodU64>() == 1);
const _: () = assert!(core::mem::size_of::<PodU64>() == 8);
```

## Type Mapping

| Rust Native | Pod Type | Size (bytes) |
|-------------|----------|-------------|
| `u8` | `u8` (no wrapper needed) | 1 |
| `i8` | `i8` (no wrapper needed) | 1 |
| `u16` | `PodU16` | 2 |
| `u32` | `PodU32` | 4 |
| `u64` | `PodU64` | 8 |
| `u128` | `PodU128` | 16 |
| `i16` | `PodI16` | 2 |
| `i32` | `PodI32` | 4 |
| `i64` | `PodI64` | 8 |
| `i128` | `PodI128` | 16 |
| `bool` | `PodBool` | 1 |

Single-byte types (`u8`, `i8`) already have alignment 1 and do not need wrappers.

## Conversion

Every Pod type implements bidirectional `From` conversions with its native type:

```rust
let pod = PodU64::from(42u64);
let native: u64 = pod.into();
let raw: u64 = pod.get();   // explicit extraction
```

`get()` reads the `[u8; N]` backing array as a little-endian native integer via `from_le_bytes`.

## Arithmetic

### Operators: `+`, `-`, `*`

Addition, subtraction, and multiplication have **dual behavior**:

- **Debug builds**: panic on overflow (via `checked_add`/`checked_sub`/`checked_mul` + `expect`)
- **Release builds**: wrapping semantics (via `wrapping_add`/`wrapping_sub`/`wrapping_mul`)

This matches Rust's native integer behavior and saves CU in production (wrapping is cheaper than checked arithmetic on SBF).

```rust
let a = PodU64::from(10);
let b = PodU64::from(20);
let c = a + b;              // PodU64(30)
let d = a + 5u64;           // Pod + native also works
```

All operators work both Pod-to-Pod and Pod-to-native. Compound assignment (`+=`, `-=`, `*=`, `/=`, `%=`) is also implemented for both operand combinations.

### Division and Remainder: `/`, `%`

Division and remainder **always panic on zero divisor**, in both debug and release builds. Unlike `+`/`-`/`*`, there is no wrapping variant — division by zero is not a recoverable overflow but a logic error.

```rust
let a = PodU64::from(100);
let b = PodU64::from(3);
let c = a / b;              // PodU64(33)
let d = a % b;              // PodU64(1)
// let e = a / PodU64::ZERO;  // panics in all builds
```

### Signed Types: Negation

Signed Pod types (`PodI16` through `PodI128`) additionally implement `Neg`:

```rust
let x = PodI64::from(42);
let y = -x;                 // PodI64(-42)
```

Negation follows the same debug/release pattern: panics on overflow in debug (negating `i64::MIN`), wraps in release.

## Checked and Saturating Methods

For cases where overflow must be detected or clamped rather than wrapped:

```rust
let a = PodU64::from(u64::MAX);

// Checked — returns None on overflow
a.checked_add(PodU64::from(1));   // None
a.checked_sub(PodU64::from(1));   // Some(PodU64(MAX-1))
a.checked_mul(PodU64::from(2));   // None
a.checked_div(PodU64::from(0));   // None (safe — no panic)

// Saturating — clamps at MIN/MAX
a.saturating_add(PodU64::from(1));   // PodU64(MAX)
a.saturating_sub(PodU64::from(1));   // PodU64(MAX-1)
a.saturating_mul(PodU64::from(2));   // PodU64(MAX)
```

All checked/saturating methods accept `impl Into<Self>`, so both Pod and native arguments work:

```rust
a.checked_add(1u64);         // works
a.saturating_sub(PodU64::from(5));  // also works
```

Note: there is no `saturating_div` — division by zero is handled by `checked_div` returning `None`.

## Bitwise Operations

Full bitwise operator support for all integer Pod types:

```rust
let a = PodU32::from(0xFF00);
let b = PodU32::from(0x0FF0);

let c = a & b;     // PodU32(0x0F00)
let d = a | b;     // PodU32(0xFFF0)
let e = a ^ b;     // PodU32(0xF0F0)
let f = !a;        // PodU32(0xFFFF00FF)
let g = a << 4;    // PodU32(0xFF000)
let h = a >> 4;    // PodU32(0xFF0)
```

Shift operators take `u32` as the right-hand side (matching Rust convention).

## Comparison and Ordering

Pod types implement `Eq`, `Ord`, `PartialEq`, and `PartialOrd`. Comparison converts to native integers internally:

```rust
let a = PodU64::from(10);
let b = PodU64::from(20);

assert!(a < b);
assert!(a == PodU64::from(10));
assert!(a < 20u64);  // Pod vs native comparison
```

Cross-type comparison with the native type is supported via `PartialEq<native>` and `PartialOrd<native>`.

## Constants

Each Pod type provides three associated constants:

```rust
PodU64::ZERO  // all-zero bytes
PodU64::MIN   // minimum value (0 for unsigned, i64::MIN for signed)
PodU64::MAX   // maximum value (u64::MAX for unsigned, i64::MAX for signed)
```

The `is_zero()` method is a fast byte-level check (`self.0 == [0u8; N]`) without conversion to native:

```rust
let x = PodU64::ZERO;
assert!(x.is_zero());
```

## PodBool

`PodBool` is a 1-byte boolean wrapper. Unlike integer Pod types, it has no arithmetic operators — only `Not` (`!`):

```rust
let flag = PodBool::from(true);
assert!(flag.get());
assert!(!(!flag).get());
```

Any non-zero byte value is treated as `true`:

```rust
// Internal: PodBool([0x01]) -> true
// Internal: PodBool([0x00]) -> false
// Internal: PodBool([0xFF]) -> true (any non-zero)
```

## Display and Debug

Pod types implement both `Display` (shows the numeric value) and `Debug` (shows the type name and value):

```rust
let x = PodU64::from(42);
// Display: "42"
// Debug:   "PodU64(42)"
```

## Usage in Account Structs

When defining account state with `#[account]`, Quasar automatically maps native Rust types to Pod types in the generated zero-copy companion struct:

```rust
#[account(discriminator = 1)]
pub struct Vault {
    pub owner: Address,
    pub balance: u64,      // becomes PodU64 in VaultZc
    pub is_locked: bool,   // becomes PodBool in VaultZc
}
```

Field access through `Deref` transparently converts Pod values back to native types where needed. The Pod layer is invisible to the program author unless they need explicit checked/saturating arithmetic.
