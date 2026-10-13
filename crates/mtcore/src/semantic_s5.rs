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
//! | 16 | not a matrix: the whole edit is done in parent space (0x0086B2E0, ported below) |
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
    if mode == 16 {
        // the provider 0x0086B2E0 does the whole edit; its scale-matrix output buffer is
        // scratch in this frame, so it is folded in without writing it
        let callee_entry = frame.wrapping_sub(8);
        let callee_frame = (callee_entry.wrapping_sub(4) & !15).wrapping_sub(0x11c + 4);
        scale_and_translate_in_parent_space(m, c, pg, this, joint, model, callee_frame, frame + 0x3c0);
        return;
    }
    let (mut local, offset_and_scale) = if mode == 6 {
        // 0x00864660 folded in: its three angles are the ones computed for the stack vector
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let callee_entry = frame.wrapping_sub(8);
        let callee_frame = (callee_entry.wrapping_sub(4) & !15).wrapping_sub(0xdc + 4);
        (aim_twist_matrix(m, c, pg, this, model, joint, Angles::Values(angles), callee_frame), true)
    } else if mode == 13 || mode == 14 {
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let (locals, matrix_fn): (u32, fn(&mut dyn Mem, &mut Cpu, Pages, u32, u32, u32, Angles, u32, u32) -> Matrix) =
            if mode == 13 { (0xdc, euler_reference_matrix_13) } else { (0xec, euler_reference_matrix_14) };
        let callee_frame = (frame.wrapping_sub(16).wrapping_sub(4) & !15).wrapping_sub(locals + 4);
        (matrix_fn(m, c, pg, this, model, joint, Angles::Values(angles), callee_frame, frame + if mode == 13 { 0x240 } else { 0x2c0 }), false)
    } else if mode == 12 {
        let callee_frame = (frame.wrapping_sub(8).wrapping_sub(4) & !15).wrapping_sub(0xec + 4);
        (rebase_joint_matrix(m, c, pg, this, model, joint, callee_frame, frame + 0x1c0), false)
    } else if mode == 8 {
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let callee_frame = (frame.wrapping_sub(16).wrapping_sub(4) & !15).wrapping_sub(0xd8 + 8);
        (euler_parent_translation_matrix(m, c, pg, this, model, joint, Angles::Values(angles), callee_frame, frame + 0x200), false)
    } else if mode == 9 || mode == 10 {
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let callee_frame = (frame.wrapping_sub(16).wrapping_sub(4) & !15).wrapping_sub(0x5c + 4);
        (euler_row_scaled_matrix(m, c, pg, this, model, joint, Angles::Values(angles), callee_frame), true)
    } else if mode == 11 {
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let callee_frame = (frame.wrapping_sub(16).wrapping_sub(4) & !15).wrapping_sub(0x6c + 4);
        (euler_weighted_scaled_matrix(m, c, pg, this, model, joint, Angles::Values(angles), callee_frame), true)
    } else if mode == 15 {
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let callee_frame = (frame.wrapping_sub(12).wrapping_sub(4) & !15).wrapping_sub(0x38 + 8);
        (euler_or_ratio_matrix(m, c, pg, this, joint, Angles::Values(angles), callee_frame), true)
    } else if mode == 7 {
        // 0x00865990 folded in (three stack arguments, so its entry ESP is F - 16)
        let angles = [0x70, 0x74, 0x78].map(|offset| read_f32(m, pg, this + offset) * DEGREES_TO_RADIANS);
        let callee_frame = (frame.wrapping_sub(16).wrapping_sub(4) & !15).wrapping_sub(0x16c + 4);
        (euler_twist_matrix(m, c, pg, this, model, joint, Angles::Values(angles), callee_frame, frame + 0x380), false)
    } else if mode <= 5 {
        (euler_local_matrix(m, c, pg, this, mode, frame), true)
    } else {
        // mode > 16: the original reads its (uninitialised) stack slots
        (read_matrix(m, pg, frame + 0x30), true)
    };
    if offset_and_scale {
        apply_offset_and_scale(m, pg, this, &mut local);
    }
    let weight = read_f32(m, pg, this + 0x44);
    if weight != 1.0 {
        local = blend_toward(m, pg, &local, &IDENTITY, weight);
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

// ---------------------------------------------------------------------
// 0x0086B2E0: mode 16 of the constraint edit

/// The scale matrix of mode 16: identity with its first three rows scaled
/// by the vector at `this + 0x80` when that vector's squared length exceeds
/// [`SCALE_EPSILON`]. Rows are only multiplied, never summed.
fn scale_matrix(m: &dyn Mem, pg: Pages, this: u32) -> Matrix {
    let mut scale = IDENTITY;
    let (x, y, z) = (read_f32(m, pg, this + 0x80), read_f32(m, pg, this + 0x84), read_f32(m, pg, this + 0x88));
    if x * x + y * y + z * z > SCALE_EPSILON {
        for e in &mut scale[0..4] {
            *e *= x;
        }
        for e in &mut scale[4..8] {
            *e *= y;
        }
        for e in &mut scale[8..12] {
            *e *= z;
        }
    }
    scale
}

/// Mode 16 of `uCnsEdit::vf24`: re-express the joint in the frame of its
/// parent bone with the constraint's scale and translation applied there:
/// `joint = (scale * (joint * inverse(parent))) * parent`, then the
/// constraint translation (`this + 0x90`), carried through the parent's 3x3
/// part, is added to the joint's translation row. Returns the scale matrix
/// (the original keeps it in a caller buffer and returns it).
///
/// `frame` is the callee's ESP after its prologue (for the parent accessor
/// call); the joint matrix is written in place.
fn scale_and_translate_in_parent_space(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, joint: u32, model: u32, frame: u32, out: u32) -> Matrix {
    let scale = scale_matrix(m, pg, this);
    let index = pg.u8(m, joint + 0x91);
    let parent = if index == 0xff {
        let vtable = pg.u32(m, model);
        let method = pg.u32(m, vtable + 0x58);
        let regs = Registers { eax: method, ecx: model, esi: out, edi: joint };
        let result = guest_call(m, c, pg, frame, method, 0x0086_b4d0, regs, &[0xffff_ffff]);
        read_matrix(m, pg, result)
    } else {
        let joints = pg.u32(m, model + 0x364);
        read_matrix(m, pg, joints.wrapping_add(index as u32 * 0xa0).wrapping_add(0x10))
    };
    let inverse = invert_parent_matrix(&parent);
    let current = read_matrix(m, pg, joint + 0x10);
    let mut result = matrix_multiply(&matrix_multiply(&scale, &matrix_multiply(&current, &inverse)), &parent);
    let (tx, ty, tz) = (read_f32(m, pg, this + 0x90), read_f32(m, pg, this + 0x94), read_f32(m, pg, this + 0x98));
    // the translation transformed by the parent, minus the parent's own translation
    let delta = |k: usize| (((ty * parent[4 + k] + tx * parent[k]) + tz * parent[8 + k]) + parent[12 + k]) - parent[12 + k];
    result[12] += delta(0);
    result[13] = delta(1) + result[13];
    result[14] = delta(2) + result[14];
    // the original loads and stores the last element through the x87 stack,
    // which turns a signalling NaN into a quiet one
    if result[15].is_nan() {
        result[15] = f32::from_bits(result[15].to_bits() | 0x0040_0000);
    }
    write_matrix(m, pg, joint + 0x10, &result);
    scale
}

/// 0x0086B2E0 (usercall: ECX = model, EDI = joint, ESI = output buffer,
/// stack argument `this`, `ret 4`): [`scale_and_translate_in_parent_space`],
/// writing the scale matrix to the buffer and returning it in EAX. Only
/// EAX, ECX/EDX and ESP change; EBX, ESI, EDI and EBP are preserved.
// Translated from 0x0086B2E0 (DDDA.exe 2017-12-13 Steam)
pub fn scale_translate_in_parent_space(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = pg.u32(m, entry.wrapping_add(4));
    // ESP after the prologue: ebp, 16-byte alignment, 0x11c bytes, ebx
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x11c + 4);
    let (model, joint, out) = (c.ecx, c.edi, c.esi);
    let scale = scale_and_translate_in_parent_space(m, c, pg, this, joint, model, frame, out);
    write_matrix(m, pg, out, &scale);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 4);
    c.esi = out;
    c.edi = joint;
}

