//! The stage filter chain as a fullscreen pass (docs/SHADING.md, "Post
//! chain"). Stage 100's filter scheduler `scr\st100\sdl\st100_f00` creates,
//! in priority order:
//! - `ImagePlaneFilter` (priority 1): `scr\sky\camera_shadow_NOMIP_BM`
//!   (black, vignette in alpha) times `mColor` (1, 1, 1, 0.5), alpha
//!   blended over the frame (blend type 1, *assumed* SrcAlpha /
//!   InvSrcAlpha), i.e. a vignette in the sRGB frame buffer;
//! - `DOFFilter` (priority 16, `mType` 1 "Light"): the frame downscaled
//!   to 640 x (640 H / W) in sRGB, a separable 5-tap Gaussian (sigma 1,
//!   step `mBlurSize` low-res texels, point sampled, `mBlurCount` times),
//!   blended over the frame by depth: alpha = min(`mFarBlurLimit`,
//!   (z - zf) / (zfar - zf)) behind the focus, min(`mNearBlurLimit`,
//!   (zf - z) / (zf - zn)) in front, with z, zf, zn, zfar post-projection
//!   depths of the pixel, `mFocal`, `mNear`, `mFar` (vf11 0x01013320,
//!   shaders 0x89E474 / 0x775888 / 0x611C08 / 0x466ADC of sc\dx9\root).
//!   Only with the player's DofFilter=ON (the filter unit is enabled in
//!   0x0050adc0). *Assumed:* the composite blends SrcAlpha / InvSrcAlpha;
//!   the frame's alpha carries the view depth (`dd_material.wgsl`);
//! - `ColorCorrectFilter` (priority 26000): the `ToneCurveRGB` table
//!   (`mtweather::post::tone_curve`) raised to `mGamma`, per channel on the
//!   linear colour; its colour matrix (`mCorrector`) is empty, so identity.
//!
//! *Assumed:* the filters run in ascending priority; the image plane uses
//! the whole screen (its UV transform is not read).

use bevy::core_pipeline::fullscreen_material::FullscreenMaterial;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;

use ddquest::sdl::{Scheduler, Value};
use ddvfs::Vfs;

/// Table entries of the tone curve (the original renders a table texture;
/// its width is not traced, 256 is used).
pub const LUT_SIZE: usize = 256;
pub const VIGNETTE_W: usize = 64;
pub const VIGNETTE_H: usize = 32;

#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
pub struct DdPost {
    /// Tone curve after gamma, 4 entries per vec4.
    pub lut: [Vec4; LUT_SIZE / 4],
    /// Vignette alpha (texture alpha x mColor.a), 64 x 32, 4 per vec4.
    pub vignette: [Vec4; VIGNETTE_W * VIGNETTE_H / 4],
    /// x: 1 = tone curve on, y: 1 = vignette on, z: 1 = depth of field on.
    pub flags: Vec4,
    /// Depth of field: (zf, zn, zfar, blur size), (near limit, far limit,
    /// 1 / (zf - zn), 1 / (zfar - zf)), (camera near cm, camera far cm,
    /// low-res width, low-res height), (gradate colour rgb, -).
    pub dof: [Vec4; 4],
}

/// The stage's `DOFFilter` properties.
#[derive(Clone, Copy, Debug)]
pub struct DofFilter {
    pub blur_size: f32,
    pub near_limit: f32,
    pub far_limit: f32,
    pub focal: f32,
    pub near: f32,
    pub far: f32,
    pub gradate: [f32; 3],
}

/// Post-projection depth (D3D z/w) of a point `dist` cm ahead.
pub fn post_projection_depth(dist: f32, near: f32, far: f32) -> f32 {
    (far / (far - near) * (1.0 - near / dist)).clamp(0.0, 1.0)
}

