//! The session's story: quests, stage scripts, scenarios, the event mode.

use crate::data::Data;
use crate::qct::Ctrl;
use crate::scenario::Scenarios;
use crate::script::Script;
use crate::{orders::Orders, Env, Error, Host, HostEvent, Inputs, Menu, Request, World};

/// A cutscene request (built by the Event order 0x005bcb70, posted by
/// 0x00516ec0, kept at aStage +0xa0 / +0xd0).
#[derive(Debug, Clone, PartialEq)]
pub struct EventRequest {
    pub row: usize,
    pub stage: u32,
    pub event: u32,
    pub sce_no: Option<u32>,
    pub set_pos_type: u8,
    pub start_pos: u32,
}

/// aStage's event fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventMode {
    /// +0x84: set when a request is accepted, cleared when it has ended.
    pub active: bool,
    /// +0x88 / +0xa0: accepted, not yet started.
    pub pending: Option<EventRequest>,
    /// +0x8c / +0xd0: playing.
    pub playing: Option<EventRequest>,
    /// +0x85: the event left loading and plays (IsEventStart).
    pub started: bool,
    /// +0x12a: a simple event (scripted camera, stage mode 4 0x0050dcd0)
    /// plays; it also keeps `active` (+0x84) set.
    pub simple: bool,
}

/// The loaded stage as far as the story is concerned (aStage).
/// The sGameSys words the script gate 0x0044d430 tests. What sets the
/// individual bits was not traced (stop requests of menus, talks and
/// similar); the host reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameFlags {
    /// sGameSys +0xb883c.
    pub b883c: u32,
    /// sGameSys +0xb8844.
    pub b8844: u32,
    /// sGameSys +0xb8848.
    pub b8848: u32,
    /// sGameSys +0x54.
    pub s54: u32,
}

/// True when story scripts and quest control tables do not run this frame:
/// a stop bit is set (+0xb883c bit 27, +0xb8848 bits 26-28, +0xb8844 bit
/// 23), +0x54 is non-zero, or the stage (+0x34) is 801-804 (0x321-0x324,
/// the character editor). Callers: uFSMTask::vf08 0x005d1160,
/// uFSMQuest::vf08 0x0078eb50 and the quest update 0x0078c830 (QCT).
// Translated from 0x0044d430 (DDDA.exe 2017-12-13 Steam).
pub fn script_gate(stage_no: u32, f: &GameFlags) -> bool {
    f.b883c & 0x0800_0000 != 0
        || f.b8848 & 0x1000_0000 != 0
        || f.b8848 & 0x0800_0000 != 0
        || f.b8848 & 0x0400_0000 != 0
        || f.b8844 & 0x0080_0000 != 0
        || f.s54 != 0
        || (0x321..=0x324).contains(&stage_no)
}

#[derive(Debug, Default)]
pub struct StageState {
    /// Stage-200 challenge state, reset on every stage load.
    pub dragon_challenge: crate::dragon::DragonChallenge,
    /// aStage +0x724; 0 while no stage is loaded.
    pub stage_no: u32,
    pub event: EventMode,
    /// sGameSys +0x16dd, reported by the host.
    pub fade_black: bool,
    pub scenarios: Scenarios,
    /// A camera requested by the Camera order is running (EndCamera false).
    pub camera_running: bool,
    /// A message requested by an order is on screen.
    pub message_showing: bool,
    /// Priority-1 pawn-message availability (0x00634090), or no GUI.
    /// IsDispCmcMess is false only when exactly three slots are available.
    pub pawn_message_slots: Option<u32>,
    /// A menu opened by a request is open (the stage is in its mode, e.g.
    /// 0x19 for the vocation menu).
    pub menu_open: bool,
    /// Who pressed interact this frame.
    pub interacting: Option<usize>,
    /// ValidQuestNum (0x00484050), computed before each frame.
    pub valid_quests: u32,
    /// NPC units this frame (from [`crate::Inputs`]).
    pub npcs: Vec<crate::NpcUnit>,
    /// sGameSys request and stop flags the script gate reads (0x0044d430);
    /// the host reports them with [`HostEvent::GameFlags`].
    pub game_flags: GameFlags,
}

/// Deferred story operations, applied in order after each phase.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    BootQuest { quest: u32, branch: u32, start: bool },
    QuestFlags { quest: u32, set: u32, clear: u32 },
    Log { quest: u32, entry: u32 },
    Activate { quest: u32 },
    Reward { quest: u32 },
    /// Start a stage script (0x005080e0), e.g. from a BootFSM scenario.
    StartScript { path: String },
}

