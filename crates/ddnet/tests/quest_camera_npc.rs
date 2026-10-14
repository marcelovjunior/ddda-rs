use ddid::{Reader, Writer};
use ddnet::{
    npc::{Combat, Motion, NpcBody},
    proto::Snapshot,
    quest_camera::QuestCamera,
};

fn snapshot(frame: u64, npcs: Vec<NpcBody>, quest_camera: Option<QuestCamera>) -> Snapshot {
    Snapshot {
        death: Default::default(),
        epoch: 0,
        frame,
        stage: Some(100),
        base: None,
        cmd_newest: None,
        bodies: vec![],
        vitals: vec![],
        enemies: vec![],
        npcs,
        quest_camera,
    }
}
fn actor() -> NpcBody {
    let mut npc = NpcBody::position(304, [12.0, 34.0, 56.0]);
    npc.generation = 17;
    npc.stage = 100;
    npc.yaw = 0.75;
    npc.scale = [1.0, 1.2, 0.8];
    npc.gauge = true;
    npc.motion = Some(Motion {
        command: 4,
        launch: 9,
        bank: 12,
        slot: 154,
        frame: 7.25,
        blend_frames: 6.0,
    });
    npc.combat = Some(Combat {
        hp: 2400.0,
        hp_max: 2500.0,
        death_phase: None,
        alpha: 1.0,
        retired: false,
    });
    npc
}
#[test]
fn whole_delta_replacement_and_explicit_end_keep_host_state() {
    let camera = QuestCamera {
        stage: 100,
        camera_id: 12,
        generation: 1,
        seconds: 0.5,
        input_locked: true,
    };
    let base = snapshot(10, vec![actor(), NpcBody::position(2, [0.0; 3])], Some(camera));
    assert_eq!(Snapshot::decode(&base.encode(None), &|_| None).unwrap(), base);
    let mut next = base.clone();
    next.frame += 1;
    next.base = Some(base.frame);
    next.npcs[0].combat.as_mut().unwrap().hp = 0.0;
    next.npcs[0].combat.as_mut().unwrap().death_phase = Some(1);
    next.npcs[0].motion.as_mut().unwrap().slot = 198;
    next.npcs[0].motion.as_mut().unwrap().launch += 1;
    next.quest_camera.as_mut().unwrap().seconds = 2.0;
    let delta = next.encode(Some(&base));
    assert!(delta.len() < next.encode(None).len());
    assert_eq!(Snapshot::decode(&delta, &|_| Some(base.clone())).unwrap(), next);
    let mut end = next.clone();
    end.frame += 1;
    end.base = Some(next.frame);
    end.quest_camera = None;
    end.stage = Some(200);
    end.npcs[0].combat.as_mut().unwrap().retired = true;
    end.npcs[0].combat.as_mut().unwrap().death_phase = Some(4);
    end.npcs[0].combat.as_mut().unwrap().alpha = 0.0;
    assert_eq!(
        Snapshot::decode(&end.encode(Some(&next)), &|_| Some(next.clone())).unwrap(),
        end
    );
    let mut replacement = snapshot(
        13,
        vec![actor()],
        Some(QuestCamera {
            generation: 2,
            ..camera
        }),
    );
    replacement.npcs[0].generation += 1;
    replacement.base = Some(end.frame);
    assert_eq!(
        Snapshot::decode(&replacement.encode(Some(&end)), &|_| Some(end.clone())).unwrap(),
        replacement
    );
    assert!(Snapshot::decode(&delta, &|_| None).is_err());
}
#[test]
fn bad_masks_partial_lifetimes_duplicate_ids_and_invalid_values_are_rejected() {
    let base = actor();
    for mask in [32, 255, 0] {
        let mut w = Writer::new();
        w.len(1).u32(base.id).u8(mask);
        assert!(ddnet::npc::read(&mut Reader::new(&w.finish()), &[]).is_err());
    }
    let mut w = Writer::new();
    w.len(1)
        .u32(base.id)
        .u8(2)
        .u64(base.generation + 1)
        .u32(100)
        .f32(0.0);
    for _ in 0..3 {
        w.f32(1.0);
    }
    assert!(
        ddnet::npc::read(&mut Reader::new(&w.finish()), &[base])
            .unwrap_err()
            .contains("partial replacement")
    );
    for kind in 0..4 {
        let mut bad = base;
        match kind {
            0 => bad.pos[1] = f32::NAN,
            1 => bad.motion.as_mut().unwrap().bank = 16,
            2 => bad.combat.as_mut().unwrap().death_phase = Some(5),
            _ => bad.combat.as_mut().unwrap().alpha = f32::INFINITY,
        }
        let mut w = Writer::new();
        ddnet::npc::write(&mut w, &[bad], &[]);
        assert!(ddnet::npc::read(&mut Reader::new(&w.finish()), &[]).is_err());
    }
    let mut w = Writer::new();
    ddnet::npc::write(&mut w, &[base, base], &[]);
    assert!(ddnet::npc::read(&mut Reader::new(&w.finish()), &[]).is_err());
    let bytes = snapshot(1, vec![base], None).encode(None);
    for end in 0..bytes.len() {
        assert!(Snapshot::decode(&bytes[..end], &|_| None).is_err());
    }
    let mut trailing = bytes;
    trailing.push(0);
    assert!(Snapshot::decode(&trailing, &|_| None).is_err());
}
#[test]
fn position_only_npcs_and_absent_cameras_remain_position_only() {
    let npc = NpcBody::position(2, [1.0, 2.0, 3.0]);
    let s = snapshot(1, vec![npc], None);
    let decoded = Snapshot::decode(&s.encode(None), &|_| None).unwrap();
    assert_eq!(decoded, s);
    let mut no_story = s.clone();
    no_story.stage = None;
    assert_eq!(
        Snapshot::decode(&no_story.encode(None), &|_| None).unwrap(),
        no_story
    );
    assert!(decoded.npcs[0].motion.is_none() && decoded.npcs[0].combat.is_none());
    for seconds in [f32::NAN, f32::INFINITY, -1.0] {
        let s = snapshot(
            1,
            vec![],
            Some(QuestCamera {
                stage: 100,
                camera_id: 12,
                generation: 1,
                seconds,
                input_locked: true,
            }),
        );
        assert!(Snapshot::decode(&s.encode(None), &|_| None).is_err());
    }
    assert_eq!(ddnet::proto::PROTOCOL, 12);
}

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> {
        Some(below.min(0.0))
    }
}
#[test]
fn installed_loopback_camera12_and_npc_hp_motion_follow_host() {
    use ddhost::{Control, Host, ProfileId};
    use ddnet::{Client, Server, ServerConfig, Transport, Udp};
    use std::sync::Arc;
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let image = exe.image().unwrap();
    let body = Arc::new(ddhost::ddenemyrt::npc_body::NpcBody::load(&vfs, &image, 304, 304, 0, 0, 0).unwrap());
    let data = Arc::new(ddhost::ddenemyrt::fight::FightData::load(&vfs, &image).unwrap());
    let region = ddhost::ddenemyrt::npc_combat::ordinary_region(
        &vfs,
        &data.tables.game,
        ddhost::ddenemyrt::npc_combat::ORDINARY_NPC_HP,
    )
    .unwrap();
    let mut story = ddstory::Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1410, false);
    story.world.set_clock(12, 0);
    story.world.set_lot(100, 3, true);
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut cameras = ddhost::quest_camera::Cameras::default();
    cameras.load(&vfs, 100).unwrap();
    world.cameras = Some(cameras);
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        world,
        Box::new(Flat),
    );
    host.data.npc = Some(Arc::new(
        ddhost::ddnpcrt::InstallData::open(&native, "eng").unwrap(),
    ));
    host.fight = Some(ddhost::ddenemyrt::fight::Fight::new(data));
    let groups = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let gates = ddhost::ddspawn::World {
        scene: 1410,
        hour: 12,
        flag: &|f| f == 3,
    };
    let placement = layouts
        .candidates(&gates)
        .placements
        .into_iter()
        .find(|p| p.npc == 304)
        .unwrap();
    host.load_story_npc_intel(placement).unwrap();
    let lifetime = host.story_npc_lifetime(304).unwrap();
    host.bind_story_npc_motion_body(
        lifetime,
        Arc::new(ddhost::npc_motion::Banks::load(&vfs, &image, 0, 0).unwrap()),
    )
    .unwrap();
    let owner = host
        .bind_story_npc_combat(lifetime, body.clone(), region, body.resolve(3226).unwrap())
        .unwrap()
        .unwrap();
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    let mut server = Server::new(
        host,
        Udp::bind("127.0.0.1:0").unwrap(),
        ServerConfig {
            build: 1,
            data: 2,
            spawn: [0.0; 3],
            spot: None,
        },
    );
    let mut guest = Client::new(
        Udp::bind("127.0.0.1:0").unwrap(),
        server.transport.local_addr(),
        2,
        1,
        2,
        None,
        ddsim::Sim::new(Default::default()),
        Box::new(Flat),
        0,
    );
    let mut now = 0;
    for _ in 0..120 {
        guest.frame(Default::default(), now);
        server.pump(now);
        server.step(&Default::default(), now).unwrap();
        guest.pump(now);
        if guest.stage() == Some(100) && !guest.ground_ready() { guest.set_ground(Box::new(Flat)); }
        now += 33;
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    assert!(guest.seat.is_some());
    assert_eq!(guest.stage(), Some(100));
    let received = guest.npcs().into_iter().find(|n| n.id == 304).unwrap();
    assert_eq!(received.generation, lifetime.generation);
    assert!(received.motion.is_some());
    assert!(received.gauge, "installed Reynard action requested SetGaugeDisp");
    assert_eq!(received.combat.unwrap().hp_max, 2500.0);
    let npc = server.host.fight.as_mut().unwrap().npcs.get_mut(&owner).unwrap();
    let ddhost::ddenemyrt::combat::Victim::Npc { region, .. } = &mut npc.fighter.victim else {
        unreachable!()
    };
    region.hp -= 100.0; // Explicit HP fixture tests replication, not a natural-hit acceptance claim.
    server
        .host
        .world
        .cameras
        .as_mut()
        .unwrap()
        .request(100, 12, false)
        .unwrap();
    let mut saw_camera = false;
    let mut saw_end = false;
    for _ in 0..180 {
        guest.frame(
            ddhost::PlayerInput {
                sim: ddsim::Input {
                    stick: [1.0, 0.0],
                    light: true,
                    jump: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            now,
        );
        server.pump(now);
        server.step(&Default::default(), now).unwrap();
        guest.pump(now);
        if let Some(c) = guest.quest_camera() {
            saw_camera = true;
            assert_eq!((c.stage, c.camera_id), (100, 12));
            assert!(guest.actions_blocked());
            assert_eq!(c, guest.latest().unwrap().quest_camera.unwrap());
        } else if saw_camera {
            saw_end = true;
            assert!(!guest.actions_blocked());
        }
        let received = guest.npcs().into_iter().find(|n| n.id == 304).unwrap();
        assert_eq!(received.combat.unwrap().hp, 2400.0);
        let actual = ddnet::npc::of(&server.host)
            .into_iter()
            .find(|n| n.id == 304)
            .unwrap();
        assert_eq!(received, actual);
        now += 33;
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
    assert!(
        saw_camera && saw_end,
        "real installed camera clock exhausted on host"
    );
    assert!(server.host.world.cameras.as_ref().unwrap().active.is_none());
}
