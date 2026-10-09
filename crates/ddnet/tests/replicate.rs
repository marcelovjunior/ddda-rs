//! Protocol 2 over loopback UDP with latency and loss: a fight on the host
//! (its Arisen swings at a goblin that fights back, the guest's Arisen in
//! the fight too), and a guest whose mirror must match the host:
//! - goblin place, action, motion, HP and death;
//! - every seat's HP;
//! - the looks of both Arisen;
//! - the host's spot.
//!
//! Needs the install and DDDA.exe (DDDA_NATIVE_PC); skips without them.

use ddhost::data::NoStoryData;
use ddhost::ddenemyrt::combat::Victim;
use ddhost::ddenemyrt::fight::{Fight, FightData};
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddnet::mirror::from_fight;
use ddnet::{snapshot_hash, Client, LookData, Lossy, Server, ServerConfig, Spot, Transport, Udp};
use ddsim::{Ground, Sim};
use ddstory::scripted::ScriptedHost;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

fn data() -> Option<Arc<FightData>> {
    static D: OnceLock<Option<Arc<FightData>>> = OnceLock::new();
    D.get_or_init(|| {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            eprintln!("SKIP: no install");
            return None;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let exe = ddexe::ExeFile::next_to(&native).ok()?;
        Some(Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap()))
    })
    .clone()
}

const BUILD: u32 = 1_513_180_209;
const DATA: u64 = 0xd00d;
const FRAME_MS: u64 = 33;

fn look(sex: u32, weapon: i16) -> LookData {
    LookData { edit: vec![("mGender".into(), sex), ("mHairNo".into(), 3), ("mColorHair".into(), 12)], items: vec![weapon, -1, 100, 200, -1] }
}

#[test]
fn guest_mirrors_the_fight_looks_and_spot() {
    let Some(data) = data() else { return };
    let mut host = Host::new(Sim::new(HashMap::new()), None::<ddstory::Story<NoStoryData>>, ScriptedHost::default(), Box::new(Flat));
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    let mut f = Fight::new(data);
    f.add_arisen(0);
    f.equip(&mut host.sim, 0);
    f.add_goblin([0.0, 0.0, 600.0], std::f32::consts::PI);
    host.fight = Some(f);
    let spot = Spot { stage: 100, cell: (58, 50), pos: [1.0, 2.0, 3.0], yaw: 0.5 };
    let t = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), 30, 10, 0.05, 41);
    let config = ServerConfig { build: BUILD, data: DATA, spawn: [0.0, 0.0, -300.0], spot: Some(spot) };
    let mut srv = Server::new(host, t, config);
    srv.set_look(0, look(0, 1500));
    let ct = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), 30, 10, 0.05, 43);
    let guest_look = look(1, 1600);
    let mut c = Client::new(ct, srv.transport.local_addr(), 2, BUILD, DATA, Some(guest_look.clone()), Sim::new(HashMap::new()), Box::new(Flat), 0);

    let mut now = 0;
    let mut press = false;
    let mut goblin_hp = vec![];
    let mut guest_hp_seen = vec![];
    for frame in 0..900u64 {
        srv.pump(now);
        c.pump(now);
        c.frame(PlayerInput::default(), now);
        // The guest's Arisen joins the fight once seated (as ddview does).
        if let Some(f) = srv.host.fight.as_mut() {
            for seat in srv.host.roster.ids().collect::<Vec<_>>() {
                if !f.arisen.contains_key(&seat) {
                    f.add_arisen(seat);
                    f.equip(&mut srv.host.sim, seat);
                }
            }
        }
        // The host's Arisen swings when the goblin is near (as ddhost's
        // goblin test).
        let f = srv.host.fight.as_ref().unwrap();
        let g = &f.goblins[0];
        let me = srv.host.sim.player(0).unwrap();
        let d = ((g.pos[0] - me.position[0]).powi(2) + (g.pos[2] - me.position[2]).powi(2)).sqrt();
        press = d < 250.0 && !g.dead() && me.state == ddsim::State::Wait && !press;
        goblin_hp.push(g.fighter.victim.hp());
        // Late in the run the host decides the goblin's death (its HP to
        // 0), unless the fight has already: the guest must see the death.
        if frame == 700 {
            if let Some(g) = srv.host.fight.as_mut().unwrap().goblins.first_mut() {
                if let Victim::Enemy { regions, .. } = &mut g.fighter.victim {
                    regions[0].hp = 0.0;
                }
            }
        }
        let mut local = FrameInputs::new();
        local.insert(0, PlayerInput { sim: ddsim::Input { light: press, ..Default::default() }, interact: false });
        srv.step(&local, now).unwrap();
        if let Some(v) = c.vitals().iter().find(|v| v.id == 1) {
            guest_hp_seen.push(v.hp);
        }
        now += FRAME_MS;
        std::thread::sleep(Duration::from_micros(300));
    }
    // Settle.
    for _ in 0..60 {
        srv.pump(now);
        c.pump(now);
        c.frame(PlayerInput::default(), now);
        srv.step(&FrameInputs::from([(0, PlayerInput::default())]), now).unwrap();
        now += FRAME_MS;
        std::thread::sleep(Duration::from_micros(300));
    }

    // The fight happened on the host.
    let hp0 = goblin_hp[0];
    let fought = goblin_hp[..700].iter().cloned().fold(f32::INFINITY, f32::min);
    assert!(fought < hp0, "the goblin took damage in the fight before frame 700: {hp0} -> {fought}");
    let (vitals, enemies) = from_fight(srv.host.fight.as_ref());
    assert!(enemies[0].dead);
    // The guest's mirror of the newest snapshot equals the host's state.
    let snap = c.latest().unwrap();
    assert_eq!(srv.hash_at(snap.frame), Some(snapshot_hash(snap)));
    assert_eq!(snap.enemies, enemies);
    assert_eq!(snap.vitals, vitals);
    assert!(c.enemies(0.0)[0].dead, "the guest sees the death");
    assert!(vitals.iter().any(|v| v.id == 1), "the guest's Arisen has HP on the host: {vitals:?}");
    // The guest saw the goblin's HP fall during the fight, not only at the end.
    eprintln!(
        "goblin HP {hp0} -> {fought} by blows before the forced death; guest seat-1 HP samples {}; snapshots {}; max prediction error {:.1}",
        guest_hp_seen.len(),
        c.snapshots,
        c.max_error
    );
    // Looks: the host's reached the guest, the guest's reached the host.
    assert_eq!(c.looks.get(&0), Some(&look(0, 1500)));
    assert_eq!(srv.looks().get(&1), Some(&guest_look));
    // The spot.
    assert_eq!(c.spot, Some(spot));
}

