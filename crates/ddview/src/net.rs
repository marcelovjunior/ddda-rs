//! `--host <port>` and `--join <addr>`: LAN co-op through `ddnet`
//! (docs/adr/0002-game-host.md section 8; protocol 2).
//!
//! - **Host.** Runs the session's `ddhost::Host` (stage collision as
//!   ground) behind a `ddnet::Server`. It plays seat 0 with W A S D and
//!   shows every Arisen from the host's simulation. Its session carries
//!   the fight (`Host::fight`): every seat's attacks, the main pawn (seat
//!   4, a sim player outside the roster) and `--goblin`'s goblins.
//! - **Guest.** Plays its own seat with the same keys. It shows its
//!   Arisen as predicted, and everything else 100 ms behind, interpolated:
//!   the other Arisen, the pawn, the goblins (place, action, motion, HP,
//!   death). Its HUD shows its own and its pawn's HP from the host.
//! - **Looks.** Each side sends its Arisen's look as its save holds it
//!   (edit fields and equipped items): the guest in its join request, the
//!   host for its Arisen and its pawn. Each side dresses the other's
//!   characters from its own install when the look arrives.
//! - **Spot.** The host announces its stage, cell and position. With
//!   [`prejoin`], a guest joins before it loads its world and loads the
//!   host's.
//!
//! Both step at the host's 30 Hz frame, whatever the render rate. The
//! camera follows the local seat.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use ddhost::data::NoStoryData;
use ddhost::{Control, FrameClock, FrameInputs, Host, PlayerInput, ProfileId};
use ddhost::PlayerId;
use ddnet::{LookData, Spot};
use ddstory::scripted::ScriptedHost;

use crate::character::Playback;
use crate::play::{self, FollowCam, PlayState, Shown};
use crate::stage::Loader;

/// The port `--join` uses when the address names none.
pub const DEFAULT_PORT: u16 = 47810;
/// How long [`prejoin`] waits for the host's welcome.
pub const PREJOIN_WAIT: Duration = Duration::from_secs(5);

#[path = "enemy_views.rs"]
pub(crate) mod enemy_views;

type Guest = ddnet::Client<ddnet::Udp>;

enum Side {
    Host(Box<ddnet::Server<NoStoryData, ScriptedHost, ddnet::Udp>>),
    /// A guest not yet asked to join: its join request carries its look,
    /// read from the save on the first frame.
    Joining(Box<Pending>),
    Guest {
        client: Box<Guest>,
        /// The newest snapshot frame and when it arrived.
        newest: Option<(u64, Instant)>,
        told: bool,
    },
}

struct Pending {
    udp: ddnet::Udp,
    server: SocketAddr,
    profile: u64,
    build: u32,
    data: u64,
    sim: ddsim::Sim,
    ground: Box<dyn ddsim::Ground + Send>,
}

/// What rebuilding a look needs from the install.
struct LookSource {
    tables: ddlook::Tables,
    pattern: ddlook::pattern::EditPattern,
    items: Vec<[u8; 0x80]>,
}

struct Session {
    side: Side,
    start: Instant,
    /// Seconds not yet stepped.
    acc: f32,
    /// The host's players before its last frame, to draw between frames.
    between: play::Between,
    log_seen: usize,
    vfs: ddvfs::Vfs,
    mfx: mtshader::ShaderPackage,
    looks: Option<LookSource>,
    /// Seats whose character wears its own look (from the save at start,
    /// or dressed from a received look).
    dressed: BTreeSet<PlayerId>,
    /// The host has announced its own and its pawn's looks.
    own_looks: bool,
    enemy_views: enemy_views::Views,
}

#[derive(Resource)]
pub struct NetRes(Mutex<Session>);

/// What `--host` / `--join` asked for.
pub enum Mode {
    Host { port: u16 },
    Join { addr: String },
}

/// Bevy's asset stores, for dressing characters when a look arrives.
#[derive(SystemParam)]
pub struct Dress<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
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

