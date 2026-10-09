//! `--play`: the headless simulation (`ddsim`) and the fight
//! (`ddenemyrt`: attacks, hits, the pawn's AI, goblins) driven by local
//! input, with every character shown.
//!
//! | | P1 | P2 |
//! |---|---|---|
//! | move (runs) | W A S D | arrow keys |
//! | walk | Left Ctrl | Right Ctrl |
//! | sprint | Left Shift | Right Shift |
//! | jump (twice: double jump) | Space | Enter |
//! | light attack | J or left mouse | Numpad 1 |
//! | heavy attack | K or middle mouse | Numpad 2 |
//!
//! The camera orbits P1 (hold right mouse to turn it). F2 switches every
//! player between the town (relaxed) walk and run and the field ones; the
//! area attribute that picks them in the original is not read yet.
//!
//! With `--save DDDA.sav` the session starts at the save's spot with its
//! Arisen and main pawn (dressed by `ddlook`); `--goblin` adds a goblin.
//! The world steps at the host's fixed 30 Hz frame in the order of
//! docs/FRAME.md (as `ddhost::Host::tick`).

use std::collections::HashMap;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use ddhost::ddenemyrt::body::MotionId;
use ddhost::ddenemyrt::fight::{Fight, FightData};
pub use ddhost::ddenemyrt::fight::Kit;
use ddsim::{Input, PlayerId, Sim, Weapon};

use crate::character::{Playback, RMOTIONLIST};
use crate::stage::{Loader, UNIT};

/// Bank 0 motion list of the male Arisen (docs/ANIMATION.md).
pub const MOTIONS: &str = ddhost::install::MOTIONS;
pub const BODY: &str = r"model\pl\m\m_base\m000\m000";
/// The goblin's model (`em0100`); its motion banks are
/// `ddenemyrt::goblin::BANKS`.
pub const GOBLIN_MODEL: &str = r"model\em\e01\e0100\e0100";

/// The seat of the pawn serving Arisen seat `s` (sim players outside the
/// host's roster, which seats 0-3).
pub fn pawn_seat(owner: PlayerId) -> PlayerId {
    4 + owner
}

/// One loaded motion list for the view: its motions and its default
/// interpolation table (the `rAdjustParam` under the same path,
/// docs/ANIMATION.md).
struct ViewBank {
    list: mtlmt::MotionList,
    blend: Vec<f32>,
}

/// The motions the view plays: the common banks (0 and 6, as
/// `ddhost::install::BANKS`) and each weapon's banks 6 and 7, looked up
/// as ddsim does (`Sim::motion_for`: the weapon's first).
#[derive(Default)]
pub struct ViewMotions {
    common: HashMap<u8, ViewBank>,
    weapon: HashMap<Weapon, HashMap<u8, ViewBank>>,
}

impl ViewMotions {
    fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let mut v = ViewMotions::default();
        for (bank, path) in ddhost::install::BANKS {
            v.common.insert(bank as u8, view_bank(vfs, path)?);
        }
        // the weapon categories' lists bound at 0x600 / 0x700 (table
        // 0x0163DF8C): sword m0001, staff m0007 (as ddenemyrt::arisen)
        for (w, n) in [(Weapon::Sword, "0001"), (Weapon::Staff, "0007")] {
            let mut m = HashMap::new();
            for (bank, kind) in [(6u8, "co"), (7u8, "at")] {
                m.insert(bank, view_bank(vfs, &format!(r"motion\pl\m\m00\m{n}_{kind}\m{n}_{kind}"))?);
            }
            v.weapon.insert(w, m);
        }
        Ok(v)
    }

    fn bank(&self, weapon: Weapon, bank: u8) -> Option<&ViewBank> {
        let w = self.weapon.get(&weapon).and_then(|m| m.get(&bank));
        w.filter(|b| !b.list.motions.is_empty()).or_else(|| self.common.get(&bank))
    }

    /// The motion `id` (`bank << 8 | slot`) for `weapon`, and its default
    /// interpolation frames.
    fn get(&self, weapon: Weapon, id: u16) -> Option<(&mtlmt::Motion, f32)> {
        let bank = (id >> 8) as u8;
        let slot = usize::from(id & 0xff);
        fn find(b: &ViewBank, slot: usize, id: u16) -> Option<(&mtlmt::Motion, f32)> {
            let m = b.list.motions.get(slot)?.as_ref()?;
            Some((m, mtlmt::blend::default_frames(&b.blend, id).unwrap_or(0.0)))
        }
        let find = |b| find(b, slot, id);
        self.weapon
            .get(&weapon)
            .and_then(|m| m.get(&bank))
            .and_then(find)
            .or_else(|| self.common.get(&bank).and_then(find))
            .or_else(|| self.bank(weapon, bank).and_then(find))
    }
}

