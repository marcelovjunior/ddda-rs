use ddenemyrt::cyclops::{ActionEnterState, CollisionSwitch, EnterBodyState, enter_action};

#[test]
fn installed_base_enter_resets_fields_and_selects_action_rows() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.is_dir() {
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let mut action = ActionEnterState {
        completion: 8,
        has_enemy_owner: false,
        scratch_words: [9; 3],
        scratch_float: 12.0,
        entered: false,
        has_cyclops_owner: false,
    };
    let mut body = EnterBodyState {
        owner_flags: 0x82,
        is_cyclops: true,
        action_flags: [99; 2],
        enemy_enter_scratch: 17,
        current_priority: 8,
        pending_priority: 0x441,
        enemy_action_sentinel: 7,
        reset_vectors: [[9.0; 4]; 2],
        cyclops_enter_scratch: 6,
        motion_frame_fallback: 99,
        collision: CollisionSwitch {
            bound: true,
            mode: 7,
            suppress_object_writes: false,
            object_suppressed: [Some(false), None, Some(false), None, None, None],
            cached_suppressed: [false; 6],
        },
    };
    // Distinct installed rows exercise the byte field and the fallback word.
    // Expected scalar observations are a regression oracle, not a bulk table.
    for (id, mode, fallback) in [(0, 0, 0), (81, 1, 3)] {
        enter_action(&exe, id, &mut action, &mut body, [2.0, 3.0, 4.0]).unwrap();
        assert_eq!(
            action,
            ActionEnterState {
                completion: 0,
                has_enemy_owner: true,
                scratch_words: [0; 3],
                scratch_float: 0.0,
                entered: true,
                has_cyclops_owner: true,
            }
        );
        assert_eq!(body.action_flags, [0; 2]);
        assert_eq!(body.enemy_enter_scratch, 0);
        assert_eq!(body.current_priority, 0x410_0001);
        assert_eq!(body.pending_priority, 0x441);
        assert_eq!(body.enemy_action_sentinel, u32::MAX);
        assert_eq!(body.reset_vectors, [[2.0, 3.0, 4.0, 0.0]; 2]);
        assert_eq!(body.cyclops_enter_scratch, 0);
        assert_eq!(body.motion_frame_fallback, fallback);
        assert_eq!(body.collision.mode, mode);
        assert_eq!(body.collision.cached_suppressed, [mode != 1; 6]);
        assert_eq!(body.collision.object_suppressed[0], Some(mode != 1));
        assert_eq!(body.collision.object_suppressed[1], None);
    }
    body.collision.suppress_object_writes = true;
    enter_action(&exe, u32::MAX, &mut action, &mut body, [0.0; 3]).unwrap();
    assert_eq!(body.motion_frame_fallback, 0);
    assert_eq!(body.collision.cached_suppressed, [true; 6]);
    assert_eq!(body.collision.object_suppressed[0], Some(false));
    // Unbound and unchanged-mode helpers leave even cached flags untouched.
    body.collision.bound = false;
    body.collision.cached_suppressed = [false; 6];
    enter_action(&exe, 81, &mut action, &mut body, [0.0; 3]).unwrap();
    assert_eq!(body.collision.mode, 0);
    assert_eq!(body.collision.cached_suppressed, [false; 6]);
    body.collision.bound = true;
    enter_action(&exe, 0, &mut action, &mut body, [0.0; 3]).unwrap();
    assert_eq!(body.collision.cached_suppressed, [false; 6]);
    body.is_cyclops = false;
    body.owner_flags = 0;
    body.current_priority = 8;
    body.enemy_enter_scratch = 17;
    body.enemy_action_sentinel = 7;
    body.motion_frame_fallback = 99;
    enter_action(&exe, 81, &mut action, &mut body, [9.0; 3]).unwrap();
    assert!(!action.has_enemy_owner && !action.has_cyclops_owner);
    assert_eq!(body.current_priority, 8);
    assert_eq!(body.enemy_enter_scratch, 17);
    assert_eq!(body.enemy_action_sentinel, 7);
    assert_eq!(body.motion_frame_fallback, 99);
}
