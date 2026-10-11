//! `MtMatrix` builders: projections, view (look-at) and reflection matrices
//! (the original's equivalents of the D3DX `PerspectiveRH/LH`,
//! `OrthoRH/LH`, `LookAtRH/LH` and `Reflect` builders).
//!
//! Each function writes all sixteen elements. The arithmetic is the
//! original's, operation by operation (see `crate::scalar`).

use crate::scalar::F32;
use crate::types::{Mat44, Plane, Vec3};

/// 2.0, read by the builders from 0x01406CEC.
pub const TWO: F32 = F32(2.0);
/// 1.0, read from 0x01406CE0.
pub const ONE: F32 = F32(1.0);
/// -1.0, read from 0x0161C20C.
pub const MINUS_ONE: F32 = F32(-1.0);
/// -2.0, read from 0x0161C908.
pub const MINUS_TWO: F32 = F32(-2.0);
/// Vectors shorter than this (FLT_EPSILON, 0x34000000, read from
/// 0x01407124) are left as they are instead of being normalized.
pub const NORMALIZE_EPSILON: F32 = F32::from_bits(0x3400_0000);

const O: F32 = F32::ZERO;

/// Right-handed perspective projection from the view volume's size at the
/// near plane (D3DXMatrixPerspectiveRH): x and y scale by `2 near / width`
/// and `2 near / height`, depth maps `-near..-far` to `0..1`, w = -z.
// Translated from 0x00CFC770 (DDDA.exe 2017-12-13 Steam), MtMatrix::perspectiveRH.
pub fn perspective_rh(width: F32, height: F32, near: F32, far: F32) -> Mat44 {
    let q = far / (near - far);
    let near2 = near * TWO;
    Mat44 {
        m: [
            [near2 / width, O, O, O],
            [O, near2 / height, O, O],
            [O, O, q, MINUS_ONE],
            [O, O, q * near, O],
        ],
    }
}

/// Left-handed perspective projection from the view volume's size at the
/// near plane (D3DXMatrixPerspectiveLH): depth maps `near..far` to `0..1`,
/// w = z.
// Translated from 0x00CFC810 (DDDA.exe 2017-12-13 Steam), MtMatrix::perspectiveLH.
pub fn perspective_lh(width: F32, height: F32, near: F32, far: F32) -> Mat44 {
    let near2 = near * TWO;
    Mat44 {
        m: [
            [near2 / width, O, O, O],
            [O, near2 / height, O, O],
            [O, O, far / (far - near), ONE],
            [O, O, near * far / (near - far), O],
        ],
    }
}

/// Right-handed orthographic projection (D3DXMatrixOrthoRH): x and y scale
/// by `2 / width` and `2 / height`, depth maps `-near..-far` to `0..1`.
// Translated from 0x00CFC8B0 (DDDA.exe 2017-12-13 Steam), MtMatrix::orthoRH.
pub fn ortho_rh(width: F32, height: F32, near: F32, far: F32) -> Mat44 {
    let depth = ONE / (near - far);
    Mat44 {
        m: [
            [TWO / width, O, O, O],
            [O, TWO / height, O, O],
            [O, O, depth, O],
            [O, O, depth * near, ONE],
        ],
    }
}

/// Left-handed orthographic projection (D3DXMatrixOrthoLH): depth maps
/// `near..far` to `0..1`.
// Translated from 0x00CFC940 (DDDA.exe 2017-12-13 Steam), MtMatrix::orthoLH.
pub fn ortho_lh(width: F32, height: F32, near: F32, far: F32) -> Mat44 {
    Mat44 {
        m: [
            [TWO / width, O, O, O],
            [O, TWO / height, O, O],
            [O, O, ONE / (far - near), O],
            [O, O, near / (near - far), ONE],
        ],
    }
}

/// The camera axes a look-at builder computes on the way to its matrix
/// (the ABI adapters rebuild the original's scratch registers from them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookAt {
    pub matrix: Mat44,
    /// The view direction `at - eye`, normalized unless shorter than
    /// [`NORMALIZE_EPSILON`].
    pub z: Vec3,
    /// `up x z`, normalized under the same rule.
    pub x: Vec3,
    /// Whether `x` was normalized.
    pub x_normalized: bool,
    /// `z x x`.
    pub y: Vec3,
}

