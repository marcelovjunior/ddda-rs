//! Semantic ports (L176) of the draw context's frustum tests, on the
//! model-draw path: a sphere, an axis-aligned box and a transformed box
//! against the six planes at `context + 0xCA0` (16-byte aligned; each
//! plane `a, b, c, d` from offset -8 of its 16 bytes, as the original
//! addresses them). They keep the original's ABI-visible result (EAX, the
//! callee-saved registers, ESP; they write no memory outside their stack
//! scratch). The float steps are the original's, in its order: Rust's
//! f32 operations are the same scalar SSE instructions under the live
//! MXCSR, never reassociated or contracted. Checked against the
//! original's recorded vectors in tests/material.rs; opt-in through
//! crates/ddports (`DDRUNTIME_CULL=1`).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// The sign mask and the absolute-value mask (image constants).
const SIGN: u32 = 0x0161_C280;
const ABS: u32 = 0x0161_C330;
/// One half (the box centre).
const HALF: u32 = 0x0140_6CF0;
/// The transformed box's distance scales (four floats).
const SCALES: u32 = 0x0162_9490;

fn f(pg: Pages, m: &dyn Mem, a: u32) -> f32 {
    f32::from_bits(pg.u32(m, a))
}

/// The first plane's `c` (planes at 16-byte steps; `a` at -8, `b` at -4,
/// `d` at +4).
fn planes(pg: Pages, m: &dyn Mem, ctx: u32) -> u32 {
    (pg.u32(m, ctx.wrapping_add(0xca0)) & !0xf).wrapping_add(8)
}

// Translated from 0x00FF0780 (DDDA.exe 2017-12-13 Steam).
/// 0x00FF0780, `context->cull_sphere(sphere) -> planes` (thiscall, `ret
/// 4`): -1 when the sphere (`x, y, z, r`) lies wholly behind a plane;
/// otherwise the mask of the planes it lies wholly in front of
/// (distance `>= r`).
pub fn cull_sphere(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let s = pg.u32(m, c.esp.wrapping_add(4));
    let r = f(pg, m, s.wrapping_add(0xc));
    let (x, y, z) = (f(pg, m, s), f(pg, m, s.wrapping_add(4)), f(pg, m, s.wrapping_add(8)));
    let mut p = planes(pg, m, c.ecx);
    let minus_r = f32::from_bits(r.to_bits() ^ pg.u32(m, SIGN));
    let mut mask = 0u32;
    c.eax = 'out: {
        for i in 0..6 {
            let d = f(pg, m, p.wrapping_sub(8)) * x + f(pg, m, p.wrapping_sub(4)) * y;
            let d = d + f(pg, m, p) * z;
            let d = d + f(pg, m, p.wrapping_add(4));
            if minus_r > d {
                break 'out u32::MAX;
            }
            if d >= r {
                mask |= 1 << i;
            }
            p = p.wrapping_add(16);
        }
        mask
    };
    c.esp = c.esp.wrapping_add(8);
}

