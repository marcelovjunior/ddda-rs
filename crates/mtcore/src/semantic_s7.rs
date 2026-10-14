//! L184 s7: semantic port of `uCnsJointOffset::vf24` (0x00F2B5E0), the
//! per-update body of the joint-offset constraint.
//!
//! A `uCnsJointOffset` is a skeleton constraint that nudges one joint
//! (the "constrained joint") by a fixed offset expressed relative to a
//! reference joint. Each update it:
//!
//! 1. builds the offset rotation from three Euler angles (degrees at
//!    `this + 0x70`, rotation order 0..5 at `this + 0xa0`);
//! 2. fetches the reference joint's world matrix: the model's default
//!    (virtual method at vtable + 0x58, argument -1) when the joint's
//!    reference index byte (`joint + 0x91`) is 0xff, else record `index`
//!    of the model's joint array;
//! 3. takes the constrained joint's matrix (`joint + 0x10`, 4x4, row
//!    vectors, translation in row 3) into the reference joint's space with
//!    the reference matrix's cofactor inverse (the identity when it is
//!    singular);
//! 4. splits that into three axis lengths, three unit axes and a
//!    translation; the lengths blend towards `this + 0x80..0x88` times
//!    themselves and the translation moves by `this + 0x90..0x98`, both by
//!    the weight at `this + 0x44`;
//! 5. blends the offset rotation (as a quaternion, taking the shorter arc)
//!    from the identity by the same weight, applies it to the axes, takes
//!    the result back through the reference matrix and writes it to the
//!    joint, scaling the three axis rows by the blended lengths.
//!
//! The arithmetic is the original's single-precision SSE arithmetic
//! operation by operation (same association, no fused operations), so the
//! written matrix is bit for bit the original's. The CRT sinf/cosf are the
//! bit-exact ports in [`crate::semantic_s7_trig`]; their rare x87 range
//! (magnitude 8192 and above) is a guest call. The only other guest call is
//! the model's virtual method.
//!
//! Translated from 0x00F2B5E0 (DDDA.exe 2017-12-13 Steam).
use crate::semantic::call_from;
use crate::semantic_s7_trig::{cosf, sinf};
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// One `float` of the original with its arithmetic pinned to the SSE
/// scalar instruction in the written operand order. Plain `f32` rounds the
/// same way; the pin only matters when both operands of an addition or
/// multiplication are NaN (SSE returns the first operand's), which the
/// compiler may otherwise swap.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub(crate) struct F(pub(crate) f32);

impl F {
    pub(crate) const fn from_bits(bits: u32) -> F {
        F(f32::from_bits(bits))
    }
    #[inline(always)]
    pub(crate) fn to_bits(self) -> u32 {
        self.0.to_bits()
    }
    #[inline(always)]
    pub(crate) fn sqrt(self) -> F {
        F(self.0.sqrt())
    }
}

impl core::ops::Neg for F {
    type Output = F;
    #[inline(always)]
    fn neg(self) -> F {
        F(-self.0)
    }
}

macro_rules! sse_binary {
    ($trait:ident, $method:ident, $insn:literal, $op:tt) => {
        impl core::ops::$trait for F {
            type Output = F;
            #[inline(always)]
            fn $method(self, rhs: F) -> F {
                #[cfg(target_arch = "x86_64")]
                {
                    let mut a = self.0;
                    // SAFETY: one register-to-register SSE instruction; no
                    // memory, stack or flags are touched. MXCSR is the
                    // host's (round to nearest), as in the lifted code.
                    unsafe {
                        core::arch::asm!(concat!($insn, " {a}, {b}"), a = inout(xmm_reg) a, b = in(xmm_reg) rhs.0,
                                         options(pure, nomem, nostack, preserves_flags));
                    }
                    F(a)
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    F(self.0 $op rhs.0)
                }
            }
        }
    };
}
sse_binary!(Add, add, "addss", +);
sse_binary!(Sub, sub, "subss", -);
sse_binary!(Mul, mul, "mulss", *);
sse_binary!(Div, div, "divss", /);

