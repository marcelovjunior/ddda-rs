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
    /// Host-format chunk, not the original file format. Only permanent
    /// row masks are serialized; tables are reread from the player's install.
    pub(crate) fn save_npc_quest_controls(&self, file: &mut crate::save::SaveFile) {
        let saved = self.saved_npc_quest_rows();
        let mut writer = ddid::Writer::new();
        writer.u8(1).len(saved.len());
        for (npc, rows) in saved {
            writer.u32(npc).len(rows.len());
            for row in rows { writer.len(row.table).len(row.row); }
        }
        file.put(crate::save::TAG_NPC_QUEST, crate::save::Owner::Shared, writer.finish());
    }

    pub(crate) fn saved_npc_quest_rows(&self) -> Vec<(u32, Vec<ddnpcrt::quest_talk::RowKey>)> {
        self.npcs.quest_controls.iter().filter_map(|(&npc, control)| {
            let rows = control.saved_disabled_rows();
            (!rows.is_empty()).then_some((npc, rows))
        }).collect()
    }

    pub(crate) fn load_npc_quest_controls(&mut self, file: &crate::save::SaveFile) -> Result<(), HostError> {
        let mut controls = std::collections::BTreeMap::new();
        if let Some(bytes) = file.get(crate::save::TAG_NPC_QUEST, crate::save::Owner::Shared) {
            let mut reader = ddid::Reader::new(bytes);
            reader.version("NPCQ", 1).map_err(HostError::Save)?;
            let count = reader.len(8).map_err(HostError::Save)?;
            if count != 0 {
                let data = self.npc_data()?;
                for _ in 0..count {
                    let npc = reader.u32().map_err(HostError::Save)?;
                    if npc >= 900 || controls.contains_key(&npc) {
                        return Err(HostError::Save("invalid or duplicate saved NPC control".into()));
                    }
                    let mut rows = Vec::new();
                    for _ in 0..reader.len(8).map_err(HostError::Save)? {
                        rows.push(ddnpcrt::quest_talk::RowKey {
                            table: reader.u32().map_err(HostError::Save)? as usize,
                            row: reader.u32().map_err(HostError::Save)? as usize,
                        });
                    }
                    let table = data.quest_control(npc).map_err(|error| HostError::Save(error.to_string()))?
                        .ok_or_else(|| HostError::Save("saved NPC QCT missing from install".into()))?;
                    let mut control = ddnpcrt::quest_talk::Control::new(table);
                    control.restore_disabled_rows(&rows).map_err(|error| HostError::Save(error.to_string()))?;
                    controls.insert(npc, control);
                }
            }
            reader.end().map_err(HostError::Save)?;
        }
        // Absent old-save chunk means empty, including load into an existing
        // runtime. No transient conversation survives a load.
        let players: Vec<_> = self.roster.ids().collect();
        for player in players {
            if let Some(npc) = self.npcs.talk_abort(player) { let _ = self.locks.release(npc, player); }
        }
        self.npcs.quest_controls = controls;
        Ok(())
    }
    /// Ordinary NPC control selection before the base/shop conversation.
    /// Called only after the materialized lifetime and dialogue slot are
    /// admitted. Missing/unsupported data never fabricates a reward.
    pub(crate) fn talk_quest_or_base(&mut self, player: PlayerId, npc: u32) -> Result<TalkOutcome, HostError> {
        let data = self.npc_data()?;
        // Preload the item rules before phase1: phase2 cannot lose its grant
        // due to a late missing data source after consuming the once row.
        if data.quest_control(npc).map_err(|error| HostError::Npc(error.to_string()))?.is_some() {
            if let Err(error) = data.quest_items() {
                return Err(HostError::Npc(error.to_string()));
            }
        }
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        let story = ddnpcrt::Story { world, equip_menus: false };
        self.locks.acquire(npc, player).map_err(HostError::Lock)?;
        let result = self.npcs.talk_quest(&*data, &story, player, npc, None);
        match result {
            Ok(Some(events)) => {
                self.after_talk(player, events)
            }
            Ok(None) => self.talk(player, npc),
            Err(error) => {
                let _ = self.locks.release(npc, player);
                Err(HostError::Npc(error.to_string()))
            }
        }
    }
    pub(crate) fn talk_authored(&mut self, player: PlayerId, npc: u32,
        set: std::sync::Arc<ddnpcrt::TalkSet>, conversation: i32) -> Result<TalkOutcome, HostError> {
        self.locks.acquire(npc, player).map_err(HostError::Lock)?;
        let world = &self.story.as_ref().ok_or(HostError::NoData("story"))?.world;
        let story = ddnpcrt::Story { world, equip_menus: false };
        match self.npcs.talk_authored(&story, player, npc, set, conversation) {
            Ok(events) => self.after_talk(player, events),
            Err(error) => {
                let _ = self.locks.release(npc, player);
                Err(HostError::Talk(error))
            }
        }
    }
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
        let clock = ddnpcrt::schedule::Clock {
            hour: story.world.hour,
            minute: story.world.minute,
            dt: self.clock.seconds(),
        };
        let mut out = vec![];
        // Step each owner exactly once. Retrying the aggregate after a later
        // owner fails would discard earlier requests and advance those owners
        // a second time in this same host frame.
        for (&id, npc) in &mut self.npcs.npcs {
            npc.observe_scene(Some(story.world.sce_no));
            match npc.step_observed(&*data, clock, Some(&story.world)) {
                Ok(requests) => out.extend(requests),
                Err(error) => {
                    if let Some(pos) = npc.position() {
                        self.npc_parked.insert(id, pos);
                    }
                    // An unsupported port graph is not original intel/controller
                    // destruction. Preserve owner state for placement and ApplyFSM.
                    npc.halt_graphs();
                    self.npc_errors.push((id, error.to_string()));
                }
            }
        }
        Ok(out)
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
        let mut guard_exit = false;
        for e in &events {
            if let TalkEvent::QuestEffects { player: owner, effects, .. } = e {
                if *owner != player { return Err(HostError::UnknownPlayer(*owner)); }
                let data = self.npc_data()?;
                let items = data.quest_items().map_err(|error| HostError::Npc(error.to_string()))?;
                for effect in effects {
                    match *effect {
                        ddnpcrt::quest_talk::Effect::Item { item, count, level } => {
                            self.economy.inventory.add(&items, ddeconomy::inventory::Owner::Arisen(player),
                                player, item, count, level, &mut self.economy.buyback);
                        }
                        ddnpcrt::quest_talk::Effect::QuestBit { quest, bit, value } => {
                            self.story.as_mut().ok_or(HostError::NoData("story"))?.world.set_quest_bit(quest, bit, value);
                        }
                        ddnpcrt::quest_talk::Effect::Gsf { flag, value } => {
                            self.story.as_mut().ok_or(HostError::NoData("story"))?.world.gsf.set(flag, value);
                        }
                    }
                }
            }
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
                TalkEvent::End { npc, .. } => {
                    guard_exit = self.story_guard_text_ended(player, npc);
                    end_npc = Some(npc);
                }
                _ => {}
            }
        }
        if let (Some(npc), None, false) = (end_npc, session, guard_exit) {
            let _ = self.locks.release(npc, player);
        }
        Ok(TalkOutcome { events, session })
    }
}