fn resolve(addr: &str) -> Result<SocketAddr, String> {
    let with_port = if addr.contains(':') { addr.to_string() } else { format!("{addr}:{DEFAULT_PORT}") };
    with_port
        .to_socket_addrs()
        .map_err(|e| format!("{with_port}: {e}"))?
        .find(|a| a.is_ipv4())
        .ok_or(format!("{with_port}: no IPv4 address"))
}

/// The looks of the save's Arisen and main pawn, as the protocol carries
/// them.
fn saved_looks(vfs: &ddvfs::Vfs, xml: Option<&str>) -> (Option<LookData>, Option<LookData>) {
    let Some(xml) = xml else { return (None, None) };
    let saved = ddlook::save::save_looks(vfs, xml, "mPlayerDataManual").unwrap_or_default();
    let data = |who: &str| saved.iter().find(|s| s.who == who).map(|s| LookData { edit: s.edit.clone(), items: s.items.clone() });
    (data("arisen"), data("pawn0"))
}

/// Item records of `etc\item\itemList` (`rItemList`: magic `ITL2`, stamp,
/// count, a reserved word, then 128-byte records; as `dditem::itl`).
fn item_records(vfs: &ddvfs::Vfs) -> Result<Vec<[u8; 0x80]>, String> {
    let b = vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList"))?;
    if b.get(..4) != Some(&b"ITL2"[..]) {
        return Err("itemList: not ITL2".into());
    }
    let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let body = b.get(16..16 + n * 0x80).ok_or("itemList: truncated")?;
    Ok(body.chunks_exact(0x80).map(|c| c.try_into().unwrap()).collect())
}

// ------------------------------------------------------------------ prejoin

/// A guest's connection made before its world loads, kept alive by a
/// thread until [`setup`] takes it.
static PREJOINED: Mutex<Option<Arc<Mutex<Option<Guest>>>>> = Mutex::new(None);

