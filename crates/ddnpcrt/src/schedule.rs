//! Daily schedules and where an NPC stands.
//!
//! Each scheduled NPC runs two graphs on the shared AI core
//! (`ddai::core::Core`, 0x00e092a0):
//! - the daily controller `ai/fsm/npc/stSSS/stSSS_nNNN[_00|_01]`
//!   (`cAIConsiderFSM_24Schedule`; path rule 0x00bbdaa0, `ddnpc::schedule_path`),
//!   whose states switch on `GlobalTime` (`GameHour * 6 + GameMinute / 10`,
//!   0x0059d790) and load an action graph (`LoadFSM`);
//! - that action graph (`cAIActionFSMNpc` / `cAIActionFSMDemo`), whose
//!   states set an *interface*: go to a point, wait, play a motion. A state
//!   moves on when its interface is done: process `SetInterface`'s state
//!   callback (0x00533a90) sets `SysCondition` bit 2 when the interface
//!   reports done, and the links test `SysCondition & 4` (operator 10).
//!
//! Locomotion, motions and turning belong to the host's actor layer: the
//! runtime emits [`Request`]s and the host reports [`Done`] events. Until a
//! headless NPC mover exists, a host may complete moves at once
//! ([`Npc::complete_moves`]); positions are then the route points.

use ddai::cond::{Value, Var};
use ddai::core::{Core, Owner, ProcAt, ProcMem};
use ddai::fields::Fields;
use ddai::Error;
use mtfsm::{Graph, Process};
use std::sync::Arc;

/// Graphs by path (`AI\FSM\Npc\...`, case-insensitive).
pub trait Graphs {
    fn graph(&self, path: &str) -> Result<Arc<Graph>, Error>;
}

/// Runtime guard configuration, reflected by 0x00578ad0 and copied into
/// NPC intel by 0x00578ee0 -> 0x00bbf580 (DDDA.exe 2017-12-13 Steam).
#[derive(Debug, Clone, PartialEq)]
pub struct Guard {
    pub kind: u32,
    pub center: [f32; 3],
    pub radius: f32,
    pub direction: f32,
    pub range: f32,
    pub height: f32,
    pub message_kind: u32,
    pub message: i32,
    pub quest: u32,
    pub guard: bool,
    pub jump_kind: u32,
    pub start_position: u32,
}

/// Reflected NPC wrapper and generic motion parameters, copied by
/// 0x00576ef0 / 0x0057e290 (DDDA.exe 2017-12-13 Steam).
#[derive(Debug, Clone, PartialEq)]
pub struct Motion {
    pub number: i32,
    pub blend_frames: f32,
    pub start_frame: f32,
    pub speed: f32,
    pub looping: bool,
    pub fix_angle: bool,
    pub fix_translation: bool,
    pub translation_off: bool,
    pub symmetry: bool,
    pub target: bool,
    pub target_type: u32,
    pub target_position: [f32; 3],
    pub target_group: u32,
    pub target_id: u32,
    pub arrive_length: f32,
    pub rotation_speed: f32,
    pub look_start_frame: f32,
    pub look_time: f32,
    pub stop: bool,
    pub fall: bool,
    pub force: bool,
    pub talk: bool,
    pub wait_in_intel: bool,
    pub object_priority: u32,
}

impl Motion {
    pub fn from_record(wrapper: &mtfsm::Record) -> Result<Self, Error> {
        Self::read(wrapper, false)
    }

    /// DemoSetMotion delegates the same generic motion but requires the
    /// materialized body in0x0056F160, without an intel-wait property.
    // Translated from0x0056F160/0x0056F210 (DDDA.exe2017-12-13 Steam).
    pub fn from_demo_record(wrapper: &mtfsm::Record) -> Result<Self, Error> {
        if wrapper.class_name != Some("cAIActionInterfaceDemoSetMotion") {
            return Err(Error::Data("expected DemoSetMotion wrapper".into()));
        }
        Self::read(wrapper, true)
    }