/// Offsets inside the constraint (`this`).
mod constraint {
    /// byte: the constraint is active
    pub const ENABLED: u32 = 0x61;
    /// float: blend weight (0 = leave the joint alone, 1 = full offset)
    pub const WEIGHT: u32 = 0x44;
    /// three floats: Euler angles in degrees
    pub const ANGLES_DEGREES: u32 = 0x70;
    /// three floats: scale multipliers of the three axes
    pub const AXIS_SCALE: u32 = 0x80;
    /// three floats: translation offset
    pub const TRANSLATION: u32 = 0x90;
    /// u32: Euler rotation order, 0..5
    pub const ROTATION_ORDER: u32 = 0xa0;
}

/// Offsets inside the constrained joint.
mod joint {
    /// 4x4 float matrix: rows 0..2 are the scaled axes, row 3 the translation
    pub const MATRIX: u32 = 0x10;
    /// byte: index of the reference joint in the model, 0xff = model default
    pub const REFERENCE_INDEX: u32 = 0x91;
}

/// Offsets inside the model and its joint records.
mod model {
    /// pointer to the joint records
    pub const JOINTS: u32 = 0x364;
    /// size of one joint record
    pub const JOINT_STRIDE: u32 = 0xa0;
    /// the world matrix inside a joint record
    pub const JOINT_MATRIX: u32 = 0x10;
    /// vtable slot of the "default reference matrix" method
    pub const DEFAULT_REFERENCE_SLOT: u32 = 0x58;
}

/// pi / 180 (the float at 0x0161C22C).
const DEGREES_TO_RADIANS: F = F::from_bits(0x3c8e_fa35);
const ZERO: F = F(0.0);
const ONE: F = F(1.0);
const HALF: F = F(0.5);
const TWO: F = F(2.0);

/// A 4x4 matrix in memory order (`m[4 * row + column]`).
type Matrix = [F; 16];

/// A quaternion `[x, y, z, w]`.
type Quaternion = [F; 4];

/// The stack frame the original builds: `push ebp; mov ebp, esp; and esp,
/// -16; sub esp, 0x174; push ebx; push esi; push edi`. Its calls are made
/// with this ESP, and the Euler matrix lives at `+ 0x110` in it.
#[inline(always)]
fn frame_of(entry_esp: u32) -> u32 {
    (entry_esp.wrapping_sub(4) & !15).wrapping_sub(0x174).wrapping_sub(12)
}

/// Where the original keeps the Euler rotation matrix inside its frame
/// (only read back when the rotation order is out of range).
const FRAME_ROTATION: u32 = 0x110;

#[inline(always)]
fn load(pg: Pages, m: &dyn Mem, address: u32) -> F {
    F::from_bits(pg.u32(m, address))
}

#[inline(always)]
fn load_vec3(pg: Pages, m: &dyn Mem, address: u32) -> [F; 3] {
    [load(pg, m, address), load(pg, m, address.wrapping_add(4)), load(pg, m, address.wrapping_add(8))]
}

/// 0x00F2B5E0, `this->update(joint, model)` (thiscall, `ret 8`, void):
/// applies the joint-offset constraint to `joint` (see the module docs).
pub fn joint_offset_update(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    if pg.u8(m, this.wrapping_add(constraint::ENABLED)) != 0 {
        update_active(m, c, pg, entry, this);
    }
    c.esp = entry.wrapping_add(4 + 8);
}

fn update_active(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, entry: u32, this: u32) {
    let frame = frame_of(entry);
    let angles = load_vec3(pg, m, this.wrapping_add(constraint::ANGLES_DEGREES)).map(|a| a * DEGREES_TO_RADIANS);
    let order = pg.u32(m, this.wrapping_add(constraint::ROTATION_ORDER));
    let rotation = offset_rotation_matrix(m, c, pg, frame, order, angles);

    let joint_ptr = pg.u32(m, entry.wrapping_add(4));
    let model_ptr = pg.u32(m, entry.wrapping_add(8));
    let reference = reference_matrix(m, c, pg, frame, joint_ptr, model_ptr);
    let inverse = invert_reference(&reference);

    // the constrained joint's matrix in the reference joint's space
    let joint_matrix: Matrix = std::array::from_fn(|k| load(pg, m, joint_ptr.wrapping_add(joint::MATRIX + 4 * k as u32)));
    let local = into_reference_space(&joint_matrix, &inverse);

    let weight = load(pg, m, this.wrapping_add(constraint::WEIGHT));
    let axis_scale = load_vec3(pg, m, this.wrapping_add(constraint::AXIS_SCALE));
    let offset = load_vec3(pg, m, this.wrapping_add(constraint::TRANSLATION));
    let split = split_axes(&local, weight, axis_scale, offset);

    let blended = blended_rotation(&rotation, weight);
    let result = rotate_and_return(&split.axes, &split.translation, &rotation_matrix_of(blended), &reference);
    for (k, v) in result.iter().enumerate() {
        // rows 0..2 carry the blended axis lengths; the translation row does not
        let v = if k == 0 || k == 3 { split.scales[0] * *v } else if k < 4 { *v * split.scales[0] } else if k < 8 { *v * split.scales[1] } else if k < 12 { *v * split.scales[2] } else { *v };
        pg.set_u32(m, joint_ptr.wrapping_add(joint::MATRIX + 4 * k as u32), v.to_bits());
    }
}

