//! The opening as a playable session (m5-cassardis): a story world for the
//! host that stands in for what has no headless system yet (as
//! `ddstory::scripted::ScriptedHost`) but leaves the cutscenes and the
//! vocation menu to the view, and a driver that walks the opening's
//! scenario volumes headless up to a chosen point ("as after the
//! prologue").
//!
//! Nothing here is the original's presentation: the view plays the events
//! (`ddstory::event::EventPlayer`) and answers the menu.

use crate::host::{Host, HostError, StoryWorld};
use crate::input::{FrameInputs, PlayerInput};
use ddstory::data::Data;
use ddstory::route::Waypoint;
use ddstory::scripted::ScriptedHost;
use ddstory::{HostEvent, Menu, NpcIntel, NpcUnit, Request, Target};

/// The story world of an opening session.
pub struct OpeningWorld {
    /// Answers everything the view does not (timings, host-run AI,
    /// enemies, placements).
    pub inner: ScriptedHost,
    /// Real placement/combat facts when the caller has connected those
    /// systems. In this mode absent reports stay absent: the scripted
    /// host never supplies enemy deaths or applied-script completion.
    pub encounters: Option<ddstory::encounter::EncounterFacts>,
    /// Explicit placement-to-fight bindings supplied by the layout
    /// adapter. Target index is not guessed from a layout record id.
    enemy_bindings: std::collections::HashMap<(u32, Target), u32>,
    /// NPC units for the scenario volumes that detect NPCs.
    pub npcs: Vec<NpcUnit>,
    /// Menus wait for the view ([`OpeningWorld::close_menu`]) instead of
    /// closing themselves.
    pub manual_menus: bool,
    /// StageLoaded is reported only after the caller installs its world.
    pub manual_stages: bool,
    /// The menu waiting for the view: menu and the seat index it opened
    /// for.
    pub menu: Option<(Menu, usize)>,
    /// Events the view played for real, waiting to be reported.
    pending: Vec<HostEvent>,
}

impl OpeningWorld {
    /// The opening's stand-ins (`ScriptedHost::opening`): events, menus
    /// and the dragon answered by the scripted host until [`Self::live`].
    pub fn new() -> Self {
        OpeningWorld {
            inner: ScriptedHost::opening(),
            encounters: None,
            enemy_bindings: Default::default(),
            npcs: vec![],
            manual_menus: false,
            manual_stages: false,
            menu: None,
            pending: vec![],
        }
    }

    /// From now on every cutscene waits for the view (`inner.awaiting`,
    /// then [`Self::event_done`]) and menus wait for [`Self::close_menu`].
    pub fn live(&mut self, rows: impl IntoIterator<Item = usize>) {
        self.inner.play_rows.extend(rows);
        self.manual_menus = true;
    }

    /// Enable authoritative actor observations independently of the
    /// presentation adapter. Call before dispatching actor requests.
    pub fn enable_live_encounters(&mut self) -> &mut ddstory::encounter::EncounterFacts {
        self.inner.on_camera.clear();
        self.encounters.get_or_insert_with(Default::default)
    }

    /// Report an actual host-system completion (AI, dialogue, UI).
    pub fn report(&mut self, event: HostEvent) {
        self.pending.push(event);
    }

    /// Bind one resolved goblin placement to its authoritative combat id.
    /// Loading/kill-record facts are reported separately through encounters.
    pub fn bind_goblin(&mut self, stage: u32, target: Target, owner: u32) {
        assert_eq!(target.kind, 2, "enemy binding requires an enemy target");
        self.enemy_bindings.insert((stage, target), owner);
    }

    /// Bind a spawned goblin using its original layout identity.
    /// Translated from 0x007613d0 (SetInfo+4 -> actor+0x20f0),
    /// 0x00cc1f60 (record id -> SetInfo+4), and lookup
    /// 0x007643d0 -> 0x004223f0 (DDDA.exe 2017-12-13 Steam).
    pub fn bind_spawned_goblin(&mut self, spawn: &ddspawn::Spawn, owner: u32) -> Target {
        let target = Target {
            kind: 2,
            group: spawn.group,
            index: spawn.record.id as u32,
        };
        self.bind_goblin(spawn.stage, target, owner);
        target
    }

    /// Forget an actor binding on unload without declaring it dead.
    pub fn unbind_enemy(&mut self, stage: u32, target: Target) {
        self.enemy_bindings.remove(&(stage, target));
        if let Some(facts) = &mut self.encounters {
            let mut unit = facts.unit(stage, &target);
            unit.present = false;
            unit.hp = None;
            unit.max_hp = None;
            facts.report_unit(stage, target, unit);
        }
    }

    fn actor_host(&self) -> &dyn ddstory::Host {
        match &self.encounters {
            Some(facts) => facts,
            None => &self.inner,
        }
    }

    /// The view finished playing the awaited cutscene.
    pub fn event_done(&mut self) {
        self.inner.event_done();
    }

