//! Vector helpers that the original keeps out of line.

use crate::matrix::{NORMALIZE_EPSILON, ONE};
use crate::scalar::F32;
use crate::types::Vec3;

/// `b x a`, component by component in the original's operand order.
pub fn cross_ba(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.z * b.y - a.y * b.z, a.x * b.z - b.x * a.z, b.x * a.y - a.x * b.y)
}

/// `(x*x + y*y) + z*z`, in that order (unlike `Vec3::length`, which starts
/// with y).
pub fn squared_length_xyz(v: Vec3) -> F32 {
    v.x * v.x + v.y * v.y + v.z * v.z
}

/// What [`unit_normal`] computed: the first cross product, the fallback
/// one if it was needed, and the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalTrace {
    pub main: Vec3,
    pub fallback: Option<Vec3>,
    pub result: Option<Vec3>,
}

/// The unit normal of `b x a`; if that cross product's squared length is
/// below FLT_EPSILON, of the fallback pair's `e x d` instead (the pair is
/// only read then); `None` if that one is short too. The chosen vector is
/// scaled by `1 / length` unless its length (not squared) is below
/// FLT_EPSILON. A NaN squared length counts as long enough.
///
/// The original takes `a` in EAX, `b` on the stack, `d` in EDX and `e` in
/// ESI, writes the normal (w = 0) through EDI and returns success in AL.
// Translated from 0x007AEED0 (DDDA.exe 2017-12-13 Steam), unit normal with fallback.
pub fn unit_normal(a: Vec3, b: Vec3, fallback: impl FnOnce() -> (Vec3, Vec3)) -> NormalTrace {
    let main = cross_ba(a, b);
    let (chosen, fallback) = if NORMALIZE_EPSILON > squared_length_xyz(main) {
        let (d, e) = fallback();
        let f = cross_ba(d, e);
        if NORMALIZE_EPSILON > squared_length_xyz(f) {
            return NormalTrace { main, fallback: Some(f), result: None };
        }
        (f, Some(f))
    } else {
        (main, None)
    };
    let len = squared_length_xyz(chosen).sqrt();
    let result = if NORMALIZE_EPSILON > len {
        chosen
    } else {
        let inv = ONE / len;
        Vec3::new(inv * chosen.x, chosen.y * inv, chosen.z * inv)
    };
    NormalTrace { main, fallback, result: Some(result) }
}

/// `z x x`, normalized as `Vec3::length` measures unless shorter than
/// FLT_EPSILON (the y axis of [`basis_from_forward`]).
pub fn basis_y(z: Vec3, x: Vec3) -> Vec3 {
    let y = Vec3::new(z.y * x.z - z.z * x.y, z.z * x.x - z.x * x.z, z.x * x.y - z.y * x.x);
    let len = y.length();
    if NORMALIZE_EPSILON > len {
        return y;
    }
    let inv = ONE / len;
    Vec3::new(inv * y.x, y.y * inv, y.z * inv)
}

/// `z` normalized unless shorter than FLT_EPSILON (the z axis of
/// [`basis_from_forward`]).
pub fn basis_z(z: Vec3) -> Vec3 {
    let len = z.length();
    if NORMALIZE_EPSILON > len {
        return z;
    }
    let inv = ONE / len;
    Vec3::new(z.x * inv, z.y * inv, z.z * inv)
}

/// An orthonormal frame around the forward vector `z`: row 0 is the unit
/// normal of `hint x z` ([`unit_normal`], falling back to `e x z` when
/// `hint` is parallel to `z`), row 1 [`basis_y`], row 2 [`basis_z`], row 3
/// the translation `t`; `None` if both normals fail (nothing is written
/// then except row 0's attempt).
///
/// The original reads `z` again after writing rows 0 and 1, so when `z`
/// or `t` points into the output those reads see the new rows; the ABI
/// adapter keeps that order. It takes `z` in EDX, `hint` in EAX, `e` in
/// ESI, the output and `t` on the stack.
// Translated from 0x007AF5B0 (DDDA.exe 2017-12-13 Steam), frame from a forward vector.
pub fn basis_from_forward(z: Vec3, hint: Vec3, e: Vec3, t: Vec3) -> Option<crate::types::Mat44> {
    let x = unit_normal(z, hint, || (z, e)).result?;
    let (y, z) = (basis_y(z, x), basis_z(z));
    let o = F32::ZERO;
    Some(crate::types::Mat44 {
        m: [[x.x, x.y, x.z, o], [y.x, y.y, y.z, o], [z.x, z.y, z.z, o], [t.x, t.y, t.z, ONE]],
    })
}