    fn read(wrapper: &mtfsm::Record, demo: bool) -> Result<Self, Error> {
        let motion = wrapper.object("mpMotion")
            .filter(|r| r.class_name == Some("cAIActionInterfaceMotion"))
            .ok_or_else(|| Error::Data("NPC motion missing generic mpMotion".into()))?;
        let missing = |name| Error::Data(format!("NPC motion missing {name}"));
        let float = |name| motion.f(name).filter(|f| f.is_finite()).ok_or_else(|| missing(name));
        let boolean = |name| motion.b(name).ok_or_else(|| missing(name));
        let uint = |name| motion.u(name).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| missing(name));
        Ok(Self {
            number: motion.s("Motion No").and_then(|n| i32::try_from(n).ok()).ok_or_else(|| missing("Motion No"))?,
            blend_frames: float("Hokan Frame")?, start_frame: float("Start Frame")?, speed: float("Motion Speed")?,
            looping: boolean("Loop")?, fix_angle: boolean("Null Angle Fix")?,
            fix_translation: boolean("Null Tranx Fix")?, translation_off: boolean("Null Tranx Off")?,
            symmetry: boolean("Symmetry")?, target: boolean("Target Flag")?,
            target_type: uint("Target Type")?, target_position: motion.vec3("Target Position")
                .filter(|p| p.iter().all(|f| f.is_finite())).ok_or_else(|| missing("Target Position"))?,
            target_group: uint("Target Group")?, target_id: uint("Target ID")?,
            arrive_length: float("Arrive Length")?, rotation_speed: float("Rot Speed")?,
            look_start_frame: float("Look Start Frame")?, look_time: float("Look Time")?,
            stop: boolean("Stop")?, fall: boolean("Fall")?, force: boolean("Force")?,
            talk: wrapper.b("会話可能フラグ").ok_or_else(|| missing("talk"))?,
            wait_in_intel: if demo { false } else {
                wrapper.b("Intel状態で再生待ちをする").ok_or_else(|| missing("wait in intel"))?
            },
            object_priority: wrapper.u("ＯＢＪ補正優先").and_then(|n| u32::try_from(n).ok()).ok_or_else(|| missing("object priority"))?,
        })
    }

    /// Translated from 0x0057db10, passed to 0x008500a0.
    pub fn attributes(&self) -> u32 {
        u32::from(self.translation_off) | if self.looping { 0 } else { 4 }
            | if self.speed == 0.0 { 0x10 } else { 0 }
            | if self.fix_translation { 0x20 } else { 0 }
            | if self.fix_angle { 0x80 } else { 0 }
            | if self.symmetry { 0x100 } else { 0 }
    }

    /// Translated from 0x00576e10: NPC body+0x2e98.
    pub fn ground_mode(&self) -> u32 {
        match self.object_priority { 0 => 0, 3 => 14, _ => 13 }
    }
}

/// Translated actor-flag callbacks (DDDA.exe 2017-12-13 Steam; addresses
/// beside each mask). The body adapter must also implement the
/// FixedPosition(false) ground-mode side effect described below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorFlag { Invincible, SuperArmor, NoDamage, NoMotionTalk, NoSurprise, LiftDisabled, NoLiftReaction, NoEscape, LiftEnabled, FixedPosition }

impl ActorFlag {
    pub fn mask(self) -> u32 {
        match self {
            Self::Invincible => 0x1000_0000, // 0x007aa040
            Self::SuperArmor => 0x0200_0000, // 0x007aa1f0
            Self::NoDamage => 0x0800_0000, // 0x007aa0d0
            Self::NoMotionTalk => 0x0100_0000, // 0x007aa280
            Self::NoSurprise => 0x1_0000, // 0x007aa3a0
            Self::LiftDisabled => 0x800, // 0x007aa430
            Self::NoLiftReaction => 0x1000, // 0x007aa4c0
            Self::NoEscape => 0x2000, // 0x007aa5e0
            Self::LiftEnabled => 0x4_0000, // 0x007ab460
            // 0x007aa790: clearing on a body additionally changes body+0x1df0
            // from zero to13; the intel-only branch has no such side effect.
            Self::FixedPosition => 0x200,
        }
    }
    fn from_process(name: &str) -> Option<Self> {
        Some(match name {
            "Set Invincible" => Self::Invincible,
            "Set Super Armor" => Self::SuperArmor,
            "Set No Damage" => Self::NoDamage,
            "Set No Motion Talk" => Self::NoMotionTalk,
            "Set No Surprise" => Self::NoSurprise,
            "Set Lift Disable" => Self::LiftDisabled,
            "Set No Lift Reaction" => Self::NoLiftReaction,
            "Set No Escape" => Self::NoEscape,
            "Set Lift Enable" => Self::LiftEnabled,
            "Set Fix Pos" => Self::FixedPosition,
            _ => return None,
        })
    }
}

/// What an NPC asks of the actor layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// 0x00534bc0 updates intel+0x547; a linked actor also creates/removes
    /// uGUINpcGauge (0x00bb8c10/0x00bb8ca0). UI allocation is presentation.
    Gauge { npc: u32, visible: bool },
    /// Typed boolean callback; the host intel path updates this exact bit.
    ActorFlag { npc: u32, flag: ActorFlag, value: bool },
    /// OR mapped authored disable bits into this materialized NPC owner.
    DisableFlags { npc: u32, mask: u32 },
    /// Configure a sensor; the actor/talk system decides eligibility.
    Guard {
        npc: u32,
        config: Guard,
    },
    Gsf {
        no: u32,
        value: bool,
    },
    /// Applied graph stopped, not the distinct parent ApplyFSM predicate.
    Finished {
        npc: u32,
    },
    /// `cAIActionInterfaceNpcSetGoto`: walk (or run) to a point, using the
    /// navigation route when `route` is set.
    Goto {
        npc: u32,
        pos: [f32; 3],
        run: bool,
        route: bool,
    },
    /// `cAIActionInterfaceNpcSetWait`: stand, facing `dir` when given.
    Wait {
        npc: u32,
        seconds: Option<f32>,
        dir: Option<[f32; 3]>,
    },
    /// `cAIActionInterfaceNpcSetMotion` / `SetActionEx`: play the motion the
    /// record names. ActionEx remains a distinct, unported payload.
    Motion {
        npc: u32,
        interface: String,
        command: u64,
        motion: Option<Motion>,
    },
    /// Placed at once (`SetInitPosNpc`, `cAIActionInterfaceDemoSetPos`).
    Place {
        npc: u32,
        pos: [f32; 3],
        dir: Option<[f32; 3]>,
    },
    /// The action graph changed (`LoadFSM` of the daily controller).
    Action {
        npc: u32,
        path: String,
    },
}

