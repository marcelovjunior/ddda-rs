use ddenemyrt::cyclops::*;

fn target(id: u32, position: [f32; 3]) -> ThinkTarget {
    ThinkTarget {
        id,
        position,
        radius: 0.0,
        flags: [0; 2],
        action_flags: 0,
    }
}

fn candidate(id: u32, position: [f32; 3], score: f32) -> SensorCandidate {
    let mut scores = [None; 32];
    scores[0] = Some(score);
    SensorCandidate {
        target: target(id, position),
        enabled: true,
        scores,
    }
}

#[test]
fn sensor_uses_score_not_distance_and_preserves_first_tie() {
    let near = candidate(1, [0.0, 0.0, 1.0], 5.0);
    let far = candidate(2, [0.0, 0.0, 1000.0], 6.0);
    assert_eq!(
        sensor_target(&[near.clone(), far.clone()], false),
        Some(far.target)
    );
    let mut tie = near.clone();
    tie.scores[1] = Some(1.0);
    assert_eq!(
        sensor_target(&[tie.clone(), far.clone()], false),
        Some(tie.target)
    );
    tie.target.flags[0] = 0x10000;
    assert_eq!(
        sensor_target(&[tie.clone(), far.clone()], false),
        Some(far.target)
    );
    assert_eq!(sensor_target(&[tie.clone(), far.clone()], true), Some(tie.target));
    tie.target.flags[1] = 0x20;
    assert_eq!(sensor_target(&[tie, far.clone()], true), Some(far.target));
    assert!(sensor_target(&[candidate(1, [0.0; 3], 0.0)], false).is_none());
    assert!(sensor_target(&[candidate(0, [0.0; 3], 100.0)], false).is_none());
    let mut slots = vec![candidate(1, [0.0; 3], 0.0); 16];
    slots.push(far);
    assert!(sensor_target(&slots, false).is_none());
}

#[test]
fn local_checks_match_thresholds_and_attached_vocation_overlap() {
    let mut state = LocalChecks {
        attachment_count: 3,
        attached_vocations: vec![14, 18],
        elapsed_frames: 100.0,
        counter: 4,
        position: [10.0, 20.0, 30.0],
        secondary_target: Some([13.0, -500.0, 34.0]),
    };
    assert!(state.check(0, 3));
    assert!(!state.check(0, 4));
    for group in [1, 2, 4] {
        assert!(state.check(1, group));
    }
    for group in [0, 3, 5] {
        assert!(!state.check(1, group));
    }
    assert!(state.check(2, 100));
    assert!(!state.check(2, 101));
    assert!(state.check(3, 4));
    assert!(!state.check(3, 5));
    assert!(state.check(4, 5)); // XZ only, inclusive.
    assert!(!state.check(4, 4));
    state.secondary_target = None;
    assert!(!state.check(4, u32::MAX));
    assert!(!state.check(5, 0));
    state.attached_vocations = vec![4, 13, 0];
    for group in [0, 3, 4] {
        assert!(state.check(1, group));
    }
    assert_eq!(think_flags(1, u32::MAX, false), (u32::MAX & !(7 | 128)) | 2);
    assert_eq!(think_flags(2, 0, true), 4 | 128);
    assert_eq!(think_flags(5, 0, false), 1);
}

#[test]
fn elapsed_checks_remain_per_actor_and_q0003_weapon_suppresses_accumulation() {
    let initialization = ThinkInitialization {
        tables: ddai::thinktbl::Tables::default(),
        current: 1,
        default: 1,
        base: 1,
        lock: false,
    };
    let mut ordinary = ThinkDriver::new(&initialization, 1.0, 0);
    let mut q0003 = ordinary.clone();
    for _ in 0..399 {
        ordinary.advance_elapsed(4.5, false, 1);
        q0003.advance_elapsed(4.5, false, 2);
    }
    assert!(!ordinary.local.check(2, 1800));
    ordinary.advance_elapsed(4.5, false, 1);
    assert!(ordinary.local.check(2, 1800));
    assert_eq!(q0003.local.elapsed_frames, 0.0);
    ordinary.advance_elapsed(1.0, true, 1);
    assert_eq!(ordinary.local.elapsed_frames, 0.0);
}