fn view_bank(vfs: &ddvfs::Vfs, path: &str) -> Result<ViewBank, String> {
    let list = mtlmt::parse(&vfs.read(path, RMOTIONLIST)?).map_err(|e| format!("{path}: {e}"))?;
    // the bank table (0x014F1D30) loads the same path as both rMotionList
    // and rAdjustParam (0x00B7FF30)
    let blend = ddhost::install::adjust_table(vfs, path).unwrap_or_default();
    Ok(ViewBank { list, blend })
}

/// Who plays: seat 0's Arisen (the save's, else the base body), more
/// local Arisen, the main pawn, the goblins.
pub struct Party {
    /// Local Arisen seats and their looks (`None`: the base male body).
    pub arisen: Vec<(PlayerId, Option<ddlook::Look>, Vec3, f32)>,
    /// The main pawn (serving seat 0): look, position, yaw.
    pub pawn: Option<(Option<ddlook::Look>, Vec3, f32)>,
    /// Goblins to add (`--goblin`).
    pub goblins: usize,
    /// The save's primary weapon category of each Arisen seat (for its
    /// kit): sword or staff.
    pub kits: HashMap<PlayerId, Kit>,
}

/// The fight's kit for a look's primary weapon: a staff plays the Mage's
/// kit, anything else the Fighter's (sword attacks; the other categories'
/// transition lists are not in ddsim).
pub fn kit_of(look: Option<&ddlook::Look>) -> Kit {
    let cat = look.and_then(|l| l.weapons.first()).map(|w| (w.id as u32 >> 24) & 0xff);
    if cat == Some(5) {
        Kit::Mage
    } else {
        Kit::Fighter
    }
}

/// A goblin's view.
pub struct GoblinView {
    pub entity: Entity,
    shown: Option<MotionId>,
    pub hp_max: f32,
}

#[derive(Resource)]
pub struct PlayState {
    pub sim: Sim,
    pub terrain: ddworld::Terrain,
    /// Stage-frame position (cm) of the viewer's origin.
    pub origin: Vec3,
    pub motions: ViewMotions,
    pub entities: HashMap<PlayerId, Entity>,
    pub follow: PlayerId,
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    /// Scripted input for unattended tests: without goblins P1 runs and
    /// jumps and P2 sprints; with one, P1 closes in and attacks it.
    pub demo: bool,
    /// Motion each character's Playback currently shows.
    shown: HashMap<PlayerId, u16>,
    /// Whether each character has an Arisen to show (network sessions
    /// spawn characters for every seat up front).
    pub visible: HashMap<Entity, bool>,
    /// The local fight (attacks, hits, pawn, goblins); `None` without the
    /// exe's data.
    pub fight: Option<Fight>,
    /// Goblin views, in the fight's goblin order.
    pub goblins: Vec<GoblinView>,
    /// The goblin data for the view (its motion banks), when loaded.
    pub fight_data: Option<std::sync::Arc<FightData>>,
    /// Seconds not yet stepped at the fixed frame.
    acc: f32,
    /// Local Arisen seats (P1, P2).
    pub locals: Vec<PlayerId>,
    /// HP of each seat at full (for the HUD).
    pub hp_max: HashMap<PlayerId, f32>,
    /// Last light press of the demo (presses are edges).
    demo_press: bool,
    /// The goblin kills logged so far.
    kills: usize,
    /// Hits dealt so far by attacker owner id: (count, damage).
    pub dealt: HashMap<u32, (u32, f32)>,
}

#[derive(Component)]
pub struct FollowCam;

/// The fight data from the install and the exe next to it.
pub fn fight_data(vfs: &ddvfs::Vfs, native_pc: &std::path::Path) -> Result<std::sync::Arc<FightData>, String> {
    let exe = ddexe::ExeFile::next_to(native_pc).map_err(|e| e.0)?;
    let image = exe.image().map_err(|e| e.0)?;
    Ok(std::sync::Arc::new(FightData::load(vfs, &image)?))
}

/// Ground height under (x, z) near `y`, else `y`.
fn ground_at(t: &ddworld::Terrain, x: f32, z: f32, y: f32) -> f32 {
    use ddsim::Ground;
    t.height(x, z, y + 300.0).unwrap_or(y)
}

