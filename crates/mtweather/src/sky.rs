//! Sky scattering inputs (docs/SHADING.md, "Sky"): the `CBSky` constant
//! buffer and the two optical depth maps the sky shaders read.
//!
//! The sky unit (`uSkyExt`, created by the weather manager with 256 x 256
//! depth maps) renders, each frame: the Rayleigh and Mie optical depth maps
//! (only when the atmosphere changes), the Rayleigh and Mie scatter maps
//! (`mDivX` x `mDivY`, over the screen) and the sky dome, which lights the
//! scatter maps with the sun colour and the two phase functions. This module
//! has the CPU half: the constants and the depth maps.

use crate::Sky;

/// Side of the optical depth maps of the weather manager's sky
/// (0x004c19f0 creates it with 0x100 x 0x100).
pub const DEPTH_MAP_SIZE: usize = 256;

/// The sky unit's own properties that feed `CBSky` (from the stage's sky
/// scheduler and the weather manager's per-weather values). Defaults are the
/// unit's constructor values (0x00f30930).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyInputs {
    /// `mBaseSunColor` (`uSkyExt`; the weather manager writes `SunColor`).
    pub sun_color: [f32; 3],
    /// `mMieScattering` (the weather manager writes `MieScattering`).
    pub mie_scattering: [f32; 3],
    /// `mMieDensity` (the weather manager writes `MieDensity`).
    pub mie_density: f32,
    /// `mEccentricity`, the Mie phase asymmetry.
    pub eccentricity: f32,
    pub cloudiness: f32,
    pub cloud_height: f32,
    pub cloud_thickness: f32,
    pub cloud_scattering: f32,
    pub cloud_eccentricity: f32,
    /// `mHeightOffset` (cm, added to the eye height).
    pub height_offset: f32,
    /// `mStarrySkyIntensity`.
    pub starry_sky_intensity: f32,
    /// `mDivX`, `mDivY`: scatter map size.
    pub scatter_map_size: [u32; 2],
    /// `mPreComputeLoopCount`: steps of the CPU optical depth integration.
    pub precompute_loop_count: u32,
}

impl Default for SkyInputs {
    fn default() -> Self {
        SkyInputs {
            sun_color: [1.0; 3],
            mie_scattering: [1.0; 3],
            mie_density: 1.0,
            eccentricity: 0.4,
            cloudiness: 0.0,
            cloud_height: 1000.0,
            cloud_thickness: 0.0,
            cloud_scattering: 0.0,
            cloud_eccentricity: 0.4,
            height_offset: 0.0,
            starry_sky_intensity: 1.0,
            scatter_map_size: [32, 32],
            precompute_loop_count: 50,
        }
    }
}

const FOUR_PI: f32 = 4.0 * std::f32::consts::PI;

/// Rayleigh scattering of the sky resource: per channel, `0.01 / ((l /
/// l_blue)^-4 * k)` with the wavelengths at +0x80 and `k` at +0x90.
// Translated from 0x00e2c920 (DDDA.exe 2017-12-13 Steam)
fn rayleigh_lengths(sky: &Sky) -> [f32; 3] {
    let inv = 1.0 / sky.vector_80[2];
    let k = sky.scalar_90;
    std::array::from_fn(|i| {
        // The exe divides the blue wavelength by itself for the third channel.
        let ratio = if i == 2 { 1.0 } else { sky.vector_80[i] * inv };
        0.01 / ((f64::from(ratio).powf(-4.0) as f32) * k)
    })
}

/// The sky unit's scattering coefficients (+0x370 Rayleigh, +0x380 Mie,
/// +0x390 cloud; per metre, before the 4 pi).
// Translated from 0x00f36d60 (tail) (DDDA.exe 2017-12-13 Steam)
fn coefficients(sky: &Sky, inp: &SkyInputs) -> ([f32; 3], [f32; 3], f32) {
    let rl = rayleigh_lengths(sky);
    let rayleigh: [f32; 3] = std::array::from_fn(|i| 0.001 / (rl[i] * 100.0));
    let mie: [f32; 3] = std::array::from_fn(|i| 0.001 / (inp.mie_scattering[i] * 100.0) * inp.mie_density);
    let cloud = inp.cloudiness * inp.cloud_scattering * 0.001;
    (rayleigh, mie, cloud)
}

