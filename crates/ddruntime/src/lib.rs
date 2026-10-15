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
pub mod archive_inflate;
pub mod procedural_geometry;
mod job_participants;
mod waits;
pub mod input;
pub mod d3d9;
pub mod d3d9copy;
pub mod d3d9dev;
pub mod d3d9draw;
pub mod d3d9res;
pub mod d3d9state;
pub mod d3dx;
pub mod dispatch;
mod critical_jobs;
mod job_order;
mod census;
mod decouple;
mod measure;
mod shadow_fee320;
mod parallel_shadow;
pub mod shadow_replay;
#[cfg(feature = "emu")]
pub mod emu;
pub mod executor;
pub mod fasthash;
pub mod guest;
pub mod crtfast;
pub mod fastmem;
pub mod files;
mod file_cache;
pub mod load_log;
pub mod image;
pub mod interfaces;
pub mod machine;
pub mod memory;
pub mod profile;
pub mod registry;
pub mod runtime;
pub mod services;
pub mod seh;
pub mod session;
pub mod steam;
pub mod system;
pub mod text;
pub mod threads;
pub mod user32;
pub mod win32;
pub mod movies;
pub mod net;
pub mod ndraw_stats;
/// L170: DDRUNTIME_SERVICE_CENSUS (service calls by service and call site)
mod service_census;
/// L119: the native path through the nDraw executor port
pub mod ndraw_native;

/// Take the TEB TLS slots the lifted code's hot paths read now, first thing
/// in a host's main (L103, L103b): the x87 live id and the dispatch cache.
/// The largest slot number; 64 or more means one of them is on the slower
/// thread_local fallback.
pub fn reserve_x87_slot() -> u32 {
    ddnative_host::reserve_resolved_slot().max(ddnative_host::x87::reserve_slot())
}