/// Builds the simulation and the fight, and spawns the characters.
#[allow(clippy::too_many_arguments)]
pub fn setup(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
    terrain: ddworld::Terrain,
    origin: Vec3,
    party: Party,
    data: Option<std::sync::Arc<FightData>>,
) -> Result<PlayState, String> {
    let mut sim = ddhost::install::sim(loader.vfs)?;
    let motions = ViewMotions::load(loader.vfs)?;
    info!("ground speeds: {:?}", sim.move_param);
    let mut fight = data.clone().map(Fight::new);
    let mut entities = HashMap::new();
    let mut hp_max = HashMap::new();
    let mut spawn_one = |commands: &mut Commands,
                         loader: &mut Loader,
                         look: Option<&ddlook::Look>,
                         pos: Vec3|
     -> Result<Entity, String> {
        let at = (pos - origin) * UNIT;
        match look {
            Some(l) => crate::character::spawn_look(commands, loader, meshes, materials, images, bindposes, l, MOTIONS, 0, at, Some(0.0)),
            None => crate::character::spawn(commands, loader, meshes, materials, images, bindposes, BODY, MOTIONS, 0, at, Some(0.0)),
        }
    };
    let mut locals = vec![];
    for (id, look, pos, yaw) in &party.arisen {
        sim.add_player(*id, pos.to_array(), *yaw);
        let p = sim.player_mut(*id).unwrap();
        // as a dagger vocation: the second jump is available
        p.double_jump = true;
        // the start area was recorded with the relaxed run (docs/MOVEMENT.md)
        p.relaxed = true;
        if let Some(f) = fight.as_mut() {
            let kit = party.kits.get(id).copied().unwrap_or(Kit::Fighter);
            f.add_arisen_as(*id, kit);
            f.equip(&mut sim, *id);
            hp_max.insert(*id, f.kit(kit).hp_max);
        }
        entities.insert(*id, spawn_one(commands, loader, look.as_ref(), *pos)?);
        locals.push(*id);
    }
    if let (Some((look, pos, yaw)), Some(f)) = (&party.pawn, fight.as_mut()) {
        let seat = pawn_seat(0);
        // Balanced inclinations (no active one; the save's are not read).
        f.add_pawn(&mut sim, seat, 0, pos.to_array(), *yaw, [500.0; 9], 7);
        let p = sim.player_mut(seat).unwrap();
        p.relaxed = true;
        hp_max.insert(seat, f.kit(Kit::Fighter).hp_max);
        entities.insert(seat, spawn_one(commands, loader, look.as_ref(), *pos)?);
    }
    let mut goblins = vec![];
    if let Some(f) = fight.as_mut() {
        if let Some(&(_, _, pos, yaw)) = party.arisen.first() {
            for k in 0..party.goblins {
                // 6 m ahead of the Arisen (k more apart), facing it.
                let side = (k as f32 - (party.goblins as f32 - 1.0) * 0.5) * 250.0;
                let (s, c) = yaw.sin_cos();
                let x = pos.x + s * 600.0 + c * side;
                let z = pos.z + c * 600.0 - s * side;
                let y = ground_at(&terrain, x, z, pos.y);
                let gpos = Vec3::new(x, y, z);
                f.add_goblin(gpos.to_array(), yaw + std::f32::consts::PI);
                let e = crate::character::spawn(
                    commands,
                    loader,
                    meshes,
                    materials,
                    images,
                    bindposes,
                    GOBLIN_MODEL,
                    ddhost::ddenemyrt::goblin::BANKS[0].1,
                    0,
                    (gpos - origin) * UNIT,
                    Some(0.0),
                )?;
                let hp_max = f.goblins.last().map_or(1.0, |g| g.fighter.victim.hp());
                goblins.push(GoblinView { entity: e, shown: None, hp_max });
            }
            info!("goblins: {}", party.goblins);
        }
        f.volumes(&sim);
    }
    if fight.is_none() {
        warn!("play: no fight data (DDDA.exe next to nativePC?): no attacks, pawn or goblins");
    }
    Ok(PlayState {
        sim,
        terrain,
        origin,
        motions,
        entities,
        follow: 0,
        cam_yaw: party.arisen.first().map_or(0.0, |a| a.3),
        cam_pitch: -0.25,
        demo: false,
        shown: HashMap::new(),
        visible: HashMap::new(),
        fight,
        goblins,
        fight_data: data,
        acc: 0.0,
        locals,
        hp_max,
        demo_press: false,
        kills: 0,
        dealt: HashMap::new(),
    })
}

