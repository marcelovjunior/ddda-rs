//! Installed-data check; no original gameplay comparison is asserted.
// The module is tested by path until its shared lib.rs export is integrated.
use ddenemyrt::body;
use ddenemyrt::combat;
#[path = "../src/dragon.rs"]
mod dragon;
#[path = "../src/enemy_stats.rs"]
mod enemy_stats;

#[test]
fn opening_dragon_definitions_are_loaded_from_the_install() {
    let path = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/DDDA.exe".into());
    if !path.is_file() {
        eprintln!("SKIP dragon definitions: install absent");
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    let exe = ddexe::Exe::parse(&bytes).unwrap();
    let data = dragon::DragonAiData::opening(&exe).unwrap();
    assert_eq!(data.tables.root(0), Some(dragon::OPENING_THINK_ROOT));
    assert_eq!(data.tables.len(), 52);
    assert_eq!(data.action_classes.len(), 93);
    assert_eq!(data.resources.len(), 57);
    assert_eq!(
        data.resources.iter().filter(|r| r.class == "rMotionList").count(),
        5
    );
    assert_eq!(
        data.resources
            .iter()
            .filter(|r| r.class == "rObjCollision")
            .count(),
        3
    );
    assert!(data.resources.iter().all(|r| !r.path.is_empty()));
    assert!(data.resources[..49].iter().all(|r| r.flags == 3));
    assert!(data.resources[49..].iter().all(|r| r.flags == 2));
    assert_eq!(data.resources[4].class, "rModel");
    assert_eq!(data.resources[5].class, "rMotionList");
    assert_eq!(data.resources[44].class, "rRegionStatus");
    // The installed entrance graph's action IDs resolve through this table.
    for (action, class) in [
        (10, "cEm5800TargetFly"),
        (21, "cEm5800BreathWide"),
        (33, "cEm5800PressAttack"),
        (52, "cEm5800HoverUp"),
        (53, "cEm5800KakkuHover"),
        (54, "cEm5800HoverKakku"),
    ] {
        assert_eq!(data.action_classes[action], class);
    }
    // All roots reached by the two nonterminal branches remain resolvable.
    for root in [0x0169_7f38, 0x0169_8b18, 0x0169_5d78] {
        assert_eq!(data.tables.get(root).unwrap().va, root);
    }
}

#[test]
fn registered_dragon_body_contains_the_entrance_motions() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP dragon body: install absent");
        return;
    }
    let exe_file = ddexe::ExeFile::next_to(&native).unwrap();
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let data = dragon::DragonData::opening(&vfs, &exe_file.image().unwrap(), 1.0).unwrap();
    let body = &data.body;
    assert!(!body.skeleton.joints.is_empty());
    assert_eq!(body.banks.len(), 5);
    assert_eq!(body.collision.len(), 3);
    // Action vf05/vf06: a6ba90, a6bd80, a6bf00, a60b70, a6aed0,
    // a6af70, a56a30, a56d10. IDs encode bank << 8 | slot.
    for id in [100u16, 121, 103, 126, 298, 299, 263, 264] {
        let motion = body::MotionId::new(((id >> 8) & 0xf) as u8, id & 0xff);
        assert!(body.frames(motion) > 0, "missing entrance motion {id}");
    }
    let installed = ddenemy::EnemyData::load(&vfs, ddenemy::EnemyId::parse("em5800").unwrap()).unwrap();
    let region = installed
        .regions
        .as_ref()
        .unwrap()
        .regions
        .iter()
        .find(|r| r.kind == 0)
        .unwrap();
    assert_eq!(data.regions.len(), region.elements.len());
    for (pool, element) in data.regions.iter().zip(&region.elements) {
        assert_eq!(
            (pool.id, pool.hp, pool.hp_max),
            (element.id, element.hp_max, element.hp_max)
        );
    }
    let common = &installed.common.as_ref().unwrap().1;
    assert_eq!(data.ai.tables.root(0), Some(dragon::OPENING_THINK_ROOT));
    let reflected = &installed.common.as_ref().unwrap().0.object;
    let Some([mtxfs::Value::U(obj_size)]) = reflected.get(&reflected.root, "OBJ補正サイズ") else {
        panic!()
    };
    assert_eq!(data.obj_size, *obj_size as u32);
    assert_eq!(data.defender.defence, common.defense);
    assert_eq!(data.defender.mgc_defence, common.magic_defense);
    let first = data.fighter(40);
    let mut second = data.fighter(41);
    assert_ne!(first.owner, second.owner);
    let combat::Victim::Enemy { regions, rates, .. } = &mut second.victim else {
        panic!()
    };
    assert_eq!(rates.flags & 1, 1);
    let hp = regions[0].hp;
    ddcombat::damage::apply_hp(&mut regions[0], hp + 100.0, rates, false);
    assert_eq!(regions[0].hp, hp, "opening initialization blocks HP damage");
    regions[0].hp -= 1.0;
    assert_eq!(first.victim.hp(), hp, "owners must not share mutable HP pools");

    // Prefix phase regression: vf06 sees the pre-integration height. A
    // crossing completes on the following phase 91, with no position snap.
    let mut actor = dragon::Dragon::new(&data, 42, [0.0; 3], 0.0);
    let effects = actor
        .start_action(&data, 52, 1.0, dragon::HoverHeight::ThinkTarget)
        .unwrap();
    assert!(effects.contains(&dragon::DragonEffect::BindMotion {
        id: 100,
        blend: 5.0,
        speed: 1.0
    }));
    assert_eq!(
        actor.start_action(&data, 54, 1.0, dragon::HoverHeight::ThinkTarget),
        Err(dragon::DragonError::Unsupported(54))
    );
    assert_eq!(actor.velocity(), Some([0.0, 35.0, 0.0]));
    let input = |notify_attached| dragon::DragonStep {
        engine_step: 1.0,
        motion_step: 0.0,
        root_translation_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        think_target_y: 10.0,
        motion_flags: 0,
        yaw_enabled: true,
        movement_condition_flags: 0,
        yaw_slow_factor: 1.0,
        external_displacement: [0.0; 3],
        acceleration: [0.0; 3],
        horizontal_status_multipliers: [None; 2],
        velocity_min: [-1000.0; 3],
        velocity_max: [1000.0; 3],
        controller_flag_c: false,
        actor_flags: 0,
        notify_attached,
        attached_owners: &[80, 81],
    };
    let effects = actor.step(&data, input(true)).unwrap();
    assert_eq!(
        effects,
        vec![
            dragon::DragonEffect::HoverAirControl,
            dragon::DragonEffect::AttachedMessage {
                owner: 80,
                message: 4
            },
            dragon::DragonEffect::AttachedMessage {
                owner: 81,
                message: 4
            }
        ]
    );
    assert!(!actor.done());
    assert_eq!(actor.pos, [0.0, 35.0, 0.0]);
    assert_eq!(
        actor.step(&data, input(false)).unwrap(),
        vec![dragon::DragonEffect::HoverAirControl]
    );
    assert!(actor.done());
    assert_eq!(actor.pos, [0.0, 35.0, 0.0]);
    let combat::Victim::Enemy { rates, .. } = &actor.fighter.victim else {
        panic!()
    };
    assert_eq!(rates.flags & 0xf, 0);
    // Invalid data must not cancel the running action or rewind its motion.
    let mut missing = data.clone();
    missing.body.banks.clear();
    actor.frame = 7.0;
    assert_eq!(
        actor.start_action(&missing, 52, 1.0, dragon::HoverHeight::ThinkTarget),
        Err(dragon::DragonError::MissingMotion(100))
    );
    assert!(actor.done());
    assert_eq!(actor.frame, 7.0);

    let effects = actor
        .start_action(&data, 53, 1.0, dragon::HoverHeight::ThinkTarget)
        .unwrap();
    assert!(effects.contains(&dragon::DragonEffect::ResetFlightLayer));
    assert!(!actor.done());
    assert_eq!(
        actor.step(&data, input(true)).unwrap(),
        vec![dragon::DragonEffect::HoverAirControl],
        "transitions do not send HoverUp's attached messages"
    );
    assert!(!actor.done());
    let mut end = input(false);
    end.motion_flags = 0x8000_0000;
    actor.step(&data, end).unwrap();
    assert!(actor.done());
    actor
        .start_hover_kakku(
            &data,
            dragon::HoverKakkuTarget::ThinkTarget {
                position: [100.0, 35.0, 0.0],
                speed: 115.0,
            },
        )
        .unwrap();
    assert_eq!(actor.flight_speed_parameters(), Some((115.0, 115.0 * 0.04)));
    assert!(!actor.done());
    actor.step(&data, input(false)).unwrap();
    assert!((actor.yaw - 1.5707964 / 30.0).abs() < 1e-6);
    actor.motion_ended = true;
    actor.step(&data, input(false)).unwrap();
    assert!(actor.done());
    let effects = actor
        .start_action(&data, 53, 1.0, dragon::HoverHeight::ThinkTarget)
        .unwrap();
    assert!(effects.contains(&dragon::DragonEffect::ClearAcceleration));
    let stopped_yaw = actor.yaw;
    actor.step(&data, input(false)).unwrap();
    assert_eq!(actor.yaw, stopped_yaw, "54 exit cancels its yaw controller");
}