/// Translated from 0x007aa840 (DDDA.exe 2017-12-13 Steam).
/// Twelve authored bits map to vf180 flags, or intel+0x5c0 without a body.
/// Higher authored bits are ignored by the original bounded switch.
pub fn disable_flag_mask(authored: u32) -> u32 {
    const BITS: [u32; 12] = [0x1000_0000, 0x0800_0000, 0x0400_0000,
        0x0200_0000, 0x0001_0000, 0x800, 0x1000, 0x2000, 0x200, 0x100, 0x40, 0x10];
    BITS.iter().enumerate().filter(|(bit, _)| authored & (1 << bit) != 0)
        .fold(0, |mask, (_, value)| mask | value)
}

/// The actor layer finished the NPC's current interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    pub npc: u32,
}

/// The clock and frame the graphs read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    pub hour: u32,
    pub minute: u32,
    /// Seconds this frame.
    pub dt: f32,
}

#[derive(Debug, Clone, PartialEq)]
enum Goal {
    None,
    Goto([f32; 3]),
    Wait(Option<f32>),
    Motion,
    /// Sensors and other interfaces that never report done here.
    Open,
}

/// Real owner observations. None means no matching materialized actor;
/// intel-only controllers have no character flags or held-goods pointers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorObservations {
    /// First flags word returned by a valid character's vf180.
    pub character_flags: Option<u32>,
    /// Presence of NPC held-goods pointers +0x6f44 and +0x6f48.
    pub npc_goods: Option<[bool; 2]>,
}

/// The `cThinkFSM` fields the NPC graphs read and write.
#[derive(Debug, Clone, PartialEq)]
struct Think {
    npc: u32,
    /// `SysCondition` (+0x68).
    sys: u32,
    /// `FreeCondition` (+0x70); `Set FreeCondition` sets bit `FlagNo`
    /// (`1 << n`: the data tests bit 31 with 0x80000000).
    free: u32,
    /// cThinkFSM +0xbc..0xc8, initialized by 0x007a74f0.
    flags: [u32; 4],
    actor: ActorObservations,
    scene: Option<u32>,
    goal: Goal,
    command: u64,
    motion_active: bool,
    pos: Option<[f32; 3]>,
    load: Option<String>,
    out: Vec<Request>,
    complete_moves: bool,
    strict: bool,
    talk_end: bool,
}

struct Env<'a> {
    clock: Clock,
    world: Option<&'a ddstory::World>,
}

fn rec(p: &Process) -> Result<&mtfsm::Record, Error> {
    p.parameter
        .as_ref()
        .ok_or_else(|| Error::Data("process without parameter".into()))
}

fn name(p: &Process) -> String {
    String::from_utf8_lossy(&p.container).into_owned()
}

impl Think {
    // Translated from 0x007aadd0/0x007aae20 (DDDA.exe 2017-12-13 Steam).
    // Enter clears the destination FreeFlag; polling refreshes it, never latches it.
    fn check_quest_flag(&mut self, quest: u64, bit: u64, flag: u64,
        action: Option<u64>, world: Option<&ddstory::World>) -> Result<(), Error> {
        if quest >= 128 || bit >= 96 || flag >= 128 { return Ok(()); }
        let value = match action {
            None => false,
            Some(mode @ 0..=1) => {
                let world = world.ok_or_else(|| Error::Unimplemented("NPC Check QuestFlag without world observation".into()))?;
                world.quest_bit(quest as usize, bit as usize) == (mode == 0)
            }
            Some(_) => return Ok(()),
        };
        let word = &mut self.flags[flag as usize / 32];
        let mask = 1 << (flag & 31);
        if value { *word |= mask; } else { *word &= !mask; }
        Ok(())
    }
    fn quest_flag_process(&mut self, p: &Process, polling: bool, world: Option<&ddstory::World>) -> Result<(), Error> {
        let r = rec(p)?;
        let field = |name| r.u(name).ok_or_else(|| Error::Data(format!("Check QuestFlag missing {name}")));
        self.check_quest_flag(field("QuestNo")?, field("BitNo")?, field("FlagNo")?,
            if polling { Some(field("ActType")?) } else { None }, world)
    }
    fn finish(&mut self) {
        self.sys |= 4;
    }