fn stick(keys: &ButtonInput<KeyCode>, up: KeyCode, down: KeyCode, left: KeyCode, right: KeyCode) -> [f32; 2] {
    let f = |k: KeyCode| if keys.pressed(k) { 1.0f32 } else { 0.0 };
    let v = [f(right) - f(left), f(up) - f(down)];
    let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if n > 1.0 {
        [v[0] / n, v[1] / n]
    } else {
        v
    }
}

/// Frame to show for a motion at simulation frame `f`: looping motions
/// repeat from their loop frame, others hold their last frame.
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

/// Interpolation frames when a player's shown motion changes to `motion`.
fn interpolation_frames(default: f32, motion: u16) -> f32 {
    // cPlCliffHang::vf05 (0x00ADCC50) requests 0 frames; every other action
    // ddsim models requests -1, i.e. the motion's default
    // (docs/ANIMATION.md)
    if motion == 0x2F {
        return 0.0;
    }
    default
}

/// Player two's controls (arrow keys, Right Ctrl / Shift, Enter, Numpad 1
/// and 2).
fn p2_input(keys: &ButtonInput<KeyCode>, cam_yaw: f32) -> Input {
    Input {
        stick: stick(keys, KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight),
        walk: keys.pressed(KeyCode::ControlRight),
        sprint: keys.pressed(KeyCode::ShiftRight),
        jump: keys.pressed(KeyCode::Enter) || keys.pressed(KeyCode::NumpadEnter),
        light: keys.pressed(KeyCode::Numpad1),
        heavy: keys.pressed(KeyCode::Numpad2),
        camera_yaw: cam_yaw,
    }
}

/// The demo's Arisen against the nearest live goblin: run at it, and press
/// light attack whenever it stands within 2.2 m (presses are edges, so
/// every other frame), as ddhost's goblin test does. `press` keeps the
/// last press.
pub fn demo_attack(sim: &Sim, fight: Option<&Fight>, seat: PlayerId, cam_yaw: f32, press: &mut bool) -> Input {
    let Some(p) = sim.player(seat) else { return Input::default() };
    let target = fight.and_then(|f| {
        f.goblins
            .iter()
            .filter(|g| !g.dead())
            .map(|g| (g.pos, (g.pos[0] - p.position[0]).hypot(g.pos[2] - p.position[2])))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    });
    let Some((g, d)) = target else {
        *press = false;
        return Input { camera_yaw: cam_yaw, ..Default::default() };
    };
    let heading = (g[0] - p.position[0]).atan2(g[2] - p.position[2]);
    let mut i = Input { camera_yaw: heading, ..Default::default() };
    if d > 220.0 {
        i.stick = [0.0, 1.0];
        *press = false;
    } else {
        let ready = matches!(p.state, ddsim::State::Wait | ddsim::State::Attack { .. });
        *press = ready && !*press;
        i.light = *press;
    }
    i
}

/// One frame of the local session at the fixed 30 Hz: the order of
/// `ddhost::Host::tick` (hits, players and pawns, attacks and shells,
/// enemies, push-out, volumes).
fn fixed_frame(st: &mut PlayState, local: &HashMap<PlayerId, Input>) {
    let step = 1.0;
    let mut inputs = HashMap::new();
    if let Some(f) = st.fight.as_mut() {
        f.hits(step);
        inputs = f.pawn_inputs(&st.sim, step);
    }
    for (&k, &v) in local {
        inputs.insert(k, v);
    }
    st.sim.step(1.0 / ddsim::MOTION_FPS, &inputs, &st.terrain);
    if let Some(f) = st.fight.as_mut() {
        f.players_moved(&mut st.sim, &st.terrain, step);
        f.enemies(&st.sim, &st.terrain, step);
        f.push_out(&mut st.sim);
        f.volumes(&st.sim);
        for e in &f.events {
            debug!("hit: {} -> {}: {:.1}", e.attacker, e.victim, e.total);
            let d = st.dealt.entry(e.attacker).or_default();
            d.0 += 1;
            d.1 += e.total;
        }
        let dead = f.goblins.iter().filter(|g| g.dead()).count();
        if dead > st.kills {
            let mut by: Vec<_> = st.dealt.iter().collect();
            by.sort_by_key(|x| *x.0);
            info!("play: goblin killed ({dead} of {}); hits so far by owner (count, damage): {by:?}", f.goblins.len());
            st.kills = dead;
        }
    }
}