/// No ground yet (the guest's world is not loaded): nothing to stand on.
struct NoGround;
impl ddsim::Ground for NoGround {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

/// Joins before the world loads and moves the viewer to the
/// host's spot (stage, origin cell, spawn). The connection stays open,
/// answering the host, until [`setup`] takes it. Without a welcome within
/// [`PREJOIN_WAIT`] the arguments stay as they were, and [`setup`] joins
/// later.
fn prejoin_spot(native_pc: &Path, addr: &str, profile: Option<u64>, save_xml: Option<&str>) -> Option<Spot> {
    let server = resolve(addr).ok()?;
    let vfs = ddvfs::Vfs::open(native_pc).ok()?;
    let sim = ddhost::install::sim(&vfs).ok()?;
    let build = exe_build(native_pc);
    let data = ddnet::data_fingerprint(native_pc).ok()?;
    let (look, _) = saved_looks(&vfs, save_xml);
    let udp = ddnet::Udp::bind("0.0.0.0:0").ok()?;
    let profile = profile.unwrap_or_else(|| std::process::id() as u64 | 1 << 32);
    let start = Instant::now();
    let mut c = ddnet::Client::new(udp, server, profile, build, data, look, sim, Box::new(NoGround), 0);
    let now = |s: Instant| s.elapsed().as_millis() as u64;
    while start.elapsed() < PREJOIN_WAIT && (c.seat.is_none() || c.spot.is_none()) && c.refused.is_none() {
        c.pump(now(start));
        c.frame(PlayerInput::default(), now(start));
        std::thread::sleep(Duration::from_millis(33));
    }
    if let Some(r) = &c.refused {
        eprintln!("net: refused: {r}");
        return None;
    }
    let spot = c.spot?;
    c.seat?;
    // Keep answering the host while the world loads (2 s of silence would
    // unseat the guest).
    let shared = Arc::new(Mutex::new(Some(c)));
    *PREJOINED.lock().unwrap() = Some(shared.clone());
    std::thread::spawn(move || loop {
        {
            let mut g = shared.lock().unwrap();
            let Some(c) = g.as_mut() else { break };
            c.pump(now(start));
            c.frame(PlayerInput::default(), now(start));
        }
        std::thread::sleep(Duration::from_millis(33));
    });
    eprintln!("net: prejoined; the host is on stage {} cell {:?}", spot.stage, spot.cell);
    Some(spot)
}

/// `--join`: joins before the world loads ([`prejoin_spot`]) and starts the
/// viewer at the host's spot: its stage, origin cell and spawn. Other
/// modes pass through.
pub fn prejoin(mut args: crate::Args) -> crate::Args {
    let Some(addr) = args.join.clone() else { return args };
    let xml = args.save_xml.clone();
    if let Some(spot) = prejoin_spot(&args.native_pc, &addr, args.profile, xml.as_deref().map(|x| x.as_str())) {
        args.stage = spot.stage;
        args.cell = spot.cell;
        let origin = Vec3::new((spot.cell.1 - 50) as f32 * 10000.0, 0.0, (spot.cell.0 - 50) as f32 * 10000.0);
        args.spawn = Vec3::from(spot.pos) - origin;
    }
    args
}

pub(crate) fn take_prejoined() -> Option<Guest> {
    let shared = PREJOINED.lock().unwrap().take()?;
    let mut g = shared.lock().unwrap();
    g.take()
}

// ------------------------------------------------------------------ setup

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
    (spawn, yaw): (Vec3, f32),
    st: &mut PlayState,
) -> Result<(), String> {
    let sim = ddhost::install::sim(vfs)?;
    let mut ground = ddworld::Terrain::load_collision(vfs, stage, a0, b0, radius);
    ground.add_scenery(vfs, stage, a0, b0, radius);
    let build = exe_build(native_pc);
    let data = ddnet::data_fingerprint(native_pc).map_err(|e| e.to_string())?;
    info!("net: exe build {build}, data fingerprint {data:016x}");
    let mut dressed = BTreeSet::new();
    let side = match mode {
        Mode::Host { port } => {
            let mut host = Host::new(sim, None, ScriptedHost::default(), Box::new(ground));
            host.join(ProfileId(profile.unwrap_or(1)), Control::Local, spawn.to_array(), yaw)
                .ok_or("cannot seat the host")?;
            // The play state's fight moves into the host: seat 0's kit, the
            // pawn (its body copied into the host's simulation) and the
            // goblins.
            if let Some(mut f) = st.fight.take() {
                f.equip(&mut host.sim, 0);
                let pawns: Vec<u8> = f.pawns.keys().copied().collect();
                for seat in pawns {
                    if let Some(p) = st.sim.player(seat) {
                        host.sim.add_player(seat, p.position, p.yaw);
                        if let Some(q) = host.sim.player_mut(seat) {
                            q.relaxed = true;
                        }
                    }
                    f.equip(&mut host.sim, seat);
                    dressed.insert(seat);
                }
                f.volumes(&host.sim);
                host.fight = Some(f);
            }
            dressed.insert(0);
            let udp = ddnet::Udp::bind(&format!("0.0.0.0:{port}")).map_err(|e| format!("bind port {port}: {e}"))?;
            info!("net: hosting on port {port}");
            let spot = Spot { stage, cell: (a0, b0), pos: spawn.to_array(), yaw };
            let config = ddnet::ServerConfig { build, data, spawn: spot.pos, spot: Some(spot) };
            st.follow = 0;
            Side::Host(Box::new(ddnet::Server::new(host, udp, config)))
        }
        Mode::Join { addr } => {
            // The host runs the fight: the guest's own goblins and pawn
            // logic go; the views stay for the replicated ones.
            st.fight = None;
            for old in st.goblins.drain(..) {
                commands.entity(old.entity).despawn();
                st.visible.remove(&old.entity);
            }
            match take_prejoined() {
                Some(mut client) => {
                    client.set_ground(Box::new(ground));
                    Side::Guest { client: Box::new(client), newest: None, told: false }
                }
                None => {
                    let server = resolve(&addr)?;
                    let udp = ddnet::Udp::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
                    let profile = profile.unwrap_or_else(|| std::process::id() as u64 | 1 << 32);
                    info!("net: joining {server} as profile {profile:x}");
                    Side::Joining(Box::new(Pending { udp, server, profile, build, data, sim, ground: Box::new(ground) }))
                }
            }
        }
    };
    let own = ddvfs::Vfs::open(native_pc)?;
    let mfx_path = native_pc.join("usershader").join("BbsShaderPackage.mfx");
    let mfx = std::fs::read(&mfx_path)
        .map_err(|e| format!("{}: {e}", mfx_path.display()))
        .and_then(|b| mtshader::ShaderPackage::parse(&b).map_err(|e| format!("{e:?}")))?;
    let session = Session {
        side,
        start: Instant::now(),
        acc: 0.0,
        between: play::Between::default(),
        log_seen: 0,
        vfs: own,
        mfx,
        looks: None,
        dressed,
        own_looks: false,
        enemy_views: enemy_views::Views::default(),
    };
    commands.insert_resource(NetRes(Mutex::new(session)));
    Ok(())
}