/// The two look-at builders share everything up to the matrix, except for
/// the order of two products when normalizing (which only matters for
/// the payload when two different NaNs meet, see `crate::scalar`).
fn look_at_axes(eye: Vec3, at: Vec3, up: Vec3, left_handed: bool) -> (Vec3, Vec3, bool, Vec3) {
    let d = Vec3::new(at.x - eye.x, at.y - eye.y, at.z - eye.z);
    let len = d.length();
    let z = if NORMALIZE_EPSILON > len {
        d
    } else {
        let inv = ONE / len;
        let x = if left_handed { d.x * inv } else { inv * d.x };
        Vec3::new(x, inv * d.y, inv * d.z)
    };
    let c = Vec3::new(up.y * z.z - up.z * z.y, up.z * z.x - z.z * up.x, z.y * up.x - up.y * z.x);
    let len = c.length();
    let x_normalized = !(NORMALIZE_EPSILON > len);
    let x = if x_normalized {
        let inv = ONE / len;
        let cz = if left_handed { c.z * inv } else { inv * c.z };
        Vec3::new(inv * c.x, inv * c.y, cz)
    } else {
        c
    };
    let y = Vec3::new(x.z * z.y - x.y * z.z, z.z * x.x - x.z * z.x, x.y * z.x - z.y * x.x);
    (z, x, x_normalized, y)
}

/// `eye . axis` for the three translation terms, in the original's order.
fn eye_dots(eye: Vec3, x: Vec3, y: Vec3, z: Vec3) -> [F32; 3] {
    [
        eye.y * x.y + x.x * eye.x + eye.z * x.z,
        eye.y * y.y + eye.x * y.x + eye.z * y.z,
        eye.y * z.y + eye.x * z.x + eye.z * z.z,
    ]
}

/// Right-handed view matrix: the camera at `eye` looks at `at`, `up`
/// roughly up. Columns hold the negated side axis, the up axis and the
/// negated view direction, so the camera looks down its -z
/// (D3DXMatrixLookAtRH).
// Translated from 0x00CFD5E0 (DDDA.exe 2017-12-13 Steam), MtMatrix::lookAtRH.
pub fn look_at_rh(eye: Vec3, at: Vec3, up: Vec3) -> LookAt {
    let (z, x, x_normalized, y) = look_at_axes(eye, at, up, false);
    let [ex, ey, ez] = eye_dots(eye, x, y, z);
    let matrix = Mat44 {
        m: [
            [-x.x, y.x, -z.x, O],
            [-x.y, y.y, -z.y, O],
            [-x.z, y.z, -z.z, O],
            [ex, -ey, ez, ONE],
        ],
    };
    LookAt { matrix, z, x, x_normalized, y }
}

/// Left-handed view matrix (D3DXMatrixLookAtLH): columns hold the side,
/// up and view axes; the translation is minus the eye in those axes.
// Translated from 0x00CFD890 (DDDA.exe 2017-12-13 Steam), MtMatrix::lookAtLH.
pub fn look_at_lh(eye: Vec3, at: Vec3, up: Vec3) -> LookAt {
    let (z, x, x_normalized, y) = look_at_axes(eye, at, up, true);
    let [ex, ey, ez] = eye_dots(eye, x, y, z);
    let matrix = Mat44 {
        m: [
            [x.x, y.x, z.x, O],
            [x.y, y.y, z.y, O],
            [x.z, y.z, z.z, O],
            [-ex, -ey, -ez, ONE],
        ],
    };
    LookAt { matrix, z, x, x_normalized, y }
}

/// What [`reflect`] computes on the way to its matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reflection {
    pub matrix: Mat44,
    /// The plane normal, normalized unless shorter than
    /// [`NORMALIZE_EPSILON`].
    pub n: Vec3,
    /// `d` divided by the normal's original length (also when the normal
    /// was too short to normalize).
    pub d: F32,
}

/// The matrix that mirrors points in `plane` (D3DXMatrixReflect):
/// `I - 2 n n^T` with the translation `-2 d n`.
// Translated from 0x00CFDB10 (DDDA.exe 2017-12-13 Steam), MtMatrix::reflect.
pub fn reflect(plane: Plane) -> Reflection {
    let len = Vec3::new(plane.a, plane.b, plane.c).length();
    let n = if NORMALIZE_EPSILON > len {
        Vec3::new(plane.a, plane.b, plane.c)
    } else {
        let inv = ONE / len;
        Vec3::new(inv * plane.a, plane.b * inv, plane.c * inv)
    };
    let d = plane.d / len;
    let k = Vec3::new(n.x * MINUS_TWO, n.y * MINUS_TWO, n.z * MINUS_TWO);
    let matrix = Mat44 {
        m: [
            [ONE - n.x * TWO * n.x, k.y * n.x, k.z * n.x, O],
            [k.x * n.y, ONE - n.y * TWO * n.y, k.z * n.y, O],
            [k.x * n.z, k.y * n.z, ONE - n.z * TWO * n.z, O],
            [k.x * d, k.y * d, k.z * d, ONE],
        ],
    };
    Reflection { matrix, n, d }
}

/// 0.5, read from 0x01406CF0.
pub const HALF: F32 = F32(0.5);

/// The global matrix the field-of-view builders start from (0x016295C0,
/// the identity in the exe's image).
pub const BASE_MATRIX: u32 = 0x0162_95C0;

