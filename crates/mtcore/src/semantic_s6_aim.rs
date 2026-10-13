//! L184 s6: semantic port of the motion filter's "aim and twist" pose
//! builder (0x00829EC0), the callee of `uCnsMotFilter::vf24` for pose
//! kinds 6, 10, 14 and 18 ([`crate::semantic_s6`]).
//!
//! // Translated from 0x00829EC0 (DDDA.exe 2017-12-13 Steam)
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

use crate::semantic::call_from;
use crate::semantic_s6::{invert4, Mat4};
use crate::semantic_s6_pose::{crt_f32_at, IDENTITY};

/// FLT_EPSILON; rotations smaller than this are skipped.
const MIN_ANGLE: f32 = 1.192_092_9e-7;
/// Degrees to radians (0x0161C22C).
const DEGREES_TO_RADIANS: f32 = 0.017_453_292;

/// What each pose kind does: the factors the source's angle (degrees)
/// is multiplied by to get the swing angle about the direction to the
/// partner joint and the roll angle about Z (tables at 0x01832D10, per
/// kind 6, 10, 14, 18; the middle factor of each table is 0 and drives
/// code that can never run).
fn angle_factors(kind: u8) -> Option<(f32, f32)> {
    match kind {
        6 => Some((-10.0, -2.0)),
        10 => Some((10.0, 2.0)),
        14 => Some((15.0, -2.0)),
        18 => Some((-15.0, 2.0)),
        _ => None,
    }
}

/// 0x00829EC0, `joint.build_aim_pose(source)` (custom convention: ECX =
/// the joint record, ESI = the output matrix (16 f32), EDI = the model,
/// one stack argument, `ret 4`; EAX = ESI on return).
///
/// Writes to ESI the identity, turned by the swing rotation about the
/// direction from the joint to its partner joint (the model's joint for
/// bone `kind + 2`, found through the byte table at `model + 0x368` and
/// the joint array at `model + 0x364`) expressed in the joint's own frame,
/// by the angle `source.angle_degrees (+0x538) * factor`, then by the roll
/// rotation about Z. With any other kind byte (+0x93) the identity is the
/// result.
pub fn aim_pose(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let source = pg.u32(m, entry.wrapping_add(4));
    // (the source is read first only when the kind has angles; the
    // matrix function reads the same values in the original's order)
    let pose = aim_pose_matrix(m, c, pg, entry, c.ecx, source, c.edi);
    for (i, v) in pose.iter().enumerate() {
        pg.set_u32(m, c.esi.wrapping_add(4 * i as u32), v.to_bits());
    }
    c.eax = c.esi;
    c.esp = entry.wrapping_add(4 + 4); // ret 4
}

/// The pose matrix of [`aim_pose`] for `joint` (ECX) and `source` (the
/// stack argument) and `model`; `entry` is the routine's ESP at
/// entry (its return address on top).
pub fn aim_pose_matrix(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, entry: u32, joint: u32, source: u32, model: u32) -> Mat4 {
    let f32_at = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
    let mut pose = IDENTITY;
    if let Some((swing_factor, roll_factor)) = angle_factors(pg.u8(m, joint.wrapping_add(0x93))) {
        let radians = f32_at(m, source.wrapping_add(0x538)) * DEGREES_TO_RADIANS;
        let swing = radians * swing_factor;
        let roll = radians * roll_factor;
        // The original's frame at its calls: EBP = entry - 4, aligned
        // down to 16, 0x110 bytes of locals.
        let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x110);
        let saved_ebp = c.ebp;
        c.ebp = entry.wrapping_sub(4);
        let crt = |m: &mut dyn Mem, c: &mut Cpu, target: u32, back: u32, x: f32| crt_f32_at(m, c, pg, frame, target, back, x);
        if swing > MIN_ANGLE {
            if let Some(partner) = partner_joint(m, pg, joint, model) {
                let axis = axis_in_joint_frame(m, pg, joint, partner);
                let half = swing * 0.5;
                let sin = crt(m, c, 0x012f_0906, 0x0082_a9f4, half);
                let cos = crt(m, c, 0x012f_0a55, 0x0082_aa32, half);
                pose = rotation_from_axis_sin_cos(axis, sin, cos);
            }
        }
        if roll > MIN_ANGLE {
            let sin = crt(m, c, 0x012f_0906, 0x0082_afc6, roll);
            let cos = crt(m, c, 0x012f_0a55, 0x0082_afda, roll);
            pose = rotate_about_z(&pose, sin, cos);
        }
        c.ebp = saved_ebp;
    }
    pose
}

