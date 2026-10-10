//! The host: one session's world, players and systems, stepped in the
//! original's frame order (docs/FRAME.md, docs/adr/0002-game-host.md).

use crate::clock::FrameClock;
use crate::hash::StateHasher;
use crate::input::{Edges, FrameInputs};
use crate::lock::{InteractionLocks, LockError};
use crate::systems::SystemData;
use crate::player::{Control, Party, PlayerId, ProfileId, Roster};
use crate::save::{self, Owner, SaveFile};
use ddsim::{Ground, Sim};
use ddstory::data::Data;
use ddstory::{HostEvent, NpcUnit, Request, Story};
use std::collections::BTreeSet;

/// Everything the story asks of the rest of the game: the actors (NPCs,
/// enemies, objects) and the presentation (cutscenes, cameras, messages,
/// menus). Until those systems exist in headless crates, a stand-in
/// answers (`ddstory::scripted::ScriptedHost`).
pub trait StoryWorld: ddstory::Host {
    /// Advance host-owned authored cameras before story completion checks.
    fn camera_step(&mut self, _seconds: f32) {}
    /// Read-only projection of the authoritative camera clock for guests.
    fn quest_camera_active(&self) -> Option<&crate::quest_camera::Active> { None }
    /// Stage event camera input policy, applied independently to every seat.
    fn player_actions_blocked(&self, _seat: PlayerId) -> bool { false }
    /// Actual creation/destruction of a host-owned NPC intel and its
    /// normal controller. This says nothing about HP or a rendered body.
    fn observe_npc_intel(&mut self, _stage: u32, _target: ddstory::Target, _present: bool) {}
    /// Observe combat at the beginning of the story phase, before this
    /// frame's hit pass. Implementations bind actors to placement keys.
    fn observe_fight(&mut self, _fight: Option<&ddenemyrt::fight::Fight>) {}
    fn observe_npc_combat(&mut self, _stage: u32, _target: ddstory::Target,
        _facts: crate::npc_combat::Facts) {}
    /// Perform this frame's requests (stage `stage` is loaded).
    fn handle(&mut self, stage: u32, requests: &[Request]);
    /// Completions and facts due this frame, in the order they happened.
    fn events(&mut self) -> Vec<HostEvent>;
    /// NPC units on the stage, for trigger volumes that detect NPCs.
    fn npcs(&self) -> Vec<NpcUnit> {
        vec![]
    }
}

impl StoryWorld for ddstory::scripted::ScriptedHost {
    fn handle(&mut self, stage: u32, requests: &[Request]) {
        ddstory::scripted::ScriptedHost::handle(self, stage, requests)
    }
    fn events(&mut self) -> Vec<HostEvent> {
        ddstory::scripted::ScriptedHost::events(self)
    }
}

#[derive(Debug)]
pub enum HostError {
    Story(ddstory::Error),
    /// Input for a seat that is not in the roster.
    UnknownPlayer(PlayerId),
    Save(String),
    /// Another Arisen holds the NPC (the interaction lock).
    Lock(LockError),
    Economy(ddeconomy::Error),
    /// A talk could not start or continue (ddnpcrt).
    Talk(ddnpcrt::TalkError),
    /// An NPC graph failed (ddnpcrt / ddai).
    Npc(String),
    Rift(String),
    /// A system's game data was not given to the host.
    NoData(&'static str),
    /// Saving needs every interaction closed (shops, the Rift room).
    Busy,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Story(e) => write!(f, "story: {e}"),
            HostError::UnknownPlayer(p) => write!(f, "input for unseated player {p}"),
            HostError::Save(e) => write!(f, "save: {e}"),
            HostError::Lock(e) => write!(f, "interaction lock: {e:?}"),
            HostError::Economy(e) => write!(f, "economy: {e:?}"),
            HostError::Talk(e) => write!(f, "talk: {e:?}"),
            HostError::Npc(e) => write!(f, "npc: {e}"),
            HostError::Rift(e) => write!(f, "rift: {e}"),
            HostError::NoData(what) => write!(f, "no {what} data"),
            HostError::Busy => write!(f, "an interaction is open"),
        }
    }
}