/// cQuest.
pub struct Quest {
    /// `quest << 16 | branch` (cQuest +0x30).
    pub id: u32,
    /// cQuest +0x2c: 1 loaded, 2 started, 4 cleared, 8 failed, 0x20
    /// finished, 0x40 removed.
    pub flags: u32,
    pub script: Option<Script>,
    pub ctrl: Option<Ctrl>,
    /// Quest-log entries in order (sQuestManager +0x154, 0x00485d50).
    pub log: Vec<u32>,
}

impl Quest {
    pub fn number(&self) -> u32 {
        self.id >> 16
    }
}

pub struct Story<D: Data> {
    pub data: D,
    pub world: World,
    pub stage: StageState,
    pub quests: Vec<Quest>,
    /// Stage scripts (uFSMTask list of cFSMTaskCtrl), creation order.
    pub tasks: Vec<Script>,
    pub frame: u64,
}

impl<D: Data> Story<D> {
    /// New game (0x00445ab0): stage 380 at start position 0, SceNo 100.
    /// The host loads the stage and reports [`HostEvent::StageLoaded`].
    pub fn new_game(data: D) -> Story<D> {
        let mut world = World::default();
        world.stage_no = 380;
        world.start_pos = 0;
        world.set_sce_no(100, false);
        // Translated from 0x00445be1 (DDDA.exe 2017-12-13 Steam):
        // sGameSys+0xaee78 bit 31, stage-index 12's layout flag zero.
        world.set_lot(380, 0, true);
        Story { data, world, stage: StageState::default(), quests: vec![], tasks: vec![], frame: 0 }
    }

    /// Continue from a saved world. Running scripts are not saved by this
    /// runtime yet (the original saves non-`fix` scripts, 0x00495150).
    pub fn from_world(data: D, world: World) -> Story<D> {
        Story { data, world, stage: StageState::default(), quests: vec![], tasks: vec![], frame: 0 }
    }

    pub fn quest(&self, number: u32) -> Option<&Quest> {
        self.quests.iter().find(|q| q.number() == number)
    }

    pub fn script_running(&self, path: &str) -> bool {
        self.tasks.iter().any(|t| t.running && t.path.eq_ignore_ascii_case(path))
            || self
                .quests
                .iter()
                .any(|q| q.script.as_ref().is_some_and(|s| s.running && s.path.eq_ignore_ascii_case(path)))
    }

    /// Stage setup when the host reports the stage loaded.
    fn enter_stage(&mut self, stage: u32, cmds: &mut Vec<Command>) -> Result<(), Error> {
        self.tasks.clear();
        self.stage.stage_no = stage;
        self.stage.dragon_challenge = Default::default();
        self.stage.event = EventMode::default();
        self.world.stage_no = stage;
        // 0x004827e0: stage 800 with SceNo 1000 boots q0000 and q0031. The
        // game also requires sGameSys +0xb8844 bit 0x20000 here; which
        // routine sets it after the prologue is untraced (assumed set).
        if stage == 800 && self.world.sce_no == 1000 {
            cmds.push(Command::BootQuest { quest: 0, branch: 0, start: false });
            cmds.push(Command::BootQuest { quest: 31, branch: 0, start: false });
        }
        // 0x005081a0: every rAIFSM of the stage archive whose path contains
        // "fsm\fix" starts as a cFSMOrder task (not for stages 800-804).
        if !(800..=804).contains(&stage) {
            for p in self.data.stage_scripts(stage)? {
                if p.contains("fsm\\fix") {
                    cmds.push(Command::StartScript { path: p });
                }
            }
        }
        // 0x005083c0: the stage's scenario files.
        self.stage.scenarios = Scenarios::load(&self.data.scenarios(stage)?)?;
        Ok(())
    }

    fn start_script(&mut self, path: &str) -> Result<(), Error> {
        // 0x005d0e10 refuses a second task with the same path (0x005d0f50).
        if self.tasks.iter().any(|t| t.path.eq_ignore_ascii_case(path)) {
            return Ok(());
        }
        let g = self.data.fsm(path)?;
        self.tasks.push(Script::new(path, g)?);
        Ok(())
    }

