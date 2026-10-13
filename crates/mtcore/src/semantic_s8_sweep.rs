//! L184 s8: swept capsule-versus-triangle collision (GJK) of 0x00D7CF70 and
//! its helpers, as readable Rust.
//!
//! The shape is two convex pieces: a capsule (two points at +0 and +0x10
//! and a radius at +0x20) and a triangle (points at +0, +0x10, +0x20). The
//! sweep moves the capsule along a displacement and finds the first
//! contact with a GJK/conservative-advancement loop over a simplex that
//! holds up to four points of the Minkowski difference. Offsets and
//! arithmetic come from the 2017-12-13 Steam executable; every float is
//! computed in the original's operation order (SSE in the original), so
//! results are bit-identical. The x87 FLD/FSTP point copies of the
//! original are bit copies except that a signalling NaN comes out quiet
//! ([`quiet`]); the one extended-precision computation (the squared
//! distance of a point to a segment, 0x00D53C70) runs on the real x87 unit
//! through [`ddnative_base::x87::X87`], like the lifted code.
use crate::semantic::call_from;
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

type Vector = [f32; 3];

const ONE: f32 = 1.0; // 0x01406CE0
const EPSILON: f32 = 1.1920929e-7; // 0x01407124
const CONTACT_MARGIN: f32 = f32::from_bits(0x3da3_d70a); // 0x01408E74
const COINCIDENT: f32 = f32::from_bits(0x3a83_126f); // 0.001, 0x01408E58
const SIGN: u32 = 0x8000_0000; // 0x0161C280

/// `.data` globals the original reads: the zero vector (0x018CFE80), the
/// default displacement (1, 0, 0) at 0x01629530 and the "no closest point
/// yet" vector (FLT_MAX x3) at 0x01629570.
const ZERO_VECTOR: u32 = 0x018c_fe80;
const DEFAULT_DISPLACEMENT: u32 = 0x0162_9530;
const FAR_POINT: u32 = 0x0162_9570;

/// The simplex record (owner of the closest-point search state).
mod simplex {
    /// Number of points (0..4).
    pub const COUNT: u32 = 0;
    /// Points of the Minkowski difference, 16 bytes each from +0x10.
    pub const POINTS: u32 = 0x10;
    /// The same points' witnesses on the capsule (+0x70) and the triangle (+0xd0).
    pub const CAPSULE_WITNESS: u32 = 0x70;
    pub const TRIANGLE_WITNESS: u32 = 0xd0;
    /// Closest point of the simplex to the origin.
    pub const CLOSEST: u32 = 0x150;
    /// The point being added.
    pub const NEWEST: u32 = 0x160;
    /// Word at +0x170: byte 0 "valid", byte 1 "dirty".
    pub const VALID: u32 = 0x170;
    pub const DIRTY: u32 = 0x171;
}

#[inline(always)]
fn rf(pg: Pages, m: &dyn Mem, a: u32) -> f32 {
    f32::from_bits(pg.u32(m, a))
}
#[inline(always)]
fn vector(pg: Pages, m: &dyn Mem, a: u32) -> Vector {
    [rf(pg, m, a), rf(pg, m, a.wrapping_add(4)), rf(pg, m, a.wrapping_add(8))]
}
#[inline(always)]
fn store(pg: Pages, m: &mut dyn Mem, a: u32, v: Vector) {
    for (i, x) in v.into_iter().enumerate() {
        pg.set_u32(m, a.wrapping_add(4 * i as u32), x.to_bits());
    }
}
#[inline(always)]
fn dot(a: Vector, b: Vector) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}
/// Dot product with the y product first (as the original's code orders it).
#[inline(always)]
fn dot_yxz(a: Vector, b: Vector) -> f32 {
    (a[1] * b[1] + a[0] * b[0]) + a[2] * b[2]
}
#[inline(always)]
fn sub(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline(always)]
fn scale(a: Vector, s: f32) -> Vector {
    [s * a[0], a[1] * s, a[2] * s]
}
/// What `FLD m32; FSTP m32` does to a word: a signalling NaN comes out
/// quiet, every other value is unchanged.
#[inline(always)]
fn quiet(bits: u32) -> u32 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        bits | 0x0040_0000
    } else {
        bits
    }
}
/// `out = vector` (through the x87 stack in the original), `out.w = 0`.
fn copy_vector(pg: Pages, m: &mut dyn Mem, out: u32, v: Vector) {
    for i in 0..3 {
        pg.set_u32(m, out.wrapping_add(4 * i as u32), quiet(v[i].to_bits()));
    }
    pg.set_u32(m, out.wrapping_add(12), 0);
}
/// `out = quiet(vector at from)`, then `out.w = 0` (x87 copy of three words).
fn copy_point(pg: Pages, m: &mut dyn Mem, out: u32, from: u32) {
    for i in 0..3 {
        let v = quiet(pg.u32(m, from.wrapping_add(4 * i)));
        pg.set_u32(m, out.wrapping_add(4 * i), v);
    }
    pg.set_u32(m, out.wrapping_add(12), 0);
}

