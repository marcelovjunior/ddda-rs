//! Host and client over loopback UDP with simulated latency, jitter and
//! loss (both directions): the client's Arisen moves under the host's
//! authority, its prediction error stays bounded, and once the input
//! stops the client's view and the host's state hash alike.

use ddhost::data::NoStoryData;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddnet::{bodies_hash, Client, Lossy, Server, ServerConfig, Transport, Udp};
use ddsim::{Ground, Input, Sim};
use ddstory::scripted::ScriptedHost;
use std::collections::HashMap;
use std::time::Duration;

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

const BUILD: u32 = 1_513_180_209;
const DATA: u64 = 0xdada;
/// One host frame, ms (30 Hz).
const FRAME_MS: u64 = 33;

type Srv = Server<NoStoryData, ScriptedHost, Lossy<Udp>>;

fn server(latency: u64, jitter: u64, loss: f64) -> Srv {
    let mut host = Host::new(Sim::new(HashMap::new()), None, ScriptedHost::default(), Box::new(Flat));
    host.join(ProfileId(1), Control::Local, [0.0, 0.0, 0.0], 0.0).unwrap();
    let t = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), latency, jitter, loss, 11);
    Server::new(host, t, ServerConfig { build: BUILD, data: DATA, spawn: [0.0, 0.0, 500.0] })
}

fn client(srv: &Srv, profile: u64, data: u64, latency: u64, jitter: u64, loss: f64, seed: u64) -> Client<Lossy<Udp>> {
    let t = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), latency, jitter, loss, seed);
    Client::new(t, srv.transport.local_addr(), profile, BUILD, data, Sim::new(HashMap::new()), Box::new(Flat), 0)
}

/// One frame of both sides at virtual time `now`.
fn frame(srv: &mut Srv, clients: &mut [&mut Client<Lossy<Udp>>], host_in: Input, client_in: Input, now: u64) {
    srv.pump(now);
    for c in clients.iter_mut() {
        c.pump(now);
        c.frame(PlayerInput { sim: client_in, interact: false }, now);
    }
    let mut local = FrameInputs::new();
    local.insert(0, PlayerInput { sim: host_in, interact: false });
    srv.step(&local, now).unwrap();
    // Let the loopback deliver.
    std::thread::sleep(Duration::from_micros(300));
}

