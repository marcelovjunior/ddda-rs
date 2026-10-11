//! Angle helpers (radians).

use crate::matrix::{NORMALIZE_EPSILON, ONE};
use crate::scalar::F32;
use crate::types::Vec3;

/// 2π as the original stores it (0x40C90FDB), read from 0x0161C274.
pub const TWO_PI: F32 = F32::from_bits(0x40C9_0FDB);
/// π (0x40490FDB), read from 0x0150A344.
pub const PI: F32 = F32::from_bits(0x4049_0FDB);

/// The difference `a - b` of two angles, taken the short way round when
/// they have opposite signs.
///
/// If `a * b < 0`, each negative angle first gets `2π` added; if the two
/// then differ by more than `π`, the result is `2π - (b - a)` when `b > a`,
/// else `(a - b) - 2π`. In every other case (same signs, a zero, a NaN
/// product, or a gap of at most `π` after the shift) it is plain `a - b`
/// of the (possibly shifted) angles.
///
/// The original takes `a` in xmm0 and `b` in xmm1 and returns in xmm0.
// Translated from 0x007AD870 (DDDA.exe 2017-12-13 Steam), angle difference.
pub fn angle_difference(a: F32, b: F32) -> F32 {
    angle_difference_traced(a, b).result
}

/// Which way [`angle_difference`] went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnglePath {
    /// `a * b` is not negative: `a - b`.
    SameSide,
    /// Shifted, then at most `π` apart: `a' - b'`.
    Shifted,
    /// Shifted, more than `π` apart, `b' > a'`: `2π - (b' - a')`.
    WrappedUp,
    /// Shifted, more than `π` apart, otherwise: `(a' - b') - 2π`.
    WrappedDown,
}

/// [`angle_difference`] with its path and the shifted angles `a'`, `b'`
/// (equal to `a`, `b` on [`AnglePath::SameSide`]). The ABI adapter
/// rebuilds the original's scratch registers from these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngleTrace {
    pub result: F32,
    pub path: AnglePath,
    pub a: F32,
    pub b: F32,
}

pub fn angle_difference_traced(a: F32, b: F32) -> AngleTrace {
    let zero = F32::ZERO;
    if !(zero > a * b) {
        return AngleTrace { result: a - b, path: AnglePath::SameSide, a, b };
    }
    let a = if zero > a { a + TWO_PI } else { a };
    let b = if zero > b { b + TWO_PI } else { b };
    let gap = a - b;
    let (result, path) = if !(gap.abs() > PI) {
        (gap, AnglePath::Shifted)
    } else if b > a {
        (TWO_PI - (b - a), AnglePath::WrappedUp)
    } else {
        (gap - TWO_PI, AnglePath::WrappedDown)
    };
    AngleTrace { result, path, a, b }
}

/// 1/(2π) (0x3E22F983), read from 0x0161C548.
pub const INV_TWO_PI: F32 = F32::from_bits(0x3E22_F983);

/// `cvttss2si`: truncation toward zero; NaN and out-of-range values give
/// `i32::MIN` (Rust's `as i32` would saturate instead).
pub fn truncate(v: F32) -> i32 {
    let x = v.0;
    if x.is_nan() || x >= 2_147_483_648.0 || x < -2_147_483_648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// What [`wrap_angle`] computes besides its result: the whole turns it
/// removed (EAX in the original).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wrapped {
    pub result: F32,
    pub turns: i32,
}

/// Brings an angle into `[-π, π)`: removes whole turns (truncated toward
/// zero), adds `2π` if the angle was not `>= 0`, then subtracts `2π` if the
/// result is `>= π`. A NaN stays NaN. The shared tail of 0x007AD9D0 and
/// 0x007AD7A0.
pub fn wrap_angle(a: F32) -> Wrapped {
    let turns = truncate(a * INV_TWO_PI);
    let mut r = a - F32(turns as f32) * TWO_PI;
    if !(a >= F32::ZERO) {
        r = r + TWO_PI;
    }
    if r >= PI {
        r = r - TWO_PI;
    }
    Wrapped { result: r, turns }
}

/// The yaw (rotation about y, 0 along +z, π/2 along +x) of the direction
/// from `from` to `to`, in `[-π, π)`: `atan2(dx, dz)` in double precision,
/// rounded to float, then [`wrap_angle`]. `atan2` is the CRT's (0x012F1109;
/// the ABI adapter calls the lifted CRT body).
///
/// The original takes `from` in ECX and `to` in EAX.
// Translated from 0x007AD9D0 (DDDA.exe 2017-12-13 Steam), yaw between two points.
pub fn yaw_between(from: Vec3, to: Vec3, atan2: impl FnOnce(f64, f64) -> f64) -> F32 {
    let (dx, dz) = (to.x - from.x, to.z - from.z);
    wrap_angle(F32(atan2(dx.0 as f64, dz.0 as f64) as f32)).result
}

/// The x and z the yaw of [`yaw_of_direction`] is taken of: `from - to`,
/// normalized as a 3D vector unless shorter than FLT_EPSILON (only x and z
/// are scaled; y only counts toward the length).
pub fn direction_xz(from: Vec3, to: Vec3) -> (F32, F32) {
    let d = Vec3::new(from.x - to.x, from.y - to.y, from.z - to.z);
    let len = d.length();
    if NORMALIZE_EPSILON > len {
        return (d.x, d.z);
    }
    let inv = ONE / len;
    (inv * d.x, d.z * inv)
}

/// The yaw of the direction from `to` to `from` (note the order), taken
/// after normalizing it ([`direction_xz`]): `atan2(nx, nz)`, then
/// [`wrap_angle`]. Normalizing changes the result only through rounding.
///
/// The original takes `from` in ECX and `to` in EAX.
// Translated from 0x007AD7A0 (DDDA.exe 2017-12-13 Steam), yaw of a normalized direction.
pub fn yaw_of_direction(from: Vec3, to: Vec3, atan2: impl FnOnce(f64, f64) -> f64) -> F32 {
    let (x, z) = direction_xz(from, to);
    wrap_angle(F32(atan2(x.0 as f64, z.0 as f64) as f32)).result
}
