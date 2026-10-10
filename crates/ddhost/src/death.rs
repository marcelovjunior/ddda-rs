//! Native death/revival scalar rules and the separate multiplayer death policy.
//!
//! These helpers do not infer animation completion from HP, choose a revive
//! target, or invent a save point. Actor, interaction and checkpoint adapters
//! must provide those observations. The original has one Arisen; profile keys
//! and the all-active-Arisen game-over rule are project policy (COOP_DESIGN).

use crate::{PlayerId, ProfileId, Roster};
use std::collections::BTreeMap;

/// Scalar inputs to the final death selector in the native damage dispatcher.
/// These are actor flags, not animation durations inferred from zero HP.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FatalRequest {
    pub dead: bool,          // actor +31EA
    pub linked_actor: bool,  // actor +2104
    pub action_locks: u32,   // actor +2360
    pub movement_flags: u32, // actor +273C
    pub stone_dead: bool,    // actor +2B51
    pub damage_gate: bool,   // actor +31EB
    pub petrified: bool,     // actor +29F7
    pub airborne_gate: bool, // actor +2B55
    pub kind_flags: u8,      // actor +206D
    pub subtype_flags: u8,   // actor +206E
    pub stage: u32,          // sGameSys +34
}

impl FatalRequest {
    /// Returns the native requested action. Request0x16 is an abstract request:
    /// 0x00B736D4 converts it into action0x18, variant2, with a throw payload.
    /// Consumers must not look up request0x16 in the common action dictionary.
    // Translated from 0x00B747D0..0x00B7493C (DDDA.exe 2017-12-13 Steam).
    pub fn action(self) -> Option<u32> {
        if !self.dead
            || self.linked_actor
            || self.action_locks & 0x2100_0000 != 0
            || self.movement_flags & 0x20 != 0
        {
            return None;
        }
        if self.stone_dead {
            return Some(0x10);
        }
        if self.damage_gate {
            return None;
        }
        if self.movement_flags & 1 != 0 {
            if self.action_locks & 0x8000 != 0 {
                return Some(0xec);
            }
            if self.petrified {
                return None;
            }
            if self.kind_flags & 8 != 0 && self.subtype_flags & 0x18 == 0 {
                return Some(0x0300_0040);
            }
            Some(0x1e)
        } else if self.airborne_gate {
            None
        } else if self.kind_flags & 2 != 0 && self.stage == 0x259 {
            Some(0xd6)
        } else {
            Some(0x16)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalCompletion {
    ArisenDead,
    PawnNearDeath,
    PawnLost,
}

/// The crumble-death action does not transition until its actual layer finishes
/// or the native +365C completion value reaches one. A pawn's variant2 is lost.
// Translated from 0x00AD5AF9..0x00AD5B64 (DDDA.exe 2017-12-13 Steam).
pub fn fatal_completion(
    layer_finished: bool,
    completion: i32,
    pawn: bool,
    variant: u32,
) -> Option<FatalCompletion> {
    if !layer_finished && completion < 1 {
        return None;
    }
    Some(if !pawn {
        FatalCompletion::ArisenDead
    } else if variant == 2 {
        FatalCompletion::PawnLost
    } else {
        FatalCompletion::PawnNearDeath
    })
}

pub const ADJUST_GAME_OVER_DELAY: usize = 0x136;
pub const ADJUST_PAWN_DOWN_LIMIT: usize = 1;
pub const ADJUST_PAWN_RECOVERY_RATIO: usize = 0xfb;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parameters {
    /// Native actor deltas are motion frames, not seconds.
    pub game_over_delay: f32,
    pub pawn_down_limit: f32,
    pub pawn_recovery_ratio: f32,
}

impl Parameters {
    /// Bulk values remain in the player's installed rAdjustParam.
    // Translated from 0x00AD7BF0, 0x00AFD110 and 0x00AFD8D0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn from_adjust(table: &[f32]) -> Result<Self, String> {
        let get = |index: usize| {
            table
                .get(index)
                .copied()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| format!("death adjust {index:#x} missing or invalid"))
        };
        Ok(Self {
            game_over_delay: get(ADJUST_GAME_OVER_DELAY)?,
            pawn_down_limit: get(ADJUST_PAWN_DOWN_LIMIT)?,
            pawn_recovery_ratio: get(ADJUST_PAWN_RECOVERY_RATIO)?,
        })
    }

    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::from_adjust(&crate::install::adjust_table(
            vfs,
            ddenemyrt::fight::ADJUST_PARAMS,
        )?)
    }
}

