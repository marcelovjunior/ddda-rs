//! Float kernels of the uCnsEdit matrix providers (0x00864660, ...), generated
//! from the original's SSE instructions by research/native/cns_kernels/ (a
//! symbolic executor that follows the data flow of every float operation and
//! merges both arms of each float compare). Every operation is kept in the
//! original's order and grouping, including the multiplications by 0.0 whose
//! NaN/infinity propagation the original has, so results are bit-identical.
//! Do not edit by hand: regenerate.
#![allow(clippy::all, unused_parens, unused_variables, dead_code, non_upper_case_globals)]
use super::{read_f32 as rd, Bases, Matrix};
use ddnative_base::mem::{Mem, Pages};

/// The x87 load/store pair the original uses on some results: a signalling NaN comes back quiet.
fn quiet(x: f32) -> f32 {
    if x.is_nan() { f32::from_bits(x.to_bits() | 0x0040_0000) } else { x }
}

const K_1406CE0: f32 = f32::from_bits(0x3f800000);
const K_1407124: f32 = f32::from_bits(0x34000000);
const K_14F0E9C: f32 = f32::from_bits(0x3f000000);
const K_14F9E04: f32 = f32::from_bits(0x40000000);
const K_150A350: f32 = f32::from_bits(0x34000000);
const K_1521690: f32 = f32::from_bits(0x00000000);
const K_1629510: f32 = f32::from_bits(0x3f800000);
const K_1629514: f32 = f32::from_bits(0x3f800000);
const K_1629518: f32 = f32::from_bits(0x3f800000);
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

/// Inputs of `kernel_m9`, in order.
pub(super) fn load_m9(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 19] {
    let _ = (m, pg, b, s);
    [
        b.angles[1],
        b.angles[2],
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 28),
        pg.u8(m, b.this + 0xb0) as f32,
        b.mscale[24 - 24],
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 44),
        b.mscale[25 - 24],
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 48),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 60),
        b.mscale[26 - 24],
        b.angles[0],
    ]
}

/// Kernel of 0x00867B70 (modes 9 and 10): Euler rotation, rows scaled by the model scale divided by the joint's row lengths.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m9(l: &[f32; 19], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = trig(false, l[0]);
    let v2: f32 = (-v1);
    let v3: f32 = (0.0_f32 * v2);
    let v4: f32 = trig(false, l[1]);
    let v5: f32 = trig(true, l[0]);
    let v6: f32 = (v4 * v5);
    let v7: f32 = (0.0_f32 * v6);
    let v8: f32 = trig(true, l[1]);
    let v9: f32 = (v5 * v8);
    let v10: f32 = ((((l[2] * l[2]) + (l[3] * l[3])) + (l[4] * l[4])) + (l[5] * l[5])).sqrt();
    let c11: bool = (l[6] != 0.0_f32);
    let v12: f32 = (if c11 { K_1629510 } else { l[7] });
    let v13: f32 = (if (!(v10 > K_150A350)) { v12 } else { (v12 / v10) });
    let v14: f32 = (0.0_f32 * v9);
    let v15: f32 = ((((l[8] * l[8]) + (l[9] * l[9])) + (l[10] * l[10])) + (l[11] * l[11])).sqrt();
    let v16: f32 = (if c11 { K_1629514 } else { l[12] });
    let v17: f32 = (if (!(v15 > K_150A350)) { v16 } else { (v16 / v15) });
    let v18: f32 = ((((l[13] * l[13]) + (l[14] * l[14])) + (l[15] * l[15])) + (l[16] * l[16])).sqrt();
    let v19: f32 = (if c11 { K_1629518 } else { l[17] });
    let v20: f32 = (if (!(v18 > K_150A350)) { v19 } else { (v19 / v18) });
    let v21: f32 = (v14 + v7);
    let v22: f32 = trig(false, l[18]);
    let v23: f32 = (v22 * v5);
    let v24: f32 = (0.0_f32 * v23);
    let v25: f32 = trig(true, l[18]);
    let v26: f32 = (v1 * v4);
    let v27: f32 = ((v25 * v8) + (v22 * v26));
    let v28: f32 = (0.0_f32 * v27);
    let v29: f32 = (v1 * v8);
    let v30: f32 = ((v22 * v29) - (v4 * v25));
    let v31: f32 = (0.0_f32 * v30);
    let v32: f32 = (v31 + v28);
    let v33: f32 = (v25 * v5);
    let v34: f32 = (0.0_f32 * v33);
    let v35: f32 = ((v25 * v26) - (v22 * v8));
    let v36: f32 = (0.0_f32 * v35);
    let v37: f32 = ((v25 * v29) + (v22 * v4));
    let v38: f32 = (0.0_f32 * v37);
    let v39: f32 = (v38 + v36);
    [(v3 + (v7 + (v9 * v13))), ((v14 + (v6 * v17)) + v3), ((v2 * v20) + v21), (v3 + v21), (v24 + (v28 + (v30 * v13))), (((v27 * v17) + v31) + v24), ((v23 * v20) + v32), (v24 + v32), (v34 + (v36 + (v37 * v13))), (((v35 * v17) + v38) + v34), (v39 + (v33 * v20)), (v34 + v39), (0.0_f32 * v13), (0.0_f32 * v17), (0.0_f32 * v20), 1.0_f32]
}

/// Inputs of `kernel_m11`, in order.
pub(super) fn load_m11(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 23] {
    let _ = (m, pg, b, s);
    [
        pg.u8(m, b.joint + 0x93) as f32,
        b.angles[1],
        b.angles[2],
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 28),
        pg.u8(m, b.this + 0xb0) as f32,
        b.mscale[24 - 24],
        rd(m, pg, b.this + 192),
        b.angles[0],
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 44),
        b.mscale[25 - 24],
        rd(m, pg, b.this + 196),
        rd(m, pg, b.joint + 48),
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 60),
        b.mscale[26 - 24],
        rd(m, pg, b.this + 200),
    ]
}

