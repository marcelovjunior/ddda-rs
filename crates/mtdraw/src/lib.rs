//! MT Framework's nDraw layer of DDDA.exe as readable Rust (docs/NDRAW.md,
//! docs/NATIVE_RENDERER.md; L112, step N1 of the native renderer).
//!
//! - [`executor`]: the frame's command executor (0x00DAB480): it walks the
//!   command records sRender queued for the frame and issues them to the
//!   Direct3D 9 device through the four apply functions and the device's
//!   draw, clear and copy methods.
//! - [`sink`]: where commands can go instead of the D3D9 device (a native
//!   implementation takes them command by command; L119).
//! - [`abi`]: the port behind the lifted calling convention; registered
//!   over the lifted body by `ddports` (crates/ddports).
//!
//! The port is verified like crates/mtcore's: it leaves the machine as the
//! original does (registers, xmm, x87, every byte written) and makes its
//! calls the way the lifted body makes them, so the apply functions, the
//! device and anything hooked behind them see the same calls with the same
//! arguments in the same order (`tests/differential.rs` runs it against the
//! lifted body on generated command lists).

pub mod abi;
pub mod executor;
pub mod sink;