// Translated from 0x00FF0D20 (DDDA.exe 2017-12-13 Steam).
/// 0x00FF0D20, `context->cull_box(box, known) -> planes` (thiscall, `ret
/// 8`): the axis-aligned box (`min` at 0, `max` at 0x10) as its centre and
/// half extents; planes whose bit is set in `known` are skipped. -1 when
/// the box lies wholly behind a plane; otherwise `known` with the bits of
/// the planes it lies wholly in front of.
pub fn cull_box(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let b = pg.u32(m, c.esp.wrapping_add(4));
    let (max_x, max_y, max_z) = (f(pg, m, b.wrapping_add(0x10)), f(pg, m, b.wrapping_add(0x14)), f(pg, m, b.wrapping_add(0x18)));
    let half = f(pg, m, HALF);
    let (min_y, min_z) = (f(pg, m, b.wrapping_add(4)), f(pg, m, b.wrapping_add(8)));
    let cx = (max_x + f(pg, m, b)) * half;
    let cy = (min_y + max_y) * half;
    let cz = (min_z + max_z) * half;
    let abs = pg.u32(m, ABS);
    let mut p = planes(pg, m, c.ecx);
    let known = pg.u32(m, c.esp.wrapping_add(8));
    let (ex, ey, ez) = (max_x - cx, max_y - cy, max_z - cz);
    let a_ = |v: f32| f32::from_bits(v.to_bits() & abs);
    let mut mask = known;
    c.eax = 'out: {
        for i in 0..6 {
            let bit = 1u32 << i;
            if known & bit == 0 {
                let (pa, pb, pc) = (f(pg, m, p.wrapping_sub(8)), f(pg, m, p.wrapping_sub(4)), f(pg, m, p));
                let d = pb * cy + pa * cx;
                let r = a_(pb) * ey + a_(pa) * ex;
                let r = r + a_(pc) * ez;
                let d = d + pc * cz;
                let d = d + f(pg, m, p.wrapping_add(4));
                if 0.0 > d + r {
                    break 'out u32::MAX;
                }
                if d - r > 0.0 {
                    mask |= bit;
                }
            }
            p = p.wrapping_add(16);
        }
        mask
    };
    c.esp = c.esp.wrapping_add(12);
}

// Translated from 0x00FF0AD0 (DDDA.exe 2017-12-13 Steam).
/// 0x00FF0AD0, `context->cull_oriented(transform, known) -> planes`
/// (thiscall, `ret 8`): as `cull_box` for a box given as a 4x4 transform
/// (row-major, 16 floats) and its half extents (floats 16..18): each plane
/// is taken into the box's space (the transform's columns, lane by lane
/// as the original's packed steps), scaled by the plane normal's length
/// and the four scales at 0x01629490.
pub fn cull_oriented(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let t = pg.u32(m, c.esp.wrapping_add(4));
    let at = |k: u32| t.wrapping_add(4 * k);
    // the transform's columns
    let column = |j: u32| [0, 4, 8, 12].map(|i| f(pg, m, at(i + j)));
    let (c0, c1, c2, c3) = (column(0), column(1), column(2), column(3));
    let abs = pg.u32(m, ABS);
    let p0 = planes(pg, m, c.ecx);
    let (hx, hy, hz) = (f(pg, m, at(16)), f(pg, m, at(17)), f(pg, m, at(18)));
    let known = pg.u32(m, c.esp.wrapping_add(8));
    let a_ = |v: f32| f32::from_bits(v.to_bits() & abs);
    let mut p = p0;
    let mut mask = known;
    c.eax = 'out: {
        for i in 0..6 {
            let bit = 1u32 << i;
            if known & bit == 0 {
                let (pa, pc, pb) = (f(pg, m, p.wrapping_sub(8)), f(pg, m, p), f(pg, m, p.wrapping_sub(4)));
                let length = ((pb * pb + pa * pa) + pc * pc).sqrt();
                let pd = f(pg, m, p.wrapping_add(4));
                // ((c0*a + c1*b) + c2*c) + d*c3, lane by lane
                let lane = |k: usize| ((c0[k] * pa + c1[k] * pb) + c2[k] * pc) + pd * c3[k];
                let (t0, t1, t2, t3) = (lane(0), lane(1), lane(2), lane(3));
                let (x0, x2, x3) = (t1 * length, t0 * length, t2 * length);
                let x4 = t3 * length * f(pg, m, SCALES.wrapping_add(12));
                let d = x0 * f(pg, m, SCALES.wrapping_add(4)) + x2 * f(pg, m, SCALES);
                let d = d + x3 * f(pg, m, SCALES.wrapping_add(8));
                let r = a_(x0) * hy + a_(x2) * hx;
                let d = d + x4;
                let r = r + a_(x3) * hz;
                if 0.0 > d + r {
                    break 'out u32::MAX;
                }
                if d - r > 0.0 {
                    mask |= bit;
                }
            }
            p = p.wrapping_add(16);
        }
        mask
    };
    c.esp = c.esp.wrapping_add(12);
}