    fn interface(&mut self, r: &mtfsm::Record) -> Result<(), Error> {
        let i = r
            .object("Interface")
            .ok_or_else(|| Error::Data("interface missing".into()))?;
        let npc = self.npc;
        // Port protocol identity, not a game field. Every replacement
        // interface invalidates an asynchronous completion of its predecessor.
        self.command = self.command.checked_add(1)
            .ok_or_else(|| Error::Data("NPC interface command exhausted".into()))?;
        self.motion_active = false;
        match i.class_name.unwrap_or("") {
            "cAIActionInterfaceNpcSetGoto" => {
                let pos = i
                    .object("mpPosition")
                    .and_then(|p| p.vec3("ポジション"))
                    .ok_or_else(|| Error::Data("goto without position".into()))?;
                let run = i.b("走り").unwrap_or(false);
                let route = i.object("mpTrace").and_then(|t| t.b("経路使用")).unwrap_or(false);
                self.out.push(Request::Goto { npc, pos, run, route });
                self.goal = Goal::Goto(pos);
            }
            "cAIActionInterfaceNpcSetWait" => {
                let t = i.object("mpWaitTimer");
                let infinite = t.and_then(|t| t.b("無限待機")).unwrap_or(false);
                let seconds = if infinite {
                    None
                } else {
                    t.and_then(|t| t.f("待機時間"))
                };
                let dir = i
                    .object("mpDirection")
                    .filter(|d| d.b("方向指定").unwrap_or(false))
                    .and_then(|d| d.vec3("方向"));
                self.out.push(Request::Wait { npc, seconds, dir });
                self.goal = Goal::Wait(seconds);
            }
            c @ ("cAIActionInterfaceNpcSetMotion" | "cAIActionInterfaceNpcSetActionEx") => {
                self.out.push(Request::Motion {
                    npc,
                    interface: c.to_string(),
                    command: self.command,
                    motion: if c == "cAIActionInterfaceNpcSetMotion" {
                        Some(Motion::from_record(i)?)
                    } else { None },
                });
                self.goal = Goal::Motion;
                self.motion_active = true;
            }
            "cAIActionInterfaceDemoSetPos" => {
                let pos = if i.b("ポジションセットOn/Off").unwrap_or(false) {
                    i.vec3("ポジション")
                } else {
                    None
                };
                let dir = if i.b("方向セットOn/Off").unwrap_or(false) {
                    i.vec3("方向")
                } else {
                    None
                };
                if let Some(p) = pos {
                    self.pos = Some(p);
                    self.out.push(Request::Place { npc, pos: p, dir });
                }
                self.goal = Goal::None;
                self.finish();
            }
            "cAIActionInterfaceNpcSetGuard" => {
                let missing = |field| Error::Data(format!("guard missing {field}"));
                let config = Guard {
                    kind: i.u("タイプ").ok_or_else(|| missing("type"))? as u32,
                    center: i.vec3("中心座標").ok_or_else(|| missing("center"))?,
                    radius: i.f("中心からの距離").ok_or_else(|| missing("radius"))?,
                    direction: i.f("向き").ok_or_else(|| missing("direction"))?,
                    range: i.f("範囲").ok_or_else(|| missing("range"))?,
                    height: i.f("高さ").ok_or_else(|| missing("height"))?,
                    message_kind: i.u("メッセージタイプ").ok_or_else(|| missing("message type"))? as u32,
                    message: i.s("メッセージ番号").ok_or_else(|| missing("message"))? as i32,
                    quest: i.u("クエスト番号").ok_or_else(|| missing("quest"))? as u32,
                    guard: i.b("番兵フラグ").ok_or_else(|| missing("guard flag"))?,
                    jump_kind: i.u("ジャンプタイプ").ok_or_else(|| missing("jump"))? as u32,
                    start_position: i.u("スタート位置").ok_or_else(|| missing("start"))? as u32,
                };
                self.out.push(Request::Guard { npc, config });
                self.goal = Goal::Open;
            }
            other if self.strict => return Err(Error::Unimplemented(format!("NPC interface {other}"))),
            _ => self.goal = Goal::Open,
        }
        Ok(())
    }
}

impl Owner for Think {
    type Env<'e> = Env<'e>;

    /// cThinkFSM vf05 on a state change. Clearing the "interface done" bit
    /// here is **guessed**: without it a finished interface would satisfy
    /// the next state's links at once.
    fn state_changed(&mut self, _env: &mut Env<'_>) {
        // A completion of a state's old interface cannot complete another
        // state's interface. Playback itself may remain visible in the host.
        self.motion_active = false;
        if self.strict {
            // Translated from cThinkFSM::vf05 0x007a9da0.
            self.sys = 0;
        } else {
            self.sys &= !4;
        }
    }

