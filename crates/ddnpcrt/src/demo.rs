//! Authored NPC Demo graphs: commands and source-owned completion checks.
//! Actor movement/reactions remain host responsibilities, never timer stubs.
// Translated from 0x00533DC0, 0x00533A90, 0x0056CFF0, 0x0056F160,
// 0x0056DCC0, 0x0056EBB0 and 0x007A9CB0 (DDDA.exe 2017-12-13 Steam).
// The body-manager retry byte and unsupported-interface result are not
// modelled here; an actor adapter must supply completion for a supported action.
use ddai::{cond::{Value, Var}, core::{Core, Owner, ProcAt, ProcMem}, fields::Fields,
    think::{self, ThinkFields}, Error};
use mtfsm::{Graph, Process, Record};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum Interface {
    /// 0x0056CFF0 requests cEventActWait and always reports running4.
    Wait,
    /// 0x0056F160 delegates the installed generic motion interface.
    Motion(crate::schedule::Motion),
    /// 0x0056DCC0 / 0x0056E230. Full trace record stays runtime data.
    GotoEx { position: [f32; 3], motion: u32, wait_end: bool,
        start_frame: f32, speed: f32, trace: Record },
    /// 0x0056EBB0 / 0x0056EDA0 requests a reaction action. This does
    /// not value an attack, subtract HP, or finish a death lifecycle.
    Damage { kind: u32, attack_position: [f32; 3], down: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Interface { npc: u32, command: u64, interface: Interface },
    ActorFlags { npc: u32, mask: u32, value: bool },
    World(think::Request),
}

/// Real observations for the current materialized lifetime.
pub struct Inputs<'a> {
    pub think: &'a think::Inputs,
    pub world: &'a ddstory::World,
    /// Native 0x007AC270 -> 0x00477EF0: NPC body is in the manager,
    /// unit state lowbits2 and readiness flag0x400. Intel alone is false.
    pub npc_ready: bool,
    /// Native validated character+0x2EA1 (0x007AB800), not HP loss inferred
    /// by this controller. The host's actual reaction supplies this value.
    pub damage_flag: bool,
    /// Owner's native pause byte +0xe, used by 0x00533A90.
    pub interface_paused: bool,
}

fn parameter(process: &Process) -> Result<&Record, Error> {
    process.parameter.as_ref().ok_or_else(|| Error::Data("Demo process missing parameters".into()))
}

/// Read the actual Demo motion wrapper. The generic payload is shared with
/// NpcSetMotion; Demo has no intel-wait property and requires a real body
/// in 0x0056F160. All resource values are preserved, without defaults.
pub fn motion(wrapper: &Record) -> Result<crate::schedule::Motion, Error> {
    if wrapper.class_name == Some("cAIActionInterfaceNpcSetMotion") {
        return crate::schedule::Motion::from_record(wrapper);
    }
    if wrapper.class_name != Some("cAIActionInterfaceDemoSetMotion") {
        return Err(Error::Unimplemented("Demo generic motion wrapper class".into()));
    }
    crate::schedule::Motion::from_demo_record(wrapper)
}

pub fn decode_interface(wrapper: &Record) -> Result<Interface, Error> {
    let missing = |field| Error::Data(format!("Demo interface missing {field}"));
    let number = |field| wrapper.u(field).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| missing(field));
    let float = |field| wrapper.f(field).filter(|n| n.is_finite()).ok_or_else(|| missing(field));
    let boolean = |field| wrapper.b(field).ok_or_else(|| missing(field));
    match wrapper.class_name {
        Some("cAIActionInterfaceDemoSetWait") => Ok(Interface::Wait),
        Some("cAIActionInterfaceDemoSetMotion" | "cAIActionInterfaceNpcSetMotion") => Ok(Interface::Motion(motion(wrapper)?)),
        Some("cAIActionInterfaceDemoSetGotoEx") => {
            let position = wrapper.object("mpPosition").and_then(|p| p.vec3("ポジション"))
                .filter(|p| p.iter().all(|n| n.is_finite())).ok_or_else(|| missing("position"))?;
            Ok(Interface::GotoEx { position, motion: number("モーション番号")?, wait_end: boolean("待機終了")?,
                start_frame: float("開始フレーム")?, speed: float("再生速度")?,
                trace: wrapper.object("mpTrace").cloned().ok_or_else(|| missing("trace"))? })
        }
        Some("cAIActionInterfaceDemoSetDamage") => {
            let kind = number("DamageType")?;
            if kind > 4 { return Err(Error::Unimplemented(format!("Demo damage action kind{kind}"))); }
            Ok(Interface::Damage { kind, attack_position: wrapper.vec3("AttackPos")
                .filter(|p| p.iter().all(|n| n.is_finite())).ok_or_else(|| missing("AttackPos"))?, down: boolean("Down")? })
        }
        class => Err(Error::Unimplemented(format!("Demo interface {class:?}"))),
    }
}