// ---------------------------------------------------------------------
// 0x00864660: mode 6, the aim/twist provider

mod kernels;

/// What the generated kernels' loaders read their inputs from: record
/// addresses in the guest and already-read values.
#[derive(Clone, Copy, Default)]
pub(crate) struct Bases {
    /// the constraint (`uCnsEdit`)
    this: u32,
    /// the constrained joint record
    joint: u32,
    /// another joint record (the aim target)
    child: u32,
    /// the parent bone's matrix
    parent: u32,
    /// the angle of the stage being run
    angle: f32,
    /// the three angles of the constraint (radians)
    angles: [f32; 3],
    /// a vector returned by a guest helper
    r: [f32; 3],
    /// the model's scale (`model + 0x60..0x68`), read only when the kernel needs it
    mscale: [f32; 3],
    /// a second matrix (the blend target)
    other: Matrix,
}

/// The joint record of `model` for joint index `index` (the byte table at
/// `model + 0x368` maps indices to joints; 0xFF is none, the null record),
/// as the accessor 0x00421ED0 (thiscall, `ret 4`) does.
fn joint_record(m: &dyn Mem, pg: Pages, model: u32, index: u32) -> u32 {
    let table = pg.u32(m, model + 0x368);
    let slot = pg.u8(m, table.wrapping_add(index & 0xff)) as u32;
    if slot == 0xff {
        return 0;
    }
    slot.wrapping_mul(0xa0).wrapping_add(pg.u32(m, model + 0x364))
}