impl std::error::Error for HostError {}

impl From<ddstory::Error> for HostError {
    fn from(e: ddstory::Error) -> Self {
        HostError::Story(e)
    }
}

/// What one frame produced for the views.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameReport {
    pub frame: u64,
    /// The story's requests, in order. `OpenMenu { player, .. }` carries a
    /// seat index into the roster (seat order).
    pub requests: Vec<Request>,
    /// The Arisen whose interact press reached the story this frame.
    pub interact: Option<PlayerId>,
    /// The NPCs' requests to the actor layer (ddnpcrt schedules).
    pub npc_requests: Vec<ddnpcrt::schedule::Request>,
    pub pawn_order_requests: Vec<crate::pawn_orders::Delivery>,
}

pub struct Host<D: Data, W: StoryWorld> {
    /// Frames run so far.
    pub frame: u64,
    pub clock: FrameClock,
    pub roster: Roster,
    pub party: Party,
    /// Movement of every Arisen (ddsim; the `Player` move line).
    pub sim: Sim,
    /// The session's story (ddstory; the `Pre Process` move line). `None`
    /// runs movement only.
    pub story: Option<Story<D>>,
    /// The actors and presentation the story talks to.
    pub world: W,
    pub ground: Box<dyn Ground + Send>,
    pub locks: InteractionLocks,
    /// Wallets, bags, shops (ddeconomy).
    pub economy: ddeconomy::Economy,
    /// NPC schedules and talks (ddnpcrt).
    pub npcs: ddnpcrt::NpcRuntime,
    pub story_npcs: crate::story_npcs::StoryNpcs,
    pub npc_combat: crate::npc_combat::NpcCombat,
    /// Current stage's observed completed collision cells, supplied by its
    /// loading adapter before the story phase; clear on leaving the field.
    pub scenario_field_cells: Option<ddstory::scenario::LoadedFieldCells>,
    /// NPCs that left the schedule on an error of their graphs (id, error),
    /// for the views to report.
    pub npc_errors: Vec<(u32, String)>,
    /// Where those NPCs stopped.
    pub npc_parked: std::collections::BTreeMap<u32, [f32; 3]>,
    /// Levels, skills, vocations (ddprogress).
    pub progression: ddprogress::Progression,
    /// The Rift: party pawns, pawn log, the room (ddrift).
    pub rift: ddrift::Rift,
    /// The systems' game data, when the host has it.
    pub data: SystemData,
    /// The seat that hired the pawn in each hired slot (Rift party slots
    /// 1 and 2).
    pub(crate) hired_by: [Option<PlayerId>; 2],
    /// Seats standing at a Riftstone; the room is open while any do.
    pub(crate) rift_visitors: BTreeSet<PlayerId>,
    edges: Edges,
    pub(crate) pawn_orders: crate::pawn_orders::Orders,
    /// The interact press made last frame, for this frame's scenarios.
    scenario_press: Option<PlayerId>,
    /// The story's stage is not loaded yet: ask the world to load it.
    boot: bool,
    /// Running hash of every request emitted, for replay checks.
    request_log: StateHasher,
    /// A fight in progress (ddenemyrt): enemies, hit volumes, damage.
    /// `None` runs without combat.
    pub fight: Option<ddenemyrt::fight::Fight>,
    pub opening_dragon_data: Option<crate::dragon::OpeningDragonData>,
    pub doors: Option<crate::doors::Doors>,
    /// Ambient enemy ownership; admission and view lifetimes are separate.
    pub ambient: crate::ambient::Population,
    /// The vocation (`ddskill::vocation::Vocation` id) whose fight kit the
    /// Arisen seats were given; 0 before any.
    kit_job: u32,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    pub fn new(sim: Sim, story: Option<Story<D>>, world: W, ground: Box<dyn Ground + Send>) -> Self {
        Host {
            frame: 0,
            clock: FrameClock::default(),
            roster: Roster::default(),
            party: Party::default(),
            sim,
            story,
            world,
            ground,
            locks: InteractionLocks::default(),
            economy: ddeconomy::Economy::default(),
            npcs: ddnpcrt::NpcRuntime::default(),
            story_npcs: Default::default(),
            npc_combat: Default::default(),
            scenario_field_cells: None,
            npc_errors: vec![],
            npc_parked: Default::default(),
            progression: ddprogress::Progression::new(),
            rift: ddrift::Rift::new(1, ddrift::PawnLog::default()),
            data: SystemData::default(),
            hired_by: [None; 2],
            rift_visitors: BTreeSet::new(),
            edges: Edges::default(),
            pawn_orders: Default::default(),
            scenario_press: None,
            boot: true,
            request_log: StateHasher::default(),
            fight: None,
            opening_dragon_data: None,
            doors: None,
            ambient: Default::default(),
            kit_job: 0,
        }
    }

