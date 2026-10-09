//! The game clock in play: it advances at the original's rate and the
//! stage lighting follows it.
//!
//! **Rate** (WEATHER_CLOCK.md, "The saved clock is a separate producer"):
//! `0x0043f2c0` adds `trunc(scale * f32(seconds * 30000))` game
//! milliseconds a frame, `seconds` being `mDeltaTime / mFps` (real time),
//! so 30 game seconds pass per real second (one day in 48 real minutes).
//! Matched to the millisecond over 239 recorded frames. The saved scale is 1
//! in every recording. A frame whose step truncates to 0 with a nonzero
//! scale takes the minimum step 1.
//!
//! **Lighting.** Each game minute the stage lighting is rebuilt from the
//! data at the new clock (`stage::data_lighting`: the sky scheduler's light
//! units at the sun, and the weather's fog) and written into every lit
//! material. The original evaluates its scheduler every frame; once a
//! game minute (2 real seconds) is the *viewer's* step, and the materials
//! are relit 64 a frame (all at once stalls a frame for ~400 ms). The sky dome's
//! constants (`CBSky`, `mtweather::sky::constants` at the new sun) are
//! renewed with it, for the dome and the materials that reflect it. Not
//! updated yet: the cloud domes' light, the environment cube tint and the
//! night spot light (they keep their start values).
//!
//! *Not modelled:* the pause predicate's cases (`0x0043f3d0`: events, SceNo
//! up to 1099, stages 800-804 and others). Here the clock stops only while
//! the viewer menu pauses play.

use bevy::prelude::*;

use crate::stage::{DdMaterial, StageLighting};

/// Game milliseconds in a day.
const DAY_MS: u32 = 24 * 3_600_000;

#[derive(Resource)]
pub struct GameClock {
    /// Milliseconds since midnight.
    pub ms: u32,
    /// Days passed (the original's 64-bit day count).
    pub days: u64,
    vfs: &'static ddvfs::Vfs,
    stage: u32,
    weather: usize,
    /// The minute the lighting was last built for.
    lit_minute: u32,
    sky: Option<crate::sky::StageSky>,
    /// Materials still to relight, a few per frame (a whole pass at once
    /// re-prepares thousands of materials in one frame: 400 ms hitches).
    pending: Vec<AssetId<DdMaterial>>,
    sky_cb: Option<[Vec4; 11]>,
}

/// Materials relit per frame.
const RELIGHT_PER_FRAME: usize = 64;

impl GameClock {
    pub fn new(vfs: &'static ddvfs::Vfs, stage: u32, weather: usize, ms: u32) -> Self {
        let sky = crate::sky::StageSky::load(vfs, stage, weather);
        GameClock { ms: ms % DAY_MS, days: 0, vfs, stage, weather, lit_minute: ms / 60_000, sky, pending: Vec::new(), sky_cb: None }
    }

    /// One frame of `seconds` real time.
    pub fn step(&mut self, seconds: f32) {
        (self.ms, self.days) = advance(self.ms, self.days, seconds);
    }
}

/// The clock after one frame of `seconds` real time: the step 0x0043f2c0
/// adds, with the day count on wrapping.
// Translated from 0x0043f2c0 (DDDA.exe 2017-12-13 Steam), scale 1.
pub fn advance(ms: u32, days: u64, seconds: f32) -> (u32, u64) {
    let scale = 1.0f32;
    let mut step = scale * (seconds * 30000.0);
    if scale != 0.0 && step as u32 == 0 {
        step = 1.0;
    }
    let total = u64::from(ms) + step as u64;
    ((total % u64::from(DAY_MS)) as u32, days + total / u64::from(DAY_MS))
}

/// Advances the clock and relights the stage each new game minute.
pub fn update(
    clock: Option<ResMut<GameClock>>,
    time: Res<Time<Real>>,
    pause: Res<crate::pause::PauseState>,
    mut lighting: ResMut<StageLighting>,
    mut materials: ResMut<Assets<DdMaterial>>,
    mut skies: ResMut<Assets<crate::sky::SkyMaterial>>,
    mut env: Option<ResMut<crate::stage::SkyEnv>>,
) {
    let _timing = crate::perf::Section::start("clock::update");
    let Some(mut c) = clock else { return };
    if pause.paused {
        return;
    }
    c.step(time.delta_secs());
    let c = &mut *c;
    let minute = c.ms / 60_000;
    if minute != c.lit_minute {
        c.lit_minute = minute;
        if let Some(mut l) = crate::stage::data_lighting(c.vfs, c.stage, c.ms, c.weather) {
            // keep the night spot light the start set up (it follows the player)
            l.spot = lighting.spot;
            *lighting = l;
            c.sky_cb = c.sky.as_ref().map(|sky| {
                let sun = mtweather::sky::sun_direction(&sky.sky, c.ms);
                let cb = mtweather::sky::constants(&sky.sky, &sky.inputs, sun);
                std::array::from_fn::<Vec4, 11, _>(|i| Vec4::from_slice(&cb[4 * i..4 * i + 4]))
            });
            // the dome is one material: at once
            if let Some(cb) = c.sky_cb {
                for (_, m) in skies.iter_mut() {
                    m.params.cb = cb;
                }
                if let Some(env) = env.as_mut() {
                    env.params.cb = cb;
                }
            }
            // a new pass (an unfinished one continues with the new values)
            if c.pending.is_empty() {
                c.pending = materials.ids().collect();
            }
        }
    }
    for _ in 0..RELIGHT_PER_FRAME {
        let Some(id) = c.pending.pop() else { break };
        if let Some(mut m) = materials.get_mut(id) {
            m.extension.params = m.extension.params.relit(&lighting);
            if let Some(cb) = c.sky_cb {
                m.extension.set_sky_cb(cb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_game_seconds_a_real_second_truncated_per_frame() {
        // 60 frames of 1/60 s: 500 ms each
        let mut c = (0, 0);
        for _ in 0..60 {
            c = advance(c.0, c.1, 1.0 / 60.0);
        }
        assert_eq!(c, (30_000, 0));
        // a recorded frame: mDeltaTime 0.330672 at mFps 30 is 330.67 ms, truncated
        assert_eq!(advance(0, 0, 0.330_672 / 30.0), (330, 0));
        // a frame too short for a millisecond still moves by 1
        assert_eq!(advance(0, 0, 0.00001), (1, 0));
        // wrapping midnight counts a day
        assert_eq!(advance(DAY_MS - 100, 0, 0.01), (200, 1));
    }
}
