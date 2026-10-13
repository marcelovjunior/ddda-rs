//! L184 s5: native port of `uCnsEdit::vf24` (0x00860AC0), the joint
//! "constraint edit" of a model's per-unit joint update.
//!
//! A constraint edit (object `this`, a `uCnsEdit`) is applied to one joint
//! record (`joint`, matrix at +0x10) of a model. It builds a 4x4 local
//! matrix in one of 17 ways selected by the dword at `this + 0xA4`:
//!
//! | mode | local matrix |
//! |------|--------------|
//! | 0..=5 | rotation from the Euler angles (degrees at `this + 0x70..0x78`) in the order XYZ, XZY, YXZ, YZX, ZXY, ZYX (row-vector convention, `Rx * Ry * Rz` for XYZ) |
//! | 6..=15 | a matrix returned by one of ten guest "matrix provider" functions |
//! | 16 | not a matrix: the guest function 0x0086B2E0 does the whole edit |
//! | >16 | whatever the original's stack slots hold (never used by the game) |
//!
//! Modes 0..=6, 9..=11 and 15 then add the translation at `this + 0x90`
//! and scale the three rows by the vector at `this + 0x80` (when its squared
//! length exceeds 1e-6). The matrix is blended toward identity by the
//! weight at `this + 0x44` (guest 0x00CFDC80, skipped for weight 1.0) and
//! multiplied into the joint matrix: `joint = local * joint`, or, when the
//! dword at `this + 0xA0` is set, in the frame of a parent bone
//! `joint = joint * inverse(parent) * local * parent` (the parent is the
//! joint of the model selected by the byte at `joint + 0x91`, or the matrix
//! of the model's virtual `+0x58(-1)` accessor for 0xFF).
//!
//! The CRT sinf/cosf (0x012F0906, 0x012F0A55) are reproduced natively, bit
//! for bit; only their x87 range-reduction range (|x| >= 8192 rad) stays a
//! guest call. The providers, the blend and the parent accessor stay guest
//! calls, made with the original's registers and frame layout.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

/// Degrees to radians, 0x0161C22C.
const DEGREES_TO_RADIANS: f32 = f32::from_bits(0x3c8e_fa35);
/// Squared scale length above which the scale is applied, 0x0161CF38.
const SCALE_EPSILON: f32 = f32::from_bits(0x3586_37be);
const IDENTITY: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Guest functions the edit calls.
const BLEND_TOWARD_IDENTITY: u32 = 0x00cf_dc80;
const SINF: u32 = 0x012f_0906;
const COSF: u32 = 0x012f_0a55;

/// A 4x4 float matrix, 16 floats in memory order (rows of 4).
pub type Matrix = [f32; 16];

fn read_f32(m: &dyn Mem, pg: Pages, address: u32) -> f32 {
    f32::from_bits(pg.u32(m, address))
}

fn read_matrix(m: &dyn Mem, pg: Pages, address: u32) -> Matrix {
    std::array::from_fn(|i| f32::from_bits(pg.u32(m, address.wrapping_add(4 * i as u32))))
}

fn write_matrix(m: &mut dyn Mem, pg: Pages, address: u32, matrix: &Matrix) {
    for (i, v) in matrix.iter().enumerate() {
        pg.set_u32(m, address.wrapping_add(4 * i as u32), v.to_bits());
    }
}

/// `a * b` for row-vector matrices, summing each element as the original
/// does: `((a0 b0 + a1 b1) + a2 b2) + a3 b3`.
pub fn matrix_multiply(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|i| {
        let (row, col) = (i & !3, i & 3);
        ((a[row] * b[col] + a[row + 1] * b[4 + col]) + a[row + 2] * b[8 + col]) + a[row + 3] * b[12 + col]
    })
}

/// Register inputs of a guest call (the matrix providers take theirs in
/// EAX/ECX/ESI/EDI, a convention of their own).
#[derive(Clone, Copy)]
struct Registers {
    eax: u32,
    ecx: u32,
    esi: u32,
    edi: u32,
}

/// A call from the original's frame to the guest function `target`:
/// `frame` is the original's ESP after its prologue, `args` are pushed as
/// it does (last first), then `back`, the return address of the call site in
/// 0x00860AC0. Returns EAX; ESP is `frame` again afterwards.
fn guest_call(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, target: u32, back: u32, regs: Registers, args: &[u32]) -> u32 {
    c.esp = frame;
    for &v in args.iter().rev() {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, back);
    (c.eax, c.ecx, c.esi, c.edi) = (regs.eax, regs.ecx, regs.esi, regs.edi);
    call_ext(m, c, target);
    c.eax
}

