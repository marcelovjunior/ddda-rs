//! L184 s6: the motion filter's pose builders, folded into the filter
//! ([`crate::semantic_s6`]) as plain Rust.
//!
//! A filter of pose kind 1, 2, 3, 5, 9, 16 or 20 builds its pose matrix
//! from the pose source's two angles (the floats at `source + 0x538` and
//! `+0x53c`, degrees): the original calls one of seven small routines
//! (0x00828BD0, 0x00828EC0, 0x008291B0, 0x008293F0, 0x008296F0,
//! 0x00829A00, 0x00829C60) that write the matrix to a stack buffer. Four of
//! them (kinds 1, 2, 5, 9) first turn the joint's rotation quaternion
//! (`joint + 0x60`) into Euler angles with 0x007E4B60 (quaternion to
//! matrix) and 0x00CFC9E0 (matrix to Euler angles); those are folded too.
//! Only the CRT calls stay guest calls: sinf 0x012F0906, cosf 0x012F0A55,
//! asinf 0x012F0D4C and the double-precision atan2 0x012F1109 (their last
//! bits are the game's). The CRT's sin(0.0) and cos(0.0) in double
//! precision (0x012F1E8A, 0x012F2042) that kinds 3, 16 and 20 call return
//! exactly 0.0 and 1.0 (checked by running them), so they are constants.
//!
//! Translated from 0x00828BD0, 0x00828EC0, 0x008291B0, 0x008293F0,
//! 0x008296F0, 0x00829A00, 0x00829C60, 0x007E4B60 and 0x00CFC9E0
//! (DDDA.exe 2017-12-13 Steam).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

use crate::semantic::call_from;
use crate::semantic_s6::Mat4;

/// Degrees to radians (0x0161C22C).
pub(crate) const DEGREES_TO_RADIANS: f32 = f32::from_bits(0x3c8e_fa35);
/// Radians to degrees (0x0161C208).
const RADIANS_TO_DEGREES: f32 = f32::from_bits(0x4265_2ee0);
pub(crate) const IDENTITY: Mat4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// What the pose routines need from the filter's call.
#[derive(Clone, Copy)]
pub struct PoseCall {
    pub pages: Pages,
    /// The joint record (the filter's `joint`; quaternion at `+0x60`).
    pub joint: u32,
    /// The pose source (the filter's `+0x70`): angles at `+0x538`, `+0x53c`.
    pub source: u32,
    /// The model (the filter's second argument).
    pub model: u32,
    /// The ESP of the filter at its calls to the routines (the return
    /// address is pushed below it).
    pub frame: u32,
}

/// Whether a pose kind (the filter's `+0x68`) has a routine. The original
/// dispatches through a 20-entry byte table at 0x00874D64.
pub fn has_pose_routine(kind: u32) -> bool {
    matches!(kind, 1 | 2 | 3 | 5 | 6 | 9 | 10 | 14 | 16 | 18 | 20)
}