/// Kernel of 0x0086D180 (mode 11): Euler rotation, rows scaled by (this scale / joint row length) * model scale.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m11(l: &[f32; 23], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let c1: bool = (l[0] != 2.0_f32);
    let v2: f32 = trig(true, l[1]);
    let v3: f32 = trig(true, l[2]);
    let v4: f32 = (v2 * v3);
    let v5: f32 = ((((l[3] * l[3]) + (l[4] * l[4])) + (l[5] * l[5])) + (l[6] * l[6])).sqrt();
    let c6: bool = (l[7] != 0.0_f32);
    let v7: f32 = (if c6 { K_1629510 } else { l[8] });
    let v8: f32 = (if (!(v5 > K_150A350)) { v7 } else { (v7 * (l[9] / v5)) });
    let v9: f32 = (if c1 { (v4 * v8) } else { v4 });
    let v10: f32 = trig(false, l[2]);
    let v11: f32 = (v10 * v2);
    let v12: f32 = (if c1 { (v11 * v8) } else { v11 });
    let v13: f32 = trig(false, l[1]);
    let v14: f32 = (-v13);
    let v15: f32 = (if c1 { (v14 * v8) } else { v14 });
    let v16: f32 = (if c1 { (0.0_f32 * v8) } else { 0.0_f32 });
    let v17: f32 = trig(false, l[10]);
    let v18: f32 = (v13 * v3);
    let v19: f32 = trig(true, l[10]);
    let v20: f32 = ((((l[11] * l[11]) + (l[12] * l[12])) + (l[13] * l[13])) + (l[14] * l[14])).sqrt();
    let v21: f32 = (if c6 { K_1629514 } else { l[15] });
    let v22: f32 = (if (!(v20 > K_150A350)) { v21 } else { (v21 * (l[16] / v20)) });
    let v23: f32 = (v13 * v10);
    let v24: f32 = ((v17 * v10) + (v19 * v18));
    let v25: f32 = ((((l[17] * l[17]) + (l[18] * l[18])) + (l[19] * l[19])) + (l[20] * l[20])).sqrt();
    let v26: f32 = (if c6 { K_1629518 } else { l[21] });
    let v27: f32 = (if (!(v25 > K_150A350)) { v26 } else { (v26 * (l[22] / v25)) });
    let v28: f32 = (if c1 { (v24 * v27) } else { v24 });
    let v29: f32 = ((v19 * v23) - (v17 * v3));
    let v30: f32 = (if c1 { (v29 * v27) } else { v29 });
    let v31: f32 = (v19 * v2);
    let v32: f32 = (if c1 { (v31 * v27) } else { v31 });
    let v33: f32 = (if c1 { (0.0_f32 * v27) } else { 0.0_f32 });
    [v9, v12, v15, v16, (((v17 * v18) - (v10 * v19)) * v22), (((v19 * v3) + (v17 * v23)) * v22), ((v17 * v2) * v22), (0.0_f32 * v22), v28, v30, v32, v33, 0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32]
}

/// Inputs of `kernel_m15_euler`.
pub(super) fn load_m15_euler(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 3] {
    let _ = (m, pg, b, s);
    [
        b.angles[1],
        b.angles[2],
        b.angles[0],
    ]
}

/// Kernel of 0x0086AFF0 (mode 15), its Euler rotation.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m15_euler(l: &[f32; 3], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
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

/// Inputs of `kernel_m8_euler`.
pub(super) fn load_m8_euler(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 3] {
    let _ = (m, pg, b, s);
    [
        b.angles[1],
        b.angles[2],
        b.angles[0],
    ]
}

/// Kernel of 0x00867420 (mode 8): its Euler rotation.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m8_euler(l: &[f32; 3], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
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

/// Inputs of `kernel_m8_parent`.
pub(super) fn load_m8_parent(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 35] {
    let _ = (m, pg, b, s);
    [
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
        rd(m, pg, b.this + 136),
        rd(m, pg, b.joint + 8),
        rd(m, pg, b.parent + 0),
        rd(m, pg, b.parent + 60),
        rd(m, pg, b.parent + 40),
        rd(m, pg, b.parent + 20),
        rd(m, pg, b.parent + 24),
        rd(m, pg, b.parent + 44),
        rd(m, pg, b.parent + 52),
        rd(m, pg, b.parent + 56),
        rd(m, pg, b.parent + 36),
        rd(m, pg, b.parent + 28),
        rd(m, pg, b.parent + 16),
        rd(m, pg, b.parent + 8),
        rd(m, pg, b.parent + 4),
        rd(m, pg, b.parent + 12),
        rd(m, pg, b.parent + 32),
        rd(m, pg, b.parent + 48),
        rd(m, pg, b.joint + 68),
        rd(m, pg, b.joint + 64),
        rd(m, pg, b.joint + 72),
        rd(m, pg, b.joint + 76),
        rd(m, pg, b.this + 132),
    ]
}

/// Kernel of 0x00867420 (mode 8): translation row from the parent bone and the weights at this+0x84/0x88.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m8_parent(l: &[f32; 35], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (l[16] * l[23]);
    let v2: f32 = (l[21] * l[19]);
    let v3: f32 = (l[25] * l[19]);
    let v4: f32 = (l[16] * l[26]);
    let v5: f32 = (l[22] * l[27]);
    let v6: f32 = (l[16] * l[27]);
    let v7: f32 = (l[15] * l[25]);
    let v8: f32 = (-((((((l[20] * v3) + (l[15] * v4)) + (l[21] * v5)) - (l[20] * v6)) - (l[26] * v2)) - (l[22] * v7)));
    let v9: f32 = (l[26] * l[18]);
    let v10: f32 = (l[23] * l[25]);
    let v11: f32 = (l[17] * l[27]);
    let v12: f32 = (l[27] * l[18]);
    let v13: f32 = ((((l[14] * ((((((l[15] * (l[16] * l[17])) + ((l[18] * l[19]) * l[20])) + (l[21] * (l[22] * l[23]))) - (v1 * l[20])) - (l[17] * v2)) - (l[22] * (l[15] * l[18])))) + (l[24] * v8)) + (l[28] * ((((((l[15] * v9) + (l[20] * v10)) + (l[21] * v11)) - (l[20] * v12)) - (l[26] * (l[21] * l[23]))) - (l[17] * v7)))) + (l[29] * (-((((((l[22] * v10) + (l[19] * v9)) + (l[16] * v11)) - (l[22] * v12)) - (l[26] * v1)) - (l[17] * v3)))));
    let c14: bool = (!(v13 == 0.0_f32));
    let v15: f32 = (1.0_f32 / v13);
    let v16: f32 = (l[27] * l[28]);
    let v17: f32 = (if c14 { (v15 * ((((((l[15] * (l[14] * l[16])) + (l[29] * v3)) + (l[21] * v16)) - (l[29] * v6)) - (l[14] * v2)) - (l[28] * v7))) } else { K_16295D4 });
    let v18: f32 = (if c14 { (v8 * v15) } else { K_16295C4 });
    let v19: f32 = (l[14] * l[22]);
    let v20: f32 = (if c14 { (-(v15 * ((((((l[29] * (l[26] * l[19])) + (l[15] * v19)) + (l[20] * v16)) - (l[29] * v5)) - (l[14] * (l[19] * l[20]))) - (l[28] * (l[15] * l[26]))))) } else { K_16295E4 });
    let v21: f32 = (if c14 { (v15 * ((((((l[29] * v4) + (l[21] * v19)) + (l[20] * (l[25] * l[28]))) - (l[29] * (l[22] * l[25]))) - (l[14] * (l[16] * l[20]))) - (l[28] * (l[21] * l[26])))) } else { K_16295F4 });
    let v22: f32 = ((((v17 * l[30]) + (v18 * l[31])) + (v20 * l[32])) + (v21 * l[33]));
    [l[0], l[1], l[2], l[3], l[4], l[5], l[6], l[7], l[8], l[9], l[10], l[11], 0.0_f32, (((l[12] - 1.0_f32) * (l[13] - v22)) + (v22 * (l[34] - 1.0_f32))), 0.0_f32, 1.0_f32]
}

