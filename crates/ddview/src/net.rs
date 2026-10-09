//! `--host <port>` and `--join <addr>`: LAN co-op through `ddnet`
//! (docs/adr/0002-game-host.md section 8).
//!
//! - **Host.** Runs the session's `ddhost::Host` (stage collision as
//!   ground) behind a `ddnet::Server`. It plays seat 0 with W A S D and
//!   shows every Arisen from the host's simulation.
//! - **Client.** Plays its own seat with the same keys. It shows its
//!   Arisen as predicted, and the others 100 ms behind, interpolated.
//!
//! Both step at the host's 30 Hz frame, whatever the render rate. The
//! camera follows the local seat.

use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use ddhost::data::NoStoryData;
use ddhost::{Control, FrameClock, FrameInputs, Host, PlayerInput, ProfileId};
use ddstory::scripted::ScriptedHost;

use crate::character::Playback;
use crate::play::{self, FollowCam, PlayState, Shown};

/// The port `--join` uses when the address names none.
pub const DEFAULT_PORT: u16 = 47810;

enum Side {
    Host(Box<ddnet::Server<NoStoryData, ScriptedHost, ddnet::Udp>>),
    Client {
        client: Box<ddnet::Client<ddnet::Udp>>,
        /// The newest snapshot frame and when it arrived.
        newest: Option<(u64, Instant)>,
        told: bool,
    },
}

struct Session {
    side: Side,
    start: Instant,
    /// Seconds not yet stepped.
    acc: f32,
    log_seen: usize,
}

#[derive(Resource)]
pub struct NetRes(Mutex<Session>);

/// What `--host` / `--join` asked for.
pub enum Mode {
    Host { port: u16 },
    Join { addr: String },
}

/// The exe build to announce: 1513180209 if the install's DDDA.exe is the
/// supported build (`ddexe` refuses any other), else 0.
fn exe_build(native_pc: &Path) -> u32 {
    let ok = ddexe::locate(native_pc).ok().and_then(|p| ddexe::ExeFile::open(p).ok()).is_some_and(|f| f.image().is_ok());
    if ok {
        1_513_180_209
    } else {
        0
    }
}

/// Starts the network session around `st` (the play state, whose
/// characters stand for the seats). `spawn` is the stage-frame position
/// of seat 0.
#[allow(clippy::too_many_arguments)]
pub fn setup(
    commands: &mut Commands,
    vfs: &ddvfs::Vfs,
    native_pc: &Path,
    mode: Mode,
    profile: Option<u64>,
    (stage, a0, b0, radius): (u32, i32, i32, i32),
    spawn: Vec3,
    st: &mut PlayState,
) -> Result<(), String> {
    let sim = ddhost::install::sim(vfs)?;
    let mut ground = ddworld::Terrain::load_collision(vfs, stage, a0, b0, radius);
    ground.add_scenery(vfs, stage, a0, b0, radius);
    let build = exe_build(native_pc);
    let data = ddnet::data_fingerprint(native_pc).map_err(|e| e.to_string())?;
    info!("net: exe build {build}, data fingerprint {data:016x}");
    let side = match mode {
        Mode::Host { port } => {
            let mut host = Host::new(sim, None, ScriptedHost::default(), Box::new(ground));
            host.join(ProfileId(profile.unwrap_or(1)), Control::Local, spawn.to_array(), 0.0)
                .ok_or("cannot seat the host")?;
            let udp = ddnet::Udp::bind(&format!("0.0.0.0:{port}")).map_err(|e| format!("bind port {port}: {e}"))?;
            info!("net: hosting on port {port}");
            let config = ddnet::ServerConfig { build, data, spawn: spawn.to_array() };
            st.follow = 0;
            Side::Host(Box::new(ddnet::Server::new(host, udp, config)))
        }
        Mode::Join { addr } => {
            let with_port = if addr.contains(':') { addr.clone() } else { format!("{addr}:{DEFAULT_PORT}") };
            let server = with_port
                .to_socket_addrs()
                .map_err(|e| format!("{with_port}: {e}"))?
                .find(|a| a.is_ipv4())
                .ok_or(format!("{with_port}: no IPv4 address"))?;
            let udp = ddnet::Udp::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
            let profile = profile.unwrap_or_else(|| std::process::id() as u64 | 1 << 32);
            info!("net: joining {server} as profile {profile:x}");
            let client = ddnet::Client::new(udp, server, profile, build, data, sim, Box::new(ground), 0);
            Side::Client { client: Box::new(client), newest: None, told: false }
        }
    };
    commands.insert_resource(NetRes(Mutex::new(Session { side, start: Instant::now(), acc: 0.0, log_seen: 0 })));
    Ok(())
}

