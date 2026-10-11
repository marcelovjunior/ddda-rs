//! One-to-one translations of DDDA.exe functions (the systematic rebuild,
//! docs/METHODOLOGY.md). Each function here:
//!
//! - cites the original address it translates, and translates the whole
//!   function: every branch, in the original's order;
//! - keeps the original's state as named fields at the original's
//!   offsets (documented per field), not a reshaped model;
//! - reaches every callee it cannot yet own through an explicit trait,
//!   never a skipped branch or a stand-in value;
//! - is checked by a differential test against the original instructions
//!   (`research/native/`, vectors replayed by `tests/`).
//!
//! Platform-layer functions (allocators, the CRT, threads, file and device
//! I/O) are not translated here: Rust's own facilities replace them.

pub mod action;
pub mod character;
pub mod enemy;
pub mod generated;
pub mod mem;
pub mod model;
pub mod particle;
pub mod platform;
pub mod player_action;
pub mod species;
pub mod unit;
pub mod x86;

/// An `fld dword` / `fstp dword` pair: the value is kept, except that a
/// signalling NaN becomes quiet (the x87 load raises the masked invalid
/// exception and sets the quiet bit).
pub fn x87_copy(v: f32) -> f32 {
    let bits = v.to_bits();
    if v.is_nan() && bits & 0x0040_0000 == 0 {
        f32::from_bits(bits | 0x0040_0000)
    } else {
        v
    }
}