/// One tick: input -> simulation -> character transforms and motions.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    net: Option<Res<crate::net::NetRes>>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
    mut hud: ResMut<Hud>,
) {
    let Some(mut st) = state else { return };
    // A network session (--host / --join) steps and shows the players itself.
    if net.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::F2) {
        for p in &mut st.sim.players {
            p.relaxed = !p.relaxed;
        }
        let relaxed = st.sim.players.first().is_some_and(|p| p.relaxed);
        info!("{} walk and run", if relaxed { "town (relaxed)" } else { "field" });
    }
    orbit(&mut st, &buttons, &mouse);
    let cam_yaw = st.cam_yaw;
    let mut local = HashMap::new();
    let locals = st.locals.clone();
    if let Some(&p1) = locals.first() {
        local.insert(p1, p1_input(&keys, &buttons, cam_yaw));
    }
    if let Some(&p2) = locals.get(1) {
        local.insert(p2, p2_input(&keys, cam_yaw));
    }
    let dt = time.delta_secs().min(0.25);
    st.acc += dt;
    let frame = 1.0 / ddsim::MOTION_FPS;
    let st = &mut *st;
    while st.acc >= frame {
        st.acc -= frame;
        if st.demo {
            let t = time.elapsed_secs();
            if !st.goblins.is_empty() {
                if let Some(&p1) = locals.first() {
                    let mut press = st.demo_press;
                    let i = demo_attack(&st.sim, st.fight.as_ref(), p1, cam_yaw, &mut press);
                    st.demo_press = press;
                    local.insert(p1, i);
                }
            } else {
                // P1 runs and jumps every 2 s; P2 sprints
                if let Some(&p1) = locals.first() {
                    local.insert(p1, Input { stick: [0.0, 1.0], jump: t % 2.0 < 0.1, camera_yaw: cam_yaw, ..Default::default() });
                }
                if let Some(&p2) = locals.get(1) {
                    local.insert(p2, Input { stick: [0.0, 1.0], sprint: true, camera_yaw: cam_yaw, ..Default::default() });
                }
            }
        }
        fixed_frame(st, &local);
    }
    let shown: Vec<Shown> = st.sim.players.iter().map(Shown::of).collect();
    show(st, &shown, &mut roots, &mut cams);
    let fight = st.fight.take();
    show_goblins(st, fight.as_ref(), &mut roots);
    hud.rows = hud_rows(st, &st.sim, fight.as_ref(), None);
    st.fight = fight;
}

/// Turns the follow camera while the right mouse button is held.
pub fn orbit(st: &mut PlayState, buttons: &ButtonInput<MouseButton>, mouse: &AccumulatedMouseMotion) {
    if buttons.pressed(MouseButton::Right) {
        st.cam_yaw -= mouse.delta.x * 0.004;
        st.cam_pitch = (st.cam_pitch - mouse.delta.y * 0.004).clamp(-1.2, 0.4);
    }
}

/// Player one's controls (W A S D, Ctrl, Shift, Space; light attack J or
/// the left mouse button, heavy attack K or the middle mouse button).
pub fn p1_input(keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>, cam_yaw: f32) -> Input {
    Input {
        stick: stick(keys, KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD),
        walk: keys.pressed(KeyCode::ControlLeft),
        sprint: keys.pressed(KeyCode::ShiftLeft),
        jump: keys.pressed(KeyCode::Space),
        light: keys.pressed(KeyCode::KeyJ) || buttons.pressed(MouseButton::Left),
        heavy: keys.pressed(KeyCode::KeyK) || buttons.pressed(MouseButton::Middle),
        camera_yaw: cam_yaw,
    }
}

/// What the view needs of one Arisen or pawn.
#[derive(Debug, Clone, Copy)]
pub struct Shown {
    pub id: PlayerId,
    pub position: [f32; 3],
    pub yaw: f32,
    pub state: ddsim::State,
    pub frame: f32,
    /// The weapon whose motions it plays (`None` when unknown: the state's
    /// own weapon for attacks, else the common banks).
    pub weapon: Weapon,
}

impl Shown {
    pub fn of(p: &ddsim::Player) -> Shown {
        Shown { id: p.id, position: p.position, yaw: p.yaw, state: p.state, frame: p.frame, weapon: p.weapon }
    }
}

