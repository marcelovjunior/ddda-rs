//! Real loopback late admission during a heartbeat-only host.
use ddhost::data::NoStoryData;
use ddhost::opening::OpeningWorld;
use ddhost::quest_camera::Cameras;
use ddhost::{Control, Host, ProfileId};
use ddnet::{Client, Server, ServerConfig, Transport, Udp};

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
}

#[test]
fn late_join_observes_current_camera_clock_and_explicit_exhaustion_without_host_step() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    for ended in [false, true] {
        let mut world = OpeningWorld::new();
        let mut cameras = Cameras::default();
        cameras.load(&vfs, 100).unwrap();
        cameras.request(100, 12, false).unwrap();
        cameras.step(if ended { 4.0 } else { 1.25 });
        world.cameras = Some(cameras);
        let mut host = Host::new(
            ddsim::Sim::new(Default::default()),
            None::<ddstory::Story<NoStoryData>>,
            world,
            Box::new(Flat),
        );
        host.frame = 42;
        host.join(ProfileId(11), Control::Local, [0.0; 3], 0.0).unwrap();
        let mut server = Server::new(
            host,
            Udp::bind("127.0.0.1:0").unwrap(),
            ServerConfig {
                build: 1513180209,
                data: 0x55,
                spawn: [0.0; 3],
                spot: None,
            },
        );
        let mut client = Client::new(
            Udp::bind("127.0.0.1:0").unwrap(),
            server.transport.local_addr(),
            22,
            1513180209,
            0x55,
            None,
            ddsim::Sim::new(Default::default()),
            Box::new(Flat),
            0,
        );
        for now in (0..400).step_by(10) {
            client.pump(now);
            client.frame(Default::default(), now);
            server.pump(now);
            server.heartbeat(now);
            std::thread::sleep(std::time::Duration::from_micros(300));
        }
        assert_eq!(client.seat, Some(1));
        assert_eq!(server.host.frame, 42, "admission must not advance simulation");
        assert_eq!(client.latest().unwrap().frame, 42);
        if ended {
            assert_eq!(client.quest_camera(), None);
            assert!(!client.actions_blocked());
        } else {
            let camera = client.quest_camera().unwrap();
            assert_eq!((camera.stage, camera.camera_id, camera.generation), (100, 12, 1));
            assert_eq!(camera.seconds, 1.25, "never replay elapsed time zero");
            assert!(client.actions_blocked());
        }
    }
}