/// The pose matrix of `kind` (which must satisfy [`has_pose_routine`]).
pub fn build_pose(m: &mut dyn Mem, c: &mut Cpu, call: &PoseCall, kind: u32) -> Mat4 {
    let angle = |m: &dyn Mem, offset: u32| f32::from_bits(call.pages.u32(m, call.source.wrapping_add(offset)));
    // The routine's frame: its return address, EBP and the locals below.
    let base = |locals: u32| (call.frame.wrapping_sub(8) & !15).wrapping_sub(locals);
    let saved_ebp = c.ebp;
    c.ebp = call.frame.wrapping_sub(8);
    let pose = match kind {
        1 | 2 | 5 | 9 => {
            let base = base(0x70);
            let (swing, twist) = (angle(m, 0x538), angle(m, 0x53c));
            // identity unless the source has an angle (NaN counts)
            if swing == 0.0 && twist == 0.0 {
                IDENTITY
            } else {
                let euler = joint_euler_degrees(m, call);
                let (swing, twist) = (angle(m, 0x538), angle(m, 0x53c));
                let angles = match kind {
                    1 | 2 => {
                        let scale = if kind == 1 { 7.0 } else { 10.0 };
                        let factor = if swing > 0.0 { -0.5 } else { -0.3 };
                        [
                            (twist * scale) * DEGREES_TO_RADIANS,
                            ((swing * (euler[1] * RADIANS_TO_DEGREES)) * factor) * DEGREES_TO_RADIANS,
                            ((swing * (euler[2] * RADIANS_TO_DEGREES)) * -0.5) * DEGREES_TO_RADIANS,
                        ]
                    }
                    5 => [
                        ((swing * (euler[0] * RADIANS_TO_DEGREES)) * -0.3) * DEGREES_TO_RADIANS,
                        (((swing * (euler[1] * RADIANS_TO_DEGREES)) * -0.3) + (twist * 10.0)) * DEGREES_TO_RADIANS,
                        ((swing * (euler[2] * RADIANS_TO_DEGREES)) * -0.3) * DEGREES_TO_RADIANS,
                    ],
                    _ => [
                        (((swing * (euler[0] * RADIANS_TO_DEGREES)) * -0.3) + (twist * 10.0)) * DEGREES_TO_RADIANS,
                        (((swing * (euler[1] * RADIANS_TO_DEGREES)) * -0.3) + (twist * -10.0)) * DEGREES_TO_RADIANS,
                        ((swing * (euler[2] * RADIANS_TO_DEGREES)) * -0.3) * DEGREES_TO_RADIANS,
                    ],
                };
                euler_matrix_of_angles(m, c, call, base, kind_calls(kind), angles)
            }
        }
        3 | 16 | 20 => {
            let (locals, calls, offset, factor) = match kind {
                3 => (0x20, (0x0082_92bc, 0x0082_92e4), 0x53c, -10.0),
                16 => (0x40, (0x0082_9b2a, 0x0082_9b5f), 0x538, -20.0),
                _ => (0x40, (0x0082_9d8a, 0x0082_9dbf), 0x538, 20.0),
            };
            let base = base(locals);
            let value = angle(m, offset);
            if value == 0.0 {
                IDENTITY
            } else {
                let radians = (value * factor) * DEGREES_TO_RADIANS;
                let sin = crt_f32(m, c, call, base, 0x012f_0906, calls.0, radians);
                let cos = crt_f32(m, c, call, base, 0x012f_0a55, calls.1, radians);
                // the CRT's double sin(0.0) = 0.0 and cos(0.0) = 1.0
                let (zero, one) = (0.0f32, 1.0f32);
                if kind == 3 {
                    euler_matrix(sin, zero, zero, cos, one, one)
                } else {
                    euler_matrix(zero, sin, zero, one, cos, one)
                }
            }
        }
        6 | 10 | 14 | 18 => crate::semantic_s6_aim::aim_pose_matrix(m, c, call.pages, call.frame.wrapping_sub(8), call.joint, call.source, call.model),
        _ => unreachable!("no pose routine for kind {kind}"),
    };
    c.ebp = saved_ebp;
    pose
}

/// The (return address after sinf, ... after cosf) of the six trig calls
/// of the Euler routines, per kind.
fn kind_calls(kind: u32) -> [u32; 6] {
    match kind {
        1 => [0x0082_8d78, 0x0082_8d89, 0x0082_8d9a, 0x0082_8daa, 0x0082_8dbb, 0x0082_8dcc],
        2 => [0x0082_9068, 0x0082_9079, 0x0082_908a, 0x0082_909a, 0x0082_90ab, 0x0082_90bc],
        5 => [0x0082_959e, 0x0082_95af, 0x0082_95c0, 0x0082_95d0, 0x0082_95e1, 0x0082_95f2],
        _ => [0x0082_98aa, 0x0082_98bb, 0x0082_98cc, 0x0082_98dc, 0x0082_98ed, 0x0082_98fe],
    }
}

/// sin and cos of the three angles (sines first, as the original calls
/// them) and the rotation matrix they make ([`euler_matrix`]).
fn euler_matrix_of_angles(m: &mut dyn Mem, c: &mut Cpu, call: &PoseCall, base: u32, backs: [u32; 6], angles: [f32; 3]) -> Mat4 {
    let sx = crt_f32(m, c, call, base, 0x012f_0906, backs[0], angles[0]);
    let sy = crt_f32(m, c, call, base, 0x012f_0906, backs[1], angles[1]);
    let sz = crt_f32(m, c, call, base, 0x012f_0906, backs[2], angles[2]);
    let cx = crt_f32(m, c, call, base, 0x012f_0a55, backs[3], angles[0]);
    let cy = crt_f32(m, c, call, base, 0x012f_0a55, backs[4], angles[1]);
    let cz = crt_f32(m, c, call, base, 0x012f_0a55, backs[5], angles[2]);
    euler_matrix(sx, sy, sz, cx, cy, cz)
}