/// The weapon whose motion an attack state plays.
fn state_weapon(s: ddsim::State, known: Weapon) -> Weapon {
    match s {
        ddsim::State::Shot { .. } => Weapon::Staff,
        ddsim::State::Attack { attack } => {
            let id = attack.motion();
            // sword rows 0x700-0x713, staff rows 0x723 on (ddsim::attack)
            if id >= 0x723 {
                Weapon::Staff
            } else if known == Weapon::None {
                Weapon::Sword
            } else {
                known
            }
        }
        _ => known,
    }
}

/// Places every character at its Arisen, plays its motion, hides the
/// characters without one, and puts the camera behind `st.follow`.
pub fn show(
    st: &mut PlayState,
    bodies: &[Shown],
    roots: &mut Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    cams: &mut Query<&mut Transform, With<FollowCam>>,
) {
    for (id, &e) in &st.entities {
        let present = bodies.iter().any(|b| b.id == *id);
        st.visible.insert(e, present);
    }
    for p in bodies {
        let Some(&e) = st.entities.get(&p.id) else {
            continue;
        };
        let Ok((mut tr, mut pb)) = roots.get_mut(e) else {
            continue;
        };
        tr.translation = (Vec3::from(p.position) - st.origin) * UNIT;
        tr.rotation = Quat::from_rotation_y(p.yaw);
        let id = p.state.motion_id();
        let weapon = state_weapon(p.state, p.weapon);
        let found = st.motions.get(weapon, id);
        if st.shown.get(&p.id) != Some(&id) {
            if let Some((m, default)) = found {
                // the first motion of a model does not blend (uModel::vf30)
                let frames = if st.shown.contains_key(&p.id) { interpolation_frames(default, id) } else { 0.0 };
                debug!("player {}: motion {:#x}, blend {frames} frames", p.id, id);
                pb.change_motion(m.clone(), frames);
            }
            st.shown.insert(p.id, id);
        }
        let shown = found.map_or(p.frame, |(m, _)| display_frame(m, p.frame));
        pb.hold_frame = Some(shown);
        // Weapons in hand while attacking (*inferred*: the original draws
        // them while uPlayer +0x273C has 0x200 and 0x400 / 0x800,
        // uPlayerBase::vf93 0x00B63F70; the writers of those bits are not
        // traced, so the attack states stand for them).
        pb.drawn = matches!(p.state, ddsim::State::Attack { .. } | ddsim::State::Shot { .. });
    }
    // Follow camera: behind the followed player at the orbit angles,
    // looking at the chest.
    if let (Some(p), Ok(mut cam)) = (bodies.iter().find(|b| b.id == st.follow), cams.single_mut()) {
        let target = (Vec3::from(p.position) - st.origin + Vec3::new(0.0, 150.0, 0.0)) * UNIT;
        let dist = 4.5;
        let dir = Vec3::new(
            st.cam_yaw.sin() * st.cam_pitch.cos(),
            -st.cam_pitch.sin(),
            st.cam_yaw.cos() * st.cam_pitch.cos(),
        );
        cam.translation = target - dir * dist;
        cam.look_at(target, Vec3::Y);
    }
}

/// Places the goblins and plays their motions (`Goblin::motion` / `frame`,
/// banks `ddenemyrt::goblin::BANKS`).
pub fn show_goblins(
    st: &mut PlayState,
    fight: Option<&Fight>,
    roots: &mut Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
) {
    let (Some(f), Some(d)) = (fight, st.fight_data.as_ref()) else { return };
    for (g, v) in f.goblins.iter().zip(st.goblins.iter_mut()) {
        let Ok((mut tr, mut pb)) = roots.get_mut(v.entity) else { continue };
        tr.translation = (Vec3::from(g.pos) - st.origin) * UNIT;
        tr.rotation = Quat::from_rotation_y(g.yaw);
        let m = d
            .goblin
            .body
            .banks
            .get(usize::from(g.motion.bank))
            .and_then(Option::as_ref)
            .and_then(|b| b.list.motions.get(usize::from(g.motion.slot)))
            .and_then(Option::as_ref);
        if v.shown != Some(g.motion) {
            if let Some(m) = m {
                // *viewer choice*: the goblin's blend frames are not read
                pb.change_motion(m.clone(), if v.shown.is_some() { 6.0 } else { 0.0 });
            }
            v.shown = Some(g.motion);
        }
        pb.hold_frame = Some(m.map_or(g.frame, |m| display_frame(m, g.frame)));
    }
}

/// The HUD's rows: (label, value, max, colour).
#[derive(Resource, Default)]
pub struct Hud {
    pub rows: Vec<(String, f32, f32, Color)>,
}

