//! Eye constraints used after motion and face-edit sampling.
//!
//! Matrices are row-major, for row vectors, in model centimetres. The
//! caller processes parents first and supplies the current pivot matrix.

/// A row-vector affine matrix.
pub type Matrix = [f32; 16];

/// Constraint type and semantic pivot ID assigned by 0x00812330.
// Translated from 0x00812330 (DDDA.exe 2017-12-13 Steam).
pub fn binding(id: u8) -> Option<(u32, Option<u8>)> {
    Some(match id {
        0x51 | 0x58 => (0x10, None),
        0x52..=0x57 => (0xd, Some(0x51)),
        0x59..=0x5e => (0xd, Some(0x58)),
        0xe2 => (0xf, Some(0x51)),
        0xe3 => (0xf, Some(0x58)),
        0xe4..=0xe6 => (0xe, Some(0xdd)),
        0xe7..=0xe9 => (0xe, Some(0xe1)),
        _ => return None,
    })
}

fn identity() -> Matrix {
    std::array::from_fn(|i| if i / 4 == i % 4 { 1.0 } else { 0.0 })
}

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| (0..4).map(|k| a[i / 4 * 4 + k] * b[k * 4 + i % 4]).sum())
}

fn vector(v: [f32; 3], matrix: Matrix) -> [f32; 3] {
    std::array::from_fn(|i| (0..3).map(|k| v[k] * matrix[k * 4 + i]).sum())
}

fn rotation(angles: [f32; 3]) -> Matrix {
    let [(sx, cx), (sy, cy), (sz, cz)] = angles.map(f32::sin_cos);
    [cy * cz, cy * sz, -sy, 0.0,
     sx * sy * cz - cx * sz, sx * sy * sz + cx * cz, sx * cy, 0.0,
     cx * sy * cz + sx * sz, cx * sy * sz - sx * cz, cx * cy, 0.0,
     0.0, 0.0, 0.0, 1.0]
}

