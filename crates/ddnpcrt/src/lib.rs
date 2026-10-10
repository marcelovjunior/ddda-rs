//! Headless NPC runtime: daily schedules and positions, talking, and the
//! co-op interaction rule (one Arisen per NPC). Host-owned: the host feeds
//! the clock and the actor layer's completions, and performs the requests
//! and talk events (docs/NPCS.md, "Runtime").
//!
//! Rules are traced from DDDA.exe (2017-12-13 Steam), addresses in the
//! modules. The NPC graphs run on the shared AI core (`ddai::core`).

pub mod guard;
pub mod demo;
pub mod demo_reaction;
pub mod quest_talk;
pub mod schedule;
pub mod talk;

pub use ddai::Error;
use ddnpc::MessageSet;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use talk::{Action, Handler, MenuKind, TalkMemory};

/// Seat number, as `ddhost` and `ddsim` use.
pub type PlayerId = u8;

/// An NPC's base talk set (`id/npc_wind/npc_base_talk/nNNN`) and its lines
/// in the session language (`..._eng` etc.), as `uNpc::vf32` (0x00bace50)
/// and `cMessageResSet` (0x00457670) load them.
#[derive(Debug, Clone)]
pub struct TalkSet {
    pub set: MessageSet,
    pub text: Vec<String>,
}

/// Game data the runtime reads.
pub trait NpcData: schedule::Graphs {
    fn talk_set(&self, npc: u32) -> Result<Arc<TalkSet>, Error>;
    fn quest_control(&self, _npc: u32) -> Result<Option<Arc<ddquest::qct::QuestCtrl>>, Error> { Ok(None) }
    fn quest_items(&self) -> Result<Arc<dditem::itl::ItemList>, Error> {
        Err(Error::Unimplemented("NPC quest item list".into()))
    }
    fn quest_talk_set(&self, _npc: u32, _quest: u32) -> Result<Option<Arc<TalkSet>>, Error> {
        Err(Error::Unimplemented("NPC quest talk resource".into()))
    }
    /// Only an actually preloaded authored Guard resource may be selected.
    fn guard_talk_set(&self, _npc: u32, _guard: &schedule::Guard) -> Result<Option<Arc<TalkSet>>, Error> {
        Err(Error::Unimplemented("NPC Guard text resource loading".into()))
    }
}

/// Game data from the player's install, cached.
pub struct InstallData {
    native_pc: std::path::PathBuf,
    vfs: ddvfs::Vfs,
    /// GMD language suffix, e.g. "eng".
    pub language: String,
    graphs: Mutex<HashMap<String, Arc<mtfsm::Graph>>>,
    sets: Mutex<HashMap<u32, Arc<TalkSet>>>,
    controls: Mutex<HashMap<u32, Option<Arc<ddquest::qct::QuestCtrl>>>>,
    quest_items: Mutex<Option<Arc<dditem::itl::ItemList>>>,
}

impl InstallData {
    pub fn open(native_pc: impl AsRef<std::path::Path>, language: &str) -> Result<Self, Error> {
        Ok(InstallData {
            native_pc: native_pc.as_ref().to_path_buf(),
            vfs: ddvfs::Vfs::open(native_pc).map_err(Error::Data)?,
            language: language.to_string(),
            graphs: Mutex::default(),
            sets: Mutex::default(),
            controls: Mutex::default(),
            quest_items: Mutex::default(),
        })
    }
}

impl schedule::Graphs for InstallData {
    fn graph(&self, path: &str) -> Result<Arc<mtfsm::Graph>, Error> {
        let key = path.to_ascii_lowercase().replace('/', "\\");
        if let Some(g) = self.graphs.lock().unwrap().get(&key) {
            return Ok(g.clone());
        }
        let b = self.vfs.read(&key, mtarc::type_hash("rAIFSM")).map_err(Error::Data)?;
        let g = Arc::new(mtfsm::Graph::parse(&b).map_err(Error::Data)?);
        self.graphs.lock().unwrap().insert(key, g.clone());
        Ok(g)
    }
}