/// The 44 floats of `CBSky` (`fRayleighScatteringK` .. `fSkyHeightOffset`,
/// layout of the shader package's `CBSky`). `sun_dir` is the unit direction
/// to the sun (`uSky` +0x2E0).
// Translated from 0x00f36d60 (tail) and 0x00f33d50 (DDDA.exe 2017-12-13 Steam)
pub fn constants(sky: &Sky, inp: &SkyInputs, sun_dir: [f32; 3]) -> [f32; 44] {
    let (rayleigh, mie, cloud) = coefficients(sky, inp);
    let r = &sky.scalars_a0_bc; // +0xA0 .. +0xBC
    let secondary = r[7];
    let g = inp.eccentricity;
    let gc = inp.cloud_eccentricity;
    let mut c = [0f32; 44];
    for i in 0..3 {
        c[i] = rayleigh[i] * 4.0 * std::f32::consts::PI;
        c[4 + i] = rayleigh[i] * 0.75;
        c[8 + i] = mie[i] * 4.0 * std::f32::consts::PI;
        c[12 + i] = (1.0 - g * g) * mie[i];
        c[16 + i] = rayleigh[i] * 4.0 * std::f32::consts::PI * secondary;
        c[20 + i] = mie[i] * 4.0 * std::f32::consts::PI * secondary;
        c[24 + i] = inp.sun_color[i];
        c[28 + i] = sun_dir[i];
    }
    c[3] = r[3] * 1000.0; // fAtmosphereHeight
    c[7] = r[4] * 1000.0; // fAtmosphereAverageDensityHeight
    c[11] = r[5] * 1000.0; // fAerosolHeight
    c[15] = r[6] * 1000.0; // fAerosolAverageDensityHeight
    c[19] = r[2] * 1000.0; // fEarthRadius
    c[23] = DEPTH_MAP_SIZE as f32; // fOpticalDepthMapSize
    c[27] = inp.starry_sky_intensity * 0.01;
    let night = if sun_dir[1] >= 0.0 { 1.0 - sun_dir[1] } else { 1.0 };
    c[31] = f64::from(night).powf(16.0) as f32; // fStarrySkyBlendFactor
    c[32] = inp.scatter_map_size[0] as f32;
    c[33] = inp.scatter_map_size[1] as f32;
    c[34] = g * g + 1.0;
    c[35] = g * -2.0;
    c[36] = gc * gc + 1.0;
    c[37] = gc * -2.0;
    c[38] = inp.cloud_thickness + inp.cloud_height; // fCloudHeightMax
    c[39] = inp.cloud_height; // fCloudHeightMin
    c[40] = cloud * FOUR_PI;
    c[41] = (1.0 - gc * gc) * cloud;
    c[42] = cloud * 4.0 * std::f32::consts::PI * secondary;
    c[43] = inp.height_offset;
    c
}

/// Unit direction to the sun at `ms` milliseconds into the day (`uSky`
/// `mTime`), in the stage frame (x east, y up, z south).
///
/// *Reconstructed, not translated:* the sky update (0x00f36d60) builds the
/// sun's rotation from the resource's hour angle (`2 pi ms / day`, negated
/// when +0x98 is set), axial tilt (+0x94, 23.4 deg), season angle (+0x9C,
/// 90 deg) and latitude (+0xA0, 47 deg); its matrix code is not translated.
/// This is the standard solar position for those inputs, with declination
/// `tilt * sin(season)` (assumed) and noon at 12:00. It matches the sun
/// direction read live at 9:46, 12:21, 15:00 and 17:45 within 0.006.
pub fn sun_direction(sky: &Sky, ms: u32) -> [f32; 3] {
    let deg = std::f64::consts::PI / 180.0;
    let mut hour = f64::from(ms) / 86_400_000.0 * std::f64::consts::TAU - std::f64::consts::PI;
    if sky.word_98 != 0 {
        hour = -hour;
    }
    let lat = f64::from(sky.scalars_a0_bc[0]) * deg;
    let decl = f64::from(sky.scalar_94) * deg * (f64::from(sky.word_9c) * deg).sin();
    let east = -decl.cos() * hour.sin();
    let north = lat.cos() * decl.sin() - lat.sin() * decl.cos() * hour.cos();
    let up = lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour.cos();
    [east as f32, up as f32, -north as f32]
}

