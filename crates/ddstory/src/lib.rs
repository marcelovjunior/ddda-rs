//! Headless story runtime for Dragon's Dogma: Dark Arisen.
//!
//! Holds the story state ([`World`]) and runs what drives it in the
//! original: cFSMOrder quest and stage scripts, quest control tables
//! (QCT) and stage scenario trigger volumes. It never renders or plays
//! anything: it emits [`Request`]s to the host and reads what the host
//! reports ([`Inputs`], [`Host`]). The host (the co-op server) owns one
//! [`Story`] for the session; positions of every player feed the
//! triggers. See docs/QUESTS.md, section "Runtime".
//!
//! Orders the opening does not use return [`Error::Unimplemented`]; they
//! are never skipped silently.

pub mod cond;
pub mod data;
pub mod event;
pub mod exe;
pub mod look;
pub mod orders;
pub mod qct;
pub mod route;
pub mod scenario;
pub mod script;
pub mod scripted;
pub mod story;
pub mod vars;
pub mod world;

pub use story::Story;
pub use world::World;

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// An order, judgment, result, variable or data form this runtime does
    /// not implement. Never ignored.
    Unimplemented(String),
    /// Bad or missing game data.
    Data(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unimplemented(s) => write!(f, "unimplemented: {s}"),
            Error::Data(s) => write!(f, "data: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Frame timing as sMain reports it: `delta` is mDeltaTime (frames at
/// `fps`), `fps` is mFps. Script timers add `max(delta, 0.01) / fps`
/// seconds per frame (0x005bac30).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Time {
    pub delta: f32,
    pub fps: f32,
}

impl Default for Time {
    fn default() -> Self {
        Time { delta: 1.0, fps: 30.0 }
    }
}

/// A unit as the trigger volumes see it: world position and yaw
/// (uCoord mAngle.y, radians).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Player {
    pub pos: [f32; 3],
    pub yaw: f32,
}

/// An NPC unit with its id (npc +0x5858).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NpcUnit {
    pub id: u32,
    pub unit: Player,
}

/// What the host reports for one frame.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub time: Time,
    /// Every Arisen of the session (host first). Trigger volumes are
    /// tested against all of them.
    pub players: Vec<Player>,
    /// The host's pawns (party members 1..3).
    pub pawns: Vec<Player>,
    /// NPC and enemy units on the stage (for volumes that detect them).
    pub npcs: Vec<NpcUnit>,
    pub enemies: Vec<Player>,
    /// The player (index into `players`) who pressed interact this frame
    /// while a scenario prompt is shown (uPlayerBase vf204 0x00b6eac0 sets
    /// sScenario +0x108).
    pub interact: Option<usize>,
    /// Things that finished or happened since the last frame.
    pub events: Vec<HostEvent>,
}

/// Completions and facts the host reports back.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// The cutscene requested by [`Request::PlayEvent`] has ended.
    EventFinished,
    /// A camera requested by [`Request::Camera`] has ended.
    CameraFinished,
    /// The message requested by [`Request::Message`] / tutorial is gone.
    MessageFinished,
    /// The vocation menu opened by [`Request::OpenMenu`] was closed.
    MenuClosed(Menu),
    /// The party arrived on `stage` (after a [`Request::JumpArea`] or a
    /// new game / load).
    StageLoaded { stage: u32 },
    /// Screen fully black / not.
    FadeBlack(bool),
    /// A host-run script (NPC think script, object script) set or cleared
    /// a story flag, e.g. the NPC script q0002_n001_ai of stage 200
    /// ("Set GSFFlag" 381).
    SetGsf { no: u32, value: bool },
    /// A host-run script set or cleared a quest bit, e.g. the dragon's AI
    /// script dragon_op of stage 200 (cThinkFSMParamQuestFlag q0 bits 20, 21).
    SetQuestBit { quest: u32, bit: u32, value: bool },
    /// The Arisen's vocation (sGameSys +0xa76e0, JobPl 0x005bb010).
    PlayerJob(u32),
    /// The sGameSys flags the script gate reads (0x0044d430).
    GameFlags(story::GameFlags),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// uGUIJobChange (stage mode 0x19, 0x00512fb0).
    Vocation,
}

/// A placement target of an order list (cLinkUnit::cTarget, props
/// 0x00764290): kind `mType` (1 party member, 2 enemy, 3/4 scripted object
/// or item, 5 NPC), `mNo0` (layout group) and `mNo1` (index in it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pub kind: u32,
    pub group: u32,
    pub index: u32,
}

/// An NPC's intel entry (sNpcIntelManager 0x018fa4dc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcIntel {
    /// NPC id (intel +0x3e4).
    pub id: u32,
    /// Hit points above zero (intel +0x3d8 > 0).
    pub hp_positive: bool,
}