// ---------------------------------------------------------------------
// CRT sinf / cosf (0x012F0906 / 0x012F0A55)

/// The CRT's single-precision sine (`cosine == false`) or cosine of `x`.
///
/// Exact reproduction of the SSE2 fast path: the angle is reduced by
/// multiples of pi/32 (table index from the rounded `x * 32/pi`), the
/// remainder goes through a short double-precision polynomial and is
/// combined with the table's sin/cos pair (read from the guest, 16 bytes
/// at `table + 16 * index`). Tiny arguments (denormal or zero) and
/// infinities/NaNs are answered inline as the original does; |x| >= 8192
/// radians falls back to the guest's x87 range reduction (`call`).
pub fn crt_sincos(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, x: f32, cosine: bool, back: u32) -> f32 {
    let bits = x.to_bits();
    // exponent and top mantissa bits, relative to the smallest normal range
    let class = ((bits >> 16) as u16 & 0x7fff).wrapping_sub(0x80);
    if class > 0x457f {
        if class >= 0x8000 {
            // denormal or zero
            let xd = x as f64;
            return if cosine {
                (1.0 - xd * xd) as f32
            } else {
                (xd * f64::from_bits(0x3fef_ffff_ff80_0000)) as f32
            };
        }
        if bits & 0x7f80_0000 == 0x7f80_0000 {
            return x - x; // infinity or NaN
        }
        // |x| >= 8192: the original's x87 reduction, run by the guest
        c.xmm[0] = [bits, 0, 0, 0];
        let regs = Registers { eax: c.eax, ecx: c.ecx, esi: c.esi, edi: c.edi };
        guest_call(m, c, pg, frame, if cosine { COSF } else { SINF }, back, regs, &[]);
        return f32::from_bits(c.xmm[0][0]);
    }
    let scaled = f32::from_bits(0x4122_f983) * x;
    let steps = scaled.round_ties_even() as i32;
    // scaled rounded to an integer by the add/subtract-magic trick
    let magic = f32::from_bits(0x4b40_0000);
    let rounded = (scaled + magic) - magic;
    // sign of x, as the bits that adjust the two reduction constants
    let sign = (bits & 0x8000_0000) as u64;
    let adjust = (sign << 32) ^ (sign << 29);
    let pi_high = f64::from_bits(0x77b9_21fb_5444_4000 ^ adjust);
    let pi_low = f64::from_bits(0x7532_e7b9_6767_33af ^ adjust);
    let step = f64::from_bits((rounded.to_bits() as u64) << 29);
    let index = steps.wrapping_add(if cosine { 0x1c_7610 } else { 0x1c_7600 }) as u32 & 0x3f;
    let table = (if cosine { 0x0144_b870u32 } else { 0x0144_b410 }).wrapping_add(index << 4);
    let (table_sin, table_cos) = (
        f64::from_bits(pg.u32(m, table) as u64 | (pg.u32(m, table + 4) as u64) << 32),
        f64::from_bits(pg.u32(m, table + 8) as u64 | (pg.u32(m, table + 12) as u64) << 32),
    );
    let reduced = x as f64 - pi_high * step;
    let corrected = pi_low * step + reduced;
    let square = reduced * reduced;
    let fourth = square * square;
    let cos_poly = (-0.5 * square + 1.0) + f64::from_bits(0x3fa5_5555_5555_5555) * fourth;
    let sin_poly = (f64::from_bits(0xbfc5_5555_5555_5555) * square + 1.0) + f64::from_bits(0x3f81_1111_1111_1111) * fourth;
    (table_sin * cos_poly + (corrected * table_cos) * sin_poly) as f32
}

// ---------------------------------------------------------------------
// local matrix builders

