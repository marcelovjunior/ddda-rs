//! `MtQuaternion` helpers that the original keeps out of line.

use crate::matrix::{HALF, MINUS_ONE, NORMALIZE_EPSILON, ONE, TWO};
use crate::scalar::{x87_copy, F32};

/// `MtQuaternion`: x, y, z (the vector part) and w (16 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Quat {
    /// +0x00
    pub x: F32,
    /// +0x04
    pub y: F32,
    /// +0x08
    pub z: F32,
    /// +0x0C
    pub w: F32,
}

impl Quat {
    pub const fn new(x: F32, y: F32, z: F32, w: F32) -> Quat {
        Quat { x, y, z, w }
    }
}

/// The global quaternion [`rotate_toward`] uses as the inverse of a zero
/// quaternion (0x018CFF10; all zero in the exe's image).
pub const FALLBACK_INVERSE: u32 = 0x018C_FF10;

/// The CRT routines [`rotate_toward`] calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuatCall {
    /// acosf (0x012F161F)
    Acos,
    /// cosf (0x012F0A55)
    Cos,
    /// sinf (0x012F0906)
    Sin,
}

/// The inverse `conj(q) / |q|²`; `fallback` when `|q|²` is not above 0.
pub fn inverse_or(q: Quat, fallback: Quat) -> Quat {
    let n = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if !(n > F32::ZERO) {
        return fallback;
    }
    let inv = ONE / n;
    Quat::new(-(q.x * inv), -(inv * q.y), -(inv * q.z), q.w * inv)
}

/// The product `t * r` (Hamilton), in the original's operation order.
pub fn relative(t: Quat, r: Quat) -> Quat {
    Quat::new(
        t.x * r.w + t.w * r.x + t.y * r.z - t.z * r.y,
        t.w * r.y + t.y * r.w + t.z * r.x - t.x * r.z,
        t.z * r.w + t.w * r.z + t.x * r.y - t.y * r.x,
        t.w * r.w - t.x * r.x - t.y * r.y - t.z * r.z,
    )
}

/// `p` or `-p`, whichever has `w >= 0` (the shorter way round); the vector
/// part is multiplied by -1, w has its sign flipped. Also says whether it
/// flipped.
pub fn shortest(p: Quat) -> (Quat, bool) {
    if F32::ZERO > p.w {
        (Quat::new(p.x * MINUS_ONE, p.y * MINUS_ONE, p.z * MINUS_ONE, -p.w), true)
    } else {
        (p, false)
    }
}

/// `w` clamped to `[-1, 1]` (NaN passes through).
pub fn clamp_unit(w: F32) -> F32 {
    if w >= ONE {
        ONE
    } else if MINUS_ONE >= w {
        MINUS_ONE
    } else {
        w
    }
}

/// The step to take: `max_angle` floored at 0, and the full angle
/// `2 acos(w)` limited to it. Returns (floored max, step).
pub fn step(acos_w: F32, max_angle: F32) -> (F32, F32) {
    let full = acos_w * TWO;
    let max = if F32::ZERO > max_angle { F32::ZERO } else { max_angle };
    // a NaN full angle is kept (the original's jb is taken when unordered)
    (max, if !(full >= max) { full } else { max })
}

/// The unit rotation by `step` about `p`'s axis: `(axis sin(step/2),
/// cos(step/2))` normalized, from `sin` and `cos` of `step/2` (`sin` only
/// called if the axis is longer than sqrt(FLT_EPSILON); else the axis is
/// zero).
pub fn rotation(p: Quat, half: F32, cos_half: F32, sin: impl FnOnce(F32) -> F32) -> Quat {
    let v2 = p.y * p.y + p.x * p.x + p.z * p.z;
    let (vx, vy, vz) = if v2 > NORMALIZE_EPSILON {
        let k = ONE / v2.sqrt() * sin(half);
        (k * p.x, k * p.y, k * p.z)
    } else {
        (F32::ZERO, F32::ZERO, F32::ZERO)
    };
    let j = ONE / (vy * vy + vx * vx + vz * vz + cos_half * cos_half).sqrt();
    Quat::new(j * vx, j * vy, j * vz, j * cos_half)
}

/// The product `q * s`, in the original's operation order.
pub fn product(q: Quat, s: Quat) -> Quat {
    Quat::new(
        q.x * s.w + q.w * s.x + q.z * s.y - q.y * s.z,
        q.w * s.y + q.y * s.w + q.x * s.z - q.z * s.x,
        q.z * s.w + q.w * s.z + q.y * s.x - q.x * s.y,
        q.w * s.w - q.x * s.x - q.y * s.y - q.z * s.z,
    )
}

/// `q` turned toward `target` by at most `max_angle` radians: the relative
/// rotation `target * q⁻¹` (taken the short way round) has angle
/// `2 acos(w)`; if `max_angle` (floored at 0) is larger, the result is
/// `target` itself (y, z and w copied through the x87); otherwise `q` is
/// multiplied by the rotation of `max_angle` about the relative rotation's
/// axis. `fallback_inverse` replaces `q⁻¹` when `q` is zero (the global at
/// [`FALLBACK_INVERSE`]). `crt` gives acosf, cosf and sinf, called in that
/// order (sinf only for a long enough axis).
///
/// The original takes `q` in ESI, writes through EDI (returned in EAX),
/// and takes `target` and `max_angle` on the stack.
// Translated from 0x007AE860 (DDDA.exe 2017-12-13 Steam), quaternion rotated toward a target.
pub fn rotate_toward(q: Quat, target: Quat, max_angle: F32, fallback_inverse: Quat,
                     mut crt: impl FnMut(QuatCall, F32) -> F32) -> Quat {
    let r = inverse_or(q, fallback_inverse);
    let (p, _) = shortest(relative(target, r));
    let (max, step) = step(crt(QuatCall::Acos, clamp_unit(p.w)), max_angle);
    if max > step {
        let t = target;
        return Quat::new(t.x, F32(x87_copy(t.y.0)), F32(x87_copy(t.z.0)), F32(x87_copy(t.w.0)));
    }
    let half = step * HALF;
    let cos_half = crt(QuatCall::Cos, half);
    product(q, rotation(p, half, cos_half, |h| crt(QuatCall::Sin, h)))
}
