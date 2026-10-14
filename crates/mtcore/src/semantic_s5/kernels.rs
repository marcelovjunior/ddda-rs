//! Float kernels of the uCnsEdit matrix providers (0x00864660, ...), generated
//! from the original's SSE instructions by research/native/cns_kernels/ (a
//! symbolic executor that follows the data flow of every float operation and
//! merges both arms of each float compare). Every operation is kept in the
//! original's order and grouping, including the multiplications by 0.0 whose
//! NaN/infinity propagation the original has, so results are bit-identical.
//! Do not edit by hand: regenerate.
#![allow(clippy::all, unused_parens, dead_code, non_upper_case_globals)]
use super::{read_f32 as rd, Bases, Matrix};
use ddnative_base::mem::{Mem, Pages};

/// The x87 load/store pair the original uses on some results: a signalling NaN comes back quiet.
fn quiet(x: f32) -> f32 {
    if x.is_nan() { f32::from_bits(x.to_bits() | 0x0040_0000) } else { x }
}

const K_14F0E9C: f32 = f32::from_bits(0x3f000000);
const K_14F9E04: f32 = f32::from_bits(0x40000000);
const K_150A350: f32 = f32::from_bits(0x34000000);
const K_1521690: f32 = f32::from_bits(0x00000000);
const K_16295C0: f32 = f32::from_bits(0x3f800000);
const K_16295C4: f32 = f32::from_bits(0x00000000);
const K_16295C8: f32 = f32::from_bits(0x00000000);
const K_16295CC: f32 = f32::from_bits(0x00000000);
const K_16295D0: f32 = f32::from_bits(0x00000000);
const K_16295D4: f32 = f32::from_bits(0x3f800000);
const K_16295D8: f32 = f32::from_bits(0x00000000);
const K_16295DC: f32 = f32::from_bits(0x00000000);
const K_16295E0: f32 = f32::from_bits(0x00000000);
const K_16295E4: f32 = f32::from_bits(0x00000000);
const K_16295E8: f32 = f32::from_bits(0x3f800000);
const K_16295EC: f32 = f32::from_bits(0x00000000);
const K_16295F0: f32 = f32::from_bits(0x00000000);
const K_16295F4: f32 = f32::from_bits(0x00000000);
const K_16295F8: f32 = f32::from_bits(0x00000000);
const K_16295FC: f32 = f32::from_bits(0x3f800000);


/// Inputs of `kernel_m6_aim`, in order (child/joint record fields, the matrix being built, the angle).
pub(super) fn load_m6_aim(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 20] {
    let _ = (m, pg, b, s);
    [
        rd(m, pg, b.child + 68),
        rd(m, pg, b.joint + 68),
        rd(m, pg, b.child + 64),
        rd(m, pg, b.joint + 64),
        rd(m, pg, b.child + 72),
        rd(m, pg, b.joint + 72),
        rd(m, pg, b.joint + 76),
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 60),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 44),
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 28),
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 48),
        b.angle,
    ]
}