/// cPlActDead's clock starts on entry to that action, after the preceding
/// fatal damage action. It must not be started early from HP alone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DeadClock {
    elapsed: f32,
    requested: bool,
}

impl DeadClock {
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }
    pub fn requested(&self) -> bool {
        self.requested
    }

    /// Emits once at equality as well as beyond the installed threshold.
    /// Pawn/NPC actors do not increment this Arisen-only clock.
    // Translated from 0x00AD7BF0, branch 0x00AD7CC0..0x00AD7D21
    // (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, delta: f32, delay: f32, is_arisen: bool) -> Result<bool, String> {
        valid_delta(delta)?;
        if !delay.is_finite() || delay < 0.0 {
            return Err("invalid death delay".into());
        }
        if self.requested || !is_arisen {
            return Ok(false);
        }
        self.elapsed += delta;
        if self.elapsed >= delay {
            self.requested = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Native facts that pause/reset a pawn's near-death expiration. Values are
/// passed by the scene/action adapter rather than guessed from combat range.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PawnDownGuards {
    pub game_flags: u32,         // sGameSys +B883C
    pub transient_flags: u32,    // sGameSys +B8848
    pub active_pause_count: i32, // sGameSys +B8850
    pub scene_gate: bool,        // active scene child +475
    pub return_gate: bool,       // 0x0043EC10
    pub event_gate: bool,        // 0x0043F480
    pub scenario_gate: bool,     // sGameSys +54 !=0
    pub weather_gate: bool,      // 0x004C2BA0
    pub upper_flags: u32,        // sGameSys +B8844
    pub arisen_reviving: bool,   // Arisen action id33
    /// 0x00B87D20: session flags, or a protected living story pawn.
    pub reset_expiration: bool,
    pub irreversible_death: bool, // actor +31EE
    pub blocked_revival: bool,    // actor +2B50
}

impl PawnDownGuards {
    // Translated from 0x00AFD110, 0x00AFD2AC..0x00AFD376
    // (DDDA.exe 2017-12-13 Steam).
    pub fn timer_paused(self) -> bool {
        self.game_flags & 0x0100_0000 != 0
            || self.transient_flags & 0x02f8_0002 != 0
            || self.active_pause_count > 0
            || self.scene_gate
            || self.return_gate
            || self.event_gate
            || self.scenario_gate
            || self.weather_gate
            || self.upper_flags & 0x00c0_0000 != 0
            || self.arisen_reviving
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PawnDownClock {
    elapsed: f32,
}

impl PawnDownClock {
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// Call only in near-death action phase2. Motion phases0/1 do not spend
    /// the expiration timer. True requests cPlActCmcDead action82.
    // Translated from 0x00AFD110, 0x00AFD278..0x00AFD4D7
    // (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, delta: f32, limit: f32, guards: PawnDownGuards) -> Result<bool, String> {
        valid_delta(delta)?;
        if !limit.is_finite() || limit < 0.0 {
            return Err("invalid pawn down limit".into());
        }
        if !guards.timer_paused() {
            self.elapsed += delta;
        }
        if guards.reset_expiration {
            self.elapsed = 0.0;
        }
        Ok(self.elapsed >= limit || guards.irreversible_death || guards.blocked_revival)
    }
}

/// A real queued type18 revive contact must reach the pawn. This does not
/// authorize revival of the Arisen or a corpse that was already lost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviveContact {
    pub pawn: bool,
    pub queued: bool,
    pub kind: u32,
    pub linked_actor: bool, // +2104 !=0
    pub irreversible_death: bool,
    pub blocked_revival: bool,
}

