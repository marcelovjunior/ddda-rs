//! Pure weather-clock step, not the saved game clock or a wall-clock timer.
//! Inputs expose the executable's gates. See docs/WEATHER_CLOCK.md.

use crate::{Error, Result};

pub const DAY_MS: u32 = 86_400_000;

/// Resolved inputs to 0x004c0e30. No process access or global state.
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub milliseconds: u32,
    /// sWeatherManager +0x40: only the byte value 1 means enabled.
    pub sync_game_time: bool,
    /// sWeatherManager.Pause (+0x41); ignored in special battle modes.
    pub pause: bool,
    /// sWeatherManager.mTimeScale (+0x44), initialized to 1 by 0x004bb200.
    pub time_scale: f32,
    pub last_battle_mode: bool,
    /// sGameSys.SceNo == 8800 selects the same special branch.
    pub scenario: u32,
    pub boss_hp_rate: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// sMain.mDeltaTime, in base-rate frames, not seconds.
    pub delta: f32,
    /// sMain.mFps, the base rate (30 in recording 005707).
    pub base_fps: f32,
    /// (*sMain +0x68 == 0) && sMain.mPause.
    pub main_pause_gate: bool,
    /// Low-byte result of 0x0043f480. Caller must resolve the event flags.
    pub event_pause_gate: bool,
    /// sGameSys +0xb8768, copied when ordinary SyncGameTime is enabled.
    pub saved_milliseconds: u32,
}

/// Clock seconds and minutes discard subsecond time, as 0x00437210 does.
pub fn hms(milliseconds: u32) -> [u32; 3] {
    // Translated from 0x00437210 (DDDA.exe 2017-12-13 Steam).
    let seconds = milliseconds / 1000 % 86_400;
    [seconds / 3600, seconds / 60 % 60, seconds % 60]
}

/// Advance the local weather clock, returning its raw millisecond member.
/// Rejects non-finite inputs, local times above the day endpoint, multi-day steps and
/// reverse special-battle progression (whose unsigned cap path is unsupported).
/// Saved-clock copying accepts the entire u32 range, just like the executable.
pub fn step(state: State, frame: Frame) -> Result<u32> {
    // Translated from 0x004c0e30 / 0x004c2b60 (DDDA.exe 2017-12-13 Steam).
    // x87 FLDCW with RC=11 at 0x004c10b2 means truncation, not nearest rounding.
    let battle = state.last_battle_mode || state.scenario == 8800;
    if state.sync_game_time && !battle {
        return Ok(frame.saved_milliseconds);
    }
    if state.milliseconds > DAY_MS
        || !state.time_scale.is_finite()
        || !frame.delta.is_finite()
        || !frame.base_fps.is_finite()
        || (battle && state.sync_game_time && !state.boss_hp_rate.is_finite())
    {
        return Err(Error("unsupported clock input".into()));
    }
    let paused = if battle {
        frame.main_pause_gate || frame.event_pause_gate
    } else {
        state.pause
    };
    let mut delta = if paused || frame.base_fps <= f32::EPSILON {
        0.0
    } else {
        state.time_scale * ((frame.delta / frame.base_fps) * 30_000.0)
    };
    if battle && state.sync_game_time {
        delta *= 24.0;
        if delta < 0.0 {
            return Err(Error("reverse special-battle clock is unsupported".into()));
        }
    }
    if !delta.is_finite() || delta.abs() >= DAY_MS as f32 {
        return Err(Error("multi-day clock step is unsupported".into()));
    }
    let mut milliseconds = state.milliseconds;
    if battle && state.sync_game_time {
        let cap = if state.boss_hp_rate > 0.8 {
            16_200_000
        } else if state.boss_hp_rate > 0.6 {
            46_800_000
        } else if state.boss_hp_rate > 0.4 {
            75_600_000
        } else {
            86_399_000
        };
        // Integer input is loaded by FILD and added in x87 extended precision.
        if (milliseconds as f64 + delta as f64).trunc() > cap as f64 {
            milliseconds = cap;
            delta = 0.0;
        }
    }
    // Preserve pre-add wrap and separate truncation of delta. In particular,
    // do not cast the whole sum to f32: that loses integer milliseconds late
    // in the day. The original uses x87 integer loads for these comparisons.
    if delta < 0.0 {
        if (milliseconds as f64) < -(delta as f64) {
            milliseconds = milliseconds.wrapping_add(DAY_MS);
        }
    } else if milliseconds as f64 >= DAY_MS as f64 - delta as f64 {
        milliseconds = milliseconds.wrapping_sub(DAY_MS);
    }
    Ok(milliseconds.wrapping_add((delta.trunc() as i32) as u32))
}