/// The model's joint record for bone `kind + 2` (0 / 0xff: none).
fn partner_joint(m: &dyn Mem, pg: Pages, joint: u32, model: u32) -> Option<u32> {
    let bone_table = pg.u32(m, model.wrapping_add(0x368));
    let bone = pg.u8(m, joint.wrapping_add(0x93)).wrapping_add(2);
    let index = pg.u8(m, bone_table.wrapping_add(bone as u32));
    if index == 0xff {
        return None;
    }
    let record = (index as u32 * 0xa0).wrapping_add(pg.u32(m, model.wrapping_add(0x364)));
    (record != 0).then_some(record)
}

/// The unit direction from `joint` to `partner` (world positions at
/// `+0x40`), transformed into the joint's own frame with the inverse of
/// the joint's world matrix (`+0x10`; the identity when it is singular)
/// and normalised again. Lengths below [`MIN_ANGLE`] are not normalised.
fn axis_in_joint_frame(m: &dyn Mem, pg: Pages, joint: u32, partner: u32) -> [f32; 3] {
    let f32_at = |a: u32| f32::from_bits(pg.u32(m, a));
    let delta = [
        f32_at(partner.wrapping_add(0x40)) - f32_at(joint.wrapping_add(0x40)),
        f32_at(partner.wrapping_add(0x44)) - f32_at(joint.wrapping_add(0x44)),
        f32_at(partner.wrapping_add(0x48)) - f32_at(joint.wrapping_add(0x48)),
    ];
    let direction = normalised(delta, ((delta[1] * delta[1]) + (delta[0] * delta[0])) + (delta[2] * delta[2]));
    let world: Mat4 = std::array::from_fn(|i| f32_at(joint.wrapping_add(0x10 + 4 * i as u32)));
    let inverse = invert4(&world).unwrap_or(IDENTITY);
    let local: [f32; 3] = std::array::from_fn(|k| {
        ((inverse[k] * direction[0]) + (inverse[4 + k] * direction[1])) + (inverse[8 + k] * direction[2])
    });
    normalised(local, ((local[0] * local[0]) + (local[1] * local[1])) + (local[2] * local[2]))
}

/// `v` scaled to unit length given its squared length, unless the length
/// is below the epsilon (a NaN length scales).
fn normalised(v: [f32; 3], length_squared: f32) -> [f32; 3] {
    let length = length_squared.sqrt();
    if MIN_ANGLE > length {
        return v;
    }
    let scale = 1.0 / length;
    [scale * v[0], v[1] * scale, v[2] * scale]
}

/// The rotation matrix of the quaternion (axis * sin, cos), in the layout
/// this builder uses (row-major, the factor 2 applied after the sums).
fn rotation_from_axis_sin_cos(axis: [f32; 3], sin: f32, cos: f32) -> Mat4 {
    let (x, y, z, w) = (sin * axis[0], sin * axis[1], sin * axis[2], cos);
    [
        1.0 - (((y * y) + (z * z)) * 2.0),
        ((x * y) + (w * z)) * 2.0,
        ((x * z) - (w * y)) * 2.0,
        0.0,
        ((x * y) - (w * z)) * 2.0,
        1.0 - (((x * x) + (z * z)) * 2.0),
        ((z * y) + (w * x)) * 2.0,
        0.0,
        ((w * y) + (x * z)) * 2.0,
        ((z * y) - (w * x)) * 2.0,
        1.0 - (((x * x) + (y * y)) * 2.0),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// `rotation_z(sin, cos) * m`, with the original's terms for the zero and
/// one entries of the rotation (they matter only for infinities and NaNs).
fn rotate_about_z(m: &Mat4, sin: f32, cos: f32) -> Mat4 {
    let mut out = [0.0f32; 16];
    for c in 0..4 {
        let (r0, r1, r2, r3) = (m[c], m[4 + c], m[8 + c], m[12 + c]);
        out[c] = (((r0 * cos) + (r1 * sin)) + (r2 * 0.0)) + (r3 * 0.0);
        out[4 + c] = (((r0 * -sin) + (r1 * cos)) + (r2 * 0.0)) + (r3 * 0.0);
        out[8 + c] = (r2 + ((r0 + r1) * 0.0)) + (r3 * 0.0);
        out[12 + c] = ((r2 * 0.0) + ((r0 + r1) * 0.0)) + r3;
    }
    out
}
