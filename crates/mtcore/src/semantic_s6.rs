//! L184 s6: semantic port of the motion filter's per-joint update.
//!
//! `uCnsMotFilter::vf24` (0x00872420) is the "constraint motion filter" of
//! the model update (reached from the joint update of `uModel::vf28`,
//! 0x01054860, and from the per-unit update callbacks 0x00B61280 and
//! 0x00B66110). A filter samples a *pose matrix* for one joint, weakens it
//! toward the rest pose by the filter's weight, and folds it into the
//! joint's local matrix.
//!
//! The port keeps the original's every observable effect (the 16 floats
//! written to `joint + 0x10`, and the guest calls in the original's order
//! with the original's registers and stack frames) but replaces the 1,821
//! instructions by plain Rust: the same IEEE single-precision operations
//! in the same order (the original is scalar SSE; a different association
//! of a sum changes the low bits), without the spills.
//!
//! Filter record (`this`): `+0x44` weight (f32), `+0x61` enabled (byte),
//! `+0x68` pose kind (1..=20, see [`PoseBuilder`]), `+0x6c` space flag
//! (0: the pose acts in the joint's own space; else in the parent's),
//! `+0x70` pose source, `+0x74` suppressed (byte).
//! Joint record: `+0x10` local matrix (16 f32, row-major), `+0x91` parent
//! joint index (0xff: the model's root). Model: `+0x364` joint array
//! (0xA0-byte records, world matrix at `+0x10`), vtable slot `+0x58` =
//! `root_matrix(-1)`.
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

use crate::semantic::call_from;
use crate::semantic_s6_pose::{build_pose, crt_f32_at, has_pose_routine, PoseCall};

/// A 4x4 float matrix, row-major (element `4 * row + column`), as the
/// game stores them.
pub type Mat4 = [f32; 16];

/// FLT_EPSILON: a filter whose weight does not exceed it does nothing.
const MIN_WEIGHT: f32 = 1.192_092_9e-7;
/// The rest pose the weight blends toward: the identity rotation
/// (x, y, z, w), the constant at 0x016296B0.
const REST_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The slerp is used while the cosine between rotations stays below this
/// (0x0161C9E4); closer rotations are blended linearly.
const SLERP_LIMIT: f32 = 0.999_999_88;
/// The model's root matrix, asked through vtable slot 0x58 (`root_matrix`)
/// when a joint has no parent.
const NO_PARENT: u8 = 0xff;
/// Size of a joint record of the model's joint array.
const JOINT_STRIDE: u32 = 0xa0;