    /// Quest boot (0x00484000 -> 0x00483f20 -> 0x00484d90 -> 0x0078d0d0).
    fn boot_quest(&mut self, quest: u32, branch: u32, start: bool) -> Result<(), Error> {
        let id = quest << 16 | branch;
        // 0x00483f20: a branch-0 story quest whose "cleared" (94) or
        // "failed" (93) bit is set is not booted again; exceptions there do
        // not concern the opening.
        if branch == 0 && (self.world.quest_bit(quest as usize, 94) || self.world.quest_bit(quest as usize, 93)) {
            return Ok(());
        }
        if self.quests.iter().any(|q| q.id == id) {
            if start {
                self.quest_flags(quest, 2, 0);
            }
            return Ok(());
        }
        let path = format!("quest\\q{quest:04}_b{branch:02}");
        let mut script = Script::new(&path, self.data.fsm(&path)?)?;
        script.owner.quest = Some(id);
        let ctrl = self.data.qct(&format!("etc\\questCtrl\\q{quest:04}_b{branch:02}"))?.map(Ctrl::new);
        self.quests.push(Quest { id, flags: 1 | if start { 2 } else { 0 }, script: Some(script), ctrl, log: vec![] });
        Ok(())
    }

    fn quest_flags(&mut self, quest: u32, set: u32, clear: u32) {
        for q in self.quests.iter_mut().filter(|q| q.number() == quest) {
            q.flags = q.flags & !clear | set;
        }
    }

    fn apply(&mut self, cmds: &mut Vec<Command>, out: &mut Vec<Request>) -> Result<(), Error> {
        for c in std::mem::take(cmds) {
            match c {
                Command::BootQuest { quest, branch, start } => self.boot_quest(quest, branch, start)?,
                Command::QuestFlags { quest, set, clear } => self.quest_flags(quest, set, clear),
                Command::Log { quest, entry } => {
                    for q in self.quests.iter_mut().filter(|q| q.number() == quest) {
                        q.log.push(entry);
                    }
                }
                Command::Activate { quest } => {
                    // 0x0044a400: sudden quests 41-49 (but 45) never become active.
                    if !((41..=49).contains(&quest) && quest != 45) {
                        self.world.active_quest = quest as i32;
                    }
                }
                Command::Reward { quest } => out.extend(crate::orders::reward(&self.data, quest)?),
                Command::StartScript { path } => self.start_script(&path)?,
            }
        }
        Ok(())
    }

    /// Event end (stage event mode substate 2, 0x0050c7a0).
    fn event_end(&mut self, out: &mut Vec<Request>, cmds: &mut Vec<Command>) -> Result<(), Error> {
        let Some(req) = self.stage.event.playing.take() else {
            return Err(Error::Data("EventFinished without a playing event".into()));
        };
        if let Some(s) = req.sce_no {
            self.world.set_sce_no(s, true);
        }
        self.stage.event.active = false;
        self.stage.event.started = false;
        match req.set_pos_type {
            0 => {}
            2 => out.push(Request::PlaceParty { start_pos: Some(req.start_pos), pos: None }),
            t => return Err(Error::Unimplemented(format!("event placement type {t}"))),
        }
        // Row 0x3e (st380 ev00, the prologue's last scene): the end raises
        // request bit 18 (0x0050d585, when sGameSys +0xb8844 & 0x40000,
        // assumed set in the prologue); stage mode 0x13 turns it into mode
        // 0x11 (0x0050f232) -> 0x0050f610 -> 0x00444840 -> 0x00444b20:
        // next stage 800, start position 0, SceNo 1000 (forced).
        if req.row == 0x3e {
            self.world.stage_no = 800;
            self.world.start_pos = 0;
            self.world.set_sce_no(1000, false);
            // 0x00444c20 (called by 0x00444b20): the post-prologue world.
            // Not modelled: the party-record resets of 0x00444840 and the
            // clock/inventory calls of 0x00444b20.
            let p = self.data.prologue_end().ok_or_else(|| {
                Error::Unimplemented("prologue end needs DDDA.exe (tables in the code of 0x00444c20)".into())
            })?;
            for &g in &p.gsf {
                self.world.gsf.set(g as usize, true);
            }
            for &(s, f) in &p.lots {
                self.world.set_lot(s, f as usize, true);
            }
            for &n in &p.revive {
                self.world.revive_npc(n as usize);
            }
            out.push(Request::JumpArea { stage: 800, pos: 0, fade: 0 });
        }
        let _ = cmds;
        Ok(())
    }

