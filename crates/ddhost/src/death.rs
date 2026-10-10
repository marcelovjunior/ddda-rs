//! Native death/revival scalar rules and the separate multiplayer death policy.
//!
//! These helpers do not infer animation completion from HP, choose a revive
//! target, or invent a save point. Actor, interaction and checkpoint adapters
//! must provide those observations. The original has one Arisen; profile keys
//! and the all-active-Arisen game-over rule are project policy (COOP_DESIGN).

use crate::{Host, PlayerId, ProfileId, Roster, StoryWorld};
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

// Native getter argument at 0x00AD7CEF (DDDA.exe 2017-12-13 Steam).
pub const ADJUST_GAME_OVER_DELAY: usize = 0x136;
// Native getter argument at 0x00AFD288 (DDDA.exe 2017-12-13 Steam).
pub const ADJUST_PAWN_DOWN_LIMIT: usize = 1;
// Native getter argument in 0x00AFDB19..0x00AFDB62 (DDDA.exe 2017-12-13 Steam).
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

/// One view's death policy, kept separate from shared authored-camera locks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub game_over: bool,
    pub seats: Vec<SeatReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatReport {
    pub seat: PlayerId,
    pub profile: ProfileId,
    pub terminal: bool,
    pub spectator: Option<PlayerId>,
    /// Unsupported branches are observable, never a grounded fallback.
    pub unsupported: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ArisenAction {
    phase: ddsim::DeathPhase,
    frame: f32,
}

/// Attempt state survives a disconnect by profile. It is not a durable save
/// chunk, and ordinary `Host::load` does not claim to reconstruct an attempt.
#[derive(Debug, Clone, Default)]
pub struct Runtime {
    session: SessionDeaths,
    parameters: Option<Parameters>,
    actions: BTreeMap<ProfileId, ArisenAction>,
    unsupported: BTreeMap<ProfileId, String>,
}

impl Runtime {
    pub fn is_dead(&self, profile: ProfileId) -> bool {
        self.session.is_dead(profile)
    }

    pub fn report(&self, roster: &Roster) -> Report {
        Report {
            game_over: self.session.game_over(roster),
            seats: roster.seats().iter().map(|seat| SeatReport {
                seat: seat.id,
                profile: seat.profile,
                terminal: self.is_dead(seat.profile),
                spectator: self.session.spectator_target(roster, seat.id),
                unsupported: self.unsupported.get(&seat.profile).cloned(),
            }).collect(),
        }
    }

    fn restore_body(&mut self, sim: &mut ddsim::Sim, profile: ProfileId, seat: PlayerId) {
        let Some(action) = self.actions.get(&profile).copied() else { return };
        // Reattaching the same attempt is co-op policy, not a native revive.
        if let Err(error) = sim.begin_grounded_death(seat, false, true) {
            self.unsupported.insert(profile, error);
            return;
        }
        let body = sim.player_mut(seat).expect("death entry validated seat");
        body.state = ddsim::State::Death { phase: action.phase };
        body.frame = action.frame;
    }
}

/// Conservative projection of the ordinary-grounded selector's actor bit.
/// The port has control states and scenery results, not native +273C. Only
/// its grounded domains and an actual floor contact admit the translated
/// 0x00B747D0 -> 0x00AD56F0 branch. Other control/actor guards remain open.
fn grounded_admission(sim: &ddsim::Sim, seat: PlayerId, ground: &dyn ddsim::Ground) -> bool {
    let Some(body) = sim.player(seat) else { return false };
    let control_grounded = body.state.on_ground_for_attacks()
        || matches!(body.state, ddsim::State::JumpStart { .. } | ddsim::State::HardLanding)
        || (matches!(body.state, ddsim::State::Bow { .. })
            && body.bow.is_some_and(|bow| matches!(bow.movement,
                ddsim::attack::BowMovement::Standing | ddsim::attack::BowMovement::Moving)));
    if !control_grounded || body.hold.is_some() { return false; }
    if ground.triangles_in(body.position, body.position, &mut Vec::new()) {
        // Same native-derived check drop and capsule as Sim::ground_move.
        ddsim::sweep::sweep(&sim.sweep_param, &ddsim::sweep::PLAYER_CAPSULE,
            body.position, [0.0, -ddsim::GROUND_CHECK_DROP * ddsim::TICK, 0.0], true,
            &|lo, hi, out| { ground.triangles_in(lo, hi, out); }).grounded()
    } else {
        // Height-only test worlds have no capsule contact. Require their
        // exact floor height; no invented snap radius for fatal admission.
        ground.height(body.position[0], body.position[2], body.position[1])
            == Some(body.position[1])
    }
}

impl<D: ddstory::data::Data, W: StoryWorld> Host<D, W> {
    /// Personal death policy never changes the global quest-camera wire bit.
    pub fn player_actions_blocked(&self, seat: PlayerId) -> bool {
        self.world.player_actions_blocked(seat)
            || self.roster.seat(seat).is_some_and(|seat| self.death.is_dead(seat.profile))
    }

    pub(crate) fn restore_arisen_death_on_join(&mut self, profile: ProfileId, seat: PlayerId) {
        self.death.restore_body(&mut self.sim, profile, seat);
    }

    /// Run after the actual HP hit pass, before input reaches actor movement.
    /// Pawns require stable admission identity and native contact binding;
    /// this adapter deliberately covers roster Arisen only.
    pub(crate) fn start_arisen_deaths(&mut self) {
        let Some(fight) = &self.fight else { return };
        if self.death.parameters.is_none() {
            match Parameters::from_adjust(&fight.data.player_adjust) {
                Ok(parameters) => self.death.parameters = Some(parameters),
                Err(error) => {
                    for seat in self.roster.seats() {
                        if fight.arisen.get(&seat.id).is_some_and(|a| a.fighter.victim.dead()) {
                            self.death.unsupported.insert(seat.profile, error.clone());
                        }
                    }
                }
            }
        }
        for seat in self.roster.seats() {
            let Some(actor) = fight.arisen.get(&seat.id) else { continue };
            if !actor.fighter.victim.dead() || self.death.is_dead(seat.profile) { continue; }
            self.death.session.enter_fatal(seat.profile);
            let error = if self.sim.player(seat.id).is_some_and(|p| p.hold.is_some()) {
                Some("held ledge fatal control domain remains untranslated".into())
            } else if actor.fighter.statuses.has(ddcombat::cstatus::kind::PETRIFY) {
                Some("petrification-state fatal branch remains untranslated".into())
            } else if !grounded_admission(&self.sim, seat.id, self.ground.as_ref()) {
                Some("airborne or unmapped fatal control domain remains untranslated".into())
            } else {
                self.sim.begin_grounded_death(seat.id, false, true).err()
            };
            if let Some(error) = error {
                self.death.unsupported.insert(seat.profile, error);
            }
            if let Some(body) = self.sim.player_mut(seat.id) { body.cancel_attack_actions(); }
        }
    }

    pub(crate) fn dead_actions_at_move_start(&self) -> std::collections::BTreeSet<ProfileId> {
        self.roster.seats().iter().filter_map(|seat| {
            matches!(self.sim.player(seat.id).map(|p| p.state), Some(ddsim::State::Death {
                phase: ddsim::DeathPhase::ArisenDead })).then_some(seat.profile)
        }).collect()
    }

    pub(crate) fn step_arisen_death_clocks(&mut self, step: f32,
        dead_at_start: &std::collections::BTreeSet<ProfileId>) {
        for seat in self.roster.seats() {
            let Some(body) = self.sim.player(seat.id) else { continue };
            let ddsim::State::Death { phase } = body.state else { continue };
            self.death.actions.insert(seat.profile, ArisenAction { phase, frame: body.frame });
            if phase != ddsim::DeathPhase::ArisenDead { continue; }
            self.death.session.enter_dead(seat.profile);
            // Native Dead starts its own clock on entry. Never charge the
            // collapse's whole outer frame to a newly entered Dead action.
            if !dead_at_start.contains(&seat.profile) { continue; }
            let Some(parameters) = self.death.parameters else { continue };
            if let Some(Some(clock)) = self.death.session.terminal.get_mut(&seat.profile) {
                if let Err(error) = clock.step(step * body.unit_rate, parameters.game_over_delay, true) {
                    self.death.unsupported.insert(seat.profile, error);
                }
            }
        }
    }
}