/// 0x00872420, `filter->apply(joint, model)` (thiscall, `ret 8`): update
/// one joint's local matrix from the filter.
///
/// Nothing happens unless the filter is enabled (byte `+0x61`), not
/// suppressed (byte `+0x74`), has a pose source (`+0x70`), a weight above
/// [`MIN_WEIGHT`] and a pose kind with a builder. The pose matrix is built
/// by a guest call, blended toward the identity rotation when the weight
/// is below 1 ([`blend_toward_rest`]), then multiplied into the joint's
/// local matrix: `pose * local` (space flag 0) or
/// `local * parent⁻¹ * pose * parent` (otherwise), with `parent` the
/// parent joint's world matrix or the model's root matrix
/// ([`fold_into_parent_space`]). A parent matrix that is not invertible
/// counts as the identity. The joint's matrix is written once, at the end.
///
/// The function returns nothing (EAX is left as it was by the last call).
pub fn motion_filter_apply(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let filter = c.ecx;
    let f32_at = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
    let done = entry.wrapping_add(4 + 8); // ret 8
    if pg.u8(m, filter.wrapping_add(0x61)) == 0 || pg.u8(m, filter.wrapping_add(0x74)) != 0 {
        c.esp = done;
        return;
    }
    let source = pg.u32(m, filter.wrapping_add(0x70));
    // Skipped when the weight is at most the epsilon; an unordered
    // (NaN) weight is not skipped.
    if source == 0 || MIN_WEIGHT >= f32_at(m, filter.wrapping_add(0x44)) {
        c.esp = done;
        return;
    }
    if !has_pose_routine(pg.u32(m, filter.wrapping_add(0x68))) {
        c.esp = done;
        return;
    }
    let joint = pg.u32(m, entry.wrapping_add(4));
    let model = pg.u32(m, entry.wrapping_add(8));
    // The original's frame: EBP = entry - 4, aligned down to 16, 0x374
    // bytes of locals, then EBX, ESI and EDI pushed.
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x374 + 12);
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| ddnative_base::x86::call_ext(m, c, target);
    // The callee-saved registers the original holds during its calls.
    c.ebx = joint;
    c.ebp = entry.wrapping_sub(4);
    c.edi = source;
    c.esi = filter;

    // 1. the pose matrix of the filter's kind (the original's calls of the
    //    pose routines are folded into build_pose)
    let kind = pg.u32(m, filter.wrapping_add(0x68));
    let call = PoseCall { pages: pg, joint, source, model, frame };
    let mut pose = build_pose(m, c, &call, kind);
    c.ebp = entry.wrapping_sub(4);

    // 2. weaker than full weight: pull the pose toward the rest pose
    let weight = f32_at(m, filter.wrapping_add(0x44));
    if weight < 1.0 {
        pose = blend_toward_rest(m, c, pg, frame, &pose, weight);
    }

    // 3. into the joint's local matrix
    let local = read_mat4(m, pg, joint.wrapping_add(0x10));
    let result = if pg.u32(m, filter.wrapping_add(0x6c)) == 0 {
        mul4(&pose, &local)
    } else {
        let parent = match pg.u8(m, joint.wrapping_add(0x91)) {
            NO_PARENT => {
                // model->root_matrix(-1)
                let method = pg.u32(m, pg.u32(m, model).wrapping_add(0x58));
                let at = call_from(m, c, pg, frame, model, &[u32::MAX], 0x0087_2fca, guest(method));
                read_mat4(m, pg, at)
            }
            index => {
                let joints = pg.u32(m, model.wrapping_add(0x364));
                read_mat4(m, pg, joints.wrapping_add(index as u32 * JOINT_STRIDE).wrapping_add(0x10))
            }
        };
        fold_into_parent_space(&local, &pose, &parent)
    };
    for (i, v) in result.iter().enumerate() {
        pg.set_u32(m, joint.wrapping_add(0x10 + 4 * i as u32), v.to_bits());
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = done;
}

#[inline(always)]
fn read_mat4(m: &dyn Mem, pg: Pages, at: u32) -> Mat4 {
    std::array::from_fn(|i| f32::from_bits(pg.u32(m, at.wrapping_add(4 * i as u32))))
}

/// `a * b` as the original sums it: element (r, c) is
/// `((a[r][0] b[0][c] + a[r][1] b[1][c]) + a[r][2] b[2][c]) + a[r][3] b[3][c]`.
#[inline(always)]
pub fn mul4(a: &Mat4, b: &Mat4) -> Mat4 {
    std::array::from_fn(|i| {
        let (r, c) = (i & !3, i & 3);
        (((a[r] * b[c]) + (a[r + 1] * b[4 + c])) + (a[r + 2] * b[8 + c])) + (a[r + 3] * b[12 + c])
    })
}

/// `local * parent⁻¹ * pose * parent`: the pose applied in the parent's
/// space. The three products are taken left to right.
pub fn fold_into_parent_space(local: &Mat4, pose: &Mat4, parent: &Mat4) -> Mat4 {
    let inverse = invert4(parent).unwrap_or(IDENTITY);
    mul4(&mul4(&mul4(local, &inverse), pose), parent)
}