fn shown(b: &ddnet::Body) -> Shown {
    Shown { id: b.id, position: b.position, yaw: b.yaw, state: b.state, frame: b.frame }
}

/// One render frame: step the session at 30 Hz, then show every Arisen.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    net: Option<Res<NetRes>>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
    mut vis: Query<&mut Visibility>,
) {
    let (Some(mut st), Some(net)) = (state, net) else { return };
    let st = &mut *st;
    play::orbit(st, &buttons, &mouse);
    let mut input = PlayerInput { sim: play::p1_input(&keys, st.cam_yaw), interact: keys.pressed(KeyCode::KeyE) };
    if st.demo {
        // Unattended: walk in a small circle near the spawn (about 2.7 m
        // across at the walk speed) and jump every 4 s.
        let t = time.elapsed_secs();
        input.sim = ddsim::Input {
            stick: [(t * 0.6).sin(), (t * 0.6).cos()],
            walk: true,
            jump: t % 4.0 < 0.1,
            camera_yaw: st.cam_yaw,
            ..default()
        };
    }
    let mut guard = net.0.lock().unwrap();
    let s = &mut *guard;
    let now = s.start.elapsed().as_millis() as u64;
    let frame = FrameClock::default().seconds();
    s.acc = (s.acc + time.delta_secs()).min(0.25);
    let bodies: Vec<Shown> = match &mut s.side {
        Side::Host(server) => {
            server.pump(now);
            while s.acc >= frame {
                s.acc -= frame;
                // As --play: the start area's relaxed run, a dagger's
                // second jump (docs/MOVEMENT.md).
                for p in &mut server.host.sim.players {
                    p.relaxed = true;
                    p.double_jump = true;
                }
                let mut local = FrameInputs::new();
                local.insert(0, input);
                if let Err(e) = server.step(&local, now) {
                    error!("net: host frame: {e}");
                }
            }
            for line in &server.log[s.log_seen..] {
                info!("net: {line}");
            }
            s.log_seen = server.log.len();
            server.host.sim.players.iter().map(Shown::of).collect()
        }
        Side::Client { client, newest, told } => {
            client.pump(now);
            while s.acc >= frame {
                s.acc -= frame;
                client.frame(input, now);
            }
            if let Some(r) = &client.refused {
                if !*told {
                    error!("net: refused: {r}");
                    *told = true;
                }
            }
            if let Some(seat) = client.seat {
                if !*told {
                    info!("net: joined as seat {seat}");
                    *told = true;
                }
                st.follow = seat;
            }
            if let Some(l) = client.latest() {
                if newest.is_none_or(|(f, _)| f != l.frame) {
                    *newest = Some((l.frame, Instant::now()));
                }
            }
            let since = newest.map_or(0.0, |(_, at)| at.elapsed().as_secs_f64() * 1000.0);
            let mut v: Vec<Shown> = client.others(since).iter().map(shown).collect();
            v.extend(client.own().as_ref().map(shown));
            v
        }
    };
    // Every 2 s: where this side sees every Arisen (for unattended runs).
    let t = time.elapsed_secs();
    if (t % 2.0) < time.delta_secs() {
        let seen: Vec<String> =
            bodies.iter().map(|b| format!("seat {} at ({:.0}, {:.0}, {:.0})", b.id, b.position[0], b.position[1], b.position[2])).collect();
        info!("net: t={t:.1}s sees {}", seen.join(", "));
    }
    play::show(st, &bodies, &mut roots, &mut cams);
    for (&e, &on) in &st.visible {
        if let Ok(mut v) = vis.get_mut(e) {
            *v = if on { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}