/// Inputs of `kernel_m8_finish`.
pub(super) fn load_m8_finish(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 19] {
    let _ = (m, pg, b, s);
    [
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

/// Kernel of 0x00867420 (mode 8): adds the translation at this+0x90.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m8_finish(l: &[f32; 19], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {

    [l[0], l[1], l[2], l[3], l[4], l[5], l[6], l[7], l[8], l[9], l[10], l[11], (l[12] + l[13]), (l[14] + l[15]), (l[16] + l[17]), quiet(l[18])]
}

/// Inputs of `kernel_m12`.
pub(super) fn load_m12(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 42] {
    let _ = (m, pg, b, s);
    [
        rd(m, pg, b.this + 128),
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 28),
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 44),
        rd(m, pg, b.parent + 20),
        rd(m, pg, b.joint + 132),
        rd(m, pg, b.parent + 4),
        rd(m, pg, b.joint + 128),
        rd(m, pg, b.joint + 48),
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 60),
        rd(m, pg, b.parent + 36),
        rd(m, pg, b.joint + 136),
        rd(m, pg, b.parent + 52),
        rd(m, pg, b.joint + 68),
        rd(m, pg, b.this + 132),
        rd(m, pg, b.parent + 48),
        rd(m, pg, b.parent + 16),
        rd(m, pg, b.joint + 96),
        rd(m, pg, b.joint + 104),
        rd(m, pg, b.parent + 0),
        rd(m, pg, b.joint + 100),
        rd(m, pg, b.joint + 108),
        rd(m, pg, b.parent + 32),
        rd(m, pg, b.parent + 60),
        rd(m, pg, b.parent + 28),
        rd(m, pg, b.parent + 12),
        rd(m, pg, b.parent + 44),
        rd(m, pg, b.joint + 76),
        rd(m, pg, b.parent + 56),
        rd(m, pg, b.parent + 24),
        rd(m, pg, b.parent + 8),
        rd(m, pg, b.parent + 40),
        rd(m, pg, b.joint + 72),
        rd(m, pg, b.joint + 64),
        rd(m, pg, b.this + 136),
    ]
}

