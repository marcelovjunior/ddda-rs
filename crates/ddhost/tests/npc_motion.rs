use ddhost::npc_motion::{Actor, Banks, Token};
use ddnpcrt::schedule::{Clock, Graphs, Motion, Npc, Request};

fn installed() -> Option<(ddnpcrt::InstallData, ddvfs::Vfs, ddexe::ExeFile)> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { eprintln!("SKIP: DDDA install absent"); return None; }
    Some((ddnpcrt::InstallData::open(&native, "eng").unwrap(),
        ddvfs::Vfs::open(&native).unwrap(), ddexe::ExeFile::next_to(&native).unwrap()))
}

fn motions(data: &ddnpcrt::InstallData) -> Vec<Motion> {
    let graph = data.graph(r"ai\fsm\npc\st100\quest\q0002_n304").unwrap();
    graph.root.as_ref().unwrap().states.iter().flat_map(|s| &s.processes)
        .filter_map(|p| p.parameter.as_ref()).filter_map(interface)
        .filter(|i| i.class_name == Some("cAIActionInterfaceNpcSetMotion"))
        .map(|i| Motion::from_record(i).unwrap()).collect()
}

// Read nested reflected objects without adding a host ddai dependency.
fn interface(record: &mtfsm::Record) -> Option<&mtfsm::Record> {
    record.fields.iter().find(|(name, _)| name == "Interface")
        .and_then(|(_, values)| values.first()).and_then(|value| match value {
            mtfsm::Parameter::Object(Some(record)) => Some(&**record), _ => None,
        })
}

fn token(command: u64) -> Token {
    Token { owner: ddhost::story_npcs::NpcLifetime { npc: 304, generation: 1 }, command, launch: command }
}

#[test]
fn installed_reynard_requests_preserve_motion_payload_and_command() {
    let Some((data, _, _)) = installed() else { return };
    let mut npc = Npc::applied(304, &data, r"ai\fsm\npc\st100\quest\q0002_n304").unwrap();
    let mut world = ddstory::World::default();
    let clock = Clock { hour: 12, minute: 0, dt: 1.0 / 30.0 };
    let mut first = None;
    for _ in 0..20 {
        for request in npc.step_observed(&data, clock, Some(&world)).unwrap() {
            if let Request::Motion { command, motion: Some(motion), .. } = request { first = Some((command, motion)); }
        }
    }
    let (first_command, tremble) = first.unwrap();
    assert_eq!(tremble.number, 3226);
    assert!(tremble.looping && tremble.stop && !tremble.fall && !tremble.talk);
    assert_eq!(tremble.attributes(), 0);
    assert_eq!(npc.motion_command(), Some(first_command));
    world.set_quest_bit(2, 20, true); // Explicit fixture, never actor output.
    let mut next = None;
    for _ in 0..20 {
        for request in npc.step_observed(&data, clock, Some(&world)).unwrap() {
            if let Request::Motion { command, motion: Some(motion), .. } = request { next = Some((command, motion)); }
        }
    }
    let (next_command, bow) = next.unwrap();
    assert_eq!(bow.number, 3089);
    assert!(!bow.looping);
    assert!(next_command > first_command);
    assert!(!npc.motion_done(first_command));
    assert!(npc.motion_done(next_command));
    assert!(!npc.motion_done(next_command));
}

#[test]
fn installed_npc_loops_remain_pending_and_nonloops_use_terminal_layer() {
    let Some((data, vfs, exe)) = installed() else { return };
    let banks = Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap();
    let commands = motions(&data);
    for number in [3226, 3073, 3074, 3089] {
        let motion = commands.iter().find(|m| m.number == number).unwrap().clone();
        let (resolved, resource) = banks.resolve(number).unwrap();
        let length = ((resource.frame_count.saturating_sub(1) as f32 - motion.start_frame)
            / motion.speed).ceil() as u32;
        let mut actor = Actor::new(token(number as u64), motion.clone(), resolved, resource).unwrap();
        actor.advance(1.0).unwrap(); // sourced action state0, no terminal read
        assert!(!actor.observe(actor.observation()));
        for _ in 0..length.saturating_sub(1) {
            actor.advance(1.0).unwrap();
            assert!(!actor.observe(actor.observation()), "early completion of {number}");
        }
        actor.advance(1.0).unwrap();
        if motion.looping {
            assert_eq!(actor.loop_count, 1);
            assert!(!actor.observe(actor.observation()));
            let mut invalid_terminal = actor.observation(); invalid_terminal.action_terminal = true;
            assert!(!actor.observe(invalid_terminal), "loop action cannot report non-loop terminal");
            for _ in 0..length * 2 { actor.advance(1.0).unwrap(); }
            assert!(!actor.observe(actor.observation()));
        } else {
            let mut terminal = actor.observation();
            if motion.target {
                assert!(!actor.observe(terminal), "terminal motion must still wait for authored target angle");
                actor.target([0.0; 3], 0.0, None);
                assert!(!actor.observe(actor.observation()), "missing actual target remains pending");
                actor.target([0.0; 3], 0.0, Some([100.0, 0.0, 0.0]));
                terminal = actor.observation();
            }
            assert!(actor.observe(terminal), "motion {number}: {actor:?}");
            assert!(!actor.observe(actor.observation()), "completion is consumed once");
        }
    }
}