impl NpcData for InstallData {
    fn quest_items(&self) -> Result<Arc<dditem::itl::ItemList>, Error> {
        let mut cache = self.quest_items.lock().unwrap();
        if let Some(items) = cache.as_ref() { return Ok(items.clone()); }
        let bytes = self.vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList")).map_err(Error::Data)?;
        let items = Arc::new(dditem::itl::ItemList::parse(&bytes).map_err(|error| Error::Data(error.0))?);
        *cache = Some(items.clone());
        Ok(items)
    }
    fn quest_control(&self, npc: u32) -> Result<Option<Arc<ddquest::qct::QuestCtrl>>, Error> {
        if let Some(cached) = self.controls.lock().unwrap().get(&npc) { return Ok(cached.clone()); }
        let path = format!(r"etc\questCtrl\n{npc:03}QuestCtrl");
        let kind = mtarc::type_hash("rQuestCtrlTbl");
        let control = if self.vfs.find(&path).iter().any(|&(hash, _)| hash == kind) {
            let bytes = self.vfs.read(&path, kind).map_err(Error::Data)?;
            Some(Arc::new(ddquest::qct::QuestCtrl::parse(&bytes).map_err(|e| Error::Data(e.0))?))
        } else { None };
        self.controls.lock().unwrap().insert(npc, control.clone());
        Ok(control)
    }
    fn quest_talk_set(&self, npc: u32, quest: u32) -> Result<Option<Arc<TalkSet>>, Error> {
        let file = ddexe::ExeFile::next_to(&self.native_pc).map_err(|e| Error::Data(e.0))?;
        let exe = file.image().map_err(|e| Error::Data(e.0))?;
        if !guard::quest_preloads(&exe, npc).map_err(|e| Error::Data(e.0))?
            .iter().any(|&id| u32::from(id) == quest) { return Ok(None); }
        Ok(Some(self.authored_talk_set(&guard::quest_path(npc, quest as u16))?))
    }
    fn guard_talk_set(&self, npc: u32, config: &schedule::Guard) -> Result<Option<Arc<TalkSet>>, Error> {
        let mut resources = guard::Resources::default();
        match config.message_kind {
            2 => {
                let file = ddexe::ExeFile::next_to(&self.native_pc).map_err(|e| Error::Data(e.0))?;
                let exe = file.image().map_err(|e| Error::Data(e.0))?;
                if !guard::quest_preloads(&exe, npc).map_err(|e| Error::Data(e.0))?
                    .iter().any(|&quest| u32::from(quest) == config.quest) { return Ok(None); }
                resources.insert(guard::ResourceKey::Quest(config.quest),
                    self.authored_talk_set(&guard::quest_path(npc, config.quest as u16))?);
            }
            5 => resources.insert(guard::ResourceKey::Base, self.talk_set(npc)?),
            // The override registry is independent of the base talk set.
            4 => return Err(Error::Unimplemented("NPC Guard override resource registry".into())),
            _ => return Ok(None),
        }
        Ok(resources.select(config))
    }
    fn talk_set(&self, npc: u32) -> Result<Arc<TalkSet>, Error> {
        if let Some(s) = self.sets.lock().unwrap().get(&npc) {
            return Ok(s.clone());
        }
        let base = format!(r"id\npc_wind\npc_base_talk\n{npc:03}");
        let mss = self.vfs.read(&base, mtarc::type_hash("rMsgSet")).map_err(Error::Data)?;
        let set = MessageSet::parse(&mss).map_err(|e| Error::Data(e.0))?;
        let gmd = self
            .vfs
            .read(&format!("{base}_{}", self.language), mtarc::type_hash("rGUIMessage"))
            .map_err(Error::Data)?;
        let g = mtgmd::MessageFile::parse(&gmd).map_err(|e| Error::Data(e.0))?;
        let text = g.messages.iter().map(|s| s.to_string()).collect();
        let s = Arc::new(TalkSet { set, text });
        self.sets.lock().unwrap().insert(npc, s.clone());
        Ok(s)
    }
}