/// Kernel of 0x00868000 (mode 12): re-bases the joint matrix on the parent (and the joint scale) and updates the output translation. Outputs: 16 floats of the output matrix, then 16 of the joint matrix.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m12(l: &[f32; 42], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 32] {
    let c1: bool = (!(l[0] == K_1629510));
    let v2: f32 = ((((l[1] * l[1]) + (l[2] * l[2])) + (l[3] * l[3])) + (l[4] * l[4])).sqrt();
    let v3: f32 = (if (!(v2 > K_150A350)) { K_1629510 } else { (1.0_f32 / v2) });
    let c4: bool = (!(v3 == K_1629510));
    let v5: f32 = ((((l[5] * l[5]) + (l[6] * l[6])) + (l[7] * l[7])) + (l[8] * l[8])).sqrt();
    let v6: f32 = (if (!(v5 > K_150A350)) { K_1629514 } else { (1.0_f32 / v5) });
    let v7: f32 = (v6 * l[9]);
    let v8: f32 = quiet(l[10]);
    let v9: f32 = (v3 * l[11]);
    let v10: f32 = quiet(l[12]);
    let v11: f32 = ((((l[13] * l[13]) + (l[14] * l[14])) + (l[15] * l[15])) + (l[16] * l[16])).sqrt();
    let v12: f32 = (if (!(v11 > K_150A350)) { K_1629518 } else { (1.0_f32 / v11) });
    let v13: f32 = (v12 * l[17]);
    let v14: f32 = quiet(l[18]);
    let v15: f32 = ((((v7 * v8) + (v9 * v10)) + (v13 * v14)) + (1.0_f32 * l[19]));
    let c16: bool = (!(v6 == K_1629514));
    let c17: bool = (v12 == K_1629518);
    let v18: f32 = (if c17 { l[20] } else { v15 });
    let v19: f32 = (if c16 { v15 } else { v18 });
    let v20: f32 = (if c4 { v15 } else { v19 });
    let v21: f32 = ((v20 - l[19]) * (l[21] - 1.0_f32));
    let v22: f32 = (0.0_f32 * l[22]);
    let v23: f32 = (v6 * l[23]);
    let v24: f32 = (l[24] * K_14F9E04);
    let v25: f32 = (l[24] * v24);
    let v26: f32 = (l[25] * K_14F9E04);
    let v27: f32 = (l[25] * v26);
    let v28: f32 = (1.0_f32 - (v25 + v27));
    let v29: f32 = (v3 * l[26]);
    let v30: f32 = (l[27] * K_14F9E04);
    let v31: f32 = (l[24] * v30);
    let v32: f32 = (v26 * l[28]);
    let v33: f32 = (v31 - v32);
    let v34: f32 = (v12 * l[29]);
    let v35: f32 = (v24 * l[28]);
    let v36: f32 = (l[27] * v26);
    let v37: f32 = (v35 + v36);
    let v38: f32 = (v22 + (((v23 * v28) + (v29 * v33)) + (v34 * v37)));
    let v39: f32 = (if c17 { l[5] } else { v38 });
    let v40: f32 = (if c16 { v38 } else { v39 });
    let v41: f32 = (if c4 { v38 } else { v40 });
    let v42: f32 = (v6 * l[31]);
    let v43: f32 = (v3 * l[32]);
    let v44: f32 = (v12 * l[33]);
    let v45: f32 = ((1.0_f32 * l[30]) + (((v42 * v8) + (v43 * v10)) + (v44 * v14)));
    let v46: f32 = (if c17 { l[34] } else { v45 });
    let v47: f32 = (if c16 { v45 } else { v46 });
    let v48: f32 = (if c4 { v45 } else { v47 });
    let v49: f32 = (0.0_f32 * l[19]);
    let v50: f32 = (v32 + v31);
    let v51: f32 = (l[27] * v30);
    let v52: f32 = (1.0_f32 - (v27 + v51));
    let v53: f32 = (l[24] * v26);
    let v54: f32 = (v30 * l[28]);
    let v55: f32 = (v53 - v54);
    let v56: f32 = (v49 + (((v7 * v50) + (v9 * v52)) + (v13 * v55)));
    let v57: f32 = (if c17 { l[2] } else { v56 });
    let v58: f32 = (if c16 { v56 } else { v57 });
    let v59: f32 = (if c4 { v56 } else { v58 });
    let v60: f32 = (0.0_f32 * l[35]);
    let v61: f32 = (v6 * l[36]);
    let v62: f32 = (v36 - v35);
    let v63: f32 = (v3 * l[37]);
    let v64: f32 = (v54 + v53);
    let v65: f32 = (v12 * l[38]);
    let v66: f32 = (1.0_f32 - (v25 + v51));
    let v67: f32 = (v60 + (((v61 * v62) + (v63 * v64)) + (v65 * v66)));
    let v68: f32 = (if c17 { l[15] } else { v67 });
    let v69: f32 = (if c16 { v67 } else { v68 });
    let v70: f32 = (if c4 { v67 } else { v69 });
    let v71: f32 = ((((v61 * v50) + (v63 * v52)) + (v65 * v55)) + v60);
    let v72: f32 = (if c17 { l[3] } else { v71 });
    let v73: f32 = (if c16 { v71 } else { v72 });
    let v74: f32 = (if c4 { v71 } else { v73 });
    let v75: f32 = (0.0_f32 * l[30]);
    let v76: f32 = (v75 + (((v42 * v62) + (v43 * v64)) + (v44 * v66)));
    let v77: f32 = (if c17 { l[16] } else { v76 });
    let v78: f32 = (if c16 { v76 } else { v77 });
    let v79: f32 = (if c4 { v76 } else { v78 });
    let v80: f32 = (v74 * v79);
    let v81: f32 = ((((v61 * v8) + (v63 * v10)) + (v65 * v14)) + (1.0_f32 * l[35]));
    let v82: f32 = (if c17 { l[39] } else { v81 });
    let v83: f32 = (if c16 { v81 } else { v82 });
    let v84: f32 = (if c4 { v81 } else { v83 });
    let v85: f32 = ((((v43 * v52) + (v42 * v50)) + (v44 * v55)) + v75);
    let v86: f32 = (if c17 { l[4] } else { v85 });
    let v87: f32 = (if c16 { v85 } else { v86 });
    let v88: f32 = (if c4 { v85 } else { v87 });
    let v89: f32 = (v49 + (((v7 * v62) + (v9 * v64)) + (v13 * v66)));
    let v90: f32 = (if c17 { l[14] } else { v89 });
    let v91: f32 = (if c16 { v89 } else { v90 });
    let v92: f32 = (if c4 { v89 } else { v91 });
    let v93: f32 = (v88 * v92);
    let v94: f32 = (v88 * v70);
    let v95: f32 = (v84 * v79);
    let v96: f32 = (v74 * v48);
    let v97: f32 = (-((((((v48 * (v59 * v70)) + (v20 * v80)) + (v84 * v93)) - (v20 * v94)) - (v59 * v95)) - (v92 * v96)));
    let v98: f32 = ((((v23 * v50) + (v29 * v52)) + (v34 * v55)) + v22);
    let v99: f32 = (if c17 { l[1] } else { v98 });
    let v100: f32 = (if c16 { v98 } else { v99 });
    let v101: f32 = (if c4 { v98 } else { v100 });
    let v102: f32 = (v60 + (((v61 * v28) + (v63 * v33)) + (v65 * v37)));
    let v103: f32 = (if c17 { l[7] } else { v102 });
    let v104: f32 = (if c16 { v102 } else { v103 });
    let v105: f32 = (if c4 { v102 } else { v104 });
    let v106: f32 = (v105 * v79);
    let v107: f32 = (v49 + (((v7 * v28) + (v9 * v33)) + (v13 * v37)));
    let v108: f32 = (if c17 { l[6] } else { v107 });
    let v109: f32 = (if c16 { v107 } else { v108 });
    let v110: f32 = (if c4 { v107 } else { v109 });
    let v111: f32 = (v75 + (((v43 * v33) + (v42 * v28)) + (v44 * v37)));
    let v112: f32 = (if c17 { l[8] } else { v111 });
    let v113: f32 = (if c16 { v111 } else { v112 });
    let v114: f32 = (if c4 { v111 } else { v113 });
    let v115: f32 = (v114 * v92);
    let v116: f32 = (v114 * v70);
    let v117: f32 = (v105 * v48);
    let v118: f32 = ((((((v20 * v106) + (v48 * (v70 * v110))) + (v84 * v115)) - (v20 * v116)) - (v110 * v95)) - (v92 * v117));
    let v119: f32 = (v22 + (((v23 * v62) + (v29 * v64)) + (v34 * v66)));
    let v120: f32 = (if c17 { l[13] } else { v119 });
    let v121: f32 = (if c16 { v119 } else { v120 });
    let v122: f32 = (if c4 { v119 } else { v121 });
    let v123: f32 = (v88 * v110);
    let v124: f32 = (v59 * v105);
    let v125: f32 = (v114 * v74);
    let v126: f32 = (v88 * v105);
    let v127: f32 = (v114 * v84);
    let v128: f32 = (((((v84 * v123) + ((v48 * v124) + (v20 * v125))) - (v20 * v126)) - (v59 * v127)) - (v110 * v96));
    let v129: f32 = ((((v23 * v8) + (v29 * v10)) + (v34 * v14)) + (1.0_f32 * l[22]));
    let v130: f32 = (if c17 { l[40] } else { v129 });
    let v131: f32 = (if c16 { v129 } else { v130 });
    let v132: f32 = (if c4 { v129 } else { v131 });
    let v133: f32 = ((((v41 * v97) + (v101 * v118)) + (v122 * v128)) + (v132 * (-((((((v92 * v125) + (v79 * v124)) + (v70 * v123)) - (v92 * v126)) - (v59 * v116)) - (v110 * v80)))));
    let c134: bool = (!(v133 == K_1521690));
    let v135: f32 = (1.0_f32 / v133);
    let v136: f32 = (v114 * v122);
    let v137: f32 = (if c134 { (-(v135 * ((((((v48 * (v41 * v70)) + (v132 * v106)) + (v84 * v136)) - (v132 * v116)) - (v41 * v95)) - (v122 * v117)))) } else { K_16295D0 });
    let v138: f32 = ((v132 - l[22]) * (l[0] - 1.0_f32));
    let v139: f32 = (if c134 { (v118 * v135) } else { K_16295C0 });
    let v140: f32 = ((v84 - l[35]) * (l[41] - 1.0_f32));
    let v141: f32 = (v20 * v79);
    let v142: f32 = (if c134 { (v135 * ((((((v48 * (v92 * v41)) + (v132 * (v110 * v79))) + (v20 * v136)) - (v132 * v115)) - (v41 * v141)) - (v122 * (v48 * v110)))) } else { K_16295E0 });
    let v143: f32 = (K_16295F0 + (((v21 * v137) + (v138 * v139)) + (v140 * v142)));
    let c144: bool = (!(l[21] == K_1629514));
    let c145: bool = (l[41] == K_1629518);
    let v146: f32 = (if c145 { K_16295F0 } else { v143 });
    let v147: f32 = (if c144 { v143 } else { v146 });
    let v148: f32 = (if c1 { v143 } else { v147 });
    let v149: f32 = (v88 * v122);
    let v150: f32 = (if c134 { (v135 * ((((((v48 * (v70 * v101)) + (v132 * v80)) + (v84 * v149)) - (v132 * v94)) - (v101 * v95)) - (v122 * v96))) } else { K_16295D4 });
    let v151: f32 = (if c134 { (v97 * v135) } else { K_16295C4 });
    let v152: f32 = (v59 * v48);
    let v153: f32 = (if c134 { (-(v135 * (((((v20 * v149) + ((v132 * (v59 * v79)) + (v48 * (v92 * v101)))) - (v132 * v93)) - (v101 * v141)) - (v122 * v152)))) } else { K_16295E4 });
    let v154: f32 = (K_16295F4 + (((v21 * v150) + (v138 * v151)) + (v140 * v153)));
    let v155: f32 = (if c145 { K_16295F4 } else { v154 });
    let v156: f32 = (if c144 { v154 } else { v155 });
    let v157: f32 = (if c1 { v154 } else { v156 });
    let v158: f32 = (v88 * v41);
    let v159: f32 = (if c134 { (-(v135 * ((((((v132 * v125) + (v48 * (v101 * v105))) + (v84 * v158)) - (v132 * v126)) - (v101 * v127)) - (v41 * v96)))) } else { K_16295D8 });
    let v160: f32 = (if c134 { (v128 * v135) } else { K_16295C8 });
    let v161: f32 = (if c134 { (v135 * ((((((v132 * (v59 * v114)) + (v48 * (v101 * v110))) + (v20 * v158)) - (v132 * v123)) - (v101 * (v114 * v20))) - (v41 * v152))) } else { K_16295E8 });
    let v162: f32 = (K_16295F8 + (((v21 * v159) + (v138 * v160)) + (v140 * v161)));
    let v163: f32 = (if c145 { K_16295F8 } else { v162 });
    let v164: f32 = (if c144 { v162 } else { v163 });
    let v165: f32 = (if c1 { v162 } else { v164 });
    let v166: f32 = quiet(K_16295FC);
    let v167: f32 = (if c145 { K_16295FC } else { v166 });
    let v168: f32 = (if c144 { v166 } else { v167 });
    let v169: f32 = (if c1 { v166 } else { v168 });
    [K_16295C0, K_16295C4, K_16295C8, K_16295CC, K_16295D0, K_16295D4, K_16295D8, K_16295DC, K_16295E0, K_16295E4, K_16295E8, K_16295EC, v148, v157, v165, v169, v101, v59, v74, v88, v41, v110, v105, v114, v122, v92, v70, v79, v132, v20, v84, v48]
}

