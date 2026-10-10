//! Co-op wire decisions: local resource names and authoritative enemy pose.
use ddnet::mirror::{Enemy, EnemyFamily, EnemyIdentity, EnemyRotation};
use ddnet::proto::Snapshot;

fn enemy(id: u32, family: EnemyFamily) -> Enemy {
    Enemy {
        id,
        identity: EnemyIdentity {
            family,
            model: format!("fixture/model/{id}"),
            banks: vec![
                (0, format!("fixture/motion/{id}/co")),
                (2, format!("fixture/motion/{id}/dm")),
            ],
        },
        pos: [1.0, 2.0, 3.0],
        rotation: EnemyRotation::Yaw(0.5),
        scale: [1.0; 3],
        action: 52,
        bank: 0,
        slot: 100,
        frame: 8.25,
        hp: 900.0,
        hp_max: 1000.0,
        dead: false,
    }
}
fn snap(frame: u64, base: Option<u64>, enemies: Vec<Enemy>) -> Snapshot {
    Snapshot {
        frame,
        base,
        cmd_newest: None,
        bodies: vec![],
        vitals: vec![],
        npcs: vec![],
        enemies,
    }
}

#[test]
fn mixed_enemy_whole_and_delta_round_trip_every_transform_component() {
    let goblin = enemy(256, EnemyFamily::Goblin);
    let mut dragon = enemy(257, EnemyFamily::Dragon);
    dragon.rotation = EnemyRotation::Quaternion([0.1, 0.2, 0.3, 0.92736185]);
    dragon.scale = [1.5, 0.75, 2.0];
    let base = snap(1, None, vec![goblin, dragon]);
    let bytes = base.encode(None);
    assert_eq!(Snapshot::decode(&bytes, &|_| None).unwrap(), base);
    let mut next = snap(2, Some(1), base.enemies.clone());
    next.enemies[0].pos[0] = 9.0;
    next.enemies[0].frame = 9.25;
    next.enemies[1].rotation = EnemyRotation::Quaternion([-0.1, 0.2, 0.3, 0.92736185]);
    next.enemies[1].scale[2] = 3.0;
    next.enemies[1].hp = 800.0;
    let bytes = next.encode(Some(&base));
    assert_eq!(Snapshot::decode(&bytes, &|_| Some(base.clone())).unwrap(), next);
    assert!(bytes.len() < next.encode(None).len());
    // The third snapshot drops the goblin and changes the dragon identity:
    // spawn/despawn is determined by the authoritative list, not order.
    let mut changed = next.enemies[1].clone();
    changed.identity.model = "fixture/replacement/model".into();
    changed.identity.banks[0].1 = "fixture/replacement/motion".into();
    let third = snap(3, Some(2), vec![changed]);
    assert_eq!(
        Snapshot::decode(&third.encode(Some(&next)), &|_| Some(next.clone())).unwrap(),
        third
    );
}

#[test]
fn unchanged_and_moving_goblin_deltas_do_not_resend_resource_paths() {
    use ddid::{Reader, Writer};
    let g = enemy(1, EnemyFamily::Goblin);
    let mut w = Writer::new();
    ddnet::mirror::write_enemies(&mut w, std::slice::from_ref(&g), std::slice::from_ref(&g));
    assert_eq!(w.0.len(), 4 + 5);
    let mut moved = g.clone();
    moved.pos[2] = 5.0;
    let mut w = Writer::new();
    ddnet::mirror::write_enemies(&mut w, std::slice::from_ref(&moved), std::slice::from_ref(&g));
    assert_eq!(w.0.len(), 4 + 5 + 12);
    let bytes = w.finish();
    assert_eq!(
        ddnet::mirror::read_enemies(&mut Reader::new(&bytes), std::slice::from_ref(&g)).unwrap(),
        vec![moved]
    );
    assert!(ddnet::mirror::read_enemies(&mut Reader::new(&bytes), &[]).is_err());
}