    /// Gives the Arisen seats the fight kit of the story's current vocation
    /// (world `player_job`, set by the vocation menu's `PlayerJob` report or
    /// the progression) when it changes: the base vocations' level-1 data
    /// and starting weapon (`ddenemyrt::arisen`), equipped in the sim.
    /// *Assumed* for co-op: every Arisen seat takes seat 0's vocation (the
    /// story world holds one `player_job`). Vocations without a kit (the
    /// advanced and hybrid ones) keep the current kit.
    fn apply_job_kit(&mut self) {
        use ddenemyrt::fight::Kit;
        use ddskill::vocation::Vocation;
        let (Some(story), Some(fight)) = (&self.story, self.fight.as_mut()) else { return };
        let job = story.world.player_job;
        if job == self.kit_job {
            return;
        }
        let kit = match Vocation::from_id(job) {
            Some(Vocation::Fighter) => Kit::Fighter,
            Some(Vocation::Strider) => Kit::Strider,
            Some(Vocation::Mage) => Kit::Mage,
            _ => return,
        };
        self.kit_job = job;
        let ids: Vec<PlayerId> = self.roster.ids().collect();
        for &id in &ids {
            fight.add_arisen_as(id, kit);
            fight.kit(kit).equip(&mut self.sim, id);
        }
        // the first vocation choice starts each seat's level-1 character
        // record (experience, level-ups), when the host has the data
        if let Some(v) = Vocation::from_id(job) {
            for id in ids {
                if self.data.tables.is_some() && self.character_of(ddprogress::Role::Arisen(id)).is_none() {
                    let _ = self.create_arisen(id, v);
                }
            }
        }
    }

    /// The fight seat of hired pawn slot `index` (0, 1). *Viewer choice*:
    /// the seats after the Arisen (0-3) and their main pawns (4, 5).
    pub fn hired_pawn_seat(index: usize) -> PlayerId {
        6 + index as PlayerId
    }

    /// Gives each hired pawn (`rift.party.hired`, e.g. Rook from q0002's
    /// story request) a body in the simulation and a seat in the fight, with
    /// its record's vocation kit and its inclinations, beside its owner.
    /// *Assumed*: 2 m behind the first Arisen seat; released pawns keep
    /// their body (release is not wired here yet).
    fn sync_hired_pawn_bodies(&mut self) {
        use ddenemyrt::fight::Kit;
        let Some(fight) = self.fight.as_mut() else { return };
        let Some(owner) = self.roster.ids().next() else { return };
        let Some(at) = self.sim.player(owner).map(|p| (p.position, p.yaw)) else { return };
        for (index, hired) in self.rift.party.hired.iter().enumerate() {
            let Some(hired) = hired else { continue };
            let seat = Self::hired_pawn_seat(index);
            if fight.pawns.contains_key(&seat) {
                continue;
            }
            let kit = match ddskill::vocation::Vocation::from_id(u32::from(hired.pawn.param.job)) {
                Some(ddskill::vocation::Vocation::Strider) => Kit::Strider,
                Some(ddskill::vocation::Vocation::Mage) => Kit::Mage,
                _ => Kit::Fighter,
            };
            let place = Self::hired_pawn_place(index, at.0, at.1);
            // the record's inclination values (mInfo); the fight's pawn AI
            // takes nine
            let mut inclinations = hired.pawn.info_value;
            if inclinations.iter().all(|v| *v == 0.0) {
                // a fresh record's mInfo is 0.0 (Cmc::new); *assumed* the
                // balanced 500 the play view's pawn uses
                inclinations = [500.0; 9];
            }
            fight.add_pawn_as(&mut self.sim, seat, owner, place, at.1, inclinations, 7 + seat as u32, kit);
        }
    }