// ---- 0x00D63FF0: support points ---------------------------------------------

/// Support points of the shape in `direction`: the capsule's farthest end
/// plus `radius * direction` into `shape + 0x10`, and the triangle vertex
/// farthest along `-direction` (all ones when that is the zero vector) into
/// `shape + 0x20`. Returns the capsule pointer (the original's EAX).
fn support(pg: Pages, m: &mut dyn Mem, shape: u32, d: Vector) -> u32 {
    let capsule = pg.u32(m, shape);
    let p0 = vector(pg, m, capsule);
    let p1 = vector(pg, m, capsule.wrapping_add(0x10));
    let near = if dot(d, p0) > (rf(pg, m, capsule.wrapping_add(0x10)) * d[0] + rf(pg, m, capsule.wrapping_add(0x14)) * d[1]) + p1[2] * d[2] {
        p0
    } else {
        p1
    };
    let radius = rf(pg, m, capsule.wrapping_add(0x20));
    pg.set_u32(m, shape.wrapping_add(0x10), (d[0] * radius + near[0]).to_bits());
    pg.set_u32(m, shape.wrapping_add(0x18), (d[2] * radius + near[2]).to_bits());
    pg.set_u32(m, shape.wrapping_add(0x14), (near[1] + d[1] * radius).to_bits());
    pg.set_u32(m, shape.wrapping_add(0x1c), 0);
    let triangle = pg.u32(m, shape.wrapping_add(4));
    let mut n = [-d[0], -d[1], -d[2]];
    if ((n[1] * n[1]) + (n[0] * n[0])) + (n[2] * n[2]) == 0.0 {
        n = [ONE; 3];
    }
    let side = |m: &dyn Mem, k: u32| -> (Vector, f32) {
        let v = vector(pg, m, triangle.wrapping_add(0x10 * k));
        (v, (v[1] * n[1] + v[0] * n[0]) + v[2] * n[2])
    };
    let (v0, e0) = side(m, 0);
    let (v1, e1) = side(m, 1);
    let (v2, e2) = side(m, 2);
    let (mut best, mut best_e) = if e0 > e1 { (v0, e0) } else { (v1, e1) };
    if e2 > best_e {
        best = v2;
        best_e = e2;
    }
    let _ = best_e;
    store(pg, m, shape.wrapping_add(0x20), best);
    pg.set_u32(m, shape.wrapping_add(0x2c), 0);
    capsule
}

/// 0x00D63FF0, `shape->support(direction)` (thiscall, `ret 4`): see [`support`].
///
/// Translated from 0x00d63ff0 (DDDA.exe 2017-12-13 Steam).
pub fn shape_support(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let d = vector(pg, m, pg.u32(m, entry.wrapping_add(4)));
    c.eax = support(pg, m, c.ecx, d);
    c.esp = entry.wrapping_add(8);
}

// ---- 0x00D1B540: simplex insertion ------------------------------------------

