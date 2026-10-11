//! `MtMatrix` builders: projection matrices (the original's equivalents of
//! the D3DX `PerspectiveRH/LH` and `OrthoRH/LH` builders).
//!
//! Each function writes all sixteen elements. The arithmetic is the
//! original's, operation by operation (see `crate::scalar`).

use crate::scalar::F32;
use crate::types::Mat44;

/// 2.0, read by the builders from 0x01406CEC.
pub const TWO: F32 = F32(2.0);
/// 1.0, read from 0x01406CE0.
pub const ONE: F32 = F32(1.0);
/// -1.0, read from 0x0161C20C.
pub const MINUS_ONE: F32 = F32(-1.0);

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