    /// Where hired pawn `index` stands beside an owner at `pos` facing `yaw`.
    /// *Assumed*: 2 m behind, 80 cm to either side.
    fn hired_pawn_place(index: usize, pos: [f32; 3], yaw: f32) -> [f32; 3] {
        let (s, c) = yaw.sin_cos();
        let side = if index == 0 { 80.0 } else { -80.0 };
        [pos[0] - s * 200.0 + c * side, pos[1], pos[2] - c * 200.0 - s * side]
    }

    /// Moves the hired pawns' bodies to their owner (the first Arisen seat),
    /// as the party travels together when an area jump places it. Call after
    /// placing the Arisen.
    pub fn place_hired_pawns(&mut self) {
        let Some(owner) = self.roster.ids().next() else { return };
        let Some((pos, yaw)) = self.sim.player(owner).map(|p| (p.position, p.yaw)) else { return };
        for index in 0..2 {
            let seat = Self::hired_pawn_seat(index);
            if let Some(p) = self.sim.player_mut(seat) {
                p.position = Self::hired_pawn_place(index, pos, yaw);
                p.yaw = yaw;
                p.velocity = [0.0; 3];
                p.drop_from = p.position[1];
            }
        }
    }

    /// Seats an Arisen and places it in the world. `None` when the roster
    /// is full or the profile is already seated.
    pub fn join(&mut self, profile: ProfileId, control: Control, pos: [f32; 3], yaw: f32) -> Option<PlayerId> {
        let id = self.roster.join(profile, control)?;
        self.sim.add_player(id, pos, yaw);
        self.economy.wallet(id);
        self.rift.party.reserved = [false, self.roster.len() >= 2];
        self.refresh_party();
        Some(id)
    }

    /// Unseats an Arisen: its body leaves the world and its locks go.
    pub fn leave(&mut self, id: PlayerId) {
        if self.roster.leave(id).is_some() {
            self.sim.players.retain(|p| p.id != id);
            self.locks.release_all(id);
            self.npcs.talk_abort(id);
            self.economy.close_all(id);
            self.rift_leave(id);
            self.rift.party.reserved = [false, self.roster.len() >= 2];
            self.refresh_party();
        }
    }