/// Squared distance of `p` to the segment `s0..s1` (0x00D53C70), computed
/// with the original's mixed precision, compared with `COINCIDENT`:
/// true when `COINCIDENT > distance` fails, i.e. the original's `JBE`
/// after `FCOMIP` (the point is far enough from the segment).
fn point_is_off_segment(c: &mut Cpu, s0: Vector, s1: Vector, p: Vector) -> bool {
    let e0 = s1[0] - s0[0];
    let e1 = s1[1] - s0[1];
    let e2 = s1[2] - s0[2];
    let dx = p[0] - s0[0];
    let dy = p[1] - s0[1];
    let dz = p[2] - s0[2];
    let inverse = ONE / ((e1 * e1 + e0 * e0) + e2 * e2).sqrt();
    let u2 = e2 * inverse;
    let u0 = inverse * e0;
    let u1 = e1 * inverse;
    let length = (e1 * u1 + e0 * u0) + e2 * u2;
    let mut t = (u1 * dy + u0 * dx) + u2 * dz;
    if !(length > t) {
        t = length;
    }
    if 0.0 > t {
        t = 0.0;
    }
    let y = dy - u1 * t;
    let x = dx - u0 * t;
    let z = dz - u2 * t;
    let st = &mut c.st;
    let mut word = |value: f32| {
        let mut buffer = [0u8; 10];
        buffer[..4].copy_from_slice(&value.to_le_bytes());
        buffer
    };
    st.exec::<0x00d9>(&mut word(y)); // FLD
    st.register::<0xc8d8>(); // FMUL ST0
    st.exec::<0x00d9>(&mut word(x));
    st.register::<0xc8d8>();
    st.register::<0xc1de>(); // FADDP
    st.exec::<0x00d9>(&mut word(z));
    st.register::<0xc8d8>();
    st.register::<0xc1de>();
    st.exec::<0x00d9>(&mut word(COINCIDENT));
    let flags = st.compare_flags::<0xf1df>(); // FCOMIP ST0, ST1
    st.register::<0xd8dd>(); // FSTP ST0
    flags.cond("be")
}

/// Try to add the Minkowski-difference point `p` (with the witnesses `a`
/// on the capsule and `b` on the triangle) to the simplex. Returns the
/// original's EAX: 0 when the point is not new enough (no progress), else
/// the pointer to the last stored slot with AL = 1.
///
/// "New enough": with one point, some coordinate differs by more than
/// 0.001; with two, the point is farther than 0.001 (squared) from their
/// segment; with three, farther than 0.001 from their plane.
fn add_point(c: &mut Cpu, pg: Pages, m: &mut dyn Mem, s: u32, p: Vector, a_at: u32, b_at: u32) -> u32 {
    copy_vector(pg, m, s.wrapping_add(simplex::NEWEST), p);
    let n = pg.u32(m, s);
    pg.set_u8(m, s.wrapping_add(simplex::DIRTY), 1);
    let point = |m: &dyn Mem, k: u32| vector(pg, m, s.wrapping_add(simplex::POINTS + 0x10 * k));
    match n {
        1 => {
            let v0 = point(m, 0);
            // a coordinate farther away than 0.001 (or NaN) makes it new
            let far = |k: usize| COINCIDENT < (p[k] - v0[k]).abs() || (p[k] - v0[k]).is_nan();
            if !far(0) && !far(1) && !far(2) {
                return 0;
            }
        }
        2 => {
            let (v0, v1) = (point(m, 0), point(m, 1));
            if !point_is_off_segment(c, v0, v1, p) {
                return 0;
            }
        }
        3 => {
            let (v0, v1, v2) = (point(m, 0), point(m, 1), point(m, 2));
            let ab = sub(v1, v0);
            let ac = sub(v2, v0);
            let mut nx = ac[2] * ab[1] - ac[1] * ab[2];
            let mut ny = ab[2] * ac[0] - ac[2] * ab[0];
            let mut nz = ac[1] * ab[0] - ab[1] * ac[0];
            let length = ((ny * ny + nx * nx) + nz * nz).sqrt();
            if !(EPSILON > length) {
                let inverse = ONE / length;
                nx *= inverse;
                ny *= inverse;
                nz *= inverse;
            }
            let away = (p[0] * nx + p[1] * ny) + p[2] * nz;
            let plane = (ny * v0[1] + nx * v0[0]) + nz * v0[2];
            let distance = (away + -plane).abs();
            if COINCIDENT > distance {
                return 0;
            }
        }
        _ => {}
    }
    let slot = |k: u32| s.wrapping_add((n.wrapping_add(k)) << 4);
    copy_vector(pg, m, slot(1), p);
    copy_point(pg, m, slot(7), a_at);
    copy_point(pg, m, slot(13), b_at);
    pg.set_u32(m, s, n.wrapping_add(1));
    (slot(13) & !0xff) | 1
}

