//! Installed basic-arrow data and distance-dependent collision bindings.
//! This is the arrow module gate; the S26 Sim/Fight input gate is separate.

use ddcombat::volumes::{Volume, contacts};
use ddenemyrt::arrow::{Arrow, ArrowAim, ArrowData};

fn data() -> Option<ArrowData> {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !path.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    Some(ArrowData::load(&ddvfs::Vfs::open(path).unwrap()).unwrap())
}

#[test]
fn installed_basic_arrow_reads_its_own_collision_and_falloff() {
    let Some(data) = data() else {
        return;
    };
    let p = data.param;
    assert_eq!(data.collision.body.resource_id, 6);
    assert!(p.speed > p.speed_max && p.acceleration < 0.0);
    assert!(p.gravity_acceleration < 0.0 && p.gravity_min < 0.0);
    assert!(p.death_distance > p.collision_distances[1]);
    // Read both augment values independently; no assumed neutral multiplier.
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    let vfs = ddvfs::Vfs::open(path).unwrap();
    let abilities = ddparam::Ability::from_bytes(
        &vfs.read(
            r"param\pl\other\plabilityparam",
            mtarc::type_hash("rPlAbilityParam"),
        )
        .unwrap(),
    )
    .unwrap();
    let row = &abilities.entries[0x46];
    assert_eq!(data.range.multiplier(&[u32::MAX; 6]), row.float_1);
    assert_eq!(data.range.multiplier(&[0x46; 6]), row.float_0);
    let aim = ArrowAim {
        bow_origin: [17.0, 91.0, 38.0],
        target: [17.0, 91.0, 1038.0],
        accuracy: 0.25,
        cone_random: 0x80000000,
        polar_random: 0x80000000,
        steady: false,
    };
    let mut arrow = Arrow::aimed(&p, 500, 1, aim, 1.0).unwrap();
    assert_eq!(arrow.position, aim.bow_origin);
    assert_eq!(arrow.direction, [0.0, 0.0, 1.0]);
    let near = arrow.volumes(&data);
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].side.owner, 500);
    assert_eq!(near[0].side.kind, 5);
    let raw = data.collision.attacks.as_ref().unwrap().attacks[1]
        .as_ref()
        .unwrap();
    assert_eq!(near[0].row.attack, raw.attack() + data.supplemental[0].attack);
    let mut attacks = vec![near[0].row];
    for distance in p.collision_distances {
        arrow.distance = distance;
        let volume = arrow.volumes(&data);
        assert_eq!(volume.len(), 1);
        attacks.push(volume[0].row);
    }
    assert!(attacks.windows(2).all(|rows| rows[0].attack > rows[1].attack));
    arrow.distance = p.repelled_distance;
    let repelled = arrow.volumes(&data);
    assert_eq!(repelled[0].row.attack, 0.0);
    assert_eq!((repelled[0].row.shrink_type, repelled[0].row.blow_type), (0, 3));
    arrow.distance = 0.0;
    assert!(arrow.step(&p, 1.0));
    assert_eq!(
        arrow.position,
        [
            aim.bow_origin[0],
            aim.bow_origin[1] + p.gravity,
            aim.bow_origin[2] + p.speed
        ]
    );
    assert_eq!(arrow.speed, p.speed + p.acceleration);
    assert_eq!(arrow.gravity, p.gravity + p.gravity_acceleration);
    // Synthetic moving-node fixture executed through original BD8E40,
    // with the attack-row callback suppressed only to isolate geometry.
    arrow.position = [10.0, 80.0, 1100.0];
    arrow.previous_position = [10.0, 76.0, 950.0];
    assert_eq!(
        arrow.volumes(&data)[0].volume,
        Volume::Capsule {
            p0: [10.0, 80.0, 1100.0],
            p1: [10.0, 76.0, 950.0],
            r: 10.0,
        }
    );
    arrow.contact(&p, true, false);
    assert!(!arrow.alive && arrow.volumes(&data).is_empty());
}

#[test]
fn installed_arrow_crosses_the_goblins_ten_metre_hurt_volume_and_can_miss() {
    let Some(data) = data() else {
        return;
    };
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    let vfs = ddvfs::Vfs::open(&path).unwrap();
    let exe = ddexe::ExeFile::next_to(&path).unwrap();
    let goblin_data = ddenemyrt::goblin::GoblinData::load(&vfs, &exe.image().unwrap()).unwrap();
    let mut goblin =
        ddenemyrt::goblin::Goblin::new(&goblin_data, 200, [0.0, 0.0, 1000.0], std::f32::consts::PI);
    goblin.volumes(&goblin_data);
    let Volume::Sphere { c: target, .. } = goblin.fighter.hurts[0].volume else {
        panic!("expected authored goblin sphere");
    };
    // The supplied origin is a synthetic bow-joint fixture. Actual posed
    // bow-joint integration and normal Sim release remain the S26 gate.
    let aim = ArrowAim {
        bow_origin: [target[0], target[1], 0.0],
        target,
        accuracy: 0.25,
        cone_random: 0x80000000,
        polar_random: 0x80000000,
        steady: false,
    };
    let mut aimed = Arrow::aimed(&data.param, 500, 0, aim, 1.0).unwrap();
    let mut missed = Arrow::aimed(
        &data.param,
        501,
        1,
        ArrowAim {
            target: [target[0] + 1000.0, target[1], target[2]],
            ..aim
        },
        1.0,
    )
    .unwrap();
    let mut hit = false;
    for _ in 0..30 {
        if aimed.step(&data.param, 1.0) && !contacts(&aimed.volumes(&data), &goblin.fighter.hurts).is_empty()
        {
            hit = true;
            aimed.contact(&data.param, true, false);
        }
        missed.step(&data.param, 1.0);
        assert!(contacts(&missed.volumes(&data), &goblin.fighter.hurts).is_empty());
    }
    assert!(hit && !aimed.alive);
    assert_eq!((aimed.shooter, missed.shooter), (0, 1));
}

