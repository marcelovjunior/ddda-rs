//! The MT Framework math library of DDDA.exe as readable Rust (docs/MATH.md).
//!
//! Each function cites the address it translates and is verified against
//! the original by differential tests (`crates/mtmath/tests/differential.rs`):
//! the lifted program's own vectors plus edge-case vectors (NaN, infinity,
//! signed zero) recorded from the original. Results are bit for bit the
//! original's; `scalar::F32` explains how.
//!
//! - [`types`]: `Vec3` (MtVector3), `Mat44` (MtMatrix), with the
//!   original's layouts.
//! - [`random`]: `Random` (MtRandom, xorshift128).
//! - [`vector`]: out-of-line vector helpers.
//! - [`matrix`]: matrix builders.
//! - [`angle`]: angle helpers.
//! - [`quat`]: `Quat` (MtQuaternion) and its out-of-line helpers.
//! - [`euler`]: matrix to Euler angles (six rotation orders).
//! - [`abi`]: the ports behind the lifted calling convention; the lifted
//!   program calls these instead of the lifted bodies.

// `!(a > b)` is how the ports spell the original's `comiss` + `jbe`: it is
// true for NaN, which `a <= b` is not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod abi;
pub mod angle;
pub mod euler;
pub mod matrix;
pub mod quat;
pub mod random;
pub mod scalar;
pub mod types;
pub mod vector;

pub use scalar::F32;
pub use types::{Mat44, Vec3};
