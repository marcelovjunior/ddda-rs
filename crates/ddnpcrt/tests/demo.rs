use ddnpcrt::demo::{Demo, Inputs, Interface, Request};
use ddnpcrt::schedule::Graphs;
use ddai::fields::Fields;

fn install() -> Option<ddnpcrt::InstallData> {
    let path = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    path.is_dir().then(|| ddnpcrt::InstallData::open(path, "eng").unwrap())
}

fn path(npc: u32) -> String {
    if npc >= 709 { "AI/FSM/Npc/st200/quest/q0000_n709_n710_AI".into() }
    else { format!("AI/FSM/Npc/st200/quest/q0000_n{npc}_AI") }
}

#[test]
fn installed_beach_demo_readiness_waits_for_a_real_npc_body() {
    let Some(data) = install() else { return };
    let mut world = ddstory::World::default();
    world.stage_no = 200;
    let think = ddai::think::Inputs { stage: Some(200), ..Default::default() };
    for npc in 704..=710 {
        let graph = data.graph(&path(npc)).unwrap();
        let mut demo = Demo::new(npc, &path(npc), graph).unwrap();
        for _ in 0..8 {
            let requests = demo.step(&Inputs { think: &think, world: &world,
                npc_ready: false, damage_flag: false, motion_number: None, interface_paused: false }).unwrap();
            for request in requests {
                match request {
                    Request::Interface { command, interface: Interface::Wait, .. } => {
                        assert!(!demo.interface_done(command), "native Wait always returns running4");
                    }
                    Request::World(ddai::think::Request::QuestFlag { .. }) => panic!("intel-only NPC cannot signal readiness"),
                    _ => {},
                }
            }
        }
        let mut readiness = false;
        for _ in 0..8 {
            for request in demo.step(&Inputs { think: &think, world: &world,
                npc_ready: true, damage_flag: false, motion_number: None, interface_paused: false }).unwrap() {
                if let Request::World(ddai::think::Request::QuestFlag { quest, bit, value }) = request {
                    assert_eq!(quest, 0);
                    assert!((10..=14).contains(&bit));
                    assert!(value);
                    world.set_quest_bit(quest as usize, bit as usize, value);
                    readiness = true;
                }
            }
        }
        assert!(readiness, "NPC{npc} must execute its real readiness state");
    }
    for bit in 10..=14 { assert!(world.quest_bit(0, bit)); }
    assert!(!world.quest_bit(0, 1), "Demo cannot start the dragon scene itself");
}

#[test]
fn every_installed_beach_interface_decodes_with_source_fields() {
    let Some(data) = install() else { return };
    fn visit(cluster: &mtfsm::Cluster, kinds: &mut [usize; 4]) {
        for state in &cluster.states {
            for process in &state.processes {
                if process.container == b"SetInterfaceDemo" || process.container == b"SetInterfaceNpc" {
                    let wrapper = process.parameter.as_ref().unwrap().object("Interface").unwrap();
                    let interface = ddnpcrt::demo::decode_interface(wrapper).unwrap();
                    kinds[match interface {
                        Interface::Wait => 0,
                        Interface::Motion(ref motion) => { assert!(motion.number >= 0); 1 },
                        Interface::GotoEx { .. } => 2,
                        Interface::Damage { .. } => 3,
                    }] += 1;
                }
            }
            if let Some(sub) = &state.subcluster { visit(sub, kinds); }
        }
    }
    let mut kinds = [0; 4];
    for npc in 704..=709 {
        visit(data.graph(&path(npc)).unwrap().root.as_ref().unwrap(), &mut kinds);
    }
    assert!(kinds.iter().all(|count| *count > 0), "all four native interfaces are exercised: {kinds:?}");
}

#[test]
fn native_interface_completions_reject_stale_commands_and_waits() {
    let Some(data) = install() else { return };
    let graph = data.graph(&path(709)).unwrap();
    let mut demo = Demo::new(709, &path(709), graph).unwrap();
    let mut world = ddstory::World::default();
    let think = ddai::think::Inputs::default();
    let mut wait_command = None;
    for _ in 0..8 {
        for request in demo.step(&Inputs { think: &think, world: &world, npc_ready: true,
            damage_flag: false, motion_number: None, interface_paused: false }).unwrap() {
            if let Request::Interface { command, interface: Interface::Wait, .. } = request { wait_command = Some(command); }
        }
    }
    let wait_command = wait_command.unwrap();
    assert!(!demo.interface_done(wait_command));
    world.set_quest_bit(0, 1, true); // Explicit authored-trigger fixture.
    let mut motion_command = None;
    for _ in 0..8 {
        for request in demo.step(&Inputs { think: &think, world: &world, npc_ready: true,
            damage_flag: false, motion_number: None, interface_paused: false }).unwrap() {
            if let Request::Interface { command, interface: Interface::Motion(_), .. } = request { motion_command = Some(command); }
        }
    }
    let command = motion_command.unwrap();
    assert_ne!(command, wait_command);
    assert!(!demo.interface_done(wait_command), "replacement cannot inherit old completion");
    // Typed scalar interface-result fixture; no real motion was played here.
    assert!(demo.interface_done(command));
    assert!(!demo.interface_done(command), "completion is consumed once");
}