/// Inputs of `kernel_m13_euler`.
pub(super) fn load_m13_euler(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 3] {
    let _ = (m, pg, b, s);
    [
        b.angles[1],
        b.angles[2],
        b.angles[0],
    ]
}

/// Kernel of 0x00869260 (mode 13): its Euler rotation.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m13_euler(l: &[f32; 3], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = trig(true, l[0]);
    let v2: f32 = trig(true, l[1]);
    let v3: f32 = trig(false, l[1]);
    let v4: f32 = trig(false, l[0]);
    let v5: f32 = trig(false, l[2]);
    let v6: f32 = (v4 * v2);
    let v7: f32 = trig(true, l[2]);
    let v8: f32 = (v4 * v3);
    [(v1 * v2), (v3 * v1), (-v4), 0.0_f32, ((v5 * v6) - (v3 * v7)), ((v5 * v8) + (v7 * v2)), (v5 * v1), 0.0_f32, ((v5 * v3) + (v7 * v6)), ((v7 * v8) - (v5 * v2)), (v7 * v1), 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32]
}

/// Inputs of `kernel_m13_ref`.
pub(super) fn load_m13_ref(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 46] {
    let _ = (m, pg, b, s);
    [
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
        rd(m, pg, b.joint + 76),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 68),
        rd(m, pg, b.joint + 60),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 72),
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 28),
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 44),
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 48),
        rd(m, pg, b.joint + 64),
        rd(m, pg, b.child + 64),
        rd(m, pg, b.parent + 16),
        rd(m, pg, b.this + 148),
        rd(m, pg, b.parent + 0),
        rd(m, pg, b.this + 144),
        rd(m, pg, b.parent + 32),
        rd(m, pg, b.this + 152),
        rd(m, pg, b.this + 128),
        rd(m, pg, b.child + 68),
        rd(m, pg, b.parent + 20),
        rd(m, pg, b.parent + 4),
        rd(m, pg, b.parent + 36),
        rd(m, pg, b.this + 132),
        rd(m, pg, b.child + 72),
        rd(m, pg, b.parent + 24),
        rd(m, pg, b.parent + 8),
        rd(m, pg, b.parent + 40),
        rd(m, pg, b.this + 136),
    ]
}

