//! Near-camera actor opacity, translated from 0x0088bdc0 (DDDA.exe
//! 2017-12-13 Steam). Camera/actor selection and the separate aiming-mode
//! pawn attenuation are caller responsibilities.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Visible,
    FadingOut,
    Hidden,
    FadingIn,
}

/// State belongs to one actor in one view, not to the shared simulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct NearFade {
    pub phase: Phase,
    pub frames: f32,
}

impl NearFade {
    /// `distance_sq` is eye-to-semantic-joint-1 distance; `horizontal_sq`
    /// excludes Y. `inner` and `outer` are the selected executable values.
    /// A bypass returns full opacity without resetting the existing state.
    pub fn step(&mut self, distance_sq: f32, horizontal_sq: f32,
        inner: f32, outer: f32, delta_frames: f32, bypass: bool) -> f32 {
        if bypass { return 1.0; }
        match self.phase {
            Phase::Visible => {
                if distance_sq < 40_000.0 && horizontal_sq < inner * inner {
                    self.phase = Phase::FadingOut;
                    self.frames = 0.0;
                }
                1.0
            }
            Phase::FadingOut => {
                self.frames += delta_frames;
                if self.frames >= 4.0 {
                    self.frames = 4.0;
                    self.phase = Phase::Hidden;
                }
                1.0 - self.frames * 0.25
            }
            Phase::Hidden => {
                if distance_sq > 40_000.0 || horizontal_sq > outer * outer {
                    self.phase = Phase::FadingIn;
                    self.frames = 0.0;
                }
                0.0
            }
            Phase::FadingIn => {
                self.frames += delta_frames;
                if self.frames >= 5.0 { self.phase = Phase::Visible; }
                // The source does not clamp this: its render consumer treats
                // opacity >= 1 as opaque (uPlayerBase::vf11 0x00b6a070).
                self.frames * 0.25
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thresholds_are_strict_and_hysteretic() {
        let mut f = NearFade::default();
        assert_eq!(f.step(40_000.0, 0.0, 64.0, 74.0, 1.0, false), 1.0);
        assert_eq!(f.phase, Phase::Visible);
        f.step(100.0, 64.0 * 64.0, 64.0, 74.0, 1.0, false);
        assert_eq!(f.phase, Phase::Visible);
        f.step(100.0, 0.0, 64.0, 74.0, 1.0, false);
        assert_eq!(f.phase, Phase::FadingOut);
        for alpha in [0.75, 0.5, 0.25, 0.0] {
            assert_eq!(f.step(100.0, 0.0, 64.0, 74.0, 1.0, false), alpha);
        }
        f.step(40_000.0, 74.0 * 74.0, 64.0, 74.0, 1.0, false);
        assert_eq!(f.phase, Phase::Hidden);
        assert_eq!(f.step(40_001.0, 0.0, 64.0, 74.0, 1.0, false), 0.0);
        for alpha in [0.25, 0.5, 0.75, 1.0, 1.25] {
            assert_eq!(f.step(100.0, 0.0, 64.0, 74.0, 1.0, false), alpha);
        }
        assert_eq!(f.phase, Phase::Visible);
    }
    #[test]
    fn bypass_preserves_state_and_views_are_independent() {
        let mut a = NearFade::default();
        let b = NearFade::default();
        a.step(0.0, 0.0, 64.0, 74.0, 1.0, false);
        assert_eq!(a.step(0.0, 0.0, 64.0, 74.0, 2.0, true), 1.0);
        assert_eq!(a.frames, 0.0);
        assert_eq!(a.step(0.0, 0.0, 64.0, 74.0, 0.5, false), 0.875);
        assert_eq!(b.phase, Phase::Visible);
        assert_eq!(a.step(0.0, 0.0, 64.0, 74.0, 100.0, false), 0.0);
    }
}
