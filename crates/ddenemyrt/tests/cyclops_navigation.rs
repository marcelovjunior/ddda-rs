use ddai::thinktbl::{Host, Slot, ThinkMgr};
use ddenemyrt::cyclops::*;

struct Unsupported;
impl Host for Unsupported {
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, ddai::Error> {
        Err(ddai::Error::Unimplemented(format!("check {id}({arg})")))
    }
    fn set(&mut self, id: u32, arg: u32) -> Result<u32, ddai::Error> {
        Err(ddai::Error::Unimplemented(format!("set {id}({arg})")))
    }
    fn rand(&mut self) -> u32 {
        0
    }
}

struct Reply(NavigationResult);
impl NavigationProvider for Reply {
    fn begin(&mut self, _: [f32; 3], _: [f32; 3]) -> bool {
        true
    }
    fn poll(&mut self, _: [f32; 3], _: f32) -> NavigationResult {
        self.0
    }
    fn cancel(&mut self) {}
}

fn actor() -> NavigationActor {
    NavigationActor {
        position: [10.0, 20.0, 30.0],
        destination: [10.0, 20.0, 1530.0],
        checks_enabled: true,
        yaw: 0.3,
        ..NavigationActor::default()
    }
}

#[test]
fn common_navigation_flags_copy_slots_and_keep_pending_state() {
    let mut world = NavigationWorld {
        actor: actor(),
        provider: Reply(NavigationResult::Waypoint([13.0, 24.0, 30.0])),
        fallback: Unsupported,
    };
    let mut mgr = ThinkMgr::new();
    mgr.slots[0] = Slot::measure([10.0, 20.0, 30.0], 0.3, [100.0, 21.0, 30.0], 50.0, 19);
    mgr.slots[0].mask48 = 12;
    mgr.slots[0].mask50 = 13;
    mgr.slots[0].x60 = 7;
    mgr.slots[0].category = 0x400;
    mgr.slots[0].categorized = true;
    assert_eq!(world.set(1002, 0).unwrap(), 0);
    assert_eq!(world.actor.path_flags, [0, 1, 0]);
    assert!(!world.check(1016, 0).unwrap());
    assert!(world.check(1017, 0).unwrap());
    world.set_with_manager(1003, 100, &mut mgr).unwrap();
    let waypoint = mgr.slots[7];
    assert!(waypoint.valid && !waypoint.categorized);
    assert_eq!(waypoint.category, 0);
    assert_eq!(waypoint.x7c, 0x10);
    assert_eq!(waypoint.obj, 0);
    assert_eq!(waypoint.dist, 5.0); // Source uses 3D even for the XZ field.
    assert_eq!(waypoint.dist3, 5.0);
    assert_eq!(waypoint.height, 4.0);
    assert!((world.actor.path_yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    world.provider.0 = NavigationResult::Pending;
    world.set_with_manager(1003, 100, &mut mgr).unwrap();
    assert_eq!(mgr.slots[7], waypoint);
    assert_eq!(world.actor.path_flags, [0, 1, 0]);
    world.provider.0 = NavigationResult::Complete;
    world.set_with_manager(1003, 100, &mut mgr).unwrap();
    assert_eq!(world.actor.path_flags, [1, 1, 1]); // Completion keeps +5ca1.
    assert!(world.check(1016, 0).unwrap());
    assert_eq!(mgr.slots[7].pos, mgr.slots[0].pos);
    assert_eq!(mgr.slots[7].dist, mgr.slots[0].dist);
    assert_eq!(mgr.slots[7].mask48, 12);
    assert_eq!(mgr.slots[7].mask50, 13);
    assert_eq!(mgr.slots[7].x60, 7);
    assert_eq!(mgr.slots[7].obj, 0);
    assert_eq!(mgr.slots[7].radius, 0.0);
    assert_eq!(mgr.slots[7].angle, 0.0);
    assert!(!mgr.slots[7].categorized);
    assert_eq!(world.actor.path_yaw, 0.3);
    world.provider.0 = NavigationResult::Direct;
    world.set_with_manager(1003, 100, &mut mgr).unwrap();
    assert_eq!(world.actor.path_flags, [1, 0, 1]);
    world.actor.checks_enabled = false;
    assert!(!world.check(1016, 0).unwrap());
    world.actor.check1027 = 2;
    assert!(world.check(1027, 0).unwrap());
    world.set(1002, 0).unwrap();
    assert_eq!(world.actor.path_flags, [0, 0, 1]);
}

struct Collision {
    blocked: bool,
}
impl ddsim::Ground for Collision {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
    fn ray(&self, from: [f32; 3], to: [f32; 3]) -> Option<ddsim::RayHit> {
        assert_eq!(from[1], 100.0);
        assert_eq!(to[1], 100.0);
        self.blocked.then_some(ddsim::RayHit {
            point: [0.0, 100.0, 500.0],
            normal: [0.0, 0.0, -1.0],
        })
    }
}

#[test]
fn viewer_straight_segment_checks_collision_without_inventing_a_detour() {
    let blocked = Collision { blocked: true };
    let clear = Collision { blocked: false };
    let mut provider = StraightLineNavigation::new(&blocked, 100.0);
    assert!(provider.begin([0.0; 3], [0.0, 0.0, 1000.0]));
    assert_eq!(provider.poll([0.0; 3], 100.0), NavigationResult::Pending);
    let mut provider = StraightLineNavigation::new(&clear, 100.0);
    assert_eq!(provider.poll([0.0; 3], 100.0), NavigationResult::Unavailable);
    provider.begin([0.0; 3], [0.0, 0.0, 1000.0]);
    assert_eq!(
        provider.poll([0.0; 3], 100.0),
        NavigationResult::Waypoint([0.0, 0.0, 1000.0])
    );
    assert_eq!(
        provider.poll([0.0, 0.0, 900.0], 100.0),
        NavigationResult::Waypoint([0.0, 0.0, 1000.0])
    );
    assert_eq!(
        provider.poll([0.0, 0.0, 901.0], 100.0),
        NavigationResult::Complete
    );
    provider.cancel();
    assert_eq!(provider.poll([0.0; 3], 100.0), NavigationResult::Unavailable);
}

fn run_installed<P: NavigationProvider>(
    initialization: &ThinkInitialization,
    provider: P,
    expected_action: i32,
    expected_category: u32,
) {
    let input = ThinkInput {
        mode: 2,
        rage_target: Some(ThinkTarget {
            id: 101,
            position: [0.0, 0.0, 1500.0],
            radius: 0.0,
            flags: [0; 2],
            action_flags: 0,
        }),
        ..ThinkInput::default()
    };
    let mut driver = ThinkDriver::new(initialization, 1.0, 0);
    let mut world = NavigationWorld {
        actor: NavigationActor {
            destination: [0.0, 0.0, 1500.0],
            checks_enabled: true,
            ..NavigationActor::default()
        },
        provider,
        fallback: Unsupported,
    };
    let result = driver.think(&initialization.tables, &input, true, false, &mut world);
    println!(
        "installed navigation: {result:?}; slot7 {:?}; flags {:?}",
        driver.manager.slots[7], world.actor.path_flags
    );
    assert!(driver.manager.slots[7].valid, "installed set1003 must run");
    assert_eq!(result.unwrap(), Some(expected_action));
    assert_eq!(driver.manager.slots[7].category, expected_category);
    assert_eq!(driver.manager.slots[7].obj, 0);
    assert_eq!(driver.manager.slots[7].pos, [0.0, 0.0, 1500.0]);
}

#[test]
fn installed_cyclops_table_runs_past_set1003_with_both_providers() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.is_dir() {
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let initialization = ThinkInitialization::read(&exe, None).unwrap();
    run_installed(&initialization, NoNavigationServices, 1, 0);
    run_installed(
        &initialization,
        StraightLineNavigation::new(&Collision { blocked: false }, 100.0),
        85,
        0,
    );
}
