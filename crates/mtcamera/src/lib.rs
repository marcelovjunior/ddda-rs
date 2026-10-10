//! Bounded, borrowed reader for DDDA's `rCameraList` (LCM v3).
//! Loader 0x01127660, writer 0x01127710, consumer 0x00fab460
//! (DDDA.exe 2017-12-13 Steam). See docs/CAMERA.md for evidence and limits.

use std::fmt;

pub mod fade;
pub mod actor_offset;
pub mod params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

/// uCamera constructor defaults, in game centimetres.
// Translated from 0x01128740 (DDDA.exe 2017-12-13 Steam).
pub const DEFAULT_NEAR_CM: f32 = 16.0;
pub const DEFAULT_FAR_CM: f32 = 4_000_000.0;

/// Motion cameras use their authored zoom, not uCameraBase's gameplay
/// CameraFov setting. `aspect` is the viewport width / height.
// Translated from 0x00faa920 (DDDA.exe 2017-12-13 Steam).
pub fn vertical_fov_radians(fov_degrees: f32, zoom_degrees: f32, fov_type: u32, aspect: f32) -> f32 {
    let fov = (fov_degrees + zoom_degrees).clamp(0.01, 179.99).to_radians();
    if fov_type == 0 { fov } else { 2.0 * ((fov * 0.5).tan() / aspect).atan() }
}

fn span(b: &[u8], offset: usize, count: usize, stride: usize) -> Result<&[u8]> {
    let end = count
        .checked_mul(stride)
        .and_then(|n| offset.checked_add(n))
        .ok_or_else(|| Error("span overflow".into()))?;
    b.get(offset..end)
        .ok_or_else(|| Error(format!("span {offset:#x}..{end:#x} outside {} bytes", b.len())))
}
fn word(b: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(b, offset, 1, 4)?.try_into().unwrap()))
}
fn floats<const N: usize>(b: &[u8], index: usize) -> [f32; N] {
    std::array::from_fn(|i| {
        let o = (index * N + i) * 4;
        f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    })
}

#[derive(Debug, Clone)]
pub struct CameraList<'a> {
    /// Null table entries retain their indices, which are the timeline's cuts.
    pub cameras: Vec<Option<Camera<'a>>>,
}

#[derive(Debug, Clone)]
pub struct Camera<'a> {
    /// `rCameraListXml::CameraInfo.mUserData[4]` (0x01128120/0x01127c30).
    /// The application-defined meaning of each integer is not established.
    pub user_data: [i32; 4],
    /// Raw enum. 0 = vertical FOV; 1 = horizontal (reference names).
    /// The projection consumer treats every nonzero value as horizontal.
    pub fov_type: u32,
    /// Preserved word at +0x18; semantics not established.
    pub opaque_18: u32,
    frames: usize,
    positions: &'a [u8],
    targets: &'a [u8],
    rotations: &'a [u8],
    fovs: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub position: [f32; 3],
    /// Stored/interpolated target, before the consumer reconstructs direction.
    pub target: [f32; 3],
    /// XYZW quaternion.
    pub rotation: [f32; 4],
    pub fov_degrees: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_degrees: f32,
}

impl<'a> CameraList<'a> {
    // Translated from 0x01127660 and 0x01127710 (DDDA.exe 2017-12-13 Steam),
    // with explicit bounds checks. All offsets are relative to the file start.
    pub fn parse(b: &'a [u8]) -> Result<Self> {
        let header = span(b, 0, 1, 8)?;
        if &header[..4] != b"LCM\0" {
            return Err(Error("bad LCM magic".into()));
        }
        if u16::from_le_bytes(header[4..6].try_into().unwrap()) != 3 {
            return Err(Error("unsupported LCM version (expected 3)".into()));
        }
        let count = u16::from_le_bytes(header[6..8].try_into().unwrap()) as usize;
        span(b, 8, count, 4)?;
        let mut cameras = Vec::with_capacity(count);
        for i in 0..count {
            let offset = word(b, 8 + i * 4)? as usize;
            if offset == 0 {
                cameras.push(None);
                continue;
            }
            let info = span(b, offset, 1, 44)?;
            let frames = word(info, 16)? as usize;
            cameras.push(Some(Camera {
                user_data: [
                    word(info, 0)? as i32,
                    word(info, 4)? as i32,
                    word(info, 8)? as i32,
                    word(info, 12)? as i32,
                ],
                fov_type: word(info, 20)?,
                opaque_18: word(info, 24)?,
                frames,
                positions: span(b, word(info, 28)? as usize, frames, 12)?,
                targets: span(b, word(info, 32)? as usize, frames, 12)?,
                rotations: span(b, word(info, 36)? as usize, frames, 16)?,
                fovs: span(b, word(info, 40)? as usize, frames, 4)?,
            }));
        }
        Ok(Self { cameras })
    }
}