fn shown(b: &ddnet::Body) -> Shown {
    // the protocol does not carry the weapon: attacks pick theirs from
    // the state (play::show)
    Shown { id: b.id, position: b.position, yaw: b.yaw, state: b.state, frame: b.frame, weapon: ddsim::Weapon::None }
}

/// Frame of motion `m` to show at frame `f` (as play.rs): loops wrap,
/// one-shots hold their last frame.
fn display_frame(m: &mtlmt::Motion, f: f32) -> f32 {
    let last = m.frame_count.saturating_sub(1).max(1) as f32;
    if f <= last {
        return f;
    }
    if m.loop_frame >= 0 && (m.loop_frame as f32) < last {
        let lp = m.loop_frame as f32;
        lp + (f - lp).rem_euclid(last - lp)
    } else {
        last
    }
}

/// Dresses the characters of seats whose look has arrived.
fn dress(
    s: &mut Session,
    st: &mut PlayState,
    looks: &BTreeMap<PlayerId, LookData>,
    d: &mut Dress,
    roots: &Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
) {
    let due: Vec<(PlayerId, LookData)> =
        looks.iter().filter(|(k, _)| !s.dressed.contains(k) && st.entities.contains_key(k)).map(|(k, v)| (*k, v.clone())).collect();
    if due.is_empty() {
        return;
    }
    if s.looks.is_none() {
        let src = (|| -> Result<LookSource, String> {
            Ok(LookSource {
                tables: ddlook::Tables::load(&s.vfs).map_err(|e| format!("{e:?}"))?,
                pattern: ddlook::pattern::EditPattern::load(&s.vfs).map_err(|e| format!("{e:?}"))?,
                items: item_records(&s.vfs)?,
            })
        })();
        match src {
            Ok(x) => s.looks = Some(x),
            Err(e) => {
                error!("net: looks: {e}");
                for (k, _) in due {
                    s.dressed.insert(k);
                }
                return;
            }
        }
    }
    let src = s.looks.as_ref().unwrap();
    let mut loader = Loader {
        vfs: &s.vfs,
        mfx: &s.mfx,
        textures: Default::default(),
        models: Default::default(),
        model_names: Default::default(),
        water: None,
        stats: Default::default(),
        lod_eye: None,
        view_range: 1,
        tag: None,
    };
    for (seat, data) in due {
        s.dressed.insert(seat);
        // Seats 0-3 are Arisen, 4 and up pawns (play::pawn_seat).
        let unit = if seat >= 4 { ddlook::Unit::Pawn } else { ddlook::Unit::Player };
        let Some(look) = ddlook::save::edit_look(&s.vfs, &src.tables, &src.pattern, &src.items, unit, &data.edit, &data.items) else {
            warn!("net: seat {seat}: the look does not build");
            continue;
        };
        let old = st.entities[&seat];
        let at = roots.get(old).map_or(Vec3::ZERO, |(t, _)| t.translation);
        match crate::character::spawn_look(
            &mut d.commands,
            &mut loader,
            &mut d.meshes,
            &mut d.materials,
            &mut d.images,
            &mut d.bindposes,
            &look,
            play::MOTIONS,
            0,
            at,
            Some(0.0),
        ) {
            Ok(e) => {
                d.commands.entity(old).despawn();
                st.entities.insert(seat, e);
                st.visible.remove(&old);
                info!("net: seat {seat} dressed from its look");
            }
            Err(e) => warn!("net: seat {seat}: {e}"),
        }
    }
}