/// Smallest angle (radians) the aim/twist stages act on, 0x0150A350.
const ANGLE_EPSILON: f32 = f32::from_bits(0x3400_0000);

/// Return addresses of the sinf/cosf call sites in 0x00864660, per stage.
const AIM_TRIG_SITES: [u32; 2] = [0x0086_5101, 0x0086_513f];
const PITCH_TRIG_SITES: [u32; 2] = [0x0086_5299, 0x0086_52aa];
const ROLL_TRIG_SITES: [u32; 2] = [0x0086_5613, 0x0086_5624];

/// The three angles (radians) a provider is driven by: in guest memory (the
/// provider's stack argument points at them) or already computed.
#[derive(Clone, Copy)]
enum Angles {
    Memory(u32),
    Values([f32; 3]),
}

impl Angles {
    fn get(self, m: &dyn Mem, pg: Pages, k: usize) -> f32 {
        match self {
            Angles::Memory(address) => read_f32(m, pg, address + 4 * k as u32),
            Angles::Values(v) => v[k],
        }
    }
}

/// The matrix of mode 6 (0x00864660): identity, then up to three stages
/// driven by the three angles (radians) at `angles`: an aim rotation of the
/// joint toward joint `this+0xA8 + 2` of the model by angle 0 (a half-angle
/// quaternion about the axis from the joint to that joint, seen in the
/// joint's own frame), and rotations by angle 1 and angle 2 about two of the
/// matrix's axes. A stage runs only when its angle exceeds [`ANGLE_EPSILON`];
/// without a model the matrix stays identity.
fn aim_twist_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32) -> Matrix {
    let mut matrix = IDENTITY;
    if model == 0 {
        return matrix;
    }
    let trig = |sites: [u32; 2]| {
        move |m: &mut dyn Mem, c: &mut Cpu, cosine: bool, x: f32| crt_sincos(m, c, pg, frame, x, cosine, sites[cosine as usize])
    };
    let a0 = angles.get(m, pg, 0);
    if a0 > ANGLE_EPSILON {
        let index = pg.u32(m, this + 0xa8).wrapping_add(2);
        if index < pg.u32(m, model + 0x360) {
            let child = joint_record(m, pg, model, index);
            let inputs = kernels::load_m6_aim(m, pg, &Bases { joint, child, angle: a0, ..Default::default() }, &matrix);
            let t = trig(AIM_TRIG_SITES);
            matrix = kernels::kernel_m6_aim(&inputs, &mut |cosine, x| t(m, c, cosine, x));
        }
    }
    let a1 = angles.get(m, pg, 1);
    if a1 > ANGLE_EPSILON {
        let inputs = kernels::load_m6_pitch(m, pg, &Bases { angle: a1, ..Default::default() }, &matrix);
        let t = trig(PITCH_TRIG_SITES);
        matrix = kernels::kernel_m6_pitch(&inputs, &mut |cosine, x| t(m, c, cosine, x));
    }
    let a2 = angles.get(m, pg, 2);
    if a2 > ANGLE_EPSILON {
        let inputs = kernels::load_m6_roll(m, pg, &Bases { angle: a2, ..Default::default() }, &matrix);
        let t = trig(ROLL_TRIG_SITES);
        matrix = kernels::kernel_m6_roll(&inputs, &mut |cosine, x| t(m, c, cosine, x));
    }
    matrix
}

