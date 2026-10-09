//! Headless skeleton posing: a motion sampled at a frame on a model's rest
//! skeleton, composed into model-space joint matrices.
//!
//! Conventions (docs/ANIMATION.md; the game's joint routine 0x01051B60):
//! matrices are row-major for row vectors (`p * M`, row 3 the translation),
//! a joint's local matrix is scale, then rotation, then translation, and
//! `world = local x parent world`. Rotations are the quaternions the motion
//! keys store, in the same sense as `ddview` applies them (a column-vector
//! rotation `R(q)`, whose transpose is the row-vector basis).
//!
//! A channel the motion keys (or gives a reference value) replaces the
//! joint's rest value; other channels keep the rest pose. Tracks on joint
//! 255 (the root's travel) do not pose the skeleton: the unit moves by them
//! (root motion), so they are left to the caller.

use crate::{Motion, Usage};

/// Row-major 4x4 matrix for row vectors.
pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// One joint's rest pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestJoint {
    /// The model's joint id (what motion tracks address).
    pub id: u8,
    /// Index of the parent joint in the skeleton (earlier in the list).
    pub parent: Option<usize>,
    /// Rotation quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

/// A model's rest skeleton, parents before children.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skeleton {
    pub joints: Vec<RestJoint>,
}

/// The column-vector rotation matrix of a unit quaternion.
fn rotation_matrix(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// The unit quaternion of a column-vector rotation matrix.
fn quaternion(r: [[f32; 3]; 3]) -> [f32; 4] {
    let t = r[0][0] + r[1][1] + r[2][2];
    let q = if t > 0.0 {
        let s = (t + 1.0).sqrt() * 2.0;
        [
            (r[2][1] - r[1][2]) / s,
            (r[0][2] - r[2][0]) / s,
            (r[1][0] - r[0][1]) / s,
            0.25 * s,
        ]
    } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
        let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (r[0][1] + r[1][0]) / s,
            (r[0][2] + r[2][0]) / s,
            (r[2][1] - r[1][2]) / s,
        ]
    } else if r[1][1] > r[2][2] {
        let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
        [
            (r[0][1] + r[1][0]) / s,
            0.25 * s,
            (r[1][2] + r[2][1]) / s,
            (r[0][2] - r[2][0]) / s,
        ]
    } else {
        let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
        [
            (r[0][2] + r[2][0]) / s,
            (r[1][2] + r[2][1]) / s,
            0.25 * s,
            (r[1][0] - r[0][1]) / s,
        ]
    };
    let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / n)
}

/// Local matrix (row vectors) from scale, rotation and translation.
pub fn compose(scale: [f32; 3], rotation: [f32; 4], translation: [f32; 3]) -> Mat4 {
    let r = rotation_matrix(rotation);
    let mut m = IDENTITY;
    for (i, row) in m.iter_mut().take(3).enumerate() {
        for j in 0..3 {
            // row i of the row-vector basis = column i of R, scaled
            row[j] = r[j][i] * scale[i];
        }
    }
    m[3] = [translation[0], translation[1], translation[2], 1.0];
    m
}

/// `a x b` (apply `a`, then `b`, for row vectors).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

impl Skeleton {
    /// A skeleton from `(joint id, parent index, row-major local matrix)`
    /// per bone, as `mtmod::Bone` stores them (`parent` 255 = none).
    pub fn from_bones(bones: impl IntoIterator<Item = (u8, u8, [f32; 16])>) -> Self {
        let mut joints = Vec::new();
        for (i, (id, parent, local)) in bones.into_iter().enumerate() {
            let m: Mat4 = std::array::from_fn(|r| std::array::from_fn(|c| local[r * 4 + c]));
            let scale: [f32; 3] =
                std::array::from_fn(|r| (m[r][0] * m[r][0] + m[r][1] * m[r][1] + m[r][2] * m[r][2]).sqrt());
            // column-vector rotation: R[j][i] = row i, component j, unscaled
            let r: [[f32; 3]; 3] = std::array::from_fn(|j| std::array::from_fn(|c| m[c][j] / scale[c]));
            let p = usize::from(parent);
            joints.push(RestJoint {
                id,
                parent: (parent != 255 && p < i).then_some(p),
                rotation: quaternion(r),
                translation: [m[3][0], m[3][1], m[3][2]],
                scale,
            });
        }
        Skeleton { joints }
    }