/// Applies a configured eye constraint to a current joint world matrix.
/// Rotation is in radians; offset is in the joint parent's axes. Missing
/// pivots follow the original helpers' fallback rather than inventing one.
// Translated from 0x00860ac0 dispatch and 0x00869260 (type d),
// 0x0086a100 (e), 0x0086aff0 (f), 0x0086b2e0 (10)
// (DDDA.exe 2017-12-13 Steam). Helper correction matrices are composed
// here directly into world space; this cancels their inverse(world).
pub fn apply(
    kind: u32, world: Matrix, parent: Matrix, pivot: Option<Matrix>,
    offset: [f32; 3], scale: [f32; 3], angles: [f32; 3],
) -> Option<Matrix> {
    let rotate = rotation(angles);
    let parent_offset = vector(offset, parent);
    let mut result;
    match kind {
        0xd | 0xe => {
            result = multiply(rotate, world);
            if let Some(pivot) = pivot {
                let target: [f32; 3] = if kind == 0xd {
                    // Type d scales the displaced lid position about its
                    // eyeball in world axes, not the lid's own basis.
                    std::array::from_fn(|i| pivot[12 + i]
                        + scale[i] * (world[12 + i] + parent_offset[i] - pivot[12 + i]))
                } else {
                    let delta = std::array::from_fn(|i| world[12 + i] - pivot[12 + i]);
                    let turned = vector(delta, rotate);
                    std::array::from_fn(|i| pivot[12 + i] + turned[i] + parent_offset[i])
                };
                result[12..15].copy_from_slice(&target);
            }
            // These two helpers bypass vf24's common offset/scale tail.
        }
        0xf => {
            let mut correction = rotate;
            if let Some(pivot) = pivot {
                correction = identity();
                for row in 0..3 {
                    let length = |m: Matrix| m[row * 4..row * 4 + 4].iter()
                        .map(|v| v * v).sum::<f32>().sqrt();
                    let denominator = length(world);
                    correction[row * 5] = if denominator > f32::EPSILON {
                        length(pivot) / denominator
                    } else { 1.0 };
                }
            }
            correction[12..15].copy_from_slice(&offset);
            if scale.iter().map(|v| v * v).sum::<f32>() > 1.000_000_1e-6 {
                for row in 0..3 {
                    for column in 0..4 { correction[row * 4 + column] *= scale[row]; }
                }
            }
            result = multiply(correction, world);
        }
        0x10 => {
            // The parent inverse/composition in 0x0086b2e0 cancels:
            // S * (world * inverse(parent)) * parent = S * world.
            // Rotation fields are deliberately ignored by this type.
            result = world;
            if scale.iter().map(|v| v * v).sum::<f32>() > 1.000_000_1e-6 {
                for row in 0..3 {
                    for column in 0..4 { result[row * 4 + column] *= scale[row]; }
                }
            }
            for i in 0..3 { result[12 + i] += parent_offset[i]; }
        }
        _ => return None,
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(position: [f32; 3], angle: f32, scale: [f32; 3]) -> Matrix {
        let mut m = rotation([0.0, 0.0, angle]);
        for r in 0..3 {
            for c in 0..4 { m[r * 4 + c] *= scale[r]; }
        }
        m[12..15].copy_from_slice(&position);
        m
    }

    /// Expected matrices from execution of the four original helpers in
    /// an x86 emulator with synthetic, rotated nonuniform input matrices.
    /// For d/e/f the helper output was composed by the vf24 contract;
    /// type10 directly writes the world matrix. No game data is embedded.
    #[test]
    fn matches_original_eye_helpers() {
        let parent = matrix([10.0, -5.0, 8.0], 0.37, [1.3, 0.7, 1.1]);
        let world = multiply(matrix([2.0, 3.0, 4.0], -0.2, [0.9, 1.2, 0.8]), parent);
        let pivot = multiply(matrix([1.0, 2.0, 3.0], 0.4, [1.1, 0.8, 1.3]), parent);
        let helper_outputs = [
            (0xd, [0.975636, 0.207949, 0.069943, 0.0, -0.215569, 0.967888, 0.129317, 0.0,
                -0.040806, -0.141243, 0.989134, 0.0, 0.511279, -0.462993, 0.812500, 1.0]),
            (0xe, [0.975636, 0.207949, 0.069943, 0.0, -0.215569, 0.967888, 0.129317, 0.0,
                -0.040806, -0.141243, 0.989134, 0.0, 0.006387, -0.108788, 0.852615, 1.0]),
            (0xf, [1.171072, 0.0, 0.0, 0.0, 0.0, 0.745507, 0.0, 0.0,
                0.0, 0.0, 1.625000, 0.0, 0.0, 0.0, 0.0, 1.0]),
        ];
        for (kind, mut helper) in helper_outputs {
            if kind == 0xf {
                helper[12..15].copy_from_slice(&[0.2, -0.3, 0.5]);
                for r in 0..3 { helper[r * 5] *= [1.2, 0.85, 1.1][r]; }
            }
            let expected = multiply(helper, world);
            let actual = apply(kind, world, parent, Some(pivot), [0.2, -0.3, 0.5],
                [1.2, 0.85, 1.1], [0.13, -0.07, 0.21]).unwrap();
            for (a, b) in actual.iter().zip(expected) {
                assert!((a - b).abs() < 0.000_01, "kind={kind:x}: {actual:?} vs {expected:?}");
            }
        }
        let actual = apply(0x10, world, parent, None, [0.2, -0.3, 0.5],
            [1.2, 0.85, 1.1], [0.13, -0.07, 0.21]).unwrap();
        let expected = [1.337208, 0.357558, 0.0, 0.0, -0.007439, 0.747675, 0.0, 0.0,
            0.0, 0.0, 0.968000, 0.0, 11.983002, -2.203681, 12.950000, 1.0];
        for (a, b) in actual.iter().zip(expected) { assert!((a - b).abs() < 0.000_01); }
    }

    #[test]
    fn missing_pivot_and_small_scale_follow_source_fallback() {
        let world = matrix([2.0, 3.0, 4.0], 0.3, [1.2, 0.8, 1.0]);
        for kind in [0xd, 0xe] {
            assert_eq!(apply(kind, world, identity(), None, [5.0; 3], [2.0; 3], [0.0; 3]), Some(world));
        }
        assert_eq!(apply(0x10, world, identity(), None, [0.0; 3], [0.0; 3], [1.0; 3]), Some(world));
        assert_eq!(binding(0x52), Some((0xd, Some(0x51))));
        assert_eq!(binding(0xe9), Some((0xe, Some(0xe1))));
        assert_eq!(binding(0xed), None);
    }
}