/// The rotation matrix from the sines and cosines of three angles, as the
/// pose routines write it (row-major, no translation).
pub fn euler_matrix(sx: f32, sy: f32, sz: f32, cx: f32, cy: f32, cz: f32) -> Mat4 {
    [
        cz * cy,
        cy * sz,
        -sy,
        0.0,
        ((cz * sy) * sx) - (cx * sz),
        (cx * cz) + ((sy * sz) * sx),
        sx * cy,
        0.0,
        (sx * sz) + ((cz * sy) * cx),
        ((sy * sz) * cx) - (cz * sx),
        cx * cy,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// A single-precision CRT call from a routine whose ESP is `base`:
/// argument and result in XMM0. sinf (0x012F0906) and cosf (0x012F0A55) are
/// computed natively ([`crate::semantic_s7_trig`], bit-exact) except for
/// arguments it hands back (finite magnitudes of 8192 and more, NaN,
/// infinities), which run the guest function.
pub(crate) fn crt_f32_at(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, base: u32, target: u32, back: u32, x: f32) -> f32 {
    let native = match target {
        0x012f_0906 => crate::semantic_s7_trig::sinf(m, pg, x),
        0x012f_0a55 => crate::semantic_s7_trig::cosf(m, pg, x),
        _ => None,
    };
    if let Some(v) = native {
        return v;
    }
    c.xmm[0][0] = x.to_bits();
    call_from(m, c, pg, base, 0, &[], back, |m, c| ddnative_base::x86::call_ext(m, c, target));
    f32::from_bits(c.xmm[0][0])
}

fn crt_f32(m: &mut dyn Mem, c: &mut Cpu, call: &PoseCall, base: u32, target: u32, back: u32, x: f32) -> f32 {
    crt_f32_at(m, c, call.pages, base, target, back, x)
}

/// The joint's rotation as the Euler angles (radians converted to degrees
/// by the caller) of 0x00CFC9E0 applied to the quaternion's matrix
/// (0x007E4B60).
fn joint_euler_degrees(m: &dyn Mem, call: &PoseCall) -> [f32; 3] {
    let f32_at = |m: &dyn Mem, offset: u32| f32::from_bits(call.pages.u32(m, call.joint.wrapping_add(offset)));
    let (x, y, z, w) = (f32_at(m, 0x60), f32_at(m, 0x64), f32_at(m, 0x68), f32_at(m, 0x6c));
    let matrix = quaternion_matrix_3x3(x, y, z, w);
    euler_from_matrix(&crate::semantic_s6_crt::Tables { memory: m, pages: call.pages }, &matrix)
}

/// The elements 0, 1, 2, 4, 5, 6 and 10 of the rotation matrix of the
/// quaternion (the only ones the Euler extraction reads), indexed as the
/// 4x4 (0x007E4B60).
pub fn quaternion_matrix_3x3(x: f32, y: f32, z: f32, w: f32) -> [f32; 11] {
    let (x2, y2, z2) = (x * 2.0, y * 2.0, z * 2.0);
    let mut m = [0.0f32; 11];
    m[0] = 1.0 - ((z * z2) + (y * y2));
    m[1] = (x * y2) + (w * z2);
    m[2] = (x * z2) - (w * y2);
    m[4] = (x * y2) - (w * z2);
    m[5] = 1.0 - ((z * z2) + (x * x2));
    m[6] = (y * z2) + (w * x2);
    m[10] = 1.0 - ((y * y2) + (x * x2));
    m
}

/// Euler angles (x, y, z) of a rotation matrix (0x00CFC9E0): the usual
/// extraction with atan2 in double precision and asinf (both native, bit
/// exact: [`crate::semantic_s6_crt`]), and the gimbal cases when `m[2]`
/// reaches +-1.
pub fn euler_from_matrix(t: &crate::semantic_s6_crt::Tables, matrix: &[f32; 11]) -> [f32; 3] {
    use crate::semantic_s6_crt::{asinf, atan2};
    // CVTPS2PD of the arguments, CVTSD2SS of the result
    let atan2 = |y: f32, x: f32| -> f32 { atan2(t, y as f64, x as f64) as f32 };
    let m2 = matrix[2];
    if 1.0 > m2 {
        if m2 > -1.0 {
            [-atan2(-matrix[6], matrix[10]), -asinf(t, m2), -atan2(-matrix[1], matrix[0])]
        } else {
            [atan2(matrix[4], matrix[5]), std::f32::consts::FRAC_PI_2, 0.0]
        }
    } else {
        [-atan2(matrix[4], matrix[5]), -std::f32::consts::FRAC_PI_2, 0.0]
    }
}
