//! Post-processing rules of the stage filters (docs/SHADING.md, "Post
//! chain"): the colour-correction tone curve.
//!
//! The stage-100 filter scheduler `scr\st100\sdl\st100_f00` creates a
//! `uColorCorrectFilter` whose `ToneCurveRGB` is an MtHermiteCurve (8 x, 8 y)
//! and `mGamma` a float3. The filter renders them into a lookup table with a
//! pixel shader (`sc\dx9\root` file offset 0x47800C, constants
//! `fHermiteParam[8]` = (x, y) and `fColorCorrectGamma`) and applies the
//! table per channel to the linear scene colour (shaders at 0x3A2D38 /
//! 0x48B98C).

/// The tone curve at `x` (linear in, linear out), as the table shader
/// computes it: a cubic Hermite segment between the two points around `x`,
/// with the tangent at an inner point the average of the neighbouring
/// differences rescaled to the current segment's width, and one-sided
/// differences at the first and last point. The result is saturated.
// Translated from the pixel shader at file offset 0x47800C of sc\dx9\root (DDDA 2017-12-13 Steam)
pub fn tone_curve(xs: &[f32; 8], ys: &[f32; 8], x: f32) -> f32 {
    // Points in use: the leading run with strictly increasing x.
    let mut n = 1;
    while n < 8 && xs[n] > xs[n - 1] {
        n += 1;
    }
    if n < 2 {
        return ys[0].clamp(0.0, 1.0);
    }
    if x <= xs[0] {
        return ys[0].clamp(0.0, 1.0);
    }
    if x >= xs[n - 1] {
        return ys[n - 1].clamp(0.0, 1.0);
    }
    let k = (0..n - 1).rfind(|&k| xs[k] <= x).unwrap_or(0);
    let w = xs[k + 1] - xs[k];
    let t = (x - xs[k]) / w;
    let dy = ys[k + 1] - ys[k];
    let m0 = if k == 0 {
        dy
    } else {
        0.5 * ((ys[k] - ys[k - 1]) * w / (xs[k] - xs[k - 1]) + dy)
    };
    let m1 = if k + 2 >= n {
        dy
    } else {
        0.5 * (dy + (ys[k + 2] - ys[k + 1]) * w / (xs[k + 2] - xs[k + 1]))
    };
    let t2 = t * t;
    let t3 = t2 * t;
    let y = (2.0 * t3 - 3.0 * t2 + 1.0) * ys[k]
        + (t3 - 2.0 * t2 + t) * m0
        + (-2.0 * t3 + 3.0 * t2) * ys[k + 1]
        + (t3 - t2) * m1;
    y.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 5e-4
    }

    /// Values of the original table shader, evaluated instruction by
    /// instruction on these curves (research interpreter, not in the repo).
    #[test]
    fn matches_the_table_shader() {
        let cases: [([f32; 8], [f32; 8], &[(f32, f32)]); 3] = [
            (
                // st100_f00's ToneCurveRGB
                [0.0, 0.2, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                [0.0, 0.155738, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                &[(0.05, 0.0376), (0.1, 0.0744), (0.2, 0.1557), (0.3, 0.2507), (0.5, 0.4561), (0.6, 0.564), (0.9, 0.893)],
            ),
            (
                [0.0, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                [0.0, 0.25, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                &[(0.1, 0.042), (0.25, 0.0938), (0.75, 0.5938), (0.9, 0.842)],
            ),
            (
                [0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0, 1.0],
                [0.0, 0.1, 0.6, 0.7, 1.0, 1.0, 1.0, 1.0],
                &[(0.1, 0.0208), (0.3, 0.1808), (0.4, 0.4096), (0.6, 0.6592), (0.8, 0.7472)],
            ),
        ];
        for (xs, ys, samples) in cases {
            for &(x, y) in samples {
                let got = tone_curve(&xs, &ys, x);
                assert!(close(got, y), "{xs:?} at {x}: {got} vs {y}");
            }
        }
    }
}