/// One render frame: step the session at 30 Hz, then show every Arisen.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn step(
    pause: Res<crate::pause::PauseState>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    args: Res<crate::Args>,
    state: Option<ResMut<PlayState>>,
    net: Option<Res<NetRes>>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
    mut vis: Query<&mut Visibility>,
    mut hud: ResMut<play::Hud>,
    mut demo_press: Local<bool>,
    mut d: Dress,
) {
    let (Some(mut st), Some(net)) = (state, net) else { return };
    let st = &mut *st;
    if !pause.paused && pause.mouse_look(buttons.pressed(MouseButton::Right)) { play::orbit(st, &buttons, &mouse); }
    let mut input = if pause.paused { PlayerInput::default() } else {
        PlayerInput { sim: play::p1_input(&keys, &buttons, st.cam_yaw), interact: keys.pressed(KeyCode::KeyE) }
    };
    if play::active_input(&input.sim) || input.interact { st.demo = false; }
    if !pause.paused && st.demo && !st.goblins.is_empty() && args.host.is_some() {
        // with goblins on the host, the demo's seat 0 fights them; walked
        // per host frame below
    } else if !pause.paused && st.demo {
        // Unattended: walk in a small circle near the spawn (about 1.3 m
        // radius at the walk speed).
        let t = time.elapsed_secs();
        input.sim = ddsim::Input { stick: [(t * 1.2).sin(), (t * 1.2).cos()], walk: true, camera_yaw: st.cam_yaw, ..default() };
    }
    let mut guard = net.0.lock().unwrap();
    let s = &mut *guard;
    let now = s.start.elapsed().as_millis() as u64;
    let frame = FrameClock::default().seconds();
    s.acc = (s.acc + time.delta_secs()).min(0.25);
    // A deferred guest joins now, its look read from the save.
    if matches!(s.side, Side::Joining(_)) {
        let Side::Joining(p) = std::mem::replace(&mut s.side, Side::Joining(Box::new(dummy_pending()))) else { unreachable!() };
        let p = *p;
        let (look, _) = saved_looks(&s.vfs, args.save_xml.as_deref().map(|x| x.as_str()));
        let client = ddnet::Client::new(p.udp, p.server, p.profile, p.build, p.data, look, p.sim, p.ground, now);
        s.side = Side::Guest { client: Box::new(client), newest: None, told: false };
    }
    let mut enemies = vec![];
    let mut looks = BTreeMap::new();
    let bodies: Vec<Shown> = match &mut s.side {
        Side::Host(server) => {
            if !s.own_looks {
                s.own_looks = true;
                let (arisen, pawn) = saved_looks(&s.vfs, args.save_xml.as_deref().map(|x| x.as_str()));
                if let Some(l) = arisen {
                    server.set_look(0, l);
                }
                if let Some(l) = pawn {
                    server.set_look(play::pawn_seat(0), l);
                }
            }
            server.pump(now);
            while s.acc >= frame {
                s.acc -= frame;
                s.between.snapshot(&server.host.sim);
                // As --play: the start area's relaxed run, a dagger's
                // second jump (docs/MOVEMENT.md).
                for p in &mut server.host.sim.players {
                    p.relaxed = true;
                    p.double_jump = true;
                }
                // guests that joined fight too (a Fighter kit each)
                if let Some(f) = server.host.fight.as_mut() {
                    let seats: Vec<u8> = server.host.roster.ids().collect();
                    for seat in seats {
                        if !f.arisen.contains_key(&seat) {
                            f.add_arisen(seat);
                            f.equip(&mut server.host.sim, seat);
                            if let Some(a) = f.arisen.get(&seat) {
                                st.hp_max.insert(seat, f.kit(a.kit).hp_max);
                            }
                            info!("net: seat {seat} joins the fight");
                        }
                    }
                }
                let mut local = FrameInputs::new();
                let mut i = input;
                if !pause.paused && st.demo && !st.goblins.is_empty() {
                    let mut press = *demo_press;
                    i.sim = play::demo_attack(&server.host.sim, server.host.fight.as_ref(), 0, st.cam_yaw, &mut press);
                    *demo_press = press;
                }
                local.insert(0, i);
                if let Err(e) = server.step(&local, now) {
                    error!("net: host frame: {e}");
                }
            }
            for line in &server.log[s.log_seen..] {
                info!("net: {line}");
            }
            s.log_seen = server.log.len();
            looks = server.looks().clone();
            s.between.shown(&server.host.sim, (s.acc / frame).clamp(0.0, 1.0))
        }
        Side::Joining(_) => vec![],
        Side::Guest { client, newest, told } => {
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
                // the guest's own seat wears its save from the start (main.rs)
                s.dressed.insert(seat);
            }
            if let Some(l) = client.latest() {
                if newest.is_none_or(|(f, _)| f != l.frame) {
                    *newest = Some((l.frame, Instant::now()));
                }
            }
            let since = newest.map_or(0.0, |(_, at)| at.elapsed().as_secs_f64() * 1000.0);
            enemies = client.enemies(since);
            looks = client.looks.clone();
            let mut v: Vec<Shown> = client.others(since).iter().map(shown).collect();
            v.extend(client.own().as_ref().map(shown));
            v
        }
    };
    dress(s, st, &looks, &mut d, &roots);
    // Every 2 s: where this side sees every Arisen and goblin (for
    // unattended runs).
    let t = time.elapsed_secs();
    if (t % 2.0) < time.delta_secs() {
        let mut seen: Vec<String> =
            bodies.iter().map(|b| format!("seat {} at ({:.0}, {:.0}, {:.0})", b.id, b.position[0], b.position[1], b.position[2])).collect();
        let shown_enemies = match &s.side {
            Side::Host(server) => ddnet::mirror::from_fight(server.host.fight.as_ref()).1,
            _ => enemies.clone(),
        };
        seen.extend(shown_enemies.iter().map(|e| format!("{:?} owner {} hp {:.0}{}",e.identity.family,e.id,e.hp,if e.dead { " dead" } else { "" })));
        info!("net: t={t:.1}s sees {}; dressed {:?}", seen.join(", "), s.dressed);
    }
    play::show(st, &bodies, &mut roots, &mut cams);
    match &s.side {
        Side::Host(server) => {
            play::show_goblins(st, server.host.fight.as_ref(), &mut roots, 1.0);
            hud.rows = play::hud_rows(st, &server.host.sim, server.host.fight.as_ref(), None);
            hud.enemy = play::enemy_gauge(st, server.host.fight.as_ref());
        }
        Side::Guest { client, .. } => {
            let mut hp = HashMap::new();
            for v in client.vitals() {
                hp.insert(v.id, v.hp);
                st.hp_max.insert(v.id, v.hp_max);
            }
            hud.rows = play::hud_rows(st, &client.sim, None, Some(&hp));
            for e in &enemies {
                hud.rows.push((format!("{:?} HP",e.identity.family), e.hp, e.hp_max, Color::srgb(0.75, 0.15, 0.12)));
            }
            let mut loader = Loader {
                vfs: &s.vfs, mfx: &s.mfx, textures: Default::default(), models: Default::default(),
                model_names: Default::default(), water: None, stats: Default::default(),
                lod_eye: None, view_range: 1, tag: None,
            };
            s.enemy_views.sync(&enemies,st.origin,&mut d.commands,&mut loader,&mut d.meshes,
                &mut d.materials,&mut d.images,&mut d.bindposes);
            s.enemy_views.show(&enemies,st.origin,false,&mut roots,&mut st.visible);
        }
        Side::Joining(_) => {}
    }
    for (&e, &on) in &st.visible {
        if let Ok(mut v) = vis.get_mut(e) {
            *v = if on { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

/// A placeholder while a deferred guest is moved out of its slot.
fn dummy_pending() -> Pending {
    Pending {
        udp: ddnet::Udp::bind("127.0.0.1:0").expect("bind a loopback port"),
        server: SocketAddr::from(([127, 0, 0, 1], 0)),
        profile: 0,
        build: 0,
        data: 0,
        sim: ddsim::Sim::new(Default::default()),
        ground: Box::new(NoGround),
    }
}
