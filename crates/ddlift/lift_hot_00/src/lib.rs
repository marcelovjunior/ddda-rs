//! Sampled hot shard; regenerate its private contents with lift.py gen.
pub use ddnative_base::{mem, platform, x86, x87};
mod generated;
pub use generated::*;