// ---------------------------------------------------------------------
// 1. the offset rotation matrix
// ---------------------------------------------------------------------

/// The CRT sine/cosine of the three angles, as the original calls them
/// (sin x, sin y, sin z, then cos x, cos y, cos z). The x87 range of the
/// CRT is a guest call from the original's frame.
fn sin_cos_of_angles(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, angles: [F; 3]) -> ([F; 3], [F; 3]) {
    const CRT_SINF: u32 = 0x012f_0906;
    const CRT_COSF: u32 = 0x012f_0a55;
    // return addresses of the original's call sites for the first order
    const SIN_BACK: [u32; 3] = [0x00f2_b656, 0x00f2_b667, 0x00f2_b678];
    const COS_BACK: [u32; 3] = [0x00f2_b6a1, 0x00f2_b6b2, 0x00f2_b6c3];
    let guest = |m: &mut dyn Mem, c: &mut Cpu, target: u32, back: u32, x: F| -> F {
        c.xmm[0] = [x.to_bits(), 0, 0, 0];
        call_from(m, c, pg, frame, 0, &[], back, |m, c| crate::semantic::call_guest(m, c, target));
        F::from_bits(c.xmm[0][0])
    };
    let mut sin = [ZERO; 3];
    let mut cos = [ZERO; 3];
    for k in 0..3 {
        sin[k] = sinf(&*m, pg, angles[k].0).map(F).unwrap_or_else(|| guest(m, c, CRT_SINF, SIN_BACK[k], angles[k]));
    }
    for k in 0..3 {
        cos[k] = cosf(&*m, pg, angles[k].0).map(F).unwrap_or_else(|| guest(m, c, CRT_COSF, COS_BACK[k], angles[k]));
    }
    (sin, cos)
}

/// The rotation matrix of Euler angles `angles` (radians) in rotation
/// `order` (0..5), row vectors, memory order. The original leaves the
/// matrix unwritten for an order above 5 and goes on to read its stack
/// slot (stale stack contents); so does this.
fn offset_rotation_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, order: u32, angles: [F; 3]) -> Matrix {
    let mut r: Matrix = [ZERO; 16];
    if order > 5 {
        // the nine entries the quaternion extraction reads
        for k in [0usize, 1, 2, 4, 5, 6, 8, 9, 10] {
            r[k] = load(pg, m, frame.wrapping_add(FRAME_ROTATION + 4 * k as u32));
        }
        return r;
    }
    let (s, co) = sin_cos_of_angles(m, c, pg, frame, angles);
    let (sx, sy, sz) = (s[0], s[1], s[2]);
    let (cx, cy, cz) = (co[0], co[1], co[2]);
    match order {
        0 => {
            r[0] = cz * cy;
            r[1] = sz * cy;
            r[2] = -sy;
            r[4] = cz * sy * sx - sz * cx;
            r[5] = sz * sy * sx + cz * cx;
            r[6] = cy * sx;
            r[8] = cz * sy * cx + sz * sx;
            r[9] = sz * sy * cx - cz * sx;
            r[10] = cy * cx;
        }
        1 => {
            let sy_cx = sy * cx;
            r[0] = cz * cy;
            r[1] = sz;
            r[2] = -(cz * sy);
            r[4] = sy * sx - sz * cx * cy;
            r[5] = cz * cx;
            r[6] = sy_cx * sz + cy * sx;
            r[8] = sz * sx * cy + sy_cx;
            r[9] = -(cz * sx);
            r[10] = cy * cx - sy * sx * sz;
        }
        2 => {
            r[0] = cz * cy - sy * sx * sz;
            r[1] = sz * cy + sy * sx * cz;
            r[2] = -(sy * cx);
            r[4] = -(sz * cx);
            r[5] = cz * cx;
            r[6] = sx;
            r[8] = cz * sy + cy * sx * sz;
            r[9] = sz * sy - cy * sx * cz;
            r[10] = cy * cx;
        }
        3 => {
            r[0] = cz * cy;
            r[1] = cy * cx * sz + sy * sx;
            r[2] = sz * cy * sx - sy * cx;
            r[4] = -sz;
            r[5] = cz * cx;
            r[6] = cz * sx;
            r[8] = cz * sy;
            r[9] = sy * cx * sz - cy * sx;
            r[10] = sy * sx * sz + cy * cx;
        }
        4 => {
            r[0] = sy * sx * sz + cz * cy;
            r[1] = sz * cx;
            r[2] = sz * sx * cy - cz * sy;
            r[4] = cz * sx * sy - sz * cy;
            r[5] = cz * cx;
            r[6] = cy * sx * cz + sz * sy;
            r[8] = sy * cx;
            r[9] = -sx;
            r[10] = cy * cx;
        }
        _ => {
            r[0] = cz * cy;
            r[1] = cz * sy * sx + sz * cx;
            r[2] = sz * sx - cz * cx * sy;
            r[4] = -(sz * cy);
            r[5] = cz * cx - sy * sx * sz;
            r[6] = sy * cx * sz + cz * sx;
            r[8] = sy;
            r[9] = -(cy * sx);
            r[10] = cy * cx;
        }
    }
    r
}