/// 0x00D1B540, `simplex->add_point(point, capsule_witness,
/// triangle_witness) -> added` (thiscall, `ret 0xc`): see [`add_point`].
///
/// Translated from 0x00d1b540 (DDDA.exe 2017-12-13 Steam).
pub fn simplex_add_point(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (p, a, b) = (arg(m, 0), arg(m, 1), arg(m, 2));
    let p = vector(pg, m, p);
    c.eax = add_point(c, pg, m, c.ecx, p, a, b);
    c.esp = entry.wrapping_add(4 + 0xc);
}

// ---- 0x00D12E10: closest point of a segment simplex --------------------------

/// The closest point to the origin of the two-point simplex: vertex 0,
/// vertex 1 (which then replaces vertex 0 and the simplex shrinks to one
/// point), or the interior point (witnesses blended by the same
/// parameter). Written to `out` (w = 0). Returns the original's EAX (`out`).
fn segment_closest(pg: Pages, m: &mut dyn Mem, s: u32, out: u32) -> u32 {
    let a = vector(pg, m, s.wrapping_add(0x10));
    let b = vector(pg, m, s.wrapping_add(0x20));
    let na = [-a[0], -a[1], -a[2]];
    let e1 = b[1] - a[1];
    let e2 = b[2] - a[2];
    let e0 = b[0] - a[0];
    let length_squared = (e1 * e1 + e0 * e0) + e2 * e2;
    let along = (e1 * na[1] + e0 * na[0]) + e2 * na[2];
    if 0.0 > along {
        pg.set_u32(m, s, 1);
        copy_point(pg, m, out, s.wrapping_add(0x10));
        return out;
    }
    if along > length_squared {
        pg.set_u32(m, s, 1);
        copy_point(pg, m, out, s.wrapping_add(0x20));
        for base in [0x10u32, 0x70, 0xd0] {
            copy_point(pg, m, s.wrapping_add(base), s.wrapping_add(base + 0x10));
        }
        return out;
    }
    let t = along / length_squared;
    let blend = |m: &dyn Mem, base: u32| {
        let lo = vector(pg, m, s.wrapping_add(base));
        let hi = vector(pg, m, s.wrapping_add(base + 0x10));
        [lo[0] + (hi[0] - lo[0]) * t, lo[1] + (hi[1] - lo[1]) * t, lo[2] + (hi[2] - lo[2]) * t]
    };
    let on_capsule = blend(m, 0x70);
    let on_triangle = blend(m, 0xd0);
    store(pg, m, out, [on_capsule[0] - on_triangle[0], on_capsule[1] - on_triangle[1], on_capsule[2] - on_triangle[2]]);
    pg.set_u32(m, out.wrapping_add(12), 0);
    out
}

/// 0x00D12E10, `simplex->closest_on_segment(out)` (thiscall, `ret 4`).
///
/// Translated from 0x00d12e10 (DDDA.exe 2017-12-13 Steam).
pub fn simplex_segment_closest(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let out = pg.u32(m, entry.wrapping_add(4));
    c.eax = segment_closest(pg, m, c.ecx, out);
    c.esp = entry.wrapping_add(8);
}

// ---- 0x00D7CF70: the sweep ---------------------------------------------------

/// Reset the search state: no points, valid and dirty flags 0/1, no closest
/// point yet (FLT_MAX) in `+0x150` and the default displacement in `+0x160`.
fn reset(pg: Pages, m: &mut dyn Mem, s: u32) {
    pg.set_u32(m, s, 0);
    pg.set_u16(m, s.wrapping_add(simplex::VALID), 0x100);
    for i in 0..3 {
        let v = pg.u32(m, ZERO_VECTOR + i * 4);
        pg.set_u32(m, s.wrapping_add(simplex::CLOSEST + i * 4), v);
    }
    pg.set_u32(m, s.wrapping_add(simplex::CLOSEST + 12), 0);
    for i in 0..3 {
        let v = pg.u32(m, FAR_POINT + i * 4);
        pg.set_u32(m, s.wrapping_add(simplex::NEWEST + i * 4), v);
    }
    pg.set_u32(m, s.wrapping_add(simplex::NEWEST + 12), 0);
}