    fn var(&self, env: &Env<'_>, v: &Var) -> Result<Value, Error> {
        Ok(match v.property {
            "SysCondition" => Value::Int(self.sys as i64),
            // cThinkFSM reflection 0x007a82d0 binds getter 0x005baf00,
            // which reads the shared scene number at sGameSys+0x30.
            "SceNo" => Value::Int(self.scene.ok_or_else(|| Error::Data("NPC SceNo observation unavailable".into()))? as i64),
            // 0x007ab890: valid actor's frame flag; absent actor is false.
            "TalkEnd (flag)" => Value::Bool(self.talk_end),
            "FreeCondition" => Value::Int(self.free as i64),
            // Translated from 0x007abda0 (DDDA.exe 2017-12-13 Steam).
            "FreeFlag" => Value::Bool(v.index.is_some_and(|index|
                index < 128 && self.flags[index as usize / 32] & (1 << (index & 31)) != 0)),
            // 0x007ab860: validated character owner, vf180 word bit 7.
            "Hit (flag)" => Value::Bool(self.actor.character_flags.is_some_and(|flags| flags & 0x80 != 0)),
            // 0x007ac320: validated NPC owner, either held-goods pointer.
            "NpcGoodsHaving" => Value::Bool(self.actor.npc_goods.is_some_and(|goods| goods[0] || goods[1])),
            // cThinkFSM getter 0x007ac350: the game hour.
            "GameHour" => Value::Int(env.clock.hour as i64),
            // cAIConsiderFSM_24Schedule +0xe4, set by 0x0059d790.
            "GlobalTime" => {
                Value::Int(ddnpc::schedule_time(env.clock.hour, env.clock.minute).unwrap_or(0) as i64)
            }
            p => return Err(Error::Unimplemented(format!("NPC variable {}.{p}", v.owner))),
        })
    }

    fn update(
        &mut self,
        _env: &mut Env<'_>,
        p: &Process,
        _m: &mut ProcMem,
        _at: ProcAt,
    ) -> Result<(), Error> {
        if let Some(flag) = ActorFlag::from_process(&name(p)) {
            let value = rec(p)?.b("Bool").ok_or_else(|| Error::Data("actor flag missing Bool".into()))?;
            self.out.push(Request::ActorFlag { npc: self.npc, flag, value });
            return Ok(());
        }
        match name(p).as_str() {
            "LoadFSM" => {
                let path = rec(p)?
                    .text("FSMPath")
                    .ok_or_else(|| Error::Data("LoadFSM without path".into()))?;
                self.load = Some(path);
            }
            "SetInitPosNpc" => {
                let pos = rec(p)?
                    .vec3("InitPos")
                    .ok_or_else(|| Error::Data("InitPos".into()))?;
                self.pos = Some(pos);
                self.out.push(Request::Place {
                    npc: self.npc,
                    pos,
                    dir: None,
                });
            }
            "SetInterfaceNpc" | "SetInterfaceDemo" | "SetInterface" => self.interface(rec(p)?)?,
            // Translated from 0x007aad10 (DDDA.exe 2017-12-13 Steam).
            "Set GSFFlag" => {
                let r = rec(p)?;
                if let Some(no) = r.u("FlagNo").filter(|&n| n < 1536) {
                    self.out.push(Request::Gsf {
                        no: no as u32,
                        value: r.b("Bool").unwrap_or(false),
                    });
                }
            }
            "Set Disable Flag" => {
                // 0x007aa840 selects the valid NPC body, otherwise its intel.
                // The host applies this request only to a materialized owner.
                let authored = rec(p)?.u("Flag").ok_or_else(|| Error::Data("Disable Flag missing Flag".into()))?;
                self.out.push(Request::DisableFlags { npc: self.npc, mask: disable_flag_mask(authored as u32) });
            }
            "Check QuestFlag" => self.quest_flag_process(p, false, None)?,
            // Registered as a state callback, not an entry callback.
            "SetGaugeDisp" => {},
            "Finish" => {}
            "Set FreeCondition" => {
                let r = rec(p)?;
                let n = r.u("FlagNo").unwrap_or(0) as u32 & 31;
                if r.b("Bool").unwrap_or(false) {
                    self.free |= 1 << n;
                } else {
                    self.free &= !(1 << n);
                }
            }
            // Presentation and behaviour switches with no effect on where
            // the NPC is (talk motion, surprise, lift, armour, timers,
            // shop hit range, ...): recorded by the data, not modelled.
            other if self.strict => return Err(Error::Unimplemented(format!("NPC process {other}"))),
            _ => {}
        }
        Ok(())
    }

