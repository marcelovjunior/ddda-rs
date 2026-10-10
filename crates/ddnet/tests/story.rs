//! The opening's Cassardis slice over loopback (protocol 3): a guest sees
//! the host's scheduled NPCs in the snapshots and gets the cutscene the
//! story asks for (`ToClient::PlayEvent`) to play itself. Skips without
//! the install.

use ddhost::opening::{fast_forward, to_newly_arisen, OpeningWorld, CASSARDIS_ROWS};
use ddhost::systems::SystemData;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddnet::proto::ToClient;
use ddnet::{Client, Server, ServerConfig, Transport, Udp};
use ddsim::{Ground, Sim};
use ddstory::data::InstallData;
use ddstory::Story;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

struct Void;
impl Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

const BUILD: u32 = 1_513_180_209;
const DATA: u64 = 0xdada;

#[test]
fn guest_sees_the_npcs_and_gets_the_cutscene() {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    if !np.exists() || !exe.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let story = Story::new_game(InstallData::open(&np, Some(&exe)).unwrap());
    let mut host = Host::new(
        Sim::new(HashMap::new()),
        Some(story),
        OpeningWorld::new(),
        Box::new(Void),
    );
    host.data = SystemData {
        npc: Some(Arc::new(ddhost::ddnpcrt::InstallData::open(&np, "eng").unwrap())),
        ..Default::default()
    };
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    fast_forward(&mut host, &to_newly_arisen(), 240, 30_000, |h| {
        h.story.as_ref().is_some_and(|s| s.quest(1).is_some())
    })
    .unwrap();
    assert!(host.spawn_npc(2, 1).unwrap());
    // as the viewer: cutscenes and the menu wait for the view
    host.world.live(CASSARDIS_ROWS);
    let mut srv = Server::new(
        host,
        Udp::bind("127.0.0.1:0").unwrap(),
        ServerConfig {
            build: BUILD,
            data: DATA,
            spawn: [0.0; 3],
            spot: None,
        },
    );
    srv.wait_for_events = true;
    let mut c = Client::new(
        Udp::bind("127.0.0.1:0").unwrap(),
        srv.transport.local_addr(),
        2,
        BUILD,
        DATA,
        None,
        Sim::new(HashMap::new()),
        Box::new(Void),
        0,
    );
    let route = ddstory::route::opening();
    let at = |name: &str, srv: &Server<InstallData, OpeningWorld, Udp>| {
        let w = route.iter().find(|w| w.name == name).unwrap();
        w.place(&srv.host.story.as_ref().unwrap().stage.scenarios)
            .unwrap()
    };
    let mut now = 0;
    let mut row = None;
    let mut menu_closed: Option<u64> = None;
    for f in 0..3000u64 {
        srv.pump(now);
        c.pump(now);
        c.frame(PlayerInput::default(), now);
        // the weapon table first (the scripted host closes the menu), then
        // the room exit
        let (name, interact) = if menu_closed.is_some_and(|m| f > m + 5) {
            ("cassardis: room exit", false)
        } else {
            ("cassardis: weapon table", f % 2 == 0 && menu_closed.is_none())
        };
        let (pos, yaw) = at(name, &srv);
        let p = srv.host.sim.player_mut(0).unwrap();
        (p.position, p.yaw) = (pos, yaw);
        let mut local = FrameInputs::new();
        local.insert(
            0,
            PlayerInput {
                interact,
                ..Default::default()
            },
        );
        srv.step(&local, now).unwrap();
        if srv.host.world.menu.is_some() {
            srv.host.world.close_menu(Some(1));
            menu_closed = Some(f);
        }
        for e in c.events.drain(..) {
            if let ToClient::PlayEvent { row: r, .. } = e {
                row = Some(r);
            }
        }
        if row.is_some() {
            break;
        }
        now += 33;
        std::thread::sleep(Duration::from_micros(200));
    }
    assert_eq!(row, Some(22), "st200 ev10 reached the guest");
    let npcs = c.npcs();
    let quina = npcs.iter().find(|n| n.id == 2).expect("Quina in the snapshot");
    assert_eq!(Some(quina.pos), srv.host.npcs.position(2));
    // A guest that arrives during the event gets its original start frame,
    // allowing its view to catch up rather than restarting the host story.
    srv.host.leave(c.seat.unwrap());
    let mut late = Client::new(
        Udp::bind("127.0.0.1:0").unwrap(),
        srv.transport.local_addr(),
        3,
        BUILD,
        DATA,
        None,
        Sim::new(HashMap::new()),
        Box::new(Void),
        now,
    );
    let mut late_start = None;
    for _ in 0..30 {
        now += 33;
        late.pump(now);
        late.frame(PlayerInput::default(), now);
        srv.pump(now);
        srv.step(&FrameInputs::new(), now).unwrap();
        for event in late.events.drain(..) {
            if let ToClient::PlayEvent { frame, row, .. } = event {
                late_start = Some((frame, row));
            }
        }
        if late_start.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    assert_eq!(late_start.map(|(_, row)| row), Some(22));
    let before = srv.host.sim.player(0).unwrap().position;
    let moving = FrameInputs::from([(
        0,
        PlayerInput {
            sim: ddsim::Input {
                stick: [1.0, 0.0],
                ..Default::default()
            },
            interact: true,
        },
    )]);
    for _ in 0..10 {
        now += 33;
        srv.step(&moving, now).unwrap();
    }
    let blocked = srv.host.sim.player(0).unwrap().position;
    assert_eq!(
        (before[0], before[2]),
        (blocked[0], blocked[2]),
        "shared cutscene suppresses movement input"
    );
    srv.event_done();
}