/// The difference of the two support points plus `direction * margin`:
/// the point of the Minkowski difference (inflated by the contact margin)
/// the search works with.
fn difference_point(pg: Pages, m: &dyn Mem, shape: u32, direction: Vector) -> Vector {
    let a = vector(pg, m, shape.wrapping_add(0x10));
    let b = vector(pg, m, shape.wrapping_add(0x20));
    [direction[0] * CONTACT_MARGIN + (a[0] - b[0]), direction[1] * CONTACT_MARGIN + (a[1] - b[1]), direction[2] * CONTACT_MARGIN + (a[2] - b[2])]
}

/// 0x00D7CF70, `owner->sweep(shape, displacement, simplex, hit_a, hit_b)
/// -> hit` (thiscall, `ret 0x14`): sweep the shape's capsule along
/// `displacement` against its triangle.
///
/// `owner` holds the iteration limit (+0x190) and the squared-distance
/// tolerance (+0x194). The search state is the `simplex` record: point
/// count at +0, up to four points of the Minkowski difference from +0x10
/// with their capsule and triangle witnesses at +0x70 and +0xd0, the
/// closest point at +0x150, the valid/dirty bytes at +0x170/+0x171.
/// Returns false when the capsule never reaches the triangle within the
/// displacement (or the search cannot proceed); on a hit it fills the
/// optional `hit_a`/`hit_b` records (contact point, normal, travelled
/// distance, a hit flag; `hit_b` with the normal reversed) and returns
/// true. Only the tetrahedron step 0x00D8E400 stays a guest call.
///
/// Returns AL; the upper bytes of EAX are the original's leftovers
/// (reproduced).
///
/// Translated from 0x00d7cf70 (DDDA.exe 2017-12-13 Steam).
pub fn sweep_capsule_triangle(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let owner = c.ecx;
    let saved = [c.ebx, c.ebp, c.esi, c.edi];
    let shape = pg.u32(m, entry.wrapping_add(4));
    let input = pg.u32(m, entry.wrapping_add(8));
    let simplex = pg.u32(m, entry.wrapping_add(12));
    // the original's ESP at its call: after the prologue and three pushes
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x140);
    let hit = sweep(m, c, pg, frame, owner, shape, input, simplex, entry);
    c.eax = (c.eax & 0xffff_ff00) | u32::from(hit);
    [c.ebx, c.ebp, c.esi, c.edi] = saved;
    c.esp = entry.wrapping_add(24);
}