#[test]
fn interpolation_preserves_flight_rotation_and_avoids_identity_replacement_blend() {
    let mut a = enemy(1, EnemyFamily::Dragon);
    a.rotation = EnemyRotation::Quaternion([0.0, 0.0, 0.0, 1.0]);
    let mut b = a.clone();
    b.rotation = EnemyRotation::Quaternion([0.0, 0.0, -1.0, 0.0]);
    b.pos[1] = 12.0;
    b.scale[0] = 3.0;
    let mid = Enemy::lerp(&a, &b, 0.5);
    let q = mid.rotation.quaternion();
    assert!((q[2].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((q.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
    assert_eq!(mid.pos[1], 7.0);
    assert_eq!(mid.scale[0], 2.0);
    b.identity.model = "replacement".into();
    assert_eq!(Enemy::lerp(&a, &b, 0.25), a);
    assert_eq!(Enemy::lerp(&a, &b, 0.75), b);
}

#[test]
fn unknown_family_survives_wire_without_becoming_a_goblin() {
    let s = snap(1, None, vec![enemy(7, EnemyFamily::Unknown(99))]);
    assert_eq!(Snapshot::decode(&s.encode(None), &|_| None).unwrap(), s);
}

/// Installed host actors and a real UDP guest. The fixture isolates the
/// S29 entrance by opening its authored LOT; it is not a full story route.
#[test]
fn installed_udp_guest_mirrors_dragon_and_goblin_then_dragon_unloads() {
    use ddhost::data::NoStoryData;
    use ddhost::ddenemyrt::fight::{Fight, FightData};
    use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
    use ddnet::{snapshot_hash, Client, Lossy, Server, ServerConfig, Transport, Udp};
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
        sync::Arc,
        time::Duration,
    };
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: no DDDA install");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let dragon_data = ddhost::dragon::OpeningDragonData::load(&vfs, &exe).unwrap();
    let mut story = ddstory::Story::new_game(NoStoryData);
    story.stage.stage_no = 200;
    story.world.stage_no = 200;
    story.world.set_sce_no(1025, false);
    story.world.set_lot(200, 0, true);
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        ddhost::opening::OpeningWorld::new(),
        Box::new(Flat),
    );
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    let mut fight = Fight::new(Arc::new(FightData::load(&vfs, &exe).unwrap()));
    fight.add_arisen(0);
    fight.equip(&mut host.sim, 0);
    fight.add_goblin([0.0, 0.0, 10000.0], 0.0);
    host.fight = Some(fight);
    host.set_opening_dragon_data(dragon_data);
    const BUILD: u32 = 1_513_180_209;
    const DATA: u64 = 0x5333;
    let transport = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), 30, 10, 0.05, 131);
    let config = ServerConfig {
        build: BUILD,
        data: DATA,
        spawn: [500.0, 0.0, 0.0],
        spot: None,
    };
    let mut server = Server::new(host, transport, config);
    let transport = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), 30, 10, 0.05, 137);
    let mut guest = Client::new(
        transport,
        server.transport.local_addr(),
        2,
        BUILD,
        DATA,
        None,
        ddsim::Sim::new(Default::default()),
        Box::new(Flat),
        0,
    );
    let mut expected = BTreeMap::new();
    let mut actions = BTreeSet::new();
    let mut saw_dragon = false;
    let mut saw_flight_rotation = false;
    let mut now = 0u64;
    for _ in 0..1500 {
        server.pump(now);
        guest.pump(now);
        guest.frame(PlayerInput::default(), now);
        if let Some(seat) = guest.seat {
            let fight = server.host.fight.as_mut().unwrap();
            if !fight.arisen.contains_key(&seat) {
                fight.add_arisen(seat);
                fight.equip(&mut server.host.sim, seat);
            }
        }
        server.step(&FrameInputs::new(), now).unwrap();
        let actors = ddnet::mirror::from_fight(server.host.fight.as_ref()).1;
        expected.insert(server.host.frame, actors);
        if let Some(dragon) = server.host.fight.as_ref().unwrap().dragon.as_ref() {
            actions.extend(dragon.actor.current_action());
            saw_flight_rotation |=
                dragon.actor.rotation[0].abs() > 1e-5 || dragon.actor.rotation[2].abs() > 1e-5;
        }
        saw_dragon |= guest
            .enemies(0.0)
            .iter()
            .any(|e| e.identity.family == EnemyFamily::Dragon);
        now += 33;
        std::thread::sleep(Duration::from_micros(300));
        if server
            .host
            .fight
            .as_ref()
            .unwrap()
            .dragon
            .as_ref()
            .unwrap()
            .entrance_complete()
            && actions.len() == 4
            && saw_dragon
        {
            break;
        }
    }
    assert_eq!(guest.seat, Some(1), "two seats joined");
    assert_eq!(
        actions,
        BTreeSet::from([10, 52, 53, 54]),
        "native action getter follows the installed FSM"
    );
    assert!(saw_dragon);
    assert!(
        saw_flight_rotation,
        "installed TargetFly has pitch/roll to preserve"
    );
    for _ in 0..30 {
        server.pump(now);
        guest.pump(now);
        guest.frame(PlayerInput::default(), now);
        server.step(&FrameInputs::new(), now).unwrap();
        expected.insert(
            server.host.frame,
            ddnet::mirror::from_fight(server.host.fight.as_ref()).1,
        );
        now += 33;
        std::thread::sleep(Duration::from_micros(300));
    }
    let latest = guest.latest().unwrap();
    assert_eq!(server.hash_at(latest.frame), Some(snapshot_hash(latest)));
    assert_eq!(&latest.enemies, &expected[&latest.frame]);
    let goblin = latest
        .enemies
        .iter()
        .find(|e| e.identity.family == EnemyFamily::Goblin)
        .unwrap();
    let dragon = latest
        .enemies
        .iter()
        .find(|e| e.identity.family == EnemyFamily::Dragon)
        .unwrap();
    assert_ne!(goblin.identity.model, dragon.identity.model);
    assert!(matches!(goblin.rotation, EnemyRotation::Yaw(_)));
    assert!(matches!(dragon.rotation, EnemyRotation::Quaternion(_)));
    assert!(dragon.hp > 0.0 && dragon.hp_max >= dragon.hp);
    assert_eq!(latest.vitals.len(), 2);
    // Closing the actual host admission gate retires the actor. A guest
    // must remove it instead of calling that retirement a death.
    server.host.story.as_mut().unwrap().world.set_lot(200, 0, false);
    for _ in 0..60 {
        server.pump(now);
        guest.pump(now);
        guest.frame(PlayerInput::default(), now);
        server.step(&FrameInputs::new(), now).unwrap();
        now += 33;
        std::thread::sleep(Duration::from_micros(300));
    }
    assert!(server.host.fight.as_ref().unwrap().dragon.is_none());
    let enemies = &guest.latest().unwrap().enemies;
    assert_eq!(enemies.len(), 1);
    assert_eq!(enemies[0].identity.family, EnemyFamily::Goblin);
}