/// 0x00864660 (usercall: EAX = this, ECX = model, EDI = joint, ESI = output
/// buffer, stack argument = pointer to the three angles, `ret 4`):
/// [`aim_twist_matrix`] written to the buffer and returned in EAX. EBX, ESI,
/// EDI and EBP are preserved.
// Translated from 0x00864660 (DDDA.exe 2017-12-13 Steam)
pub fn aim_twist_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let angles = pg.u32(m, entry.wrapping_add(4));
    // ESP after the prologue: ebp, 16-byte alignment, 0xdc bytes of locals, ebx
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xdc + 4);
    let (this, model, joint, out) = (c.eax, c.ecx, c.edi, c.esi);
    let matrix = aim_twist_matrix(m, c, pg, this, model, joint, Angles::Memory(angles), frame);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 4);
    c.esi = out;
    c.edi = joint;
}

// ---------------------------------------------------------------------
// 0x00865990: mode 7, the Euler-with-twist provider

/// Return addresses of the sinf/cosf calls of 0x00865990 (first sine and cosine of the angles).
const EULER_TRIG_SITES: [u32; 2] = [0x0086_59af, 0x0086_59f5];
/// ... and of the second rotation built from the decomposed matrix.
const TWIST_TRIG_SITES: [u32; 2] = [0x0086_6da0, 0x0086_6de5];

/// The parent bone's matrix (joint byte `+0x91`; 0xFF: the model's virtual
/// +0x58(-1) accessor) as a guest address. `back` is the call site's return
/// address when the accessor is called.
fn parent_matrix_address(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, joint: u32, model: u32, frame: u32, regs: Registers, back: u32, method_in_eax: bool) -> u32 {
    let index = pg.u8(m, joint + 0x91);
    if index == 0xff {
        let vtable = pg.u32(m, model);
        let method = pg.u32(m, vtable + 0x58);
        let eax = if method_in_eax { method } else { vtable };
        guest_call(m, c, pg, frame, method, back, Registers { eax, edi: regs.edi, ..regs }, &[0xffff_ffff])
    } else {
        let joints = pg.u32(m, model + 0x364);
        joints.wrapping_add(index as u32 * 0xa0).wrapping_add(0x10)
    }
}

/// The matrix of mode 7 (0x00865990): the Euler rotation of the three
/// angles (radians); then, when the constraint's weight at `this + 0x84` is
/// not 1, the joint's rotation relative to its parent bone is decomposed
/// into angles (0x00CFC9E0), scaled by `weight - 1` (x and y by `this +
/// 0x80` and `this + 0x84`) and rotated back in; finally row 0 is scaled by
/// `this + 0x88` and the translation (`this + 0x90`) added.
///
/// `out` is the guest address of the original's output buffer (it is passed
/// to the guest helper); `frame` the provider's frame.
fn euler_twist_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32, out: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let mut matrix = IDENTITY;
    let mut bases = Bases { this, joint, angles, ..Default::default() };
    let inputs = kernels::load_m7_euler(m, pg, &bases, &matrix);
    matrix = kernels::kernel_m7_euler(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, EULER_TRIG_SITES[cosine as usize]));
    if read_f32(m, pg, this + 0x84) != 1.0 {
        let regs = Registers { eax: 0, ecx: model, esi: out, edi: joint };
        bases.parent = parent_matrix_address(m, c, pg, joint, model, frame, regs, 0x0086_5b62, false);
        let inputs = kernels::load_m7_in_parent_space(m, pg, &bases, &matrix);
        let relative = kernels::kernel_m7_in_parent_space(&inputs, &mut |_, x| x);
        // 0x00CFC9E0, the rotation matrix to Euler angles (s6's bit-exact atan2/asinf), folded in
        let mut elements = [0.0f32; 11];
        for k in [0usize, 1, 2, 4, 5, 6, 10] {
            elements[k] = relative[k];
        }
        bases.r = crate::semantic_s6_pose::euler_from_matrix(&crate::semantic_s6_crt::Tables { memory: &*m, pages: pg }, &elements);
        let inputs = kernels::load_m7_scale_rotation(m, pg, &bases, &matrix);
        matrix = kernels::kernel_m7_scale_rotation(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, TWIST_TRIG_SITES[cosine as usize]));
    }
    let inputs = kernels::load_m7_scale_translate(m, pg, &bases, &matrix);
    kernels::kernel_m7_scale_translate(&inputs, &mut |_, x| x)
}

