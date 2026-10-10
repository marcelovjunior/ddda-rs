//! Native death/revival scalar rules and the separate multiplayer death policy.
//!
//! These helpers do not infer animation completion from HP, choose a revive
//! target, or invent a save point. Actor, interaction and checkpoint adapters
//! must provide those observations. The original has one Arisen; profile keys
//! and the all-active-Arisen game-over rule are project policy (COOP_DESIGN).

use crate::{Host, PlayerId, ProfileId, Roster, StoryWorld};
use std::collections::BTreeMap;
pub use ddrift::PawnRef;

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
    epoch: u64,
    session: SessionDeaths,
    parameters: Option<Parameters>,
    actions: BTreeMap<ProfileId, ArisenAction>,
    unsupported: BTreeMap<ProfileId, String>,
}

impl Runtime {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn is_dead(&self, profile: ProfileId) -> bool {
        self.session.is_dead(profile)
    }

    pub fn report(&self, roster: &Roster) -> Report {
        Report {
            game_over: self.session.game_over(roster),
            seats: roster
                .seats()
                .iter()
                .map(|seat| SeatReport {
                    seat: seat.id,
                    profile: seat.profile,
                    terminal: self.is_dead(seat.profile),
                    spectator: self.session.spectator_target(roster, seat.id),
                    unsupported: self.unsupported.get(&seat.profile).cloned(),
                })
                .collect(),
        }
    }

    fn restore_body(&mut self, sim: &mut ddsim::Sim, profile: ProfileId, seat: PlayerId) {
        let Some(action) = self.actions.get(&profile).copied() else {
            return;
        };
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

/// Inputs to a fresh headless attempt factory. The roster is the live one;
/// the save contains checkpoint state plus durable absent-profile chunks.
/// Factories must construct a new world and provide installed stage collision
/// and authored party placement for missing seated profiles before returning.
pub struct RetrySeed {
    pub roster: Roster,
    pub save: crate::save::SaveFile,
    pub absent_profiles: std::collections::BTreeSet<ProfileId>,
}

impl RetrySeed {
    /// No Clone bounds on the story data or world: only installed sim data is
    /// reused. Missing seats must receive caller-observed checkpoint party
    /// places, never their old live-stage position or a fabricated offset.
    pub fn sim(
        &self,
        template: &ddsim::Sim,
        party_places: &BTreeMap<ProfileId, ([f32; 3], f32)>,
    ) -> Result<ddsim::Sim, crate::HostError> {
        let mut sim = template.fresh_attempt();
        for seat in self.roster.seats() {
            let place = if self.absent_profiles.contains(&seat.profile) {
                party_places.get(&seat.profile).copied()
            } else {
                self.save
                    .get(crate::save::TAG_PLACE, crate::save::Owner::Profile(seat.profile))
                    .and_then(crate::save::place_from)
            }
            .ok_or_else(|| {
                crate::HostError::Save(format!("retry party place absent for profile {:?}", seat.profile))
            })?;
            if !place.1.is_finite() || place.0.iter().any(|v| !v.is_finite()) {
                return Err(crate::HostError::Save("retry nonfinite party place".into()));
            }
            sim.add_player(seat.id, place.0, place.1);
        }
        Ok(sim)
    }
}

/// Project retry policy (head7952): a restored checkpoint keeps its shared
/// pawn slots. A colliding live hire is ordinarily dismissed; no RC/items
/// are invented or transferred by this notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileNotice {
    pub profile: ProfileId,
    pub slot: u8,
    pub released: ddrift::PawnRef,
}

pub struct RetryCommit {
    pub epoch: u64,
    pub frame: u64,
    pub notices: Vec<ProfileNotice>,
}

/// Holds the mutable live-host borrow while presentation is prepared, so the
/// roster/input/death state cannot change between validation and commit.
/// Dropping it without commit leaves that live host unchanged.
pub struct PreparedAttempt<'a, D: ddstory::data::Data, W: StoryWorld> {
    live: &'a mut Host<D, W>,
    candidate: Box<Host<D, W>>,
    notices: Vec<ProfileNotice>,
}