fn flag_mask(name: &str) -> Option<u32> {
    Some(match name {
        "Set Invincible" => 0x1000_0000, // 0x007AA040
        "Set No Damage" => 0x0800_0000, // 0x007AA0D0
        "Set Leave Life" => 0x0400_0000, // 0x007AA160
        "Set Super Armor" => 0x0200_0000, // 0x007AA1F0
        "Set No Delete" => 0x0040_0000, // 0x007AA310
        "Set No Talk" => 0x0080_0000, // 0x007AA670
        "Set Lift Disable" => 0x800, // 0x007AA430
        "Set No Lift Reaction" => 0x1000, // 0x007AA4C0
        _ => return None,
    })
}

struct FieldsState {
    npc: u32,
    common: ThinkFields,
    free_flags: [u32; 4],
    command: u64,
    active: Option<Interface>,
    complete: bool,
}

struct Env<'a> {
    inputs: &'a Inputs<'a>,
    requests: &'a mut Vec<Request>,
}

impl FieldsState {
    fn check_quest(&mut self, process: &Process, world: Option<&ddstory::World>) -> Result<(), Error> {
        let parameter = parameter(process)?;
        let read = |field| parameter.u(field).ok_or_else(|| Error::Data(format!("Demo QuestFlag missing {field}")));
        let (quest, bit, flag, act) = (read("QuestNo")?, read("BitNo")?, read("FlagNo")?, read("ActType")?);
        if quest >= 128 || bit >= 96 || flag >= 128 || act > 1 { return Ok(()); }
        let value = world.is_some_and(|w| w.quest_bit(quest as usize, bit as usize) != (act != 0));
        let word = &mut self.free_flags[flag as usize / 32];
        let mask = 1 << (flag & 31);
        if value { *word |= mask; } else { *word &= !mask; }
        Ok(())
    }
}