/// Kernel of 0x00864660 `m6_aim` (0x864779..0x86527c): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m6_aim`; output: the 16 floats of the matrix.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m6_aim(l: &[f32; 20], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (l[0] - l[1]);
    let v2: f32 = (l[2] - l[3]);
    let v3: f32 = (l[4] - l[5]);
    let v4: f32 = (((v1 * v1) + (v2 * v2)) + (v3 * v3)).sqrt();
    let c5: bool = (!(K_150A350 > v4));
    let v6: f32 = (1.0_f32 / v4);
    let v7: f32 = (if c5 { (v2 * v6) } else { v2 });
    let v8: f32 = (l[9] * l[10]);
    let v9: f32 = (l[11] * l[12]);
    let v10: f32 = (l[12] * l[8]);
    let v11: f32 = (l[5] * l[9]);
    let v12: f32 = (l[6] * l[10]);
    let v13: f32 = ((((((l[6] * (l[7] * l[8])) + (l[1] * v8)) + (l[5] * v9)) - (l[1] * v10)) - (l[7] * v11)) - (l[11] * v12));
    let v14: f32 = (l[9] * l[15]);
    let v15: f32 = (l[11] * l[16]);
    let v16: f32 = (l[8] * l[16]);
    let v17: f32 = (l[6] * l[15]);
    let v18: f32 = (-((((((l[6] * (l[8] * l[14])) + (l[1] * v14)) + (l[5] * v15)) - (l[1] * v16)) - (v11 * l[14])) - (l[11] * v17)));
    let v19: f32 = (l[7] * l[16]);
    let v20: f32 = (l[10] * l[14]);
    let v21: f32 = (l[12] * l[15]);
    let v22: f32 = (l[10] * l[16]);
    let v23: f32 = (l[5] * l[12]);
    let v24: f32 = (((((l[5] * v19) + ((l[6] * v20) + (l[1] * v21))) - (l[1] * v22)) - (l[14] * v23)) - (l[7] * v17));
    let v25: f32 = ((((v13 * l[13]) + (v18 * l[17])) + (v24 * l[18])) + (l[3] * (-((((((l[11] * v21) + (l[9] * v20)) + (l[8] * v19)) - (l[11] * v22)) - (v10 * l[14])) - (l[7] * v14)))));
    let c26: bool = (!(v25 == K_1521690));
    let v27: f32 = (1.0_f32 / v25);
    let v28: f32 = (if c26 { (v13 * v27) } else { K_16295C0 });
    let v29: f32 = (if c5 { (v1 * v6) } else { v1 });
    let v30: f32 = (l[12] * l[18]);
    let v31: f32 = (if c26 { (-(v27 * (((((l[5] * v30) + ((l[6] * (l[8] * l[17])) + (l[3] * v8))) - (l[3] * v10)) - (v11 * l[17])) - (v12 * l[18])))) } else { K_16295D0 });
    let v32: f32 = (if c5 { (v3 * v6) } else { v3 });
    let v33: f32 = (l[1] * l[9]);
    let v34: f32 = (if c26 { (v27 * ((((((l[6] * (l[11] * l[17])) + (l[3] * (l[9] * l[7]))) + (l[1] * v30)) - (l[3] * v9)) - (l[17] * v33)) - (l[18] * (l[6] * l[7])))) } else { K_16295E0 });
    let v35: f32 = (((v7 * v28) + (v29 * v31)) + (v32 * v34));
    let v36: f32 = (if c26 { (v18 * v27) } else { K_16295C4 });
    let v37: f32 = (l[16] * l[18]);
    let v38: f32 = (if c26 { (v27 * ((((((l[6] * (l[8] * l[13])) + (l[3] * v14)) + (l[5] * v37)) - (l[3] * v16)) - (v11 * l[13])) - (v17 * l[18]))) } else { K_16295D4 });
    let v39: f32 = (l[6] * l[14]);
    let v40: f32 = (if c26 { (-(v27 * ((((((l[6] * (l[11] * l[13])) + (l[3] * (l[9] * l[14]))) + (l[1] * v37)) - (l[3] * v15)) - (l[13] * v33)) - (l[18] * v39)))) } else { K_16295E4 });
    let v41: f32 = (((v7 * v36) + (v29 * v38)) + (v32 * v40));
    let v42: f32 = (if c26 { (v24 * v27) } else { K_16295C8 });
    let v43: f32 = (l[16] * l[17]);
    let v44: f32 = (if c26 { (-(v27 * ((((((l[6] * (l[10] * l[13])) + (l[3] * v21)) + (l[5] * v43)) - (l[3] * v22)) - (v23 * l[13])) - (v17 * l[17])))) } else { K_16295D8 });
    let v45: f32 = (if c26 { (v27 * (((((l[1] * v43) + ((l[6] * (l[7] * l[13])) + (l[3] * (l[12] * l[14])))) - (l[3] * v19)) - (l[13] * (l[1] * l[12]))) - (l[17] * v39))) } else { K_16295E8 });
    let v46: f32 = (((v7 * v42) + (v29 * v44)) + (v32 * v45));
    let v47: f32 = (((v35 * v35) + (v41 * v41)) + (v46 * v46)).sqrt();
    let c48: bool = (!(K_150A350 > v47));
    let v49: f32 = (1.0_f32 / v47);
    let v50: f32 = (if c48 { (v41 * v49) } else { v41 });
    let v51: f32 = (l[19] * K_14F0E9C);
    let v52: f32 = trig(false, v51);
    let v53: f32 = (v50 * v52);
    let v54: f32 = (v53 * v53);
    let v55: f32 = (if c48 { (v46 * v49) } else { v46 });
    let v56: f32 = (v55 * v52);
    let v57: f32 = (v56 * v56);
    let v58: f32 = (if c48 { (v35 * v49) } else { v35 });
    let v59: f32 = (v58 * v52);
    let v60: f32 = (v59 * v53);
    let v61: f32 = trig(true, v51);
    let v62: f32 = (v56 * v61);
    let v63: f32 = (v59 * v56);
    let v64: f32 = (v53 * v61);
    let v65: f32 = (v59 * v59);
    let v66: f32 = (v56 * v53);
    let v67: f32 = (v59 * v61);
    [(1.0_f32 - (K_14F9E04 * (v54 + v57))), (K_14F9E04 * (v60 + v62)), (K_14F9E04 * (v63 - v64)), 0.0_f32, (K_14F9E04 * (v60 - v62)), (1.0_f32 - (K_14F9E04 * (v57 + v65))), (K_14F9E04 * (v66 + v67)), 0.0_f32, (K_14F9E04 * (v63 + v64)), (K_14F9E04 * (v66 - v67)), (1.0_f32 - (K_14F9E04 * (v54 + v65))), 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32]
}

