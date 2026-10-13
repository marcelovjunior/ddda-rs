//! The CRT's `asinf` (0x012F0D4C) and double-precision `atan2`
//! (0x012F1109), ported bit for bit like [`crate::semantic_s7_trig`]'s sine
//! and cosine. Both are SSE2 routines taking their arguments in XMM0
//! (and XMM1 for atan2) and returning in XMM0.
//!
//! The code below follows the original instruction by instruction at the
//! level of its registers, with the lanes the original uses spelled out:
//! the routines mix scalar single-precision arithmetic, table lookups and
//! bit manipulation of the float's representation, and every rounding step
//! is part of the result.
//!
//! Translated from 0x012F0D4C and 0x012F1109 (DDDA.exe 2017-12-13 Steam).
use ddnative_base::mem::{Mem, Pages};

/// The CRT's constant tables (the exe's data at 0x0144C640 ..= 0x0144DE5F:
/// correction tables, split constants, quadrant constants), read from the
/// player's own copy of the game in guest memory when needed.
pub struct Tables<'a> {
    pub memory: &'a dyn Mem,
    pub pages: Pages,
}

impl Tables<'_> {
    #[inline(always)]
    fn word(&self, address: u32) -> u32 {
        self.pages.u32(self.memory, address)
    }
    #[inline(always)]
    fn float(&self, address: u32) -> f32 {
        f32::from_bits(self.word(address))
    }
    /// Four consecutive words (a 16-byte vector load).
    #[inline(always)]
    fn vector(&self, address: u32) -> [u32; 4] {
        std::array::from_fn(|i| self.word(address + 4 * i as u32))
    }
    /// A 64-bit table word.
    #[inline(always)]
    fn quad(&self, address: u32) -> u64 {
        self.word(address) as u64 | ((self.word(address + 4) as u64) << 32)
    }
    #[inline(always)]
    fn double(&self, address: u32) -> f64 {
        f64::from_bits(self.quad(address))
    }
    /// Two consecutive 64-bit table words.
    #[inline(always)]
    fn pair(&self, address: u32) -> [u64; 2] {
        [self.quad(address), self.quad(address + 8)]
    }
}

/// `asinf(x)` as the CRT computes it, by the magnitude of `x`:
///
/// * `0.0625 <= |x| <= 0.866` ([`asin_middle`]): a correction table indexed
///   by the exponent and top mantissa bits of `x`, and a division;
/// * `0.866 < |x| <= 0.996` ([`asin_upper`]) and `0.996 < |x| < 1`
///   ([`asin_near_one`]): the half-angle identity
///   `asin(x) = pi/2 - 2 asin(sqrt((1 - x) / 2))`-style, evaluated in split
///   (high and low) parts, the second with a polynomial instead of the
///   table;
/// * `|x| < 0.0625`: a short odd polynomial ([`asin_small`]), or `x` itself
///   plus a tiny constant ([`asin_tiny`]);
/// * `|x| >= 1`: exactly 1 gives `+-pi/2`, NaN gives NaN, anything else the
///   default NaN ([`asin_out_of_range`]).
pub fn asinf(t: &Tables, x: f32) -> f32 {
    let bits = x.to_bits();
    let magnitude = bits & 0x7fff_ffff;
    let above_low = magnitude.wrapping_sub(0x3d80_0000); // |x| - 0.0625, as bits
    // both |x| - 0.0625 >= 0 and 0.866... - |x| >= 0 (as signed bits)
    if ((0x3f5d_b3d7u32.wrapping_sub(magnitude) | above_low) as i32) >= 0 {
        return asin_middle(t, x, bits, above_low);
    }
    if ((0x01fe_ffffu32.wrapping_sub(above_low) | above_low) as i32) >= 0 {
        return asin_upper(t, x, bits);
    }
    if (above_low as i32) < 0 {
        let shifted = above_low.wrapping_add(0x0600_0000);
        return if (shifted as i32) < 0 { asin_tiny(x) } else { asin_small(t, x) };
    }
    if above_low >= 0x0200_0000 {
        return asin_out_of_range(t, x, magnitude);
    }
    asin_near_one(t, x, bits)
}

