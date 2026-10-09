//! The host: one session's world, players and systems, stepped in the
//! original's frame order (docs/FRAME.md, docs/adr/0002-game-host.md).

use crate::clock::FrameClock;
use crate::hash::StateHasher;
use crate::input::{Edges, FrameInputs};
use crate::lock::InteractionLocks;
use crate::player::{Control, Party, PlayerId, ProfileId, Roster};
use crate::save::{self, Owner, SaveFile};
use ddsim::{Ground, Sim};
use ddstory::data::Data;
use ddstory::{HostEvent, NpcUnit, Request, Story};

/// Everything the story asks of the rest of the game: the actors (NPCs,
/// enemies, objects) and the presentation (cutscenes, cameras, messages,
/// menus). Until those systems exist in headless crates, a stand-in
/// answers (`ddstory::scripted::ScriptedHost`).
pub trait StoryWorld: ddstory::Host {
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
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Story(e) => write!(f, "story: {e}"),
            HostError::UnknownPlayer(p) => write!(f, "input for unseated player {p}"),
            HostError::Save(e) => write!(f, "save: {e}"),
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
    pub ground: Box<dyn Ground>,
    pub locks: InteractionLocks,
    edges: Edges,
    /// The interact press made last frame, for this frame's scenarios.
    scenario_press: Option<PlayerId>,
    /// The story's stage is not loaded yet: ask the world to load it.
    boot: bool,
    /// Running hash of every request emitted, for replay checks.
    request_log: StateHasher,
}

impl<D: Data, W: StoryWorld> Host<D, W> {
    pub fn new(sim: Sim, story: Option<Story<D>>, world: W, ground: Box<dyn Ground>) -> Self {
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
            edges: Edges::default(),
            scenario_press: None,
            boot: true,
            request_log: StateHasher::default(),
        }
    }

    /// Seats an Arisen and places it in the world. `None` when the roster
    /// is full or the profile is already seated.
    pub fn join(&mut self, profile: ProfileId, control: Control, pos: [f32; 3], yaw: f32) -> Option<PlayerId> {
        let id = self.roster.join(profile, control)?;
        self.sim.add_player(id, pos, yaw);
        self.party = Party::for_roster(&self.roster);
        Some(id)
    }

    /// Unseats an Arisen: its body leaves the world and its locks go.
    pub fn leave(&mut self, id: PlayerId) {
        if self.roster.leave(id).is_some() {
            self.sim.players.retain(|p| p.id != id);
            self.locks.release_all(id);
            self.party = Party::for_roster(&self.roster);
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
        // Steps 18-20, sGameSys::vf06 then sAreaExt::vf06: the managers of
        // aGameMain (sQuestManager's QCT), aStage's mode (events) and
        // sScenario, then (step 36, line 0) the story scripts. ddstory runs
        // all of them in one tick; the world's completions since the last
        // frame come first. The story sees every Arisen where the last frame
        // left them: line 10 has not moved yet.
        let events = self.world.events();
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
        // Steps 22-25, input (sQlocInput::vf06 0x00409EE0, sKeyboard,
        // sMouse): this frame's inputs, before anything moves.
        let presses = self.edges.interact_presses(inputs);
        // Steps 30 (sAI), 33 (hits from the last frame's volumes, damage):
        // no headless AI or hit pass yet.
        // Step 36, pass A, line 10 "Player": the Arisen (and later the
        // pawns) move: one Sim::step is one engine frame.
        let sim_inputs = inputs.iter().map(|(&id, i)| (id, i.sim)).collect();
        self.sim.step(self.clock.seconds(), &sim_inputs, self.ground.as_ref());
        // One prompt slot: the lowest seat that pressed takes it.
        self.scenario_press = presses.first().copied();
        // Pass A line 15 (enemies, NPCs), steps 37-42 (push-out, AI senses,
        // hit volumes), 51-53 (cameras, sound, weather): not composed yet.
        // The world performs the story's requests (actors, presentation).
        let stage = self.story.as_ref().map_or(0, |s| s.stage.stage_no);
        self.world.handle(stage, &requests);
        for r in &requests {
            self.request_log.debug(r);
        }
        Ok(FrameReport { frame: self.frame, requests, interact })
    }

    /// A hash of the simulated state: frame, seats, every Arisen (all
    /// fields), the story world, quests, scripts and the requests emitted
    /// so far. Equal hashes after the same inputs are the replay property.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::default();
        h.u64(self.frame).debug(&self.roster).debug(&self.party).debug(&self.locks).debug(&self.scenario_press);
        for p in &self.sim.players {
            h.debug(p);
        }
        if let Some(s) = &self.story {
            h.u64(s.frame).bytes(&s.world.save()).debug(&s.stage);
            for q in &s.quests {
                h.u64(q.id as u64).u64(q.flags as u64).debug(&q.log);
                if let Some(sc) = &q.script {
                    h.bytes(sc.path.as_bytes()).u64(sc.running as u64);
                }
            }
            for t in &s.tasks {
                h.bytes(t.path.as_bytes()).u64(t.running as u64);
            }
        }
        h.u64(self.request_log.finish());
        h.finish()
    }

    /// The session's save: the story world (shared) and each seated
    /// Arisen's place (by profile).
    pub fn save(&self) -> SaveFile {
        let mut f = SaveFile::default();
        if let Some(s) = &self.story {
            f.put(save::TAG_WORLD, Owner::Shared, s.world.save());
        }
        for seat in self.roster.seats() {
            if let Some(p) = self.sim.player(seat.id) {
                f.put(save::TAG_PLACE, Owner::Profile(seat.profile), save::place_bytes(p.position, p.yaw));
            }
        }
        f
    }

    /// Restores what [`Host::save`] wrote: the story world, and the place
    /// of every seated Arisen whose profile has one.
    pub fn load(&mut self, f: &SaveFile, data: D) -> Result<(), HostError> {
        if let Some(b) = f.get(save::TAG_WORLD, Owner::Shared) {
            let world = ddstory::World::load(b).map_err(HostError::Save)?;
            self.story = Some(Story::from_world(data, world));
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
        Ok(())
    }
}
