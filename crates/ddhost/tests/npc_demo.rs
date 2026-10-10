use ddhost::{npc_motion::Banks, npc_placements::{group_list, LoadedLayouts}, Host};
use std::sync::Arc;

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
}

fn installed() -> Option<(ddnpcrt::InstallData, ddvfs::Vfs, ddexe::ExeFile)> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { eprintln!("SKIP: DDDA install absent"); return None; }
    Some((ddnpcrt::InstallData::open(&native, "eng").unwrap(),
        ddvfs::Vfs::open(&native).unwrap(), ddexe::ExeFile::next_to(&native).unwrap()))
}

#[test]
fn installed_demo_readiness_requires_current_lifetime_bodies_and_retires_on_unload() {
    let Some((data, vfs, exe)) = installed() else { return };
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.set_sce_no(1025, false);
    story.world.set_lot(200, 1, true);
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(Arc::new(data));
    let groups = group_list(&vfs, 200).unwrap();
    let layouts = LoadedLayouts::read(&vfs, 200, &groups, &[(0, 0)]).unwrap();
    let gate = ddspawn::World { scene: 1025, hour: 12, flag: &|flag| flag == 1 };
    let placements: Vec<_> = layouts.candidates(&gate).placements.into_iter()
        .filter(|p| p.target.group == 3).collect();
    assert_eq!(placements.len(), 7);
    host.reconcile_story_npc_intels(200, placements.clone()).unwrap();
    for _ in 0..16 { host.tick(&Default::default()).unwrap(); }
    assert!(host.npc_errors.is_empty(), "{:?}", host.npc_errors);
    for npc in 704..=710 {
        let owner = host.story_npc_lifetime(npc).unwrap();
        assert!(matches!(host.story_npc_demo_command(owner), Some((_, ddnpcrt::demo::Interface::Wait))));
        assert!(host.story_npc_motion(npc).is_none());
    }
    for bit in 10..=14 { assert!(!host.story.as_ref().unwrap().world.quest_bit(0, bit)); }
    let banks = Arc::new(Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap());
    let old = host.story_npc_lifetime(708).unwrap();
    for npc in 704..=710 {
        assert!(host.bind_story_npc_motion_body(host.story_npc_lifetime(npc).unwrap(), banks.clone()).unwrap());
    }
    host.unbind_story_npc_motion_body(old);
    for _ in 0..16 { host.tick(&Default::default()).unwrap(); }
    for bit in 10..=13 { assert!(host.story.as_ref().unwrap().world.quest_bit(0, bit)); }
    assert!(!host.story.as_ref().unwrap().world.quest_bit(0, 14), "NPC708 still lacks its body");
    host.bind_story_npc_motion_body(old, banks.clone()).unwrap();
    for _ in 0..16 { host.tick(&Default::default()).unwrap(); }
    assert!(host.story.as_ref().unwrap().world.quest_bit(0, 14));
    assert!(!host.story.as_ref().unwrap().world.quest_bit(0, 1), "readiness cannot start the scene");
    assert!(host.npc_errors.is_empty(), "{:?}", host.npc_errors);
    // Real resource/body retirement; changing LOT admission alone is not unload.
    host.reconcile_story_npc_intels(200, vec![]).unwrap();
    assert!(host.story_npc_demo_command(old).is_none());
    host.reconcile_story_npc_intels(200, placements).unwrap();
    assert_ne!(host.story_npc_lifetime(708).unwrap(), old);
    assert!(!host.bind_story_npc_motion_body(old, banks).unwrap());
}

#[test]
fn installed_beach_demo_motion_loops_and_authored_flag_changes_remain_distinct_from_hp() {
    let Some((data, vfs, exe)) = installed() else { return };
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200; story.world.stage_no = 200;
    let mut world = ddhost::opening::OpeningWorld::new(); world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(Arc::new(data));
    let groups = group_list(&vfs, 200).unwrap();
    let layouts = LoadedLayouts::read(&vfs, 200, &groups, &[(0, 0)]).unwrap();
    let gate = ddspawn::World { scene: 1025, hour: 12, flag: &|flag| flag == 1 };
    let placement = layouts.candidates(&gate).placements.into_iter().find(|p| p.npc == 709).unwrap();
    host.load_story_npc_intel(placement).unwrap();
    let owner = host.story_npc_lifetime(709).unwrap();
    let banks = Arc::new(Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap());
    host.bind_story_npc_motion_body(owner, banks).unwrap();
    for _ in 0..16 { host.tick(&Default::default()).unwrap(); }
    host.story.as_mut().unwrap().world.set_quest_bit(0, 1, true); // Explicit authored trigger fixture.
    for _ in 0..900 { host.tick(&Default::default()).unwrap(); }
    let motion = host.story_npc_motion(709).unwrap();
    assert_eq!(motion.authored.number, 2967);
    assert!(motion.loop_count > 0);
    assert!(!motion.observation().action_terminal, "the real loop is not a motion completion");
    let flags = host.story_npc_intel_flags(709).unwrap();
    assert_eq!(flags & 0x0e00_0000, 0x0e00_0000); // NoDamage/LeaveLife/SuperArmor.
    assert_eq!(flags & 0x1000_0000, 0); // Authored Invincible(false).
    host.story.as_mut().unwrap().world.gsf.set(465, true); // Explicit later authored trigger fixture.
    for _ in 0..16 { host.tick(&Default::default()).unwrap(); }
    let flags = host.story_npc_intel_flags(709).unwrap();
    assert_eq!(flags & (0x0e00_0000 | 0x800), 0);
    assert_eq!(flags & (0x2000 | 0x0080_0000), 0x2000 | 0x0080_0000);
    assert!(host.story_npc_death_state(owner).is_none(), "flags are not a damage/death lifecycle");
    assert!(!host.story.as_ref().unwrap().world.npc_app_die.get(709));
    assert!(host.npc_errors.is_empty(), "{:?}", host.npc_errors);
}

