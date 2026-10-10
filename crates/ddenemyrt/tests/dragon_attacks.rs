//! Installed motion/character-parameter evidence; no original replay claim.
use ddenemyrt::dragon::{Dragon, DragonData, DragonEffect, DragonError, DragonStep, attacks::*};

#[test]
fn installed_collision_effect_reads_runtime_fields_and_keeps_spawn_separate_from_fade() {
    use ddenemyrt::dragon::particles::CollisionUnit;
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP collision effect: install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    // Explicit list-slot fixture, not a claim that the native LOD selector
    // or provider's runtime parameter overrides have been implemented.
    let unit = CollisionUnit::load(&vfs, 0).unwrap();
    assert!(unit.capacity > 0 && unit.path_points > 1);
    assert_eq!(unit.generator.flags & 2, 0, "this installed unit is one-shot");
    let mut counter = 0;
    let durations = unit.sample_raw_durations(&mut counter,
        |_| panic!("installed zero-variation ranges must not read RNG table")).unwrap();
    assert_eq!(counter, 3);
    assert_eq!(durations[0], 0);
    assert_eq!(durations[2], 0);
    assert!(durations[1] > 0);
    let mut generator = GeneratorClock::new(counter, 4);
    assert!(generator.update(unit.generator, 0, |_| panic!("no restart RNG")).spawn);
    let mut fade = FadeClock::enter(durations, unit.hold_limit, false);
    for _ in 0..durations[1] {
        assert!(!generator.update(unit.generator, 0, |_| panic!("one-shot")).spawn);
        assert!(fade.update(false, false, false));
    }
    assert_eq!(fade.phase, FadePhase::Done);
    assert!(!fade.update(false, false, false));
    // No particle position or party hit was fabricated by this reader/test.
}

fn step() -> DragonStep<'static> {
    DragonStep {
        engine_step: 1.0,
        motion_step: 1.0,
        root_translation_transform: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        think_target_y: 0.0,
        motion_flags: 0,
        yaw_enabled: true,
        movement_condition_flags: 0,
        yaw_slow_factor: 1.0,
        external_displacement: [0.0; 3],
        acceleration: [0.0; 3],
        horizontal_status_multipliers: [None; 2],
        velocity_min: [f32::NEG_INFINITY; 3],
        velocity_max: [f32::INFINITY; 3],
        controller_flag_c: true,
        actor_flags: 0,
        notify_attached: false,
        attached_owners: &[],
    }
}

#[test]
fn installed_breath_particle_entries_use_native_sequence_metadata_and_normal_contacts() {
    use ddcombat::volumes::{contacts, AttackFlags, HurtVolume, Side, Volume};
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let data = DragonData::opening(&vfs, &exe.image().unwrap(), 1.0).unwrap();
    // Explicit native primary-resource fixture, selected by binding ordinal1.
    let primary = &data.body.collision[1].body;
    let reference = &primary.seq_index[1];
    let group = &primary.groups[reference.group_no as usize];
    let row = primary.attacks[reference.attack_no as usize].as_ref().unwrap();
    // Synthetic positions isolate the source collision-entry adapter. They
    // are not claimed to be the installed effect's runtime trajectory.
    let samples = [[100.0, 200.0, 300.0, 40.0]; 16];
    let attacks = ddenemyrt::dragon::particles::collision_attacks(primary, None, 42, &samples).unwrap();
    assert_eq!(attacks.len(), 15);
    let attack = &attacks[0];
    assert_eq!(attack.volume, Volume::Sphere { c: [100.0, 200.0, 300.0], r: 40.0 });
    assert_eq!(attack.side, Side { owner: 42, kind: group.kind });
    assert_eq!(attack.flags, AttackFlags::from_attack(row));
    assert_eq!(attack.row, ddcombat::damage::AttackRow::from_attack(row));
    assert_eq!(attack.key, ((primary.seq_index[0].no << 8) | group.no) << 16);
    assert_eq!(attack.relation, primary.seq_index[0].relation);
    let target = |center| HurtVolume { side: Side { owner: 0, kind: 0 },
        volume: Volume::Sphere { c: center, r: 10.0 }, region_id: 0, hit_prio: 0 };
    assert!(!contacts(&attacks, &[target([100.0, 200.0, 300.0])]).is_empty());
    assert!(contacts(&attacks, &[target([1000.0, 200.0, 300.0])]).is_empty());
    assert!(ddenemyrt::dragon::particles::collision_attacks(primary, None, 42, &[[0.0, 0.0, 0.0, -1.0]]).is_err());
}