const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// Slerp the pose's rotation from the rest rotation toward itself by
/// `weight` (< 1) and rebuild a pure rotation matrix: the translation row
/// and the last column are dropped. Rotations closer than [`SLERP_LIMIT`]
/// are blended linearly; farther ones use the CRT's acosf (0x012F161F) and
/// sinf (0x012F0906), all native and bit-exact (acosf from
/// [`crate::semantic_s7_math`], sinf from [`crate::semantic_s7_trig`]; the
/// guest function only runs for large finite arguments of sinf).
fn blend_toward_rest(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, pose: &Mat4, weight: f32) -> Mat4 {
    let [qx, qy, qz, qw] = rotation_to_quaternion(pose);
    let [rx, ry, rz, rw] = REST_ROTATION;
    // cosine between the rest rotation and the pose's; the shorter arc
    let mut cosine = (((qx * rx) + (qy * ry)) + (qz * rz)) + (qw * rw);
    let mut side = 1.0f32;
    if 0.0 > cosine {
        side = -1.0;
        cosine = -cosine;
    }
    let (rest_weight, pose_weight) = if SLERP_LIMIT > cosine {
        let call = |m: &mut dyn Mem, c: &mut Cpu, target: u32, back: u32, x: f32| crt_f32_at(m, c, pg, frame, target, back, x);
        let angle = crate::semantic_s7_math::acosf(m, pg, cosine);
        let sin_angle = call(m, c, 0x012f_0906, 0x0087_2844, angle);
        let scale = 1.0 / sin_angle;
        let rest = call(m, c, 0x012f_0906, 0x0087_286d, (1.0 - weight) * angle) * scale;
        let toward = call(m, c, 0x012f_0906, 0x0087_288a, angle * weight) * scale;
        (rest, toward)
    } else {
        (1.0 - weight, weight)
    };
    let rest_weight = rest_weight * side;
    let blended = [
        (rest_weight * rx) + (pose_weight * qx),
        (rest_weight * ry) + (pose_weight * qy),
        (rest_weight * rz) + (pose_weight * qz),
        (rest_weight * rw) + (pose_weight * qw),
    ];
    quaternion_to_rotation(blended)
}

/// The rotation part of `m` as a quaternion (x, y, z, w): the standard
/// trace method, picking the largest diagonal element when the trace is
/// not positive.
pub fn rotation_to_quaternion(m: &Mat4) -> [f32; 4] {
    let trace = (m[5] + m[0]) + m[10];
    if trace > 0.0 {
        let root = (trace + 1.0).sqrt();
        let scale = 0.5 / root;
        return [(m[6] - m[9]) * scale, (m[8] - m[2]) * scale, (m[1] - m[4]) * scale, root * 0.5];
    }
    const NEXT: [usize; 3] = [1, 2, 0];
    let mut i = 0;
    if m[5] > m[0] {
        i = 1;
    }
    if m[10] > m[5 * i] {
        i = 2;
    }
    let (j, k) = (NEXT[i], NEXT[NEXT[i]]);
    let root = (((m[5 * i] - m[5 * j]) - m[5 * k]) + 1.0).sqrt();
    let scale = 0.5 / root;
    let mut q = [0.0f32; 4];
    q[i] = root * 0.5;
    q[3] = (m[4 * j + k] - m[4 * k + j]) * scale;
    q[j] = (m[4 * j + i] + m[4 * i + j]) * scale;
    q[k] = (m[4 * k + i] + m[4 * i + k]) * scale;
    q
}