    /// One host frame, in the original's order. The steps are those of
    /// `sBbsRpgMain::vf06` (0x0041DD60) in docs/FRAME.md section 2; section
    /// 7 maps them to the host. Systems that do not exist yet are named
    /// where they will go.
    pub fn tick(&mut self, inputs: &FrameInputs) -> Result<FrameReport, HostError> {
        if let Some(&p) = inputs.keys().find(|p| self.roster.seat(**p).is_none()) {
            return Err(HostError::UnknownPlayer(p));
        }
        self.frame += 1;
        // Step 6, sMain::vf06 (0x00DBFA90): the step. Constant here.
        let time = self.clock.story_time();
        self.world.camera_step(self.clock.seconds());
        // Steps 18-20, sGameSys::vf06 then sAreaExt::vf06: the managers of
        // aGameMain (sQuestManager's QCT), aStage's mode (events) and
        // sScenario, then (step 36, line 0) the story scripts. ddstory runs
        // all of them in one tick; the world's completions since the last
        // frame come first. The story sees every Arisen where the last frame
        // left them: line 10 has not moved yet.
        self.world.observe_fight(self.fight.as_ref());
        self.sync_npc_combat();
        self.observe_npc_combat();
        let events = self.world.events();
        // Step 20, aGameMain's managers: sNpcManager::vf06 (0x00475AD0)
        // restocks the shops once per new day, before sQuestManager.
        // `blocked` is sGameSys +0xB8844 bit 0x80000, not modelled (false).
        if let Some(story) = &self.story {
            self.economy.update(story.world.day, false);
        }
        let mut requests = vec![];
        // The scenario prompt reads sScenario +0x108, which the player's move
        // (line 10, uPlayerBase vf204 0x00B6EAC0) set during the last frame:
        // a press reaches the story one frame after it was made.
        let interact = self.scenario_press.take();
        if let Some(story) = self.story.as_mut() {
            let seats: Vec<PlayerId> = self.roster.ids().collect();
            let players = seats
                .iter()
                .map(|&id| {
                    let p = self.sim.player(id).expect("seated players have a body");
                    // Yaw convention: ddsim's 0 faces +Z, like uCoord mAngle.y
                    // (assumed, not traced).
                    ddstory::Player { pos: p.position, yaw: p.yaw }
                })
                .collect();
            let story_inputs = ddstory::Inputs {
                time,
                players,
                npcs: self.world.npcs(),
                loaded_field_cells: self.scenario_field_cells.clone(),
                interact: interact.and_then(|p| seats.iter().position(|&s| s == p)),
                events,
                ..Default::default()
            };
            requests = story.tick(&story_inputs, &self.world)?;
            // New game (0x00445AB0) and load only name the stage; the host
            // loads it and reports StageLoaded (ddstory's contract).
            if self.boot && story.stage.stage_no == 0 {
                requests.push(Request::JumpArea { stage: story.world.stage_no, pos: story.world.start_pos, fade: 0 });
            }
            self.boot = false;
        }
        self.apply_job_kit();
        self.sync_opening_dragon()?;
        for request in &requests {
            if let Request::StoryPawn { stock, slot } = request {
                self.recruit_story_pawn(*stock, *slot)?;
            }
        }
        self.sync_hired_pawn_bodies();
        // Steps 22-25, input (sQlocInput::vf06 0x00409EE0, sKeyboard,
        // sMouse): this frame's inputs, before anything moves.
        let effective_inputs: FrameInputs = inputs.iter().map(|(&id, &input)| {
            (id, if self.world.player_actions_blocked(id) { Default::default() } else { input })
        }).collect();
        let inputs = &effective_inputs;
        let presses = self.edges.interact_presses(inputs);
        // Step 30 (sAI objects): no headless AI objects yet. Step 33: hits
        // from the volumes the fight switched on last frame, damage applied,
        // before anyone moves (FRAME.md 5.1).
        let step = self.clock.seconds() * ddsim::MOTION_FPS;
        let mut kills = vec![];
        if let Some(f) = self.fight.as_mut() {
            f.hits(step);
            kills = f.kill_awards();
        }
        self.start_npc_deaths()?;
        // A goblin's death gives the party its experience (0x00443470);
        // without progression data (or party records) there is no one to
        // award.
        if self.data.tables.is_some() && self.progression.party.iter().any(Option::is_some) {
            for exp in kills {
                self.award_exp(ddprogress::Award::new(exp))?;
            }
        }
        // Step 36, pass A, line 10 "Player": the Arisen (and later the
        // pawns) move: one Sim::step is one engine frame.
        let mut sim_inputs: std::collections::HashMap<PlayerId, ddsim::Input> =
            inputs.iter().map(|(&id, i)| (id, i.sim)).collect();
        // The fight's pawns move on the same line, driven by the virtual
        // pads of their AI (ddenemyrt::pawn, uCmc through the player
        // pipeline). Their seats are sim players outside the roster.
        if let Some(f) = self.fight.as_mut() {
            for (id, i) in f.pawn_inputs(&self.sim, step) {
                // 0x00B61A20, checks 0x00B62303..0x00B6239B: camera
                // action locks include the Pawn actor domain (flag7).
                // AI/world phases still run; only its generated pad is gated.
                let i = if self.world.player_actions_blocked(id) { Default::default() } else { i };
                sim_inputs.entry(id).or_insert(i);
            }
        }
        self.sim.step(self.clock.seconds(), &sim_inputs, self.ground.as_ref());
        // uNpcIntel line12 precedes the NPC actor/action on line15.
        self.step_npc_death_intels()?;
        self.step_opening_dragon_a(step)?;
        // Line 10 continued (fight): scripted attacks advance and move their
        // Arisen; line 15: the enemies think, act and move; step 38: the
        // push-out between characters; pass C (step 42): poses and the hit
        // volumes for the next frame.
        if let Some(f) = self.fight.as_mut() {
            f.players_moved(&mut self.sim, self.ground.as_ref(), step);
            f.enemies(&self.sim, self.ground.as_ref(), step);
            f.push_out(&mut self.sim);
            f.volumes(&self.sim);
        }
        self.step_opening_dragon_c(step)?;
        // 0x00B768E0 queues orders with 0x00887250; 0x00887360
        // delivers vf98 in pass C. The pawn already chose this frame's
        // virtual pad, so its mailbox affects the next move frame.
        let cooling = self.advance_pawn_order_cooldowns(step);
        let pawn_order_requests = self.dispatch_pawn_orders(inputs, &cooling);
        self.ambient
            .step(step, &self.sim.players, self.ground.as_ref());
        // Same line, phase 90 of each character: at most one level-up
        // (0x00B5BD20) and one rank-up (0x00B5C230) per frame. The original
        // keeps the vocation in sGameSys, so it is mirrored into the story
        // world.
        if let Some(t) = self.data.tables.as_deref() {
            self.progression.step(t);
            if let Some(story) = self.story.as_mut() {
                self.progression.sync_to_world(&mut story.world);
            }
            // the fight carries each Arisen's current level stats
            if let Some(f) = self.fight.as_mut() {
                for m in self.progression.party.iter().flatten() {
                    let ddprogress::Role::Arisen(seat) = m.role else { continue };
                    let c = &self.progression.characters[m.character];
                    let st = &c.progress.stats;
                    f.apply_level(seat, c.progress.level, st.hp_max, st.attack, st.defend, st.magic_attack, st.magic_defend);
                }
            }
        }
        // Pass A line 15, the NPCs: daily schedules and their actions
        // (ddnpcrt on the ddai core).
        self.prepare_story_npcs()?;
        let mut npc_requests = self.step_npcs()?;
        npc_requests.extend(self.step_story_npcs()?);
        self.record_story_npc_requests(&npc_requests);
        self.sync_npc_combat();
        self.step_doors(&presses);
        for request in &npc_requests {
            if let ddnpcrt::schedule::Request::Gsf { no, value } = request {
                if let Some(story) = &mut self.story {
                    story.world.gsf.set(*no as usize, *value);
                }
            }
        }
        // One prompt slot: the lowest seat that pressed takes it.
        self.scenario_press = presses.first().copied();
        // Pass A line 15 (enemies, NPCs), steps 37-42 (push-out, AI senses,
        // hit volumes), 51-53 (cameras, sound, weather): not composed yet.
        // The world performs the story's requests (actors, presentation).
        let stage = self.story.as_ref().map_or(0, |s| s.stage.stage_no);
        for request in &requests {
            if let Request::ApplyScript { target, path } = request {
                self.apply_story_npc(stage, *target, path);
            }
        }
        self.world.handle(stage, &requests);
        for r in &requests {
            self.request_log.debug(r);
        }
        Ok(FrameReport { frame: self.frame, requests, interact, npc_requests, pawn_order_requests })
    }