#[allow(clippy::too_many_arguments)]
fn sweep(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, owner: u32, shape: u32, input: u32, s: u32, entry: u32) -> bool {
    let mut velocity = vector(pg, m, input);
    reset(pg, m, s);
    let magnitude = dot(velocity, velocity);
    let tolerance = rf(pg, m, owner.wrapping_add(0x194));
    let mut direction = if !(tolerance > magnitude) {
        scale(velocity, ONE / magnitude.sqrt())
    } else {
        velocity = vector(pg, m, DEFAULT_DISPLACEMENT);
        velocity
    };
    let mut normal = vector(pg, m, ZERO_VECTOR);
    let mut point = normal;
    let mut distance = 0f32;
    c.eax = support(pg, m, shape, direction);
    direction = sub(normal, difference_point(pg, m, shape, direction));
    let initial_squared = dot(direction, direction);
    let unit = scale(direction, ONE / initial_squared.sqrt());
    c.eax = support(pg, m, shape, unit);
    let mut support_point = difference_point(pg, m, shape, unit);
    let mut delta = sub(normal, support_point);
    let mut separation = dot_yxz(delta, direction);
    let mut approach = dot(direction, velocity);
    normal = vector(pg, m, ZERO_VECTOR);
    if initial_squared > rf(pg, m, owner.wrapping_add(0x194)) && pg.u32(m, owner.wrapping_add(0x190)) != 0 {
        let mut iteration = 0u32;
        c.eax = owner;
        loop {
            if separation > 0.0 {
                if approach >= 0.0 {
                    return false;
                }
                reset(pg, m, s);
                distance -= separation / approach;
                point = [velocity[0] * distance, velocity[1] * distance, velocity[2] * distance];
                delta = sub(point, support_point);
                if distance > ONE {
                    return false;
                }
                normal = direction;
            }
            c.eax = add_point(c, pg, m, s, delta, shape.wrapping_add(0x10), shape.wrapping_add(0x20));
            if c.eax as u8 == 0 {
                break;
            }
            if pg.u8(m, s.wrapping_add(simplex::DIRTY)) == 0 {
                direction = vector(pg, m, s.wrapping_add(simplex::CLOSEST));
                if pg.u8(m, s.wrapping_add(simplex::VALID)) == 0 {
                    return false;
                }
            } else {
                pg.set_u8(m, s.wrapping_add(simplex::DIRTY), 0);
                c.eax = pg.u32(m, s);
                match c.eax {
                    0 => {
                        pg.set_u8(m, s.wrapping_add(simplex::VALID), 0);
                        return false;
                    }
                    1 => copy_point(pg, m, s.wrapping_add(simplex::CLOSEST), s.wrapping_add(simplex::POINTS)),
                    2 => c.eax = segment_closest(pg, m, s, s.wrapping_add(simplex::CLOSEST)),
                    3 => reduce_triangle(pg, m, s),
                    4 => {
                        let ok = call_from(
                            m,
                            c,
                            pg,
                            frame,
                            s,
                            &[s.wrapping_add(0x150), s.wrapping_add(16), s.wrapping_add(32), s.wrapping_add(48), s.wrapping_add(64)],
                            0x00d7_df2d,
                            |m, c| x86::call_ext(m, c, 0x00d8_e400),
                        );
                        if ok as u8 == 0 {
                            for i in 0..3 {
                                let v = pg.u32(m, ZERO_VECTOR + i * 4);
                                pg.set_u32(m, s.wrapping_add(simplex::CLOSEST + i * 4), v);
                            }
                            pg.set_u32(m, s.wrapping_add(simplex::CLOSEST + 12), 0);
                        }
                    }
                    _ => return false,
                }
                direction = vector(pg, m, s.wrapping_add(simplex::CLOSEST));
            }
            let squared = dot(direction, direction);
            let length = squared.sqrt();
            let unit = if !(EPSILON > length) { scale(direction, ONE / length) } else { direction };
            if rf(pg, m, owner.wrapping_add(0x194)) >= squared {
                break;
            }
            c.eax = support(pg, m, shape, unit);
            support_point = difference_point(pg, m, shape, unit);
            delta = sub(point, support_point);
            approach = dot(direction, velocity);
            separation = dot_yxz(delta, direction);
            iteration = iteration.wrapping_add(1);
            c.eax = iteration;
            if iteration >= pg.u32(m, owner.wrapping_add(0x190)) {
                break;
            }
        }
    }
    let length = dot_yxz(normal, normal).sqrt();
    if !(EPSILON > length) {
        normal = scale(normal, ONE / length);
    }
    for (argument, negative) in [(16u32, false), (20, true)] {
        let out = pg.u32(m, entry.wrapping_add(argument));
        c.eax = out;
        if out == 0 {
            continue;
        }
        let sign = if negative { pg.u32(m, 0x0161_c280) } else { 0 };
        for i in 0..3 {
            let v = pg.u32(m, ZERO_VECTOR + i * 4);
            pg.set_u32(m, out.wrapping_add(i * 4), v);
        }
        pg.set_u32(m, out.wrapping_add(12), 0);
        for (i, v) in normal.into_iter().enumerate() {
            pg.set_u32(m, out.wrapping_add(16 + i as u32 * 4), v.to_bits() ^ sign);
        }
        pg.set_u32(m, out.wrapping_add(28), 0);
        pg.set_u32(m, out.wrapping_add(32), 0);
        pg.set_u32(m, out.wrapping_add(36), distance.to_bits());
        pg.set_u8(m, out.wrapping_add(40), 1);
    }
    true
}
fn move_vertex(pg: Pages, m: &mut dyn Mem, s: u32, to: u32, from: u32) {
    for base in [0x10, 0x70, 0xd0] {
        copy_point(pg, m, s.wrapping_add(base + to * 16), s.wrapping_add(base + from * 16));
    }
}
fn closest(pg: Pages, m: &mut dyn Mem, s: u32, v: Vector) {
    store(pg, m, s.wrapping_add(0x150), v);
    pg.set_u32(m, s.wrapping_add(0x15c), 0);
}
fn vertex_region(pg: Pages, m: &mut dyn Mem, s: u32, index: u32) {
    let third_x = if index == 2 {
        Some(pg.u32(m, s.wrapping_add(48)))
    } else {
        None
    };
    pg.set_u32(m, s, 1);
    if index != 0 {
        move_vertex(pg, m, s, 0, index);
    }
    // The third-vertex arm writes X with MOVSS; its other copies use x87.
    if let Some(bits) = third_x {
        pg.set_u32(m, s.wrapping_add(16), bits);
    }
    copy_point(pg, m, s.wrapping_add(0x150), s.wrapping_add(16 + index * 16));
}
fn inverse_or_zero(value: f32) -> f32 {
    if value == 0.0 {
        0.0
    } else {
        ONE / value
    }
}

