//! More CRT float routines, ported bit for bit: `acosf` (0x012F161F).
//!
//! The routine is a table-driven polynomial/double-float-split evaluation
//! in single precision: the argument's magnitude picks one of five ranges
//! (index by `|x|` bits minus 0x3d800000) and every range does its own
//! sequence of hi/lo-split products. The tables (at 0x0144DE60,
//! 0x0144E660 and the constants at 0x0144EA60..0x0144EA88) are read from
//! guest memory, like the original does, so no game data is copied here.
//!
//! Translated from 0x012F161F (DDDA.exe 2017-12-13 Steam).
use crate::semantic_s7::F;
use ddnative_base::mem::{Mem, Pages};

const TABLE_A: u32 = 0x0144_de60; // 16-byte rows, two floats used
const TABLE_B: u32 = 0x0144_e660; // single floats
const PI_PARTS: u32 = 0x0144_ea60; // two floats: pi/2 high and low
const MASK_PAIR: u32 = 0x0144_ea70;
const SIGNED_PI_PARTS: u32 = 0x0144_ea80;

#[inline(always)]
fn fl(bits: u32) -> F {
    F::from_bits(bits)
}
#[inline(always)]
fn rd(pg: Pages, m: &dyn Mem, address: u32) -> F {
    fl(pg.u32(m, address))
}