impl Owner for FieldsState {
    type Env<'a> = Env<'a>;
    fn state_changed(&mut self, env: &mut Env<'_>) {
        let mut discarded = Vec::new();
        self.common.state_changed(&mut think::Env { inputs: env.inputs.think, requests: &mut discarded });
        // The interface launch latch is cAIActionFSM+0x6c (0x00533A90).
        // Every replacement receives a fresh command token below.
    }
    fn var(&self, env: &Env<'_>, variable: &Var) -> Result<Value, Error> {
        match variable.property {
            "NpcUnit" => Ok(Value::Bool(env.inputs.npc_ready)),
            "Damage (flag)" => Ok(Value::Bool(env.inputs.damage_flag)),
            "GSF" => Ok(Value::Bool(variable.index.is_some_and(|n| env.inputs.world.gsf.get(n as usize)))),
            "FreeFlag" => Ok(Value::Bool(variable.index.is_some_and(|n|
                n < 128 && self.free_flags[n as usize / 32] & (1 << (n & 31)) != 0))),
            _ => self.common.var(&think::Env { inputs: env.inputs.think, requests: &mut Vec::new() }, variable),
        }
    }
    fn update(&mut self, env: &mut Env<'_>, process: &Process, memory: &mut ProcMem, at: ProcAt) -> Result<(), Error> {
        let name = String::from_utf8_lossy(&process.container);
        if let Some(mask) = flag_mask(&name) {
            let value = parameter(process)?.b("Bool").ok_or_else(|| Error::Data("Demo boolean flag missing Bool".into()))?;
            env.requests.push(Request::ActorFlags { npc: self.npc, mask, value });
            return Ok(());
        }
        match name.as_ref() {
            "SetInterfaceDemo" | "SetInterfaceNpc" => {}, // Empty native entry callback.
            "Check QuestFlag" => self.check_quest(process, None)?, // 0x007AADD0 clears first.
            _ => {
                let mut requests = Vec::new();
                self.common.update(&mut think::Env { inputs: env.inputs.think, requests: &mut requests }, process, memory, at)?;
                env.requests.extend(requests.into_iter().map(Request::World));
            }
        }
        Ok(())
    }
    fn state(&mut self, env: &mut Env<'_>, process: &Process, memory: &mut ProcMem, at: ProcAt) -> Result<(), Error> {
        let name = String::from_utf8_lossy(&process.container);
        if flag_mask(&name).is_some() { return Ok(()); }
        match name.as_ref() {
            // SetInterfaceDemo callback0x00533E20 is JMP0x00533A90.
            "SetInterfaceDemo" | "SetInterfaceNpc" => {
                if env.inputs.interface_paused { return Ok(()); }
                if self.common.arm == 0 {
                    let Some(interface) = parameter(process)?.object("Interface") else { return Ok(()) };
                    let interface = decode_interface(interface)?;
                    self.command = self.command.checked_add(1).ok_or_else(|| Error::Data("Demo command exhausted".into()))?;
                    self.active = Some(interface.clone());
                    self.complete = false;
                    self.common.arm = 1;
                    env.requests.push(Request::Interface { npc: self.npc, command: self.command, interface });
                } else if self.common.arm == 1 && self.complete {
                    self.common.sys_condition |= 4;
                }
            }
            "Check QuestFlag" => self.check_quest(process, Some(env.inputs.world))?, // 0x007AAE20
            _ => {
                let mut requests = Vec::new();
                self.common.state(&mut think::Env { inputs: env.inputs.think, requests: &mut requests }, process, memory, at)?;
                env.requests.extend(requests.into_iter().map(Request::World));
            }
        }
        Ok(())
    }
}

pub struct Demo {
    core: Core,
    fields: FieldsState,
}

impl Demo {
    pub fn new(npc: u32, path: &str, graph: Arc<Graph>) -> Result<Self, Error> {
        if graph.owner != b"cAIActionFSMDemo" { return Err(Error::Data("expected cAIActionFSMDemo graph".into())); }
        Ok(Self { core: Core::new_with_cluster_work(path, graph)?, fields: FieldsState {
            npc, common: ThinkFields::default(), free_flags: [0; 4], command: 0, active: None, complete: false,
        } })
    }
    pub fn current_states(&self) -> Vec<u32> { self.core.current_states() }
    /// Actor observation for this exact command. Wait cannot finish:
    /// its native update always returns running4, even after motion end.
    pub fn interface_done(&mut self, command: u64) -> bool {
        if self.fields.common.arm != 1 || command != self.fields.command || self.fields.active.is_none()
            || self.fields.active == Some(Interface::Wait) || self.fields.complete { return false; }
        self.fields.complete = true;
        true
    }
    pub fn finished(&self) -> bool { self.fields.common.sys_condition & 0x11 != 0 }
    // Shared cThinkFSM vf06 ordering0x007A9CB0: countdowns, core, timers.
    pub fn step(&mut self, inputs: &Inputs<'_>) -> Result<Vec<Request>, Error> {
        // Applied wrappers retire on Finish/Kill; unlike enemy consideration
        // drivers they do not fabricate a new Core/clear ClusterWork here.
        if self.finished() { return Ok(Vec::new()); }
        let common = &mut self.fields.common;
        if common.after_change > 0 { common.after_change -= 1; }
        if common.countdown_a5 > 0 { common.countdown_a5 -= 1; }
        let mut requests = Vec::new();
        self.core.step(&mut self.fields, &mut Env { inputs, requests: &mut requests })?;
        let common = &mut self.fields.common;
        let seconds = inputs.think.time.delta / inputs.think.time.fps;
        for n in 0..4 { if common.timer_run & (1 << n) != 0 { common.timers[n] += seconds; } }
        if common.random_timer > 0.0 { common.random_timer -= seconds; } else { common.random_timer = 0.0; }
        Ok(requests)
    }
}