    /// The view closed the waiting menu, the vocation menu with `job`
    /// (`ddskill::vocation::Vocation` id) picked.
    pub fn close_menu(&mut self, job: Option<u32>) {
        if let Some((menu, _)) = self.menu.take() {
            if let Some(j) = job {
                self.pending.push(HostEvent::PlayerJob(j));
            }
            self.pending.push(HostEvent::MenuClosed(menu));
        }
    }
}

impl Default for OpeningWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl StoryWorld for OpeningWorld {
    fn observe_npc_combat(&mut self, stage: u32, target: Target, report: crate::npc_combat::Facts) {
        let Some(facts) = &mut self.encounters else { return };
        let mut unit = facts.unit(stage, &target);
        // 0x005C0820 reads intel HP positivity independently of body HP's
        // readiness gate for CheckHp. Binding already validates the lifetime.
        unit.npc = Some(NpcIntel { id: report.npc, hp_positive: report.hp > 0.0 });
        unit.hp = Some(report.hp);
        unit.max_hp = Some(report.max_hp);
        facts.report_unit(stage, target, unit);
    }
    fn observe_npc_intel(&mut self, stage: u32, target: Target, present: bool) {
        let Some(facts) = &mut self.encounters else { return };
        let mut unit = facts.unit(stage, &target);
        unit.present = present;
        unit.apply_fsm_ready = present;
        if present { unit.layout_loaded = true; }
        if !present { unit.npc = None; }
        facts.report_unit(stage, target, unit);
    }
    fn observe_fight(&mut self, fight: Option<&ddenemyrt::fight::Fight>) {
        let Some(facts) = &mut self.encounters else { return };
        for (&(stage, target), &owner) in &self.enemy_bindings {
            let mut unit = facts.unit(stage, &target);
            let goblin = fight.and_then(|f| f.goblins.iter().find(|g| g.fighter.owner == owner));
            unit.present = goblin.is_some();
            let pool = goblin.and_then(|g| match &g.fighter.victim {
                ddenemyrt::combat::Victim::Enemy { regions, .. } => regions.iter().find(|r| r.id == 0),
                _ => None,
            });
            unit.hp = pool.map(|p| p.hp);
            unit.max_hp = pool.map(|p| p.hp_max);
            // Persistent death is a placement-system fact; never infer it
            // from unloading, absence, an empty region list, or a timer.
            facts.report_unit(stage, target, unit);
        }
    }
    fn handle(&mut self, stage: u32, requests: &[Request]) {
        if !self.manual_menus && !self.manual_stages && self.encounters.is_none() {
            return self.inner.handle(stage, requests);
        }
        let mut rest = Vec::with_capacity(requests.len());
        for r in requests {
            match r {
                Request::OpenMenu { menu, player } if self.manual_menus => self.menu = Some((*menu, *player)),
                Request::JumpArea { .. } if self.manual_stages || self.encounters.is_some() => {}
                Request::Placement { .. }
                | Request::Group { .. }
                | Request::ApplyScript { .. }
                | Request::NpcLife { .. }
                    if self.encounters.is_some() =>
                {
                    // The caller receives these in FrameReport and
                    // performs them through its actor adapters. Never
                    // acknowledge them through ScriptedHost's timers.
                }
                other => rest.push(other.clone()),
            }
        }
        self.inner.handle(stage, &rest);
    }
    fn events(&mut self) -> Vec<HostEvent> {
        let mut e = self.inner.events();
        if self.manual_stages {
            e.retain(|event| !matches!(event, HostEvent::StageLoaded { .. }));
        }
        if self.encounters.is_some() {
            // Also discard synthetic callbacks queued before live activation.
            // Actual observations live in our separate pending queue below.
            e.retain(|event| {
                !matches!(
                    event,
                    HostEvent::SetQuestBit { .. } | HostEvent::SetGsf { .. } | HostEvent::StageLoaded { .. }
                )
            });
        }
        e.append(&mut self.pending);
        e
    }
    fn npcs(&self) -> Vec<NpcUnit> {
        self.npcs.clone()
    }
}

impl ddstory::Host for OpeningWorld {
    fn dragon_challenge_actors(
        &self,
    ) -> (
        Option<ddstory::dragon::ChallengeDragon>,
        Vec<ddstory::dragon::ChallengePlayer>,
    ) {
        self.actor_host().dragon_challenge_actors()
    }
    fn layout_loaded(&self, s: u32, t: &Target) -> bool {
        self.actor_host().layout_loaded(s, t)
    }
    fn unit_exists(&self, s: u32, t: &Target) -> bool {
        self.actor_host().unit_exists(s, t)
    }
    fn group_loaded(&self, s: u32, k: u32, g: u32) -> bool {
        self.actor_host().group_loaded(s, k, g)
    }
    fn enemy_group_done(&self, s: u32, g: u32) -> bool {
        self.actor_host().enemy_group_done(s, g)
    }
    fn enemy_group_active(&self, s: u32, g: u32) -> bool {
        self.actor_host().enemy_group_active(s, g)
    }
    fn enemy_check(&self, s: u32, t: &Target, f: u32) -> bool {
        self.actor_host().enemy_check(s, t, f)
    }
    fn npc_intel(&self, s: u32, t: &Target) -> Option<NpcIntel> {
        self.actor_host().npc_intel(s, t)
    }
    fn npc_unit(&self, n: u32) -> bool {
        self.actor_host().npc_unit(n)
    }
    fn apply_fsm_ready(&self, s: u32, t: &Target) -> bool {
        self.actor_host().apply_fsm_ready(s, t)
    }
    fn unit_hp(&self, s: u32, t: &Target, p: bool) -> Option<f32> {
        self.actor_host().unit_hp(s, t, p)
    }
    fn tutorials_enabled(&self) -> bool {
        self.actor_host().tutorials_enabled()
    }
    fn pl_check(&self, k: u32) -> i32 {
        self.actor_host().pl_check(k)
    }
    fn player_damaged(&self) -> bool {
        self.actor_host().player_damaged()
    }
    fn pawn_lost(&self) -> bool {
        self.actor_host().pawn_lost()
    }
    fn has_item(&self, i: u32, c: u32) -> bool {
        self.actor_host().has_item(i, c)
    }
}

