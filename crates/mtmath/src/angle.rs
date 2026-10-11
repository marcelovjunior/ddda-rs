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
    angle_difference_path(a, b).0
}

/// Which way [`angle_difference`] went: the original leaves xmm0's upper
/// lanes as they came in on every path except the `2π - (b - a)` one,
/// which zeroes them. The ABI adapter needs this; callers of the readable
/// function do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnglePath {
    /// The result came from `a - b` or `(a - b) - 2π`.
    Difference,
    /// The result is `2π - (b - a)`.
    WrappedUp,
}

pub fn angle_difference_path(mut a: F32, mut b: F32) -> (F32, AnglePath) {
    let zero = F32::ZERO;
    if zero > a * b {
        if zero > a {
            a = a + TWO_PI;
        }
        if zero > b {
            b = b + TWO_PI;
        }
        let gap = a - b;
        if gap.abs() > PI {
            if b > a {
                return (TWO_PI - (b - a), AnglePath::WrappedUp);
            }
            return (gap - TWO_PI, AnglePath::Difference);
        }
    }
    (a - b, AnglePath::Difference)
}
