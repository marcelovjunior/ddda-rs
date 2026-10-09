//! Source-backed arithmetic fragments, not a complete stamina simulation.
//!
//! Inputs belong to one actor. Callers must resolve action/status gates and
//! parameter selections first; see `docs/STAMINA.md`. Inputs must be finite,
//! rate divisors positive, and caps nonnegative. No wall-clock timer is guessed.
use crate::StaminaEntry;

/// The two sWorkRate.mRate entries and sMain.mDeltaTime (30-Hz frame units).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub work_rate_0: f32,
    pub work_rate_1: f32,
    pub main_delta: f32,
}
impl Timing {
    /// Translate a continuous rate to one update, preserving multiplication order.
    pub fn delta(self, rate: f32) -> f32 {
        // Translated from 0x00b814cd..0x00b8152d (DDDA.exe 2017-12-13 Steam).
        self.work_rate_1 * self.work_rate_0 * self.main_delta * rate
    }
}

/// Selected recovery numerator divided by body + encumbrance frame terms.
/// `load` is the entry at index `4 + encumbrance`, not `encumbrance`.
pub fn recovery_base(body: &StaminaEntry, load: &StaminaEntry) -> f32 {
    // Translated from 0x00b81846..0x00b81875 (DDDA.exe 2017-12-13 Steam).
    body.stamina / (load.frame + body.frame)
}

/// Actor fields +0x4ce4, +0x4ce8, +0x4cec; these are amounts, not seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RecoveryDebt {
    pub remaining: f32,
    pub initial: f32,
    pub override_rate: f32,
}
impl RecoveryDebt {
    /// Advance with the already-selected recovery rate and cUnit.mDeltaTime.
    /// The caller selects the rate BEFORE this step, as the original does.
    pub fn advance(&mut self, unit_delta: f32, selected_recovery_rate: f32) {
        // Translated from 0x00b8148c..0x00b814cd (DDDA.exe 2017-12-13 Steam).
        if self.remaining > 0.0 {
            self.remaining -= unit_delta * selected_recovery_rate;
            if self.remaining <= 0.0 {
                *self = Self::default();
            }
        }
    }
}

/// `suppressed` is the motion flag/action gate; `dash_bypass` is valid only
/// for action 5/0x7d with vf190 bit 0x20 or sGameSys +0xb8844 bit 0x400000.
pub fn recovery_rate(
    base: f32,
    suppressed: bool,
    dash_bypass: bool,
    debt: RecoveryDebt,
    actor_multiplier: f32,
) -> f32 {
    // Translated from 0x00b81810..0x00b8191e (DDDA.exe 2017-12-13 Steam).
    let rate = if debt.remaining > 0.0 {
        debt.override_rate
    } else if !suppressed || dash_bypass {
        base
    } else {
        0.0
    };
    actor_multiplier * rate
}

/// Raw sprint drain, before negative-delta status/equipment modifiers.
/// `multiplier` is 1 unless ability 0x6c selects adjust parameter 0x1db.
pub fn dash_rate(row: &StaminaEntry, multiplier: f32, suppressed: bool) -> f32 {
    // Translated from 0x00b81b21..0x00b81bee (DDDA.exe 2017-12-13 Steam).
    if suppressed {
        0.0
    } else {
        -(row.stamina * multiplier / row.frame)
    }
}

/// Raw dash-jump entry cost: no frame divisor or delta-time multiplication.
/// Apply through the same resolved negative-delta path as continuous costs.
pub fn dash_jump_cost(row: &StaminaEntry) -> f32 {
    // Translated from cPlActDashJumpBegin::vf05 at 0x00acdac5's call site
    // (DDDA.exe 2017-12-13 Steam); the caller gates Arisen/vf190 bit 0x20.
    -row.stamina
}

/// Select the continuous locomotion row (CVTTSS2SI truncates toward zero).
/// Returns None for a non-finite/out-of-range modifier or invalid table index.
pub fn locomotion_row(load: i32, ability_adjustment: f32, lighter_status: bool) -> Option<usize> {
    // Translated from 0x00b81ad5..0x00b81b04 (DDDA.exe 2017-12-13 Steam).
    if !ability_adjustment.is_finite() || !(-2147483648.0..2147483648.0).contains(&ability_adjustment) {
        return None;
    }
    let mut index = load.checked_add(ability_adjustment as i32)?;
    if lighter_status {
        index = index.checked_sub(1)?.max(0);
    }
    usize::try_from(index).ok()
}

/// Arithmetic tail of the stamina writer AFTER its external gates/modifiers.
/// A forced positive delta bypasses exhaustion but does not itself clear it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaminaState {
    pub current: f32,
    pub exhausted: bool,
}
impl StaminaState {
    pub fn apply_resolved_delta(&mut self, delta: f32, cap: f32, forced: bool) {
        // Translated from 0x00b81f24..0x00b8202f (DDDA.exe 2017-12-13 Steam).
        // Omitted engine gates and tired-action messages are caller responsibilities.
        let delta = if delta > 0.0 && self.exhausted && !forced {
            0.0
        } else {
            delta
        };
        let previous = self.current;
        self.current += delta;
        if self.current > cap {
            self.current = cap;
        } else if self.current <= 0.0 {
            self.current = 0.0;
            if previous > 0.0 {
                self.exhausted = true;
            }
        }
    }
}

/// The effective cap is supplied by the actor's max-stamina consumer.
pub fn status_cap(effective_max: f32, reduced_cap_status: bool) -> f32 {
    // Translated from 0x00b81fcf..0x00b81ff2 (DDDA.exe 2017-12-13 Steam).
    if reduced_cap_status {
        effective_max * 0.6
    } else {
        effective_max
    }
}