/// `0.0625 <= |x| <= 0.866...` (0x012F0D82).
fn asin_middle(t: &Tables, x: f32, bits: u32, above_low: u32) -> f32 {
    let one = t.float(0x0144_d280);
    let index = (above_low >> 16) & 0x1fe;
    let scale = t.float(0x0144_ce40 + index * 2);
    let entry = t.vector(0x0144_c640 + index * 4);
    let root = (one - x * x).sqrt();
    // x's top bits (the mask 0xFFFE0000) with a marker bit
    let marked = f32::from_bits((t.word(0x0144_d284) & bits) | 0x0001_0000);
    let sum = x + marked;
    let scaled = x * scale;
    let difference = (x - marked) * sum;
    let denominator = scaled + marked * root;
    let quotient = difference / denominator;
    let reduced = scaled - marked * root;
    let sign = bits & 0x8000_0000;
    let correction = (t.float(0x0144_d288) * reduced) * (reduced * reduced);
    let tail = correction + f32::from_bits(entry[0] ^ sign);
    (quotient + tail) + f32::from_bits(entry[1] ^ sign)
}

/// `0.866... < |x| <= 0x3f7effff` (0x012F0E2F).
fn asin_upper(t: &Tables, x: f32, bits: u32) -> f32 {
    let one = t.float(0x0144_d280);
    let root = (one - x * x).sqrt();
    let sign = bits & 0x8000_0000;
    let magnitude = f32::from_bits(bits & 0x7fff_ffff);
    let high = f32::from_bits(bits & 0x7fff_f000);
    let low = magnitude - high;
    let high_squared = high * high;
    let sum = magnitude + high;
    let one_less = one - high_squared;
    let product = sum * low;
    let marked = f32::from_bits((t.word(0x0144_d284) & root.to_bits()) | 0x0001_0000);
    let exponent = ((root.to_bits() >> 16).wrapping_sub(0x3d80)) & 0xfffe;
    let scaled_root = root * t.float(0x0144_ce40 + exponent * 2);
    let high_marked = high * marked;
    let low_marked = low * marked;
    let marked_squared = marked * marked;
    let t6 = (high_marked - scaled_root) + low_marked;
    let t4 = (one_less - marked_squared) - product;
    let doubled = scaled_root + scaled_root;
    let quotient = t4 / (doubled + t6);
    let entry = t.vector(0x0144_c640 + exponent * 4);
    let constants = t.vector(0x0144_d270);
    let poly = (t.float(0x0144_d288) * t6) * (t6 * t6);
    let p0 = f32::from_bits(constants[0]) - f32::from_bits(entry[0]);
    let p1 = f32::from_bits(constants[1]) - f32::from_bits(entry[1]);
    let mut result = poly + p0;
    let q4 = quotient - p1;
    let p3 = p1 + q4;
    let q1 = quotient - p3;
    result -= q1;
    result -= q4;
    f32::from_bits(sign | result.to_bits())
}

fn asin_small(t: &Tables, x: f32) -> f32 {
    let x2 = x * x;
    let a = x * t.float(0x0144_d288);
    let b = x * t.float(0x0144_d28c);
    let c = x * t.float(0x0144_d290);
    let x4 = x2 * x2;
    let a = a * x2;
    let x6 = x2 * x4;
    let b = x4 * b;
    let c = c * x6;
    ((b + a) + c) + x
}

/// `|x|` below about 2^-26 (0x012F10EA): the result is `x` (the original
/// adds a tiny constant to a scratch register only to raise the inexact
/// flag).
fn asin_tiny(x: f32) -> f32 {
    x
}

fn asin_out_of_range(t: &Tables, x: f32, magnitude: u32) -> f32 {
    if magnitude == 0x3f80_0000 {
        let sign = x.to_bits() & t.word(0x0144_d294);
        let half_pi = t.float(0x0144_d274) + t.float(0x0144_d270);
        return f32::from_bits(sign | half_pi.to_bits());
    }
    if magnitude > 0x7f80_0000 {
        // x + 0.0 of a NaN: the same NaN, quieted
        return f32::from_bits(x.to_bits() | 0x0040_0000);
    }
    // 0 * infinity: the hardware's default NaN (written out: the compiler
    // folds the product to a positive NaN)
    f32::from_bits(0xffc0_0000)
}

