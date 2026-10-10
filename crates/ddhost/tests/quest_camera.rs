use ddhost::opening::OpeningWorld;
use ddhost::quest_camera::Cameras;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId, StoryWorld};
use ddstory::{
    data::{Data, InstallData},
    HostEvent, Request, Story,
};

fn install() -> Option<ddvfs::Vfs> {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !path.is_dir() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none());
        return None;
    }
    Some(ddvfs::Vfs::open(path).unwrap())
}

#[test]
fn q2_event1_waits_for_installed_camera_exhaustion_before_q5() {
    let Some(vfs) = install() else { return };
    let data = InstallData::open(&vfs.root, None).unwrap();
    let mut story = Story::new_game(data);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1400, false);
    // Fixture begins after the separate installed scenario volume admitted
    // its task and q2's QCT set95. Never set25/5 or fabricate camera end.
    story.world.set_quest_bit(2, 95, true);
    let path = "scr/st100/fsm/q0002_event1";
    story
        .tasks
        .push(ddstory::script::Script::new(path, story.data.fsm(path).unwrap()).unwrap());
    let mut world = OpeningWorld::new();
    world.enable_live_encounters();
    world.inner.message_frames = 1;
    let mut cameras = Cameras::default();
    cameras.load(&vfs, 100).unwrap();
    world.cameras = Some(cameras);
    let mut started = false;
    let mut ended = false;
    let mut camera_end = false;
    for frame in 0..180 {
        world.camera_step(1.0 / 30.0);
        let events = world.events();
        if events.contains(&HostEvent::CameraFinished) {
            ended = true;
        }
        let requests = story
            .tick(
                &ddstory::Inputs {
                    events,
                    ..Default::default()
                },
                &world,
            )
            .unwrap();
        if requests.iter().any(|r| {
            matches!(
                r,
                Request::Camera {
                    cam_id: 12,
                    simple: false,
                    ..
                }
            )
        }) {
            started = true;
            assert!(story.world.quest_bit(2, 25));
        }
        camera_end |= requests.iter().any(|r| {
            matches!(
                r,
                Request::CameraEnd {
                    set_pos_type: 0,
                    start_pos: 0
                }
            )
        });
        world.handle(100, &requests);
        assert!(world.cameras.as_ref().unwrap().error.is_none());
        if started && !ended {
            assert!(!story.world.quest_bit(2, 5), "premature q5 at frame{frame}");
        }
        if story.world.quest_bit(2, 5) {
            assert!(started && ended);
            // CameraEnd placement is a simple-event operation. The start
            // record for12 is non-simple, so its end order clears the slot
            // and preserves the party positions without emitting placement.
            assert!(!camera_end);
            assert_eq!(story.tasks[0].owner.camera.state, 0);
            return;
        }
    }
    panic!(
        "installed event1 failed: started{started} ended{ended}, {:?}",
        story.tasks.iter().map(|s| s.current_names()).collect::<Vec<_>>()
    );
}

#[test]
fn timer_cannot_finish_real_camera_and_overlapping_request_cannot_fallback() {
    let Some(vfs) = install() else { return };
    let mut world = OpeningWorld::new();
    world.inner.camera_frames = 1;
    // Queue a stale timer before real activation, then prove it is filtered.
    world.handle(
        100,
        &[Request::Camera {
            cam_id: 12,
            simple: false,
            quest: 2,
        }],
    );
    let mut cameras = Cameras::default();
    cameras.load(&vfs, 100).unwrap();
    assert!(cameras.supports_authored(100, 12, false));
    assert!(cameras.supports_authored(100, 149, false));
    assert!(!cameras.supports_authored(200, 24, true));
    world.cameras = Some(cameras);
    for _ in 0..5 {
        assert!(!world.events().contains(&HostEvent::CameraFinished));
    }
    world.handle(
        100,
        &[Request::Camera {
            cam_id: 12,
            simple: false,
            quest: 2,
        }],
    );
    assert!(world.cameras.as_ref().unwrap().active.is_some());
    for _ in 0..60 {
        world.camera_step(1.0 / 30.0);
        assert!(!world.events().contains(&HostEvent::CameraFinished));
    }
    world.handle(
        100,
        &[Request::Camera {
            cam_id: u32::MAX,
            simple: false,
            quest: 2,
        }],
    );
    assert!(world.cameras.as_ref().unwrap().error.is_some());
}

#[test]
fn stage_handoff_releases_previous_camera_and_reports_one_real_end() {
    let Some(vfs) = install() else { return };
    let mut world = OpeningWorld::new();
    let mut cameras = Cameras::default();
    cameras.load(&vfs, 100).unwrap();
    cameras.request(100, 12, false).unwrap();
    world.cameras = Some(cameras);
    assert!(world.player_actions_blocked(7));
    // Normal stage load invalidates the old camera's native parameter identity.
    world.cameras.as_mut().unwrap().load(&vfs, 200).unwrap();
    assert!(!world.player_actions_blocked(7));
    assert!(world.cameras.as_ref().unwrap().active.is_none());
    world.camera_step(1.0 / 30.0);
    assert_eq!(
        world
            .events()
            .iter()
            .filter(|e| **e == HostEvent::CameraFinished)
            .count(),
        1
    );
    world.camera_step(1.0 / 30.0);
    assert!(!world.events().contains(&HostEvent::CameraFinished));
}