impl DofFilter {
    /// The constants for a camera's near and far planes (cm) and the frame
    /// size: `mNear` / `mFar` clamped by their setters (0x0100c490,
    /// 0x0100c4f0), the low-res target 640 x round(floor(1280 H / W) / 2)
    /// (vf10 0x0100c310).
    // Translated from 0x01013320 (DDDA.exe 2017-12-13 Steam)
    pub fn constants(&self, near: f32, far: f32, size: (u32, u32)) -> [Vec4; 4] {
        let n = self.near.min(self.focal).max(near).max(0.0);
        let f = self.far.max(self.focal).min(far);
        let zf = post_projection_depth(self.focal, near, far);
        let zn = post_projection_depth(n, near, far);
        let mut zfar = post_projection_depth(f, near, far);
        if zfar - zn <= 0.0 {
            zfar = zn + 1e-5;
        }
        let h = ((1280.0 * size.1 as f32 / size.0 as f32).floor() * 0.5).round();
        [
            Vec4::new(zf, zn, zfar, self.blur_size),
            Vec4::new(self.near_limit, self.far_limit, 1.0 / (zf - zn), 1.0 / (zfar - zf)),
            Vec4::new(near, far, 640.0, h),
            Vec3::from(self.gradate).extend(0.0),
        ]
    }
}

impl Default for DdPost {
    fn default() -> Self {
        DdPost {
            lut: [Vec4::ZERO; LUT_SIZE / 4],
            vignette: [Vec4::ZERO; VIGNETTE_W * VIGNETTE_H / 4],
            flags: Vec4::ZERO,
            dof: [Vec4::ZERO; 4],
        }
    }
}

impl FullscreenMaterial for DdPost {
    fn fragment_shader() -> ShaderRef {
        "embedded://ddview/dd_post.wgsl".into()
    }
}

/// Key value at frame 0 of the first track named `name` under `parent`.
fn value(s: &Scheduler, parent: usize, name: &str) -> Option<Value> {
    s.tracks
        .iter()
        .find(|t| t.link as usize == parent && t.name == name)
        .and_then(|t| t.keys.first())
        .map(|k| k.value.clone())
}

impl DdPost {
    /// The stage's `DOFFilter` (stage 100: type 1, blur type 2), if any.
    pub fn dof_filter(vfs: &Vfs, stage: u32) -> Option<DofFilter> {
        let path = format!("scr\\st{stage}\\sdl\\st{stage}_f00");
        let bytes = vfs.read(&path, mtarc::type_hash("rScheduler")).ok()?;
        let s = Scheduler::parse(&bytes).ok()?;
        let d = s.tracks.iter().position(|t| t.name == "DOFFilter")?;
        let f = |name, def| match value(&s, d, name) {
            Some(Value::Float(v)) => v,
            _ => def,
        };
        let gradate = match value(&s, d, "mGradateColor") {
            Some(Value::Vec4(c)) => [c[0], c[1], c[2]],
            _ => [1.0; 3],
        };
        // Defaults: the unit's constructor (0x0100c1e0).
        Some(DofFilter {
            blur_size: f("mBlurSize", 1.0),
            near_limit: f("mNearBlurLimit", 1.0),
            far_limit: f("mFarBlurLimit", 1.0),
            focal: f("mFocal", 1000.0),
            near: f("mNear", 200.0),
            far: f("mFar", 200_000.0),
            gradate,
        })
    }

    /// Turns on the depth of field for a camera (near / far in cm) and
    /// frame size.
    pub fn with_dof(mut self, dof: &DofFilter, near: f32, far: f32, size: (u32, u32)) -> Self {
        self.dof = dof.constants(near, far, size);
        self.flags.z = 1.0;
        self
    }

    /// Reads the stage's filter scheduler. `None` if it is missing.
    pub fn load(vfs: &Vfs, stage: u32) -> Option<DdPost> {
        let path = format!("scr\\st{stage}\\sdl\\st{stage}_f00");
        let bytes = vfs.read(&path, mtarc::type_hash("rScheduler")).ok()?;
        let s = Scheduler::parse(&bytes).ok()?;
        let mut post = DdPost::default();
        // ColorCorrectFilter: ToneCurveRGB (x[8], y[8]) and mGamma.
        if let Some(cc) = s.tracks.iter().position(|t| t.name == "ColorCorrectFilter") {
            if let Some(Value::Matrix(m)) = value(&s, cc, "ToneCurveRGB") {
                let xs: [f32; 8] = m[..8].try_into().unwrap();
                let ys: [f32; 8] = m[8..].try_into().unwrap();
                let gamma = match value(&s, cc, "mGamma") {
                    Some(Value::Vec4(g)) => g[0],
                    _ => 1.0,
                };
                let lut: Vec<f32> = (0..LUT_SIZE)
                    .map(|i| {
                        let x = i as f32 / (LUT_SIZE - 1) as f32;
                        mtweather::post::tone_curve(&xs, &ys, x).powf(gamma)
                    })
                    .collect();
                for (k, v) in post.lut.iter_mut().enumerate() {
                    *v = Vec4::from_slice(&lut[4 * k..4 * k + 4]);
                }
                post.flags.x = 1.0;
            }
        }
        // ImagePlaneFilter: texture alpha x mColor.a.
        if let Some(ip) = s.tracks.iter().position(|t| t.name == "ImagePlaneFilter") {
            let color = match value(&s, ip, "mColor") {
                Some(Value::Vec4(c)) => c,
                _ => [1.0; 4],
            };
            let tex = match value(&s, ip, "mpTexture") {
                Some(Value::Resource(Some((_, p)))) => Some(p),
                _ => None,
            };
            if let Some(alpha) = tex.and_then(|p| vignette_alpha(vfs, &p)) {
                let a: Vec<f32> = alpha.iter().map(|x| x * color[3]).collect();
                for (k, v) in post.vignette.iter_mut().enumerate() {
                    *v = Vec4::from_slice(&a[4 * k..4 * k + 4]);
                }
                post.flags.y = 1.0;
            }
        }
        Some(post)
    }
}

