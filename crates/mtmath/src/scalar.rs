//! `F32`: a single-precision value whose arithmetic matches the original's
//! SSE scalar instructions bit for bit.
//!
//! Plain Rust `f32` arithmetic already rounds like SSE (no fused
//! multiply-add, no reassociation), with one exception: LLVM treats
//! addition and multiplication as commutative and may swap the operands of
//! `addss`/`mulss` to save a register move. When both operands are NaN, SSE
//! returns the first operand's NaN (quieted), so a swap can change the
//! payload. `F32` pins every binary operation to one SSE instruction with
//! the operands in the written order: `a + b` is `addss a, b`. Single-
//! operand operations (`sqrt`, `abs`, negation) have no such freedom and
//! use the standard library.
//!
//! On a non-x86-64 host the operators fall back to `f32` arithmetic, which
//! is exact except for the two-NaN payload rule above.

use core::ops::{Add, Div, Mul, Neg, Sub};

/// One `float` of the original (`MtFloat`).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct F32(pub f32);

impl F32 {
    pub const ZERO: F32 = F32(0.0);
    pub const ONE: F32 = F32(1.0);

    pub const fn from_bits(bits: u32) -> F32 {
        F32(f32::from_bits(bits))
    }
    pub const fn to_bits(self) -> u32 {
        self.0.to_bits()
    }
    /// `sqrtss`.
    #[inline(always)]
    pub fn sqrt(self) -> F32 {
        F32(self.0.sqrt())
    }
    /// `andps` with 0x7FFFFFFF: clears the sign bit, NaN payload kept.
    #[inline(always)]
    pub fn abs(self) -> F32 {
        F32(self.0.abs())
    }
}

impl From<f32> for F32 {
    fn from(v: f32) -> F32 {
        F32(v)
    }
}

impl From<F32> for f32 {
    fn from(v: F32) -> f32 {
        v.0
    }
}

/// `xorps` with 0x80000000: flips the sign bit only (a NaN stays the same
/// NaN with the other sign; nothing is quieted).
impl Neg for F32 {
    type Output = F32;
    #[inline(always)]
    fn neg(self) -> F32 {
        F32(-self.0)
    }
}

macro_rules! sse_binary {
    ($trait:ident, $method:ident, $insn:literal, $op:tt) => {
        impl $trait for F32 {
            type Output = F32;
            #[inline(always)]
            fn $method(self, rhs: F32) -> F32 {
                #[cfg(target_arch = "x86_64")]
                {
                    let mut a = self.0;
                    // SAFETY: one register-to-register SSE instruction; no
                    // memory, stack or flags are touched. MXCSR is the
                    // host's, as for the lifted code (round to nearest).
                    unsafe {
                        core::arch::asm!(concat!($insn, " {a}, {b}"), a = inout(xmm_reg) a,
                                         b = in(xmm_reg) rhs.0,
                                         options(pure, nomem, nostack, preserves_flags));
                    }
                    F32(a)
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    F32(self.0 $op rhs.0)
                }
            }
        }
    };
}

sse_binary!(Add, add, "addss", +);
sse_binary!(Sub, sub, "subss", -);
sse_binary!(Mul, mul, "mulss", *);
sse_binary!(Div, div, "divss", /);