/// Inputs of `kernel_m6_pitch`, in order (child/joint record fields, the matrix being built, the angle).
pub(super) fn load_m6_pitch(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 17] {
    let _ = (m, pg, b, s);
    [
        s[12],
        s[8],
        b.angle,
        s[4],
        s[0],
        s[13],
        s[5],
        s[1],
        s[9],
        s[14],
        s[6],
        s[2],
        s[10],
        s[15],
        s[7],
        s[3],
        s[11],
    ]
}

/// Kernel of 0x00864660 `m6_pitch` (0x86528e..0x8655f6): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m6_pitch`; output: the 16 floats of the matrix.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m6_pitch(l: &[f32; 17], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (0.0_f32 * l[0]);
    let v2: f32 = trig(false, l[2]);
    let v3: f32 = (-v2);
    let v4: f32 = (0.0_f32 * l[3]);
    let v5: f32 = trig(true, l[2]);
    let v6: f32 = (0.0_f32 * l[5]);
    let v7: f32 = (0.0_f32 * l[6]);
    let v8: f32 = (0.0_f32 * l[9]);
    let v9: f32 = (0.0_f32 * l[10]);
    let v10: f32 = (0.0_f32 * l[13]);
    let v11: f32 = (0.0_f32 * l[14]);
    let v12: f32 = (0.0_f32 * l[1]);
    let v13: f32 = (0.0_f32 * l[4]);
    let v14: f32 = (0.0_f32 * l[8]);
    let v15: f32 = (0.0_f32 * l[7]);
    let v16: f32 = (0.0_f32 * l[12]);
    let v17: f32 = (0.0_f32 * l[11]);
    let v18: f32 = (0.0_f32 * l[16]);
    let v19: f32 = (0.0_f32 * l[15]);
    [(v1 + ((l[1] * v3) + (v4 + (v5 * l[4])))), (v6 + ((v7 + (v5 * l[7])) + (v3 * l[8]))), (v8 + ((v9 + (v5 * l[11])) + (v3 * l[12]))), (v10 + ((v11 + (v5 * l[15])) + (v3 * l[16]))), (v1 + (v12 + (l[3] + v13))), (v6 + (v14 + (l[6] + v15))), (v8 + (v16 + (l[10] + v17))), (v10 + (v18 + (l[14] + v19))), (v1 + ((v5 * l[1]) + (v4 + (v2 * l[4])))), (v6 + ((v5 * l[8]) + (v7 + (v2 * l[7])))), (v8 + ((v5 * l[12]) + (v9 + (v2 * l[11])))), (v10 + ((v5 * l[16]) + (v11 + (v2 * l[15])))), (l[0] + (v12 + (v4 + v13))), (l[5] + (v14 + (v7 + v15))), (l[9] + (v16 + (v9 + v17))), (l[13] + (v18 + (v11 + v19)))]
}

/// Inputs of `kernel_m6_roll`, in order (child/joint record fields, the matrix being built, the angle).
pub(super) fn load_m6_roll(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 17] {
    let _ = (m, pg, b, s);
    [
        s[12],
        b.angle,
        s[4],
        s[0],
        s[8],
        s[13],
        s[9],
        s[1],
        s[5],
        s[14],
        s[10],
        s[6],
        s[2],
        s[15],
        s[11],
        s[7],
        s[3],
    ]
}