/// Rotation matrix of Euler `order` (0..=5: XYZ, XZY, YXZ, YZX, ZXY, ZYX)
/// from the sines and cosines of the three axis angles, with the original's
/// products and sums in their exact order. Only the 3x3 block is set; the
/// translation column/row are zero and `m[15]` is 1.
pub fn euler_matrix(order: u32, s: [f32; 3], c: [f32; 3]) -> Matrix {
    let [sx, sy, sz] = s;
    let [cx, cy, cz] = c;
    let mut m = [0.0f32; 16];
    match order {
        // XYZ
        0 => {
            m[0] = cz * cy;
            m[1] = cy * sz;
            m[4] = cz * sy * sx - sz * cx;
            m[8] = sz * sx + cz * sy * cx;
            m[5] = cz * cx + sz * sy * sx;
            m[6] = cy * sx;
            m[9] = sz * sy * cx - cz * sx;
            m[10] = cy * cx;
            m[2] = -sy;
        }
        // XZY
        1 => {
            m[1] = sz;
            m[2] = -(cz * sy);
            m[6] = sy * cx * sz + cy * sx;
            m[0] = cz * cy;
            m[8] = sz * sx * cy + sy * cx;
            m[4] = sy * sx - sz * cx * cy;
            m[5] = cz * cx;
            m[9] = -(cz * sx);
            m[10] = cy * cx - sy * sx * sz;
        }
        // YXZ
        2 => {
            m[1] = sy * sx * cz + cy * sz;
            m[0] = cz * cy - sy * sx * sz;
            m[4] = -(sz * cx);
            m[5] = cz * cx;
            m[8] = cy * sx * sz + cz * sy;
            m[9] = sz * sy - cy * sx * cz;
            m[10] = cy * cx;
            m[6] = sx;
            m[2] = -(sy * cx);
        }
        // YZX
        3 => {
            m[1] = cy * cx * sz + sy * sx;
            m[10] = sy * sx * sz + cy * cx;
            m[0] = cz * cy;
            m[4] = -sz;
            m[2] = cy * sz * sx - sy * cx;
            m[5] = cz * cx;
            m[8] = cz * sy;
            m[6] = cz * sx;
            m[9] = sy * cx * sz - cy * sx;
        }
        // ZXY
        4 => {
            m[0] = sy * sx * sz + cz * cy;
            m[1] = sz * cx;
            m[2] = sz * sx * cy - cz * sy;
            m[4] = cz * sx * sy - cy * sz;
            m[5] = cz * cx;
            m[9] = -sx;
            m[8] = sy * cx;
            m[10] = cy * cx;
            m[6] = cy * sx * cz + sz * sy;
        }
        // ZYX
        _ => {
            m[1] = cz * sy * sx + cx * sz;
            m[2] = sx * sz - cx * cz * sy;
            m[5] = cx * cz - sx * sy * sz;
            m[9] = -(sx * cy);
            m[6] = cx * sy * sz + sx * cz;
            m[8] = sy;
            m[0] = cz * cy;
            m[4] = -(cy * sz);
            m[10] = cx * cy;
        }
    }
    m[15] = 1.0;
    m
}