/// 0x00865990 (usercall: EDI = joint, ESI = output buffer; stack arguments
/// `this`, `model`, pointer to the three angles; `ret 0xc`):
/// [`euler_twist_matrix`] written to the buffer and returned in EAX. EBX,
/// ESI, EDI and EBP are preserved.
// Translated from 0x00865990 (DDDA.exe 2017-12-13 Steam)
pub fn euler_twist_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, model, angles) = (pg.u32(m, entry.wrapping_add(4)), pg.u32(m, entry.wrapping_add(8)), pg.u32(m, entry.wrapping_add(12)));
    // ESP after the prologue: ebp, 16-byte alignment, 0x16c bytes of locals, ebx
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x16c + 4);
    let (joint, out) = (c.edi, c.esi);
    let matrix = euler_twist_matrix(m, c, pg, this, model, joint, Angles::Memory(angles), frame, out);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 12);
    c.esi = out;
    c.edi = joint;
}

// ---------------------------------------------------------------------
// 0x00867B70 (modes 9, 10), 0x0086D180 (mode 11), 0x0086AFF0 (mode 15)

/// Return addresses of the first sinf/cosf call sites of each provider.
const M9_TRIG_SITES: [u32; 2] = [0x0086_7b8c, 0x0086_7bc9];
const M11_TRIG_SITES: [u32; 2] = [0x0086_d19c, 0x0086_d1d9];
const M15_TRIG_SITES: [u32; 2] = [0x0086_b00f, 0x0086_b04c];

/// The model's scale vector, which the providers use when the byte at
/// `this + 0xB0` is clear (otherwise they scale from 1).
fn model_scale(m: &dyn Mem, pg: Pages, this: u32, model: u32) -> [f32; 3] {
    if pg.u8(m, this + 0xb0) == 0 {
        [read_f32(m, pg, model + 0x60), read_f32(m, pg, model + 0x64), read_f32(m, pg, model + 0x68)]
    } else {
        [0.0; 3]
    }
}

/// Matrix of modes 9 and 10 (0x00867B70): the Euler rotation of the angles
/// with its three rows scaled by (the model's scale, or 1) divided by the
/// length of the joint's own rows.
fn euler_row_scaled_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let bases = Bases { this, joint, angles, mscale: model_scale(m, pg, this, model), ..Default::default() };
    let inputs = kernels::load_m9(m, pg, &bases, &IDENTITY);
    kernels::kernel_m9(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, M9_TRIG_SITES[cosine as usize]))
}

/// Matrix of mode 11 (0x0086D180): like [`euler_row_scaled_matrix`] with
/// the scale weighted by the floats at `this + 0xC0`; a joint with byte
/// `+0x93` equal to 2 only scales its second row.
fn euler_weighted_scaled_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let bases = Bases { this, joint, angles, mscale: model_scale(m, pg, this, model), ..Default::default() };
    let inputs = kernels::load_m11(m, pg, &bases, &IDENTITY);
    kernels::kernel_m11(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, M11_TRIG_SITES[cosine as usize]))
}

/// Matrix of mode 15 (0x0086AFF0): the Euler rotation, replaced by a pure
/// scale matrix when `this + 0xAC` selects a reference joint (of the model
/// at `this + 0x54`): each diagonal element is the ratio of the length of
/// a row of the reference joint to that of the same row of `joint`
/// (1 where the joint's row is too short).
fn euler_or_ratio_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, joint: u32, angles: Angles, frame: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let bases = Bases { this, joint, angles, ..Default::default() };
    let inputs = kernels::load_m15_euler(m, pg, &bases, &IDENTITY);
    let euler = kernels::kernel_m15_euler(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, M15_TRIG_SITES[cosine as usize]));
    let reference_index = pg.u32(m, this + 0xac);
    if reference_index == 0xff {
        return euler;
    }
    let model = pg.u32(m, this + 0x54);
    let reference = joint_record(m, pg, model, reference_index);
    if reference == 0 {
        return euler;
    }
    // 0x01629510..18: the default ratios (1.0)
    let mut ratio = [1.0f32; 3];
    for (row, r) in ratio.iter_mut().enumerate() {
        let at = |base: u32, k: u32| read_f32(m, pg, base + 0x10 + 0x10 * row as u32 + 4 * k);
        let own = (((at(joint, 0) * at(joint, 0) + at(joint, 1) * at(joint, 1)) + at(joint, 2) * at(joint, 2)) + at(joint, 3) * at(joint, 3)).sqrt();
        if own > ANGLE_EPSILON {
            let other = (((at(reference, 0) * at(reference, 0) + at(reference, 1) * at(reference, 1)) + at(reference, 2) * at(reference, 2)) + at(reference, 3) * at(reference, 3)).sqrt();
            *r = other / own;
        }
    }
    let mut matrix = [0.0f32; 16];
    matrix[0] = ratio[0];
    matrix[5] = ratio[1];
    matrix[10] = ratio[2];
    matrix[15] = 1.0;
    matrix
}