#[test]
fn legacy_simple_prerequisite_cannot_complete_following_authored_camera() {
    let Some(vfs) = install() else { return };
    let mut world = OpeningWorld::new();
    world.enable_live_encounters();
    let mut cameras = Cameras::default();
    cameras.load(&vfs, 200).unwrap();
    cameras.load(&vfs, 100).unwrap();
    world.cameras = Some(cameras);
    world.inner.camera_frames = 4;
    // Installed sontyoHouseHatudeCam starts simple camera24 at the door.
    world.handle(
        200,
        &[Request::Camera {
            cam_id: 24,
            simple: true,
            quest: -1,
        }],
    );
    assert!(world.cameras.as_ref().unwrap().active.is_none());
    assert!(world.cameras.as_ref().unwrap().error.is_none());
    // A supported camera requested during the stand-in's ownership fails;
    // contention must not turn that real camera into another timer.
    world.handle(100, &[Request::Camera { cam_id: 12, simple: false, quest: 2 }]);
    assert!(world.cameras.as_ref().unwrap().error.is_some());
    assert!(world.cameras.as_ref().unwrap().active.is_none());
    for frame in 1..=5 {
        assert_eq!(world.events().contains(&HostEvent::CameraFinished), frame == 4);
    }
    world.handle(
        100,
        &[Request::Camera {
            cam_id: 12,
            simple: false,
            quest: 2,
        }],
    );
    assert!(world.cameras.as_ref().unwrap().active.is_some());
    assert!(world.cameras.as_ref().unwrap().error.is_none());
    // Neither the earlier legacy callback nor its four-frame duration may
    // acknowledge camera12. Its installed final key is just over3seconds.
    for _ in 0..90 {
        world.camera_step(1.0 / 30.0);
        assert!(!world.events().contains(&HostEvent::CameraFinished));
    }
    world.camera_step(1.0 / 30.0);
    assert!(world.events().contains(&HostEvent::CameraFinished));
    assert!(world.cameras.as_ref().unwrap().active.is_none());
    assert!(!world.events().contains(&HostEvent::CameraFinished));
}

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

#[test]
fn host_camera_blocks_each_arisen_and_pawn_without_stopping_ai_or_world_frames() {
    let Some(vfs) = install() else { return };
    let mut world = OpeningWorld::new();
    let mut cameras = Cameras::default();
    cameras.load(&vfs, 100).unwrap();
    cameras.request(100, 12, false).unwrap();
    world.cameras = Some(cameras);
    let mut host = Host::<ddhost::data::NoStoryData, _>::new(
        ddhost::install::sim(&vfs).unwrap(),
        None,
        world,
        Box::new(Flat),
    );
    for seat in 0..2 {
        host.join(
            ProfileId(seat + 1),
            Control::Local,
            [seat as f32 * 300.0, 0.0, 0.0],
            0.0,
        )
        .unwrap();
    }
    let exe = ddexe::ExeFile::next_to(&vfs.root).unwrap();
    let data = ddenemyrt::fight::FightData::load(&vfs, &exe.image().unwrap()).unwrap();
    let mut fight = ddenemyrt::fight::Fight::new(std::sync::Arc::new(data));
    for seat in 0..2 {
        fight.add_arisen(seat);
        fight.equip(&mut host.sim, seat);
        // Fixture beyond the installed battle-follow threshold. Each pawn
        // serves a distinct occupied seat; camera12 must block both domains.
        fight.add_pawn(&mut host.sim, 4 + seat, seat,
            [if seat == 0 { -2500.0 } else { 2500.0 }, 0.0, 0.0],
            0.0, [500.0; 9], 7 + u32::from(seat));
    }
    fight.add_goblin([0.0, 0.0, 2500.0], std::f32::consts::PI);
    host.fight = Some(fight);
    let initial = host.sim.players.iter().map(|p| p.position).collect::<Vec<_>>();
    let inputs: FrameInputs = (0..2)
        .map(|id| {
            (
                id,
                PlayerInput {
                    sim: ddsim::Input {
                        stick: [0.0, 1.0],
                        light: true,
                        jump: true,
                        ..Default::default()
                    },
                    interact: true,
                },
            )
        })
        .collect();
    for _ in 0..30 {
        host.tick(&inputs).unwrap();
    }
    assert_eq!(host.frame, 30);
    let fight = host.fight.as_ref().unwrap();
    for pawn in fight.pawns.values() {
        assert!(!pawn.brain.history.is_empty(), "Pawn AI must continue ticking");
    }
    assert!(fight.goblins[0].frame > 0.0, "Enemy world phases must continue");
    for (p, initial) in host.sim.players.iter().zip(initial) {
        assert_eq!(p.position[0], initial[0]);
        assert_eq!(p.position[2], initial[2]);
    }
    assert!(
        host.world
            .cameras
            .as_ref()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .seconds
            > 0.0
    );
    while host.world.cameras.as_ref().unwrap().active.is_some() {
        host.tick(&inputs).unwrap();
    }
    let before = host.sim.player(0).unwrap().position;
    let pawn_before = [host.sim.player(4).unwrap().position, host.sim.player(5).unwrap().position];
    for _ in 0..30 {
        host.tick(&inputs).unwrap();
    }
    assert_ne!(host.sim.player(0).unwrap().position, before);
    for (id, before) in [4, 5].into_iter().zip(pawn_before) {
        assert_ne!(host.sim.player(id).unwrap().position, before, "Pawn{id} did not resume");
    }
}