// ---------------------------------------------------------------------
// 2. the reference matrix and its inverse
// ---------------------------------------------------------------------

/// The reference joint's world matrix: the model's default one (a guest
/// call, argument -1) for index 0xff, else the matrix in the model's joint
/// record `index`.
fn reference_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, joint_ptr: u32, model_ptr: u32) -> Matrix {
    let index = pg.u8(m, joint_ptr.wrapping_add(joint::REFERENCE_INDEX)) as u32;
    let address = if index == 0xff {
        call_from(m, c, pg, frame, model_ptr, &[0xffff_ffff], 0x00f2_bf47, |m, c| {
            let vtable = pg.u32(m, model_ptr);
            let method = pg.u32(m, vtable.wrapping_add(model::DEFAULT_REFERENCE_SLOT));
            crate::semantic::call_guest(m, c, method)
        })
    } else {
        let joints = pg.u32(m, model_ptr.wrapping_add(model::JOINTS));
        joints.wrapping_add(index.wrapping_mul(model::JOINT_STRIDE)).wrapping_add(model::JOINT_MATRIX)
    };
    std::array::from_fn(|k| load(pg, m, address.wrapping_add(4 * k as u32)))
}

/// The identity matrix, the original's answer for a singular reference.
const IDENTITY: Matrix = [ONE, ZERO, ZERO, ZERO, ZERO, ONE, ZERO, ZERO, ZERO, ZERO, ONE, ZERO, ZERO, ZERO, ZERO, ONE];