/// Kernel of 0x00869260 (mode 13): translation row blended between the reference joint and the joint seen through the parent, per axis weights this+0x80..0x88.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m13_ref(l: &[f32; 46], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (l[13] * l[14]);
    let v2: f32 = (l[16] * l[17]);
    let v3: f32 = (l[19] * l[20]);
    let v4: f32 = (l[13] * l[20]);
    let v5: f32 = (l[18] * l[16]);
    let v6: f32 = (l[12] * l[17]);
    let v7: f32 = (-((((((l[12] * v1) + (l[15] * v2)) + (l[18] * v3)) - (l[15] * v4)) - (v5 * l[14])) - (l[19] * v6)));
    let v8: f32 = (l[22] * l[13]);
    let v9: f32 = (l[23] * l[16]);
    let v10: f32 = (l[24] * l[19]);
    let v11: f32 = (l[13] * l[24]);
    let v12: f32 = (l[12] * l[23]);
    let v13: f32 = ((((((l[12] * v8) + (l[15] * v9)) + (l[18] * v10)) - (l[15] * v11)) - (l[22] * v5)) - (l[19] * v12));
    let v14: f32 = (l[22] * l[20]);
    let v15: f32 = (l[23] * l[14]);
    let v16: f32 = (l[24] * l[17]);
    let v17: f32 = (l[23] * l[20]);
    let v18: f32 = (l[18] * l[24]);
    let v19: f32 = (((((l[18] * v14) + ((l[12] * v15) + (l[15] * v16))) - (l[15] * v17)) - (l[14] * v18)) - (l[22] * v6));
    let v20: f32 = ((((v7 * l[21]) + (v13 * l[25])) + (v19 * l[26])) + (l[27] * (-((((((l[19] * v16) + (l[16] * v15)) + (l[13] * v14)) - (l[19] * v17)) - (v11 * l[14])) - (l[22] * v2)))));
    let c21: bool = (!(v20 == K_1521690));
    let v22: f32 = (1.0_f32 / v20);
    let v23: f32 = (l[19] * l[21]);
    let v24: f32 = (l[13] * l[15]);
    let v25: f32 = (if c21 { (-(v22 * ((((((v8 * l[27]) + (l[18] * v23)) + (l[15] * (l[23] * l[26]))) - (l[27] * (l[23] * l[19]))) - (l[21] * v24)) - (l[26] * (l[22] * l[18]))))) } else { K_16295F0 });
    let v26: f32 = (l[28] + ((((((l[29] * l[30]) + (l[31] * l[32])) + (l[33] * l[34])) + l[27]) - l[28]) * l[35]));
    let v27: f32 = (if c21 { (v13 * v22) } else { K_16295C0 });
    let v28: f32 = (l[36] + ((((((l[37] * l[30]) + (l[38] * l[32])) + (l[39] * l[34])) + l[15]) - l[36]) * l[40]));
    let v29: f32 = (l[24] * l[26]);
    let v30: f32 = (if c21 { (-(v22 * (((((l[18] * v29) + ((l[27] * v9) + (l[12] * (l[13] * l[21])))) - (l[27] * v11)) - (v5 * l[21])) - (v12 * l[26])))) } else { K_16295D0 });
    let v31: f32 = (l[41] + ((((((l[42] * l[30]) + (l[43] * l[32])) + (l[44] * l[34])) + l[18]) - l[41]) * l[45]));
    let v32: f32 = (l[15] * l[16]);
    let v33: f32 = (if c21 { (v22 * ((((((l[12] * v23) + (l[27] * (l[22] * l[16]))) + (l[15] * v29)) - (l[27] * v10)) - (l[21] * v32)) - (l[26] * (l[12] * l[22])))) } else { K_16295E0 });
    let v34: f32 = (l[19] * l[25]);
    let v35: f32 = (l[18] * l[14]);
    let v36: f32 = (if c21 { (v22 * ((((((l[27] * v1) + (l[18] * v34)) + (l[15] * (l[17] * l[26]))) - (l[27] * (l[19] * l[17]))) - (l[25] * v24)) - (l[26] * v35))) } else { K_16295F4 });
    let v37: f32 = (if c21 { (v7 * v22) } else { K_16295C4 });
    let v38: f32 = (l[20] * l[26]);
    let v39: f32 = (if c21 { (v22 * ((((((l[12] * (l[13] * l[25])) + (l[27] * v2)) + (l[18] * v38)) - (l[27] * v4)) - (v5 * l[25])) - (v6 * l[26]))) } else { K_16295D4 });
    let v40: f32 = (l[12] * l[14]);
    let v41: f32 = (if c21 { (-(v22 * (((((l[15] * v38) + ((l[12] * v34) + (l[27] * (l[16] * l[14])))) - (l[27] * v3)) - (l[25] * v32)) - (l[26] * v40)))) } else { K_16295E4 });
    let v42: f32 = (l[22] * l[25]);
    let v43: f32 = (if c21 { (-(v22 * (((((l[15] * (l[17] * l[21])) + ((l[27] * v15) + (l[18] * v42))) - (l[27] * (l[22] * l[17]))) - (l[25] * (l[15] * l[23]))) - (l[21] * v35)))) } else { K_16295F8 });
    let v44: f32 = (if c21 { (v19 * v22) } else { K_16295C8 });
    let v45: f32 = (l[20] * l[21]);
    let v46: f32 = (if c21 { (-(v22 * (((((l[18] * v45) + ((l[12] * (l[23] * l[25])) + (l[27] * v16))) - (l[27] * v17)) - (v18 * l[25])) - (v6 * l[21])))) } else { K_16295D8 });
    let v47: f32 = (if c21 { (v22 * ((((((l[12] * v42) + (l[27] * (l[24] * l[14]))) + (l[15] * v45)) - (l[27] * v14)) - (l[25] * (l[15] * l[24]))) - (l[21] * v40))) } else { K_16295E8 });
    [l[0], l[1], l[2], l[3], l[4], l[5], l[6], l[7], l[8], l[9], l[10], l[11], (v25 + (((v26 * v27) + (v28 * v30)) + (v31 * v33))), (v36 + (((v26 * v37) + (v28 * v39)) + (v31 * v41))), (v43 + (((v26 * v44) + (v28 * v46)) + (v31 * v47))), 1.0_f32]
}

/// Inputs of `kernel_m14_euler`.
pub(super) fn load_m14_euler(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 3] {
    let _ = (m, pg, b, s);
    [
        b.angles[1],
        b.angles[2],
        b.angles[0],
    ]
}

/// Kernel of 0x0086A100 (mode 14): its Euler rotation.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m14_euler(l: &[f32; 3], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = trig(true, l[0]);
    let v2: f32 = trig(true, l[1]);
    let v3: f32 = trig(false, l[1]);
    let v4: f32 = trig(false, l[0]);
    let v5: f32 = trig(false, l[2]);
    let v6: f32 = (v4 * v2);
    let v7: f32 = trig(true, l[2]);
    let v8: f32 = (v4 * v3);
    [(v1 * v2), (v3 * v1), (-v4), 0.0_f32, ((v5 * v6) - (v3 * v7)), ((v7 * v2) + (v5 * v8)), (v5 * v1), 0.0_f32, ((v5 * v3) + (v7 * v6)), ((v7 * v8) - (v5 * v2)), (v7 * v1), 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32]
}

/// Inputs of `kernel_m14_ref`.
pub(super) fn load_m14_ref(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 43] {
    let _ = (m, pg, b, s);
    [
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
        rd(m, pg, b.joint + 68),
        rd(m, pg, b.joint + 40),
        rd(m, pg, b.joint + 60),
        rd(m, pg, b.joint + 76),
        rd(m, pg, b.joint + 36),
        rd(m, pg, b.joint + 56),
        rd(m, pg, b.joint + 72),
        rd(m, pg, b.joint + 44),
        rd(m, pg, b.joint + 52),
        rd(m, pg, b.joint + 16),
        rd(m, pg, b.joint + 24),
        rd(m, pg, b.joint + 20),
        rd(m, pg, b.joint + 28),
        rd(m, pg, b.joint + 32),
        rd(m, pg, b.joint + 48),
        rd(m, pg, b.joint + 64),
        rd(m, pg, b.child + 64),
        rd(m, pg, b.child + 68),
        rd(m, pg, b.child + 72),
        rd(m, pg, b.parent + 0),
        rd(m, pg, b.this + 144),
        rd(m, pg, b.parent + 16),
        rd(m, pg, b.this + 148),
        rd(m, pg, b.parent + 32),
        rd(m, pg, b.this + 152),
        rd(m, pg, b.parent + 4),
        rd(m, pg, b.parent + 20),
        rd(m, pg, b.parent + 36),
        rd(m, pg, b.parent + 8),
        rd(m, pg, b.parent + 24),
        rd(m, pg, b.parent + 40),
    ]
}

