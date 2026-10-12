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
pub use ddnative_base::{mem, platform, x86, x87};
pub mod model;
pub mod particle;
pub mod player_action;
pub mod species;
pub mod unit;

/// An `fld dword` / `fstp dword` pair (now in `mtmath::scalar`, which the
/// lifted shards can reach; tested against the CPU in tests/units.rs).
pub use mtmath::scalar::x87_copy;

