//! Installed applied-owner selection and Demo actor request composition.
//! Pending navigation/reaction commands are observations, never fake Done.

use ddnpcrt::{demo::{self, ActorInputs, Demo, Interface, WorldRequest},
    schedule::{Clock, Graphs, Npc, Request}, Error};
use ddquest::order::Fields;
#[path = "npc_demo_physics.rs"]
mod physics;
#[path = "npc_demo_reaction.rs"]
mod reaction;

pub(super) enum Action {
    Npc(Npc),
    Demo { owner: Demo, pending: Option<(u64, Interface)>, halted: bool },
}

#[derive(Default)]
pub(super) struct Frame {
    pub scheduled: Vec<Request>,
    pub replaced: Vec<u32>,
    pub flags: Vec<(u32, u32, bool)>,
    pub world: Vec<WorldRequest>,
}

impl Action {
    /// Native graph class chooses the owner type (0x0059E260). The existing
    /// adapter also supports DemoSetPos and inherited NPC interfaces;
    /// keep that tested subset until this owner ports those callbacks too.
    /// Neither path nor NPC number is used to select the supported adapter.
    pub fn new(npc: u32, graphs: &dyn Graphs, path: &str) -> Result<Self, Error> {
        let graph = graphs.graph(path)?;
        fn demo_interfaces(cluster: &mtfsm::Cluster) -> bool {
            cluster.states.iter().any(|state|
                state.processes.iter().any(|p| {
                    // Native 0x00533A90 -> 0x0056C940 dispatches through the
                    // interface object's type. SetInterfaceDemo alone does not imply
                    // the new adapter: NPC1 uses it for DemoSetPos, which the
                    // existing adapter already handles before its NPC Guard.
                    matches!(p.container.as_slice(), b"SetInterfaceDemo" | b"SetInterfaceNpc" | b"SetInterface")
                        && p.parameter.as_ref().and_then(|r| r.object("Interface"))
                            .and_then(|r| r.class_name).is_some_and(|class|
                                class.starts_with("cAIActionInterfaceDemo")
                                && class != "cAIActionInterfaceDemoSetPos")
                })
                || state.subcluster.as_ref().is_some_and(|sub| demo_interfaces(sub)))
        }
        if graph.owner == b"cAIActionFSMDemo" && graph.root.as_ref().is_some_and(demo_interfaces) {
            Ok(Self::Demo { owner: Demo::new(npc, path, graph)?, pending: None, halted: false })
        } else {
            Ok(Self::Npc(Npc::applied(npc, graphs, path)?))
        }
    }
    pub fn applied_finished(&self) -> bool {
        match self { Self::Npc(npc) => npc.applied_finished(), Self::Demo { owner, halted, .. } => !halted && owner.finished() }
    }
    pub fn observe_talk_end(&mut self, ended: bool) {
        if let Self::Npc(npc) = self { npc.observe_talk_end(ended); }
        // Demo TalkEnd is not implemented; installed beach graphs do not read it.
    }
    pub fn motion_command(&self) -> Option<u64> {
        match self { Self::Npc(npc) => npc.motion_command(), Self::Demo { owner, halted, .. } => if *halted { None } else { owner.motion_command() } }
    }
    pub fn motion_done(&mut self, command: u64) -> bool {
        match self {
            Self::Npc(npc) => npc.motion_done(command),
            Self::Demo { owner, halted, .. } => !*halted && owner.motion_command() == Some(command) && owner.interface_done(command),
        }
    }
    pub fn done(&mut self, position: Option<[f32; 3]>) {
        // The legacy arrival API does not prove a Demo GotoEx action result.
        if let Self::Npc(npc) = self { npc.done(position); }
    }
    pub fn position(&self) -> Option<[f32; 3]> {
        match self { Self::Npc(npc) => npc.position(), Self::Demo { .. } => None }
    }
    pub fn halt_graphs(&mut self) {
        match self { Self::Npc(npc) => npc.halt_graphs(), Self::Demo { halted, .. } => *halted = true }
    }
    pub fn pending(&self) -> Option<(u64, &Interface)> {
        match self {
            Self::Demo { pending: Some((command, interface)), halted: false, owner } if !owner.finished() => Some((*command, interface)),
            _ => None,
        }
    }
    pub fn step(&mut self, graphs: &dyn Graphs, clock: Clock, inputs: &ActorInputs,
        world: &ddstory::World, ready: bool, motion_number: Option<u16>, paused: bool) -> Result<Frame, Error> {
        match self {
            Self::Npc(npc) => {
                npc.observe_scene(Some(world.sce_no));
                Ok(Frame { scheduled: npc.step_observed(graphs, clock, Some(world))?, ..Frame::default() })
            }
            Self::Demo { owner, pending, halted } => {
                if *halted || owner.finished() { return Ok(Frame::default()); }
                let mut frame = Frame::default();
                for request in owner.step(&demo::Inputs { think: inputs, world,
                    npc_ready: ready, damage_flag: false,
                    motion_number, interface_paused: paused })? {
                    // No Demo reaction action currently sets character+0x2EA1.
                    // This remains false until that actual actor path is ported.
                    match request {
                        demo::Request::Interface { npc, command, interface } => {
                            frame.replaced.push(npc);
                            match &interface {
                                Interface::Motion(motion) => frame.scheduled.push(Request::Motion {
                                    npc, command, interface: "cAIActionInterfaceDemoSetMotion".into(), motion: Some(motion.clone()),
                                }),
                                Interface::Wait => frame.scheduled.push(Request::Wait { npc, seconds: None, dir: None }),
                                // Native GotoEx and Damage have distinct action result checks.
                                // Keep their exact runtime payload pending for their actor ports.
                                Interface::GotoEx { .. } | Interface::Damage { .. } => {
                                }
                            }
                            *pending = Some((command, interface));
                        }
                        demo::Request::ActorFlags { npc, mask, value } => frame.flags.push((npc, mask, value)),
                        demo::Request::World(request @ (WorldRequest::QuestFlag { .. } | WorldRequest::Gsf { .. } | WorldRequest::LayoutFlag { .. })) => frame.world.push(request),
                        demo::Request::World(request) => return Err(Error::Unimplemented(format!("Demo actor request {request:?}"))),
                    }
                }
                Ok(frame)
            }
        }
    }
}