#[test]
fn installed_demo_owner_inherits_existing_reynard_npc_interfaces() {
    use ddnpcrt::schedule::Graphs;
    let Some((data, vfs, exe)) = installed() else { return };
    let graph = data.graph(r"ai\fsm\npc\st100\quest\q0002_n304").unwrap();
    assert_eq!(graph.owner, b"cAIActionFSMDemo");
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1410, false);
    let mut world = ddhost::opening::OpeningWorld::new(); world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(Arc::new(data));
    let groups = group_list(&vfs, 100).unwrap();
    let layouts = LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let gate = ddspawn::World { scene: 1410, hour: 12, flag: &|flag| flag == 3 };
    let placement = layouts.candidates(&gate).placements.into_iter().find(|p| p.npc == 304).unwrap();
    host.load_story_npc_intel(placement).unwrap();
    let banks = Arc::new(Banks::load(&vfs, &exe.image().unwrap(), 0, 0).unwrap());
    host.bind_story_npc_motion_body(host.story_npc_lifetime(304).unwrap(), banks).unwrap();
    for _ in 0..20 { host.tick(&Default::default()).unwrap(); }
    assert!(host.npc_errors.is_empty(), "{:?}", host.npc_errors);
    assert_eq!(host.story_npc_motion(304).unwrap().authored.number, 3226);
}

struct DeathGraph {
    data: ddnpcrt::InstallData,
    graph: Arc<mtfsm::Graph>,
    path: String,
}
impl ddnpcrt::schedule::Graphs for DeathGraph {
    fn graph(&self, path: &str) -> Result<Arc<mtfsm::Graph>, ddnpcrt::Error> {
        if path.eq_ignore_ascii_case(&self.path) { Ok(self.graph.clone()) }
        else { ddnpcrt::schedule::Graphs::graph(&self.data, path) }
    }
}
impl ddnpcrt::NpcData for DeathGraph {
    fn talk_set(&self, npc: u32) -> Result<Arc<ddnpcrt::TalkSet>, ddnpcrt::Error> {
        ddnpcrt::NpcData::talk_set(&self.data, npc)
    }
}

#[test]
fn installed_demo_death_motion_uses_native_bank1_resource_and_real_terminal() {
    use ddnpcrt::schedule::Graphs;
    let Some((data, vfs, exe)) = installed() else { return };
    let image = exe.image().unwrap();
    let banks = Arc::new(Banks::load(&vfs, &image, 0, 0).unwrap());
    // Native registration table, not the collision survey tuple: bank1 dm.
    let table = 0x014f_1d30;
    assert_eq!(image.u32(table + 8).unwrap(), 0x100);
    let native_path = image.cstr(image.u32(table + 12).unwrap()).unwrap();
    let expected = mtlmt::parse(&vfs.read(&native_path, mtarc::type_hash("rMotionList")).unwrap()).unwrap();
    for number in [0x109, 0x115] {
        let (resolved, resource) = banks.resolve(number).unwrap();
        assert_eq!(resolved, number as u16);
        let original = expected.motions[(number & 255) as usize].as_ref().unwrap();
        assert_eq!((resource.frame_count, resource.loop_frame), (original.frame_count, original.loop_frame));
    }
    let groups = group_list(&vfs, 200).unwrap();
    let layouts = LoadedLayouts::read(&vfs, 200, &groups, &[(0, 0)]).unwrap();
    let gate = ddspawn::World { scene: 1025, hour: 12, flag: &|flag| flag == 1 };
    let placement = layouts.candidates(&gate).placements.into_iter().find(|p| p.npc == 704).unwrap();
    let mut graph = (*data.graph(&placement.authored_action).unwrap()).clone();
    // Explicit installed-state checkpoint fixture. This changes only the
    // initial state, not authored callbacks, links, motion or completion.
    // It does not claim normal navigation/reaction reaches the death state.
    graph.root.as_mut().unwrap().initial_state = 9;
    let fixture = DeathGraph { data, graph: Arc::new(graph), path: placement.authored_action.clone() };
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200; story.world.stage_no = 200;
    let mut world = ddhost::opening::OpeningWorld::new(); world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(Arc::new(fixture));
    host.load_story_npc_intel(placement.clone()).unwrap();
    let lifetime = host.story_npc_lifetime(704).unwrap();
    host.bind_story_npc_motion_body(lifetime, banks).unwrap();
    host.tick(&Default::default()).unwrap();
    let actor = host.story_npc_motion(704).unwrap();
    assert_eq!(actor.motion, 0x109);
    assert_eq!(actor.frame, 0.0);
    assert!(!actor.observation().action_terminal);
    let old = actor.observation();
    let length = expected.motions[9].as_ref().unwrap().frame_count.saturating_sub(1);
    host.tick(&Default::default()).unwrap(); // Native EventActMotion state0 initializes counters.
    assert_eq!(host.story_npc_motion(704).unwrap().frame, 0.0);
    for _ in 0..length { host.tick(&Default::default()).unwrap(); }
    assert!(host.story_npc_motion(704).unwrap().observation().action_terminal);
    assert!(!host.observe_story_npc_motion(old), "completed command cannot consume a delayed observation");
    assert!(host.npc_errors.is_empty(), "{:?}", host.npc_errors);
    assert!(!host.story.as_ref().unwrap().world.npc_app_die.get(704), "scripted motion is separate from apparent death");
    host.unbind_story_npc(200, placement.target);
    assert!(!host.observe_story_npc_motion(old), "retired lifetime cannot report terminal");
}
