//! NPCs in the host (ddnpcrt): daily schedules on the clock, talking, and
//! the hand-over from a talk to a shop screen under one interaction lock.
//!
//! A talk takes the NPC's lock for the talking seat (COOP_DESIGN.md). When
//! the talk ends with a shop command, the lock stays with the shop session
//! the host opens (`Host::open_shop`, `Economy::open_upgrade`) and goes
//! with `Host::close_shop`; any other ending frees it.

use crate::host::{Host, HostError, StoryWorld};
use crate::player::PlayerId;
use ddnpcrt::talk::{Action, MenuKind};
use ddnpcrt::TalkEvent;
use ddstory::data::Data;

/// What a talk step produced: the events for the view, and the shop
/// session a shop command opened.
#[derive(Debug, Clone, PartialEq)]
pub struct TalkOutcome {
    pub events: Vec<TalkEvent>,
    pub session: Option<ddeconomy::Session>,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    fn npc_data(&self) -> Result<std::sync::Arc<dyn ddnpcrt::NpcData + Send + Sync>, HostError> {
        self.data.npc.clone().ok_or(HostError::NoData("npc"))
    }

    /// Starts NPC `npc`'s daily schedule on the current stage
    /// (`NpcRuntime::spawn`). Without a headless NPC mover the host
    /// completes moves at once: the NPC stands at each route point it is
    /// sent to.
    pub fn spawn_npc(&mut self, npc: u32, use_24_schedule: u8) -> Result<bool, HostError> {
        let data = self.npc_data()?;
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        let ok = self.npcs.spawn(&*data, world, npc, use_24_schedule).map_err(|e| HostError::Npc(e.to_string()))?;
        if let Some(n) = self.npcs.npcs.get_mut(&npc) {
            n.complete_moves(true);
        }
        Ok(ok)
    }

    /// Pass A line 15 for NPCs: one frame of every scheduled NPC.
    ///
    /// An NPC whose graphs need what ddnpcrt does not translate yet (e.g.
    /// the `cAIActionFSMNpc.Timer` variables) leaves the schedule with its
    /// error noted in `Host::npc_errors`, instead of stopping the frame for
    /// everyone. It stays where it was (`Host::npc_parked`), and can still
    /// be talked to.
    pub(crate) fn step_npcs(&mut self) -> Result<Vec<ddnpcrt::schedule::Request>, HostError> {
        let (Some(data), Some(story)) = (self.data.npc.clone(), self.story.as_ref()) else {
            return Ok(vec![]);
        };
        match self.npcs.step(&*data, &story.world, self.clock.seconds(), &[]) {
            Ok(r) => Ok(r),
            Err(_) => {
                // find the NPCs that fail on their own and drop them
                let clock = ddnpcrt::schedule::Clock {
                    hour: story.world.hour,
                    minute: story.world.minute,
                    dt: self.clock.seconds(),
                };
                let ids: Vec<u32> = self.npcs.npcs.keys().copied().collect();
                let mut out = vec![];
                for id in ids {
                    let Some(npc) = self.npcs.npcs.get_mut(&id) else { continue };
                    match npc.step(&*data, clock) {
                        Ok(r) => out.extend(r),
                        Err(e) => {
                            if let Some(n) = self.npcs.npcs.remove(&id) {
                                if let Some(p) = n.position() {
                                    self.npc_parked.insert(id, p);
                                }
                            }
                            self.npc_errors.push((id, e.to_string()));
                        }
                    }
                }
                Ok(out)
            }
        }
    }

    /// Seat `player` talks to NPC `npc`. Refused while the other Arisen
    /// holds the NPC.
    pub fn talk(&mut self, player: PlayerId, npc: u32) -> Result<TalkOutcome, HostError> {
        self.roster.seat(player).ok_or(HostError::UnknownPlayer(player))?;
        let data = self.npc_data()?;
        let shop = self.data.catalog.as_ref().is_some_and(|c| c.shops.contains_key(&npc));
        self.locks.acquire(npc, player).map_err(HostError::Lock)?;
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        let story = ddnpcrt::Story { world, equip_menus: false };
        match self.npcs.talk(&*data, &story, player, npc, shop) {
            Ok(ev) => self.after_talk(player, ev),
            Err(e) => {
                let _ = self.locks.release(npc, player);
                Err(HostError::Talk(e))
            }
        }
    }

    /// The current line was read.
    pub fn talk_next(&mut self, player: PlayerId) -> Result<TalkOutcome, HostError> {
        let ev = self.npcs.talk_next(player).map_err(HostError::Talk)?;
        self.after_talk(player, ev)
    }

    /// The player answered with option `index`.
    pub fn talk_choose(&mut self, player: PlayerId, index: u8) -> Result<TalkOutcome, HostError> {
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        let story = ddnpcrt::Story { world, equip_menus: false };
        let ev = self.npcs.talk_choose(&story, player, index).map_err(HostError::Talk)?;
        self.after_talk(player, ev)
    }

    /// Ends a shop session opened from a talk and shows the keeper's
    /// after-screen line (`MenuKind::AfterBuy` / `AfterSell`).
    pub fn close_shop_talk(&mut self, s: ddeconomy::Session, kind: Option<MenuKind>) -> Result<TalkOutcome, HostError> {
        self.close_shop(s);
        let Some(kind) = kind else {
            return Ok(TalkOutcome { events: vec![], session: None });
        };
        let data = self.npc_data()?;
        self.locks.acquire(s.npc, s.player).map_err(HostError::Lock)?;
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        let story = ddnpcrt::Story { world, equip_menus: false };
        match self.npcs.talk_after_shop(&*data, &story, s.player, s.npc, kind) {
            Ok(ev) => self.after_talk(s.player, ev),
            Err(e) => {
                let _ = self.locks.release(s.npc, s.player);
                Err(HostError::Talk(e))
            }
        }
    }

    /// Opens what a talk's command asks for and settles the lock.
    fn after_talk(&mut self, player: PlayerId, events: Vec<TalkEvent>) -> Result<TalkOutcome, HostError> {
        let mut session = None;
        let mut end_npc = None;
        for e in &events {
            match *e {
                TalkEvent::Action { npc, action: Action::Shop { mode }, .. } => {
                    session = Some(match mode {
                        // Buy / sell: the NPC's shop list.
                        0 | 1 => self.open_shop(player, npc)?,
                        // The upgrade screens (no shop list for mode 3).
                        2 | 3 => {
                            self.locks.acquire(npc, player).map_err(HostError::Lock)?;
                            self.economy.open_upgrade(player, npc, mode == 3).map_err(HostError::Economy)?
                        }
                        // The equipment screen is not a headless system yet.
                        _ => continue,
                    });
                }
                TalkEvent::End { npc, .. } => end_npc = Some(npc),
                _ => {}
            }
        }
        if let (Some(npc), None) = (end_npc, session) {
            let _ = self.locks.release(npc, player);
        }
        Ok(TalkOutcome { events, session })
    }
}
