//! L170 opt-in counterparts of x86::Flags::alu_* (same full lazy state).
//! Selected only by LIFT_INLINE_FLAGS=1 during generation. Normal inline
//! hints allow optimized shards to specialize widths without expanding every
//! debug call site. Existing out-of-line helpers and defaults are unchanged.
use crate::x86::{Flags, ADD, SUB, LOGIC};

macro_rules! alu {
    ($name:ident, $kind:expr, $op:expr) => {
        impl Flags {
            #[inline]
            pub fn $name(&mut self, a: u32, b: u32, bits: u32) -> u32 {
                let mask = u32::MAX >> (32 - bits);
                let (a, b) = (a & mask, b & mask);
                let r = $op(a, b) & mask;
                *self = Flags { kind: $kind, a, b, r, bits, cf_in: false };
                r
            }
        }
    };
}

alu!(alu_add_inline, ADD, u32::wrapping_add);
alu!(alu_sub_inline, SUB, u32::wrapping_sub);
alu!(alu_and_inline, LOGIC, |a: u32, b: u32| a & b);
alu!(alu_or_inline, LOGIC, |a: u32, b: u32| a | b);
alu!(alu_xor_inline, LOGIC, |a: u32, b: u32| a ^ b);
