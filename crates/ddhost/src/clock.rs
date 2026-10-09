//! The host's fixed frame clock.
//!
//! The original measures time in 1/30-second frames and scales every
//! per-frame rule by a variable step (docs/RECORDINGS.md, "The clock"):
//! - `sMain.mFps` (+0x38) is 30.0, the base rate;
//! - `sMain::vf06` (0x00DBFA90) computes `mDeltaTime` (+0x70) from the
//!   QPC timer, in frames at `mFps`;
//! - each unit's `cUnit.mDeltaTime` (+0x14) is that step times its move
//!   line's rate (`sUnit` line record +0x10, `0x00DB7384`), and the rate
//!   is 1.0 on all 37 lines in a live read (claude-4, board #872).
//!
//! The host runs the same model at a constant step (a design decision for
//! determinism and co-op; see docs/adr/0002-game-host.md): one host frame
//! is `delta` original frames, 1.0 by default (30 host frames per second).

/// `sMain.mFps`: frames per second of the original's time unit.
pub const BASE_FPS: f32 = 30.0;

/// `sMain.mDeltaTimeLimite` (+0x78): the largest step, in frames.
/// `sBbsRpgMain::vf06` (0x0041DD60) writes 2.0 every frame, or 16.0 while
/// `sGameSys` +0xB8844 has bit 0x2000000 or 0x4000000 (meaning untraced).
pub const DELTA_LIMIT: f32 = 2.0;

/// A constant step, in original frames (1/30 s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameClock {
    pub delta: f32,
}

impl Default for FrameClock {
    fn default() -> Self {
        FrameClock { delta: 1.0 }
    }
}

impl FrameClock {
    /// A clock of `delta` frames per host frame, within (0, DELTA_LIMIT].
    pub fn new(delta: f32) -> Option<FrameClock> {
        (delta > 0.0 && delta <= DELTA_LIMIT).then_some(FrameClock { delta })
    }

    /// Host frames per second.
    pub fn rate(&self) -> f32 {
        BASE_FPS / self.delta
    }

    /// Seconds per host frame, as `ddsim::Sim::step` takes them.
    pub fn seconds(&self) -> f32 {
        self.delta / BASE_FPS
    }

    /// The story's view of the step (`sMain` mDeltaTime and mFps).
    pub fn story_time(&self) -> ddstory::Time {
        ddstory::Time {
            delta: self.delta,
            fps: BASE_FPS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Sim::step` accumulates `seconds * MOTION_FPS`; for the supported
    /// steps that product must be exact, or the 1/120-s ticks would drift
    /// against host frames.
    #[test]
    fn seconds_convert_back_exactly() {
        for d in [0.25f32, 0.5, 1.0, 2.0] {
            let c = FrameClock::new(d).unwrap();
            assert_eq!(c.seconds() * ddsim::MOTION_FPS, d, "delta {d}");
            assert_eq!(c.seconds() * ddsim::MOTION_FPS / ddsim::TICK, d * 4.0);
        }
    }

    #[test]
    fn limits() {
        assert!(FrameClock::new(0.0).is_none());
        assert!(FrameClock::new(2.5).is_none());
        assert_eq!(FrameClock::default().rate(), 30.0);
    }
}
