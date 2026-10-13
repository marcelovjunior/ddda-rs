//! Shared machine state and platform models for hand ports and lifted shards.
//! This crate has no dependency on the generated code.
pub mod mem;
pub mod execution;
pub mod hooks;
pub mod platform;
pub mod x86;
pub mod x87;