#[test]
fn installed_scenery_node_sweeps_its_real_posed_horizontal_capsule() {
    use ddcombat::volumes::{Pose, transform_point};
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP dragon scenery: install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let data = DragonData::opening(&vfs, &exe.image().unwrap(), 1.0).unwrap();
    let groups: Vec<_> = data.body.collision.iter().flat_map(|set| &set.body.groups)
        .filter(|group| group.is_scr_adj).collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].nodes.len(), 1);
    let node = &groups[0].nodes[0];
    let origin = [200.0, 300.0, -400.0];
    let mut actor = Dragon::new(&data, 100, origin, 0.0);
    actor.start_press(&data, PressPhase::Waiting, 0.0).unwrap();
    let pose = actor.pose(&data).unwrap();
    let cap = scenery_capsule(node, &pose, origin, 1.0).unwrap();
    for (joint, offset, actual) in [(node.joint0, node.offset0, cap.start),
        (node.joint1, node.offset1, cap.end)] {
        let expected = transform_point(&pose.joint_matrix(joint).unwrap(), offset);
        for i in 0..3 { assert!((actual[i] - (expected[i] - origin[i])).abs() < 0.001); }
    }
    assert_eq!(cap.radius, node.radius);
    let distance = (0..3).map(|i| (cap.end[i] - cap.start[i]).powi(2)).sum::<f32>().sqrt();
    assert!((cap.end[1] - cap.start[1]).abs() < distance * 0.1,
        "installed scenery capsule must be horizontal: {cap:?}");
    // Synthetic plane located below the actual posed capsule: verifies the
    // adapter and source contact projection, not stage terrain or a replay.
    let floor = origin[1] + cap.start[1].min(cap.end[1]) - cap.radius - 40.0;
    let terrain = |_: [f32; 3], _: [f32; 3], out: &mut Vec<ddsim::sweep::Face>| {
        out.push(([[-10000.0, floor, -10000.0], [-10000.0, floor, 10000.0],
            [10000.0, floor, -10000.0]], 0));
        out.push(([[10000.0, floor, -10000.0], [-10000.0, floor, 10000.0],
            [10000.0, floor, 10000.0]], 0));
    };
    let result = ddsim::sweep::sweep_capsule(&Default::default(), &cap, origin,
        [0.0, -100.0, 0.0], false, None, &terrain);
    assert!(ground_contact(false, result.met, false), "{result:?}");
    assert!((result.position[1] - (origin[1] - 40.0)).abs() < 0.2, "{result:?}");
}