impl<D: ddstory::data::Data, W: StoryWorld> PreparedAttempt<'_, D, W> {
    pub fn candidate(&self) -> &Host<D, W> {
        &self.candidate
    }
    pub fn notices(&self) -> &[ProfileNotice] {
        &self.notices
    }

    /// No parsing, IO, rebuilding or allocations occur during the swap.
    /// ddnet's owning Server remains outside this Host and must retain its
    /// connections, reset baselines and send the typed notice for this epoch.
    pub fn commit_retry(self) -> RetryCommit {
        let result = RetryCommit {
            epoch: self.candidate.death.epoch(),
            frame: self.candidate.frame,
            notices: self.notices,
        };
        *self.live = *self.candidate;
        result
    }
}

/// Conservative projection of the ordinary-grounded selector's actor bit.
/// The port has control states and scenery results, not native +273C. Only
/// its grounded domains and an actual floor contact admit the translated
/// 0x00B747D0 -> 0x00AD56F0 branch. Other control/actor guards remain open.
fn grounded_admission(sim: &ddsim::Sim, seat: PlayerId, ground: &dyn ddsim::Ground) -> bool {
    let Some(body) = sim.player(seat) else {
        return false;
    };
    let control_grounded = body.state.on_ground_for_attacks()
        || matches!(
            body.state,
            ddsim::State::JumpStart { .. } | ddsim::State::HardLanding
        )
        || (matches!(body.state, ddsim::State::Bow { .. })
            && body.bow.is_some_and(|bow| {
                matches!(
                    bow.movement,
                    ddsim::attack::BowMovement::Standing | ddsim::attack::BowMovement::Moving
                )
            }));
    if !control_grounded || body.hold.is_some() {
        return false;
    }
    if ground.triangles_in(body.position, body.position, &mut Vec::new()) {
        // Same native-derived check drop and capsule as Sim::ground_move.
        ddsim::sweep::sweep(
            &sim.sweep_param,
            &ddsim::sweep::PLAYER_CAPSULE,
            body.position,
            [0.0, -ddsim::GROUND_CHECK_DROP * ddsim::TICK, 0.0],
            true,
            &|lo, hi, out| {
                ground.triangles_in(lo, hi, out);
            },
        )
        .grounded()
    } else {
        // Height-only test worlds have no capsule contact. Require their
        // exact floor height; no invented snap radius for fatal admission.
        ground.height(body.position[0], body.position[2], body.position[1]) == Some(body.position[1])
    }
}

