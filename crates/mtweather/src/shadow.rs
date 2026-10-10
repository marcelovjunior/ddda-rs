//! Source-derived stable-shadow geometry. These helpers do not choose a
//! graphics quality, allocate maps, or reproduce the original projection.

/// Far boundaries for the automatic cascade split mode. `far` is the
/// camera far distance already capped by the shadow's view distance.
/// Distribution weights the linear split; zero is logarithmic, one linear.
/// Explicit custom splits (+0x180 enabled, +0x184/+0x188) use another path.
// Translated from 0x00ee5ce0, specifically 0x00ee7980..0x00ee7a79
// (DDDA.exe 2017-12-13 Steam). The pow helper takes XMM0=far/near,
// XMM1=cascade fraction; disassembly confirms the decompiler's lost return.
pub fn cascade_boundaries(near: f32, far: f32, count: usize, distribution: f32) -> Option<Vec<f32>> {
    if !near.is_finite()
        || !far.is_finite()
        || !distribution.is_finite()
        || near <= 0.0
        || far <= near
        || count == 0
        || count > 32
    {
        // Defensive API bound, not an original engine limit. The original
        // constructor 0x00eefc60 selects three cascades.
        return None;
    }
    Some(
        (1..=count)
            .map(|i| {
                let fraction = i as f32 / count as f32;
                let logarithmic = (far / near).powf(fraction) * near;
                let linear = (far - near) * fraction + near;
                (linear - logarithmic) * distribution + logarithmic
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::cascade_boundaries;

    #[test]
    fn cassardis_authored_distribution_has_three_distinct_ranges() {
        // Recorded active camera near=16 cm; scheduler view=6000 cm,
        // distribution=.25. Expected values calculated in f64 separately.
        let got = cascade_boundaries(16.0, 6000.0, 3, 0.25).unwrap();
        for (actual, expected) in got.iter().zip([589.201_66, 1625.358_5, 6000.0]) {
            assert!((actual - expected).abs() < 0.001);
        }
    }

    #[test]
    fn distribution_endpoints_preserve_linear_and_logarithmic_rules() {
        assert_eq!(
            cascade_boundaries(1.0, 1000.0, 3, 1.0).unwrap(),
            [334.0, 667.0, 1000.0]
        );
        let logarithmic = cascade_boundaries(1.0, 1000.0, 3, 0.0).unwrap();
        for (actual, expected) in logarithmic.iter().zip([10.0, 100.0, 1000.0]) {
            assert!((actual - expected).abs() < 0.0001);
        }
        assert_eq!(cascade_boundaries(16.0, 6000.0, 1, 0.25).unwrap(), [6000.0]);
        assert!(cascade_boundaries(0.0, 6000.0, 3, 0.25).is_none());
        assert!(cascade_boundaries(16.0, 6000.0, 0, 0.25).is_none());
    }
}