    /// The index of the joint with id `id`.
    pub fn index_of(&self, id: u8) -> Option<usize> {
        self.joints.iter().position(|j| j.id == id)
    }

    /// Model-space matrices of every joint (index-aligned with `joints`):
    /// the rest pose with `motion`'s channels at `frame` (no motion: the
    /// rest pose).
    pub fn pose(&self, motion: Option<&Motion>, frame: f32) -> Vec<Mat4> {
        let mut locals: Vec<([f32; 3], [f32; 4], [f32; 3])> = self
            .joints
            .iter()
            .map(|j| (j.scale, j.rotation, j.translation))
            .collect();
        if let Some(m) = motion {
            for t in &m.tracks {
                if t.joint == 255 {
                    continue;
                }
                let Some(i) = self.index_of(t.joint) else { continue };
                let v = t.sample(frame);
                match t.usage {
                    Usage::LocalRotation => locals[i].1 = v,
                    Usage::LocalPosition => locals[i].2 = [v[0], v[1], v[2]],
                    Usage::LocalScale => locals[i].0 = [v[0], v[1], v[2]],
                    _ => {}
                }
            }
        }
        let mut world: Vec<Mat4> = Vec::with_capacity(self.joints.len());
        for (i, j) in self.joints.iter().enumerate() {
            let (s, r, t) = locals[i];
            let local = compose(s, r, t);
            world.push(match j.parent {
                Some(p) => mul(&local, &world[p]),
                None => local,
            });
        }
        world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, Track};

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    fn row(m: &Mat4) -> [f32; 3] {
        [m[3][0], m[3][1], m[3][2]]
    }

    /// A matrix decomposed and recomposed is itself.
    #[test]
    fn rest_round_trip() {
        let q = [0.0, (0.5f32).sin(), 0.0, (0.5f32).cos()]; // 1 rad about y
        let m = compose([2.0, 2.0, 2.0], q, [1.0, 2.0, 3.0]);
        let flat: [f32; 16] = std::array::from_fn(|k| m[k / 4][k % 4]);
        let s = Skeleton::from_bones([(0u8, 255u8, flat)]);
        let back = s.pose(None, 0.0)[0];
        for (a, b) in m.iter().flatten().zip(back.iter().flatten()) {
            assert!((a - b).abs() < 1e-5, "{m:?} {back:?}");
        }
    }

    /// A child 10 along +z of a root turned 90 degrees about y ends at +x
    /// (the sense ddview gives the keys: +z turns to +x), and a keyed
    /// translation replaces the rest one.
    #[test]
    fn chain_and_keyed_channels() {
        let flat = |m: Mat4| -> [f32; 16] { std::array::from_fn(|k| m[k / 4][k % 4]) };
        let s = Skeleton::from_bones([
            (0u8, 255u8, flat(IDENTITY)),
            (
                5,
                0,
                flat(compose([1.0; 3], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 10.0])),
            ),
        ]);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let track = |joint, usage, v: [f32; 4]| Track {
            codec: 0,
            usage,
            bone_type: 0,
            joint,
            weight: 1.0,
            reference: v,
            keys: vec![Key { frame: 0, value: v }],
        };
        let motion = Motion {
            frame_count: 10,
            loop_frame: -1,
            end_position: [0.0; 4],
            end_rotation: [0.0; 4],
            flags: 0,
            tracks: vec![
                track(0, Usage::LocalRotation, [0.0, h, 0.0, h]),
                track(0, Usage::LocalPosition, [0.0, 100.0, 0.0, 0.0]),
                // the root's travel does not pose
                track(255, Usage::AbsolutePosition, [50.0, 0.0, 0.0, 0.0]),
            ],
        };
        let w = s.pose(Some(&motion), 3.0);
        assert!(close(row(&w[0]), [0.0, 100.0, 0.0]), "{:?}", w[0]);
        assert!(close(row(&w[1]), [10.0, 100.0, 0.0]), "{:?}", w[1]);
    }
}
