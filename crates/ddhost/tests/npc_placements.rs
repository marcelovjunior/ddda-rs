use ddhost::{npc_placements::{self, Placement}, Host};
use ddstory::{Host as _, Story};
use graph_fixture::GraphsWithFault;

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
    let exe_file = ddexe::ExeFile::next_to(&native).unwrap();
    let families = npc_placements::NameFamilies::from_exe(&exe_file.image().unwrap()).unwrap();
    assert_eq!(families.select("nPc_fixture"), Some(false));
    assert_eq!(families.select("MultiNpc_fixture"), Some(true));
    assert_eq!(families.select("Enemy_fixture"), None);
    let vfs = ddvfs::Vfs::open(native).unwrap();
    for (stage, group, npc) in [(200, 1, 39), (200, 27, 1), (100, 4, 304)] {
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
        assert!(found, "installed layout identity {stage}:{group:?}");
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

#[test]
fn later_owner_error_does_not_retry_or_discard_earlier_owner_progress() {
    use ddnpcrt::schedule::{Npc, Request};
    let story = Story::new_game(ddhost::data::NoStoryData);
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story),
        ddhost::opening::OpeningWorld::new(), Box::new(Void));
    host.data.npc = Some(std::sync::Arc::new(GraphsWithFault));
    host.npcs.npcs.insert(1, Npc::applied(1, &GraphsWithFault, "healthy").unwrap());
    let mut broken = Npc::applied(2, &GraphsWithFault, "broken").unwrap();
    broken.done(Some([7.0, 8.0, 9.0]));
    host.npcs.npcs.insert(2, broken);
    let first = host.tick(&Default::default()).unwrap();
    assert_eq!(first.npc_requests, vec![Request::Place { npc: 1, pos: [4.0, 5.0, 6.0], dir: None }],
        "the successful owner's entry callback must survive the later owner's failure");
    assert_eq!(host.npcs.position(1), Some([4.0, 5.0, 6.0]));
    assert!(host.npcs.npcs[&1].action.is_some());
    assert!(host.npcs.npcs[&2].action.is_none());
    assert_eq!(host.npc_parked[&2], [7.0, 8.0, 9.0]);
    assert_eq!(host.npc_errors.len(), 1);
    assert_eq!(host.npc_errors[0].0, 2);
    assert!(host.tick(&Default::default()).unwrap().npc_requests.is_empty(), "entry callback is not replayed");
    assert_eq!(host.npc_errors.len(), 1, "failed graph is halted, owner and diagnostic retained");
}

mod graph_fixture {
    use ddnpcrt::{schedule::Graphs, NpcData};
    use mtfsm::{Parameter, Record};
    pub struct GraphsWithFault;
    impl Graphs for GraphsWithFault {
        fn graph(&self, path: &str) -> Result<std::sync::Arc<mtfsm::Graph>, ddnpcrt::Error> {
            let healthy = path == "healthy";
            Ok(std::sync::Arc::new(mtfsm::Graph {
                owner: b"cAIActionFSMNpc".to_vec(), attribute: 0, conditions: vec![],
                root: Some(mtfsm::Cluster { id: 0, owner_node_unique_id: 0, initial_state: 1,
                    states: vec![mtfsm::State { id: 1, unique_id: 1, owner_id: 0,
                        name: b"owner fixture".to_vec(), setting: 0, user_attribute: 0,
                        from_all: None, links: vec![], subcluster: None,
                        processes: vec![mtfsm::Process {
                            container: if healthy { b"SetInitPosNpc".to_vec() } else { b"SetInterfaceNpc".to_vec() },
                            category: vec![], parameter: healthy.then(|| Record {
                                class_hash: 0, class_name: None,
                                fields: vec![("InitPos".into(), vec![Parameter::Scalar(mtxfs::Value::F(vec![4.0, 5.0, 6.0, 0.0]))])],
                            }),
                        }] }] }),
            }))
        }
    }
    impl NpcData for GraphsWithFault {
        fn talk_set(&self, _: u32) -> Result<std::sync::Arc<ddnpcrt::TalkSet>, ddnpcrt::Error> {
            Err(ddnpcrt::Error::Data("no talk fixture".into()))
        }
    }
}

#[test]
fn placement_action_suspends_normal_graph_and_later_apply_supersedes_default() {
    use ddnpcrt::schedule::Request;
    for explicit in [false, true] {
        let mut story = Story::new_game(ddhost::data::NoStoryData);
        story.stage.stage_no = 200;
        story.world.stage_no = 200;
        let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story),
            ddhost::opening::OpeningWorld::new(), Box::new(Void));
        host.data.npc = Some(std::sync::Arc::new(GraphsWithFault));
        let mut p = placement(1);
        p.authored_action = if explicit { "broken" } else { "healthy" }.into();
        if explicit { host.apply_story_npc(200, p.target, "healthy"); }
        host.load_story_npc_intel(p.clone()).unwrap();
        assert!(host.npcs.npcs[&1].controller.is_some());
        let frame = host.tick(&Default::default()).unwrap();
        assert_eq!(frame.npc_requests, vec![Request::Place { npc: 1, pos: [4.0, 5.0, 6.0], dir: None }]);
        assert_eq!(host.story_npc_action(1).unwrap().2, "healthy");
        assert_eq!(host.story_npc_normal_graph_present(1), Some(true));
        assert!(!host.npcs.npcs.contains_key(&1), "normal graph is suspended, not stepped alongside authored action");
        assert!(host.npc_errors.is_empty(), "unsupported normal fixture must not execute while suspended");
        host.unbind_story_npc(200, p.target);
        assert!(host.story_npc_action(1).is_none());
        assert!(!host.npcs.npcs.contains_key(&1), "unload cannot restore a retired normal owner");
    }
}

#[test]
fn reconciled_materialized_set_preserves_owners_and_retires_removed_placements() {
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story),
        ddhost::opening::OpeningWorld::new(), Box::new(Void));
    let first = placement(1);
    let mut second = placement(2);
    second.target.index = 18;
    host.reconcile_story_npc_intels(200, vec![second.clone(), first.clone()]).unwrap();
    host.npcs.npcs.get_mut(&1).unwrap().done(Some([9.0, 8.0, 7.0]));
    host.reconcile_story_npc_intels(200, vec![first.clone(), second.clone()]).unwrap();
    assert_eq!(host.npcs.position(1), Some([9.0, 8.0, 7.0]));
    assert_eq!(host.story_npc_intels().count(), 2);
    assert!(host.reconcile_story_npc_intels(200, vec![first.clone(), first]).is_err());
    assert_eq!(host.story_npc_intels().count(), 2, "invalid snapshot cannot retire owners");
    let external = ddstory::Target { kind: 5, group: 99, index: 0 };
    host.bind_story_npc(200, external, 3);
    let mut conflicting = placement(3);
    conflicting.target.index = 19;
    assert!(host.reconcile_story_npc_intels(200, vec![conflicting]).is_err());
    assert_eq!(host.story_npc_intels().count(), 2, "external-owner conflict is validated before retirement");
    host.unbind_story_npc(200, external);
    host.reconcile_story_npc_intels(200, vec![second]).unwrap();
    assert!(!host.npcs.npcs.contains_key(&1));
    assert!(host.npcs.npcs.contains_key(&2));
    host.reconcile_story_npc_intels(200, vec![]).unwrap();
    assert_eq!(host.story_npc_intels().count(), 0);
    assert!(host.npcs.npcs.is_empty());
}