/// Shadow-map direction, distinct from the direct-light direction.
/// `limit` is sWeatherManager.mShadowLimit (+0xe0), read from the stage
/// scheduler or its constructor default. No clamping is done by the source.
// Translated from 0x004c1110, consumed by 0x00c6c970 (DDDA.exe 2017-12-13 Steam).
pub fn shadow_direction(sun: [f32; 3], limit: f32) -> [f32; 3] {
    let y = (1.0 - limit) * sun[1].abs() + limit;
    let inverse_length = (sun[0] * sun[0] + y * y + sun[2] * sun[2]).sqrt().recip();
    [sun[0] * inverse_length, y * inverse_length, sun[2] * inverse_length]
}

#[cfg(test)]
mod shadow_direction_tests {
    use super::shadow_direction;

    #[test]
    fn matches_recorded_weather_and_shadow_unit_direction() {
        // Cassardis weather +0xd0 at 16:30:31; its shadow unit +0x160
        // agrees within 0.000013 (one update's sky phase difference).
        let got = shadow_direction([-0.848_688_4, 0.528_669, -0.015_401_542], 0.5);
        for (value, expected) in got.into_iter().zip([-0.743_002_3, 0.669_152_86, -0.013_483_61]) {
            assert!((value - expected).abs() < 0.000_001);
        }
    }

    #[test]
    fn night_reflects_height_and_limit_preserves_horizontal_bearing() {
        assert_eq!(shadow_direction([0.0, -1.0, 0.0], 0.5), [0.0, 1.0, 0.0]);
        assert_eq!(shadow_direction([0.6, -0.8, 0.0], 0.0), [0.6, 0.8, 0.0]);
        let got = shadow_direction([0.6, 0.0, 0.8], 1.0);
        assert!((got[0] / got[2] - 0.75).abs() < 1e-6);
        assert!((got[1] - 1.0_f32 / 2.0_f32.sqrt()).abs() < 1e-6);
    }
}

/// Density integral from `pos` (2D: across, up from the earth's centre,
/// m) along `dir` over `length` m in `steps` trapezoid steps, density
/// `exp(-h / scale)` (1 below the ground after the first sample).
// Translated from 0x00f2ffc0 (DDDA.exe 2017-12-13 Steam)
fn depth_along(pos: [f32; 2], dir: [f32; 2], length: f32, radius: f32, scale: f32, steps: u32) -> f32 {
    let dt = length / steps as f32;
    let [mut x, mut y] = pos;
    let mut prev = (-(((x * x + y * y).sqrt() - radius) * (1.0 / scale))).exp();
    let mut acc = 0.0;
    for _ in 0..steps {
        x += dir[0] * dt;
        y += dt * dir[1];
        let h = (y * y + x * x).sqrt() - radius;
        let d = if h <= 0.0 { 1.0 } else { (-(h * (1.0 / scale))).exp() };
        acc += (d + prev) * 0.5;
        prev = d;
    }
    dt * acc
}

/// Near and far distances to a sphere of radius `shell` about the earth's
/// centre from a point at radius `r` (`r2 = r^2`), `b` twice the ray's
/// projection on the up axis; `None` without a hit.
fn shell_hits(b: f32, r2: f32, shell: f32) -> Option<(f32, f32)> {
    let disc = b * b - (r2 - shell * shell) * 4.0;
    (disc >= 0.0).then(|| {
        let s = disc.sqrt();
        ((-b - s) * 0.5, (s - b) * 0.5)
    })
}