/// Kernel of 0x0086A100 (mode 14): translation row from the reference joint seen through the parent.
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_m14_ref(l: &[f32; 43], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = (l[13] * l[14]);
    let v2: f32 = (l[16] * l[17]);
    let v3: f32 = (l[19] * l[20]);
    let v4: f32 = (l[19] * l[17]);
    let v5: f32 = (l[18] * l[14]);
    let v6: f32 = (l[13] * l[15]);
    let v7: f32 = ((((((l[12] * v1) + (l[15] * v2)) + (l[18] * v3)) - (l[12] * v4)) - (l[16] * v5)) - (l[20] * v6));
    let v8: f32 = (l[22] * l[14]);
    let v9: f32 = (l[17] * l[23]);
    let v10: f32 = (l[20] * l[24]);
    let v11: f32 = (l[17] * l[24]);
    let v12: f32 = (l[22] * l[15]);
    let v13: f32 = (-((((((l[12] * v8) + (l[15] * v9)) + (l[18] * v10)) - (l[12] * v11)) - (v5 * l[23])) - (l[20] * v12)));
    let v14: f32 = (l[16] * l[24]);
    let v15: f32 = (l[13] * l[23]);
    let v16: f32 = (l[19] * l[22]);
    let v17: f32 = (l[13] * l[24]);
    let v18: f32 = (l[18] * l[19]);
    let v19: f32 = (((((l[18] * v14) + ((l[15] * v15) + (l[12] * v16))) - (l[12] * v17)) - (l[23] * v18)) - (l[16] * v12));
    let v20: f32 = ((((v7 * l[21]) + (v13 * l[25])) + (v19 * l[26])) + (l[27] * (-((((((l[20] * v16) + (l[14] * v15)) + (l[17] * v14)) - (l[20] * v17)) - (v4 * l[23])) - (l[16] * v8)))));
    let c21: bool = (!(v20 == K_1521690));
    let v22: f32 = (1.0_f32 / v20);
    let v23: f32 = (l[20] * l[25]);
    let v24: f32 = (l[12] * l[17]);
    let v25: f32 = (if c21 { (-(v22 * ((((((l[27] * v2) + (l[18] * v23)) + (l[12] * (l[13] * l[26]))) - (l[27] * (l[13] * l[20]))) - (l[25] * v24)) - (l[26] * (l[18] * l[16]))))) } else { K_16295F0 });
    let v26: f32 = (l[12] - l[29]);
    let v27: f32 = (l[27] - l[28]);
    let v28: f32 = (l[18] - l[30]);
    let v29: f32 = ((l[28] + (((v26 * l[4]) + (v27 * l[0])) + (v28 * l[8]))) + (((l[31] * l[32]) + (l[33] * l[34])) + (l[35] * l[36])));
    let v30: f32 = (if c21 { (v7 * v22) } else { K_16295C0 });
    let v31: f32 = ((l[29] + (((v26 * l[5]) + (v27 * l[1])) + (v28 * l[9]))) + (((l[37] * l[32]) + (l[38] * l[34])) + (l[39] * l[36])));
    let v32: f32 = (l[19] * l[26]);
    let v33: f32 = (if c21 { (-(v22 * (((((l[18] * v32) + ((l[27] * v1) + (l[15] * (l[17] * l[25])))) - (l[27] * v4)) - (v5 * l[25])) - (v6 * l[26])))) } else { K_16295D0 });
    let v34: f32 = ((l[30] + (((v26 * l[6]) + (v27 * l[2])) + (v28 * l[10]))) + (((l[40] * l[32]) + (l[41] * l[34])) + (l[42] * l[36])));
    let v35: f32 = (l[12] * l[14]);
    let v36: f32 = (if c21 { (v22 * ((((((l[15] * v23) + (l[27] * (l[16] * l[14]))) + (l[12] * v32)) - (l[27] * v3)) - (l[25] * v35)) - (l[26] * (l[16] * l[15])))) } else { K_16295E0 });
    let v37: f32 = (l[20] * l[21]);
    let v38: f32 = (l[18] * l[23]);
    let v39: f32 = (if c21 { (v22 * ((((((l[18] * v37) + (l[27] * v9)) + (l[12] * (l[22] * l[26]))) - (l[27] * (l[22] * l[20]))) - (l[21] * v24)) - (l[26] * v38))) } else { K_16295F4 });
    let v40: f32 = (if c21 { (v13 * v22) } else { K_16295C4 });
    let v41: f32 = (l[24] * l[26]);
    let v42: f32 = (if c21 { (v22 * ((((((l[15] * (l[17] * l[21])) + (l[27] * v8)) + (l[18] * v41)) - (l[27] * v11)) - (v5 * l[21])) - (v12 * l[26]))) } else { K_16295D4 });
    let v43: f32 = (l[15] * l[23]);
    let v44: f32 = (if c21 { (-(v22 * (((((l[12] * v41) + ((l[15] * v37) + (l[27] * (l[14] * l[23])))) - (l[27] * v10)) - (l[21] * v35)) - (l[26] * v43)))) } else { K_16295E4 });
    let v45: f32 = (l[16] * l[21]);
    let v46: f32 = (if c21 { (-(v22 * ((((((l[27] * v15) + (l[18] * v45)) + (l[12] * (l[22] * l[25]))) - (l[27] * (l[16] * l[22]))) - (l[21] * (l[12] * l[13]))) - (l[25] * v38)))) } else { K_16295F8 });
    let v47: f32 = (if c21 { (v19 * v22) } else { K_16295C8 });
    let v48: f32 = (l[24] * l[25]);
    let v49: f32 = (if c21 { (-(v22 * (((((l[18] * v48) + ((l[15] * (l[13] * l[21])) + (l[27] * v16))) - (l[27] * v17)) - (v18 * l[21])) - (v12 * l[25])))) } else { K_16295D8 });
    let v50: f32 = (if c21 { (v22 * ((((((l[15] * v45) + (l[27] * (l[19] * l[23]))) + (l[12] * v48)) - (l[27] * v14)) - (l[21] * (l[12] * l[19]))) - (l[25] * v43))) } else { K_16295E8 });
    [l[0], l[1], l[2], l[3], l[4], l[5], l[6], l[7], l[8], l[9], l[10], l[11], (v25 + (((v29 * v30) + (v31 * v33)) + (v34 * v36))), (v39 + (((v29 * v40) + (v31 * v42)) + (v34 * v44))), (v46 + (((v29 * v47) + (v31 * v49)) + (v34 * v50))), 1.0_f32]
}