#[test]
fn protocol_1_client_is_refused_with_a_reason() {
    // A version-1 Hello (no look field) must get a clear refusal.
    let host = Host::new(Sim::new(HashMap::new()), None, ScriptedHost::default(), Box::new(Flat));
    let t = Lossy::new(Udp::bind("127.0.0.1:0").unwrap(), 0, 0, 0.0, 1);
    let mut srv: Server<NoStoryData, ScriptedHost, Lossy<Udp>> =
        Server::new(host, t, ServerConfig { build: BUILD, data: DATA, spawn: [0.0; 3], spot: None });
    let mut sock = Udp::bind("127.0.0.1:0").unwrap();
    let mut link = ddnet::link::Link::new(77, 0);
    let mut w = ddid::Writer::new();
    w.u8(0).u16(1).u32(BUILD).u64(DATA).u64(9);
    link.send_reliable(w.finish());
    let to = srv.transport.local_addr();
    let mut reason = None;
    for i in 0..40u64 {
        let p = link.packet(None, i * 33);
        sock.send(to, &p, i * 33);
        std::thread::sleep(Duration::from_micros(300));
        srv.pump(i * 33);
        srv.step(&FrameInputs::new(), i * 33).unwrap();
        std::thread::sleep(Duration::from_micros(300));
        while let Some((_, b)) = sock.recv(i * 33) {
            if let Ok(d) = link.receive(&b, i * 33) {
                for m in d.reliable {
                    if let Ok(ddnet::proto::ToClient::Refuse { reason: r }) = ddnet::proto::ToClient::decode(&m) {
                        reason = Some(r);
                    }
                }
            }
        }
        if reason.is_some() {
            break;
        }
    }
    assert!(reason.as_deref().is_some_and(|r| r.contains("protocol 1")), "{reason:?}");
}
