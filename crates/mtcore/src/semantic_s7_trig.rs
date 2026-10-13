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

/// `(sin(k pi/32), cos(k pi/32))` as doubles for k = 0..63 (the table at
/// 0x0144B410 / 0x0144B870, which is the same in both functions).
const TABLE: [(u64, u64); 64] = [
    (0x0000000000000000, 0x3ff0000000000000),
    (0x3fb917a6bc29b42c, 0x3fefd88da3d12526),
    (0x3fc8f8b83c69a60b, 0x3fef6297cff75cb0),
    (0x3fd294062ed59f06, 0x3fee9f4156c62dda),
    (0x3fd87de2a6aea963, 0x3fed906bcf328d46),
    (0x3fde2b5d3806f63b, 0x3fec38b2f180bdb1),
    (0x3fe1c73b39ae68c8, 0x3fea9b66290ea1a3),
    (0x3fe44cf325091dd6, 0x3fe8bc806b151741),
    (0x3fe6a09e667f3bcd, 0x3fe6a09e667f3bcd),
    (0x3fe8bc806b151741, 0x3fe44cf325091dd6),
    (0x3fea9b66290ea1a3, 0x3fe1c73b39ae68c8),
    (0x3fec38b2f180bdb1, 0x3fde2b5d3806f63b),
    (0x3fed906bcf328d46, 0x3fd87de2a6aea963),
    (0x3fee9f4156c62dda, 0x3fd294062ed59f06),
    (0x3fef6297cff75cb0, 0x3fc8f8b83c69a60b),
    (0x3fefd88da3d12526, 0x3fb917a6bc29b42c),
    (0x3ff0000000000000, 0x0000000000000000),
    (0x3fefd88da3d12526, 0xbfb917a6bc29b42c),
    (0x3fef6297cff75cb0, 0xbfc8f8b83c69a60b),
    (0x3fee9f4156c62dda, 0xbfd294062ed59f06),
    (0x3fed906bcf328d46, 0xbfd87de2a6aea963),
    (0x3fec38b2f180bdb1, 0xbfde2b5d3806f63b),
    (0x3fea9b66290ea1a3, 0xbfe1c73b39ae68c8),
    (0x3fe8bc806b151741, 0xbfe44cf325091dd6),
    (0x3fe6a09e667f3bcd, 0xbfe6a09e667f3bcd),
    (0x3fe44cf325091dd6, 0xbfe8bc806b151741),
    (0x3fe1c73b39ae68c8, 0xbfea9b66290ea1a3),
    (0x3fde2b5d3806f63b, 0xbfec38b2f180bdb1),
    (0x3fd87de2a6aea963, 0xbfed906bcf328d46),
    (0x3fd294062ed59f06, 0xbfee9f4156c62dda),
    (0x3fc8f8b83c69a60b, 0xbfef6297cff75cb0),
    (0x3fb917a6bc29b42c, 0xbfefd88da3d12526),
    (0x0000000000000000, 0xbff0000000000000),
    (0xbfb917a6bc29b42c, 0xbfefd88da3d12526),
    (0xbfc8f8b83c69a60b, 0xbfef6297cff75cb0),
    (0xbfd294062ed59f06, 0xbfee9f4156c62dda),
    (0xbfd87de2a6aea963, 0xbfed906bcf328d46),
    (0xbfde2b5d3806f63b, 0xbfec38b2f180bdb1),
    (0xbfe1c73b39ae68c8, 0xbfea9b66290ea1a3),
    (0xbfe44cf325091dd6, 0xbfe8bc806b151741),
    (0xbfe6a09e667f3bcd, 0xbfe6a09e667f3bcd),
    (0xbfe8bc806b151741, 0xbfe44cf325091dd6),
    (0xbfea9b66290ea1a3, 0xbfe1c73b39ae68c8),
    (0xbfec38b2f180bdb1, 0xbfde2b5d3806f63b),
    (0xbfed906bcf328d46, 0xbfd87de2a6aea963),
    (0xbfee9f4156c62dda, 0xbfd294062ed59f06),
    (0xbfef6297cff75cb0, 0xbfc8f8b83c69a60b),
    (0xbfefd88da3d12526, 0xbfb917a6bc29b42c),
    (0xbff0000000000000, 0x0000000000000000),
    (0xbfefd88da3d12526, 0x3fb917a6bc29b42c),
    (0xbfef6297cff75cb0, 0x3fc8f8b83c69a60b),
    (0xbfee9f4156c62dda, 0x3fd294062ed59f06),
    (0xbfed906bcf328d46, 0x3fd87de2a6aea963),
    (0xbfec38b2f180bdb1, 0x3fde2b5d3806f63b),
    (0xbfea9b66290ea1a3, 0x3fe1c73b39ae68c8),
    (0xbfe8bc806b151741, 0x3fe44cf325091dd6),
    (0xbfe6a09e667f3bcd, 0x3fe6a09e667f3bcd),
    (0xbfe44cf325091dd6, 0x3fe8bc806b151741),
    (0xbfe1c73b39ae68c8, 0x3fea9b66290ea1a3),
    (0xbfde2b5d3806f63b, 0x3fec38b2f180bdb1),
    (0xbfd87de2a6aea963, 0x3fed906bcf328d46),
    (0xbfd294062ed59f06, 0x3fee9f4156c62dda),
    (0xbfc8f8b83c69a60b, 0x3fef6297cff75cb0),
    (0xbfb917a6bc29b42c, 0x3fefd88da3d12526),
];

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
fn evaluate(x: f32, kind: Kind) -> f32 {
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
    let (table_sin, table_cos) = TABLE[index];
    let (table_sin, table_cos) = (f64::from_bits(table_sin), f64::from_bits(table_cos));
    (table_sin * cos_poly + (correction * table_cos) * sin_poly) as f32
}

/// The CRT `sinf`; `None` when the original takes its x87 path.
#[inline]
pub fn sinf(x: f32) -> Option<f32> {
    match classify(x) {
        Range::Normal => Some(evaluate(x, Kind::Sin)),
        Range::Tiny => Some(((x as f64) * SIN_TINY_SCALE) as f32),
        Range::Large if x.to_bits() & 0x7f80_0000 == 0x7f80_0000 => Some(non_finite_result(x)),
        Range::Large => None,
    }
}

/// The CRT `cosf`; `None` when the original takes its x87 path.
#[inline]
pub fn cosf(x: f32) -> Option<f32> {
    match classify(x) {
        Range::Normal => Some(evaluate(x, Kind::Cos)),
        Range::Tiny => {
            let x = x as f64;
            Some((ONE - x * x) as f32)
        }
        Range::Large if x.to_bits() & 0x7f80_0000 == 0x7f80_0000 => Some(non_finite_result(x)),
        Range::Large => None,
    }
}