/// Kernel of 0x00864660 `m6_roll` (0x865608..0x86597d): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m6_roll`; output: the 16 floats of the matrix.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m6_roll(l: &[f32; 17], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (0.0_f32 * l[0]);
    let v2: f32 = trig(false, l[1]);
    let v3: f32 = trig(true, l[1]);
    let v4: f32 = (0.0_f32 * l[4]);
    let v5: f32 = (0.0_f32 * l[5]);
    let v6: f32 = (0.0_f32 * l[6]);
    let v7: f32 = (0.0_f32 * l[9]);
    let v8: f32 = (0.0_f32 * l[10]);
    let v9: f32 = (0.0_f32 * l[13]);
    let v10: f32 = (0.0_f32 * l[14]);
    let v11: f32 = (-v2);
    let v12: f32 = (0.0_f32 * (l[2] + l[3]));
    let v13: f32 = (0.0_f32 * (l[7] + l[8]));
    let v14: f32 = (0.0_f32 * (l[12] + l[11]));
    let v15: f32 = (0.0_f32 * (l[15] + l[16]));
    [(v1 + (((v2 * l[2]) + (v3 * l[3])) + v4)), (v5 + (v6 + ((v3 * l[7]) + (v2 * l[8])))), (v7 + (v8 + ((v2 * l[11]) + (v3 * l[12])))), (v9 + (v10 + ((v2 * l[15]) + (v3 * l[16])))), (v1 + (v4 + ((l[3] * v11) + (v3 * l[2])))), (v5 + (v6 + ((v3 * l[8]) + (l[7] * v11)))), (v7 + (v8 + ((v3 * l[11]) + (l[12] * v11)))), (v9 + (v10 + ((v11 * l[16]) + (v3 * l[15])))), (v1 + (l[4] + v12)), (v5 + (l[6] + v13)), (v7 + (l[10] + v14)), (v9 + (l[14] + v15)), (l[0] + (v4 + v12)), (l[5] + (v6 + v13)), (l[9] + (v8 + v14)), (l[13] + (v10 + v15))]
}

/// Inputs of `kernel_m7_euler`, in order.
pub(super) fn load_m7_euler(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 3] {
    let _ = (m, pg, b, s);
    [
        b.angles[1],
        b.angles[2],
        b.angles[0],
    ]
}

/// Kernel of 0x00865990 `m7_euler` (0x8659a0..0x865b21): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m7_euler`.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m7_euler(l: &[f32; 3], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = trig(true, l[0]);
    let v2: f32 = trig(true, l[1]);
    let v3: f32 = trig(false, l[1]);
    let v4: f32 = trig(false, l[0]);
    let v5: f32 = trig(false, l[2]);
    let v6: f32 = (v4 * v2);
    let v7: f32 = trig(true, l[2]);
    let v8: f32 = (v4 * v3);
    [(v1 * v2), (v3 * v1), (-v4), 0.0_f32, ((v5 * v6) - (v3 * v7)), ((v5 * v8) + (v7 * v2)), (v5 * v1), 0.0_f32, ((v7 * v6) + (v5 * v3)), ((v7 * v8) - (v5 * v2)), (v7 * v1), 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32]
}

/// Inputs of `kernel_m7_in_parent_space`, in order.
pub(super) fn load_m7_in_parent_space(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 32] {
    let _ = (m, pg, b, s);
    [
        rd(m, pg, b.parent + 16),
        rd(m, pg, b.parent + 60),
        rd(m, pg, b.parent + 40),
        rd(m, pg, b.parent + 4),
        rd(m, pg, b.parent + 52),
        rd(m, pg, b.parent + 44),
        rd(m, pg, b.parent + 8),
        rd(m, pg, b.parent + 56),
        rd(m, pg, b.parent + 12),
        rd(m, pg, b.parent + 36),
        rd(m, pg, b.parent + 0),
        rd(m, pg, b.parent + 28),
        rd(m, pg, b.parent + 24),
        rd(m, pg, b.parent + 20),
        rd(m, pg, b.parent + 32),
        rd(m, pg, b.parent + 48),
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 28),
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 44),
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 48),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 60),
        rd(m, pg, b.joint + 68),
        rd(m, pg, b.joint + 64),
        rd(m, pg, b.joint + 72),
        rd(m, pg, b.joint + 76),
    ]
}