/// Transmittance of sunlight to a point `height_cm` above the ground along
/// `sun_dir` (unit, towards the sun): `exp(-4 pi (Rayleigh depth + Mie
/// depth + cloud path))` per channel; zero when the ray misses the
/// atmosphere's top. *Not translated:* the branch for a point above the
/// aerosol layer (over 10 km; the viewer's eye never is).
// Translated from 0x00f32840 (DDDA.exe 2017-12-13 Steam)
pub fn transmittance(sky: &Sky, inp: &SkyInputs, height_cm: f32, sun_dir: [f32; 3]) -> [f32; 3] {
    let (rayleigh, mie, cloud_k) = coefficients(sky, inp);
    let a = &sky.scalars_a0_bc;
    let (radius, atmosphere, rayleigh_scale) = (a[2] * 1000.0, a[3] * 1000.0, a[4] * 1000.0);
    let (aerosol, mie_scale) = (a[5] * 1000.0, a[6] * 1000.0);
    let steps = inp.precompute_loop_count.max(1);
    let h = height_cm * 0.01;
    let r = h + radius;
    let mu = sun_dir[1];
    let sin = (1.0 - mu * mu).sqrt();
    let b = (r * mu + sin * 0.0) * 2.0;
    let r2 = r * r;
    let Some((_, t)) = shell_hits(b, r2, atmosphere + radius) else {
        return [0.0; 3];
    };
    if t <= 0.0 {
        return [0.0; 3];
    }
    let ray_depth = depth_along([0.0, r], [sin, mu], t, radius, rayleigh_scale, steps);
    let (_near, far) = shell_hits(b, r2, aerosol + radius).unwrap_or((-1.0, -1.0));
    let mie_depth = if far > 0.0 { depth_along([0.0, r], [sin, mu], far, radius, mie_scale, steps) } else { 0.0 };
    let mut cloud = 0.0;
    if inp.cloudiness > 0.0 {
        let (thick, base) = (inp.cloud_thickness, inp.cloud_height);
        let (top_near, top_far) = shell_hits(b, r2, thick + base + radius).unwrap_or((-1.0, -1.0));
        let (bottom_far, bottom_exit) = match shell_hits(b, r2, base + radius) {
            Some((n, f)) => (f, if n < 0.0 { top_far } else { n }),
            None => (-1.0, top_far),
        };
        let seg_a = if top_near >= 0.0 { (top_near, bottom_exit) } else { (0.0, 0.0) };
        let seg_b = if h <= base + thick { (0.0, bottom_exit) } else { seg_a };
        let seg = if base <= h { seg_b } else { (bottom_far, top_far) };
        cloud = cloud_k * (seg.1 - seg.0).max(0.0);
    }
    std::array::from_fn(|i| ((rayleigh[i] * ray_depth + mie[i] * mie_depth + cloud) * -12.566_371).exp())
}