/// HUD rows for the followed Arisen, its pawn and the nearest goblin.
/// `hp` overrides the fight's HP (a network client has none).
pub fn hud_rows(
    st: &PlayState,
    sim: &Sim,
    fight: Option<&Fight>,
    hp: Option<&HashMap<PlayerId, f32>>,
) -> Vec<(String, f32, f32, Color)> {
    let mut rows = vec![];
    let red = Color::srgb(0.75, 0.15, 0.12);
    let green = Color::srgb(0.2, 0.65, 0.25);
    let hp_of = |seat: PlayerId| -> Option<f32> {
        if let Some(h) = hp {
            return h.get(&seat).copied();
        }
        fight?.arisen.get(&seat).map(|a| a.fighter.victim.hp())
    };
    let me = st.follow;
    if let (Some(h), Some(&max)) = (hp_of(me), st.hp_max.get(&me)) {
        rows.push(("Arisen HP".to_string(), h, max, red));
    }
    if let Some(p) = sim.player(me) {
        rows.push(("Arisen stamina".to_string(), p.stamina, p.stamina_max, green));
    }
    let pawn = pawn_seat(me);
    if let (Some(h), Some(&max)) = (hp_of(pawn), st.hp_max.get(&pawn)) {
        rows.push(("Pawn HP".to_string(), h, max, red));
    }
    if let (Some(f), Some(p)) = (fight, sim.player(me)) {
        let near = f
            .goblins
            .iter()
            .zip(&st.goblins)
            .filter(|(g, _)| !g.dead())
            .min_by(|a, b| {
                let d = |g: &ddhost::ddenemyrt::goblin::Goblin| (g.pos[0] - p.position[0]).hypot(g.pos[2] - p.position[2]);
                d(a.0).total_cmp(&d(b.0))
            });
        if let Some((g, v)) = near {
            rows.push(("Goblin HP".to_string(), g.fighter.victim.hp(), v.hp_max, Color::srgb(0.6, 0.45, 0.1)));
        } else if !f.goblins.is_empty() {
            rows.push(("Goblins: all down".to_string(), 0.0, 1.0, Color::srgb(0.6, 0.45, 0.1)));
        }
    }
    rows
}

/// A HUD bar: fill and text by row.
#[derive(Component)]
pub struct HudFill(usize);
#[derive(Component)]
pub struct HudText(usize);
#[derive(Component)]
pub struct HudBack(usize);

/// The HUD: a viewer overlay, not the game's GUI (`mtgui` formats are not
/// drawn yet). Labelled so on screen.
pub fn spawn_hud(commands: &mut Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(16.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Text::new("viewer HUD (not the game's GUI)"),
                TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ));
            for i in 0..4 {
                c.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })
                    .with_children(|r| {
                        r.spawn((
                            Node { width: Val::Px(200.0), height: Val::Px(12.0), ..default() },
                            BackgroundColor(Color::NONE),
                            HudBack(i),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Node { width: Val::Percent(0.0), height: Val::Percent(100.0), ..default() },
                                BackgroundColor(Color::NONE),
                                HudFill(i),
                            ));
                        });
                        r.spawn((Text::new(""), TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() }, TextColor(Color::WHITE), HudText(i)));
                    });
            }
        });
}

/// Writes the HUD rows into the bars.
pub fn update_hud(
    hud: Res<Hud>,
    mut fills: Query<(&HudFill, &mut Node, &mut BackgroundColor)>,
    mut texts: Query<(&HudText, &mut Text)>,
    mut backs: Query<(&HudBack, &mut BackgroundColor), Without<HudFill>>,
) {
    for (b, mut bg) in &mut backs {
        let on = hud.rows.get(b.0).is_some_and(|r| r.2 > 1.0);
        *bg = BackgroundColor(if on { Color::srgba(0.0, 0.0, 0.0, 0.55) } else { Color::NONE });
    }
    for (f, mut node, mut bg) in &mut fills {
        match hud.rows.get(f.0) {
            Some((_, v, max, c)) => {
                node.width = Val::Percent((v / max.max(1e-3) * 100.0).clamp(0.0, 100.0));
                *bg = BackgroundColor(*c);
            }
            None => {
                node.width = Val::Percent(0.0);
                *bg = BackgroundColor(Color::NONE);
            }
        }
    }
    for (t, mut text) in &mut texts {
        **text = match hud.rows.get(t.0) {
            Some((label, v, max, _)) if *max > 1.0 => format!("{label} {v:.0} / {max:.0}"),
            Some((label, ..)) => label.clone(),
            None => String::new(),
        };
    }
}

