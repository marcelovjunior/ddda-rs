//! The clock and the frame-rate-sensitive systems of DDDA.exe as readable
//! Rust (docs/FRAME_RATE.md, "Readable ports"): the 1:1 baseline that an
//! opt-in high-frame-rate mode will change, one small diff per system.
//!
//! Each function cites the address it translates and is verified against
//! the original by differential tests (`crates/mtframe/tests/
//! differential.rs`): the lifted program's own vectors plus edge cases
//! recorded from the original (`research/native/vectors_frame.py`).
//! Guest objects are addressed through small handles over guest memory;
//! the fields the code uses are named, with their offsets.
//!
//! - [`smain`]: `sMain`'s timing fields.
//! - [`effect`]: the effect manager's frame counter and the time inputs
//!   that read it.
//! - [`lunge`]: em5501's rush-bite lunge.
//! - [`abi`]: the ports behind the lifted calling convention; the lifted
//!   program calls these instead of the lifted bodies.

pub mod abi;
pub mod effect;
pub mod lunge;
pub mod smain;