/// RGB to hue (0..6), lightness, saturation, alpha.
// Translated from 0x00cf58d0 (DDDA.exe 2017-12-13 Steam)
pub fn rgb_to_hls(c: [f32; 4]) -> [f32; 4] {
    let [r, g, b, a] = c;
    let max = if g < r {
        if r <= b { b } else { r }
    } else if g <= b {
        b
    } else {
        g
    };
    let min = if r < g {
        if b <= r { b } else { r }
    } else if b <= g {
        b
    } else {
        g
    };
    let sum = min + max;
    let l = sum * 0.5;
    if min == max {
        return [0.0, l, 0.0, a];
    }
    let d = max - min;
    let s = d / if 0.5 <= l { (2.0 - max) - min } else { sum };
    let mut h = if r == max {
        (g - b) / d
    } else if g == max {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    if h < 0.0 {
        h += 6.0;
    }
    if 6.0 <= h {
        h -= 6.0;
    }
    [h, l, s, a]
}

/// Inverse of [`rgb_to_hls`].
// Translated from 0x00cf5a10 (DDDA.exe 2017-12-13 Steam)
pub fn hls_to_rgb(c: [f32; 4]) -> [f32; 4] {
    let [h, l, s, a] = c;
    let m2 = if 0.5 < l { (l + s) - l * s } else { (s + 1.0) * l };
    let m1 = l * 2.0 - m2;
    if s == 0.0 {
        return [l, l, l, a];
    }
    let channel = |mut x: f32| {
        if 6.0 <= x {
            x -= 6.0;
        }
        if x < 0.0 {
            x += 6.0;
        }
        if x < 1.0 {
            (m2 - m1) * x + m1
        } else if x < 3.0 {
            m2
        } else if x < 4.0 {
            (m2 - m1) * (4.0 - x) + m1
        } else {
            m1
        }
    };
    [channel(h + 2.0), channel(h), channel(h - 2.0), a]
}

/// The sun colour at `height_cm` above the sky's height offset: the
/// transmittance times the sky's sun colour.
// Translated from 0x00f32fd0 (DDDA.exe 2017-12-13 Steam)
pub fn sun_at_height(sky: &Sky, inp: &SkyInputs, height_cm: f32, sun_dir: [f32; 3]) -> [f32; 3] {
    let t = transmittance(sky, inp, inp.height_offset + height_cm, sun_dir);
    std::array::from_fn(|i| t[i] * inp.sun_color[i])
}

/// The sky's ambient colour at `height_cm` above its height offset, times
/// `scale`: the resource's +0x70 colour times (1 - sun height)^16 (1 below
/// the horizon) plus the sun colour there times the Rayleigh and Mie
/// scattering at that height (densities exp(-h / scale height)) x 4 pi.
// Translated from 0x00f330a0 (DDDA.exe 2017-12-13 Steam)
pub fn ambient_at_height(sky: &Sky, inp: &SkyInputs, height_cm: f32, scale: f32, sun_dir: [f32; 3]) -> [f32; 3] {
    let (rayleigh, mie, _) = coefficients(sky, inp);
    let a = &sky.scalars_a0_bc;
    let h = (inp.height_offset + height_cm) * -1e-5;
    let er = (h / a[4]).exp();
    let em = (h / a[6]).exp();
    let sun = sun_at_height(sky, inp, height_cm, sun_dir);
    let night = if 0.0 <= sun_dir[1] { 1.0 - sun_dir[1] } else { 1.0 };
    let base = f64::from(night).powf(16.0) as f32;
    std::array::from_fn(|i| {
        sky.vector_70[i] * base + sun[i] * (rayleigh[i] * er + mie[i] * em) * 4.0 * std::f32::consts::PI * scale
    })
}

/// A cloud dome's (`uSkyCloudModel`) sun colour: the sun at
/// `mSunColorHeight` times `mSunColorScale`, its HLS saturation times
/// `mSaturationScale`, times `mIntensityScale`; and its ambient colour at
/// `mAmbColorHeight` times `mAmbColorScale`.
// Translated from 0x00c64620 (DDDA.exe 2017-12-13 Steam)
pub fn cloud_light(
    sky: &Sky,
    inp: &SkyInputs,
    sun_dir: [f32; 3],
    unit: &CloudUnit,
) -> ([f32; 3], [f32; 3]) {
    let s = sun_at_height(sky, inp, unit.sun_height, sun_dir);
    let mut hls = rgb_to_hls([s[0] * unit.sun_scale, s[1] * unit.sun_scale, s[2] * unit.sun_scale, 1.0]);
    hls[2] *= unit.saturation;
    let rgb = hls_to_rgb(hls);
    let sun = [rgb[0] * unit.intensity, rgb[1] * unit.intensity, rgb[2] * unit.intensity];
    (sun, ambient_at_height(sky, inp, unit.amb_height, unit.amb_scale, sun_dir))
}

/// The light properties of a `uSkyCloudModel`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudUnit {
    pub sun_height: f32,
    pub sun_scale: f32,
    pub saturation: f32,
    pub intensity: f32,
    pub amb_height: f32,
    pub amb_scale: f32,
}

/// The weather manager's day/night blend (`mDayNightBlendRate`): 0 by day
/// (sun over 0.2 up), 1 by night (under -0.2), linear between.
// Translated from 0x004c18c0 (DDDA.exe 2017-12-13 Steam)
pub fn night_blend(sun_dir: [f32; 3]) -> f32 {
    let len = (sun_dir[0] * sun_dir[0] + sun_dir[1] * sun_dir[1] + sun_dir[2] * sun_dir[2]).sqrt();
    let up = (1.0 / len * sun_dir[1]).clamp(-0.2, 0.2);
    (-(up * 5.0) + 1.0) * 0.5
}