/// The inverse of `a` by cofactors (the expansion, term order and
/// association of the original, which differ from a textbook one), or the
/// identity when the determinant is exactly zero.
fn invert_reference(a: &Matrix) -> Matrix {
    let [m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, m10, m11, m12, m13, m14, m15] = *a;
    // cofactors of the first column (the original nests the three-factor
    // products differently from term to term; each is as the original's)
    let cof0 = (((((m15 * (m10 * m5)) + (m13 * (m11 * m6))) + (m14 * (m9 * m7))) - (m13 * (m7 * m10))) - (m5 * (m14 * m11))) - (m9 * (m6 * m15));
    let cof1 = -((((((m15 * (m10 * m1)) + (m13 * (m11 * m2))) + (m14 * (m9 * m3))) - (m13 * (m10 * m3))) - (m1 * (m14 * m11))) - (m9 * (m15 * m2)));
    let cof2 = (((((m15 * (m6 * m1)) + (m13 * (m7 * m2))) + (m14 * (m5 * m3))) - (m13 * (m6 * m3))) - ((m14 * m7) * m1)) - (m5 * (m15 * m2));
    let cof3 = -((((((m11 * (m6 * m1)) + (m9 * (m7 * m2))) + (m10 * (m5 * m3))) - (m9 * (m6 * m3))) - (m1 * (m7 * m10))) - (m5 * (m11 * m2)));
    let determinant = (((cof0 * m0) + (cof1 * m4)) + (cof2 * m8)) + (cof3 * m12);
    if determinant == ZERO {
        return IDENTITY;
    }
    let d = ONE / determinant;
    let i0 = d * cof0;
    let i1 = d * cof1;
    let i2 = d * cof2;
    let i3 = d * cof3;
    let i4 = -((((((((m10 * m4) * m15) + (m12 * (m11 * m6))) + ((m7 * m8) * m14)) - (m12 * (m7 * m10))) - (m4 * (m14 * m11))) - (m8 * (m6 * m15))) * d);
    let i5 = (((((((m0 * m10) * m15) + (m12 * (m11 * m2))) + ((m8 * m3) * m14)) - (m12 * (m10 * m3))) - (m0 * (m14 * m11))) - (m8 * (m15 * m2))) * d;
    let i6 = -((((((((m0 * m6) * m15) + (m12 * (m7 * m2))) + ((m4 * m3) * m14)) - ((m6 * m3) * m12)) - (m0 * (m14 * m7))) - (m4 * (m15 * m2))) * d);
    let i7 = (((((((m0 * m6) * m11) + (m8 * (m7 * m2))) + ((m4 * m3) * m10)) - ((m6 * m3) * m8)) - (m0 * (m7 * m10))) - (m4 * (m11 * m2))) * d;
    let i8 = (((((((m11 * m5) * m12) + ((m9 * m4) * m15)) + ((m7 * m8) * m13)) - (m12 * (m9 * m7))) - ((m13 * m11) * m4)) - ((m15 * m5) * m8)) * d;
    let i9 = -((((((((m0 * m9) * m15) + ((m11 * m1) * m12)) + ((m8 * m3) * m13)) - (m12 * (m9 * m3))) - ((m13 * m11) * m0)) - ((m15 * m1) * m8)) * d);
    let i10 = (((((((m0 * m5) * m15) + ((m7 * m1) * m12)) + ((m4 * m3) * m13)) - ((m5 * m3) * m12)) - ((m7 * m13) * m0)) - ((m15 * m1) * m4)) * d;
    let i11 = -((((((((m0 * m5) * m11) + ((m7 * m1) * m8)) + ((m4 * m3) * m9)) - ((m5 * m3) * m8)) - (m0 * (m9 * m7))) - ((m11 * m1) * m4)) * d);
    let i12 = -((((((((m9 * m4) * m14) + (m12 * (m10 * m5))) + ((m6 * m8) * m13)) - ((m9 * m6) * m12)) - ((m13 * m10) * m4)) - ((m14 * m5) * m8)) * d);
    let i13 = (((((((m0 * m9) * m14) + (m12 * (m10 * m1))) + ((m8 * m2) * m13)) - ((m9 * m2) * m12)) - ((m13 * m10) * m0)) - ((m14 * m1) * m8)) * d;
    let i14 = -((((((((m0 * m5) * m14) + (m12 * (m6 * m1))) + ((m4 * m2) * m13)) - ((m5 * m2) * m12)) - ((m13 * m6) * m0)) - ((m14 * m1) * m4)) * d);
    let i15 = (((((((m0 * m5) * m10) + (m8 * (m6 * m1))) + ((m4 * m2) * m9)) - ((m5 * m2) * m8)) - ((m9 * m6) * m0)) - (m4 * (m10 * m1))) * d;
    [i0, i1, i2, i3, i4, i5, i6, i7, i8, i9, i10, i11, i12, i13, i14, i15]
}

// ---------------------------------------------------------------------
// 3. the joint matrix in the reference space
// ---------------------------------------------------------------------

/// `((a * x0 + b * x1) + c * x2) + d * x3`, the four-term sums of the
/// matrix products: `terms` are the products in the original's order.
#[inline(always)]
fn sum4(t0: F, t1: F, t2: F, t3: F) -> F {
    t0 + t1 + t2 + t3
}

