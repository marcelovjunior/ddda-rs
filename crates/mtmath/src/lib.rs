//! The MT Framework math library of DDDA.exe as readable Rust (docs/MATH.md).
//!
//! Each function cites the address it translates and is verified against
//! the original by differential tests (`crates/ddnative/tests/math.rs`):
//! the lifted program's own vectors plus edge-case vectors (NaN, infinity,
//! signed zero) recorded from the original. Results are bit for bit the
//! original's; `scalar::F32` explains how.
//!
//! - [`types`]: `Vec3` (MtVector3), `Mat44` (MtMatrix), with the
//!   original's layouts.
//! - [`random`]: `Random` (MtRandom, xorshift128).
//! - [`matrix`]: matrix builders.
//! - [`angle`]: angle helpers.
//! - [`abi`]: the ports behind the lifted calling convention; the lifted
//!   program calls these instead of the lifted bodies.

pub mod abi;
pub mod angle;
pub mod matrix;
pub mod random;
pub mod scalar;
pub mod types;

pub use scalar::F32;
pub use types::{Mat44, Vec3};