/// Colour of the sky's infinite (sun) light, `uSkyInfiniteLight`: the sun
/// colour through the atmosphere to 100 cm above the sky's height offset
/// (0x00f32fd0), times `mLightScale`, its HLS saturation times the
/// weather's `SaturationScale`, times its `IntensityScale`, faded by the
/// night blend (the unit's branch without a night colour).
// Translated from 0x00c688a0 (DDDA.exe 2017-12-13 Steam)
pub fn sun_light_color(
    sky: &Sky,
    inp: &SkyInputs,
    sun_dir: [f32; 3],
    light_scale: f32,
    intensity: f32,
    saturation: f32,
) -> [f32; 3] {
    let t = transmittance(sky, inp, inp.height_offset + 100.0, sun_dir);
    let c = [
        t[0] * inp.sun_color[0] * light_scale,
        t[1] * inp.sun_color[1] * light_scale,
        t[2] * inp.sun_color[2] * light_scale,
        1.0,
    ];
    let mut hls = rgb_to_hls(c);
    hls[2] *= saturation;
    let rgb = hls_to_rgb(hls);
    let fade = 1.0 - night_blend(sun_dir);
    [rgb[0] * intensity * fade, rgb[1] * intensity * fade, rgb[2] * intensity * fade]
}

/// One optical depth map, `DEPTH_MAP_SIZE` squared, row-major: texel (x, y)
/// holds the density integral along a ray from height `x` (over `top`) with
/// zenith cosine `2 y - 1` to the top of the layer, density `exp(-h /
/// scale)` (1 below the ground). Rayleigh: `top`, `scale` = `CBSky[3]`,
/// `CBSky[7]`; Mie: `CBSky[11]`, `CBSky[15]`. `radius` = `CBSky[19]`.
/// Texel centres are at `(i + 0.5) / size` (assumed, the pass's vertex
/// shader is not traced).
// Translated from the pixel shaders at file offsets 0x59745C (Rayleigh) and 0x38E804 (Mie) of sc\dx9\root (DDDA 2017-12-13 Steam)
pub fn optical_depth_map(top: f32, scale: f32, radius: f32) -> Vec<f32> {
    let n = DEPTH_MAP_SIZE;
    let mut out = vec![0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32;
            let v = (y as f32 + 0.5) / n as f32;
            out[y * n + x] = optical_depth(u, v, top, scale, radius);
        }
    }
    out
}

// The shaders' 1.44269502.
const LOG2_E: f32 = std::f32::consts::LOG2_E;

fn density(p: [f32; 2], scale: f32, radius: f32) -> f32 {
    let h = (p[0] * p[0] + p[1] * p[1]).sqrt() - radius;
    let h = if h >= 0.0 { -h } else { 0.0 };
    (h * (1.0 / scale) * LOG2_E).exp2()
}

