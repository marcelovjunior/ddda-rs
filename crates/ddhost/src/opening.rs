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
    /// NPC units for the scenario volumes that detect NPCs.
    pub npcs: Vec<NpcUnit>,
    /// Menus wait for the view ([`OpeningWorld::close_menu`]) instead of
    /// closing themselves.
    pub manual_menus: bool,
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
        OpeningWorld { inner: ScriptedHost::opening(), npcs: vec![], manual_menus: false, menu: None, pending: vec![] }
    }

    /// From now on every cutscene waits for the view (`inner.awaiting`,
    /// then [`Self::event_done`]) and menus wait for [`Self::close_menu`].
    pub fn live(&mut self, rows: impl IntoIterator<Item = usize>) {
        self.inner.play_rows.extend(rows);
        self.manual_menus = true;
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
    fn handle(&mut self, stage: u32, requests: &[Request]) {
        if !self.manual_menus {
            return self.inner.handle(stage, requests);
        }
        let mut rest = Vec::with_capacity(requests.len());
        for r in requests {
            match r {
                Request::OpenMenu { menu, player } => self.menu = Some((*menu, *player)),
                other => rest.push(other.clone()),
            }
        }
        self.inner.handle(stage, &rest);
    }
    fn events(&mut self) -> Vec<HostEvent> {
        let mut e = self.inner.events();
        e.append(&mut self.pending);
        e
    }
    fn npcs(&self) -> Vec<NpcUnit> {
        self.npcs.clone()
    }
}

impl ddstory::Host for OpeningWorld {
    fn layout_loaded(&self, s: u32, t: &Target) -> bool {
        self.inner.layout_loaded(s, t)
    }
    fn unit_exists(&self, s: u32, t: &Target) -> bool {
        self.inner.unit_exists(s, t)
    }
    fn group_loaded(&self, s: u32, k: u32, g: u32) -> bool {
        self.inner.group_loaded(s, k, g)
    }
    fn enemy_group_done(&self, s: u32, g: u32) -> bool {
        self.inner.enemy_group_done(s, g)
    }
    fn enemy_group_active(&self, s: u32, g: u32) -> bool {
        self.inner.enemy_group_active(s, g)
    }
    fn enemy_check(&self, s: u32, t: &Target, f: u32) -> bool {
        self.inner.enemy_check(s, t, f)
    }
    fn npc_intel(&self, s: u32, t: &Target) -> Option<NpcIntel> {
        self.inner.npc_intel(s, t)
    }
    fn npc_unit(&self, n: u32) -> bool {
        self.inner.npc_unit(n)
    }
    fn applied_script_finished(&self, s: u32, t: &Target) -> bool {
        self.inner.applied_script_finished(s, t)
    }
    fn unit_hp(&self, s: u32, t: &Target, p: bool) -> Option<f32> {
        self.inner.unit_hp(s, t, p)
    }
    fn tutorials_enabled(&self) -> bool {
        self.inner.tutorials_enabled()
    }
    fn pl_check(&self, k: u32) -> i32 {
        self.inner.pl_check(k)
    }
    fn player_damaged(&self) -> bool {
        self.inner.player_damaged()
    }
    fn pawn_lost(&self) -> bool {
        self.inner.pawn_lost()
    }
    fn has_item(&self, i: u32, c: u32) -> bool {
        self.inner.has_item(i, c)
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
    let mut out = Forwarded { ladder: vec![h.story.as_ref().map_or(0, |s| s.world.sce_no)], ..Default::default() };
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
            .map(|w| w.npcs.iter().map(|&id| NpcUnit { id, unit: ddstory::Player { pos, yaw: 0.0 } }).collect())
            .unwrap_or_default();
        let mut inputs = FrameInputs::new();
        for (k, &id) in seats.iter().enumerate() {
            let interact = k == 0 && w.is_some_and(|w| w.interact) && frame % 2 == 0;
            inputs.insert(id, PlayerInput { interact, ..Default::default() });
        }
        h.tick(&inputs)?;
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
    Err(HostError::Story(ddstory::Error::Data(format!("opening stalled at waypoint {step} after {max_frames} frames"))))
}

/// The opening route up to q0001's start in Cassardis (stage 200, SceNo
/// 1100, after event st200 ev05): the prologue and q0000.
pub fn to_newly_arisen() -> Vec<Waypoint> {
    let mut r = ddstory::route::opening();
    let end = r.iter().position(|w| w.name == "cassardis: dragon attack").map_or(r.len(), |i| i + 1);
    r.truncate(end);
    r
}

/// The event rows the opening's Cassardis part plays (st200 ev10 and ev15,
/// EVENTS.md).
pub const CASSARDIS_ROWS: [usize; 2] = [22, 23];