impl ReviveContact {
    // Translated from 0x00B6D54D..0x00B6D599
    // (DDDA.exe 2017-12-13 Steam).
    pub fn accepts(self) -> bool {
        self.pawn
            && self.queued
            && self.kind == 0x18
            && !self.linked_actor
            && !self.irreversible_death
            && !self.blocked_revival
    }
}

/// HP restored on cPlActCmcReturn entry. The recipient has already accepted
/// the revive contact; ordinary curatives continue to reject dead actors.
// Translated from 0x00AFDB19..0x00AFDB62
// (DDDA.exe 2017-12-13 Steam).
pub fn pawn_recovery_hp(max_hp: f32, installed_ratio: f32, contact_ratio: f32) -> Result<f32, String> {
    if !max_hp.is_finite()
        || max_hp <= 0.0
        || !installed_ratio.is_finite()
        || installed_ratio < 0.0
        || !contact_ratio.is_finite()
        || contact_ratio < 0.0
    {
        return Err("invalid pawn recovery values".into());
    }
    Ok((max_hp * installed_ratio.max(contact_ratio)).min(max_hp))
}

fn valid_delta(delta: f32) -> Result<(), String> {
    if delta.is_finite() && delta >= 0.0 {
        Ok(())
    } else {
        Err("invalid death actor delta".into())
    }
}

/// Transient attempt state keyed by persistent profile. Actor death selection
/// stays native; this container adds the multiplayer policy approved in S53.
#[derive(Debug, Clone, Default)]
pub struct SessionDeaths {
    terminal: BTreeMap<ProfileId, Option<DeadClock>>,
}

impl SessionDeaths {
    /// Co-op input/spectator policy starts with fatality, while the native
    /// cPlActDead delay remains unstarted throughout the collapse motion.
    pub fn enter_fatal(&mut self, profile: ProfileId) {
        self.terminal.entry(profile).or_insert(None);
    }

    /// Called on entry to the native cPlActDead action, not directly on HP0.
    /// Repeated observations and a reconnect never reset an existing clock.
    pub fn enter_dead(&mut self, profile: ProfileId) {
        self.terminal
            .entry(profile)
            .or_insert(None)
            .get_or_insert_default();
    }

    pub fn is_dead(&self, profile: ProfileId) -> bool {
        self.terminal.contains_key(&profile)
    }

    pub fn step(&mut self, roster: &Roster, delta: f32, delay: f32) -> Result<(), String> {
        for seat in roster.seats() {
            if let Some(Some(clock)) = self.terminal.get_mut(&seat.profile) {
                clock.step(delta, delay, true)?;
            }
        }
        Ok(())
    }

    /// Co-op policy: absent/disconnected profiles do not veto; an empty
    /// roster is not a party death and must not open a game-over screen.
    pub fn game_over(&self, roster: &Roster) -> bool {
        !roster.is_empty()
            && roster.seats().iter().all(|seat| {
                self.terminal
                    .get(&seat.profile)
                    .and_then(Option::as_ref)
                    .is_some_and(DeadClock::requested)
            })
    }

    /// Co-op camera policy: deterministic lowest living seat for a spectator.
    pub fn spectator_target(&self, roster: &Roster, viewer: PlayerId) -> Option<PlayerId> {
        let seat = roster.seat(viewer)?;
        if !self.is_dead(seat.profile) {
            return Some(viewer);
        }
        roster
            .seats()
            .iter()
            .find(|seat| !self.is_dead(seat.profile))
            .map(|seat| seat.id)
    }

    /// Only successful shared checkpoint replacement clears the attempt.
    pub fn restored(&mut self) {
        self.terminal.clear();
    }
}
