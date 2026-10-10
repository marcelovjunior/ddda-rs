use mtcamera::quest::{Animation, Blend, Key, List, Pose, ease};

fn animation(keys: &[(f32, f32, i32)]) -> Animation {
    let curve = [
        0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
    ];
    let blend = Blend {
        kind: 3,
        frame_type: 0,
        frames: 0.0,
        curve,
    };
    Animation {
        keys: keys
            .iter()
            .map(|&(seconds, x, kind)| Key {
                seconds,
                kind,
                pose: Pose {
                    eye: [x, 2.0, 3.0],
                    target: [0.0, 0.0, 0.0],
                    roll_degrees: 0.0,
                    fov_degrees: 60.0,
                },
            })
            .collect(),
        camera_target: 2,
        option_flags: 1,
        start: blend.clone(),
        end: blend,
        cant_act_player: true,
        cant_act_flags: 3,
        cant_act_frames: [2.0, 5.0],
        world_slow: false,
    }
}

#[test]
fn native_sequence_channels_and_strict_exhaustion() {
    let a = animation(&[(0.0, 0.0, 0), (3.0, 12.0, 0)]);
    assert_eq!(a.sample(1.5).0.eye[0], 6.0);
    assert!(a.sample(3.0).1);
    assert!(!a.sample(3.01).1);
    assert_eq!(a.sample(3.01).0.eye[0], 12.0);
    let a = animation(&[(0.0, 0.0, 1), (3.0, 12.0, 0)]);
    // Native equal-tangent shortcut is linear, including two zero tangents.
    assert_eq!(a.sample(0.75).0.eye[0], 3.0);
    let a = animation(&[(0.0, 4.0, 2), (3.0, 12.0, 0)]);
    assert_eq!(a.sample(1.5).0.eye[0], 4.0); // type2 holds preceding value
    let a = animation(&[(0.0, 0.0, 0), (1.0, 4.0, 0), (3.0, 8.0, 0)]);
    assert!((a.sample(2.0).0.eye[0] - 6.1666665).abs() < 0.00001);
}

#[test]
fn native_ease_curve_action_range_and_polar_transition() {
    let a = animation(&[(0.0, 0.0, 0), (3.0, 12.0, 0)]);
    for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert!((ease(&a.start.curve, f) - f).abs() < 0.00001);
    }
    assert!(!a.blocks_actions(1.99));
    assert!(a.blocks_actions(2.0));
    assert!(a.blocks_actions(7.0));
    assert!(!a.blocks_actions(7.01));
    let from = Pose {
        eye: [0.0, 0.0, 10.0],
        target: [0.0; 3],
        roll_degrees: 0.0,
        fov_degrees: 40.0,
    };
    let to = Pose {
        eye: [10.0, 0.0, 0.0],
        ..from
    };
    let mid = from.interpolate(to, 0.5, 3).unwrap();
    assert!((mid.eye[0] - 50.0f32.sqrt()).abs() < 0.00001);
    assert!((mid.eye[2] - 50.0f32.sqrt()).abs() < 0.00001);
    assert_eq!(from.interpolate(to, 0.5, 2).unwrap().eye, [5.0, 0.0, 5.0]);
    assert!(from.interpolate(to, 0.5, 0).is_none());
}

#[test]
fn installed_camera12_uses_authored_world_keys_and_actual_end() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none());
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let bytes = vfs
        .read("scr/st100/etc/st100", mtarc::type_hash("rCameraParamList"))
        .unwrap();
    let list = List::parse(&bytes).unwrap();
    let a = list.animations[12].as_ref().expect("indexed Anim camera12");
    assert!(list.animations[149].as_ref().unwrap().supported_world_camera());
    assert!(a.supported_world_camera());
    assert!(a.blocks_actions(0.0));
    assert_eq!(a.keys.len(), 2);
    assert!(a.duration() > 0.0);
    let (first, running) = a.sample(a.keys[0].seconds);
    assert!(running);
    assert_eq!(first, a.keys[0].pose);
    let (middle, running) = a.sample(a.duration() * 0.5);
    assert!(running);
    assert!(
        middle
            .eye
            .iter()
            .zip(first.eye)
            .any(|(a, b)| (*a - b).abs() > 1.0)
    );
    assert_eq!(a.sample(a.duration()).0, a.keys.last().unwrap().pose);
    assert!(a.sample(a.duration()).1);
    assert!(!a.sample(a.duration() + 0.01).1);
    assert_eq!(a.start.kind, 3);
    assert_eq!(a.end.kind, 3);
    assert!(!a.world_slow);
    eprintln!(
        "camera12: {} keys, {}s, entry {} frames, return {} frames, FOV {}",
        a.keys.len(),
        a.duration(),
        a.start.frames,
        a.end.frames,
        first.fov_degrees
    );
}

#[test]
fn unsupported_native_blend_and_input_domains_are_rejected() {
    let a = animation(&[(0.0, 0.0, 0), (3.0, 12.0, 0)]);
    assert!(a.supported_world_camera());
    let mut unsupported = a.clone();
    unsupported.start.frames = 15.0;
    unsupported.start.frame_type = 1;
    assert!(!unsupported.supported_world_camera());
    unsupported.start.frame_type = 0;
    unsupported.start.kind = 1;
    assert!(!unsupported.supported_world_camera());
    unsupported.start.kind = 3;
    assert!(unsupported.supported_world_camera());
    for flags in [1, 2, 4] {
        let mut unsupported = a.clone();
        unsupported.cant_act_flags = flags;
        assert!(!unsupported.supported_world_camera());
    }
    let mut unblocked = a.clone();
    unblocked.cant_act_flags = 0;
    assert!(!unblocked.supported_world_camera());
    unblocked.cant_act_player = false;
    assert!(unblocked.supported_world_camera());
}