    fn state(&mut self, env: &mut Env<'_>, p: &Process, _m: &mut ProcMem, _at: ProcAt) -> Result<(), Error> {
        // Translated from0x00534bc0, registered by0x005342c0
        // (DDDA.exe 2017-12-13 Steam). Repeated writes are intentional.
        if name(p) == "SetGaugeDisp" {
            let visible = rec(p)?.b("Bool").ok_or_else(|| Error::Data("SetGaugeDisp missing Bool".into()))?;
            self.out.push(Request::Gauge { npc: self.npc, visible });
            return Ok(());
        }
        if name(p) == "Check QuestFlag" {
            return self.quest_flag_process(p, true, env.world);
        }
        // Translated from 0x007497e0: the state callback sets Finish.
        if name(p) == "Finish" {
            self.sys |= 1;
            return Ok(());
        }
        if !matches!(name(p).as_str(), "SetInterfaceNpc" | "SetInterface") {
            return Ok(());
        }
        match self.goal.clone() {
            Goal::Wait(Some(t)) => {
                let left = t - env.clock.dt;
                self.goal = Goal::Wait(Some(left));
                if left <= 0.0 {
                    self.finish();
                }
            }
            Goal::Goto(pos) if self.complete_moves => {
                self.pos = Some(pos);
                self.finish();
            }
            Goal::Motion if self.complete_moves => self.finish(),
            _ => {}
        }
        Ok(())
    }
}

/// A core for an NPC graph. NPC graphs carry `mFSMAttribute` bit 2
/// (per-cluster work memory, 0x00e09750 -> 0x00e06e00 / 0x00e087e0) and
/// have sub-machines, which `ddai` refuses until that memory is ported.
/// Here the bit is cleared, so a sub-machine restarts at its initial state
/// whenever its parent state is entered: **guessed**, the behaviour of the
/// per-cluster memory is not traced. The daily controllers' sub-machines
/// hold one state each, so they are unaffected.
fn core(path: &str, g: Arc<Graph>) -> Result<Core, Error> {
    if g.attribute & 2 != 0 {
        let mut plain = (*g).clone();
        plain.attribute &= !2;
        return Core::new(path, Arc::new(plain));
    }
    Core::new(path, g)
}

/// One scheduled NPC.
pub struct Npc {
    pub id: u32,
    pub controller: Option<Core>,
    pub action: Option<Core>,
    think: Think,
    /// Owner fields of the controller graph (its own SysCondition etc.).
    ctrl: Think,
    pub action_path: Option<String>,
    applied: bool,
    finished: bool,
}

impl Npc {
    /// Allocate the intel's consideration/action controller state before
    /// any graph is loaded (0x00bbd930, DDDA.exe 2017-12-13 Steam).
    /// `controller: None` means no loaded graph, not an absent controller.
    pub fn intel(id: u32) -> Npc {
        let mk = || Think {
            npc: id,
            sys: 0,
            free: 0,
            flags: [0; 4],
            actor: ActorObservations::default(), scene: None,
            goal: Goal::None,
            command: 0,
            motion_active: false,
            pos: None,
            load: None,
            out: vec![],
            complete_moves: false,
            strict: false,
            talk_end: false,
        };
        Npc {
            id,
            controller: None,
            action: None,
            think: mk(),
            ctrl: mk(),
            action_path: None,
            applied: false,
            finished: false,
        }
    }

    /// An NPC with a daily controller (`path` from `ddnpc::schedule_path`).
    pub fn scheduled(id: u32, graphs: &dyn Graphs, path: &str) -> Result<Npc, Error> {
        let mut npc = Self::intel(id);
        npc.controller = Some(core(path, graphs.graph(path)?)?);
        Ok(npc)
    }

    /// Run an explicitly applied action graph, without a daily controller.
    /// Source: 0x00765070 -> 0x00bb45e0 -> 0x0059e260.
    pub fn applied(id: u32, graphs: &dyn Graphs, path: &str) -> Result<Npc, Error> {
        let graph = graphs.graph(path)?;
        if graph.attribute & 2 != 0
            && graph
                .root
                .as_ref()
                .is_some_and(|c| c.states.iter().any(|s| s.subcluster.is_some()))
        {
            return Err(Error::Unimplemented("applied NPC per-cluster work memory".into()));
        }
        let mut npc = Self::scheduled(id, graphs, path)?;
        npc.action = npc.controller.take();
        npc.action_path = Some(path.into());
        npc.applied = true;
        npc.think.strict = true;
        Ok(npc)
    }

    /// One-frame actor observation. Cleared by uCharacterBase::vf111
    /// (0x00850121), set from actual dialogue by uNpc::vf111 (0x00bb32cb).
    pub fn observe_talk_end(&mut self, ended: bool) {
        self.think.talk_end = ended;
    }

    /// Both controller graphs resolve the same actor owner.
    pub fn observe_actor(&mut self, actor: ActorObservations) {
        self.ctrl.actor = actor;
        self.think.actor = actor;
    }

    /// Observe the shared scene number for both controller and action graphs.
    /// Absence remains an error if an authored graph reads SceNo.
    pub fn observe_scene(&mut self, scene: Option<u32>) {
        self.ctrl.scene = scene;
        self.think.scene = scene;
    }

    /// Set a graph's private FreeFlag, distinct from FreeCondition.
    /// Translated from 0x007abdd0 (DDDA.exe 2017-12-13 Steam).
    pub fn set_free_flag(&mut self, controller: bool, index: u32, value: bool) {
        if index >= 128 { return; }
        let fields = if controller { &mut self.ctrl } else { &mut self.think };
        let word = &mut fields.flags[index as usize / 32];
        let bit = 1 << (index & 31);
        if value { *word |= bit; } else { *word &= !bit; }
    }

