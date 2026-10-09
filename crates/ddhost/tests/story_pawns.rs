//! Story recruitment is host-owned, persists, and respects co-op seats.
use ddhost::{Control, Host, Member, ProfileId};
use ddstory::{data::InstallData, scripted::ScriptedHost, Story};

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

#[test]
fn applied_npc_host_requires_binding_and_real_talk_observation() {
    use ddhost::ddnpcrt::schedule::Request as NpcRequest;
    use ddstory::Host as StoryHost;
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() {
        return;
    }
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        world,
        Box::new(Void),
    );
    host.data.npc = Some(std::sync::Arc::new(
        ddhost::ddnpcrt::InstallData::open(native, "eng").unwrap(),
    ));
    let target = ddstory::Target {
        kind: 5,
        group: 1,
        index: 0,
    };
    host.apply_story_npc(200, target, r"ai\fsm\npc\st200\quest\q0002_n001_ai");
    for _ in 0..10 {
        assert!(host.tick(&Default::default()).unwrap().npc_requests.is_empty());
    }
    assert!(host.story_npc_action(1).is_none());
    host.bind_story_npc(200, target, 1);
    let mut guard_seen = false;
    for _ in 0..300 {
        guard_seen |= host
            .tick(&Default::default())
            .unwrap()
            .npc_requests
            .iter()
            .any(|r| matches!(r, NpcRequest::Guard { config, .. } if config.kind == 1));
        assert!(!host.story.as_ref().unwrap().world.gsf.get(381));
    }
    assert!(guard_seen);
    assert_eq!(host.story_npc_normal_graph_present(1), Some(false));
    host.observe_story_npc_talk_end(1);
    let mut finishes = 0;
    for _ in 0..600 {
        for request in host.tick(&Default::default()).unwrap().npc_requests {
            match request {
                NpcRequest::Goto { npc, pos, .. } => host.story_npc_arrived(npc, pos),
                NpcRequest::Finished { .. } => finishes += 1,
                _ => {}
            }
        }
    }
    assert!(host.story.as_ref().unwrap().world.gsf.get(381));
    assert_eq!(finishes, 1);
    assert!(host.story_npc_action(1).unwrap().3);
    assert!(
        !host.world.applied_script_finished(200, &target),
        "graph Finish is not parent ApplyFSM readiness"
    );
}

#[test]
fn installed_q2_goblin_script_indices_are_layout_record_ids() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() {
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let mut groups = ddhost::spawns::group_list(&vfs, 100).unwrap();
    groups.groups.retain(|g| g.slot == 23);
    let cells = groups.groups[0].cells.clone();
    let mut opening = ddhost::opening::OpeningWorld::new();
    let mut indices = std::collections::BTreeSet::new();
    for cell in cells {
        let (_, spawns) = ddspawn::spawns(
            100,
            &groups,
            (cell.m, cell.n),
            &|path| vfs.read(path, mtarc::type_hash("rLayout")).ok(),
            &ddspawn::World {
                scene: 1400,
                hour: 12,
                flag: &|_| true,
            },
            &[],
        )
        .unwrap();
        for spawn in spawns {
            assert_eq!(spawn.class, "cSetInfoEnemy0100");
            let target = opening.bind_spawned_goblin(&spawn, 1);
            assert_eq!(target.index, spawn.record.id as u32);
            indices.insert(target.index);
        }
    }
    assert_eq!(indices, [0, 1, 2].into_iter().collect());
}

#[test]
fn manual_stage_loading_waits_for_actual_completion_without_enabling_actor_facts() {
    use ddhost::StoryWorld;
    let mut world = ddhost::opening::OpeningWorld::new();
    world.manual_stages = true;
    world.handle(
        200,
        &[ddstory::Request::JumpArea {
            stage: 100,
            pos: 2,
            fade: 0,
        }],
    );
    for _ in 0..150 {
        assert!(!world
            .events()
            .iter()
            .any(|e| matches!(e, ddstory::HostEvent::StageLoaded { .. })));
    }
    assert!(world.encounters.is_none());
    world.report(ddstory::HostEvent::StageLoaded { stage: 100 });
    assert!(world
        .events()
        .contains(&ddstory::HostEvent::StageLoaded { stage: 100 }));
}

#[test]
fn live_camera_and_handoff_never_emit_scripted_quest_or_stage_callbacks() {
    use ddhost::StoryWorld;
    let camera = ddstory::Request::Camera {
        simple: false,
        cam_id: 17,
        quest: 0,
    };
    let mut world = ddhost::opening::OpeningWorld::new();
    // Include a callback already queued before activation.
    world.handle(200, &[camera.clone()]);
    world.handle(
        200,
        &[ddstory::Request::JumpArea {
            stage: 100,
            pos: 0,
            fade: 0,
        }],
    );
    world.enable_live_encounters();
    world.handle(
        200,
        &[
            camera.clone(),
            ddstory::Request::JumpArea {
                stage: 210,
                pos: 0,
                fade: 0,
            },
        ],
    );
    for _ in 0..150 {
        assert!(!world.events().iter().any(|event| matches!(
            event,
            ddstory::HostEvent::SetQuestBit { .. }
                | ddstory::HostEvent::SetGsf { .. }
                | ddstory::HostEvent::StageLoaded { .. }
        )));
    }
    let actual = ddstory::HostEvent::StageLoaded { stage: 100 };
    world.report(actual.clone());
    assert!(world.events().contains(&actual));
    let mut fixture = ddhost::opening::OpeningWorld::new();
    fixture.handle(200, &[camera]);
    assert!((0..150).flat_map(|_| fixture.events()).any(|event| matches!(
        event,
        ddstory::HostEvent::SetQuestBit {
            quest: 0,
            bit: 5,
            value: true
        }
    )));
}