/// Facts about units the host owns (party, NPCs, enemies, objects) and
/// options. Only orders that need them ask.
pub trait Host {
    /// The placement's layout set is loaded (0x005cc210).
    fn layout_loaded(&self, stage: u32, t: &Target) -> bool;
    /// The placement resolves to a unit (0x007643d0).
    fn unit_exists(&self, stage: u32, t: &Target) -> bool;
    /// A whole layout group of kind 2 (enemy), 4 (item) or 5 (NPC) is
    /// loaded (0x005b75d0 / 0x005b7600 / 0x005b7630).
    fn group_loaded(&self, stage: u32, kind: u32, group: u32) -> bool;
    /// Enemy group done (sSetManager 0x004a52e0) / active (0x004a52b0).
    fn enemy_group_done(&self, stage: u32, group: u32) -> bool;
    fn enemy_group_active(&self, stage: u32, group: u32) -> bool;
    /// EnemyCheck's per-placement test 0x005bf3d0 with `flag` 0 exists, 1
    /// defeated, 2 alive, 3 dead (0x1000 is always true and not asked).
    fn enemy_check(&self, stage: u32, t: &Target, flag: u32) -> bool;
    /// The NPC intel behind a placement, if any.
    fn npc_intel(&self, stage: u32, t: &Target) -> Option<NpcIntel>;
    /// An active NPC unit with this id exists (IsNpcUnit, 0x00476400).
    fn npc_unit(&self, npc: u32) -> bool;
    /// The script given by ApplyFSM to this target has finished (or the
    /// target is gone; 0x005c8460).
    fn applied_script_finished(&self, stage: u32, t: &Target) -> bool;
    /// HP of a target (CheckHp 0x005c9250): absolute, or percent of max.
    fn unit_hp(&self, stage: u32, t: &Target, percent: bool) -> Option<f32>;
    /// Whether tutorials are on (option word 0x018fa4f8 +0x20e774 & 0x800,
    /// read by 0x005c67c0).
    fn tutorials_enabled(&self) -> bool;
    /// PlCheck's count for kind `kind` on the Arisen (0x005c1880).
    fn pl_check(&self, kind: u32) -> i32;
    /// The Arisen took damage this frame (IsDamagePl 0x005bb2d0).
    fn player_damaged(&self) -> bool;
    /// A pawn is lost (IsMyCmcLost 0x005bb580).
    fn pawn_lost(&self) -> bool;
    /// The party holds at least `count` of item `item` (sItemManager
    /// 0x00460350, QCT judgment 16).
    fn has_item(&self, item: u32, count: u32) -> bool;
}

/// What the story asks the host to do. Each variant names the order or
/// routine that produces it.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Play cutscene `row` of the exe's event table (stage, event); see
    /// EVENTS.md. Answer with [`HostEvent::EventFinished`].
    PlayEvent { row: usize, stage: u32, event: u32 },
    /// Place the party after an event / jump: start position `pos` of the
    /// current stage, or an explicit position.
    PlaceParty { start_pos: Option<u32>, pos: Option<[f32; 3]> },
    /// Load `stage` at start position `pos` (JumpArea 0x005bc700 ->
    /// 0x00517ca0). Answer with [`HostEvent::StageLoaded`].
    JumpArea { stage: u32, pos: u32, fade: u32 },
    /// Open a menu for a player. Answer with [`HostEvent::MenuClosed`].
    OpenMenu { menu: Menu, player: usize },
    /// Show a quest/stage message.
    Message { kind: u8, quest: u16, mes_id: u16, disp_sec: f32 },
    /// Quest banner (sIdCockpit 0x004525d0): kind 1 start, 2 objective,
    /// 3 report, 4 fail, 6 clear.
    QuestBanner { kind: u32, quest: u32, log: u32 },
    /// Give the rewards of a cleared quest (rQuestReward, 0x00790539).
    Reward { quest: u32, gold: i32, exp: i32, rift: i32, items: Vec<(i32, i32)> },
    /// Set or clear a quest marker (QCT results 30/20, 0x00485f90).
    Marker { quest: u32, set: bool, raw: [u32; 11] },
    /// Set the game clock (TimeJump 0x005c2890).
    SetClock { hour: u32, minute: u32 },
    /// A layout flag changed (SetLayout / QCT 6/7): the host loads or
    /// unloads the gated placement groups (sSetManager 0x004a95c0).
    Layout { stage: u32, flag: u32, on: bool },
    /// Place-name banner (0x00454760).
    PlaceName { mes_id: u32 },
    /// Weather kind (sWeatherManager 0x004c3520 / 0x004c3830 for 4).
    Weather { kind: u32 },
    /// Tutorial: a message window (`window`) or a panel of up to three
    /// ids (0x005183a0). Answer with [`HostEvent::MessageFinished`].
    Tutorial { window: bool, ids: [u32; 4], disp_sec: f32 },
    /// Make placements present (act 1/2) or absent (act 3/4) (SetObj
    /// 0x005bd6c0); idempotent, re-sent once per state entry.
    Placement { target: Target, present: bool, act: u32 },
    /// Enable / disable a layout group (SetGroup 0x005bdec0).
    Group { kind: u32, group: u32, on: bool },
    /// Revive (clear both death bits) or kill an NPC (0x00446540 /
    /// 0x00446460).
    NpcLife { npc: u32, alive: bool },
    /// Give an AI script to a unit (ApplyFSM 0x005c8460).
    ApplyScript { target: Target, path: String },
    /// Stage collision part on/off (Collision 0x005c8140).
    Collision { kind: u32, split: (u32, u32), part: u32, on: bool },
    /// A scripted camera (Camera order 0x005c3750): `simple` events play as
    /// a stage mode. Answer with [`HostEvent::CameraFinished`].
    Camera { cam_id: u32, simple: bool, quest: i32 },
    /// End a simple-event camera and place the party (0x005c4790).
    CameraEnd { set_pos_type: u32, start_pos: u32 },
    /// A scenario action the story does not interpret (ladder, scenario
    /// camera, stage message, pawn control).
    Scenario { task: u32, name: String, arg: Option<mtfsm::Record> },
    /// Any presentation order the story does not interpret itself (sound,
    /// music, camera shake, effects, spawning): its container name and
    /// parameter record, for the host to perform.
    Order { script: String, container: String, parameter: Option<mtfsm::Record> },
}

/// Everything an order or a QCT row can touch during one frame.
pub struct Env<'a> {
    pub world: &'a mut World,
    pub stage: &'a mut story::StageState,
    pub host: &'a dyn Host,
    pub time: Time,
    pub out: &'a mut Vec<Request>,
    pub cmds: &'a mut Vec<story::Command>,
    pub events: Option<&'a ddquest::events::EventTable>,
}
