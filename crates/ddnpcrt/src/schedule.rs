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

/// What an NPC asks of the actor layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// `cAIActionInterfaceNpcSetGoto`: walk (or run) to a point, using the
    /// navigation route when `route` is set.
    Goto { npc: u32, pos: [f32; 3], run: bool, route: bool },
    /// `cAIActionInterfaceNpcSetWait`: stand, facing `dir` when given.
    Wait { npc: u32, seconds: Option<f32>, dir: Option<[f32; 3]> },
    /// `cAIActionInterfaceNpcSetMotion` / `SetActionEx`: play the motion the
    /// record names (passed through unread).
    Motion { npc: u32, interface: String },
    /// Placed at once (`SetInitPosNpc`, `cAIActionInterfaceDemoSetPos`).
    Place { npc: u32, pos: [f32; 3], dir: Option<[f32; 3]> },
    /// The action graph changed (`LoadFSM` of the daily controller).
    Action { npc: u32, path: String },
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

/// The `cThinkFSM` fields the NPC graphs read and write.
#[derive(Debug, Clone, PartialEq)]
struct Think {
    npc: u32,
    /// `SysCondition` (+0x68).
    sys: u32,
    /// `FreeCondition` (+0x70); `Set FreeCondition` sets bit `FlagNo`
    /// (`1 << n`: the data tests bit 31 with 0x80000000).
    free: u32,
    goal: Goal,
    pos: Option<[f32; 3]>,
    load: Option<String>,
    out: Vec<Request>,
    complete_moves: bool,
}

struct Env<'a> {
    clock: Clock,
    _graphs: std::marker::PhantomData<&'a ()>,
}

fn rec(p: &Process) -> Result<&mtfsm::Record, Error> {
    p.parameter.as_ref().ok_or_else(|| Error::Data("process without parameter".into()))
}

fn name(p: &Process) -> String {
    String::from_utf8_lossy(&p.container).into_owned()
}

impl Think {
    fn finish(&mut self) {
        self.sys |= 4;
    }

    fn interface(&mut self, r: &mtfsm::Record) -> Result<(), Error> {
        let i = r.object("Interface").ok_or_else(|| Error::Data("interface missing".into()))?;
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
                let seconds = if infinite { None } else { t.and_then(|t| t.f("待機時間")) };
                let dir = i
                    .object("mpDirection")
                    .filter(|d| d.b("方向指定").unwrap_or(false))
                    .and_then(|d| d.vec3("方向"));
                self.out.push(Request::Wait { npc, seconds, dir });
                self.goal = Goal::Wait(seconds);
            }
            c @ ("cAIActionInterfaceNpcSetMotion" | "cAIActionInterfaceNpcSetActionEx") => {
                self.out.push(Request::Motion { npc, interface: c.to_string() });
                self.goal = Goal::Motion;
            }
            "cAIActionInterfaceDemoSetPos" => {
                let pos = if i.b("ポジションセットOn/Off").unwrap_or(false) { i.vec3("ポジション") } else { None };
                let dir = if i.b("方向セットOn/Off").unwrap_or(false) { i.vec3("方向") } else { None };
                if let Some(p) = pos {
                    self.pos = Some(p);
                    self.out.push(Request::Place { npc, pos: p, dir });
                }
                self.goal = Goal::None;
                self.finish();
            }
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
        self.sys &= !4;
    }

    fn var(&self, env: &Env<'_>, v: &Var) -> Result<Value, Error> {
        Ok(match v.property {
            "SysCondition" => Value::Int(self.sys as i64),
            "FreeCondition" => Value::Int(self.free as i64),
            // cThinkFSM getter 0x007ac350: the game hour.
            "GameHour" => Value::Int(env.clock.hour as i64),
            // cAIConsiderFSM_24Schedule +0xe4, set by 0x0059d790.
            "GlobalTime" => Value::Int(ddnpc::schedule_time(env.clock.hour, env.clock.minute).unwrap_or(0) as i64),
            p => return Err(Error::Unimplemented(format!("NPC variable {}.{p}", v.owner))),
        })
    }

    fn update(&mut self, _env: &mut Env<'_>, p: &Process, _m: &mut ProcMem, _at: ProcAt) -> Result<(), Error> {
        match name(p).as_str() {
            "LoadFSM" => {
                let path = rec(p)?.text("FSMPath").ok_or_else(|| Error::Data("LoadFSM without path".into()))?;
                self.load = Some(path);
            }
            "SetInitPosNpc" => {
                let pos = rec(p)?.vec3("InitPos").ok_or_else(|| Error::Data("InitPos".into()))?;
                self.pos = Some(pos);
                self.out.push(Request::Place { npc: self.npc, pos, dir: None });
            }
            "SetInterfaceNpc" | "SetInterfaceDemo" | "SetInterface" => self.interface(rec(p)?)?,
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
            _ => {}
        }
        Ok(())
    }

    fn state(&mut self, env: &mut Env<'_>, p: &Process, _m: &mut ProcMem, _at: ProcAt) -> Result<(), Error> {
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
}

impl Npc {
    /// An NPC with a daily controller (`path` from `ddnpc::schedule_path`).
    pub fn scheduled(id: u32, graphs: &dyn Graphs, path: &str) -> Result<Npc, Error> {
        let mk = || Think {
            npc: id,
            sys: 0,
            free: 0,
            goal: Goal::None,
            pos: None,
            load: None,
            out: vec![],
            complete_moves: false,
        };
        Ok(Npc {
            id,
            controller: Some(core(path, graphs.graph(path)?)?),
            action: None,
            think: mk(),
            ctrl: mk(),
            action_path: None,
        })
    }

    /// Complete moves and motions at once (a host without an NPC mover).
    pub fn complete_moves(&mut self, on: bool) {
        self.think.complete_moves = on;
    }

    pub fn position(&self) -> Option<[f32; 3]> {
        self.think.pos
    }

    /// The action graph's current state names, root first.
    pub fn action_states(&self) -> Vec<String> {
        self.action.as_ref().map(|c| c.current_names()).unwrap_or_default()
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
        let mut env = Env { clock, _graphs: std::marker::PhantomData };
        if let Some(c) = self.controller.as_mut() {
            c.step(&mut self.ctrl, &mut env)?;
        }
        if let Some(path) = self.ctrl.load.take() {
            let g = graphs.graph(&path)?;
            self.action = Some(core(&path, g)?);
            self.think.sys = 0;
            self.think.goal = Goal::None;
            self.think.out.push(Request::Action { npc: self.id, path: path.clone() });
            self.action_path = Some(path);
        }
        if let Some(a) = self.action.as_mut() {
            a.step(&mut self.think, &mut env)?;
        }
        Ok(std::mem::take(&mut self.think.out))
    }
}