#[test]
fn prediction_under_latency_and_loss() {
    let (lat, jit, loss) = (40, 20, 0.10);
    let mut srv = server(lat, jit, loss);
    let mut c = client(&srv, 2, DATA, lat, jit, loss, 23);
    let mut now = 0;
    let idle = Input::default();
    // Join.
    for _ in 0..30 {
        frame(&mut srv, &mut [&mut c], idle, idle, now);
        now += FRAME_MS;
    }
    assert_eq!(c.seat, Some(1), "joined: {:?}", srv.log);
    let mut last = srv.host.sim.player(1).unwrap().position;
    let mut moved = 0.0f32;
    // Six seconds of input: the client runs and turns, the host walks.
    let mut errors = vec![];
    for f in 0..180u64 {
        let t = f as f32;
        let ci = Input { stick: [(t * 0.05).sin(), (t * 0.05).cos()], sprint: f % 90 > 60, jump: f % 70 < 2, ..Default::default() };
        let hi = Input { stick: [0.0, 1.0], walk: true, ..Default::default() };
        frame(&mut srv, &mut [&mut c], hi, ci, now);
        now += FRAME_MS;
        errors.push(c.last_error);
        let p = srv.host.sim.player(1).unwrap().position;
        moved += ((p[0] - last[0]).powi(2) + (p[2] - last[2]).powi(2)).sqrt();
        last = p;
    }
    // The client's commands moved its Arisen on the host (path length).
    assert!(moved > 1000.0, "the host moved the client's Arisen {moved} cm");
    // Settle: no input for three seconds.
    for _ in 0..90 {
        frame(&mut srv, &mut [&mut c], idle, idle, now);
        now += FRAME_MS;
    }
    let (misses, sent, dropped) = (srv.remotes()[0].1, c.transport.sent, c.transport.dropped);
    let mean = errors.iter().sum::<f32>() / errors.len() as f32;
    eprintln!(
        "moved {moved:.0} cm; prediction error max {:.1} cm, mean {mean:.1} cm; snapshots {}; rtt {:.0} ms; host misses {misses}; client sent {sent}, dropped {dropped}; lead changes {}",
        c.max_error,
        c.snapshots,
        c.rtt_ms(),
        c.lead_changes
    );
    assert!(dropped > 0, "the loss is real");
    // Bounded: well under one frame of sprint (22.5 cm/frame) per frame of
    // round trip, and no runaway.
    assert!(c.max_error < 150.0, "max prediction error {} cm", c.max_error);
    assert!(mean < 20.0, "mean prediction error {mean} cm");
    // Agreement after the input stopped.
    let snap = c.latest().unwrap();
    assert_eq!(srv.hash_at(snap.frame), Some(bodies_hash(&snap.bodies)), "client's snapshot equals the host's state");
    assert!(srv.host.frame - snap.frame <= 6, "the client is current: host {} snapshot {}", srv.host.frame, snap.frame);
    let own = c.own().unwrap();
    let auth = snap.bodies.iter().find(|b| b.id == 1).unwrap();
    // The prediction runs ahead of the snapshot by the lead and latency,
    // so only its motion frame counter is ahead; everything else is the
    // host's.
    let mut ahead = own.clone();
    ahead.frame = auth.frame;
    assert_eq!(&ahead, auth, "the prediction has converged");
    assert!(own.frame >= auth.frame);
    assert_eq!(c.last_error, 0.0);
    // The host's Arisen as the client shows it (interpolated, behind).
    let others = c.others(0.0);
    assert_eq!(others.len(), 1);
    assert_eq!(others[0].position, srv.host.sim.player(0).unwrap().position);
}

#[test]
fn join_checks_and_a_full_session() {
    let mut srv = server(5, 0, 0.0);
    let mut bad = client(&srv, 7, DATA ^ 1, 5, 0, 0.0, 3);
    let mut good = client(&srv, 8, DATA, 5, 0, 0.0, 5);
    let mut late = client(&srv, 9, DATA, 5, 0, 0.0, 7);
    let mut again = client(&srv, 8, DATA, 5, 0, 0.0, 9);
    let mut now = 0;
    let idle = Input::default();
    for _ in 0..40 {
        frame(&mut srv, &mut [&mut bad, &mut good], idle, idle, now);
        now += FRAME_MS;
    }
    assert!(bad.refused.as_deref().is_some_and(|r| r.contains("game data")), "{:?}", bad.refused);
    assert_eq!(good.seat, Some(1));
    for _ in 0..40 {
        frame(&mut srv, &mut [&mut late, &mut again, &mut good], idle, idle, now);
        now += FRAME_MS;
    }
    assert!(late.refused.as_deref().is_some_and(|r| r.contains("full")), "{:?}", late.refused);
    assert!(again.refused.is_some(), "the same profile twice");
    // The guest leaves; its seat frees and the host is told.
    good.leave(now);
    for _ in 0..10 {
        frame(&mut srv, &mut [], idle, idle, now);
        // The goodbye leaves the delayed transport when it is polled.
        good.pump(now);
        now += FRAME_MS;
    }
    assert_eq!(srv.host.roster.len(), 1, "{:?}", srv.log);
}

#[test]
fn a_silent_client_times_out() {
    let mut srv = server(0, 0, 0.0);
    let mut c = client(&srv, 2, DATA, 0, 0, 0.0, 3);
    let mut now = 0;
    let idle = Input::default();
    for _ in 0..20 {
        frame(&mut srv, &mut [&mut c], idle, idle, now);
        now += FRAME_MS;
    }
    assert_eq!(srv.host.roster.len(), 2);
    for _ in 0..70 {
        frame(&mut srv, &mut [], idle, idle, now);
        now += FRAME_MS;
    }
    assert_eq!(srv.host.roster.len(), 1);
}