/// The inverse of a general 4x4 matrix by cofactors, in the exact operation
/// order 0x00860AC0 uses for the parent bone (decoded from its SSE code:
/// the products are grouped as the compiler grouped them, so the float
/// result is bit-identical). The first column of cofactors gives the
/// determinant; a determinant that is exactly 0 yields the identity.
pub fn invert_parent_matrix(p: &Matrix) -> Matrix {
    let [p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15] = *p;
    let m1 = p6 * p11;
    let m2 = p5 * p10;
    let m3 = p7 * p9;
    let m4 = p7 * p10;
    let m5 = p14 * p11;
    let m6 = p15 * p6;
    let m7 = (((((p13 * m1) + (p15 * m2)) + (p14 * m3)) - (p13 * m4)) - (p5 * m5)) - (p9 * m6);
    let m8 = p2 * p11;
    let m9 = p1 * p10;
    let m10 = p3 * p9;
    let m11 = p3 * p10;
    let m12 = p15 * p2;
    let m13 = -(((((((p13 * m8) + (p15 * m9)) + (p14 * m10)) - (p13 * m11)) - (p1 * m5)) - (p9 * m12)));
    let m14 = p2 * p7;
    let m15 = p6 * p1;
    let m16 = p3 * p5;
    let m17 = p6 * p3;
    let m18 = p14 * p7;
    let m19 = (((((p13 * m14) + (p15 * m15)) + (p14 * m16)) - (p13 * m17)) - (p1 * m18)) - (p5 * m12);
    let m20 = -(((((((p9 * m14) + (p11 * m15)) + (p10 * m16)) - (p9 * m17)) - (p1 * m4)) - (p5 * m8)));
    let det = (((p4 * m13) + (p0 * m7)) + (p8 * m19)) + (p12 * m20);
    if det == 0.0 {
        return IDENTITY;
    }
    let m21 = 1.0_f32 / det;
    let q0 = m7 * m21;
    let q1 = m13 * m21;
    let q2 = m19 * m21;
    let q3 = m20 * m21;
    let m22 = p7 * p8;
    let q4 = -((m21 * ((((((p15 * (p10 * p4)) + (m1 * p12)) + (p14 * m22)) - (m4 * p12)) - (m5 * p4)) - (m6 * p8))));
    let m23 = p3 * p8;
    let q5 = m21 * ((((((m8 * p12) + (p15 * (p0 * p10))) + (p14 * m23)) - (p12 * m11)) - (p0 * m5)) - (p8 * m12));
    let m24 = p0 * p6;
    let m25 = p3 * p4;
    let q6 = -((m21 * ((((((p12 * m14) + (p15 * m24)) + (p14 * m25)) - (p12 * m17)) - (p0 * m18)) - (p4 * m12))));
    let q7 = m21 * ((((((p8 * m14) + (p11 * m24)) + (p10 * m25)) - (p8 * m17)) - (p0 * m4)) - (m8 * p4));
    let m26 = p9 * p4;
    let m27 = p13 * p11;
    let q8 = m21 * ((((((p12 * (p5 * p11)) + (p15 * m26)) + (p13 * m22)) - (m3 * p12)) - (p4 * m27)) - (p8 * (p15 * p5)));
    let m28 = p1 * p11;
    let m29 = p0 * p9;
    let m30 = p15 * p1;
    let q9 = -((m21 * ((((((p15 * m29) + (p12 * m28)) + (p13 * m23)) - (p12 * m10)) - (p0 * m27)) - (p8 * m30))));
    let m31 = p1 * p7;
    let m32 = p0 * p5;
    let q10 = m21 * ((((((p15 * m32) + (p12 * m31)) + (p13 * m25)) - (p12 * m16)) - (p0 * (p13 * p7))) - (p4 * m30));
    let q11 = -((m21 * ((((((p11 * m32) + (p8 * m31)) + (p9 * m25)) - (p8 * m16)) - (p0 * m3)) - (p4 * m28))));
    let m33 = p6 * p9;
    let m34 = p13 * p10;
    let q12 = -((m21 * ((((((m2 * p12) + (p14 * m26)) + (p13 * (p6 * p8))) - (p12 * m33)) - (p4 * m34)) - (p8 * (p14 * p5)))));
    let m35 = p14 * p1;
    let q13 = m21 * ((((((p12 * m9) + (p14 * m29)) + (p13 * (p2 * p8))) - (p12 * (p2 * p9))) - (p0 * m34)) - (p8 * m35));
    let m36 = p2 * p4;
    let m37 = p2 * p5;
    let q14 = -((m21 * ((((((p14 * m32) + (p12 * m15)) + (p13 * m36)) - (p12 * m37)) - (p0 * (p13 * p6))) - (p4 * m35))));
    let q15 = m21 * ((((((p10 * m32) + (p8 * m15)) + (p9 * m36)) - (p8 * m37)) - (p0 * m33)) - (p4 * m9));
    [q0, q1, q2, q3, q4, q5, q6, q7, q8, q9, q10, q11, q12, q13, q14, q15]
}

/// Add the constraint's translation (`this + 0x90`) to the matrix and
/// scale its three rows by the vector at `this + 0x80` when that vector's
/// squared length exceeds [`SCALE_EPSILON`].
fn apply_offset_and_scale(m: &dyn Mem, pg: Pages, this: u32, local: &mut Matrix) {
    local[14] += read_f32(m, pg, this + 0x98);
    local[12] = read_f32(m, pg, this + 0x90) + local[12];
    local[13] += read_f32(m, pg, this + 0x94);
    let (x, y, z) = (read_f32(m, pg, this + 0x80), read_f32(m, pg, this + 0x84), read_f32(m, pg, this + 0x88));
    if x * x + y * y + z * z > SCALE_EPSILON {
        for e in &mut local[0..4] {
            *e *= x;
        }
        for e in &mut local[4..8] {
            *e *= y;
        }
        for e in &mut local[8..12] {
            *e *= z;
        }
    }
}