/// `0x3f7f0000 <= |x| < 1` (0x012F0F95).
fn asin_near_one(t: &Tables, x: f32, bits: u32) -> f32 {
    let one = t.float(0x0144_d280);
    let root = (one - x * x).sqrt();
    let high = f32::from_bits(bits & 0xffff_f000);
    let root_high = f32::from_bits(root.to_bits() & 0xffff_f000);
    let low = x - high;
    let high_squared = high * high;
    let double_high_low = (high + high) * low;
    let mut t4 = one - high_squared;
    let root_high_squared = root_high * root_high;
    let low_squared = low * low;
    t4 -= double_high_low;
    let root_low = root_high - root; // the (negative) low part of the root
    let double_root = root + root;
    t4 -= root_high_squared;
    let root_poly = root * t.float(0x0144_d288);
    t4 -= low_squared;
    let double_root_adjusted = double_root + root_low;
    let double_root_lane = root + root;
    let product = double_root_adjusted * root_low;
    t4 += product;
    let quotient = t4 / double_root_lane;
    let s = root;
    let s_cubed = s * t.float(0x0144_d28c);
    let s_squared = s * s;
    let a = root_poly * s_squared;
    let s_fourth = s_squared * s_squared;
    let sign = (bits >> 16) & 0x8000;
    let constants = t.vector(0x0144_d270);
    let b = s_cubed * s_fourth;
    let poly = a + b;
    let half_pi = f32::from_bits(constants[1]);
    let diff = s - half_pi;
    let mut result = f32::from_bits(constants[0]) - poly;
    let sum = half_pi + diff;
    result -= quotient;
    let rest = s - sum;
    result -= rest;
    result -= diff;
    f32::from_bits(result.to_bits() | (sign << 16))
}

// ---------------------------------------------------------------------------
// atan2

/// The high 16-bit word of a double's representation.
#[inline(always)]
fn top_word(bits: u64) -> u32 {
    (bits >> 48) as u32
}

/// `bits` with `add` added to its high 16-bit word (`PADDW`, wrapping).
#[inline(always)]
fn add_top_word(bits: u64, add: u32) -> u64 {
    (bits & 0x0000_ffff_ffff_ffff) | (((top_word(bits).wrapping_add(add) & 0xffff) as u64) << 48)
}