impl<D: ddstory::data::Data, W: StoryWorld> Host<D, W> {
    /// Dedicated shared retry; ordinary save loading keeps its old contract.
    /// The factory is headless and must not modify the live world, renderer,
    /// game installation or network. It preloads fresh stage/camera resources
    /// from the seed before returning. Presentation may then inspect the
    /// checked candidate while PreparedAttempt keeps the live host borrowed.
    pub fn prepare_retry<F>(
        &mut self,
        checkpoint: &crate::save::SaveFile,
        data: D,
        factory: F,
    ) -> Result<PreparedAttempt<'_, D, W>, crate::HostError>
    where
        F: FnOnce(&RetrySeed) -> Result<Host<D, W>, crate::HostError>,
    {
        use crate::save::{Owner, TAG_HOST, TAG_PLACE};
        let err = crate::HostError::Save;
        if checkpoint.get(TAG_HOST, Owner::Shared).is_none() {
            return Err(err("retry checkpoint lacks HOST".into()));
        }
        for (index, chunk) in checkpoint.chunks.iter().enumerate() {
            if checkpoint.chunks[..index]
                .iter()
                .any(|c| c.tag == chunk.tag && c.owner == chunk.owner)
            {
                return Err(err("retry checkpoint has duplicate chunks".into()));
            }
        }
        let epoch = self
            .death
            .epoch
            .checked_add(1)
            .ok_or_else(|| err("retry epoch exhausted".into()))?;
        let checkpoint_vitals = crate::vitals::read(checkpoint)?;
        if checkpoint_vitals.values().any(|value| value.hp <= 0.0) {
            return Err(err("retry checkpoint VITL must be alive".into()));
        }
        let live_save = self.export_durable_state()?;
        let checkpoint_profiles: std::collections::BTreeSet<_> = checkpoint
            .chunks
            .iter()
            .filter_map(|c| match c.owner {
                Owner::Profile(profile) => Some(profile),
                _ => None,
            })
            .collect();
        let absent_profiles: std::collections::BTreeSet<_> = live_save
            .chunks
            .iter()
            .filter_map(|c| match c.owner {
                Owner::Profile(profile) if !checkpoint_profiles.contains(&profile) => Some(profile),
                _ => None,
            })
            .collect();
        let mut save = checkpoint.clone();
        let mut absent_dead = std::collections::BTreeSet::new();
        for chunk in &live_save.chunks {
            let Owner::Profile(profile) = chunk.owner else {
                continue;
            };
            if !absent_profiles.contains(&profile) {
                continue;
            }
            // An active newcomer enters at the restored party's place.
            // Unseated profiles keep durable PLCE for a later admission.
            if chunk.tag == TAG_PLACE && self.roster.seats().iter().any(|s| s.profile == profile) {
                continue;
            }
            if chunk.tag == crate::save::TAG_VITALS
                && self.roster.seats().iter().any(|s| s.profile == profile)
                && (self.death.is_dead(profile) || crate::vitals::Vitals::from_bytes(&chunk.bytes)?.hp <= 0.0)
            {
                // Head8055: retain durable newcomer identity/state, but its
                // dead attempt vitals cannot carry death into the new attempt.
                absent_dead.insert(profile);
                continue;
            }
            save.put(chunk.tag, chunk.owner, chunk.bytes.clone());
        }
        let seed = RetrySeed {
            roster: self.roster.clone(),
            save,
            absent_profiles,
        };
        let mut candidate = factory(&seed)?;
        if candidate.roster != seed.roster {
            return Err(err("retry factory changed live seats/profiles/control".into()));
        }
        if candidate.frame != 0 {
            return Err(err("retry factory did not construct a fresh host".into()));
        }
        for seat in seed.roster.seats() {
            if candidate.sim.player(seat.id).is_none() {
                return Err(err("retry factory omitted a seated body".into()));
            }
        }
        if self.fight.is_some() && candidate.fight.is_none() {
            return Err(crate::HostError::NoData("retry fight"));
        }
        // Reuse installed system catalogues, never the old attempt actors.
        candidate.clock = self.clock;
        candidate.data = self.data.clone();
        candidate.item_data = self.item_data.clone();
        candidate.reset_attempt_controls();
        candidate.fight = candidate
            .fight
            .take()
            .map(|fight| ddenemyrt::fight::Fight::new(fight.data));
        candidate.ambient = Default::default();
        candidate.locks = Default::default();
        candidate.npcs = Default::default();
        candidate.story_npcs = Default::default();
        candidate.npc_combat = Default::default();
        candidate.npc_errors.clear();
        candidate.npc_parked.clear();
        candidate.load(&seed.save, data)?;
        let restored_vitals = crate::vitals::read(&seed.save)?;
        let mut notices = Vec::new();
        for index in 0..2 {
            let Some(profile) = self.hired_by[index] else {
                continue;
            };
            if !seed.absent_profiles.contains(&profile)
                || !seed.roster.seats().iter().any(|s| s.profile == profile)
            {
                continue;
            }
            let Some(hire) = self.rift.party.hired[index].as_ref() else {
                continue;
            };
            if candidate.rift.party.hired[index].is_none() && !candidate.rift.party.reserved[index] {
                candidate.rift.party.hired[index] = Some(hire.clone());
                candidate.hired_by[index] = Some(profile);
            } else {
                // Head7952: the checkpoint owns shared slots. Reuse ordinary
                // release bookkeeping at the live departure time, temporarily
                // installing only the departing record in this candidate.
                let restored = candidate.rift.party.hired[index].replace(hire.clone());
                // The departing live hire did not occupy the restored room.
                // Keep that room's counters/selection independent of this
                // departure's log and history bookkeeping.
                let restored_room = candidate.rift.room.take();
                let released = candidate
                    .rift
                    .release(index + 1, self.play_time(), false)
                    .expect("ordinary release of an installed candidate hire");
                candidate.rift.party.hired[index] = restored;
                candidate.rift.room = restored_room;
                notices.push(ProfileNotice {
                    profile,
                    slot: (index + 1) as u8,
                    released,
                });
            }
        }
        // PLCE restoration alone does not replace hidden action state. Rebuild
        // all seated bodies AFTER every save/hire operation, with the loaded
        // place and heading but no private queued controls or fractional time.
        let places: Vec<_> = seed
            .roster
            .seats()
            .iter()
            .map(|seat| {
                let p = candidate.sim.player(seat.id).expect("validated candidate body");
                (seat.id, p.position, p.yaw)
            })
            .collect();
        candidate.sim = candidate.sim.fresh_attempt();
        for (seat, place, yaw) in places {
            candidate.sim.add_player(seat, place, yaw);
        }
        candidate.fight = candidate
            .fight
            .take()
            .map(|fight| ddenemyrt::fight::Fight::new(fight.data));
        candidate.refresh_party();
        candidate.rebuild_profile_fighters();
        candidate.vitals_diagnostics.clear();
        candidate.apply_loaded_vitals(&restored_vitals);
        for seat in seed
            .roster
            .seats()
            .iter()
            .filter(|seat| absent_dead.contains(&seat.profile))
        {
            if let Some(body) = candidate.sim.player_mut(seat.id) {
                body.stamina = body.stamina_max;
                body.ran_out = false;
            }
        }
        candidate.death = Runtime {
            epoch,
            ..Default::default()
        };
        // Reviewed S52 runtime: preserve installed data/RNG/monotonic IDs,
        // while retiring the old speech/recognition lifetime exactly once.
        candidate.pawn_chatter = self.pawn_chatter.fork_for_retry();
        for seat in seed.roster.seats() {
            let p = candidate.sim.player(seat.id).expect("reconstructed seated body");
            if candidate.seat_vitals(seat.id).is_some_and(|v| v.hp <= 0.0) {
                return Err(err("retry candidate did not reconstruct a living Arisen".into()));
            }
            if !p.yaw.is_finite()
                || p.position.iter().any(|v| !v.is_finite())
                || !grounded_admission(&candidate.sim, seat.id, candidate.ground.as_ref())
            {
                return Err(err("retry requires a validated grounded party place".into()));
            }
            if candidate.fight.is_some() && candidate.sim.motion_for(p.weapon, 0x109).is_none() {
                return Err(err("retry missing installed fatal layer".into()));
            }
        }
        Ok(PreparedAttempt {
            live: self,
            candidate: Box::new(candidate),
            notices,
        })
    }