impl Camera<'_> {
    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn frame(&self, index: usize) -> Option<Sample> {
        (index < self.frames).then(|| Sample {
            position: floats(self.positions, index),
            target: floats(self.targets, index),
            rotation: floats(self.rotations, index),
            fov_degrees: floats::<1>(self.fovs, index)[0],
        })
    }

    /// Samples local camera time, before offsets, actor adjustment or projection.
    /// Non-repeat clamps to [0, frames-1]. Repeat allows the last-to-first
    /// interval but requires time < frames: advancing/cut rollover is a caller
    /// responsibility. Empty cameras and nonfinite time return None.
    // Translated from 0x00fab460 (DDDA.exe 2017-12-13 Steam), through 0x00fab8e4.
    // The x87 conversion at 0x00fab543..580 explicitly truncates toward zero.
    pub fn sample(&self, frame: f32, repeat: bool) -> Option<Sample> {
        if self.frames == 0 || !frame.is_finite() {
            return None;
        }
        let frame = frame.max(0.0);
        let frame = if repeat {
            if frame >= self.frames as f32 {
                return None;
            }
            frame
        } else {
            frame.min((self.frames - 1) as f32)
        };
        let i = frame as usize;
        let mut a = self.frame(i)?;
        let t = frame - i as f32;
        if t > 0.0 {
            let b = self.frame(if i + 1 < self.frames { i + 1 } else { 0 })?;
            for k in 0..3 {
                a.position[k] += (b.position[k] - a.position[k]) * t;
                a.target[k] += (b.target[k] - a.target[k]) * t;
            }
            a.fov_degrees = b.fov_degrees * t + (1.0 - t) * a.fov_degrees;
            let dot = a.rotation.iter().zip(b.rotation).map(|(x, y)| x * y).sum::<f32>();
            let sign = if dot < 0.0 { -1.0 } else { 1.0 };
            let mut weight_a = 1.0 - t;
            let mut weight_b = t;
            if dot.abs() < 0.999_999_9 {
                let angle = dot.abs().acos();
                let reciprocal_sin = 1.0 / angle.sin();
                weight_a = ((1.0 - t) * angle).sin() * reciprocal_sin;
                weight_b = (t * angle).sin() * reciprocal_sin;
            }
            // The original negates the FIRST endpoint for a negative dot.
            a.rotation =
                std::array::from_fn(|k| a.rotation[k] * (weight_a * sign) + b.rotation[k] * weight_b);
        }
        Some(a)
    }
}

impl Sample {
    /// Rebuilds up and target from the quaternion and stored eye-target distance.
    /// This is the base motion-camera pose, not uOfsMotionCamera's final pose.
    // Translated from 0x00fab8e4..0x00faba6a (DDDA.exe 2017-12-13 Steam).
    pub fn pose(self) -> Pose {
        let [x, y, z, w] = self.rotation;
        let distance = self
            .position
            .iter()
            .zip(self.target)
            .map(|(a, b)| (b - a) * (b - a))
            .sum::<f32>()
            .sqrt();
        let forward = [
            -(2.0 * z * x + w * 2.0 * y),
            -(y * 2.0 * z - w * x * 2.0),
            -(1.0 - (y * 2.0 * y + x * 2.0 * x)),
        ];
        Pose {
            position: self.position,
            target: std::array::from_fn(|k| self.position[k] + forward[k] * distance),
            up: [
                2.0 * y * x - w * 2.0 * z,
                1.0 - (2.0 * z * z + x * 2.0 * x),
                y * 2.0 * z + w * x * 2.0,
            ],
            fov_degrees: self.fov_degrees,
        }
    }
}