/// `RCPSS`: the host's reciprocal estimate, as the lifted program executes
/// it (the estimate differs a little between CPU makers; it moves the
/// double result by an ulp at most, which the callers' conversion to float
/// all but never shows).
#[inline(always)]
fn reciprocal_estimate(x: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: SSE is part of the x86_64 baseline
    unsafe {
        use std::arch::x86_64::{_mm_cvtss_f32, _mm_rcp_ss, _mm_set_ss};
        _mm_cvtss_f32(_mm_rcp_ss(_mm_set_ss(x)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        1.0 / x
    }
}

/// `atan2(y, x)` in double precision (0x012F1109; `y` in XMM0, `x` in
/// XMM1, the result in XMM0).
///
/// Arguments whose binary exponents are within 2^+-120 take the main path
/// ([`atan2_main`]); extreme or special ones are rescaled and retried, or
/// answered directly (zeros, infinities, NaNs, quotients beyond the table).
pub fn atan2(t: &Tables, y: f64, x: f64) -> f64 {
    let (y_bits, x_bits) = (y.to_bits(), x.to_bits());
    let in_range = |bits: u64| (top_word(bits) & 0x7ff0).wrapping_sub(0x3870) <= 0xf00;
    if in_range(y_bits) && in_range(x_bits) {
        return atan2_main(t, y_bits, x_bits, y_bits, x_bits);
    }
    atan2_special(t, y_bits, x_bits)
}

/// The sum of the two halves of a two-double constant at `address`
/// (`ADDSD` of its high double to its low one).
fn constant_sum(t: &Tables, address: u32) -> f64 {
    let p = t.pair(address);
    f64::from_bits(p[0]) + f64::from_bits(p[1])
}

/// Arguments outside the main range (0x012F1404): infinities and NaNs,
/// zeros and denormals, exponents too far apart or too extreme.
fn atan2_special(t: &Tables, y_bits: u64, x_bits: u64) -> f64 {
    let y_exponent = top_word(y_bits) & 0x7ff0;
    let x_exponent = top_word(x_bits) & 0x7ff0;
    let sign_of = |bits: u64| bits & (1 << 63);
    let mantissa_nonzero = |bits: u64| bits & 0x000f_ffff_ffff_ffff != 0;
    if y_exponent == 0x7ff0 {
        // y is infinite or NaN
        if mantissa_nonzero(y_bits) {
            return f64::from_bits(y_bits);
        }
        let sign = sign_of(y_bits);
        if x_exponent < 0x7ff0 {
            return f64::from_bits(sign | constant_sum(t, 0x0144_dce0).to_bits());
        }
        // x is infinite or NaN as well
        if mantissa_nonzero(x_bits) {
            return f64::from_bits(x_bits);
        }
        if x_bits >> 63 == 0 {
            return f64::from_bits(sign | constant_sum(t, 0x0144_dd00).to_bits());
        }
        let a = t.pair(0x0144_dd00);
        let b = t.pair(0x0144_dce0);
        let s = [f64::from_bits(a[0]) + f64::from_bits(b[0]), f64::from_bits(a[1]) + f64::from_bits(b[1])];
        return f64::from_bits(sign | (s[0] + s[1]).to_bits());
    }
    if x_exponent == 0x7ff0 {
        // x is infinite or NaN, y finite
        if mantissa_nonzero(x_bits) {
            return f64::from_bits(x_bits);
        }
        let sign = sign_of(y_bits);
        if x_bits >> 63 == 0 {
            return f64::from_bits(sign);
        }
        return f64::from_bits(sign | constant_sum(t, 0x0144_dcf0).to_bits());
    }
    // finite: bring both exponents into range
    let scale = t.double(0x0144_de48);
    let mut offset: u32 = 0x400;
    let (mut y, mut x) = (f64::from_bits(y_bits), f64::from_bits(x_bits));
    let (mut y_exp, mut x_exp) = (y_exponent, x_exponent);
    if y_exp == 0 {
        offset = offset.wrapping_sub(0x370);
        y *= scale;
        y_exp = top_word(y.to_bits()) & 0x7ff0;
        if y_exp == 0 {
            // y is zero
            return if top_word(x_bits) & 0x8000 != 0 { f64::from_bits(y.to_bits() | constant_sum(t, 0x0144_dcf0).to_bits()) } else { y };
        }
    }
    if x_exp == 0 {
        offset = offset.wrapping_add(0x370);
        x *= scale;
        x_exp = top_word(x.to_bits()) & 0x7ff0;
        if x_exp == 0 {
            // x is zero: +-pi/2
            return f64::from_bits(sign_of(y_bits) | constant_sum(t, 0x0144_dce0).to_bits());
        }
    }
    let offset = offset.wrapping_add(y_exp).wrapping_sub(x_exp);
    if offset > 0x800 {
        return atan2_generic(t, y_bits, x_bits);
    }
    let exponent = ((offset.wrapping_add(0x3bf0) & 0xffff) as u64) << 48;
    let y_rescaled = (y.to_bits() & t.quad(0x0144_de40)) | exponent;
    let x_rescaled = (x.to_bits() & t.quad(0x0144_de40)) | t.quad(0x0144_de50);
    atan2_main(t, y_rescaled, x_rescaled, y_bits, x_bits)
}

/// The quotient is outside what the table covers (0x012F139A): `+-pi/2`
/// when `y`'s exponent is the larger, `+-pi` for a negative `x`, else
/// `y / x`.
fn atan2_generic(t: &Tables, y_bits: u64, x_bits: u64) -> f64 {
    let y_exponent = (top_word(y_bits) & 0x7ff0) as i32;
    let x_exponent = (top_word(x_bits) & 0x7ff0) as i32;
    let sign_mask = t.quad(0x0144_dd10);
    if y_exponent > x_exponent {
        return f64::from_bits((y_bits & sign_mask) ^ t.quad(0x0144_dce0));
    }
    if top_word(x_bits) > 0x7fff {
        return f64::from_bits((y_bits & sign_mask) ^ t.quad(0x0144_dcf0));
    }
    f64::from_bits(y_bits) / f64::from_bits(x_bits)
}

/// The main path (0x012F114C). `y_bits` and `x_bits` are the (possibly
/// rescaled) operands; `y_original` and `x_original` the arguments for the
/// generic fallback.
///
/// A coarse quotient `q` of `|y| / |x|` (a single-precision reciprocal
/// estimate of x times y, rounded to 4 mantissa bits and capped at 32)
/// picks a table entry holding `atan(q)` in two parts; the quotient
/// `r` of the remainder `|y| - |x| q` over `|x| + |y| q` is then the small
/// argument of an odd polynomial that adds to it, with the quadrant
/// constants selected by the signs of the operands.
fn atan2_main(t: &Tables, y_bits: u64, x_bits: u64, y_original: u64, x_original: u64) -> f64 {
    let y = f64::from_bits(y_bits);
    // x as a float built from the double's top bits (exponent rebased by
    // 0x80 in the 16-bit word), its estimated reciprocal moved back into
    // a double and its exponent rebased again
    let x_as_float = (add_top_word(x_bits, 0x800) >> 29) as u32;
    let estimate = reciprocal_estimate(f32::from_bits(x_as_float));
    let reciprocal = add_top_word((estimate.to_bits() as u64) << 29, 0x3800);
    let coarse = f64::from_bits(reciprocal) * y;
    // round at mantissa bit 47, drop the sign and the lower 48 bits
    let rounded = coarse.to_bits().wrapping_add(1 << 47) & (0x7fffu64 << 48);
    let q = f64::from_bits(rounded);
    let capped = if q < 32.0 { q } else { 32.0 };
    let signs = (y_bits >> 63) as u32 | (((x_bits >> 63) as u32) << 1);
    let abs_y = y_bits & !(1 << 63);
    let abs_x = x_bits & !(1 << 63);
    let small = q <= 32.0;
    let x_high = f64::from_bits(abs_x & !0xffff);
    let x_low = f64::from_bits(abs_x) - x_high;
    let y_by_q = f64::from_bits(abs_y) * capped;
    let x_high_by_q = x_high * capped;
    let x_low_by_q = x_low * capped;
    let y_kept = if small { abs_y } else { 0 };
    let x_kept = if small { abs_x } else { 0 };
    let numerator = (f64::from_bits(y_kept) - x_high_by_q) - x_low_by_q;
    let denominator = y_by_q + f64::from_bits(x_kept);
    let index = (top_word(rounded) & 0x7ff0).wrapping_sub(0x3f9e);
    if index > 0x461 {
        // the quotient is tiny (a series in y/x) or huge (generic)
        if index.wrapping_add(0x3ae) > 0x3ae {
            return atan2_generic(t, y_original, x_original);
        }
        return atan2_tiny_quotient(t, numerator, abs_y, abs_x, signs, capped);
    }
    let r = numerator / denominator;
    let table = signs * 2 * 8;
    let quadrant = t.pair(0x0144_dd20 + table);
    let flip = t.pair(0x0144_dd60 + table);
    let entry_index = top_word(capped.to_bits()).wrapping_sub(0x3f9e).wrapping_add(if small { 0 } else { 1 }).wrapping_mul(2);
    let entry = t.pair(0x0144_d2a0 + entry_index * 8);
    let entry_high = f64::from_bits(entry[0] ^ flip[0]) + f64::from_bits(quadrant[0]);
    let entry_low = f64::from_bits(entry[1] ^ flip[1]) + f64::from_bits(quadrant[1]);
    let head = f64::from_bits(flip[0] ^ r.to_bits());
    let r_squared = r * r;
    let c0 = t.double(0x0144_de20) * r_squared;
    let c1 = t.double(0x0144_de30) + r_squared;
    let sum = head + entry_high;
    let mut tail = entry_high - sum;
    tail += head;
    let c0 = c0 + t.double(0x0144_de28);
    let c1 = c1 * r_squared;
    let head_cubed = head * r_squared;
    tail += entry_low;
    let c0 = c0 * head_cubed;
    let c1 = c1 + t.double(0x0144_de38);
    let poly = c0 * c1;
    sum + (poly + tail)
}

/// A small quotient `|y| / |x|` (0x012F12F7): the polynomial in `y / x`
/// itself, with the quadrant constants.
fn atan2_tiny_quotient(t: &Tables, numerator: f64, abs_y: u64, abs_x: u64, signs: u32, capped: f64) -> f64 {
    let reciprocal = 1.0 / f64::from_bits(abs_x);
    let table = signs * 2 * 8;
    let flip = t.pair(0x0144_dd60 + table);
    let mask = t.pair(0x0144_dde0 + table);
    let base = t.pair(0x0144_dda0 + table);
    let u = f64::from_bits(abs_y ^ flip[0]) * reciprocal;
    let v = f64::from_bits(numerator.to_bits() ^ flip[0]) * reciprocal;
    let capped_lanes = [capped.to_bits() ^ flip[0], capped.to_bits() ^ flip[1]];
    let masked = [capped_lanes[0] & mask[0], capped_lanes[1] & mask[1]];
    let u_squared = u * u;
    let c0 = t.double(0x0144_de20) * u_squared;
    let c1 = t.double(0x0144_de30) + u_squared;
    let u_cubed = u * u_squared;
    let c0 = c0 + t.double(0x0144_de28);
    let c1 = c1 * u_squared;
    let lane0 = (f64::from_bits(base[0]) + v) + f64::from_bits(masked[0]);
    let lane1 = f64::from_bits(base[1]) + f64::from_bits(masked[1]);
    let c0 = c0 * u_cubed;
    let c1 = c1 + t.double(0x0144_de38);
    let poly = c1 * c0;
    (poly + lane0) + lane1
}