/// How a mode obtains its local matrix from a guest function: where it is
/// called from, the registers and stack arguments it takes (offsets are
/// relative to the original's frame, `F`).
struct Provider {
    target: u32,
    /// Return address of the call site in 0x00860AC0.
    back: u32,
    regs: Registers,
    args: [u32; 3],
    nargs: usize,
    /// Whether the angle vector at `F + 0x120` (the three angles in
    /// radians and a zero) must be on the stack: the provider is passed its
    /// address.
    angles: bool,
    /// Whether the constraint's translation and scale are applied after.
    offset_and_scale: bool,
}

/// Offset of the angle vector in the original's frame.
const F_ANGLES: u32 = 0x120;

/// The guest function of `mode` 6..=16 (`None` above). `this`, `joint` and
/// `model` are the constraint, joint record and model; `frame` is `F`.
fn provider_for(mode: u32, this: u32, joint: u32, model: u32, frame: u32) -> Option<Provider> {
    let f = |offset: u32| frame.wrapping_add(offset);
    let angles = f(F_ANGLES);
    // mode: (target, call site return, [eax, ecx, esi, edi], args, angles, offset and scale)
    let (target, back, r, args, nargs, takes_angles, offset_and_scale) = match mode {
        6 => (0x0086_4660, 0x0086_1498, [this, model, f(0xe0), joint], [angles, 0, 0], 1, true, true),
        7 => (0x0086_5990, 0x0086_156b, [7, angles, f(0x380), joint], [this, model, angles], 3, true, false),
        8 => (0x0086_7420, 0x0086_163c, [angles, this, f(0x200), this], [this, joint, model], 3, true, false),
        9 => (0x0086_7b70, 0x0086_1709, [9, model, f(0x300), angles], [this, joint, model], 3, true, true),
        10 => (0x0086_7b70, 0x0086_1709, [10, this, f(0x280), angles], [this, joint, model], 3, true, true),
        11 => (0x0086_d180, 0x0086_17ec, [angles, model, f(0x180), joint], [this, model, angles], 3, true, true),
        12 => (0x0086_8000, 0x0086_1803, [12, model, joint, f(0x1c0)], [this, 0, 0], 1, false, false),
        13 => (0x0086_9260, 0x0086_18d6, [model, this, joint, f(0x240)], [this, model, angles], 3, true, false),
        14 => (0x0086_a100, 0x0086_19a9, [14, angles, joint, f(0x2c0)], [this, model, angles], 3, true, false),
        15 => (0x0086_aff0, 0x0086_1a76, [angles, this, f(0x340), this], [this, joint, 0], 2, true, true),
        16 => (0x0086_b2e0, 0x0086_1b39, [16, model, f(0x3c0), joint], [this, 0, 0], 1, false, false),
        _ => return None,
    };
    let regs = Registers { eax: r[0], ecx: r[1], esi: r[2], edi: r[3] };
    Some(Provider { target, back, regs, args, nargs, angles: takes_angles, offset_and_scale })
}

/// 0x00860AC0 `uCnsEdit::vf24(joint, model)` (thiscall, `ret 8`), see the
/// module documentation.
// Translated from 0x00860AC0 (DDDA.exe 2017-12-13 Steam)
pub fn constraint_edit_apply(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    let entry = c.esp;
    let joint = pg.u32(m, entry.wrapping_add(4));
    let model = pg.u32(m, entry.wrapping_add(8));
    let (ebx, ebp, esi, edi) = (c.ebx, c.ebp, c.esi, c.edi);
    if pg.u8(m, this.wrapping_add(0x61)) != 0 && pg.u8(m, this.wrapping_add(0xb1)) == 0 {
        // the original's ESP after its prologue (frame pointer, 16-byte
        // alignment, 0x3f4 bytes of locals, three saved registers)
        let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x400);
        apply(m, c, pg, this, joint, model, frame);
    }
    (c.ebx, c.ebp, c.esi, c.edi) = (ebx, ebp, esi, edi);
    c.esp = entry.wrapping_add(4 + 8);
}

/// Mode 0..=5: the Euler rotation built with the CRT sinf/cosf.
fn euler_local_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, mode: u32, frame: u32) -> Matrix {
    let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
    // return addresses of the six trig calls of this mode's code
    let base = [0x0086_0b4fu32, 0x0086_0d12, 0x0086_0e9e, 0x0086_1014, 0x0086_118b, 0x0086_12f1][mode as usize];
    let back = |k: u32| base + 0x17 * k + 5;
    let sines = [0, 1, 2].map(|k| crt_sincos(m, c, pg, frame, angles[k], false, back(k as u32)));
    let cosines = [0, 1, 2].map(|k| crt_sincos(m, c, pg, frame, angles[k], true, back(3 + k as u32)));
    euler_matrix(mode, sines, cosines)
}