#[test]
fn installed_ordered_shot_projection_uses_basic_runtime_record() {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !path.is_dir() {
        eprintln!("SKIP: no install");
        return;
    }
    let vfs = ddvfs::Vfs::open(path).unwrap();
    let shot = ddenemyrt::arrow::load_shot_param(&vfs).unwrap();
    let bytes = vfs
        .read(
            ddenemyrt::arrow::BOW_PARAM_PATH,
            mtarc::type_hash("rBowActParamList"),
        )
        .unwrap();
    let x = mtxfs::parse(&bytes).unwrap();
    let object = |o: &mtxfs::Object, name| match x.get(o, name).unwrap().first().unwrap() {
        mtxfs::Value::Object(Some(o)) => (**o).clone(),
        _ => panic!("bad {name}"),
    };
    let rows = object(&x.root, "mParamList");
    let basic = object(&rows, "mpArray");
    let list = object(&basic, "shootCtrlList");
    assert_eq!(shot.rows.len(), x.get(&list, "mpArray").unwrap().len());
    assert!(!shot.rows.is_empty());
    assert!(
        shot.rows
            .iter()
            .all(|r| r.angle == [0.0; 2] && r.spread == [0.0; 2])
    );
    // Basic uncharged shell is supported by the independently loaded reader.
    let arrow = ddenemyrt::arrow::ArrowParam::from_list(
        &vfs.read(ddenemyrt::arrow::PARAM_PATH, mtarc::type_hash("rShlParamList"))
            .unwrap(),
        shot.rows[0].group as usize,
        shot.rows[0].index as usize,
    )
    .unwrap();
    assert!(arrow.speed > 0.0);
    let mut scheduler = ddsim::BowShotState::default();
    scheduler.request(&shot);
    assert!(scheduler.start_due(0.25));
    scheduler.begin(0, 0.0, shot.stamina_attack_rate);
    assert_eq!(scheduler.advance(&shot.rows, 0.25), shot.rows);
    assert!(scheduler.complete);
    assert!(shot.reload_speed > 0.0);
    assert!(shot.accuracy_wait > 0.0);
    assert!(shot.weapon_stamina.is_finite());
}

#[test]
fn shot_boundary_requires_real_bow_joint_and_rejects_unsupported_rows() {
    let Some(data) = data() else { return };
    let shot = ddsim::BowShot {
        row: ddsim::attack::BowShotRow {
            wait_frame: 0.0,
            charge_level: 0,
            group: 0,
            index: 0,
            angle: [0.0; 2],
            spread: [0.0; 2],
            shell_wait: 0.0,
        },
        stamina_attack: 0.0,
        accuracy: 0.7,
    };
    let model = ddenemyrt::body::model_matrix([999.0; 3], 0.0);
    let mut pose = ddenemyrt::body::Posed {
        model,
        joints: Vec::new(),
    };
    assert!(
        Arrow::from_shot(
            &data.param,
            1000,
            3,
            shot,
            &pose,
            [20.0, 90.0, 1030.0],
            [0x80000000; 2],
            false,
            1.0
        )
        .is_err()
    );
    pose.joints
        .push((0, ddenemyrt::body::model_matrix([20.0, 90.0, 30.0], 0.0)));
    let arrow = Arrow::from_shot(
        &data.param,
        1000,
        3,
        shot,
        &pose,
        [20.0, 90.0, 1030.0],
        [0x80000000; 2],
        false,
        1.0,
    )
    .unwrap();
    assert_eq!(arrow.position, [20.0, 90.0, 30.0]); // supplied joint, not model/feet
    assert_eq!(arrow.direction, [0.0, 0.0, 1.0]);
    let mut unsupported = shot;
    unsupported.row.group = 9;
    assert!(
        Arrow::from_shot(
            &data.param,
            1001,
            7,
            unsupported,
            &pose,
            [20.0, 90.0, 1030.0],
            [0x80000000; 2],
            false,
            1.0
        )
        .is_err()
    );
}
