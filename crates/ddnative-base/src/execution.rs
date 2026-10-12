//! Alternate, restartable instruction ABI for the runtime. The vector ABI
//! remains `x86::LiftFn`; an instruction never calls a host service itself.
use crate::{
    mem::Mem,
    x86::{self, Cpu},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Next(u32),
    /// A taken branch, distinct from fallthrough/CALL for stack restoration.
    Jump(u32),
    Return(u32),
}
pub type Step = fn(&mut dyn Mem, &mut Cpu, u32) -> Flow;
pub type Lookup = fn(u32) -> Option<Step>;

/// Only this payload denotes integer #DE. Host panics are not guest faults.
#[derive(Debug)]
pub struct DivideError;

pub fn divide(low: u32, high: u32, divisor: u32, bits: u32, signed: bool) -> (u32, u32) {
    x86::divide(low, high, divisor, bits, signed).unwrap_or_else(|| std::panic::panic_any(DivideError))
}
