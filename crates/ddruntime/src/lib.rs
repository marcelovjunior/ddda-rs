//! The Phase 1 runtime (docs/RUNTIME.md): the machinery that runs the
//! verified 1:1 translations of DDDA.exe outside their tests.
//!
//! - [`memory`]: the guest's 32-bit address space with page permissions and
//!   checked accesses (stage R03).
//! - [`image`]: maps the player's installed DDDA.exe into it.
//! - [`services`]: Rust replacements for platform code (CRT now), bound at
//!   the original addresses, and the guest heap.
//! - [`dispatch`]: the address-to-function registry behind `x86::call_ext`,
//!   with structured failures for untranslated targets.
//!
//! Nothing here invents behaviour: an access the original would fault on
//! faults, and a call to code that is neither translated nor modelled stops
//! with its address instead of returning something made up.
pub mod audio;
pub mod input;
pub mod d3d9;
pub mod d3d9copy;
pub mod d3d9dev;
pub mod d3d9draw;
pub mod d3d9res;
pub mod d3d9state;
pub mod dispatch;
pub mod files;
pub mod image;
pub mod interfaces;
pub mod memory;
pub mod profile;
pub mod registry;
pub mod runtime;
pub mod services;
pub mod session;
pub mod steam;
pub mod system;
pub mod text;
pub mod threads;
pub mod user32;
pub mod win32;
pub mod movies;