/// Kernel of 0x00865990 `m7_in_parent_space` (0x865b62..0x866d45): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m7_in_parent_space`.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m7_in_parent_space(l: &[f32; 32], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (l[2] * l[3]);
    let v2: f32 = (l[5] * l[6]);
    let v3: f32 = (l[8] * l[9]);
    let v4: f32 = (l[2] * l[8]);
    let v5: f32 = (l[5] * l[7]);
    let v6: f32 = (l[6] * l[1]);
    let v7: f32 = (-((((((l[1] * v1) + (l[4] * v2)) + (l[7] * v3)) - (l[4] * v4)) - (l[3] * v5)) - (l[9] * v6)));
    let v8: f32 = (l[11] * l[9]);
    let v9: f32 = (l[5] * l[12]);
    let v10: f32 = (l[2] * l[13]);
    let v11: f32 = (l[2] * l[11]);
    let v12: f32 = (l[12] * l[1]);
    let v13: f32 = (((((v8 * l[7]) + ((l[4] * v9) + (v10 * l[1]))) - (l[4] * v11)) - (l[13] * v5)) - (l[9] * v12));
    let v14: f32 = (l[12] * l[3]);
    let v15: f32 = (l[6] * l[11]);
    let v16: f32 = (l[13] * l[8]);
    let v17: f32 = (l[12] * l[8]);
    let v18: f32 = (l[11] * l[7]);
    let v19: f32 = ((((((l[1] * v14) + (l[4] * v15)) + (l[7] * v16)) - (l[4] * v17)) - (l[3] * v18)) - (l[13] * v6));
    let v20: f32 = (-((((((l[5] * v14) + (l[9] * v15)) + (l[2] * v16)) - (l[9] * v17)) - (l[3] * v11)) - (l[13] * v2)));
    let v21: f32 = ((((l[0] * v7) + (l[10] * v13)) + (l[14] * v19)) + (l[15] * v20));
    let c22: bool = (!(v21 == K_1521690));
    let v23: f32 = (1.0_f32 / v21);
    let v24: f32 = (l[11] * l[14]);
    let v25: f32 = (if c22 { (-(v23 * ((((((l[1] * (l[2] * l[0])) + (v9 * l[15])) + (l[7] * v24)) - (v11 * l[15])) - (l[0] * v5)) - (l[14] * v12)))) } else { K_16295D0 });
    let v26: f32 = (if c22 { (v13 * v23) } else { K_16295C0 });
    let v27: f32 = (l[0] * l[9]);
    let v28: f32 = (l[5] * l[4]);
    let v29: f32 = (if c22 { (v23 * ((((((l[15] * (l[5] * l[13])) + (l[1] * v27)) + (l[4] * v24)) - (v8 * l[15])) - (l[0] * v28)) - (l[14] * (l[13] * l[1])))) } else { K_16295E0 });
    let v30: f32 = (l[12] * l[9]);
    let v31: f32 = (l[2] * l[4]);
    let v32: f32 = (if c22 { (-(v23 * ((((((l[7] * v27) + (v10 * l[15])) + (l[4] * (l[12] * l[14]))) - (l[15] * v30)) - (l[0] * v31)) - (l[14] * (l[13] * l[7]))))) } else { K_16295F0 });
    let v33: f32 = (l[8] * l[14]);
    let v34: f32 = (if c22 { (v23 * ((((((l[1] * (l[10] * l[2])) + (l[15] * v2)) + (l[7] * v33)) - (l[15] * v4)) - (l[10] * v5)) - (l[14] * v6))) } else { K_16295D4 });
    let v35: f32 = (if c22 { (v7 * v23) } else { K_16295C4 });
    let v36: f32 = (l[10] * l[9]);
    let v37: f32 = (l[5] * l[3]);
    let v38: f32 = (l[3] * l[1]);
    let v39: f32 = (if c22 { (-(v23 * ((((((l[1] * v36) + (l[15] * v37)) + (l[4] * v33)) - (l[15] * v3)) - (l[10] * v28)) - (l[14] * v38)))) } else { K_16295E4 });
    let v40: f32 = (l[3] * l[7]);
    let v41: f32 = (if c22 { (v23 * ((((((l[7] * v36) + (l[15] * v1)) + (l[4] * (l[6] * l[14]))) - (l[15] * (l[6] * l[9]))) - (l[10] * v31)) - (l[14] * v40))) } else { K_16295F4 });
    let v42: f32 = (l[10] * l[12]);
    let v43: f32 = (l[8] * l[0]);
    let v44: f32 = (if c22 { (-(v23 * ((((((l[1] * v42) + (l[15] * v15)) + (l[7] * v43)) - (l[15] * v17)) - (l[10] * v18)) - (l[0] * v6)))) } else { K_16295D8 });
    let v45: f32 = (if c22 { (v19 * v23) } else { K_16295C8 });
    let v46: f32 = (l[10] * l[13]);
    let v47: f32 = (l[3] * l[11]);
    let v48: f32 = (if c22 { (v23 * ((((((l[1] * v46) + (l[15] * v47)) + (l[4] * v43)) - (l[15] * v16)) - (l[10] * (l[4] * l[11]))) - (l[0] * v38))) } else { K_16295E8 });
    let v49: f32 = (l[6] * l[0]);
    let v50: f32 = (l[13] * l[6]);
    let v51: f32 = (if c22 { (-(v23 * ((((((l[7] * v46) + (l[15] * v14)) + (l[4] * v49)) - (l[15] * v50)) - (l[10] * (l[12] * l[4]))) - (l[0] * v40)))) } else { K_16295F8 });
    let v52: f32 = (if c22 { (v23 * ((((((l[2] * v46) + (l[14] * v14)) + (l[9] * v49)) - (l[14] * v50)) - (l[10] * v30)) - (l[0] * v1))) } else { K_16295FC });
    let v53: f32 = (if c22 { (v23 * ((((((l[5] * v42) + (l[14] * v15)) + (l[2] * v43)) - (l[14] * v17)) - (l[10] * v11)) - (l[0] * v2))) } else { K_16295DC });
    let v54: f32 = (if c22 { (v20 * v23) } else { K_16295CC });
    let v55: f32 = (if c22 { (-(v23 * ((((((l[5] * v46) + (l[14] * v47)) + (l[9] * v43)) - (l[14] * v16)) - (l[10] * v8)) - (l[0] * v37)))) } else { K_16295EC });
    [((((v25 * l[16]) + (v26 * l[17])) + (v29 * l[18])) + (v32 * l[19])), ((((v34 * l[16]) + (v35 * l[17])) + (v39 * l[18])) + (v41 * l[19])), ((((v44 * l[16]) + (v45 * l[17])) + (v48 * l[18])) + (v51 * l[19])), ((v52 * l[19]) + (((v53 * l[16]) + (v54 * l[17])) + (v55 * l[18]))), ((((v25 * l[20]) + (v26 * l[21])) + (v29 * l[22])) + (v32 * l[23])), ((((v34 * l[20]) + (v35 * l[21])) + (v39 * l[22])) + (v41 * l[23])), ((((v44 * l[20]) + (v45 * l[21])) + (v48 * l[22])) + (v51 * l[23])), ((((v53 * l[20]) + (v54 * l[21])) + (v55 * l[22])) + (v52 * l[23])), ((((v25 * l[24]) + (v26 * l[25])) + (v29 * l[26])) + (v32 * l[27])), ((((v34 * l[24]) + (v35 * l[25])) + (v39 * l[26])) + (v41 * l[27])), ((((v44 * l[24]) + (v45 * l[25])) + (v48 * l[26])) + (v51 * l[27])), ((v52 * l[27]) + (((v53 * l[24]) + (v54 * l[25])) + (v55 * l[26]))), ((((v25 * l[28]) + (v26 * l[29])) + (v29 * l[30])) + (v32 * l[31])), ((((v34 * l[28]) + (v35 * l[29])) + (v39 * l[30])) + (v41 * l[31])), ((((v45 * l[29]) + (v44 * l[28])) + (v48 * l[30])) + (v51 * l[31])), ((v52 * l[31]) + (((v54 * l[29]) + (v53 * l[28])) + (v55 * l[30])))]
}