/// 0x00867B70 (modes 9, 10; usercall: EDI = pointer to the three angles, ESI =
/// output buffer; stack arguments `this`, `joint`, `model`; `ret 0xc`).
// Translated from 0x00867B70 (DDDA.exe 2017-12-13 Steam)
pub fn euler_row_scaled_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, joint, model) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8), pg.u32(m, entry + 12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x5c + 4);
    let (angles, out) = (c.edi, c.esi);
    let matrix = euler_row_scaled_matrix(m, c, pg, this, model, joint, Angles::Memory(angles), frame);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 12);
    c.esi = out;
    c.edi = angles;
}

/// 0x0086D180 (mode 11; usercall: EDI = joint, ESI = output buffer; stack
/// arguments `this`, `model`, pointer to the angles; `ret 0xc`).
// Translated from 0x0086D180 (DDDA.exe 2017-12-13 Steam)
pub fn euler_weighted_scaled_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, model, angles) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8), pg.u32(m, entry + 12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x6c + 4);
    let (joint, out) = (c.edi, c.esi);
    let matrix = euler_weighted_scaled_matrix(m, c, pg, this, model, joint, Angles::Memory(angles), frame);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 12);
    c.esi = out;
    c.edi = joint;
}

/// 0x0086AFF0 (mode 15; usercall: EAX = pointer to the angles, ESI = output
/// buffer; stack arguments `this`, `joint`; `ret 8`).
// Translated from 0x0086AFF0 (DDDA.exe 2017-12-13 Steam)
pub fn euler_or_ratio_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, joint) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x38 + 8);
    let (angles, out) = (c.eax, c.esi);
    let matrix = euler_or_ratio_matrix(m, c, pg, this, joint, Angles::Memory(angles), frame);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 8);
    c.esi = out;
}

// ---------------------------------------------------------------------
// 0x00867420: mode 8, Euler rotation with a parent-relative translation

const M8_TRIG_SITES: [u32; 2] = [0x0086_7442, 0x0086_7488];

/// Matrix of mode 8 (0x00867420): the Euler rotation of the angles; when the
/// weight at `this + 0x84` or the one at `this + 0x88` is not 1, its
/// translation row is computed from the parent bone's matrix and the
/// joint's own translation and `this + 0x08`-th element (the weights
/// interpolate the joint's translation toward the parent's frame); then the
/// translation at `this + 0x90` is added.
fn euler_parent_translation_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32, out: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let mut bases = Bases { this, joint, angles, ..Default::default() };
    let inputs = kernels::load_m8_euler(m, pg, &bases, &IDENTITY);
    let mut matrix = kernels::kernel_m8_euler(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, M8_TRIG_SITES[cosine as usize]));
    if read_f32(m, pg, this + 0x84) != 1.0 || read_f32(m, pg, this + 0x88) != 1.0 {
        let regs = Registers { eax: 0, ecx: model, esi: out, edi: joint };
        bases.parent = parent_matrix_address(m, c, pg, joint, model, frame, regs, 0x0086_7606, false);
        let inputs = kernels::load_m8_parent(m, pg, &bases, &matrix);
        matrix = kernels::kernel_m8_parent(&inputs, &mut |_, x| x);
    }
    let inputs = kernels::load_m8_finish(m, pg, &bases, &matrix);
    kernels::kernel_m8_finish(&inputs, &mut |_, x| x)
}

/// 0x00867420 (usercall: EAX = pointer to the three angles, ESI = output
/// buffer; stack arguments `this`, `joint`, `model`; `ret 0xc`).
// Translated from 0x00867420 (DDDA.exe 2017-12-13 Steam)
pub fn euler_parent_translation_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, joint, model) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8), pg.u32(m, entry + 12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xd8 + 8);
    let (angles, out, saved_edi) = (c.eax, c.esi, c.edi);
    let matrix = euler_parent_translation_matrix(m, c, pg, this, model, joint, Angles::Memory(angles), frame, out);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 12);
    c.esi = out;
    c.edi = saved_edi;
}