/// Unimplemented world answers fail visibly instead of becoming false.
struct UnknownWorld;
impl ddai::thinktbl::Host for UnknownWorld {
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, ddai::Error> {
        Err(ddai::Error::Unimplemented(format!(
            "cyclops world check {id}({arg})"
        )))
    }
    fn set(&mut self, id: u32, arg: u32) -> Result<u32, ddai::Error> {
        Err(ddai::Error::Unimplemented(format!(
            "cyclops world set {id}({arg})"
        )))
    }
    fn rand(&mut self) -> u32 {
        0
    }
}

/// Controlled observations, not a claim about stage100's live services.
/// 0x0059f8e0 returns0 when FieldWayPoint/NavigationMesh are absent; the
/// common setters return0 and clear path-result flags (0x008ab3b0/5b0).
struct DisabledNavigation;
impl ddai::thinktbl::Host for DisabledNavigation {
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, ddai::Error> {
        match id {
            1016 | 1017 => Ok(false),
            _ => UnknownWorld.check(id, arg),
        }
    }
    fn set(&mut self, id: u32, arg: u32) -> Result<u32, ddai::Error> {
        match id {
            1002 | 1003 => Ok(0),
            _ => UnknownWorld.set(id, arg),
        }
    }
    fn rand(&mut self) -> u32 {
        0
    }
}

#[test]
fn installed_cyclops_think_choices_require_world_observations() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.is_dir() {
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let initialization = ThinkInitialization::read(&exe, None).unwrap();
    let input = ThinkInput {
        mode: 2,
        sensors: vec![candidate(1, [0.0, 0.0, 1500.0], 1.0)],
        ..ThinkInput::default()
    };
    let mut driver = ThinkDriver::new(&initialization, 1.0, 0);
    assert_eq!(
        driver
            .think(&initialization.tables, &input, false, false, &mut UnknownWorld)
            .unwrap(),
        None
    );
    let result = driver.think(&initialization.tables, &input, true, false, &mut UnknownWorld);
    println!("world diagnostic: {result:?}");
    assert!(matches!(result, Err(ddai::Error::Unimplemented(message))
        if message == "cyclops world set 1002(0)"));
    // Controlled combat-mode fixture. This supplies disabled navigation
    // observations; it does not certify stage100's live navigation state.
    let attack_input = ThinkInput {
        mode: 2,
        sensors: vec![candidate(1, [0.0, 0.0, 600.0], 1.0)],
        ..ThinkInput::default()
    };
    let mut driver = ThinkDriver::new(&initialization, 1.0, 0);
    let action = driver
        .think(
            &initialization.tables,
            &attack_input,
            true,
            false,
            &mut DisabledNavigation,
        )
        .unwrap()
        .unwrap();
    assert_eq!(action, 50);
    assert_eq!(driver.target, Some(1));
    let catalogue = Catalogue::read(&exe).unwrap();
    let descriptor = &catalogue.actions[action as usize];
    assert_eq!(descriptor.class, "cEm5000ActAttack");
    let request = SimpleAction::Attack.motion_request(descriptor.argument);
    let body = catalogue
        .load_body(&ddvfs::Vfs::open(native).unwrap(), 1.0)
        .unwrap();
    let motion = body.motion(request.motion).unwrap();
    assert_eq!(request.motion, ddenemyrt::body::MotionId::new(1, 0));
    assert!(motion.frame_count > 1);
    println!(
        "controlled attack {action}: packed {:#x}, motion {:?}, {} frames",
        descriptor.argument, request.motion, motion.frame_count
    );
}