/// Inputs of `kernel_m7_scale_rotation`, in order.
pub(super) fn load_m7_scale_rotation(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 21] {
    let _ = (m, pg, b, s);
    [
        s[12],
        b.r[2],
        rd(m, pg, b.this + 132),
        b.r[1],
        s[4],
        s[0],
        s[8],
        s[13],
        s[5],
        s[1],
        s[9],
        s[14],
        s[2],
        s[6],
        s[10],
        s[15],
        s[7],
        s[3],
        s[11],
        rd(m, pg, b.this + 128),
        b.r[0],
    ]
}

/// Kernel of 0x00865990 `m7_scale_rotation` (0x866d4a..0x867384): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m7_scale_rotation`.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m7_scale_rotation(l: &[f32; 21], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (K_1521690 * l[0]);
    let v2: f32 = (l[1] * K_1521690);
    let v3: f32 = trig(false, v2);
    let v4: f32 = ((l[2] - 1.0_f32) * l[3]);
    let v5: f32 = trig(true, v4);
    let v6: f32 = (v3 * v5);
    let v7: f32 = trig(true, v2);
    let v8: f32 = (v5 * v7);
    let v9: f32 = trig(false, v4);
    let v10: f32 = (-v9);
    let v11: f32 = (K_1521690 * l[7]);
    let v12: f32 = (K_1521690 * l[11]);
    let v13: f32 = (K_1521690 * l[15]);
    let v14: f32 = ((l[19] - 1.0_f32) * l[20]);
    let v15: f32 = trig(false, v14);
    let v16: f32 = (v15 * v5);
    let v17: f32 = (v9 * v3);
    let v18: f32 = trig(true, v14);
    let v19: f32 = ((v15 * v17) + (v18 * v7));
    let v20: f32 = (v9 * v7);
    let v21: f32 = ((v15 * v20) - (v3 * v18));
    let v22: f32 = ((v18 * v17) - (v15 * v7));
    let v23: f32 = ((v15 * v3) + (v18 * v20));
    let v24: f32 = (v18 * v5);
    [(v1 + (((v6 * l[4]) + (v8 * l[5])) + (v10 * l[6]))), (v11 + (((v6 * l[8]) + (v8 * l[9])) + (v10 * l[10]))), (v12 + (((v8 * l[12]) + (v6 * l[13])) + (v10 * l[14]))), (v13 + (((v6 * l[16]) + (v8 * l[17])) + (v10 * l[18]))), (v1 + ((v16 * l[6]) + ((v19 * l[4]) + (v21 * l[5])))), (v11 + (((v19 * l[8]) + (v21 * l[9])) + (v16 * l[10]))), (v12 + (((v19 * l[13]) + (v21 * l[12])) + (v16 * l[14]))), (v13 + (((v19 * l[16]) + (v21 * l[17])) + (v16 * l[18]))), (v1 + (((l[4] * v22) + (v23 * l[5])) + (v24 * l[6]))), (v11 + (((v22 * l[8]) + (v23 * l[9])) + (v24 * l[10]))), (v12 + (((v22 * l[13]) + (v23 * l[12])) + (v24 * l[14]))), (v13 + (((v22 * l[16]) + (v23 * l[17])) + (v24 * l[18]))), (l[0] + (0.0_f32 * (l[6] + (l[4] + l[5])))), (l[7] + (0.0_f32 * (l[10] + (l[8] + l[9])))), (l[11] + (0.0_f32 * (l[14] + (l[12] + l[13])))), (l[15] + (0.0_f32 * (l[18] + (l[16] + l[17]))))]
}