/// The save's spot: stage, the Arisen's and the main pawn's positions and
/// angles (stage frame, origin at cell 50m50n, docs/WORLD.md), clock and
/// weather, from slot `mPlayerDataManual`.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedSpot {
    pub stage: u32,
    /// `mPlPos`, `mPlAng`.
    pub pos: Vec3,
    pub yaw: f32,
    /// `mCmcPos[0]`, `mCmcAng[0]`.
    pub pawn: Option<(Vec3, f32)>,
    /// `mGameHour`, `mGameMinute`, `mGameSecond` as ms into the day.
    pub clock_ms: u32,
    /// `mWeatherType`.
    pub weather: u32,
}

/// Reads [`SavedSpot`] from a save's XML (`ddsave::SaveFile::xml`).
pub fn saved_spot(xml: &str) -> Option<SavedSpot> {
    let start = xml.find("<class name=\"mPlayerDataManual\"")?;
    let end = xml[start + 10..].find("<class name=\"mPlayerData").map_or(xml.len(), |i| start + 10 + i);
    let s = &xml[start..end];
    let attr = |tag: &str, a: &str| -> Option<f32> {
        let i = s.find(tag)?;
        let rest = &s[i..];
        let line = &rest[..rest.find('>')?];
        let k = line.find(&format!("{a}=\""))? + a.len() + 2;
        line[k..].split('"').next()?.parse().ok()
    };
    let value = |name: &str| attr(&format!("name=\"{name}\""), "value");
    let v3 = |tag: &str| Some(Vec3::new(attr(tag, "x")?, attr(tag, "y")?, attr(tag, "z")?));
    let first_in = |array: &str, tag: &str| -> Option<String> {
        let i = s.find(&format!("name=\"{array}\""))?;
        let rest = &s[i..];
        let j = rest.find(&format!("<{tag} "))?;
        Some(rest[j..j + rest[j..].find('>')?].to_string())
    };
    let pawn = first_in("mCmcPos", "vector3").and_then(|l| {
        let f = |a: &str| -> Option<f32> {
            let k = l.find(&format!("{a}=\""))? + a.len() + 2;
            l[k..].split('"').next()?.parse().ok()
        };
        let pos = Vec3::new(f("x")?, f("y")?, f("z")?);
        let ang = first_in("mCmcAng", "f32").and_then(|a| {
            let k = a.find("value=\"")? + 7;
            a[k..].split('"').next()?.parse().ok()
        });
        Some((pos, ang.unwrap_or(0.0)))
    });
    let h = value("mGameHour")? as u32;
    let m = value("mGameMinute").unwrap_or(0.0) as u32;
    let sec = value("mGameSecond").unwrap_or(0.0) as u32;
    Some(SavedSpot {
        stage: value("mStageNo")? as u32,
        pos: v3("name=\"mPlPos\"")?,
        yaw: value("mPlAng").unwrap_or(0.0),
        pawn,
        clock_ms: (h * 3600 + m * 60 + sec) * 1000,
        weather: value("mWeatherType").unwrap_or(0.0) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_spot_reads_the_manual_slot() {
        let xml = r#"<class name="mPlayerDataManual" type="sSave::playerData">
<u32 name="mGameHour" value="12"/>
<u32 name="mGameMinute" value="35"/>
<u32 name="mGameSecond" value="43"/>
<u32 name="mWeatherType" value="1"/>
<s32 name="mStageNo" value="100"/>
<vector3 name="mPlPos" x="-5643.5" y="40161.0" z="83345.25"/>
<array name="mCmcPos" type="vector3" count="3">
<vector3 x="-5270.5" y="40240.5" z="83359.0"/>
<vector3 x="1.0" y="2.0" z="3.0"/>
</array>
<f32 name="mPlAng" value="-2.0"/>
<array name="mCmcAng" type="f32" count="3">
<f32 value="-2.5"/>
</array>
</class>
<class name="mPlayerDataBase" type="sSave::playerData">
<u32 name="mGameHour" value="6"/>
</class>"#;
        let s = saved_spot(xml).unwrap();
        assert_eq!(s.stage, 100);
        assert_eq!(s.pos, Vec3::new(-5643.5, 40161.0, 83345.25));
        assert_eq!(s.yaw, -2.0);
        assert_eq!(s.pawn, Some((Vec3::new(-5270.5, 40240.5, 83359.0), -2.5)));
        assert_eq!(s.clock_ms, (12 * 3600 + 35 * 60 + 43) * 1000);
        assert_eq!(s.weather, 1);
    }
}
