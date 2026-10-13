//! L184 s7: semantic port of 0x00D1B840, the segment-versus-box test.
//!
//! `bool segment_vs_box(const Aabb *box, const Segment *segment, float *t)`
//! (cdecl, caller pops): does the line segment from `segment[0..3]` to
//! `segment[4..7]` (16-byte aligned, xyz + pad) touch the axis-aligned box
//! with minimum `box[0..3]` and maximum `box[4..7]`? On a hit, `*t` (when
//! non-null) receives the distance along the segment at which it enters
//! the box (0 for a degenerate, point-like segment; -FLT_MAX when no
//! slab bound it).
//!
//! A segment shorter than 2^-23 squared is tested as a point. Otherwise
//! the slab method is run per axis in x, z, y order on the normalised
//! direction: an axis whose direction component is below 0.001 only
//! checks the start lies between the box planes; the others intersect the
//! entry/exit distances (rejecting a box wholly behind the start or
//! beyond the end of the segment). Division by a zero component gives
//! 0, as in the original.
//!
//! The arithmetic is the original's single-precision SSE arithmetic in
//! the original's order. The original returns a bool in AL and leaves
//! the rest of EAX as the compiler left it (a pointer argument); that is
//! reproduced too.
//!
//! Translated from 0x00D1B840 (DDDA.exe 2017-12-13 Steam).
use crate::semantic_s7::F;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

const EPSILON_SQUARED: F = F::from_bits(0x3400_0000); // 2^-23 (0x01407124)
const PARALLEL_LIMIT: F = F::from_bits(0x3a83_126f); // 0.001 (0x01408E58)
const NEGATIVE_MAX: F = F::from_bits(0xff7f_ffff); // -FLT_MAX (0x01408E5C)
const POSITIVE_MAX: F = F::from_bits(0x7f7f_ffff); // FLT_MAX (0x0161C1F4)
const ZERO: F = F(0.0);
const ONE: F = F(1.0);

#[inline(always)]
fn load(pg: Pages, m: &dyn Mem, address: u32) -> F {
    F::from_bits(pg.u32(m, address))
}

#[inline(always)]
fn gt(a: F, b: F) -> bool {
    a.0 > b.0
}

/// `1 / v`, or 0 for an exactly zero `v` (what the original does for every
/// direction component).
#[inline(always)]
fn reciprocal_or_zero(v: F) -> F {
    if v.0 == 0.0 { ZERO } else { ONE / v }
}

#[inline(always)]
fn abs(v: F) -> F {
    F::from_bits(v.to_bits() & 0x7fff_ffff)
}

/// What the slab test of one axis does after the common rejections.
struct Slab {
    low: F,
    high: F,
}

/// The entry/exit distances of one non-parallel axis, ordered, or `None`
/// when the box is wholly behind the start or past the end.
#[inline(always)]
fn slab(t_min: F, t_max: F, t_end: F) -> Option<Slab> {
    if gt(ZERO, t_min) && gt(ZERO, t_max) {
        return None;
    }
    if gt(t_min, t_end) && gt(t_max, t_end) {
        return None;
    }
    Some(if gt(t_min, t_max) { Slab { low: t_max, high: t_min } } else { Slab { low: t_min, high: t_max } })
}

/// 0x00D1B840: see the module docs. Cdecl: `[esp+4]` box, `[esp+8]`
/// segment, `[esp+0xc]` out distance (may be null).
pub fn segment_vs_box(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let segment = pg.u32(m, entry.wrapping_add(8));
    let (hit, eax) = test(m, pg, entry, segment);
    c.eax = eax.unwrap_or(if hit { 1 } else { segment & !0xff });
    let _ = hit;
    c.esp = entry.wrapping_add(4);
}