/// One element of `row * a` (column `col`) in the original's term and
/// operand orders: `first_term_zero` sums the row's element 0 first (else
/// element 1 first); `matrix_first` writes each product matrix element
/// first (it only matters when both operands are NaN).
#[inline(always)]
fn row_times_column(row: [F; 4], a: &Matrix, col: usize, first_term_zero: bool, matrix_first: bool) -> F {
    let term = |k: usize| if matrix_first { a[4 * k + col] * row[k] } else { row[k] * a[4 * k + col] };
    if first_term_zero {
        sum4(term(0), term(1), term(2), term(3))
    } else {
        sum4(term(1), term(0), term(2), term(3))
    }
}

/// `joint * inverse`: the constrained joint's matrix in the reference
/// joint's space. Column 0 of the third and fourth rows sums its terms in
/// element order, every other element sums element 1 first.
fn into_reference_space(joint: &Matrix, inverse: &Matrix) -> Matrix {
    let mut out = [ZERO; 16];
    for row in 0..4 {
        let r = [joint[4 * row], joint[4 * row + 1], joint[4 * row + 2], joint[4 * row + 3]];
        for col in 0..4 {
            out[4 * row + col] = row_times_column(r, inverse, col, row >= 2 && col == 0, row >= 2);
        }
    }
    out
}

/// The joint split into unit axes, lengths and a translation.
struct SplitAxes {
    /// the three axes, normalised, as rows of four
    axes: [[F; 4]; 3],
    /// the translation row after adding the weighted offset
    translation: [F; 4],
    /// the blended axis lengths
    scales: [F; 3],
}

#[inline(always)]
fn square_sum4(v: [F; 4]) -> F {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2] + v[3] * v[3]
}

/// Splits `local` (see [`into_reference_space`]) into axes, lengths and
/// translation, blending the lengths towards `axis_scale * length` and
/// moving the translation by `offset`, both by `weight`.
fn split_axes(local: &Matrix, weight: F, axis_scale: [F; 3], offset: [F; 3]) -> SplitAxes {
    let rows: [[F; 4]; 3] = std::array::from_fn(|r| [local[4 * r], local[4 * r + 1], local[4 * r + 2], local[4 * r + 3]]);
    let lengths = rows.map(|row| square_sum4(row).sqrt());
    let remaining = ONE - weight;
    let scales: [F; 3] = std::array::from_fn(|k| axis_scale[k] * lengths[k] * weight + lengths[k] * remaining);
    // normalise each axis; the original's operand order differs per element
    let axes: [[F; 4]; 3] = std::array::from_fn(|k| {
        let inverse_length = ONE / lengths[k];
        std::array::from_fn(|col| {
            let v = rows[k][col];
            if k == 2 || (k == 1 && col == 3) { v * inverse_length } else { inverse_length * v }
        })
    });
    let translation = [
        local[12] + offset[0] * weight,
        local[13] + offset[1] * weight,
        local[14] + offset[2] * weight,
        local[15],
    ];
    SplitAxes { axes, translation, scales }
}

// ---------------------------------------------------------------------
// 4. the blended offset rotation
// ---------------------------------------------------------------------

/// The unit quaternion of the rotation part of `r` (a rotation matrix, row
/// vectors): the trace form when the trace is positive, else the form
/// around the largest diagonal element.
fn quaternion_of(r: &Matrix) -> Quaternion {
    let trace = r[5] + r[0] + r[10];
    if trace > ZERO {
        let root = (trace + ONE).sqrt();
        let w = root * HALF;
        let k = HALF / root;
        [(r[6] - r[9]) * k, (r[8] - r[2]) * k, (r[1] - r[4]) * k, w]
    } else {
        const NEXT: [usize; 3] = [1, 2, 0];
        let mut i = 0;
        if r[0] < r[5] {
            i = 1;
        }
        if r[5 * i] < r[10] {
            i = 2;
        }
        let (j, k) = (NEXT[i], NEXT[NEXT[i]]);
        let root = (((r[5 * i] - r[5 * j]) - r[5 * k]) + ONE).sqrt();
        let scale = HALF / root;
        let mut q = [ZERO; 4];
        q[i] = root * HALF;
        q[3] = (r[k + 4 * j] - r[j + 4 * k]) * scale;
        q[j] = (r[i + 4 * j] + r[j + 4 * i]) * scale;
        q[k] = (r[i + 4 * k] + r[k + 4 * i]) * scale;
        q
    }
}