// ---------------------------------------------------------------------
// 0x00868000: mode 12, re-base the joint on its parent

const DECOMPOSE_QUATERNION_SITE: u32 = 0x0086_833e;

/// Write `matrix` as the joint's matrix (record + 0x10), touching only the
/// elements that change.
fn write_joint_matrix_changes(m: &mut dyn Mem, pg: Pages, joint: u32, matrix: &Matrix) {
    for (k, v) in matrix.iter().enumerate() {
        let address = joint + 0x10 + 4 * k as u32;
        if pg.u32(m, address) != v.to_bits() {
            pg.set_u32(m, address, v.to_bits());
        }
    }
}

/// Mode 12 (0x00868000): the output matrix is the identity with its
/// translation moved by the constraint's weights; as a side effect the joint's
/// own matrix is re-based on the parent bone: its rows are scaled by the
/// inverse of the lengths of the joint's rows (when they differ from 1),
/// multiplied by the matrix of the joint's quaternion (`joint + 0x60`, via
/// 0x007E4B60, folded in) and the parent, and the translation difference
/// between the old and new frame, weighted by `this + 0x80..0x88`, goes to the
/// output's translation row.
///
/// Returns the output matrix; the joint matrix is updated in guest memory.
fn rebase_joint_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, frame: u32, out: u32) -> Matrix {
    let regs = Registers { eax: 0, ecx: model, esi: joint, edi: out };
    let bases = Bases { this, joint, parent: parent_matrix_address(m, c, pg, joint, model, frame, regs, 0x0086_81aa, true), ..Default::default() };
    let inputs = kernels::load_m12(m, pg, &bases, &IDENTITY);
    let result = kernels::kernel_m12(&inputs, &mut |_, x| x);
    let out_matrix: Matrix = std::array::from_fn(|k| result[k]);
    let joint_matrix: Matrix = std::array::from_fn(|k| result[16 + k]);
    write_joint_matrix_changes(m, pg, joint, &joint_matrix);
    let _ = DECOMPOSE_QUATERNION_SITE;
    out_matrix
}

/// 0x00868000 (usercall: ECX = model, ESI = joint, EDI = output buffer; stack
/// argument `this`; `ret 4`).
// Translated from 0x00868000 (DDDA.exe 2017-12-13 Steam)
pub fn rebase_joint_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = pg.u32(m, entry + 4);
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xec + 4);
    let (model, joint, out) = (c.ecx, c.esi, c.edi);
    let matrix = rebase_joint_matrix(m, c, pg, this, model, joint, frame, out);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 4);
    c.esi = joint;
    c.edi = out;
}

// ---------------------------------------------------------------------
// 0x00869260 (mode 13) and 0x0086A100 (mode 14): Euler rotation with a
// translation taken relative to a reference joint

const M13_TRIG_SITES: [u32; 2] = [0x0086_9282, 0x0086_92c5];
const M14_TRIG_SITES: [u32; 2] = [0x0086_a11f, 0x0086_a15c];

/// The reference joint record of the constraint: joint `this + 0xAC` of the
/// model at `this + 0x54` (0 when none: index 0xFF, a missing joint or a
/// null record).
fn reference_joint(m: &dyn Mem, pg: Pages, this: u32) -> u32 {
    let index = pg.u32(m, this + 0xac);
    if index == 0xff {
        return 0;
    }
    let model = pg.u32(m, this + 0x54);
    joint_record(m, pg, model, index)
}

/// Matrix of mode 13 (0x00869260): the Euler rotation; with a reference joint
/// its translation row becomes a blend, per axis weight `this + 0x80..0x88`, of
/// the reference joint's translation and the joint's own translation moved by
/// `this + 0x90` through the parent bone, relative to the joint's matrix.
fn euler_reference_matrix_13(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32, out: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let mut bases = Bases { this, joint, angles, ..Default::default() };
    let inputs = kernels::load_m13_euler(m, pg, &bases, &IDENTITY);
    let mut matrix = kernels::kernel_m13_euler(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, M13_TRIG_SITES[cosine as usize]));
    bases.child = reference_joint(m, pg, this);
    if bases.child != 0 {
        let regs = Registers { eax: 0, ecx: model, esi: joint, edi: out };
        bases.parent = parent_matrix_address(m, c, pg, joint, model, frame, regs, 0x0086_9454, true);
        let inputs = kernels::load_m13_ref(m, pg, &bases, &matrix);
        matrix = kernels::kernel_m13_ref(&inputs, &mut |_, x| x);
    }
    matrix
}

