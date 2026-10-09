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

/// What an NPC asks of the actor layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
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
    /// record names (passed through unread).
    Motion {
        npc: u32,
        interface: String,
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
    goal: Goal,
    pos: Option<[f32; 3]>,
    load: Option<String>,
    out: Vec<Request>,
    complete_moves: bool,
    strict: bool,
    talk_end: bool,
}

struct Env<'a> {
    clock: Clock,
    _graphs: std::marker::PhantomData<&'a ()>,
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
    fn finish(&mut self) {
        self.sys |= 4;
    }

    fn interface(&mut self, r: &mtfsm::Record) -> Result<(), Error> {
        let i = r
            .object("Interface")
            .ok_or_else(|| Error::Data("interface missing".into()))?;
        let npc = self.npc;
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
                });
                self.goal = Goal::Motion;
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
            actor: ActorObservations::default(),
            goal: Goal::None,
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

    /// One frame: the controller, then the action graph it selected.
    pub fn step(&mut self, graphs: &dyn Graphs, clock: Clock) -> Result<Vec<Request>, Error> {
        let mut env = Env {
            clock,
            _graphs: std::marker::PhantomData,
        };
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
            actor: ActorObservations::default(), goal: Goal::None, pos: None,
            load: None, out: vec![], complete_moves: false, strict: true, talk_end: false }
    }
    fn value(fields: &Think, owner: &str, property: &str, index: Option<u32>) -> Value {
        fields.var(&Env { clock: Clock { hour: 12, minute: 0, dt: 0.0 },
            _graphs: std::marker::PhantomData }, &Var { owner, property, index }).unwrap()
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
