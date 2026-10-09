//! The stage filter chain as a fullscreen pass (docs/SHADING.md, "Post
//! chain"). Stage 100's filter scheduler `scr\st100\sdl\st100_f00` creates,
//! in priority order:
//! - `ImagePlaneFilter` (priority 1): `scr\sky\camera_shadow_NOMIP_BM`
//!   (black, vignette in alpha) times `mColor` (1, 1, 1, 0.5), alpha
//!   blended over the frame (blend type 1, *assumed* SrcAlpha /
//!   InvSrcAlpha), i.e. a vignette in the sRGB frame buffer;
//! - `DOFFilter` (priority 16): not done;
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
    /// x: 1 = tone curve on, y: 1 = vignette on.
    pub flags: Vec4,
}

impl Default for DdPost {
    fn default() -> Self {
        DdPost {
            lut: [Vec4::ZERO; LUT_SIZE / 4],
            vignette: [Vec4::ZERO; VIGNETTE_W * VIGNETTE_H / 4],
            flags: Vec4::ZERO,
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
    for (bi, blk) in data.chunks_exact(16).enumerate().take(bw * h.div_ceil(4)) {
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