/// The CRT `acosf` (argument and result in XMM0).
pub fn acosf(m: &dyn Mem, pg: Pages, x: f32) -> f32 {
    let x = F(x);
    let eax = x.to_bits();
    let one = fl(0x3f80_0000);
    let magnitude = (eax & 0x7fff_ffff).wrapping_sub(0x3d80_0000);
    // |x| in [0.0625, ~0.4]
    if magnitude < 0x01dd_b3d7 {
        let index = (magnitude >> 16) & 0x1fe;
        let s = (one - x * x).sqrt();
        let t5 = rd(pg, m, TABLE_B + index * 2);
        let row0 = rd(pg, m, TABLE_A + index * 4);
        let row1 = rd(pg, m, TABLE_A + index * 4 + 4);
        // c: x's sign, exponent and top mantissa bits, plus one at bit 16
        let c = fl((eax & 0xfffe_0000) | 0x0001_0000);
        let sum = x + c; // xmm1
        let xt = x * t5; // xmm6
        let diff = x - c; // xmm7
        let prod = diff * sum;
        let cs = c * s;
        let denom = xt + cs;
        let quotient = prod / denom; // xmm7
        let d = cs - xt; // xmm2
        let cubic = (fl(0x3e2a_aaab) * d) * (d * d); // xmm0
        let sign = fl(eax & 0x8000_0000);
        let sign_xor = |v: F| fl(v.to_bits() ^ sign.to_bits());
        let lane0 = rd(pg, m, PI_PARTS) - sign_xor(row0);
        let lane1 = rd(pg, m, PI_PARTS + 4) - sign_xor(row1);
        let r0 = cubic + lane0; // xmm0
        let a = lane1 - quotient; // xmm5
        let b = lane1 - a; // xmm6
        let c2 = quotient - b; // xmm7
        return ((r0 - c2) + a).0;
    }
    // |x| in [~0.4, ~0.9]
    let rest = magnitude.wrapping_sub(0x01dd_b3d7);
    if rest < 0x0020_4c29 {
        // split x into a 12-bit-cleared head and a tail
        let head = fl(eax & 0xffff_f000);
        let tail = x - head;
        let head_sq = head * head;
        let r_plus = x + head;
        let mul = r_plus * tail;
        let t = one - head_sq; // xmm3 (then xmm4 copy)
        let t_minus = t - mul;
        let s = t_minus.sqrt(); // xmm3
        let sign_bits = eax & 0x8000_0000;
        let negative_mask = if sign_bits != 0 { 0xffff_ffffu32 } else { 0 };
        let s_bits = s.to_bits();
        let word1 = s_bits >> 16; // pextrw 1
        let c = fl((s_bits & 0xfffe_0000) | 0x0001_0000 | sign_bits);
        let index = word1.wrapping_sub(0x3d80) & 0xfffe;
        let s_scaled = s * rd(pg, m, TABLE_B + index * 2); // xmm3
        let xh_c = head * c; // xmm6
        let xl_c = tail * c; // xmm1
        let c_sq = c * c; // xmm2
        let mask_lane0 = fl(negative_mask & pg.u32(m, MASK_PAIR));
        let mask_lane1 = fl(negative_mask & pg.u32(m, MASK_PAIR + 4));
        let q = (xh_c - s_scaled) + xl_c; // xmm6
        let num = (t - c_sq) - mul; // xmm4
        let den = (s_scaled + s_scaled) + q; // xmm3
        let poly_scale = (fl(0xbe2a_aaab) * q) * (q * q); // xmm5
        let ratio = num / den;
        let lane0 = mask_lane0 + rd(pg, m, TABLE_A + index * 4);
        let lane1 = mask_lane1 + rd(pg, m, TABLE_A + index * 4 + 4);
        let result = ((poly_scale + lane0) + ratio) + lane1;
        return f32::from_bits((negative_mask & 0x8000_0000) ^ result.to_bits());
    }
    // |x| in [2^-16, 0.0625): short polynomial
    let tail_class = magnitude.wrapping_add(0x0600_0000);
    if tail_class < 0x0600_0000 {
        let xm = x; // xmm5 / xmm4 copies
        let x2 = xm * xm;
        let a = fl(0xbe2a_aaab) * xm; // -x/6
        let b = fl(0xbd99_999a) * xm; // -0.075 x
        let hi = rd(pg, m, PI_PARTS);
        let lo = rd(pg, m, PI_PARTS + 4);
        let x4 = x2 * x2;
        let a = a * x2;
        let r = xm - lo; // xmm4
        let x4b = x4 * b;
        let lo2 = lo + r; // xmm7
        let a = a + hi;
        let d = xm - lo2; // xmm5
        let out = x4b + a;
        return ((out - d) - r).0;
    }
    // |x| in [0.9921875, 1)
    let near_one = tail_class.wrapping_sub(0x07fe_0000);
    if near_one < 0x0002_0000 {
        let half = fl(0x3f00_0000);
        let ax = fl(eax & 0x7fff_ffff);
        let ax_half = ax * half; // xmm7
        let t = half - ax_half; // xmm4
        let sqrt_t = t.sqrt(); // xmm4
        let negative_mask = if eax & 0x8000_0000 != 0 { 0xffff_ffffu32 } else { 0 };
        let c1 = fl(0xbd99_999a) * t; // xmm1
        let t2 = t * t;
        let p = fl(0xbe2a_aaab) + c1; // xmm2
        let c3 = fl(0xbd36_db6e) * t2;
        let p = p + c3;
        let mask0 = fl(negative_mask & pg.u32(m, SIGNED_PI_PARTS));
        let mask1 = fl(negative_mask & pg.u32(m, SIGNED_PI_PARTS + 4));
        let p = p * t;
        let head = fl(0xffff_f000 & sqrt_t.to_bits());
        let s = sqrt_t; // xmm3 (broadcast)
        let tail = sqrt_t - head; // xmm4
        let two_s = s + s;
        let head_sq = head * head;
        let two_s = two_s - tail; // xmm3 low lane
        let t_minus = t - head_sq; // xmm5
        let prod = tail * two_s; // xmm4
        let t_minus = t_minus - prod;
        let q = t_minus / s; // xmm5 (xmm3 = lane 1 = s)
        let two_s2 = s + s; // xmm3
        let p = p * two_s2;
        let p = p + mask0;
        let flip = (negative_mask & 0x8000_0000) ^ 0x8000_0000;
        let p = p - q;
        let p = p - two_s2;
        let r = mask1 + p;
        return f32::from_bits(r.to_bits() ^ flip);
    }
    // |x| < 2^-16, |x| == 1, NaN, |x| > 1
    let abs_bits = near_one.wrapping_add(0x3f7e_0000);
    if abs_bits < 0x3780_0000 {
        let hi = rd(pg, m, PI_PARTS);
        let lo = rd(pg, m, PI_PARTS + 4);
        let r = x - lo; // xmm1
        let u = lo + r; // xmm2
        let u = u - x;
        let out = hi + u;
        return (out - r).0;
    }
    if abs_bits == 0x3f80_0000 {
        let negative_mask = if eax & 0x8000_0000 != 0 { 0xffff_ffffu32 } else { 0 };
        let a = pg.u32(m, SIGNED_PI_PARTS) & negative_mask;
        let b = negative_mask & pg.u32(m, SIGNED_PI_PARTS + 4);
        return (fl(b) + fl(a)).0;
    }
    if abs_bits > 0x7f80_0000 {
        return (x + fl(0)).0;
    }
    (fl(0) * fl(0x7f80_0000)).0
}