/// The offset rotation of `rotation` blended from the identity by
/// `weight` along the shorter arc: `nlerp(identity, q, weight)`.
fn blended_rotation(rotation: &Matrix, weight: F) -> Quaternion {
    const IDENTITY_QUATERNION: Quaternion = [ZERO, ZERO, ZERO, ONE];
    let q = quaternion_of(rotation);
    let dot = q[0] * IDENTITY_QUATERNION[0] + q[1] * IDENTITY_QUATERNION[1] + q[2] * IDENTITY_QUATERNION[2] + q[3] * IDENTITY_QUATERNION[3];
    // the shorter arc: negate q when it points away from the identity
    let q = if dot < ZERO { q.map(|v| -v) } else { q };
    let lerped: Quaternion = std::array::from_fn(|k| (q[k] - IDENTITY_QUATERNION[k]) * weight + IDENTITY_QUATERNION[k]);
    // normalise: the original sums y, x, z, w squares in that order
    let length = (((lerped[1] * lerped[1] + lerped[0] * lerped[0]) + lerped[2] * lerped[2]) + lerped[3] * lerped[3]).sqrt();
    let inverse = ONE / length;
    lerped.map(|v| inverse * v)
}

/// The rotation matrix (3x3 part, row vectors) of unit quaternion `q`.
fn rotation_matrix_of(q: Quaternion) -> Matrix {
    let [x, y, z, w] = q;
    let (x2, y2, z2) = (x * TWO, y * TWO, z * TWO);
    let xw2 = x2 * w;
    let yw2 = y2 * w;
    let zw2 = z2 * w;
    let xx2 = x * x2;
    let xz2 = x * z2;
    let xy2 = x * y2;
    let yy2 = y2 * y;
    let zz2 = z2 * z;
    let zy2 = z2 * y;
    let mut r = [ZERO; 16];
    r[0] = ONE - (zz2 + yy2);
    r[1] = xy2 + zw2;
    r[2] = xz2 - yw2;
    r[4] = xy2 - zw2;
    r[5] = ONE - (zz2 + xx2);
    r[6] = zy2 + xw2;
    r[8] = xz2 + yw2;
    r[9] = zy2 - xw2;
    r[10] = ONE - (yy2 + xx2);
    r
}

// ---------------------------------------------------------------------
// 5. rotate the axes, go back through the reference matrix
// ---------------------------------------------------------------------

/// Applies the rotation `q` to the unit `axes`, appends the translation and
/// multiplies by the reference matrix: the new joint matrix (before the
/// axis lengths are applied).
fn rotate_and_return(axes: &[[F; 4]; 3], translation: &[F; 4], q: &Matrix, reference: &Matrix) -> Matrix {
    // the original adds `translation * 0` to every rotated element, which
    // keeps NaN/infinity in the translation visible in the result
    let zero = translation.map(|t| t * ZERO);
    let [n0, n1, n2] = *axes;
    let [q0, q1, q2, q4, q5, q6, q8, q9, q10] = [q[0], q[1], q[2], q[4], q[5], q[6], q[8], q[9], q[10]];
    // rotated[row][col]: three terms (axis0, axis1, axis2 of column `col`)
    // times the rotation's column, in the original's order per row
    let first_row = |col: usize| n0[col] * q0 + n1[col] * q1 + n2[col] * q2 + zero[col];
    let second_row = |col: usize| if col == 0 { q5 * n1[0] + n0[0] * q4 + q6 * n2[0] + zero[0] } else { n1[col] * q5 + n0[col] * q4 + n2[col] * q6 + zero[col] };
    let third_row = |col: usize| if col == 0 { q9 * n1[0] + n0[0] * q8 + q10 * n2[0] + zero[0] } else { n1[col] * q9 + n0[col] * q8 + n2[col] * q10 + zero[col] };
    let mut x = [[ZERO; 4]; 4];
    for col in 0..4 {
        x[0][col] = first_row(col);
        x[1][col] = second_row(col);
        x[2][col] = third_row(col);
        // the translation row: a zero multiple of the axis sums, plus the
        // translation (column 0 sums axis 0 first, the others axis 1 first)
        let axis_sum = if col == 0 { (n0[0] + n1[0]) + n2[0] } else { (n1[col] + n0[col]) + n2[col] };
        x[3][col] = axis_sum * ZERO + translation[col];
    }
    let mut out = [ZERO; 16];
    for row in 0..4 {
        for col in 0..4 {
            out[4 * row + col] = row_times_column(x[row], reference, col, col == 0, false);
        }
    }
    out
}