    /// One frame, in the order of the original's frame (docs/FRAME.md 6.2,
    /// `sAreaExt::vf06` 0x00419910 walking the area stack root first):
    /// 1. the host's reports since the last frame (completions, loads);
    /// 2. `aGameMain` slot 7 (0x004FB960): sQuestManager vf06 0x00482dd0,
    ///    the quests and their QCT;
    /// 3. `aStage` slot 7 (0x00504330): the stage mode (event end
    ///    0x0050c7a0, the vocation menu's close in mode 0x19, a pending
    ///    event request taken by play mode 0x0050b870);
    /// 4. then sScenario vf06 0x0049a0e0 (trigger volumes);
    /// 5. unit pass A, line 0: stage scripts (uFSMTask) and quest scripts
    ///    (uFSMQuest). The original runs them as concurrent jobs, so their
    ///    relative order is not defined; here stage scripts go first.
    pub fn tick(&mut self, inputs: &Inputs, host: &dyn Host) -> Result<Vec<Request>, Error> {
        let mut out = vec![];
        let mut cmds = vec![];
        self.frame += 1;
        // 1. Reports. The event end and the menu close are stage-mode
        // steps: they run in 3.
        let mut event_end = false;
        let mut menu_closed = false;
        for e in &inputs.events {
            match e {
                HostEvent::StageLoaded { stage } => self.enter_stage(*stage, &mut cmds)?,
                HostEvent::EventFinished => event_end = true,
                HostEvent::FadeBlack(b) => self.stage.fade_black = *b,
                HostEvent::CameraFinished => self.stage.camera_running = false,
                HostEvent::MessageFinished => self.stage.message_showing = false,
                HostEvent::PawnMessageSlots(slots) => self.stage.pawn_message_slots = *slots,
                HostEvent::MenuClosed(Menu::Vocation) => menu_closed = true,
                HostEvent::SetGsf { no, value } => self.world.gsf.set(*no as usize, *value),
                HostEvent::PlayerJob(j) => self.world.player_job = *j,
                HostEvent::GameFlags(f) => self.stage.game_flags = *f,
                HostEvent::SetQuestBit { quest, bit, value } => {
                    if *quest < 0x80 && *bit < 0x60 {
                        self.world.set_quest_bit(*quest as usize, *bit as usize, *value)
                    }
                }
            }
        }
        self.apply(&mut cmds, &mut out)?;
        if self.stage.stage_no == 0 {
            if event_end {
                self.event_end(&mut out, &mut cmds)?;
                self.apply(&mut cmds, &mut out)?;
            }
            return Ok(out);
        }
        let events = self.data.event_table().cloned();
        self.stage.npcs = inputs.npcs.clone();
        // 0x00484050: started, not finished quests (q < 0x80, not 31, not a
        // sudden quest); the per-quest log-slot test is read as "has a log
        // entry" (guessed).
        self.stage.valid_quests = self
            .quests
            .iter()
            .filter(|q| {
                let n = q.number();
                q.flags & 2 != 0
                    && q.flags & 0x20 == 0
                    && n <= 0x7f
                    && n != 31
                    && !((41..=49).contains(&n) && n != 45)
                    && !q.log.is_empty()
            })
            .count() as u32;
        // 2. Quests and QCT.
        self.quest_manager(inputs, host, &mut out, &mut cmds, events.as_ref())?;
        self.apply(&mut cmds, &mut out)?;
        // 3. Stage mode.
        if event_end {
            self.event_end(&mut out, &mut cmds)?;
        }
        // Stage mode 0x19 (0x00512fb0) on closing uGUIJobChange:
        // sGameSys +0xac710 |= 0x800, i.e. GSF 372.
        if menu_closed {
            self.world.gsf.set(372, true);
            self.stage.menu_open = false;
        }
        // Play mode (0x0050b870): a pending request becomes the playing one
        // (0x00516fc0).
        if self.stage.event.playing.is_none() {
            if let Some(req) = self.stage.event.pending.take() {
                // Translated from event setup 0x0050c290, case 0x17,
                // calling 0x00440190 (DDDA.exe 2017-12-13 Steam).
                // Recruitment precedes the movie/event start: story stock
                // 14 becomes pawn slot 1 (overall party member 2).
                if req.row == 0x17 {
                    out.push(Request::StoryPawn { stock: 14, slot: 1 });
                }
                out.push(Request::PlayEvent { row: req.row, stage: req.stage, event: req.event });
                self.stage.event.playing = Some(req);
                self.stage.event.started = true;
            }
        }
        self.apply(&mut cmds, &mut out)?;
        // Translated from aStage slot 7 -> slot 12 (0x00504330) and
        // aStage200::vf12 0x0051ccc0 (DDDA.exe 2017-12-13 Steam).
        // The stage-specific check follows event-mode work and precedes
        // scenarios; absent actor observations cannot complete it.
        if self.stage.stage_no == 200 && self.stage.game_flags.b8844 & 0x80000 == 0 {
            let (dragon, players) = host.dragon_challenge_actors();
            if self.stage.dragon_challenge.update(self.world.gsf.get(378), self.world.gsf.get(379), dragon, &players) {
                self.world.gsf.set(379, true);
            }
        }
        // 4. Scenarios.
        {
            let mut env = Env {
                world: &mut self.world,
                stage: &mut self.stage,
                host,
                time: inputs.time,
                out: &mut out,
                cmds: &mut cmds,
                events: events.as_ref(),
            };
            let mut sc = std::mem::take(&mut env.stage.scenarios);
            let r = sc.frame(&mut env, inputs);
            env.stage.scenarios = sc;
            r?;
        }
        self.apply(&mut cmds, &mut out)?;
        // 5. Scripts (line 0), unless gated (0x0044d430 in uFSMTask /
        // uFSMQuest vf08).
        if script_gate(self.world.stage_no, &self.stage.game_flags) {
            self.apply(&mut cmds, &mut out)?;
            return Ok(out);
        }
        let mut tasks = std::mem::take(&mut self.tasks);
        let mut res = Ok(());
        for t in tasks.iter_mut() {
            let mut env = Env {
                world: &mut self.world,
                stage: &mut self.stage,
                host,
                time: inputs.time,
                out: &mut out,
                cmds: &mut cmds,
                events: events.as_ref(),
            };
            if let Err(e) = t.step::<Orders>(&mut env) {
                res = Err(Error::Data(format!("{}: {e}", t.path)).with_kind(&e));
                break;
            }
        }
        // cFSMTaskCtrl vf05 0x005d0ce0 drops tasks whose script stopped.
        tasks.retain(|t| t.running);
        let started = std::mem::replace(&mut self.tasks, tasks);
        self.tasks.extend(started);
        res?;
        for q in self.quests.iter_mut() {
            if let Some(s) = q.script.as_mut() {
                let mut env = Env {
                    world: &mut self.world,
                    stage: &mut self.stage,
                    host,
                    time: inputs.time,
                    out: &mut out,
                    cmds: &mut cmds,
                    events: events.as_ref(),
                };
                s.step::<Orders>(&mut env).map_err(|e| Error::Data(format!("{}: {e}", s.path)).with_kind(&e))?;
            }
        }
        self.apply(&mut cmds, &mut out)?;
        Ok(out)
    }