/// Inputs of `kernel_blend`.
pub(super) fn load_blend(m: &dyn Mem, pg: Pages, b: &Bases, s: &Matrix) -> [f32; 33] {
    let _ = (m, pg, b, s);
    [
        s[0],
        s[1],
        s[2],
        s[3],
        b.angle,
        b.other[1],
        b.other[0],
        b.other[2],
        b.other[3],
        s[9],
        s[8],
        s[10],
        s[11],
        b.other[9],
        b.other[8],
        b.other[10],
        b.other[11],
        s[5],
        s[4],
        s[6],
        s[7],
        b.other[5],
        b.other[4],
        b.other[6],
        b.other[7],
        b.other[12],
        s[12],
        b.other[13],
        s[13],
        b.other[14],
        s[14],
        b.other[15],
        s[15],
    ]
}

/// Kernel of 0x00CFDC80: blends the matrix toward the identity by the weight (rotation by a slerp-like angle scaling, translation lerp).
#[allow(clippy::all, unused_parens)]
pub(super) fn kernel_blend(l: &[f32; 33], trig: &mut dyn FnMut(bool, f32) -> f32) -> [f32; 16] {
    let v1: f32 = ((((l[0] * l[0]) + (l[1] * l[1])) + (l[2] * l[2])) + (l[3] * l[3])).sqrt();
    let c2: bool = (!(K_1407124 > v1));
    let v3: f32 = (K_1406CE0 / v1);
    let v4: f32 = (if c2 { (l[0] * v3) } else { l[0] });
    let v5: f32 = ((((l[5] * l[5]) + (l[6] * l[6])) + (l[7] * l[7])) + (l[8] * l[8])).sqrt();
    let c6: bool = (!(K_1407124 > v5));
    let v7: f32 = (K_1406CE0 / v5);
    let v8: f32 = (if c6 { (l[6] * v7) } else { l[6] });
    let v9: f32 = (K_1406CE0 - l[4]);
    let v10: f32 = ((v4 * l[4]) + (v8 * v9));
    let v11: f32 = (if c6 { (l[5] * v7) } else { l[5] });
    let v12: f32 = (if c2 { (l[1] * v3) } else { l[1] });
    let v13: f32 = ((v11 * v9) + (v12 * l[4]));
    let v14: f32 = (if c6 { (l[7] * v7) } else { l[7] });
    let v15: f32 = (if c2 { (l[2] * v3) } else { l[2] });
    let v16: f32 = ((v14 * v9) + (v15 * l[4]));
    let v17: f32 = (if c6 { (l[8] * v7) } else { l[8] });
    let v18: f32 = (if c2 { (l[3] * v3) } else { l[3] });
    let v19: f32 = ((v17 * v9) + (v18 * l[4]));
    let v20: f32 = ((((v10 * v10) + (v13 * v13)) + (v16 * v16)) + (v19 * v19)).sqrt();
    let c21: bool = (!(K_1407124 > v20));
    let v22: f32 = (K_1406CE0 / v20);
    let v23: f32 = (if c21 { (v10 * v22) } else { v10 });
    let v24: f32 = ((v1 * l[4]) + (v5 * v9));
    let v25: f32 = (if c21 { (v13 * v22) } else { v13 });
    let v26: f32 = (if c21 { (v16 * v22) } else { v16 });
    let v27: f32 = (if c21 { (v19 * v22) } else { v19 });
    let v28: f32 = ((((l[9] * l[9]) + (l[10] * l[10])) + (l[11] * l[11])) + (l[12] * l[12])).sqrt();
    let c29: bool = (!(K_1407124 > v28));
    let v30: f32 = (K_1406CE0 / v28);
    let v31: f32 = (if c29 { (l[10] * v30) } else { l[10] });
    let v32: f32 = ((((l[13] * l[13]) + (l[14] * l[14])) + (l[15] * l[15])) + (l[16] * l[16])).sqrt();
    let c33: bool = (!(K_1407124 > v32));
    let v34: f32 = (K_1406CE0 / v32);
    let v35: f32 = (if c33 { (l[14] * v34) } else { l[14] });
    let v36: f32 = ((v31 * l[4]) + (v35 * v9));
    let v37: f32 = (if c33 { (l[13] * v34) } else { l[13] });
    let v38: f32 = (if c29 { (l[9] * v30) } else { l[9] });
    let v39: f32 = ((v37 * v9) + (v38 * l[4]));
    let v40: f32 = (if c33 { (l[15] * v34) } else { l[15] });
    let v41: f32 = (if c29 { (l[11] * v30) } else { l[11] });
    let v42: f32 = ((v40 * v9) + (v41 * l[4]));
    let v43: f32 = (if c33 { (l[16] * v34) } else { l[16] });
    let v44: f32 = (if c29 { (l[12] * v30) } else { l[12] });
    let v45: f32 = ((v43 * v9) + (v44 * l[4]));
    let v46: f32 = ((((v36 * v36) + (v39 * v39)) + (v42 * v42)) + (v45 * v45)).sqrt();
    let c47: bool = (!(K_1407124 > v46));
    let v48: f32 = (K_1406CE0 / v46);
    let v49: f32 = (if c47 { (v39 * v48) } else { v39 });
    let v50: f32 = (if c47 { (v42 * v48) } else { v42 });
    let v51: f32 = ((v26 * v49) - (v25 * v50));
    let v52: f32 = (if c47 { (v36 * v48) } else { v36 });
    let v53: f32 = ((v23 * v50) - (v26 * v52));
    let v54: f32 = ((v25 * v52) - (v23 * v49));
    let v55: f32 = ((((v51 * v51) + (v53 * v53)) + (v54 * v54)) + (0.0_f32 * 0.0_f32)).sqrt();
    let c56: bool = (!(K_1407124 > v55));
    let v57: f32 = (K_1406CE0 / v55);
    let v58: f32 = (if c56 { (v51 * v57) } else { v51 });
    let v59: f32 = ((((l[17] * l[17]) + (l[18] * l[18])) + (l[19] * l[19])) + (l[20] * l[20])).sqrt();
    let v60: f32 = ((((l[21] * l[21]) + (l[22] * l[22])) + (l[23] * l[23])) + (l[24] * l[24])).sqrt();
    let v61: f32 = ((v59 * l[4]) + (v60 * v9));
    let v62: f32 = (if c56 { (v53 * v57) } else { v53 });
    let v63: f32 = (if c56 { (v54 * v57) } else { v54 });
    let v64: f32 = (if c56 { (0.0_f32 * v57) } else { 0.0_f32 });
    let v65: f32 = ((v32 * v9) + (v28 * l[4]));
    [(v23 * v24), (v25 * v24), (v26 * v24), (v27 * v24), (v58 * v61), (v62 * v61), (v63 * v61), (v64 * v61), (((v25 * v63) - (v26 * v62)) * v65), (((v26 * v58) - (v23 * v63)) * v65), (((v23 * v62) - (v25 * v58)) * v65), (0.0_f32 * v65), ((l[25] * v9) + (l[26] * l[4])), ((l[27] * v9) + (l[28] * l[4])), ((l[29] * v9) + (l[30] * l[4])), ((l[31] * v9) + (l[32] * l[4]))]
}