    pub fn applied_finished(&self) -> bool {
        self.applied && self.finished
    }

    /// Complete moves and motions at once (a host without an NPC mover).
    pub fn complete_moves(&mut self, on: bool) {
        self.think.complete_moves = on;
    }

    /// Port error recovery: stop unsupported graphs without destroying
    /// their allocated controller fields or inventing action completion.
    pub fn halt_graphs(&mut self) {
        self.controller = None;
        self.action = None;
        self.ctrl.load = None;
        self.think.out.clear();
        self.ctrl.out.clear();
        // A partially processed frame cannot publish a pending completion
        // after its graph has been stopped for an unsupported operation.
        self.think.sys &= !1;
    }

    pub fn position(&self) -> Option<[f32; 3]> {
        self.think.pos
    }

    /// The action graph's current state names, root first.
    pub fn action_states(&self) -> Vec<String> {
        self.action
            .as_ref()
            .map(|c| c.current_names())
            .unwrap_or_default()
    }

    /// The actor layer reports the current interface done (an arrival with
    /// the position reached, or a motion end).
    pub fn done(&mut self, pos: Option<[f32; 3]>) {
        if pos.is_some() {
            self.think.pos = pos;
        }
        self.think.finish();
    }

    /// Current command only while this graph owns an unfinished motion.
    pub fn motion_command(&self) -> Option<u64> {
        (self.action.is_some() && self.think.motion_active && self.think.sys & 4 == 0)
            .then_some(self.think.command)
    }

    pub fn motion_done(&mut self, command: u64) -> bool {
        if self.motion_command() != Some(command) { return false; }
        self.think.finish();
        true
    }

    /// One frame: the controller, then the action graph it selected.
    pub fn step(&mut self, graphs: &dyn Graphs, clock: Clock) -> Result<Vec<Request>, Error> {
        self.step_observed(graphs, clock, None)
    }

    /// Run against the host's current story state. Missing observations remain
    /// unsupported if a graph actually reads them; they never imply false flags.
    pub fn step_observed(&mut self, graphs: &dyn Graphs, clock: Clock,
        world: Option<&ddstory::World>) -> Result<Vec<Request>, Error> {
        let mut env = Env { clock, world };
        if let Some(c) = self.controller.as_mut() {
            c.step(&mut self.ctrl, &mut env)?;
        }
        if let Some(path) = self.ctrl.load.take() {
            let g = graphs.graph(&path)?;
            self.action = Some(core(&path, g)?);
            self.think.sys = 0;
            self.think.goal = Goal::None;
            self.think.out.push(Request::Action {
                npc: self.id,
                path: path.clone(),
            });
            self.action_path = Some(path);
        }
        let result = if let Some(a) = self.action.as_mut() {
            a.step(&mut self.think, &mut env)
        } else {
            Ok(())
        };
        self.think.talk_end = false;
        result?;
        // Applied wrappers stop at Finish (0x005a1300), unlike the enemy
        // driver's rewind cycle. Normal-controller restoration is external.
        if self.applied && !self.finished && self.think.sys & 1 != 0 {
            self.finished = true;
            self.action = None;
            self.think.sys = 0;
            self.think.out.push(Request::Finished { npc: self.id });
        }
        Ok(std::mem::take(&mut self.think.out))
    }
}