/// Returns the verdict and, when the original's EAX is not the plain
/// "segment pointer with AL cleared", that EAX.
fn test(m: &mut dyn Mem, pg: Pages, entry: u32, segment: u32) -> (bool, Option<u32>) {
    let at = |i: u32| segment.wrapping_add(4 * i);
    let (sx, sy, sz) = (load(pg, m, at(0)), load(pg, m, at(1)), load(pg, m, at(2)));
    let (ex, ey, ez) = (load(pg, m, at(4)), load(pg, m, at(5)), load(pg, m, at(6)));
    let dx = ex - sx;
    let dy = ey - sy;
    let dz = ez - sz;
    let length_squared = (dy * dy + dx * dx) + dz * dz;
    if gt(EPSILON_SQUARED, length_squared) {
        return point_in_box(m, pg, entry, segment);
    }
    // `lahf` after the last `ucomiss` (direction x against 0) leaves the
    // flags in AH, which survives into EAX on every failing return
    let fail_eax = |nx: F| -> u32 {
        let flags: u32 = if nx.0.is_nan() { 0x47 } else if nx.0 == 0.0 { 0x42 } else if nx.0 < 0.0 { 0x03 } else { 0x02 };
        (segment & 0xffff_0000) | (flags << 8)
    };
    let box_ptr = pg.u32(m, entry.wrapping_add(4));
    let b = |i: u32| load(pg, m, box_ptr.wrapping_add(4 * i));
    let (min_x, min_y, min_z) = (b(0), b(1), b(2));
    let (max_x, max_y, max_z) = (b(4), b(5), b(6));
    // the box planes relative to the segment start
    let (rel_min_x, rel_min_y, rel_min_z) = (min_x - sx, min_y - sy, min_z - sz);
    let (rel_max_x, rel_max_y, rel_max_z) = (max_x - sx, max_y - sy, max_z - sz);
    let inverse_length = ONE / ((dy * dy + dx * dx) + dz * dz).sqrt();
    let nx = dx * inverse_length;
    let ny = dy * inverse_length;
    let nz = dz * inverse_length;
    let inv_z = reciprocal_or_zero(nz);
    let inv_y = reciprocal_or_zero(ny);
    let inv_x = reciprocal_or_zero(nx);
    let t_end_y = dy * inv_y;
    let t_y_min = inv_y * rel_min_y;
    let t_y_max = inv_y * rel_max_y;
    let t_end_z = dz * inv_z;
    let t_end_x = inv_x * dx;
    let t_x_min = inv_x * rel_min_x;
    let t_x_max = inv_x * rel_max_x;
    let t_z_min = inv_z * rel_min_z;
    let t_z_max = inv_z * rel_max_z;
    let mut near = NEGATIVE_MAX;
    let mut far = POSITIVE_MAX;

    // x
    if gt(PARALLEL_LIMIT, abs(nx)) {
        if gt(min_x, sx) || gt(sx, b(4)) {
            return (false, Some(fail_eax(nx)));
        }
    } else {
        let Some(s) = slab(t_x_min, t_x_max, t_end_x) else { return (false, Some(fail_eax(nx))) };
        if gt(s.low, NEGATIVE_MAX) {
            near = s.low;
        }
        if gt(POSITIVE_MAX, s.high) {
            far = s.high;
        }
        if gt(near, far) {
            return (false, Some(fail_eax(nx)));
        }
    }
    // z
    if gt(PARALLEL_LIMIT, abs(nz)) {
        if gt(min_z, sz) || gt(sz, b(6)) {
            return (false, Some(fail_eax(nx)));
        }
    } else {
        let Some(s) = slab(t_z_min, t_z_max, t_end_z) else { return (false, Some(fail_eax(nx))) };
        if gt(s.low, near) {
            near = s.low;
        }
        if gt(far, s.high) {
            far = s.high;
        }
        if gt(near, far) {
            return (false, Some(fail_eax(nx)));
        }
    }
    // y (a parallel y axis ends the test without a further interval)
    if gt(PARALLEL_LIMIT, abs(ny)) {
        if gt(min_y, sy) || gt(sy, b(5)) {
            return (false, Some(fail_eax(nx)));
        }
    } else {
        let Some(s) = slab(t_y_min, t_y_max, t_end_y) else { return (false, Some(fail_eax(nx))) };
        if gt(s.low, near) {
            near = s.low;
        }
        if gt(far, s.high) {
            far = s.high;
        }
        if gt(near, far) {
            return (false, Some(fail_eax(nx)));
        }
    }
    let out = pg.u32(m, entry.wrapping_add(12));
    if out != 0 {
        pg.set_u32(m, out, near.to_bits());
    }
    (true, Some((out & !0xff) | 1))
}

/// A segment shorter than 2^-11: is its start inside the box (x, y, z
/// inclusive; the pad lane is ignored)? Writes distance 0.
fn point_in_box(m: &mut dyn Mem, pg: Pages, entry: u32, segment: u32) -> (bool, Option<u32>) {
    let out = pg.u32(m, entry.wrapping_add(12));
    if out != 0 {
        pg.set_u32(m, out, 0);
    }
    let box_ptr = pg.u32(m, entry.wrapping_add(4));
    let inside = (0..3u32).all(|i| {
        let p = load(pg, m, segment.wrapping_add(4 * i));
        let lo = load(pg, m, box_ptr.wrapping_add(4 * i));
        let hi = load(pg, m, box_ptr.wrapping_add(0x10 + 4 * i));
        lo.0 <= p.0 && p.0 <= hi.0
    });
    (inside, Some(inside as u32))
}