    /// Every running script's state, stage scripts first (creation order),
    /// then quest scripts (quest order). Read-only; for replay hashes and
    /// debugging.
    pub fn script_states(&self) -> Vec<crate::script::ScriptState> {
        self.tasks
            .iter()
            .chain(self.quests.iter().filter_map(|q| q.script.as_ref()))
            .map(|s| s.state())
            .collect()
    }

    /// sQuestManager vf06 0x00482dd0 (story part): drop removed quests,
    /// then each quest's update (0x0078c830).
    fn quest_manager(
        &mut self,
        inputs: &Inputs,
        host: &dyn Host,
        out: &mut Vec<Request>,
        cmds: &mut Vec<Command>,
        events: Option<&ddquest::events::EventTable>,
    ) -> Result<(), Error> {
        self.quests.retain(|q| q.flags & 0x40 == 0);
        for q in self.quests.iter_mut() {
            // 0x0078c830 for a story quest (kind -1):
            match &q.script {
                // No unit: with nothing pending (0x0078e320), removed.
                None => {
                    q.flags |= 0x40;
                    continue;
                }
                // The script stopped: the unit is deleted, nothing else
                // this frame.
                Some(s) if !s.running => {
                    q.script = None;
                    continue;
                }
                _ => {}
            }
            let number = q.number();
            // 0x0078c830 skips the QCT while the script gate is shut
            // (0x0078ca80).
            if script_gate(self.world.stage_no, &self.stage.game_flags) {
                continue;
            }
            if let Some(ctrl) = q.ctrl.as_mut() {
                let mut env = Env {
                    world: &mut self.world,
                    stage: &mut self.stage,
                    host,
                    time: inputs.time,
                    out: &mut *out,
                    cmds: &mut *cmds,
                    events,
                };
                ctrl.frame(&mut env, number)?;
            }
        }
        Ok(())
    }
}

impl Error {
    fn with_kind(self, inner: &Error) -> Error {
        match (inner, self) {
            (Error::Unimplemented(_), Error::Data(s)) => Error::Unimplemented(s),
            (_, e) => e,
        }
    }
}