#[cfg(test)]
mod variable_tests {
    use super::*;
    fn fields() -> Think {
        Think { npc: 1, sys: 0, free: u32::MAX, flags: [0; 4],
            actor: ActorObservations::default(), scene: None, goal: Goal::None, command: 0, motion_active: false, pos: None,
            load: None, out: vec![], complete_moves: false, strict: true, talk_end: false }
    }
    fn value(fields: &Think, owner: &str, property: &str, index: Option<u32>) -> Value {
        fields.var(&Env { clock: Clock { hour: 12, minute: 0, dt: 0.0 },
            world: None }, &Var { owner, property, index }).unwrap()
    }
    #[test]
    fn quest_flag_polling_clears_sets_inverts_and_preserves_other_private_bits() {
        let mut npc = Npc::intel(3);
        let mut world = ddstory::World::default();
        npc.set_free_flag(false, 127, true);
        npc.set_free_flag(false, 31, true);
        npc.think.check_quest_flag(2, 95, 31, None, None).unwrap();
        assert_eq!(npc.think.flags, [0, 0, 0, 0x8000_0000]);
        assert!(npc.think.check_quest_flag(2, 95, 31, Some(0), None).is_err());
        for (authored, mode, expected) in [(true, 0, true), (false, 0, false),
            (false, 1, true), (true, 1, false)] {
            world.set_quest_bit(2, 95, authored);
            npc.think.check_quest_flag(2, 95, 31, Some(mode), Some(&world)).unwrap();
            assert_eq!(npc.think.flags[0] != 0, expected);
            assert_eq!(npc.think.flags[3], 0x8000_0000);
        }
        let before = npc.think.flags;
        for (quest, bit, flag, mode) in [(128, 95, 31, 0), (2, 96, 31, 0),
            (2, 95, 128, 0), (2, 95, 31, 2)] {
            npc.think.check_quest_flag(quest, bit, flag, Some(mode), None).unwrap();
        }
        assert_eq!(npc.think.flags, before);
        assert_eq!(npc.ctrl.flags, [0; 4], "controller private flags remain separate");
    }
    #[test]
    fn boolean_actor_flag_callbacks_have_distinct_sourced_masks() {
        for (name, expected) in [("Set Invincible", 0x1000_0000), ("Set No Damage", 0x0800_0000), ("Set Super Armor", 0x0200_0000), ("Set No Motion Talk", 0x0100_0000),
            ("Set No Surprise", 0x1_0000),
            ("Set Lift Disable", 0x800), ("Set No Lift Reaction", 0x1000),
            ("Set No Escape", 0x2000), ("Set Lift Enable", 0x4_0000), ("Set Fix Pos", 0x200)] {
            assert_eq!(ActorFlag::from_process(name).unwrap().mask(), expected);
        }
        assert!(ActorFlag::from_process("unknown").is_none());
    }
    #[test]
    fn authored_disable_bits_map_without_touching_unselected_flags() {
        for (bit, expected) in [0x1000_0000, 0x0800_0000, 0x0400_0000, 0x0200_0000,
            0x10000, 0x800, 0x1000, 0x2000, 0x200, 0x100, 0x40, 0x10].into_iter().enumerate() {
            assert_eq!(disable_flag_mask(1 << bit), expected);
        }
        assert_eq!(disable_flag_mask(0xffff_f000), 0);
        assert_eq!(disable_flag_mask(352), 0x1a00);
    }
    #[test]
    fn scene_observation_is_shared_and_refreshes_after_story_progress() {
        let mut npc = Npc::intel(1);
        let env = Env { clock: Clock { hour: 12, minute: 0, dt: 0.0 }, world: None };
        let var = Var { owner: "cAIActionFSMDemo", property: "SceNo", index: None };
        assert!(npc.think.var(&env, &var).is_err());
        for scene in [1100, 1200, 1400] {
            npc.observe_scene(Some(scene));
            assert_eq!(npc.think.var(&env, &var).unwrap(), Value::Int(scene as i64));
            assert_eq!(value(&npc.ctrl, "cAIConsiderFSM_24Schedule", "SceNo", None), Value::Int(scene as i64));
        }
        npc.observe_scene(None);
        assert!(npc.think.var(&env, &var).is_err());
        assert!(npc.ctrl.var(&env, &var).is_err());
    }
    #[test]
    fn free_flags_are_128_private_bits_not_free_condition() {
        let mut action = fields(); let control = fields();
        for i in [0, 31, 32, 95, 127] { action.flags[i / 32] |= 1 << (i % 32); }
        for i in 0..130 {
            assert_eq!(value(&action, "cAIActionFSMNpc", "FreeFlag", Some(i)),
                Value::Bool([0, 31, 32, 95, 127].contains(&i)));
            assert_eq!(value(&control, "cAIConsiderFSM_24Schedule", "FreeFlag", Some(i)), Value::Bool(false));
        }
    }
    #[test]
    fn actor_variables_require_matching_owner_and_exact_flags_or_goods() {
        let mut f = fields();
        assert_eq!(value(&f, "cAIActionFSMNpc", "Hit (flag)", None), Value::Bool(false));
        assert_eq!(value(&f, "cAIActionFSMNpc", "NpcGoodsHaving", None), Value::Bool(false));
        for flags in [0, 0x7f, 0x80, 0x100, u32::MAX] {
            f.actor.character_flags = Some(flags);
            assert_eq!(value(&f, "cAIActionFSMNpc", "Hit (flag)", None), Value::Bool(flags & 0x80 != 0));
        }
        for goods in [[false, false], [true, false], [false, true], [true, true]] {
            f.actor.npc_goods = Some(goods);
            assert_eq!(value(&f, "cAIActionFSMNpc", "NpcGoodsHaving", None), Value::Bool(goods[0] || goods[1]));
        }
    }
    #[test]
    fn halt_discards_uncommitted_finish_without_destroying_owner() {
        struct Unused;
        impl Graphs for Unused {
            fn graph(&self, _: &str) -> Result<Arc<Graph>, Error> {
                panic!("halted graph must not load data")
            }
        }
        let mut npc = Npc::intel(1);
        npc.applied = true;
        npc.think.sys = 1;
        npc.think.pos = Some([1.0, 2.0, 3.0]);
        npc.halt_graphs();
        assert!(npc.step(&Unused, Clock { hour: 12, minute: 0, dt: 0.1 }).unwrap().is_empty());
        assert!(!npc.applied_finished());
        assert_eq!(npc.position(), Some([1.0, 2.0, 3.0]));
    }

}