    /// Personal death policy never changes the global quest-camera wire bit.
    pub fn player_actions_blocked(&self, seat: PlayerId) -> bool {
        self.world.player_actions_blocked(seat)
            || self
                .roster
                .seat(seat)
                .is_some_and(|seat| self.death.is_dead(seat.profile))
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
                        if fight
                            .arisen
                            .get(&seat.id)
                            .is_some_and(|a| a.fighter.victim.dead())
                        {
                            self.death.unsupported.insert(seat.profile, error.clone());
                        }
                    }
                }
            }
        }
        for seat in self.roster.seats() {
            let Some(actor) = fight.arisen.get(&seat.id) else {
                continue;
            };
            if !actor.fighter.victim.dead() || self.death.is_dead(seat.profile) {
                continue;
            }
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
            if let Some(body) = self.sim.player_mut(seat.id) {
                body.cancel_attack_actions();
            }
        }
    }

    pub(crate) fn dead_actions_at_move_start(&self) -> std::collections::BTreeSet<ProfileId> {
        self.roster
            .seats()
            .iter()
            .filter_map(|seat| {
                matches!(
                    self.sim.player(seat.id).map(|p| p.state),
                    Some(ddsim::State::Death {
                        phase: ddsim::DeathPhase::ArisenDead
                    })
                )
                .then_some(seat.profile)
            })
            .collect()
    }

    pub(crate) fn step_arisen_death_clocks(
        &mut self,
        step: f32,
        dead_at_start: &std::collections::BTreeSet<ProfileId>,
    ) {
        for seat in self.roster.seats() {
            let Some(body) = self.sim.player(seat.id) else {
                continue;
            };
            let ddsim::State::Death { phase } = body.state else {
                continue;
            };
            self.death.actions.insert(
                seat.profile,
                ArisenAction {
                    phase,
                    frame: body.frame,
                },
            );
            if phase != ddsim::DeathPhase::ArisenDead {
                continue;
            }
            self.death.session.enter_dead(seat.profile);
            // Native Dead starts its own clock on entry. Never charge the
            // collapse's whole outer frame to a newly entered Dead action.
            if !dead_at_start.contains(&seat.profile) {
                continue;
            }
            let Some(parameters) = self.death.parameters else {
                continue;
            };
            if let Some(Some(clock)) = self.death.session.terminal.get_mut(&seat.profile) {
                if let Err(error) = clock.step(step * body.unit_rate, parameters.game_over_delay, true) {
                    self.death.unsupported.insert(seat.profile, error);
                }
            }
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use crate::{
        Control,
        data::NoStoryData,
        save::{Owner, SaveFile, TAG_NPC_QUEST},
    };
    type TestHost = Host<NoStoryData, ddstory::scripted::ScriptedHost>;
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }
    struct NoFloor;
    impl ddsim::Ground for NoFloor {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            None
        }
    }
    fn host() -> TestHost {
        let mut host = TestHost::new(
            ddsim::Sim::new([(0x109, ddsim::RootMotion::none(4))].into_iter().collect()),
            None,
            Default::default(),
            Box::new(Flat),
        );
        host.join(ProfileId(1), Control::Local, [10.0, 0.0, 20.0], 0.25)
            .unwrap();
        host
    }
    fn factory(
        seed: &RetrySeed,
        template: &ddsim::Sim,
        places: &BTreeMap<ProfileId, ([f32; 3], f32)>,
    ) -> Result<TestHost, crate::HostError> {
        let mut host = TestHost::new(
            seed.sim(template, places)?,
            None,
            Default::default(),
            Box::new(Flat),
        );
        host.roster = seed.roster.clone();
        Ok(host)
    }

    #[test]
    fn retry_failed_factory_late_chunk_and_stage_preserve_complete_live_attempt() {
        let mut live = host();
        let checkpoint = live.save().unwrap();
        let checkpoint_bytes = checkpoint.to_bytes();
        live.sim.begin_grounded_death(0, false, true).unwrap();
        live.death.session.enter_fatal(ProfileId(1));
        live.economy.wallet(0).gold = 99;
        let before_hash = live.state_hash();
        let before_save = live.export_durable_state().unwrap().to_bytes();
        let template = live.sim.fresh_attempt();
        let mut invalid = checkpoint.clone();
        invalid.put(TAG_NPC_QUEST, Owner::Shared, vec![255]);
        assert!(
            live.prepare_retry(&invalid, NoStoryData, |seed| factory(
                seed,
                &template,
                &BTreeMap::new()
            ))
            .is_err()
        );
        assert_eq!(live.state_hash(), before_hash);
        assert_eq!(live.export_durable_state().unwrap().to_bytes(), before_save);
        assert!(
            live.prepare_retry(&checkpoint, NoStoryData, |_| Err(crate::HostError::Save(
                "fixture resource failure".into()
            )))
            .is_err()
        );
        assert_eq!(live.state_hash(), before_hash);
        assert!(
            live.prepare_retry(&checkpoint, NoStoryData, |seed| {
                let mut candidate = factory(seed, &template, &BTreeMap::new())?;
                candidate.ground = Box::new(NoFloor);
                Ok(candidate)
            })
            .is_err()
        );
        assert_eq!(live.state_hash(), before_hash);
        {
            let prepared = live
                .prepare_retry(&checkpoint, NoStoryData, |seed| {
                    factory(seed, &template, &BTreeMap::new())
                })
                .unwrap();
            assert!(!prepared.candidate().player_actions_blocked(0));
            // A failure while preparing presentation can drop the candidate.
            drop(prepared);
        }
        assert_eq!(live.state_hash(), before_hash);
        assert_eq!(checkpoint.to_bytes(), checkpoint_bytes);
    }

    #[test]
    fn retry_rebuilds_death_actions_and_keeps_exact_live_roster_holes() {
        let mut live = host();
        let remote = live
            .join(ProfileId(2), Control::Remote, [400.0, 0.0, 0.0], 0.5)
            .unwrap();
        live.frame = 7;
        let checkpoint = live.save().unwrap();
        live.leave(0);
        let roster = live.roster.clone();
        live.sim.begin_grounded_death(remote, false, true).unwrap();
        live.death.session.enter_dead(ProfileId(2));
        let body = live.sim.player_mut(remote).unwrap();
        body.frame = 2.0;
        body.velocity = [1.0, 2.0, 3.0];
        body.skills.request = Some(123);
        body.skills.released.push(123);
        body.bow_latched = true;
        body.attacks_started = 12;
        body.volleys_fired = 13;
        let template = live.sim.fresh_attempt();
        let result = live
            .prepare_retry(&checkpoint, NoStoryData, |seed| {
                factory(seed, &template, &BTreeMap::new())
            })
            .unwrap()
            .commit_retry();
        assert_eq!(result.frame, 7);
        assert_eq!(result.epoch, 1);
        assert_eq!(live.roster, roster);
        assert!(live.roster.seat(0).is_none());
        assert_eq!(live.roster.seat(remote).unwrap().control, Control::Remote);
        assert!(!live.player_actions_blocked(remote));
        assert!(!live.death.report(&live.roster).game_over);
        let body = live.sim.player(remote).unwrap();
        assert_eq!(body.state, ddsim::State::Wait);
        assert_eq!(body.frame, 0.0);
        assert_eq!(body.velocity, [0.0; 3]);
        assert_eq!(body.heading, body.yaw);
        assert_eq!((body.attacks_started, body.volleys_fired), (0, 0));
        assert!(body.skills.request.is_none());
        assert!(body.skills.released.is_empty());
        assert!(!body.bow_latched);
        assert!(live.profiles.contains_key(&ProfileId(1)));
        let second = live
            .prepare_retry(&checkpoint, NoStoryData, |seed| {
                factory(seed, &template, &BTreeMap::new())
            })
            .unwrap()
            .commit_retry();
        assert_eq!(second.epoch, 2);
    }

    #[test]
    fn retry_absent_profile_keeps_durable_wallet_and_uses_checkpoint_party_place() {
        let mut live = host();
        live.economy.wallet(0).gold = 10;
        let checkpoint = live.save().unwrap();
        let saved_place = crate::save::place_from(
            checkpoint
                .get(crate::save::TAG_PLACE, Owner::Profile(ProfileId(1)))
                .unwrap(),
        )
        .unwrap();
        let remote = live
            .join(ProfileId(2), Control::Remote, [900.0, 0.0, 900.0], 0.0)
            .unwrap();
        live.economy.wallet(0).gold = 99;
        live.economy.wallet(remote).gold = 22;
        let template = live.sim.fresh_attempt();
        let places = [(ProfileId(2), saved_place)].into_iter().collect();
        live.prepare_retry(&checkpoint, NoStoryData, |seed| {
            assert!(seed.absent_profiles.contains(&ProfileId(2)));
            factory(seed, &template, &places)
        })
        .unwrap()
        .commit_retry();
        assert_eq!(live.economy.wallet(0).gold, 10);
        assert_eq!(live.economy.wallet(remote).gold, 22);
        assert_eq!(live.sim.player(remote).unwrap().position, saved_place.0);
        assert_eq!(live.sim.player(remote).unwrap().yaw, saved_place.1);
    }

    fn hire(id: u32) -> ddrift::PartyPawn {
        let mut pawn = ddpawn::cmc::Cmc::initial();
        pawn.rom_id = id; // Synthetic identity exercises bookkeeping only.
        ddrift::PartyPawn {
            pawn,
            online: ddrift::PawnRef::rom(id),
            hire_time: 0,
            quests_cleared: 0,
        }
    }

    #[test]
    fn retry_absent_parked_profile_keeps_durable_wallet_and_place() {
        let mut live = host();
        let checkpoint = live.save().unwrap();
        let remote = live
            .join(ProfileId(2), Control::Remote, [900.0, 0.0, 700.0], 0.75)
            .unwrap();
        live.economy.wallet(remote).gold = 22;
        live.leave(remote);
        let template = live.sim.fresh_attempt();
        live.prepare_retry(&checkpoint, NoStoryData, |seed| {
            factory(seed, &template, &BTreeMap::new())
        })
        .unwrap()
        .commit_retry();
        assert_eq!(live.roster.len(), 1);
        assert!(live.profiles.contains_key(&ProfileId(2)));
        assert_eq!(
            live.profiles[&ProfileId(2)].place,
            Some(([900.0, 0.0, 700.0], 0.75))
        );
        let remote = live.join(ProfileId(2), Control::Remote, [0.0; 3], 0.0).unwrap();
        assert_eq!(live.economy.wallet(remote).gold, 22);
        assert_eq!(
            live.sim.player(remote).unwrap().position,
            [0.0; 3],
            "rejoin retains existing current-stage spawn policy"
        );
    }

    #[test]
    fn retry_absent_profile_hire_keeps_free_slot_or_ordinary_dismisses_collision() {
        for collision in [false, true] {
            let mut live = host();
            if collision {
                live.rift.party.hired[0] = Some(hire(7));
                live.hired_by[0] = Some(ProfileId(1));
            }
            let checkpoint: SaveFile = live.save().unwrap();
            let saved_place = crate::save::place_from(
                checkpoint
                    .get(crate::save::TAG_PLACE, Owner::Profile(ProfileId(1)))
                    .unwrap(),
            )
            .unwrap();
            let remote = live
                .join(ProfileId(2), Control::Remote, [900.0, 0.0, 0.0], 0.0)
                .unwrap();
            live.rift.party.hired[0] = Some(hire(8));
            live.hired_by[0] = Some(ProfileId(2));
            live.economy.wallet(remote).rift_crystals = 44;
            live.frame = 30;
            let template = live.sim.fresh_attempt();
            let places = [(ProfileId(2), saved_place)].into_iter().collect();
            let result = live
                .prepare_retry(&checkpoint, NoStoryData, |seed| factory(seed, &template, &places))
                .unwrap()
                .commit_retry();
            assert_eq!(live.economy.wallet(remote).rift_crystals, 44);
            assert!(
                live.rift.room.is_none(),
                "retry must not keep the old live Rift room"
            );
            if collision {
                assert_eq!(live.rift.party.hired[0].as_ref().unwrap().online.rom_id, 7);
                assert_eq!(live.hired_by[0], Some(ProfileId(1)));
                assert_eq!(result.notices.len(), 1);
                assert_eq!(result.notices[0].profile, ProfileId(2));
                assert_eq!(result.notices[0].released.rom_id, 8);
                assert_eq!(result.notices[0].released.die, 0);
            } else {
                assert_eq!(live.rift.party.hired[0].as_ref().unwrap().online.rom_id, 8);
                assert_eq!(live.hired_by[0], Some(ProfileId(2)));
                assert!(result.notices.is_empty());
            }
        }
    }
}
