//! The CRT's single-precision sine and cosine (SSE2 entries 0x012F0906 and
//! 0x012F0A55: argument and result in XMM0), ported bit for bit.
//!
//! Both reduce `x` by multiples of pi/32 (double precision, split constant),
//! then evaluate a short polynomial around the tabulated `sin`/`cos` of the
//! multiple, in double precision, and round the result to float. They differ
//! only in a quarter-turn offset into the table and in the tiny-argument
//! result. Arguments with magnitude below 2^-126 (denormals, zero) take a
//! short path; infinities and NaNs return `x - x`; magnitudes of 8192 and
//! above go to an x87 routine the original reaches through 0x012F68B0 /
//! 0x012F69E0, which is not ported: [`sinf`] and [`cosf`] return `None`
//! there and the caller makes the guest call.
//!
//! Translated from 0x012F0906 and 0x012F0A55 (DDDA.exe 2017-12-13 Steam).
use ddnative_base::mem::{Mem, Pages};

/// Address of the table of `(sin(k pi/32), cos(k pi/32))` as doubles for
/// k = 0..63 (16 bytes per entry; the same table serves both functions,
/// each keeps its own copy: sinf at 0x0144B410, cosf at 0x0144B870). It is read from the
/// guest's image on each call (entry `k` only), never copied here.
const SIN_TABLE_ADDRESS: u32 = 0x0144_b410;
const COS_TABLE_ADDRESS: u32 = 0x0144_b870;

/// Float 32/pi, the multiplier that gives the argument in units of pi/32.
const UNITS_PER_RADIAN: f32 = f32::from_bits(0x4122_f983);
/// 1.5 * 2^23: adding and subtracting it rounds a float to an integer.
const ROUND_MAGIC: f32 = f32::from_bits(0x4b40_0000);
/// pi/32 split into a high and a low double. The original builds `hi` and
/// `lo` with the sign of `x` folded into the exponent field (it multiplies
/// them by the quarter count's float bits shifted into a double).
const STEP_HIGH_BITS: u64 = 0x77b9_21fb_5444_4000;
const STEP_LOW_BITS: u64 = 0x7532_e7b9_6767_33af;
const ONE: f64 = 1.0;
const MINUS_HALF: f64 = f64::from_bits(0xbfe0_0000_0000_0000);
const MINUS_SIXTH: f64 = f64::from_bits(0xbfc5_5555_5555_5555);
const ONE_24TH: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
const ONE_120TH: f64 = f64::from_bits(0x3f81_1111_1111_1111);
/// sin(x) for denormal x: x * 0.99999999906...
const SIN_TINY_SCALE: f64 = f64::from_bits(0x3fef_ffff_ff80_0000);

/// Which of the two functions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sin,
    Cos,
}

/// Argument classes by the high half-word of the float (the original's
/// `pextrw`, mask 0x7fff, subtract 0x80, compare with 0x457f).
enum Range {
    /// 2^-126 <= |x| < 8192
    Normal,
    /// zero and denormals (and NaN/inf are not here)
    Tiny,
    /// |x| >= 8192, infinities and NaNs
    Large,
}

#[inline(always)]
fn classify(x: f32) -> Range {
    let high = ((x.to_bits() >> 16) & 0x7fff) as u16;
    let shifted = high.wrapping_sub(0x80);
    if shifted <= 0x457f {
        Range::Normal
    } else if (shifted as i16) > 0x457f {
        Range::Large
    } else {
        Range::Tiny
    }
}

/// `x - x` as SSE computes it for an infinity (the default NaN) or a NaN
/// (itself, quieted).
#[inline(always)]
fn non_finite_result(x: f32) -> f32 {
    let bits = x.to_bits();
    if bits & 0x007f_ffff == 0 {
        f32::from_bits(0xffc0_0000)
    } else {
        f32::from_bits(bits | 0x0040_0000)
    }
}

/// Table-driven evaluation for `Range::Normal`.
#[inline(always)]
fn evaluate(m: &dyn Mem, pg: Pages, x: f32, kind: Kind) -> f32 {
    let scaled = UNITS_PER_RADIAN * x;
    // cvtss2si: round to nearest even (the default MXCSR rounding)
    let quarter_count = scaled.round_ties_even() as i32 as u32;
    let sign = (x.to_bits() & 0x8000_0000) as u64;
    let x_double = x as f64;
    let rounded = (scaled + ROUND_MAGIC) - ROUND_MAGIC;
    let sign_fold = (sign << 32) ^ (sign << 29);
    let bias = if kind == Kind::Cos { 16 } else { 0 };
    let index = (quarter_count.wrapping_add(bias) & 0x3f) as usize;
    let step_high = f64::from_bits(STEP_HIGH_BITS ^ sign_fold);
    let step_low = f64::from_bits(STEP_LOW_BITS ^ sign_fold);
    // the rounded count as a double with its exponent field shifted (see above)
    let count = f64::from_bits((rounded.to_bits() as u64) << 29);
    let reduced = x_double - step_high * count;
    let correction = step_low * count + reduced;
    let square = reduced * reduced;
    let cos_poly = (MINUS_HALF * square + ONE) + ONE_24TH * (square * square);
    let sin_poly = (MINUS_SIXTH * square + ONE) + ONE_120TH * (square * square);
    let table = if kind == Kind::Cos { COS_TABLE_ADDRESS } else { SIN_TABLE_ADDRESS };
    let entry = table + 16 * index as u32;
    let read = |a: u32| f64::from_bits(pg.u32(m, a) as u64 | (pg.u32(m, a + 4) as u64) << 32);
    let (table_sin, table_cos) = (read(entry), read(entry + 8));
    (table_sin * cos_poly + (correction * table_cos) * sin_poly) as f32
}

/// The CRT `sinf`; `None` when the original takes its x87 path.
#[inline]
pub fn sinf(m: &dyn Mem, pg: Pages, x: f32) -> Option<f32> {
    match classify(x) {
        Range::Normal => Some(evaluate(m, pg, x, Kind::Sin)),
        Range::Tiny => Some(((x as f64) * SIN_TINY_SCALE) as f32),
        Range::Large if x.to_bits() & 0x7f80_0000 == 0x7f80_0000 => Some(non_finite_result(x)),
        Range::Large => None,
    }
}

/// The CRT `cosf`; `None` when the original takes its x87 path.
#[inline]
pub fn cosf(m: &dyn Mem, pg: Pages, x: f32) -> Option<f32> {
    match classify(x) {
        Range::Normal => Some(evaluate(m, pg, x, Kind::Cos)),
        Range::Tiny => {
            let x = x as f64;
            Some((ONE - x * x) as f32)
        }
        Range::Large if x.to_bits() & 0x7f80_0000 == 0x7f80_0000 => Some(non_finite_result(x)),
        Range::Large => None,
    }
}
