use ddhost::{npc_placements::{self, Placement}, Host};
use ddstory::{Host as _, Story};

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
}

fn placement(npc: i32) -> Placement {
    use mtlot::Value::*;
    Placement::from_record(200, 1, mtlot::Record {
        id: 17, kind: 47, class: "cSetInfoNpc",
        fields: vec![
            ("mNpcId", Int(npc as i64)), ("mUse24Schedule", Int(1)),
            ("FSMPath", Str(vec![])), ("mSetID", Int(999)),
            ("mName", Str(b"NPC".to_vec())), ("mPosition", V3([1.0, 2.0, 3.0])),
            ("mAngle", V3([0.0; 3])), ("mScale", V3([1.0; 3])),
            ("mDrawDistance", F32(1000.0)),
        ],
    }).unwrap()
}

#[test]
fn allocated_intel_survives_missing_graph_but_not_replacement_or_stage_unload() {
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    let p = placement(1);
    assert_eq!(p.target.index, 17, "record id is independent of NPC id and mSetID");
    assert!(host.load_story_npc_intel(p.clone()).unwrap().is_some());
    assert!(host.npcs.npcs[&1].controller.is_none(), "data unavailable, graph absent");
    assert_eq!(host.story_npc_apply_fsm_ready(200, p.target), Some(true));
    assert!(host.world.unit_exists(200, &p.target));
    assert!(host.world.apply_fsm_ready(200, &p.target));
    assert!(host.world.npc_intel(200, &p.target).is_none(), "no invented HP/life observation");
    host.npcs.npcs.get_mut(&1).unwrap().done(Some([4.0, 5.0, 6.0]));
    host.load_story_npc_intel(p.clone()).unwrap();
    assert_eq!(host.npcs.position(1), Some([4.0, 5.0, 6.0]), "same placement is idempotent");
    host.load_story_npc_intel(placement(304)).unwrap();
    assert!(!host.npcs.npcs.contains_key(&1));
    assert!(host.npcs.npcs.contains_key(&304));
    host.story.as_mut().unwrap().stage.stage_no = 100;
    host.story.as_mut().unwrap().world.stage_no = 100;
    host.tick(&Default::default()).unwrap();
    assert!(!host.npcs.npcs.contains_key(&304));
    assert!(!host.world.unit_exists(200, &p.target));
    assert!(!host.world.apply_fsm_ready(200, &p.target));
}

#[test]
fn installed_npc_targets_read_layout_identity_and_schedule_mode() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { return; }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    for (stage, group, npc) in [(200, 1, 39), (100, 4, 304)] {
        let list = npc_placements::group_list(&vfs, stage).unwrap();
        let group = list.groups.iter().find(|g| g.slot == group).unwrap();
        let mut found = false;
        for cell in &group.cells {
            let path = ddspawn::layout_path(stage, cell.m, cell.n, 'n', group.slot);
            let bytes = vfs.read(&path, mtarc::type_hash("rLayout")).unwrap();
            for record in mtlot::parse(&bytes).unwrap() {
                if record.id == 0 && record.class == "cSetInfoNpc" {
                    let p = Placement::from_record(stage, group.slot, record).unwrap();
                    assert_eq!(p.npc, npc);
                    assert!(p.use_24_schedule <= 1);
                    found = true;
                }
            }
        }
        assert!(found, "q2 target {stage}:{group:?}");
    }
}

#[test]
fn unsupported_daily_action_halts_graph_without_destroying_allocated_intel() {
    use ddnpcrt::{schedule::Graphs, NpcData};
    struct BrokenGraph;
    impl Graphs for BrokenGraph {
        fn graph(&self, _: &str) -> Result<std::sync::Arc<mtfsm::Graph>, ddnpcrt::Error> {
            Ok(std::sync::Arc::new(mtfsm::Graph {
                owner: b"cAIConsiderFSM_24Schedule".to_vec(), attribute: 0, conditions: vec![],
                root: Some(mtfsm::Cluster { id: 0, owner_node_unique_id: 0, initial_state: 1,
                    states: vec![mtfsm::State { id: 1, unique_id: 1, owner_id: 0, name: b"broken fixture".to_vec(),
                        setting: 0, user_attribute: 0, from_all: None, links: vec![], subcluster: None,
                        processes: vec![mtfsm::Process { container: b"SetInterfaceNpc".to_vec(),
                            category: vec![], parameter: None }] }] }),
            }))
        }
    }
    impl NpcData for BrokenGraph {
        fn talk_set(&self, _: u32) -> Result<std::sync::Arc<ddnpcrt::TalkSet>, ddnpcrt::Error> {
            Err(ddnpcrt::Error::Data("no talk fixture".into()))
        }
    }
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.sce_no = 1100;
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(std::sync::Arc::new(BrokenGraph));
    let p = placement(2);
    assert!(host.load_story_npc_intel(p.clone()).unwrap().is_none());
    assert!(host.npcs.npcs[&2].controller.is_some());
    for _ in 0..100 { host.tick(&Default::default()).unwrap(); }
    assert_eq!(host.npc_errors.len(), 1);
    assert!(host.npcs.npcs.contains_key(&2));
    assert!(host.npcs.npcs[&2].controller.is_none());
    assert!(host.world.apply_fsm_ready(200, &p.target));
}
