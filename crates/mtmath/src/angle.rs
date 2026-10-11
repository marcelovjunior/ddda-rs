//! Angle helpers (radians).

use crate::scalar::F32;

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