/// What a talk shows or asks the host to do.
#[derive(Debug, Clone, PartialEq)]
pub enum TalkEvent {
    /// Actual selected NPC QCT phase2, following the last read line.
    /// The host applies it once to the interacting seat and world.
    QuestEffects { player: PlayerId, npc: u32, effects: Vec<quest_talk::Effect> },
    /// Show one line (GMD index in the NPC's talk file, raw text with its
    /// markup). Answer with [`NpcRuntime::talk_next`].
    Line { player: PlayerId, npc: u32, conversation: i32, index: u32, text: String },
    /// Offer the conversation's choices. Answer with
    /// [`NpcRuntime::talk_choose`].
    Choices { player: PlayerId, npc: u32, conversation: i32, options: Vec<String> },
    /// The talk's command: the host opens the shop screen, runs the inn,
    /// etc. The talk has ended; the host keeps the NPC locked for the
    /// screen it opens.
    Action { player: PlayerId, npc: u32, action: Action },
    /// The talk is over; the NPC is free.
    End { player: PlayerId, npc: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkError {
    /// Another Arisen is talking to this NPC.
    Busy(PlayerId),
    /// The player is not talking to anyone (or not to this NPC).
    NotTalking,
    /// No conversation applies (or the NPC has no talk set).
    Silent,
}

#[derive(Debug, Clone)]
struct Talk {
    npc: u32,
    set: Arc<TalkSet>,
    conversation: i32,
    lines: Vec<u32>,
    at: usize,
    handler: Option<Handler>,
    quest_row: Option<quest_talk::RowKey>,
}

/// The story state a talk reads.
pub struct Story<'a> {
    pub world: &'a ddstory::World,
    /// sGameSys +0xb8840 bit 0x8000000 (shop menus with an equipment
    /// option); not in `World` yet.
    pub equip_menus: bool,
}

/// The NPC runtime of one session.
#[derive(Default)]
pub struct NpcRuntime {
    /// Scheduled NPCs on the current stage.
    pub npcs: BTreeMap<u32, schedule::Npc>,
    /// Talk memory per NPC (world state, saved with the session).
    pub memory: BTreeMap<u32, TalkMemory>,
    talks: BTreeMap<PlayerId, Talk>,
    pub quest_controls: BTreeMap<u32, quest_talk::Control>,
}

impl NpcRuntime {
    /// Ordinary materialized NPC talk: 0x00B9DC20 checks its installed
    /// control before 0x00BB4830's base chooser. Missing control/no selected
    /// row returns None; unsupported control is an explicit error.
    pub fn talk_quest(&mut self, data: &dyn NpcData, story: &Story,
        player: PlayerId, npc: u32, talk_counter: Option<u32>)
        -> Result<Option<Vec<TalkEvent>>, Error>
    {
        if self.talking_to(npc).is_some() || self.talks.contains_key(&player) {
            return Err(Error::Data("NPC quest talk already owned".into()));
        }
        if !self.quest_controls.contains_key(&npc) {
            let Some(table) = data.quest_control(npc)? else { return Ok(None); };
            self.quest_controls.insert(npc, quest_talk::Control::new(table));
        }
        let control = self.quest_controls.get_mut(&npc).unwrap();
        let context = quest_talk::Context { world: story.world, npc, talk_counter };
        let Some(selected) = control.begin(&context)? else { return Ok(None); };
        let set = match data.quest_talk_set(npc, selected.quest) {
            Ok(Some(set)) if set.set.by_number(selected.conversation).is_some() => set,
            result => {
                control.abort();
                return Err(match result {
                    Err(error) => error,
                    _ => Error::Data("selected NPC quest dialogue unavailable".into()),
                });
            }
        };
        if !set.set.by_number(selected.conversation).unwrap().active_choices().is_empty() {
            control.abort();
            return Err(Error::Unimplemented("NPC quest dialogue choice-result routing".into()));
        }
        let lines = talk::lines(&set.set, selected.conversation, &|f| talk::TalkState::gsf(story.world, f));
        self.talks.insert(player, Talk { npc, set, conversation: selected.conversation,
            lines, at: 0, handler: None, quest_row: Some(selected.row) });
        Ok(Some(self.show(player)))
    }