/// Alpha of a DXT5 texture, box-filtered to VIGNETTE_W x VIGNETTE_H.
fn vignette_alpha(vfs: &Vfs, path: &str) -> Option<Vec<f32>> {
    let bytes = vfs.read(path, mtarc::type_hash("rTexture")).ok()?;
    let t = mttex::parse(&bytes).ok()?;
    if t.header.format != mttex::D3dFormat::Dxt5 {
        return None;
    }
    let (w, h) = (t.header.width as usize, t.header.height as usize);
    let data = t.surfaces.first()?.data;
    let mut alpha = vec![0f32; w * h];
    let bw = w.div_ceil(4);
    for (bi, blk) in data.as_chunks::<16>().0.iter().enumerate().take(bw * h.div_ceil(4)) {
        let (a0, a1) = (blk[0] as f32, blk[1] as f32);
        let pal: [f32; 8] = if blk[0] > blk[1] {
            std::array::from_fn(|i| match i {
                0 => a0,
                1 => a1,
                _ => ((8 - i) as f32 * a0 + (i - 1) as f32 * a1) / 7.0,
            })
        } else {
            std::array::from_fn(|i| match i {
                0 => a0,
                1 => a1,
                6 => 0.0,
                7 => 255.0,
                _ => ((6 - i) as f32 * a0 + (i - 1) as f32 * a1) / 5.0,
            })
        };
        let bits = u64::from_le_bytes([blk[2], blk[3], blk[4], blk[5], blk[6], blk[7], 0, 0]);
        let (bx, by) = (bi % bw * 4, bi / bw * 4);
        for p in 0..16 {
            let (x, y) = (bx + p % 4, by + p / 4);
            if x < w && y < h {
                alpha[y * w + x] = pal[((bits >> (3 * p)) & 7) as usize] / 255.0;
            }
        }
    }
    let (sx, sy) = (w / VIGNETTE_W, h / VIGNETTE_H);
    Some(
        (0..VIGNETTE_W * VIGNETTE_H)
            .map(|i| {
                let (gx, gy) = (i % VIGNETTE_W, i / VIGNETTE_W);
                let mut sum = 0.0;
                for y in gy * sy..(gy + 1) * sy {
                    for x in gx * sx..(gx + 1) * sx {
                        sum += alpha[y * w + x];
                    }
                }
                sum / (sx * sy) as f32
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 100's DOFFilter with ref001's camera (near 96 cm, far 40 km).
    #[test]
    fn dof_constants() {
        let dof = DofFilter {
            blur_size: 1.0,
            near_limit: 0.22,
            far_limit: 0.385,
            focal: 1000.0,
            near: 50.0,
            far: 200_000.0,
            gradate: [1.0; 3],
        };
        let c = dof.constants(96.0, 4_000_000.0, (1920, 1080));
        // mNear clamps up to the camera's near plane: zn = 0.
        assert_eq!(c[0].y, 0.0);
        let zf = post_projection_depth(1000.0, 96.0, 4_000_000.0);
        assert!((c[0].x - zf).abs() < 1e-6 && (zf - 0.904).abs() < 1e-3);
        assert!((c[1].z - 1.0 / zf).abs() < 1e-3);
        assert_eq!([c[2].z, c[2].w], [640.0, 360.0]);
    }
}