/// Far below the world: where seats stand while a waypoint does not need
/// them.
pub const AWAY: [f32; 3] = [0.0, -100_000.0, 0.0];

/// What [`fast_forward`] saw.
#[derive(Debug, Default, Clone)]
pub struct Forwarded {
    pub frames: u64,
    pub ladder: Vec<u32>,
    /// Last event/script party placement; world.start_pos is the area-entry selector.
    pub arrival: Option<(u32, u32)>,
}

/// Walks `route` headless: seat 0 stands in each waypoint's volume (the
/// host may place an Arisen directly), presses interact every other frame
/// where the waypoint asks for it, and moves on when the waypoint's SceNo
/// is reached (else after `stay` frames). Other seats stand [`AWAY`].
/// After the last waypoint the seats stand [`AWAY`] until `done` holds.
pub fn fast_forward<D: Data>(
    h: &mut Host<D, OpeningWorld>,
    route: &[Waypoint],
    stay: u64,
    max_frames: u64,
    done: impl Fn(&Host<D, OpeningWorld>) -> bool,
) -> Result<Forwarded, HostError> {
    let seats: Vec<_> = h.roster.ids().collect();
    let (mut step, mut at_step) = (0usize, 0u64);
    let mut out = Forwarded {
        ladder: vec![h.story.as_ref().map_or(0, |s| s.world.sce_no)],
        ..Default::default()
    };
    for frame in 0..max_frames {
        let w = route.get(step);
        if w.is_none() && done(h) {
            out.frames = frame;
            return Ok(out);
        }
        let s = h.story.as_ref().ok_or(HostError::NoData("story"))?;
        let (pos, yaw) = match w {
            Some(w) if w.stage == s.stage.stage_no => w.place(&s.stage.scenarios).unwrap_or((AWAY, 0.0)),
            _ => (AWAY, 0.0),
        };
        for (k, &id) in seats.iter().enumerate() {
            if let Some(p) = h.sim.player_mut(id) {
                (p.position, p.yaw) = if k == 0 { (pos, yaw) } else { (AWAY, 0.0) };
            }
        }
        h.world.npcs = w
            .map(|w| {
                w.npcs
                    .iter()
                    .map(|&id| NpcUnit {
                        id,
                        unit: ddstory::Player { pos, yaw: 0.0 },
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut inputs = FrameInputs::new();
        for (k, &id) in seats.iter().enumerate() {
            let interact = k == 0 && w.is_some_and(|w| w.interact) && frame % 2 == 0;
            inputs.insert(
                id,
                PlayerInput {
                    interact,
                    ..Default::default()
                },
            );
        }
        let report = h.tick(&inputs)?;
        for request in report.requests {
            if let Request::PlaceParty {
                start_pos: Some(n), ..
            } = request
            {
                out.arrival = h.story.as_ref().map(|s| (s.world.stage_no, n));
            }
        }
        let s = h.story.as_ref().ok_or(HostError::NoData("story"))?;
        if out.ladder.last() != Some(&s.world.sce_no) {
            out.ladder.push(s.world.sce_no);
        }
        at_step += 1;
        let Some(w) = w else { continue };
        let reached = match w.until {
            Some(n) => s.world.sce_no >= n && at_step >= 2,
            None => at_step >= stay,
        };
        if reached && w.stage == s.stage.stage_no {
            step += 1;
            at_step = 0;
        }
    }
    Err(HostError::Story(ddstory::Error::Data(format!(
        "opening stalled at waypoint {step} after {max_frames} frames"
    ))))
}

/// The opening route up to q0001's start in Cassardis (stage 200, SceNo
/// 1100, after event st200 ev05): the prologue and q0000.
pub fn to_newly_arisen() -> Vec<Waypoint> {
    let mut r = ddstory::route::opening();
    let end = r
        .iter()
        .position(|w| w.name == "cassardis: dragon attack")
        .map_or(r.len(), |i| i + 1);
    r.truncate(end);
    r
}

/// The event rows the opening's Cassardis part plays (st200 ev10 and ev15,
/// EVENTS.md).
pub const CASSARDIS_ROWS: [usize; 2] = [22, 23];