/// Inputs of `kernel_m7_scale_translate`, in order.
pub(super) fn load_m7_scale_translate(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 20] {
    let _ = (m, pg, b, s);
    [
        rd(m, pg, b.this + 136),
        s[0],
        s[1],
        s[2],
        s[3],
        s[4],
        s[5],
        s[6],
        s[7],
        s[8],
        s[9],
        s[10],
        s[11],
        s[12],
        rd(m, pg, b.this + 144),
        rd(m, pg, b.this + 148),
        s[13],
        rd(m, pg, b.this + 152),
        s[14],
        s[15],
    ]
}

/// Kernel of 0x00865990 `m7_scale_translate` (0x867384..0x86740e): float code decoded from its SSE instructions, every
/// operation in the original order. Inputs `l` as listed by `load_m7_scale_translate`.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m7_scale_translate(l: &[f32; 20], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let c1: bool = (l[0] == 1.0_f32);
    let v2: f32 = (if c1 { l[1] } else { (l[0] * l[1]) });
    let v3: f32 = (if c1 { l[2] } else { (l[0] * l[2]) });
    let v4: f32 = (if c1 { l[3] } else { (l[0] * l[3]) });
    let v5: f32 = (if c1 { l[4] } else { (l[0] * l[4]) });
    [v2, v3, v4, v5, l[5], l[6], l[7], l[8], l[9], l[10], l[11], l[12], (l[13] + l[14]), (l[15] + l[16]), (l[17] + l[18]), quiet(l[19])]
}