/// Triangle Voronoi regions in the executable's branch and arithmetic order.
pub fn reduce_triangle(pg: Pages, m: &mut dyn Mem, s: u32) {
    let a = vector(pg, m, s.wrapping_add(16));
    let b = vector(pg, m, s.wrapping_add(32));
    let d = vector(pg, m, s.wrapping_add(48));
    let origin = vector(pg, m, ZERO_VECTOR);
    let ab = sub(b, a);
    let ac = sub(d, a);
    let ao = sub(origin, a);
    let ab_a = dot_yxz(ao, ab);
    let ac_a = ao[2] * ac[2] + (ao[1] * ac[1] + ao[0] * ac[0]);
    if ab_a <= 0.0 && ac_a <= 0.0 {
        vertex_region(pg, m, s, 0);
        return;
    }
    let bo = sub(origin, b);
    let ab_b = dot_yxz(bo, ab);
    let ac_b = dot_yxz(bo, ac);
    if ab_b >= 0.0 && ab_b >= ac_b {
        vertex_region(pg, m, s, 1);
        return;
    }
    let area_ab = ac_b * ab_a - ab_b * ac_a;
    if area_ab <= 0.0 && ab_a >= 0.0 && ab_b <= 0.0 {
        let weight = inverse_or_zero(ab_a - ab_b) * ab_a;
        if weight < 0.0 {
            vertex_region(pg, m, s, 0);
            return;
        }
        pg.set_u32(m, s, 2);
        let first = vector(pg, m, s.wrapping_add(16));
        closest(pg, m, s, std::array::from_fn(|i| ab[i] * weight + first[i]));
        return;
    }
    let co = sub(origin, d);
    let ab_c = dot(co, ab);
    let ac_c = dot(co, ac);
    if ac_c >= 0.0 && ac_c >= ab_c {
        vertex_region(pg, m, s, 2);
        return;
    }
    let area_ac = ab_c * ac_a - ac_c * ab_a;
    if area_ac <= 0.0 && ac_a >= 0.0 && ac_c <= 0.0 {
        let weight = inverse_or_zero(ac_a - ac_c) * ac_a;
        if weight < 0.0 {
            vertex_region(pg, m, s, 0);
            return;
        }
        pg.set_u32(m, s, 2);
        move_vertex(pg, m, s, 1, 2);
        let first = vector(pg, m, s.wrapping_add(16));
        closest(pg, m, s, std::array::from_fn(|i| ac[i] * weight + first[i]));
        return;
    }
    let area_bc = ac_c * ab_b - ab_c * ac_b;
    let span = ac_b - ab_b;
    if !(area_bc <= 0.0 && span >= 0.0) {
        let inverse = ONE / ((area_bc + area_ac) + area_ab);
        let wb = inverse * area_ac;
        let wc = inverse * area_ab;
        let first = vector(pg, m, s.wrapping_add(16));
        closest(
            pg,
            m,
            s,
            [
                (ab[0] * wb + a[0]) + ac[0] * wc,
                (ab[1] * wb + first[1]) + ac[1] * wc,
                (ab[2] * wb + first[2]) + ac[2] * wc,
            ],
        );
    } else {
        // Preserve the original reciprocal-times-span expression, including
        // its zero/NaN behavior; do not simplify it to one.
        let weight = inverse_or_zero(span) * span;
        if weight < 0.0 {
            vertex_region(pg, m, s, 1);
            return;
        }
        pg.set_u32(m, s, 2);
        move_vertex(pg, m, s, 0, 1);
        move_vertex(pg, m, s, 1, 2);
        let second = vector(pg, m, s.wrapping_add(32));
        let third = vector(pg, m, s.wrapping_add(48));
        closest(
            pg,
            m,
            s,
            std::array::from_fn(|i| (third[i] - second[i]) * weight + second[i]),
        );
    }
}