    /// A hash of the whole simulated state, for replays.
    ///
    /// It covers everything [`Host::saved_hash`] covers, plus what a save
    /// does not keep:
    /// - every field of every body;
    /// - the story's stage, quests and script states;
    /// - open interactions and pending presses;
    /// - the requests emitted so far.
    ///
    /// Equal hashes after the same inputs are the replay property.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.saved_hash()).debug(&self.locks).debug(&self.scenario_press);
        for p in &self.sim.players {
            h.debug(p);
        }
        if let Some(s) = &self.story {
            h.u64(s.frame).debug(&s.stage).debug(&s.script_states());
            for q in &s.quests {
                h.u64(q.id as u64).u64(q.flags as u64).debug(&q.log);
            }
        }
        h.u64(self.story_npcs.lifetime_counter);
        for placement in self.story_npc_intels() {
            h.debug(&(placement.stage, placement.target, placement.npc,
                self.story_npc_position(placement.npc as u32),
                self.story_npc_intel_flags(placement.npc as u32),
                self.story_npc_lifetime(placement.npc as u32)));
        }
        h.debug(&self.rift.room).debug(&self.rift_visitors);
        h.debug(&self.pawn_orders);
        if let Some(f) = &self.fight {
            h.debug(&f.digest());
            for (&seat, pawn) in &f.pawns {
                h.u64(u64::from(seat)).debug(&pawn.brain.orders);
            }
        }
        if let Some(doors) = &self.doors { h.debug(&doors.digest()); }
        h.u64(self.ambient.digest());
        h.u64(self.request_log.finish());
        h.finish()
    }

    /// A hash of exactly what [`Host::save`] keeps:
    /// - the frame;
    /// - the seats and the party;
    /// - each Arisen's place;
    /// - the story world;
    /// - the economy and progression;
    /// - the Rift, except its stock, which a load rebuilds as the original
    ///   does.
    ///
    /// Equal before a save and after its load.
    pub fn saved_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.frame).debug(&self.roster).debug(&self.party).debug(&self.hired_by);
        for p in &self.sim.players {
            h.debug(&(p.id, p.position, p.yaw));
        }
        if let Some(s) = &self.story {
            h.bytes(&s.world.save());
        }
        h.debug(&self.economy).debug(&self.progression);
        h.debug(&self.rift.party).debug(&self.rift.log).debug(&self.rift.cost_multiplier);
        h.debug(&self.saved_npc_quest_rows());
        h.finish()
    }

    /// Whether any interaction is open: an NPC lock, a shop session or the
    /// Rift room.
    pub fn interacting(&self) -> bool {
        self.locks.iter().next().is_some() || self.economy.sessions_open() || self.rift.visit_open()
    }

    /// The session's save (ADR 0002 section 6):
    /// - `HOST`: the frame count, the seats with their profiles, and who
    ///   hired each pawn;
    /// - `WRLD`: the story world;
    /// - `STOR`: native-derived quest/task graph and register state;
    /// - optional `STCX`: versioned project continuation (clocks/scratch),
    ///   beyond the original's state re-entry behavior;
    /// - `PLCE`: each Arisen's place, by profile;
    /// - `ECON`, `PROG`: shared parts, and per-seat parts filed by profile;
    /// - `RIFT`: the shared Rift.
    ///
    /// Refused while an interaction is open, as the original cannot save in
    /// a shop or at a Riftstone.
    pub fn save(&self) -> Result<SaveFile, HostError> {
        if self.interacting() {
            return Err(HostError::Busy);
        }
        let mut f = SaveFile::default();
        let mut w = ddid::Writer::new();
        w.u8(1).u64(self.frame).len(self.roster.len());
        for seat in self.roster.seats() {
            w.u8(seat.id).u64(seat.profile.0);
        }
        for h in self.hired_by {
            w.bool(h.is_some()).u8(h.unwrap_or(0));
        }
        f.put(save::TAG_HOST, Owner::Shared, w.finish());
        if let Some(s) = &self.story {
            f.put(save::TAG_WORLD, Owner::Shared, s.world.save());
            f.put(save::TAG_STORY, Owner::Shared, s.save_scripts().map_err(HostError::Save)?);
            f.put(save::TAG_STORY_CONTINUATION, Owner::Shared, s.save_continuation().map_err(HostError::Save)?);
        }
        for seat in self.roster.seats() {
            if let Some(p) = self.sim.player(seat.id) {
                f.put(save::TAG_PLACE, Owner::Profile(seat.profile), save::place_bytes(p.position, p.yaw));
            }
        }
        let profile = |p: PlayerId| self.roster.seat(p).map(|s| s.profile);
        for (tag, parts) in [(save::TAG_ECON, self.economy.save()), (save::TAG_PROG, self.progression.save())] {
            f.put(tag, Owner::Shared, parts.shared);
            for (seat, bytes) in parts.players {
                if let Some(pr) = profile(seat) {
                    f.put(tag, Owner::Profile(pr), bytes);
                }
            }
        }
        f.put(save::TAG_RIFT, Owner::Shared, self.rift.save().shared);
        self.save_npc_quest_controls(&mut f);
        Ok(f)
    }

    /// Restores what [`Host::save`] wrote into this host.
    ///
    /// The roster must already be seated with the save's profiles, though
    /// seats may differ. Profiles of the save that are not seated now are
    /// left out. Seated profiles without chunks keep what they have. The
    /// Rift's stock starts empty until [`Host::refill_rift`], as on the
    /// original's load.
    pub fn load(&mut self, f: &SaveFile, data: D) -> Result<(), HostError> {
        let err = HostError::Save;
        // Build/validate the complete script candidate before touching this
        // host, including its frame or player places. Old WRLD-only saves
        // still use from_world. A script chunk cannot stand without its world.
        let loaded_story = match f.get(save::TAG_WORLD, Owner::Shared) {
            Some(b) => {
                let world = ddstory::World::load(b).map_err(err)?;
                Some(Story::from_save_with_continuation(data, world, f.get(save::TAG_STORY, Owner::Shared),
                    f.get(save::TAG_STORY_CONTINUATION, Owner::Shared)).map_err(err)?)
            }
            None if f.get(save::TAG_STORY, Owner::Shared).is_some()
                || f.get(save::TAG_STORY_CONTINUATION, Owner::Shared).is_some() =>
                return Err(err("STOR script chunk without WRLD".into())),
            None => None,
        };
        // Old seat -> profile, from HOST; then -> the seat it has now.
        let mut old: Vec<(PlayerId, ProfileId)> = vec![];
        let mut hired_old = [None; 2];
        if let Some(b) = f.get(save::TAG_HOST, Owner::Shared) {
            let mut r = ddid::Reader::new(b);
            r.version("HOST", 1).map_err(err)?;
            self.frame = r.u64().map_err(err)?;
            for _ in 0..r.len(9).map_err(err)? {
                old.push((r.u8().map_err(err)?, ProfileId(r.u64().map_err(err)?)));
            }
            for h in &mut hired_old {
                let (some, seat) = (r.bool().map_err(err)?, r.u8().map_err(err)?);
                *h = some.then_some(seat);
            }
            r.end().map_err(err)?;
        }
        let roster = self.roster.clone();
        let seat_now = |s: PlayerId| -> Option<PlayerId> {
            let p = old.iter().find(|(o, _)| *o == s)?.1;
            roster.seats().iter().find(|x| x.profile == p).map(|x| x.id)
        };
        if let Some(story) = loaded_story {
            self.story = Some(story);
            self.boot = true;
        }
        for seat in self.roster.seats() {
            if let Some((pos, yaw)) = f.get(save::TAG_PLACE, Owner::Profile(seat.profile)).and_then(save::place_from) {
                if let Some(p) = self.sim.player_mut(seat.id) {
                    p.position = pos;
                    p.yaw = yaw;
                    p.drop_from = pos[1];
                }
            }
        }
        let parts = |tag: [u8; 4]| -> Option<ddid::Parts> {
            let shared = f.get(tag, Owner::Shared)?.to_vec();
            let players = roster
                .seats()
                .iter()
                .filter_map(|s| Some((s.id, f.get(tag, Owner::Profile(s.profile))?.to_vec())))
                .collect();
            Some(ddid::Parts { shared, players })
        };
        if let Some(p) = parts(save::TAG_ECON) {
            self.economy = ddeconomy::Economy::load(&p).map_err(err)?;
        }
        if let Some(p) = parts(save::TAG_PROG) {
            self.progression = ddprogress::Progression::load(&p, &seat_now).map_err(err)?;
        }
        if let Some(p) = parts(save::TAG_RIFT) {
            self.rift = ddrift::Rift::load(&p).map_err(err)?;
            self.rift.party.reserved = [false, self.roster.len() >= 2];
        }
        self.hired_by = hired_old.map(|h| h.map(|s| seat_now(s).unwrap_or(0)));
        for s in self.roster.ids().collect::<Vec<_>>() {
            self.economy.wallet(s);
        }
        self.load_npc_quest_controls(f)?;
        self.refresh_party();
        Ok(())
    }
}

