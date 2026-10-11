//! The math types, with the original's memory layout (`#[repr(C)]`, field
//! offsets in the comments). Conventions follow Direct3D, as the original
//! does: row vectors (`v * M`), the translation in the fourth row, and
//! angles in radians.

use crate::scalar::F32;

/// `MtVector3`: three floats plus a fourth word (16 bytes). The fourth
/// word is padding for the functions ported here: they never read it, and
/// most producers in the original write 0 there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct Vec3 {
    /// +0x00
    pub x: F32,
    /// +0x04
    pub y: F32,
    /// +0x08
    pub z: F32,
    /// +0x0C (padding)
    pub pad: F32,
}

impl Vec3 {
    pub const fn new(x: F32, y: F32, z: F32) -> Vec3 {
        Vec3 { x, y, z, pad: F32::ZERO }
    }
}

/// `MtMatrix`: a 4x4 matrix of floats, row-major (64 bytes, 16-aligned).
/// `m[r][c]` is at `+0x10 * r + 4 * c`. Rows 0..2 are the transformed x, y
/// and z axes, row 3 is the translation; a point `p` maps to `p * M`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C, align(16))]
pub struct Mat44 {
    pub m: [[F32; 4]; 4],
}

impl Mat44 {
    pub const ZERO: Mat44 = Mat44 { m: [[F32::ZERO; 4]; 4] };

    /// Element `index` (0..16) in memory order: `+4 * index`.
    pub fn at(&self, index: usize) -> F32 {
        self.m[index / 4][index % 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_original() {
        assert_eq!(std::mem::size_of::<Vec3>(), 0x10);
        assert_eq!(std::mem::size_of::<Mat44>(), 0x40);
        assert_eq!(std::mem::align_of::<Mat44>(), 0x10);
        let m = Mat44::ZERO;
        let base = &m as *const Mat44 as usize;
        assert_eq!(&m.m[3][2] as *const F32 as usize - base, 0x38);
    }
}