/// The local matrix from a guest provider (modes 6..=15). `None` for mode
/// 16, whose provider does the whole edit itself.
fn provider_local_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, provider: &Provider, mode: u32, frame: u32) -> Option<Matrix> {
    if provider.angles {
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        for (k, v) in angles.iter().enumerate() {
            pg.set_u32(m, frame + F_ANGLES + 4 * k as u32, v.to_bits());
        }
        pg.set_u32(m, frame + F_ANGLES + 12, 0);
    }
    let result = guest_call(m, c, pg, frame, provider.target, provider.back, provider.regs, &provider.args[..provider.nargs]);
    (mode != 16).then(|| read_matrix(m, pg, result))
}

/// Blend `local` toward identity by `weight` through the guest function
/// 0x00CFDC80, which works in place on the matrix at `F + 0x30`.
fn blend_toward_identity(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, frame: u32, weight: f32, local: &Matrix) -> Matrix {
    write_matrix(m, pg, frame + 0x30, local);
    write_matrix(m, pg, frame + 0x140, &IDENTITY);
    let regs = Registers { eax: frame + 0x140, ecx: frame + 0x30, esi: c.esi, edi: this };
    guest_call(m, c, pg, frame, BLEND_TOWARD_IDENTITY, 0x0086_1ddf, regs, &[frame + 0x30, frame + 0x140, weight.to_bits()]);
    read_matrix(m, pg, frame + 0x30)
}

/// The parent bone's matrix for the parent-space multiply: the joint of the
/// model selected by byte `joint + 0x91`, or for 0xFF the matrix the model's
/// virtual method at vtable +0x58 returns for -1.
fn parent_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, joint: u32, model: u32, frame: u32) -> Matrix {
    let index = pg.u8(m, joint + 0x91);
    if index == 0xff {
        let vtable = pg.u32(m, model);
        let method = pg.u32(m, vtable + 0x58);
        let regs = Registers { eax: method, ecx: model, esi: c.esi, edi: this };
        let result = guest_call(m, c, pg, frame, method, 0x0086_2372, regs, &[0xffff_ffff]);
        read_matrix(m, pg, result)
    } else {
        let joints = pg.u32(m, model + 0x364);
        read_matrix(m, pg, joints.wrapping_add(index as u32 * 0xa0).wrapping_add(0x10))
    }
}

fn apply(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, joint: u32, model: u32, frame: u32) {
    let mode = pg.u32(m, this.wrapping_add(0xa4));
    let (mut local, offset_and_scale) = if mode <= 5 {
        (euler_local_matrix(m, c, pg, this, mode, frame), true)
    } else if let Some(provider) = provider_for(mode, this, joint, model, frame) {
        match provider_local_matrix(m, c, pg, this, &provider, mode, frame) {
            Some(matrix) => (matrix, provider.offset_and_scale),
            None => return, // mode 16 did everything
        }
    } else {
        // mode > 16: the original reads its (uninitialised) stack slots
        (read_matrix(m, pg, frame + 0x30), true)
    };
    if offset_and_scale {
        apply_offset_and_scale(m, pg, this, &mut local);
    }
    let weight = read_f32(m, pg, this + 0x44);
    if weight != 1.0 {
        local = blend_toward_identity(m, c, pg, this, frame, weight, &local);
    }
    let joint_matrix = joint + 0x10;
    if pg.u32(m, this + 0xa0) == 0 {
        // joint = local * joint
        let current = read_matrix(m, pg, joint_matrix);
        write_matrix(m, pg, joint_matrix, &matrix_multiply(&local, &current));
    } else {
        // joint = joint * inverse(parent) * local * parent
        let parent = parent_matrix(m, c, pg, this, joint, model, frame);
        let inverse = invert_parent_matrix(&parent);
        let current = read_matrix(m, pg, joint_matrix);
        let result = matrix_multiply(&matrix_multiply(&matrix_multiply(&current, &inverse), &local), &parent);
        write_matrix(m, pg, joint_matrix, &result);
    }
}
