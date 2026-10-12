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

/// An `fld dword` / `fstp dword` pair: the value is kept, except that a
/// signalling NaN becomes quiet (the x87 load raises the masked invalid
/// exception and sets the quiet bit). Checked against the CPU by
/// `crates/ddnative/tests/units.rs` (`x87_copy_matches_the_cpu`).
pub fn x87_copy(v: f32) -> f32 {
    let bits = v.to_bits();
    if v.is_nan() && bits & 0x0040_0000 == 0 {
        f32::from_bits(bits | 0x0040_0000)
    } else {
        v
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

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::F32;

    /// The host CPU's rule, which `F32` keeps and the vector recorder
    /// applies where Unicorn differs (Intel SDM vol. 1, 4.8.3.5): with
    /// two NaN operands, SSE returns the first one, quieted.
    #[test]
    fn two_nans_give_the_first_operand_quieted() {
        let q = F32::from_bits(0x7FC0_1234);
        let s = F32::from_bits(0xFFA0_0007);
        let ops: [fn(F32, F32) -> F32; 4] = [|a, b| a + b, |a, b| a - b, |a, b| a * b, |a, b| a / b];
        for op in ops {
            assert_eq!(op(q, s).to_bits(), 0x7FC0_1234);
            assert_eq!(op(s, q).to_bits(), 0xFFE0_0007);
        }
        // one NaN: that NaN, quieted, whichever side it is on
        assert_eq!((F32(1.0) * s).to_bits(), 0xFFE0_0007);
        // invalid operations give the x86 default NaN
        assert_eq!((F32(f32::INFINITY) - F32(f32::INFINITY)).to_bits(), 0xFFC0_0000);
    }
}