/// The rotation matrix of the quaternion (x, y, z, w), in the layout the
/// filter uses (row-major, last row and column those of the identity).
/// The quaternion is not normalised.
pub fn quaternion_to_rotation([x, y, z, w]: [f32; 4]) -> Mat4 {
    let (x2, y2, z2) = (x * 2.0, y * 2.0, z * 2.0);
    [
        1.0 - ((z2 * z) + (y2 * y)),
        (x * y2) + (z2 * w),
        (x * z2) - (y2 * w),
        0.0,
        (x * y2) - (z2 * w),
        1.0 - ((z2 * z) + (x * x2)),
        (z2 * y) + (x2 * w),
        0.0,
        (x * z2) + (y2 * w),
        (z2 * y) - (x2 * w),
        1.0 - ((y2 * y) + (x * x2)),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// The six products of a 3x3 minor of a 4x4 matrix, as the original
/// computes them: `(m[a] * m[b]) * m[c]` for each `[a, b, c]` (the
/// compiler paired the factors differently in each term, which matters
/// for the last bits). The first three are added, the other three
/// subtracted, left to right.
type MinorTerms = [[u8; 3]; 6];

/// Entry `k` is the minor behind element `k` of the inverse.
const MINORS: [MinorTerms; 16] = [
    [[11, 6, 13], [10, 5, 15], [9, 7, 14], [7, 10, 13], [14, 11, 5], [6, 15, 9]],
    [[11, 2, 13], [10, 1, 15], [9, 3, 14], [10, 3, 13], [14, 11, 1], [15, 2, 9]],
    [[6, 1, 15], [7, 2, 13], [5, 3, 14], [6, 3, 13], [14, 7, 1], [15, 2, 5]],
    [[6, 1, 11], [7, 2, 9], [5, 3, 10], [6, 3, 9], [7, 10, 1], [11, 2, 5]],
    [[10, 4, 15], [11, 6, 12], [7, 8, 14], [7, 10, 12], [14, 11, 4], [6, 15, 8]],
    [[0, 10, 15], [11, 2, 12], [8, 3, 14], [10, 3, 12], [14, 11, 0], [15, 2, 8]],
    [[0, 6, 15], [7, 2, 12], [4, 3, 14], [6, 3, 12], [14, 7, 0], [15, 2, 4]],
    [[0, 6, 11], [7, 2, 8], [4, 3, 10], [6, 3, 8], [7, 10, 0], [11, 2, 4]],
    [[11, 5, 12], [9, 4, 15], [7, 8, 13], [9, 7, 12], [13, 11, 4], [15, 5, 8]],
    [[0, 9, 15], [11, 1, 12], [8, 3, 13], [9, 3, 12], [13, 11, 0], [15, 1, 8]],
    [[0, 5, 15], [7, 1, 12], [4, 3, 13], [5, 3, 12], [7, 13, 0], [15, 1, 4]],
    [[0, 5, 11], [7, 1, 8], [4, 3, 9], [5, 3, 8], [9, 7, 0], [11, 1, 4]],
    [[9, 4, 14], [10, 5, 12], [6, 8, 13], [9, 6, 12], [13, 10, 4], [14, 5, 8]],
    [[0, 9, 14], [10, 1, 12], [8, 2, 13], [9, 2, 12], [13, 10, 0], [14, 1, 8]],
    [[0, 5, 14], [6, 1, 12], [4, 2, 13], [5, 2, 12], [13, 6, 0], [14, 1, 4]],
    [[0, 5, 10], [6, 1, 8], [4, 2, 9], [5, 2, 8], [9, 6, 0], [10, 1, 4]],
];

#[inline(always)]
fn minor(m: &Mat4, terms: &MinorTerms) -> f32 {
    let product = |t: &[u8; 3]| (m[t[0] as usize] * m[t[1] as usize]) * m[t[2] as usize];
    ((((product(&terms[0]) + product(&terms[1])) + product(&terms[2])) - product(&terms[3])) - product(&terms[4])) - product(&terms[5])
}

/// The inverse of a general 4x4 matrix by cofactors (the adjugate over the
/// determinant), or `None` when the determinant compares equal to zero (a
/// NaN determinant is not zero and gives a NaN inverse).
///
/// The determinant expands along the first column, `m0 c0 + m4 c4 + m8 c8
/// + m12 c12`, and every inverse element is its cofactor times
/// `1 / determinant`.
pub fn invert4(m: &Mat4) -> Option<Mat4> {
    let minors: [f32; 16] = std::array::from_fn(|k| minor(m, &MINORS[k]));
    let det = (((minors[0] * m[0]) + ((-minors[1]) * m[4])) + (minors[2] * m[8])) + ((-minors[3]) * m[12]);
    if det == 0.0 {
        return None;
    }
    let inverse_det = 1.0 / det;
    Some(std::array::from_fn(|k| {
        // The checkerboard of signs: an odd (row + column) is negated. The
        // original multiplies the first row's cofactors from the left with
        // the sign already applied and the others negated after the
        // product; the values are the same, only a NaN's sign could differ.
        let negative = (k ^ (k >> 2)) & 1 != 0;
        if k < 4 {
            inverse_det * if negative { -minors[k] } else { minors[k] }
        } else {
            let scaled = minors[k] * inverse_det;
            if negative { -scaled } else { scaled }
        }
    }))
}
