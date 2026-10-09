//! Stage lighting inputs of the material pixel shaders (docs/SHADING.md):
//! the ambient spherical harmonics `fSHCoef[7]` built from the sky's
//! hemisphere and infinite lights.
//!
//! The per-light projections are translated from the exe; they compute the
//! same coefficients as D3DX's `D3DXSHEvalDirectionalLight` (factor 16*pi/17)
//! and `D3DXSHEvalHemisphereLight` (factor 3*sqrt(pi)), order 3, in D3DX's
//! basis order and signs. The packing into the seven float4 registers the
//! pixel shaders read is Sloan's irradiance packing ("Stupid Spherical
//! Harmonics Tricks"). *Assumed:* the engine's packer is not traced; the
//! pixel shaders evaluate exactly that form (`dp4(c0..c2, (N,1)) +
//! dp4(c3..c5, N.yzzx*N.xyzz) + c6 * (x^2 - y^2)`), and renders with it match
//! the shadowed and lit ground of the live captures to within the original's
//! untraced post-processing.
//!
//! Which lights take part: a light applies to a model when its
//! `uLight.mGroup` (+0x34) shares a bit with the model's light group (MOD
//! header, 1 for stage cells and scenery). *Assumed from data:* the sky's
//! group-0x10 lights (a grey hemisphere and a weak infinite light) would
//! double the ambient of the captures if included.

/// Order-3 SH coefficients of one colour channel, D3DX basis order.
pub type Sh9 = [f32; 9];

/// Coefficients of a directional light of colour `color` towards `dir`.
// Translated from 0x0130d420 (DDDA.exe 2017-12-13 Steam)
pub fn sh_directional(dir: [f32; 3], color: [f32; 3]) -> [Sh9; 3] {
    const SCALE: f32 = 2.956793;
    let z = dir[2];
    let nx = -dir[0];
    let ny = -dir[1];
    let basis = [
        0.282095,
        ny * 0.488603,
        z * 0.488603,
        nx * 0.488603,
        ny * 1.0952548 * nx,
        ny * 1.0952548 * z,
        (z * 3.0 * z - 1.0) * 0.315392,
        z * 1.0952548 * nx,
        (nx * nx - ny * ny) * 0.546274,
    ];
    std::array::from_fn(|c| std::array::from_fn(|k| color[c] * basis[k] * SCALE))
}

/// Coefficients of a hemisphere light: `sky` colour around `dir`,
/// `ground` colour opposite.
// Translated from 0x0130d6d0 (DDDA.exe 2017-12-13 Steam)
pub fn sh_hemisphere(dir: [f32; 3], sky: [f32; 3], ground: [f32; 3]) -> [Sh9; 3] {
    let s = 2.0466514f32 * 1.5000013;
    let (dz, dx, dy) = (dir[2] * s, -dir[0] * s, -dir[1] * s);
    std::array::from_fn(|c| {
        let half_diff = (sky[c] - ground[c]) * 0.5;
        [
            (sky[c] + ground[c]) * 0.5 * 5.317363,
            half_diff * dy,
            half_diff * dz,
            half_diff * dx,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ]
    })
}

/// Adds `b` into `a`.
pub fn sh_add(a: &mut [Sh9; 3], b: &[Sh9; 3]) {
    for c in 0..3 {
        for k in 0..9 {
            a[c][k] += b[c][k];
        }
    }
}

/// The seven `fSHCoef` registers (irradiance / pi) for RGB coefficients.
/// *Assumed* (Sloan's packing, see the module docs).
pub fn pack_sh(l: &[Sh9; 3]) -> [[f32; 4]; 7] {
    let sqrt_pi = std::f32::consts::PI.sqrt();
    let c0 = 1.0 / (2.0 * sqrt_pi);
    let c1 = 3.0f32.sqrt() / (3.0 * sqrt_pi);
    let c2 = 15.0f32.sqrt() / (8.0 * sqrt_pi);
    let c3 = 5.0f32.sqrt() / (16.0 * sqrt_pi);
    let c4 = 0.5 * c2;
    let mut out = [[0.0; 4]; 7];
    for c in 0..3 {
        let s = &l[c];
        out[c] = [-c1 * s[3], -c1 * s[1], c1 * s[2], c0 * s[0] - c3 * s[6]];
        out[3 + c] = [c2 * s[4], -c2 * s[5], 3.0 * c3 * s[6], -c2 * s[7]];
    }
    out[6] = [c4 * l[0][8], c4 * l[1][8], c4 * l[2][8], 1.0];
    out
}

/// What the pixel shaders compute from `fSHCoef` for a unit normal `n`.
pub fn eval_sh(sh: &[[f32; 4]; 7], n: [f32; 3]) -> [f32; 3] {
    let n1 = [n[0], n[1], n[2], 1.0];
    let q = [n[1] * n[0], n[2] * n[1], n[2] * n[2], n[0] * n[2]];
    let dot = |a: &[f32; 4], b: &[f32; 4]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let t = n[0] * n[0] - n[1] * n[1];
    std::array::from_fn(|c| dot(&sh[c], &n1) + dot(&sh[3 + c], &q) + sh[6][c] * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn directional_light_gives_its_colour_facing_it() {
        // D3DX normalisation: a white light seen head-on evaluates to 1.
        for dir in [[0.0, 1.0, 0.0], [0.6, 0.0, 0.8], [0.0, 0.0, -1.0]] {
            let sh = pack_sh(&sh_directional(dir, [1.0, 0.5, 0.25]));
            let e = eval_sh(&sh, dir);
            assert!(close(e[0], 1.0) && close(e[1], 0.5) && close(e[2], 0.25), "{dir:?}: {e:?}");
        }
    }

    #[test]
    fn hemisphere_light_values() {
        // sky 1, ground 0 around +Y: values of the D3DX-equivalent
        // projection, packed: 1.25 * sky + 0.25 * ground facing the sky.
        let sh = pack_sh(&sh_hemisphere([0.0, 1.0, 0.0], [1.0; 3], [0.0; 3]));
        let up = eval_sh(&sh, [0.0, 1.0, 0.0]);
        let down = eval_sh(&sh, [0.0, -1.0, 0.0]);
        assert!(close(up[0], 1.25) && close(down[0], 0.25), "{up:?} {down:?}");
        // Sideways sees the average.
        let side = eval_sh(&sh, [1.0, 0.0, 0.0]);
        assert!(close(side[0], 0.75), "{side:?}");
    }
}