#[test]
fn motion_completion_rejects_stale_owner_command_action_and_wrong_motion() {
    let Some((data, vfs, exe)) = installed() else { return };
    let banks = Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap();
    let motion = motions(&data).into_iter().find(|m| m.number == 3073).unwrap();
    let (resolved, resource) = banks.resolve(motion.number).unwrap();
    let mut actor = Actor::new(token(2), motion, resolved, resource).unwrap();
    let mut valid = actor.observation(); valid.action_terminal = true;
    for invalid in [
        { let mut o = valid; o.token.owner.generation += 1; o },
        { let mut o = valid; o.token.command += 1; o },
        { let mut o = valid; o.token.launch += 1; o },
        { let mut o = valid; o.event_action = false; o },
        { let mut o = valid; o.motion += 1; o },
    ] { assert!(!actor.observe(invalid)); }
    actor.authored.target = true;
    assert!(!actor.observe(valid));
    valid.target_angle_reached = true;
    assert!(actor.observe(valid));
}

#[test]
fn sourced_layer_pause_and_target_radius_boundaries_do_not_fake_completion() {
    let Some((data, vfs, exe)) = installed() else { return };
    let banks = Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap();
    let mut motion = motions(&data).into_iter().find(|m| m.number == 3073).unwrap();
    let (resolved, resource) = banks.resolve(motion.number).unwrap();
    motion.speed = 0.0;
    let mut frozen = Actor::new(token(1), motion.clone(), resolved, resource).unwrap();
    for _ in 0..resource.frame_count * 2 { frozen.advance(1.0).unwrap(); }
    assert_eq!(frozen.frame, 0.0);
    assert!(!frozen.observe(frozen.observation()));
    motion.start_frame = resource.frame_count.saturating_sub(1) as f32;
    let mut at_end = Actor::new(token(2), motion.clone(), resolved, resource).unwrap();
    at_end.advance(1.0).unwrap(); at_end.advance(0.0).unwrap();
    assert!(at_end.observe(at_end.observation()), "frozen layer at its terminal frame sets the real flag");
    motion.looping = true; motion.target = true; motion.arrive_length = 100.0;
    let mut target = Actor::new(token(3), motion, resolved, resource).unwrap();
    target.target([0.0; 3], 0.0, Some([100.0, 0.0, 0.0]));
    assert!(!target.observe(target.observation()), "arrival radius is strict and loop remains pending");
    target.target([0.0; 3], 0.0, Some([99.0, 0.0, 0.0]));
    assert!(target.observe(target.observation()), "source target arrival is independent of motion terminal");
}

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
}

#[test]
fn installed_host_reynard_moves_past_real_motion_waits_only_with_a_body_and_target() {
    let Some((data, vfs, exe)) = installed() else { return };
    use std::sync::Arc;
    let banks = Arc::new(Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap());
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 100; story.world.stage_no = 100;
    story.world.set_sce_no(1410, true);
    let mut world = ddhost::opening::OpeningWorld::new(); world.enable_live_encounters();
    let mut host = ddhost::Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(Arc::new(data));
    let groups = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let gates = ddspawn::World { scene: 1410, hour: 12, flag: &|flag| flag == 3 };
    let placement = layouts.candidates(&gates).placements.into_iter()
        .find(|p| p.npc == 304 && p.target.group == 4 && p.target.index == 0).unwrap();
    let target = placement.target;
    host.load_story_npc_intel(placement.clone()).unwrap();
    let owner = host.story_npc_lifetime(304).unwrap();
    for _ in 0..90 { host.tick(&Default::default()).unwrap(); }
    assert!(host.story_npc_motion(304).is_none(), "intel-only requests do not fabricate a body");
    assert!(host.bind_story_npc_motion_body(owner, banks.clone()).unwrap());
    for _ in 0..900 { host.tick(&Default::default()).unwrap(); }
    assert_eq!(host.story_npc_motion(304).unwrap().authored.number, 3226);
    assert!(host.story_npc_motion(304).unwrap().loop_count > 0);
    host.story.as_mut().unwrap().world.set_quest_bit(2, 20, true); // Explicit quest fixture.
    for _ in 0..900 { host.tick(&Default::default()).unwrap(); }
    let bow = host.story_npc_motion(304).unwrap();
    assert_eq!(bow.authored.number, 3089);
    assert!(bow.observation().action_terminal);
    assert!(!bow.observation().target_angle_reached, "absent party target keeps source interface pending");
    let old = bow.observation();
    host.unbind_story_npc_motion_body(owner);
    host.bind_story_npc_motion_body(owner, banks.clone()).unwrap();
    assert!(!host.observe_story_npc_motion(old), "recreated body rejects its predecessor's completion");
    let first = host.join(ddhost::ProfileId(1), ddhost::Control::Local, [0.0; 3], 0.0).unwrap();
    let other = host.join(ddhost::ProfileId(2), ddhost::Control::Local, [100.0, 0.0, 100.0], 0.0).unwrap();
    assert_ne!(first, other);
    for _ in 0..900 { host.tick(&Default::default()).unwrap(); }
    let idle = host.story_npc_motion(304).unwrap();
    assert_eq!(idle.authored.number, 3074, "actual 3089 and 3073 terminal layers advance installed graph");
    assert!(idle.authored.looping && idle.authored.talk);
    assert!(idle.loop_count > 0);
    assert_eq!(host.story_npc_intel_flags(304).unwrap() & 0x20, 0x20);
    assert!(host.npc_errors.is_empty(), "{:?}", host.npc_errors);
    host.unbind_story_npc(100, target);
    host.load_story_npc_intel(placement).unwrap();
    assert!(!host.bind_story_npc_motion_body(owner, banks).unwrap());
    assert!(!host.observe_story_npc_motion(old));
    assert!(host.story_npc_motion(304).is_none());
}