    /// Intel/body lifetime loss cancels selected text without phase2.
    /// Row mask memory belongs to the NPC, so survives body reconstruction.
    pub fn retire_quest_talk(&mut self, npc: u32) {
        let players: Vec<_> = self.talks.iter().filter(|(_, talk)| talk.npc == npc)
            .map(|(&player, _)| player).collect();
        for player in players { self.talk_abort(player); }
    }
    /// Start an NPC's daily schedule (`uNpcIntel::vf05` 0x00bbc4e0 ->
    /// 0x00bbdaa0). `use_24` is the placement's `mUse24Schedule`.
    pub fn spawn(&mut self, data: &dyn NpcData, world: &ddstory::World, npc: u32, use_24: u8) -> Result<bool, Error> {
        let Some(path) = ddnpc::schedule_path(world.stage_no as i32, world.sce_no as i32, npc as i32, use_24) else {
            return Ok(false);
        };
        self.npcs.insert(npc, schedule::Npc::scheduled(npc, data, &path)?);
        Ok(true)
    }

    /// One frame of every scheduled NPC. `done` lists the NPCs whose
    /// interface the actor layer finished since the last frame, with the
    /// position reached when it was a move.
    pub fn step(
        &mut self,
        data: &dyn NpcData,
        world: &ddstory::World,
        dt: f32,
        done: &[(u32, Option<[f32; 3]>)],
    ) -> Result<Vec<schedule::Request>, Error> {
        for &(n, pos) in done {
            if let Some(npc) = self.npcs.get_mut(&n) {
                npc.done(pos);
            }
        }
        let clock = schedule::Clock { hour: world.hour, minute: world.minute, dt };
        let mut out = vec![];
        for npc in self.npcs.values_mut() {
            npc.observe_scene(Some(world.sce_no));
            out.extend(npc.step_observed(data, clock, Some(world))?);
        }
        Ok(out)
    }

    /// Where an NPC stands, as far as the runtime knows.
    pub fn position(&self, npc: u32) -> Option<[f32; 3]> {
        self.npcs.get(&npc).and_then(|n| n.position())
    }

    /// Who is talking to `npc`.
    pub fn talking_to(&self, npc: u32) -> Option<PlayerId> {
        self.talks.iter().find(|(_, t)| t.npc == npc).map(|(&p, _)| p)
    }

    /// `player` talks to `npc`. A shop NPC (one with a shop list) opens its
    /// menu conversation (`0x00b9ddc0`, kind 0); any other NPC the
    /// conversation the chooser picks for the story band (`0x00bb4830`).
    pub fn talk(
        &mut self,
        data: &dyn NpcData,
        story: &Story,
        player: PlayerId,
        npc: u32,
        shop: bool,
    ) -> Result<Vec<TalkEvent>, TalkError> {
        if let Some(p) = self.talking_to(npc) {
            if p != player {
                return Err(TalkError::Busy(p));
            }
        }
        let set = data.talk_set(npc).map_err(|_| TalkError::Silent)?;
        let w = story.world;
        let conversation = if shop {
            talk::shop_menu(MenuKind::Open, npc, w.stage_no, story.equip_menus, false)
        } else {
            let mem = self.memory.entry(npc).or_default();
            talk::entry(&set.set, mem, npc as i32, w.sce_band, w)
        }
        .filter(|&c| set.set.by_number(c).is_some())
        .ok_or(TalkError::Silent)?;
        self.start(player, npc, set, conversation, w)
    }

    /// Start an authored conversation from an explicitly loaded resource.
    /// Guard routing uses 0x00bb47d0 -> 0x00455990. The caller must first
    /// establish actor eligibility, reservation and activation; this API
    /// neither decides those conditions nor synthesizes actor TalkEnd.
    pub fn talk_authored(
        &mut self, story: &Story, player: PlayerId, npc: u32,
        set: Arc<TalkSet>, conversation: i32,
    ) -> Result<Vec<TalkEvent>, TalkError> {
        if let Some(other) = self.talking_to(npc) {
            if other != player { return Err(TalkError::Busy(other)); }
        }
        if set.set.by_number(conversation).is_none() {
            return Err(TalkError::Silent);
        }
        self.start(player, npc, set, conversation, story.world)
    }