fn optical_depth(u: f32, v: f32, top: f32, scale: f32, radius: f32) -> f32 {
    let r = u * top + radius;
    let mu = v * 2.0 - 1.0;
    let sin = (1.0 - mu * mu).abs().sqrt();
    let outer = radius + top;
    let b = 2.0 * r * mu;
    let disc = b * b - (r * r - outer * outer) * 4.0;
    // One of 32 steps to the outer shell (half the quadratic's root).
    let ds = if disc >= 0.0 { (disc.abs().sqrt() - b) * (1.0 / 64.0) } else { -1.0 / 32.0 };
    let mut p = [sin * ds, mu * ds + r];
    let mut prev = density(p, scale, radius);
    let mut acc = 0.0;
    for _ in 0..32 {
        p = [sin * ds + p[0], mu * ds + p[1]];
        let d = density(p, scale, radius);
        acc += (prev + d) * 0.5;
        prev = d;
    }
    ds * acc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values of the original depth map shaders, evaluated instruction by
    /// instruction (research interpreter, not in the repo) with the
    /// `st_all_sky_F` atmosphere: Rayleigh top 100 km, scale 25 km; Mie top
    /// 10 km, scale 2.5 km; earth radius 6367 km.
    #[test]
    fn matches_the_depth_shaders() {
        let cases: [(f32, f32, &[(usize, usize, f64)]); 2] = [
            (
                100_000.0,
                25_000.0,
                &[
                    (0, 0, 12192865.26),
                    (0, 128, 437151.486),
                    (0, 255, 21603.619),
                    (10, 130, 309364.930),
                    (128, 128, 58315.582),
                    (255, 0, 12286753.55),
                    (40, 200, 20258.925),
                    (3, 127, 462779.306),
                ],
            ),
            (
                10_000.0,
                2_500.0,
                &[
                    (0, 0, 12106535.96),
                    (0, 128, 124222.201),
                    (0, 255, 2160.4096),
                    (10, 130, 63854.637),
                    (128, 128, 16474.152),
                    (255, 0, 12115924.27),
                    (40, 200, 2037.7749),
                    (3, 127, 164619.815),
                ],
            ),
        ];
        for (top, scale, samples) in cases {
            for &(x, y, want) in samples {
                let u = (x as f32 + 0.5) / 256.0;
                let v = (y as f32 + 0.5) / 256.0;
                let got = f64::from(optical_depth(u, v, top, scale, 6_367_000.0));
                assert!((got - want).abs() <= want * 2e-3, "{top} ({x}, {y}): {got} vs {want}");
            }
        }
    }

    fn sky_f() -> Sky {
        Sky {
            vector_60: [21.6, 23.49, 27.0, 0.0],
            scalar_90: 0.000933,
            vector_80: [665.0, 555.0, 455.0, 0.0],
            vector_70: [0.0; 4],
            scalar_94: 23.4,
            word_98: 0,
            word_9c: 90,
            scalars_a0_bc: [47.0, 1.0, 6367.0, 100.0, 25.0, 10.0, 2.5, 0.0],
            scalars_c4_cc: [0.1, 1392000.0, 149597870.0],
            sun_texture: Vec::new(),
            matrix_d0: [0.0; 16],
            scalars_110_120: [0.0; 5],
            vector_140: [0.0; 4],
            scalar_130: 1.0,
            scalars_124_12c: [0.0; 3],
            scalar_150: 0.01,
            moon_texture: Vec::new(),
        }
    }

    /// Sun light colours read live (refs-lit series, cloudy weather: the
    /// live fog equals st100_fog_e) for the live sun directions.
    #[test]
    fn sun_light_matches_the_live_reads() {
        let inp = SkyInputs {
            sun_color: [16.388, 17.0, 15.300001],
            mie_scattering: [7.55; 3],
            cloudiness: 0.5,
            cloud_height: 2000.0,
            cloud_thickness: 150.0,
            cloud_scattering: 0.4,
            height_offset: 30000.0,
            ..SkyInputs::default()
        };
        for (dir, want) in [
            ([0.4994f32, 0.8156, 0.2923], [3.2974f32, 3.1546, 2.3459]),
            ([-0.0897, 0.9134, 0.3971], [3.5077, 3.3847, 2.5685]),
            ([-0.6547, 0.7291, 0.1995], [3.0761, 2.9151, 2.1198]),
            ([-0.9163, 0.3260, -0.2328], [1.3858, 1.1803, 0.6673]),
        ] {
            let got = sun_light_color(&sky_f(), &inp, dir, 0.1, 3.6, 1.0);
            for i in 0..3 {
                assert!((got[i] - want[i]).abs() <= want[i] * 0.01, "{dir:?}: {got:?} vs {want:?}");
            }
        }
        // Night: the blend fades it and the ray crosses the earth.
        assert!(sun_light_color(&sky_f(), &inp, [-0.7438, -0.0762, -0.6641], 0.1, 3.6, 1.0)[0] < 1e-6);
        assert_eq!(night_blend([0.0, 0.5, 0.0]), 0.0);
        assert_eq!(night_blend([0.0, -0.5, 0.0]), 1.0);
    }

    #[test]
    fn cloud_light_rules() {
        let inp = SkyInputs { sun_color: [23.04, 23.936, 25.6], mie_scattering: [7.554876; 3], height_offset: 30000.0, ..SkyInputs::default() };
        let unit = CloudUnit { sun_height: 600_000.0, sun_scale: 0.1, saturation: 0.0, intensity: 1.0, amb_height: 600_000.0, amb_scale: 1.0 };
        let (sun, amb) = cloud_light(&sky_f(), &inp, [0.0, 1.0, 0.0], &unit);
        // Saturation 0: grey at the lightness of the scaled sun colour.
        assert!((sun[0] - sun[1]).abs() < 1e-6 && (sun[1] - sun[2]).abs() < 1e-6 && sun[0] > 1.0);
        // The resource's +0x70 colour is zero: the ambient is the in-scatter,
        // proportional to the scale.
        let unit2 = CloudUnit { amb_scale: 2.0, ..unit };
        let (_, amb2) = cloud_light(&sky_f(), &inp, [0.0, 1.0, 0.0], &unit2);
        for i in 0..3 {
            assert!(amb[i] > 0.0 && (amb2[i] - 2.0 * amb[i]).abs() < 1e-6);
        }
        // Blue scatters most.
        assert!(amb[2] > amb[0]);
    }

    #[test]
    fn hls_round_trip() {
        for c in [[0.9f32, 0.5, 0.2, 1.0], [0.1, 0.2, 0.7, 1.0], [0.4, 0.4, 0.4, 1.0], [0.97, 0.94, 0.71, 1.0]] {
            let back = hls_to_rgb(rgb_to_hls(c));
            for i in 0..4 {
                assert!((back[i] - c[i]).abs() < 1e-5, "{c:?} -> {back:?}");
            }
        }
    }

    /// Sun directions read live (refs-lit series) against the clock.
    #[test]
    fn sun_follows_the_clock() {
        let sky = sky_f();
        for (ms, want) in [
            (35_207_936u32, [0.4994f32, 0.8156, 0.2923]),
            (44_468_403, [-0.0897, 0.9134, 0.3971]),
            (54_043_959, [-0.6547, 0.7291, 0.1995]),
            (63_917_141, [-0.9163, 0.3260, -0.2328]),
        ] {
            let got = sun_direction(&sky, ms);
            for i in 0..3 {
                assert!((got[i] - want[i]).abs() < 0.006, "{ms}: {got:?} vs {want:?}");
            }
        }
        // Midnight: below the horizon.
        assert!(sun_direction(&sky, 0)[1] < -0.3);
    }

    #[test]
    fn fine_weather_constants() {
        let inp = SkyInputs {
            sun_color: [23.04, 23.936, 25.6],
            mie_scattering: [7.554876; 3],
            height_offset: 30000.0,
            starry_sky_intensity: 3.0,
            cloud_height: 4000.0,
            cloud_thickness: 100.0,
            ..SkyInputs::default()
        };
        let c = constants(&sky_f(), &inp, [0.0, 1.0, 0.0]);
        let close = |a: f32, b: f32| (a - b).abs() <= b.abs() * 1e-4;
        // Rayleigh: 0.001 / (100 * 0.01 / ((l / 455)^-4 * 0.000933)) * 4 pi
        let blue = 0.001 * 0.000933 / 1.0;
        assert!(close(c[2], blue * FOUR_PI), "{}", c[2]);
        assert!(close(c[0], blue * (665.0f32 / 455.0).powi(-4) * FOUR_PI), "{}", c[0]);
        assert!(close(c[6], blue * 0.75));
        // Mie: 0.001 / (100 * 7.554876) * density
        let mie = 0.001 / 755.4876;
        assert!(close(c[8], mie * FOUR_PI));
        assert!(close(c[12], mie * 0.84));
        assert_eq!([c[3], c[7], c[11], c[15], c[19], c[23]], [100000.0, 25000.0, 10000.0, 2500.0, 6367000.0, 256.0]);
        assert_eq!([c[24], c[25], c[26]], [23.04, 23.936, 25.6]);
        assert!(close(c[27], 0.03));
        assert_eq!(c[31], 0.0); // sun at the zenith: no stars
        assert_eq!([c[32], c[33]], [32.0, 32.0]);
        assert!(close(c[34], 1.16) && close(c[35], -0.8));
        assert_eq!([c[38], c[39], c[40], c[43]], [4100.0, 4000.0, 0.0, 30000.0]);
    }
}