#[test]
fn installed_press_parameters_and_primary_motion_events_drive_the_fall() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP dragon attacks: install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe_file = ddexe::ExeFile::next_to(&native).unwrap();
    let data = DragonData::opening(&vfs, &exe_file.image().unwrap(), 1.0).unwrap();
    let params = PressParams::load(&vfs).unwrap();
    let bytes = vfs
        .read("charparam/em/em5800Dragon", mtarc::type_hash("rPropParam"))
        .unwrap();
    let doc = mtprp::parse(&bytes).unwrap().object;
    for (actual, property) in [
        (params.initial_velocity, "プレスATTACKの初速"),
        (params.terminal_velocity, "プレスATTACKの最高速"),
        (params.acceleration, "プレス攻撃の加速度"),
    ] {
        let Some([mtxfs::Value::F(expected)]) = doc.get(&doc.root, property) else {
            panic!()
        };
        assert_eq!(actual, expected[0] as f32);
    }
    let bank = data.body.banks[1].as_ref().unwrap();
    let motion = bank.list.motions[42].as_ref().unwrap(); // 298 = bank1/slot42.
    let events = &bank.list.events[42][0]; // actor primary +0x110.
    let falling_frame = (0..motion.frame_count as i32)
        .find(|&frame| mtlmt::event_bits(events, frame, frame) & 0x2000_0000 != 0)
        .expect("installed Press motion has no native falling event");
    let bits = mtlmt::event_bits(events, falling_frame, falling_frame);
    let mut press = PressAttack::enter(params, PressPhase::Falling, 0.0);
    let out = press.update(
        params,
        PressInput {
            engine_step: 1.0,
            primary_events: bits,
            ground_contact: false,
            motion_frame: falling_frame as f32,
            motion_ended: false,
            notify_landing: true,
            cached_position: [0.0; 3],
        },
    );
    assert_eq!(out.actor_velocity, Some([0.0, params.initial_velocity, 0.0]));
    assert_eq!(
        press.velocity,
        (params.initial_velocity + params.acceleration).max(params.terminal_velocity)
    );
    assert_eq!(press.phase, PressPhase::Falling);
    for slot in [7, 8, 42, 43] {
        // Native 263/264 and 298/299.
        assert!(bank.list.motions[slot].as_ref().unwrap().frame_count > 0);
    }

    // Exercise the actor's real installed clocks and packed bank selection.
    // Contact is an explicit synthetic adapter observation in this test;
    // this is not proof of the production terrain solver or a route replay.
    let mut actor = Dragon::new(&data, 100, [0.0; 3], 0.0);
    actor.start_press(&data, PressPhase::Waiting, 0.0).unwrap();
    assert_eq!(actor.current_action(), Some(33));
    assert_eq!(actor.motion(), Some(ddenemyrt::body::MotionId::new(1, 42)));
    assert_eq!(actor.step(&data, step()), Err(DragonError::MissingTerrainContact));
    actor.press_contact = Some((false, true));
    for _ in 0..motion.frame_count {
        actor.step(&data, step()).unwrap();
        if actor.velocity().unwrap()[1] < 0.0 {
            break;
        }
    }
    assert!(
        actor.velocity().unwrap()[1] < 0.0,
        "installed primary event never starts falling"
    );
    assert!(!actor.done());
    actor.press_contact = Some((true, true));
    let effects = actor.step(&data, step()).unwrap();
    assert!(effects.contains(&DragonEffect::BindMotion {
        id: 299,
        blend: 0.0,
        speed: 1.0
    }));
    assert_eq!(actor.motion(), Some(ddenemyrt::body::MotionId::new(1, 43)));
    let mut cleared = false;
    for _ in 0..=bank.list.motions[43].as_ref().unwrap().frame_count {
        let effects = actor.step(&data, step()).unwrap();
        cleared |= effects.contains(&DragonEffect::Gsf {
            no: 463,
            value: false,
        });
    }
    assert!(actor.done() && cleared);

    actor.start_opening_breath(&data).unwrap();
    assert_eq!(actor.current_action(), Some(21));
    assert_eq!(actor.motion(), Some(ddenemyrt::body::MotionId::new(1, 7)));
    let mut saw_start = false;
    let mut saw_stop = false;
    let total = [7, 8]
        .iter()
        .map(|&slot| bank.list.motions[slot].as_ref().unwrap().frame_count)
        .sum::<u32>();
    for _ in 0..total + 3 {
        let before = actor.frame;
        let effects = actor.step(&data, step()).unwrap();
        if effects.contains(&DragonEffect::BreathStart) {
            assert_eq!(before, 35.0);
            saw_start = true;
        }
        if effects.contains(&DragonEffect::BreathStop) {
            assert_eq!(before, 165.0);
            assert!(effects.contains(&DragonEffect::BreathParticles));
            saw_stop = true;
        }
        if actor.done() {
            break;
        }
    }
    assert_eq!(actor.motion(), Some(ddenemyrt::body::MotionId::new(1, 8)));
    assert!(actor.done() && saw_start && saw_stop);
}