    /// Show a shop NPC's menu again after its screen closed (kinds 1/2).
    pub fn talk_after_shop(
        &mut self,
        data: &dyn NpcData,
        story: &Story,
        player: PlayerId,
        npc: u32,
        kind: MenuKind,
    ) -> Result<Vec<TalkEvent>, TalkError> {
        let set = data.talk_set(npc).map_err(|_| TalkError::Silent)?;
        let c = talk::shop_menu(kind, npc, story.world.stage_no, story.equip_menus, false)
            .filter(|&c| set.set.by_number(c).is_some())
            .ok_or(TalkError::Silent)?;
        self.start(player, npc, set, c, story.world)
    }

    fn start(&mut self, player: PlayerId, npc: u32, set: Arc<TalkSet>, c: i32, w: &ddstory::World) -> Result<Vec<TalkEvent>, TalkError> {
        let lines = talk::lines(&set.set, c, &|f| talk::TalkState::gsf(w, f));
        self.talks.insert(player, Talk { npc, set, conversation: c, lines, at: 0, handler: None, quest_row: None });
        Ok(self.show(player))
    }

    fn show(&mut self, player: PlayerId) -> Vec<TalkEvent> {
        let Some(t) = self.talks.get_mut(&player) else { return vec![] };
        if let Some(&index) = t.lines.get(t.at) {
            let text = t.set.text.get(index as usize).cloned().unwrap_or_default();
            if let Some(h) = talk::line_handler(&text) {
                t.handler = Some(h);
            }
            return vec![TalkEvent::Line { player, npc: t.npc, conversation: t.conversation, index, text }];
        }
        let c = t.set.set.by_number(t.conversation).cloned();
        let options: Vec<String> = c
            .as_ref()
            .map(|c| c.active_choices().iter().map(|&i| t.set.text.get(i as usize).cloned().unwrap_or_default()).collect())
            .unwrap_or_default();
        if !options.is_empty() {
            return vec![TalkEvent::Choices { player, npc: t.npc, conversation: t.conversation, options }];
        }
        // No choices: the pending handler runs without an answer.
        self.finish(player, None)
    }

    fn finish(&mut self, player: PlayerId, choice: Option<u8>) -> Vec<TalkEvent> {
        let Some(t) = self.talks.remove(&player) else { return vec![] };
        let mut out = vec![];
        if let Some(row) = t.quest_row {
            if let Some(effects) = self.quest_controls.get_mut(&t.npc).and_then(|control| control.complete(row)) {
                out.push(TalkEvent::QuestEffects { player, npc: t.npc, effects });
            }
        }
        if let Some(h) = t.handler {
            let a = talk::action(h, choice);
            if a != Action::Nothing {
                out.push(TalkEvent::Action { player, npc: t.npc, action: a });
            }
        }
        out.push(TalkEvent::End { player, npc: t.npc });
        out
    }

    /// The current line was read.
    pub fn talk_next(&mut self, player: PlayerId) -> Result<Vec<TalkEvent>, TalkError> {
        let t = self.talks.get_mut(&player).ok_or(TalkError::NotTalking)?;
        t.at += 1;
        Ok(self.show(player))
    }

    /// The player picked option `index`. A choice with a jump
    /// (`mSelMsgJump`) continues with that conversation; otherwise the
    /// pending handler runs with the answer (`sIdMessage::vf06`).
    pub fn talk_choose(&mut self, story: &Story, player: PlayerId, index: u8) -> Result<Vec<TalkEvent>, TalkError> {
        let t = self.talks.get(&player).ok_or(TalkError::NotTalking)?;
        let jump = t
            .set
            .set
            .by_number(t.conversation)
            .and_then(|c| c.choice_jumps.get(index as usize).copied())
            .unwrap_or(-1);
        if jump >= 0 && t.set.set.by_number(jump as i32).is_some() {
            let (npc, set) = (t.npc, t.set.clone());
            return self.start(player, npc, set, jump as i32, story.world);
        }
        Ok(self.finish(player, Some(index)))
    }

    /// The player walked away or left the session.
    pub fn talk_abort(&mut self, player: PlayerId) -> Option<u32> {
        let talk = self.talks.remove(&player)?;
        if talk.quest_row.is_some() {
            if let Some(control) = self.quest_controls.get_mut(&talk.npc) { control.abort(); }
        }
        Some(talk.npc)
    }
}

#[cfg(test)]
mod tests;