/// Matrix of mode 14 (0x0086A100): like mode 13, without the per-axis weights.
fn euler_reference_matrix_14(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, model: u32, joint: u32, angles: Angles, frame: u32, out: u32) -> Matrix {
    let angles = [angles.get(m, pg, 0), angles.get(m, pg, 1), angles.get(m, pg, 2)];
    let mut bases = Bases { this, joint, angles, ..Default::default() };
    let inputs = kernels::load_m14_euler(m, pg, &bases, &IDENTITY);
    let mut matrix = kernels::kernel_m14_euler(&inputs, &mut |cosine, x| crt_sincos(m, c, pg, frame, x, cosine, M14_TRIG_SITES[cosine as usize]));
    bases.child = reference_joint(m, pg, this);
    if bases.child != 0 {
        let regs = Registers { eax: 0, ecx: model, esi: joint, edi: out };
        bases.parent = parent_matrix_address(m, c, pg, joint, model, frame, regs, 0x0086_a356, false);
        let inputs = kernels::load_m14_ref(m, pg, &bases, &matrix);
        matrix = kernels::kernel_m14_ref(&inputs, &mut |_, x| x);
    }
    matrix
}

/// 0x00869260 (mode 13; usercall: ESI = joint, EDI = output buffer; stack
/// arguments `this`, `model`, pointer to the angles; `ret 0xc`).
// Translated from 0x00869260 (DDDA.exe 2017-12-13 Steam)
pub fn euler_reference_provider_13(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, model, angles) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8), pg.u32(m, entry + 12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xdc + 4);
    let (joint, out) = (c.esi, c.edi);
    let matrix = euler_reference_matrix_13(m, c, pg, this, model, joint, Angles::Memory(angles), frame, out);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 12);
    c.esi = joint;
    c.edi = out;
}

/// 0x0086A100 (mode 14; usercall: ESI = joint, EDI = output buffer; stack
/// arguments `this`, `model`, pointer to the angles; `ret 0xc`).
// Translated from 0x0086A100 (DDDA.exe 2017-12-13 Steam)
pub fn euler_reference_provider_14(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (this, model, angles) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8), pg.u32(m, entry + 12));
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0xec + 4);
    let (joint, out) = (c.esi, c.edi);
    let matrix = euler_reference_matrix_14(m, c, pg, this, model, joint, Angles::Memory(angles), frame, out);
    write_matrix(m, pg, out, &matrix);
    c.eax = out;
    c.esp = entry.wrapping_add(4 + 12);
    c.esi = joint;
    c.edi = out;
}

// ---------------------------------------------------------------------
// 0x00CFDC80: blend a matrix toward another (the identity) by a weight

/// `matrix` blended toward `other` by `weight` (0x00CFDC80, `other` is the identity
/// for the uCnsEdit): each row's rotation part is scaled in angle, its length
/// interpolated, and the translation row interpolated linearly. Pure float
/// math, kept in the original's order by the generated kernel.
pub fn blend_toward(m: &dyn Mem, pg: Pages, matrix: &Matrix, other: &Matrix, weight: f32) -> Matrix {
    let bases = Bases { angle: weight, other: *other, ..Default::default() };
    let inputs = kernels::load_blend(m, pg, &bases, matrix);
    kernels::kernel_blend(&inputs, &mut |_, x| x)
}

/// 0x00CFDC80 (usercall: ECX = the matrix to blend, which is also the first
/// stack argument; stack arguments matrix, other matrix, weight; `ret 0xc`).
// Translated from 0x00CFDC80 (DDDA.exe 2017-12-13 Steam)
pub fn blend_toward_provider(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let (matrix_address, other_address, weight) = (pg.u32(m, entry + 4), pg.u32(m, entry + 8), f32::from_bits(pg.u32(m, entry + 12)));
    let matrix = read_matrix(m, pg, matrix_address);
    let other = read_matrix(m, pg, other_address);
    let result = blend_toward(m, pg, &matrix, &other, weight);
    write_matrix(m, pg, c.ecx, &result);
    c.esp = entry.wrapping_add(4 + 12);
}
