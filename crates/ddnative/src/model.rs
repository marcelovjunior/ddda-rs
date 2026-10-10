//! `uObjModel` accessors.

/// The world origin row of a model's world matrix (`+0xE0..+0xEC`, the
/// fourth row: translation x, y, z and w).
// Translated from 0x004C6B50 (DDDA.exe 2017-12-13 Steam), uObjModel::vf85.
pub fn world_origin(world_row3: &[f32; 4]) -> [f32; 4] {
    world_row3.map(crate::x87_copy)
}
