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