#[test]
fn live_encounters_never_complete_applied_scripts_or_enemies_on_a_timer() {
    use ddhost::StoryWorld;
    use ddstory::Host as StoryHost;
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let npc = ddstory::Target {
        kind: 5,
        group: 1,
        index: 0,
    };
    let enemy = ddstory::Target {
        kind: 2,
        group: 23,
        index: 0,
    };
    world.handle(
        200,
        &[
            ddstory::Request::ApplyScript {
                target: npc,
                path: r"ai\fsm\npc\st200\quest\q0002_n001_ai".into(),
            },
            ddstory::Request::Placement {
                target: enemy,
                present: true,
                act: 1,
            },
        ],
    );
    for _ in 0..1000 {
        assert!(!world
            .events()
            .iter()
            .any(|e| matches!(e, ddstory::HostEvent::SetGsf { no: 381, value: true })));
        assert!(!world.enemy_check(200, &enemy, 3));
        assert!(!world.applied_script_finished(200, &npc));
        assert!(!world.unit_exists(200, &enemy));
    }
    world.report(ddstory::HostEvent::SetGsf { no: 381, value: true });
    assert!(world
        .events()
        .contains(&ddstory::HostEvent::SetGsf { no: 381, value: true }));
    let facts = world.encounters.as_mut().unwrap();
    facts.report_unit(
        100,
        enemy,
        ddstory::encounter::UnitFacts {
            layout_loaded: true,
            present: true,
            hp: Some(0.0),
            max_hp: Some(100.0),
            ..Default::default()
        },
    );
    assert!(world.enemy_check(100, &enemy, 3));
    world.unbind_enemy(100, enemy);
    assert!(
        !world.enemy_check(100, &enemy, 3),
        "unloading is not persistent death"
    );
}

#[test]
fn stage_dragon_hook_waits_for_real_observations_and_respects_stage_stop() {
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    assert!(
        story.world.lot(380, 0),
        "new-game initializer 0x00445be1 enables the prologue enemy layout"
    );
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.gsf.set(378, true);
    let mut facts = ddstory::encounter::EncounterFacts::default();
    story.tick(&Default::default(), &facts).unwrap();
    assert_eq!(story.stage.dragon_challenge.baseline_hp, None);
    facts.challenge_dragon = Some(ddstory::dragon::ChallengeDragon { life_hp: Some(100.0) });
    story.stage.game_flags.b8844 = 0x80000;
    story.tick(&Default::default(), &facts).unwrap();
    assert_eq!(story.stage.dragon_challenge.baseline_hp, None);
    story.stage.game_flags.b8844 = 0;
    story.tick(&Default::default(), &facts).unwrap();
    assert_eq!(story.stage.dragon_challenge.baseline_hp, Some(100.0));
    facts.challenge_dragon = Some(ddstory::dragon::ChallengeDragon { life_hp: Some(95.0) });
    story.stage.stage_no = 100;
    story.tick(&Default::default(), &facts).unwrap();
    assert!(!story.world.gsf.get(379));
    story.stage.stage_no = 200;
    story.tick(&Default::default(), &facts).unwrap();
    assert!(story.world.gsf.get(379));
    // A real stage load recreates its challenge baseline.
    story
        .tick(
            &ddstory::Inputs {
                events: vec![ddstory::HostEvent::StageLoaded { stage: 200 }],
                ..Default::default()
            },
            &facts,
        )
        .unwrap();
    assert_eq!(story.stage.dragon_challenge.baseline_hp, None);
}

#[test]
fn rook_record_is_shared_protected_and_saved_without_a_rift_visit() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let story = Story::new_game(InstallData::open(native, None).unwrap());
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        ScriptedHost::default(),
        Box::new(Void),
    );
    let owner = host.join(ProfileId(17), Control::Local, [0.0; 3], 0.0).unwrap();
    let guest = host.join(ProfileId(29), Control::Remote, [0.0; 3], 0.0).unwrap();
    assert!(host.recruit_story_pawn(14, 1).unwrap());
    let rook = host.rift.party.hired[0].as_ref().unwrap();
    assert_eq!(rook.pawn.rom_id, 0x2000_000e);
    assert_eq!(rook.pawn.pawn_type, 2);
    assert_eq!(rook.pawn.param.level, 6);
    assert_eq!(host.party.slots[2], Some(Member::Hired { owner, slot: 1 }));
    assert_eq!(host.party.slots[3], Some(Member::Arisen(guest)));
    let bag = host
        .economy
        .inventory
        .bag(ddeconomy::inventory::Owner::Pawn(1))
        .unwrap()
        .clone();
    assert_eq!(bag.count(64), 1);
    for item in [414, 575, 634, 985, 1062] {
        assert_eq!(bag.count(item), 1);
        assert!(bag.slots.iter().find(|s| s.item == item).unwrap().flags & 2 != 0);
    }
    let saved = host.rift.save();
    assert_eq!(ddrift::Rift::load(&saved).unwrap().party, host.rift.party);
    host.frame += 100;
    assert!(!host.recruit_story_pawn(14, 1).unwrap());
    assert_eq!(
        host.rift.save(),
        saved,
        "duplicate request cannot reset the record"
    );
    assert_eq!(
        host.economy.inventory.bag(ddeconomy::inventory::Owner::Pawn(1)),
        Some(&bag)
    );
    assert!(
        host.recruit_story_pawn(15, 2).is_err(),
        "guest reservation is authoritative"
    );
    assert!(
        host.recruit_story_pawn(15, 1).is_err(),
        "occupied story slot cannot be overwritten"
    );
    assert!(
        host.rift.release(1, 100, true).is_none(),
        "pawn type 2 cannot be lost through death"
    );
}