/// What the field-of-view builders share: `q = far / (far - near)` and
/// `1 / tan(fovy / 2)`, `tan` being the CRT's tanf (0x012F0BAC).
fn fov_terms(near: F32, far: F32, fovy: F32, tan: impl FnOnce(F32) -> F32) -> (F32, F32) {
    let q = far / (far - near);
    (q, ONE / tan(fovy * HALF))
}

/// Right-handed perspective projection from a vertical field of view
/// (D3DXMatrixPerspectiveFovRH), written over a copy of `base` (the
/// global at [`BASE_MATRIX`]): x and y scale by `cot(fovy/2) / aspect` and
/// `cot(fovy/2)`, depth maps `-near..-far` to `0..1`. Elements 1..4, 6..9,
/// 12 and 13 keep `base`'s values.
// Translated from 0x00CFC350 (DDDA.exe 2017-12-13 Steam), MtMatrix::perspectiveFovRH.
pub fn perspective_fov_rh(base: &Mat44, near: F32, far: F32, fovy: F32, aspect: F32,
                          tan: impl FnOnce(F32) -> F32) -> Mat44 {
    let (q, ys) = fov_terms(near, far, fovy, tan);
    let mut m = *base;
    m.m[0][0] = ys / aspect;
    m.m[1][1] = ys;
    m.m[2][2] = -q;
    m.m[2][3] = MINUS_ONE;
    m.m[3][2] = -(q * near);
    m.m[3][3] = O;
    m
}

/// [`perspective_fov_rh`] with the x and y scales multiplied by `sx` and
/// `sy`.
// Translated from 0x00CFC4B0 (DDDA.exe 2017-12-13 Steam), MtMatrix::perspectiveFovRH (scaled).
pub fn perspective_fov_rh_scaled(base: &Mat44, near: F32, far: F32, fovy: F32, aspect: F32, sx: F32, sy: F32,
                                 tan: impl FnOnce(F32) -> F32) -> Mat44 {
    let (q, ys) = fov_terms(near, far, fovy, tan);
    let mut m = *base;
    m.m[0][0] = ys / aspect * sx;
    m.m[1][1] = ys * sy;
    m.m[2][2] = -q;
    m.m[2][3] = MINUS_ONE;
    m.m[3][2] = -(q * near);
    m.m[3][3] = O;
    m
}

/// Left-handed perspective projection from a vertical field of view
/// (D3DXMatrixPerspectiveFovLH) over a copy of `base` (element 15 of
/// `base` is not read: it is set to 0): depth maps `near..far` to `0..1`.
// Translated from 0x00CFC620 (DDDA.exe 2017-12-13 Steam), MtMatrix::perspectiveFovLH.
pub fn perspective_fov_lh(base: &Mat44, near: F32, far: F32, fovy: F32, aspect: F32,
                          tan: impl FnOnce(F32) -> F32) -> Mat44 {
    let (q, ys) = fov_terms(near, far, fovy, tan);
    let mut m = *base;
    m.m[0][0] = ys / aspect;
    m.m[1][1] = ys;
    m.m[2][2] = q;
    m.m[2][3] = ONE;
    m.m[3][2] = -(q * near);
    m.m[3][3] = O;
    m
}

/// Which CRT routine a builder calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trig {
    /// sinf (0x012F0906)
    Sin,
    /// cosf (0x012F0A55)
    Cos,
}

/// A rotation from Euler angles (radians about x, y and z; the matrix is
/// `Rz * Ry * Rx` in row-vector form, i.e. x is applied first) with a
/// translation. The six sines and cosines come from `trig`, called in the
/// original's order: sin x, sin y, sin z, cos x, cos y, cos z. The
/// translation is copied through the x87 (`fld`/`fstp`), which quiets a
/// signalling NaN.
// Translated from 0x00CFC1D0 (DDDA.exe 2017-12-13 Steam), MtMatrix from Euler angles.
pub fn from_euler(angles: Vec3, translation: Vec3, mut trig: impl FnMut(Trig, F32) -> F32) -> Mat44 {
    let (sx, sy, sz) = (trig(Trig::Sin, angles.x), trig(Trig::Sin, angles.y), trig(Trig::Sin, angles.z));
    let (cx, cy, cz) = (trig(Trig::Cos, angles.x), trig(Trig::Cos, angles.y), trig(Trig::Cos, angles.z));
    let copy = |v: F32| F32(crate::scalar::x87_copy(v.0));
    Mat44 {
        m: [
            [cz * cy, cy * sz, -sy, O],
            [cz * sy * sx - cx * sz, sz * sy * sx + cx * cz, cy * sx, O],
            [cz * sy * cx + sx * sz, sz * sy * cx - cz * sx, cx * cy, O],
            [copy(translation.x), copy(translation.y), copy(translation.z), ONE],
        ],
    }
}
