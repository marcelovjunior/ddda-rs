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
//! | equipped skill slots | 1-6 | Numpad 4-9 |
//! | quick-use saved restorative | U | Numpad 0 |
//! | bow toggle / hold | configured STG_TOGGLE_BOW_MODE / STG_SUB_WEP | Numpad - / Numpad / |
//!
//! The grabbed mouse orbits P1; Esc opens a viewer pause menu. F4 switches every
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

#[path = "goblin_view.rs"]
pub(crate) mod goblin_view;

/// Bank 0 motion list of the male Arisen (docs/ANIMATION.md).
pub const MOTIONS: &str = ddhost::install::MOTIONS;
pub const BODY: &str = r"model\pl\m\m_base\m000\m000";
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
    skills: crate::skills::Motions,
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
        // 0x0163DF8C): sword m0001, dagger m0005, staff m0007 (as
        // ddenemyrt::arisen)
        for (w, n) in [(Weapon::Sword, "0001"), (Weapon::Dagger, "0005"), (Weapon::Staff, "0007")] {
            let mut m = HashMap::new();
            for (bank, kind) in [(6u8, "co"), (7u8, "at")] {
                m.insert(bank, view_bank(vfs, &format!(r"motion\pl\m\m00\m{n}_{kind}\m{n}_{kind}"))?);
            }
            v.weapon.insert(w, m);
        }
        // Bow lower0 / upper1 both resolve bank9, B16FE0.
        v.weapon.insert(Weapon::Bow, [(9, view_bank(vfs, r"motion\pl\m\m00\m0006_at\m0006_at")?)].into());
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
    match cat {
        Some(5) => Kit::Mage,
        Some(4) => Kit::Strider,
        _ => Kit::Fighter,
    }
}

/// A goblin's view.
pub struct GoblinView {
    pub entity: Entity,
    shown: Option<MotionId>,
    pub hp_max: f32,
}

impl GoblinView {
    pub fn new(entity: Entity, hp_max: f32) -> Self {
        GoblinView { entity, shown: None, hp_max }
    }
}

#[derive(Resource)]
pub struct PlayState {
    pub sim: Sim,
    pub terrain: ddworld::Terrain,
    pub door_collision: dddoor::collision::Set,
    /// Stage-frame position (cm) of the viewer's origin.
    pub origin: Vec3,
    pub motions: ViewMotions,
    pub entities: HashMap<PlayerId, Entity>,
    pub follow: PlayerId,
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    pub camera_params: mtcamera::params::Normal,
    pub bow_view: crate::bow_view::Aiming,
    /// Scripted input for unattended tests: without goblins P1 runs and
    /// jumps and P2 sprints; with one, P1 closes in and attacks it.
    pub demo: bool,
    /// Motion each character's Playback currently shows.
    shown: HashMap<PlayerId, (Option<u32>, u16)>,
    /// Whether each character has an Arisen to show (network sessions
    /// spawn characters for every seat up front).
    pub visible: HashMap<Entity, bool>,
    /// The local fight (attacks, hits, pawn, goblins); `None` without the
    /// exe's data.
    pub fight: Option<Fight>,
    pawn_orders: ddhost::pawn_orders::Orders,
    /// Goblin views, in the fight's goblin order.
    pub goblins: Vec<GoblinView>,
    /// The goblin data for the view (its motion banks), when loaded.
    pub fight_data: Option<std::sync::Arc<FightData>>,
    /// Seconds not yet stepped at the fixed frame.
    acc: f32,
    /// Players' and goblins' (position, yaw) before the last fixed frame,
    /// for drawing between frames.
    prev: Between,
    prev_goblins: Vec<([f32; 3], f32)>,
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

impl PlayState {
    /// Forgets the motion shown for `id`, so the next frame poses a freshly
    /// spawned body.
    pub fn forget_shown(&mut self, id: PlayerId) {
        self.shown.remove(&id);
    }
}

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
    let camera_bytes = loader.vfs.read("param\\cam_param", mtarc::type_hash("rCameraParamList"))
        .map_err(|e| e.to_string())?;
    let camera_params = mtcamera::params::Parameters::parse(&camera_bytes)
        .map_err(|e| e.to_string())?.normal.first().copied().flatten()
        .ok_or("default normal camera parameter slot is missing")?;
    let mut sim = ddhost::install::sim(loader.vfs)?;
    let mut motions = ViewMotions::load(loader.vfs)?;
    if let Some(data) = data.as_deref() {
        motions.skills = crate::skills::Motions::load(loader.vfs, data);
    }
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
        // S27 owns a separate animated shortbow, so remove only the old
        // rigid secondary bow part to avoid drawing it twice.
        let mut look = look.clone();
        if party.kits.get(id) == Some(&Kit::Strider) {
            if let Some(look) = &mut look { look.weapons.retain(|w| (w.id as u32 >> 24) != 10); }
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
                let appearance = f.goblins.last().and_then(|g| g.appearance.as_ref())
                    .ok_or("goblin appearance was not resolved by the host")?;
                let e = goblin_view::spawn(
                    commands,
                    loader,
                    meshes,
                    materials,
                    images,
                    bindposes,
                    &f.data.goblin,
                    appearance,
                    (gpos - origin) * UNIT,
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
    let mut bow_view = crate::bow_view::Aiming::load(loader.vfs);
    if let Some(fight) = &fight { bow_view.spawn_models(commands, loader, meshes, materials, images, bindposes, fight)?; }
    Ok(PlayState {
        door_collision: Default::default(),
        sim,
        terrain,
        origin,
        motions,
        entities,
        follow: 0,
        cam_yaw: party.arisen.first().map_or(0.0, |a| a.3),
        // the recordings look down 12-30 degrees
        cam_pitch: 0.35,
        camera_params,
        bow_view,
        demo: false,
        shown: HashMap::new(),
        visible: HashMap::new(),
        fight,
        pawn_orders: Default::default(),
        goblins,
        fight_data: data,
        acc: 0.0,
        prev: Between::default(),
        prev_goblins: Vec::new(),
        locals,
        hp_max,
        demo_press: false,
        kills: 0,
        dealt: HashMap::new(),
    })
}

fn stick(keys: &ButtonInput<KeyCode>, up: KeyCode, down: KeyCode, left: KeyCode, right: KeyCode) -> [f32; 2] {
    // A press and release can both arrive before a render frame (e.g. a
    // slow loading frame). Keep that edge until the next simulation tick.
    let f = |k: KeyCode| if keys.pressed(k) || keys.just_pressed(k) { 1.0f32 } else { 0.0 };
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
pub(crate) fn p2_input(keys: &ButtonInput<KeyCode>, cam_yaw: f32) -> Input {
    Input {
        stick: stick(keys, KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight),
        walk: keys.pressed(KeyCode::ControlRight),
        sprint: keys.pressed(KeyCode::ShiftRight),
        jump: keys.pressed(KeyCode::Enter) || keys.pressed(KeyCode::NumpadEnter),
        light: keys.pressed(KeyCode::Numpad1),
        heavy: keys.pressed(KeyCode::Numpad2),
        // Local second-Arisen bindings are a viewer choice; the original
        // has one keyboard Arisen (F1/F2/F3).
        pawn_orders: ddsim::PawnOrderButtons { up: keys.pressed(KeyCode::Numpad7) || keys.just_pressed(KeyCode::Numpad7),
            down: keys.pressed(KeyCode::Numpad8) || keys.just_pressed(KeyCode::Numpad8), left: false,
            right: keys.pressed(KeyCode::Numpad9) || keys.just_pressed(KeyCode::Numpad9) },
        camera_yaw: cam_yaw,
        ..Default::default()
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
    // A careful player doesn't swing through a bystander: with a live NPC
    // body (e.g. Reynard) within 3 m and inside 60 degrees of the swing,
    // step sideways instead (friendly fire is real outside Japan, S34).
    let bystander = fight.is_some_and(|f| {
        f.npcs.values().any(|n| {
            let (dx, dz) = (n.position[0] - p.position[0], n.position[2] - p.position[2]);
            let mut a = dx.atan2(dz) - heading;
            a = (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            !n.fighter.victim.dead() && dx.hypot(dz) < 300.0 && a.abs() < 60f32.to_radians()
        })
    });
    if bystander && d <= 400.0 {
        i.stick = [1.0, 0.0];
        *press = false;
    } else if d > 220.0 {
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
    // Camera targets are evaluated per owned seat; P2 currently shares the
    // local viewport's orbit, but never another seat's muzzle or RNG stream.
    let views: Vec<_> = st.sim.players.iter().filter(|p| p.bow.is_some()).map(|p| {
        let (eye, target) = bow_camera_pose(st, Vec3::from(p.position), true);
        (p.id, eye, target)
    }).collect();
    if let Some(f) = st.fight.as_mut() {
        st.bow_view.prepare(&st.sim, f, &st.door_collision.over(&st.terrain), &views);
        // hard landings' damage (ddsim queues it, the fight owns HP)
        for p in &mut st.sim.players {
            if p.fall_damage > 0.0 {
                let taken = f.fall_damage(p.id, p.fall_damage);
                info!("play: seat {} fell: {taken:.0} HP", p.id);
                p.fall_damage = 0.0;
            }
        }
        f.players_moved(&mut st.sim, &st.terrain, step);
        f.enemies(&st.sim, &st.terrain, step);
        f.push_out(&mut st.sim);
        f.volumes(&st.sim);
        let owner_inputs = local.iter().map(|(&seat, &sim)|
            (seat, ddhost::PlayerInput { sim, interact: false })).collect();
        let ownership = f.pawns.iter().map(|(&seat, pawn)| (seat, pawn.owner)).collect();
        let cooling = st.pawn_orders.advance(step, |seat|
            !f.arisen.get(&seat).is_some_and(|body| body.fighter.victim.dead()));
        st.pawn_orders.dispatch(&owner_inputs, &cooling, f, Some(pawn_seat(0)), &ownership);
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
    pause: Res<crate::pause::PauseState>,
    time: Res<Time>,
    order_devices: PawnOrderDevices,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    net: Option<Res<crate::net::NetRes>>,
    items: Option<ResMut<crate::items::QuickItems>>,
    opening: Option<Res<crate::opening::OpeningRes>>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
    mut hud: ResMut<Hud>,
    mut projs: Query<&mut Projection, With<FollowCam>>,
    mut projected: Local<bool>,
    mut pending: Local<HashMap<PlayerId, Input>>,
    mut sounds: Option<ResMut<crate::audio::PlayerMotionSounds>>,
) {
    let keys = &order_devices.keys;
    let gamepads = &order_devices.gamepads;
    let Some(mut st) = state else { return };
    if pause.paused { pending.clear(); return; }
    if !*projected {
        camera_projection(&mut projs);
        *projected = true;
    }
    // A network session (--host / --join) and the opening (opening.rs)
    // step and show the players themselves.
    if net.is_some() || opening.is_some() {
        return;
    }
    if let Some(mut items) = items {
        let seats = st.locals.clone();
        for (index, key) in [KeyCode::KeyU, KeyCode::Numpad0].into_iter().enumerate() {
            if keys.just_pressed(key) {
                if let Some(&seat) = seats.get(index) {
                    let st = &mut *st;
                    if let Some(fight) = st.fight.as_mut() {
                        match items.use_next(seat, &mut st.sim, fight) {
                            Ok(item) => info!("quick-use: seat {seat} used item {item}"),
                            Err(e) => info!("quick-use: seat {seat}: {e}"),
                        }
                    }
                }
            }
        }
    }
    if keys.just_pressed(KeyCode::F4) {
        for p in &mut st.sim.players {
            p.relaxed = !p.relaxed;
        }
        let relaxed = st.sim.players.first().is_some_and(|p| p.relaxed);
        info!("{} walk and run", if relaxed { "town (relaxed)" } else { "field" });
    }
    if pause.mouse_look(buttons.pressed(MouseButton::Right)) { orbit(&mut st, &buttons, &mouse); }
    let cam_yaw = st.cam_yaw;
    let mut local = HashMap::new();
    let locals = st.locals.clone();
    if crate::skills::keys(&mut st.sim, &locals, &keys) { st.demo = false; }
    if let Some(&p1) = locals.first() {
        local.insert(p1, gamepad_order_input(p1_input(&keys, &buttons, cam_yaw), &gamepads, 0));
    }
    if let Some(&p2) = locals.get(1) {
        local.insert(p2, gamepad_order_input(p2_input(&keys, cam_yaw), &gamepads, 1));
    }
    for (index, seat) in locals.iter().enumerate() {
        if let Some(input) = local.get_mut(seat) {
            let active = st.sim.player(*seat).is_some_and(|p| p.bow.is_some());
            st.bow_view.controls.apply(index, input, &keys, &buttons, active);
        }
    }
    // Explicit local input takes over an unattended --demo session.
    if local.values().any(active_input) { st.demo = false; }
    for (&seat, &input) in &local {
        let queued = pending.entry(seat).or_default();
        *queued = latch_input(*queued, input);
    }
    local.clone_from(&pending);
    let dt = time.delta_secs().min(0.25);
    st.acc += dt;
    let frame = 1.0 / ddsim::MOTION_FPS;
    let st = &mut *st;
    while st.acc >= frame && !crate::bow_view::capture_frozen(st) {
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
        crate::bow_view::capture_input(st, &mut local);
        st.prev.snapshot(&st.sim);
        st.prev_goblins = st.fight.as_ref().map_or_else(Vec::new, |f| f.goblins.iter().map(|g| (g.pos, g.yaw)).collect());
        fixed_frame(st, &local);
        if crate::bow_view::freeze_capture(st) {
            st.prev.snapshot(&st.sim);
            st.prev_goblins = st.fight.as_ref().map_or_else(Vec::new, |f| f.goblins.iter().map(|g| (g.pos,g.yaw)).collect());
            st.acc = 0.;
        }
        if let (Some(sounds), Some(data)) = (sounds.as_deref_mut(), st.fight_data.as_ref()) {
            sounds.sample_sim(&st.sim, data, &st.terrain);
        }
        pending.clear();
    }
    // The simulation runs in fixed 1/30-s frames; the screen is drawn between
    // the last two of them, so movement and the follow camera are smooth at
    // any frame rate (*viewer choice*: the original steps once per drawn
    // frame with the frame's delta).
    let alpha = (st.acc / frame).clamp(0.0, 1.0);
    let shown = st.prev.shown(&st.sim, alpha);
    show(st, &shown, &mut roots, &mut cams);
    crate::bow_view::show_models(st, &mut roots);
    let aiming = st.sim.player(st.follow).is_some_and(|p| p.bow.is_some_and(|b| b.phase != ddsim::BowPhase::Released));
    for mut projection in &mut projs {
        if let Projection::Perspective(camera) = &mut *projection {
            camera.fov = (if aiming { st.bow_view.camera.as_ref().map_or(CAMERA_FOV, |c| c.normal.fov_degrees) }
                else { CAMERA_FOV } + config_camera_fov()).to_radians();
        }
    }
    let fight = st.fight.take();
    show_goblins(st, fight.as_ref(), &mut roots, alpha);
    hud.rows = hud_rows(st, &st.sim, fight.as_ref(), None);
    hud.enemy = enemy_gauge(st, fight.as_ref());
    st.fight = fight;
}

/// The players as they were before the last fixed frame, so the screen can
/// be drawn between it and the current one. Every fixed-step loop (play,
/// the opening, a network session) snapshots before stepping and draws
/// `shown(sim, acc / frame)`.
#[derive(Default)]
pub struct Between(HashMap<PlayerId, ([f32; 3], f32, ddsim::State, f32)>);

impl Between {
    pub fn snapshot(&mut self, sim: &Sim) {
        self.0 = sim.players.iter().map(|p| (p.id, (p.position, p.yaw, p.state, p.frame))).collect();
    }

    pub fn shown(&self, sim: &Sim, alpha: f32) -> Vec<Shown> {
        sim.players
            .iter()
            .map(|p| {
                let mut s = Shown::of(p);
                if let Some(&(pos, yaw, state, frame)) = self.0.get(&p.id) {
                    (s.position, s.yaw) = between(pos, yaw, p.position, p.yaw, alpha);
                    // the pose too, within one motion (not across a loop's wrap)
                    if state == p.state && p.frame >= frame {
                        s.frame = frame + (p.frame - frame) * alpha;
                    }
                }
                s
            })
            .collect()
    }
}

/// A body drawn `alpha` of the way from its previous fixed frame to the
/// current one: position linear, yaw the short way round. A jump of more
/// than `SNAP` (a warp or respawn) is drawn at once.
pub fn between(p0: [f32; 3], y0: f32, p1: [f32; 3], y1: f32, alpha: f32) -> ([f32; 3], f32) {
    const SNAP: f32 = 200.0;
    let d = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > SNAP * SNAP {
        return (p1, y1);
    }
    let pos = [p0[0] + d[0] * alpha, p0[1] + d[1] * alpha, p0[2] + d[2] * alpha];
    let dy = (y1 - y0 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    (pos, y0 + dy * alpha)
}

pub fn active_input(input: &Input) -> bool {
    input.stick != [0.0; 2] || input.jump || input.light || input.heavy || input.bow || input.bow_toggle
        || input.pawn_orders.native_bits() != 0
}

/// Retain a short input until a fixed tick consumes it. Held input still
/// refreshes direction and camera yaw every render frame.
pub(crate) fn latch_input(previous: Input, mut latest: Input) -> Input {
    if latest.stick == [0.0; 2] && previous.stick != [0.0; 2] {
        latest.stick = previous.stick;
        latest.walk = previous.walk;
        latest.sprint = previous.sprint;
    }
    latest.jump |= previous.jump;
    latest.light |= previous.light;
    latest.heavy |= previous.heavy;
    latest.bow |= previous.bow;
    latest.bow_toggle |= previous.bow_toggle;
    latest.pawn_orders.up |= previous.pawn_orders.up;
    latest.pawn_orders.down |= previous.pawn_orders.down;
    latest.pawn_orders.left |= previous.pawn_orders.left;
    latest.pawn_orders.right |= previous.pawn_orders.right;
    latest
}

/// Turns the follow camera directly with the grabbed mouse.
pub fn orbit(st: &mut PlayState, _buttons: &ButtonInput<MouseButton>, mouse: &AccumulatedMouseMotion) {
    (st.cam_yaw, st.cam_pitch) = orbit_angles(st.cam_yaw, st.cam_pitch, mouse.delta);
}

fn orbit_angles(yaw: f32, pitch: f32, delta: Vec2) -> (f32, f32) {
    // Viewer mouse sensitivity (radians/pixel), not an original-game constant.
    // Mouse right turns the view right. Yaw 0 looks along +Z and screen-right
    // is -X there (ddsim), so a larger yaw turns the view left: subtract.
    let yaw = yaw - delta.x * 0.004;
    // mAngLimitXMin / Max -60 / 30 degrees in the player camera lists
    // (sign *inferred*: up to 60 degrees above, 30 below)
    let pitch = (pitch + delta.y * 0.004).clamp(-60f32.to_radians(), 30f32.to_radians());
    (yaw, pitch)
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
        // 0x0040AB20: original UP/DOWN/RIGHT defaults are scan codes
        // 0x3B/0x3C/0x3D, mapped by 0x0040B460 to F1/F2/F3; LEFT unbound.
        pawn_orders: ddsim::PawnOrderButtons { up: keys.pressed(KeyCode::F1) || keys.just_pressed(KeyCode::F1),
            down: keys.pressed(KeyCode::F2) || keys.just_pressed(KeyCode::F2), left: false,
            right: keys.pressed(KeyCode::F3) || keys.just_pressed(KeyCode::F3) },
        camera_yaw: cam_yaw,
        ..Default::default()
    }
}

/// Connected controller order maps to local Arisen order. LAN guests use
/// their first local controller regardless of the host-assigned seat.
/// That assignment is a viewer choice; the four directions are native.
pub(crate) fn gamepad_order_input(mut input: Input, gamepads: &Query<(Entity, &Gamepad)>, local_index: usize) -> Input {
    let mut pads: Vec<_> = gamepads.iter().collect();
    pads.sort_by_key(|(entity, _)| entity.to_bits());
    if let Some((_, pad)) = pads.get(local_index) {
        input.pawn_orders.up |= pad.pressed(GamepadButton::DPadUp) || pad.just_pressed(GamepadButton::DPadUp);
        input.pawn_orders.down |= pad.pressed(GamepadButton::DPadDown) || pad.just_pressed(GamepadButton::DPadDown);
        input.pawn_orders.left |= pad.pressed(GamepadButton::DPadLeft) || pad.just_pressed(GamepadButton::DPadLeft);
        input.pawn_orders.right |= pad.pressed(GamepadButton::DPadRight) || pad.just_pressed(GamepadButton::DPadRight);
    }
    input
}

/// Keep keyboard and controllers in one system parameter: the play step
/// already uses Bevy's maximum number of function parameters.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct PawnOrderDevices<'w, 's> {
    pub keys: Res<'w, ButtonInput<KeyCode>>,
    pub gamepads: Query<'w, 's, (Entity, &'static Gamepad)>,
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
            use ddsim::Attack::*;
            match attack {
                StaffStrike1 | StaffStrike2 | StaffAirStrike => Weapon::Staff,
                DaggerLight1 | DaggerLight2 | DaggerLight3 | DaggerHeavy | DaggerAirLight1 | DaggerAirLight2
                | DaggerAirHeavy | DaggerAirHeavyLand => Weapon::Dagger,
                _ => Weapon::Sword,
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
        let active = st.sim.player(p.id).and_then(|p| p.skills.active);
        let id = active.map_or_else(|| p.state.motion_id(), |a| a.motion);
        let key = (active.map(|a| a.id), id);
        let weapon = if matches!(p.state, ddsim::State::Bow { .. }) { Weapon::Bow } else { state_weapon(p.state, p.weapon) };
        let found = if let Some(active) = active {
            st.motions.skills.get(active.id, id)
        } else {
            st.motions.get(weapon, id)
        };
        if st.shown.get(&p.id) != Some(&key) {
            if let Some((m, default)) = found {
                // the first motion of a model does not blend (uModel::vf30)
                let frames = if st.shown.contains_key(&p.id) { interpolation_frames(default, id) } else { 0.0 };
                debug!("player {}: motion {:#x}, blend {frames} frames", p.id, id);
                pb.change_motion(m.clone(), frames);
            }
            st.shown.insert(p.id, key);
        }
        let shown = found.map_or(p.frame, |(m, _)| display_frame(m, p.frame));
        pb.hold_frame = Some(shown);
        if let Some(bow) = st.sim.player(p.id).and_then(|p| p.bow) {
            if let Some((motion, _)) = st.motions.get(Weapon::Bow, bow.upper.motion) {
                // B16FE0 draw3; B1C7D0 wait10; B1CAC0 shot3;
                // B17940 release10, all embedded request lengths.
                let blend = match bow.phase { ddsim::BowPhase::Draw | ddsim::BowPhase::Shot => 3., _ => 10. };
                pb.upper_motion(bow.upper.motion, motion, display_frame(motion, bow.upper.frame), blend);
            }
        } else { pb.upper = None; }
        // Weapons in hand while attacking (*inferred*: the original draws
        // them while uPlayer +0x273C has 0x200 and 0x400 / 0x800,
        // uPlayerBase::vf93 0x00B63F70; the writers of those bits are not
        // traced, so the attack states stand for them).
        pb.drawn = active.is_some() || matches!(p.state, ddsim::State::Attack { .. } | ddsim::State::Shot { .. });
    }
    // Follow camera: behind the followed player at the orbit angles.
    if let (Some(p), Ok(mut cam)) = (bodies.iter().find(|b| b.id == st.follow), cams.single_mut()) {
        let aiming = st.sim.player(st.follow).is_some_and(|p| p.bow.is_some_and(|b| b.phase != ddsim::BowPhase::Released));
        let (eye, target) = bow_camera_pose(st, Vec3::from(p.position), aiming);
        cam.translation = (eye - st.origin) * UNIT;
        cam.look_at((target - st.origin) * UNIT, Vec3::Y);
    }
}

fn bow_camera_pose(st: &PlayState, feet: Vec3, aiming: bool) -> (Vec3, Vec3) {
    if let Some(camera) = st.bow_view.camera.as_ref().filter(|_| aiming) {
        let (eye, target) = camera.pose(feet, st.cam_yaw, st.cam_pitch);
        let anchor = camera.normal.collision_anchor(target.to_array(), Some((feet.to_array(), feet.to_array())),
            -st.cam_pitch, true, false).map(Vec3::from).unwrap_or(target);
        return collide_camera_query(&st.door_collision.over(&st.terrain), eye, target, anchor,
            (eye - target).normalize_or_zero(), CAMERA_NEAR);
    }
    follow_camera_with_params(&st.door_collision.over(&st.terrain), feet, st.cam_yaw, st.cam_pitch, Some(&st.camera_params))
}

/// The follow camera's target height above the feet and its distance (cm),
/// measured on the original: in the seven recorded reference poses
/// (`uCameraGame` eye and target with the player's position, 2026-10-08)
/// the eye is 230 cm from the target every time and the target 127-144 cm
/// above the feet (140 taken). The camera parameter that sets them was not
/// identified (*measured*, not traced).
pub const CAMERA_DISTANCE: f32 = 230.0;
pub const CAMERA_TARGET_HEIGHT: f32 = 140.0;
/// The active uCameraCtrl near plane (cm): uCamera ctor 0x01128740,
/// inherited through 0x007d3a80/0x007d6cd0. Collision 0x007de570 reads
/// sCamera+0xce0, not the child uCameraGame's separate 96 cm field.
/// Verified live in Cassardis: active controller 16, child game camera 96.
pub const CAMERA_NEAR: f32 = 16.0;
/// The field camera's `mFov` in those recordings; the render adds the
/// player's CameraFov setting (docs/SHADING.md).
pub const CAMERA_FOV: f32 = 55.0;

/// Eye and target (stage frame, cm) for the followed feet `feet` at the
/// orbit angles (`pitch` > 0: the eye above the target), kept out of
/// walls as the original's game camera does (`0x007de570`, called from
/// `0x007e1400`):
/// 1. A ray from the target to the eye; on a hit the eye moves to it.
/// 2. A sphere one near-plane distance in front of the eye, toward the
///    target, of radius `near x 1.3 + (1 - |cos|) x 10`. The cosine is
///    between the view direction and the eye-target direction, which are
///    the same here, so the radius is `1.3 x near`. If it overlaps
///    collision, it is swept from the target toward that centre (`0x010fe5e0`) and the eye
///    goes one near-plane distance behind the sweep's stop.
/// Static surfaces matching the exclusion mask 0x0081d01c are skipped
/// before querying (0x00422f40); the caller retains its final bit-0x10
/// check (0x007c6f90). Convex contact uses the same geometric replacement
/// as character collision, not the original GJK callback implementation.
#[cfg(test)]
fn follow_camera(ground: &dyn ddsim::Ground, feet: Vec3, yaw: f32, pitch: f32) -> (Vec3, Vec3) {
    follow_camera_with_params(ground, feet, yaw, pitch, None)
}

fn follow_camera_with_params(ground: &dyn ddsim::Ground, feet: Vec3, yaw: f32, pitch: f32,
    params: Option<&mtcamera::params::Normal>) -> (Vec3, Vec3) {
    let target = feet + Vec3::Y * CAMERA_TARGET_HEIGHT;
    let dir = Vec3::new(yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
    let desired_eye = target - dir * params.map_or(CAMERA_DISTANCE, |p| p.distance);
    let anchor = match params {
        // Viewer pitch has the opposite sign to uCameraGame's stored angle.
        // Actor overlap and model/world root offsets are not yet represented
        // by this adapter; ordinary followed-actor positions coincide here.
        Some(p) => p.collision_anchor(target.to_array(), Some((feet.to_array(), feet.to_array())),
            -pitch, true, false),
        None => Some(target.to_array()),
    };
    match anchor {
        Some(anchor) => collide_camera_query(ground, desired_eye, target, Vec3::from(anchor), -dir, CAMERA_NEAR),
        None => (desired_eye, target),
    }
}

/// Collision-only pose update, separated from the measured follow offset
/// so original candidate poses can be compared without assuming its lag,
/// authored target height or actor avoidance offset.
#[cfg(test)]
fn collide_camera(ground: &dyn ddsim::Ground, desired_eye: Vec3, target: Vec3, near: f32) -> (Vec3, Vec3) {
    let view_outward = (desired_eye - target).normalize_or_zero();
    collide_camera_query(ground, desired_eye, target, target, view_outward, near)
}

/// 0x007de570 takes the collision anchor (EDI) independently from the
/// outward view direction (argument 1). The outer caller can choose an
/// actor-relative anchor, so recomputing the view direction from it is wrong.
fn collide_camera_query(ground: &dyn ddsim::Ground, desired_eye: Vec3, target: Vec3,
    anchor: Vec3, view_outward: Vec3, near: f32) -> (Vec3, Vec3) {
    let mut eye = desired_eye;
    let ray_direction = (eye - anchor).normalize_or_zero();
    // Translated from 0x007de5dd..0x007de657 (DDDA.exe 2017-12-13 Steam).
    let radius = near * 1.3 + (1.0 - view_outward.dot(ray_direction).abs()) * 10.0;
    if let Some((hit, flags)) = camera_ray(ground, anchor, eye) {
        if flags & 0x10 == 0 { eye = Vec3::from(hit.point); }
    }
    let toward = -view_outward.normalize_or_zero();
    let centre = eye + toward * near;
    if sphere_hits(ground, centre, radius) {
        if let Some((stop, flags)) = camera_sphere_cast(ground, anchor, centre, radius) {
            if flags & 0x10 == 0 { eye = stop - toward * near; }
        }
    }
    // Translated from 0x007e1b11..0x007e1b68 (DDDA.exe 2017-12-13 Steam):
    // the caller of 0x007de570 translates the look target by the complete
    // eye correction, including off-axis penetration recovery. Keeping the
    // target fixed here would turn every sideways/ceiling contact into a
    // camera rotation when the presentation layer calls look_at.
    (eye, target + (eye - desired_eye))
}

/// Translated from camera Param setup 0x007e178d / 0x00de4710 and
/// sCollisionExt::vf10 0x00422f40 (DDDA.exe 2017-12-13 Steam).
/// Bit 0x40000000 selects exclusion; the remaining low 30 bits test the
/// triangle material before nearest-contact selection (0x01165660).
fn camera_surface(flags: u32) -> bool {
    flags & 0x0081_d01c == 0
}

fn camera_ray(ground: &dyn ddsim::Ground, from: Vec3, to: Vec3) -> Option<(ddsim::RayHit, u32)> {
    let nearest = ground.ray_with_flags(from.to_array(), to.to_array())?;
    if camera_surface(nearest.1) { return Some(nearest); }
    let mut faces = Vec::new();
    ground.triangles_in(from.min(to).to_array(), from.max(to).to_array(), &mut faces);
    let motion = to - from;
    let mut best = None;
    // Same two-sided Moller-Trumbore geometry policy as Terrain's ray.
    // The original per-triangle geometric callback remains a replacement;
    // the source-backed change here is filtering before nearest selection.
    for (triangle, flags) in faces {
        if !camera_surface(flags) { continue; }
        let a = Vec3::from(triangle[0]);
        let edge1 = Vec3::from(triangle[1]) - a;
        let edge2 = Vec3::from(triangle[2]) - a;
        let cross = motion.cross(edge2);
        let determinant = edge1.dot(cross);
        if determinant.abs() < 1e-9 { continue; }
        let inverse = determinant.recip();
        let offset = from - a;
        let u = offset.dot(cross) * inverse;
        if !(0.0..=1.0).contains(&u) { continue; }
        let q = offset.cross(edge1);
        let v = motion.dot(q) * inverse;
        if v < 0.0 || u + v > 1.0 { continue; }
        let fraction = edge2.dot(q) * inverse;
        if !(1e-4..=1.0).contains(&fraction) || best.is_some_and(|(previous, _, _)| previous <= fraction) { continue; }
        let normal = edge1.cross(edge2).normalize_or_zero();
        let normal = if normal.dot(motion) > 0.0 { -normal } else { normal };
        best = Some((fraction, normal, flags));
    }
    best.map(|(fraction, normal, flags)| (ddsim::RayHit {
        point: (from + motion * fraction).to_array(), normal: normal.to_array(),
    }, flags))
}

/// Translated query direction/response from 0x010fdea0 / 0x010fd710
/// (DDDA.exe 2017-12-13 Steam). The queried start is the target. Only the
/// nearest eligible contact's flags are returned after material exclusion.
fn camera_sphere_cast(ground: &dyn ddsim::Ground, start: Vec3, end: Vec3, radius: f32) -> Option<(Vec3, u32)> {
    let mut faces = Vec::new();
    let pad = Vec3::splat(radius);
    ground.triangles_in((start.min(end) - pad).to_array(), (start.max(end) + pad).to_array(), &mut faces);
    let motion = end - start;
    let mut nearest: Option<(f32, Vec3, f32, u32)> = None;
    for (face, flags) in &faces {
        if !camera_surface(*flags) { continue; }
        let Some((fraction, normal, gap)) = ddsim::sweep::sphere_contact(start.to_array(), radius, motion.to_array(), face) else { continue };
        if nearest.is_none_or(|hit| fraction < hit.0) {
            nearest = Some((fraction, Vec3::from(normal), gap, *flags));
        }
    }
    nearest.map(|(fraction, normal, gap, flags)| {
        let stop = if gap < 0.0 { start + normal * (0.1 - gap) }
            else { start + motion * fraction + normal * 0.1 };
        (stop, flags)
    })
}

/// Whether a sphere touches any collision triangle.
fn sphere_hits(ground: &dyn ddsim::Ground, c: Vec3, r: f32) -> bool {
    let mut tris = Vec::new();
    let (lo, hi) = ((c - Vec3::splat(r)).to_array(), (c + Vec3::splat(r)).to_array());
    if !ground.triangles_in(lo, hi, &mut tris) {
        return false;
    }
    tris.iter().any(|(t, flags)| camera_surface(*flags) && Vec3::from(ddsim::sweep::closest_on_triangle(c.to_array(), t)).distance_squared(c) < r * r)
}

/// The follow camera's projection: `CAMERA_FOV` plus the player's
/// CameraFov setting, and the recorded near plane.
fn camera_projection(cams: &mut Query<&mut Projection, With<FollowCam>>) {
    for mut p in cams.iter_mut() {
        if let Projection::Perspective(pp) = &mut *p {
            let fov = (CAMERA_FOV + config_camera_fov()).to_radians();
            if pp.fov != fov {
                pp.fov = fov;
                pp.near = CAMERA_NEAR * UNIT;
            }
        }
    }
}

/// `CameraFov` from the player's game settings (config.ini), else 0.
pub(crate) fn config_camera_fov() -> f32 {
    let Some(dir) = std::env::var_os("LOCALAPPDATA") else { return 0.0 };
    let path = std::path::PathBuf::from(dir).join("CAPCOM").join("DRAGONS DOGMA DARK ARISEN").join("config.ini");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("CameraFov=").and_then(|v| v.trim().parse().ok())))
        .unwrap_or(0.0)
}

/// Places the goblins and plays their motions (`Goblin::motion` / `frame`,
/// banks `ddenemyrt::goblin::BANKS`).
pub fn show_goblins(
    st: &mut PlayState,
    fight: Option<&Fight>,
    roots: &mut Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    alpha: f32,
) {
    let (Some(f), Some(d)) = (fight, st.fight_data.as_ref()) else { return };
    for (k, (g, v)) in f.goblins.iter().zip(st.goblins.iter_mut()).enumerate() {
        let Ok((mut tr, mut pb)) = roots.get_mut(v.entity) else { continue };
        let (pos, yaw) = st.prev_goblins.get(k).map_or((g.pos, g.yaw), |&(p, y)| between(p, y, g.pos, g.yaw, alpha));
        tr.translation = (Vec3::from(pos) - st.origin) * UNIT;
        tr.rotation = Quat::from_rotation_y(yaw);
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
    /// The small-enemy gauge (`Fight::small_gauge`): the goblin's body, HP
    /// and maximum HP.
    pub enemy: Option<(Entity, f32, f32)>,
}

/// The small-enemy gauge's goblin, if the fight shows one: its body (by the
/// fight's goblin order, as [`show_goblins`] pairs them), HP and maximum.
pub fn enemy_gauge(st: &PlayState, fight: Option<&Fight>) -> Option<(Entity, f32, f32)> {
    let f = fight?;
    let (owner, _) = f.small_gauge?;
    let k = f.goblins.iter().position(|g| g.fighter.owner == owner)?;
    let view = st.goblins.get(k)?;
    Some((view.entity, f.goblins[k].fighter.victim.hp(), view.hp_max))
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
    fn native_order_keys_and_second_local_keys_survive_short_presses() {
        for (key, mask) in [(KeyCode::F1, 1), (KeyCode::F2, 2), (KeyCode::F3, 8)] {
            let mut keys = ButtonInput::default();
            keys.press(key); keys.release(key);
            let input = p1_input(&keys, &ButtonInput::default(), 0.0);
            assert_eq!(input.pawn_orders.native_bits() >> 19, mask);
            assert_eq!(latch_input(input, Input::default()).pawn_orders, input.pawn_orders);
            assert!(active_input(&input));
            assert_eq!(p2_input(&keys, 0.0).pawn_orders.native_bits(), 0);
        }
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Numpad8);
        assert!(p2_input(&keys, 0.0).pawn_orders.down);
        assert_eq!(p1_input(&keys, &ButtonInput::default(), 0.0).pawn_orders.native_bits(), 0);
    }

    #[test]
    fn dpad_directions_are_scoped_to_the_matching_local_controller() {
        let mut world = World::new();
        for button in [GamepadButton::DPadUp, GamepadButton::DPadRight] {
            let mut pad = Gamepad::default();
            pad.digital_mut().press(button);
            world.spawn(pad);
        }
        let mut state: bevy::ecs::system::SystemState<Query<(Entity, &Gamepad)>> =
            bevy::ecs::system::SystemState::new(&mut world);
        let query = state.get(&world).unwrap();
        let first = gamepad_order_input(Input::default(), &query, 0);
        let second = gamepad_order_input(Input::default(), &query, 1);
        assert_ne!(first.pawn_orders.native_bits(), second.pawn_orders.native_bits());
        assert_eq!(first.pawn_orders.native_bits() | second.pawn_orders.native_bits(), 0x0048_0000);
        assert_eq!(gamepad_order_input(Input::default(), &query, 2).pawn_orders.native_bits(), 0);
    }

    /// A wall at z = -100 facing +z.
    struct Wall;
    impl ddsim::Ground for Wall {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
        fn ray(&self, from: [f32; 3], to: [f32; 3]) -> Option<ddsim::RayHit> {
            if to[2] >= -100.0 || from[2] < -100.0 {
                return None;
            }
            let t = (from[2] + 100.0) / (from[2] - to[2]);
            let p = std::array::from_fn(|k| from[k] + (to[k] - from[k]) * t);
            Some(ddsim::RayHit { point: p, normal: [0.0, 0.0, 1.0] })
        }
    }

    #[test]
    fn short_w_press_is_retained_until_a_fixed_tick() {
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::KeyW);
        keys.release(KeyCode::KeyW);
        let pressed = p1_input(&keys, &ButtonInput::default(), 0.0);
        assert_eq!(pressed.stick, [0.0, 1.0]);
        let buffered = latch_input(pressed, Input::default());
        assert_eq!(buffered.stick, [0.0, 1.0]);
        assert!(active_input(&buffered));
        assert!(!active_input(&latch_input(Input::default(), Input::default())));
    }

    #[test]
    fn held_wasd_moves_default_and_saved_spots() {
        let path = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !path.exists() {
            return;
        }
        let vfs = ddvfs::Vfs::open(path).unwrap();
        let mut spots = vec![(100, 58, 50, [215.9, 41054.3, 83921.1], 0.0)];
        if let Some(save) = std::env::var_os("DDDA_TEST_SAVE") {
            let save = ddsave::SaveFile::parse(&std::fs::read(save).unwrap()).unwrap();
            let spot = saved_spot(&save.xml).unwrap();
            spots.push((spot.stage, 50 + (spot.pos.z / 10000.0).floor() as i32,
                50 + (spot.pos.x / 10000.0).floor() as i32, spot.pos.to_array(), spot.yaw));
        }
        for (stage, a, b, start, yaw) in spots {
        let mut terrain = ddworld::Terrain::load_collision(&vfs, stage, a, b, 1);
        terrain.add_scenery(&vfs, stage, a, b, 1);
        for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
            let mut sim = ddhost::install::sim(&vfs).unwrap();
            sim.add_player(0, start, yaw);
            sim.player_mut(0).unwrap().relaxed = true;
            let mut keys = ButtonInput::default();
            keys.press(key);
            let buttons = ButtonInput::default();
            let input = p1_input(&keys, &buttons, yaw);
            let inputs = HashMap::from([(0, input)]);
            for _ in 0..30 {
                sim.step(1.0 / ddsim::MOTION_FPS, &inputs, &terrain);
            }
            let p = sim.player(0).unwrap();
            let moved = (p.position[0] - start[0]).hypot(p.position[2] - start[2]);
            eprintln!(
                "{key:?}: {start:?} -> {:?}, {:?}, moved {moved}",
                p.position, p.state
            );
            assert!(moved > 1.0, "{key:?} did not move: {:?}", p.position);
        }
        }
    }

    #[test]
    fn mouse_orbit_is_direct_and_respects_pitch_limits() {
        let (yaw, pitch) = orbit_angles(0.0, 0.0, Vec2::new(20.0, -20.0));
        // mouse right -> view turns right -> yaw decreases
        assert!(yaw < 0.0 && pitch < 0.0);
        assert_eq!(
            orbit_angles(0.0, 0.0, Vec2::new(0.0, -10000.0)).1,
            -60f32.to_radians()
        );
        assert_eq!(
            orbit_angles(0.0, 0.0, Vec2::new(0.0, 10000.0)).1,
            30f32.to_radians()
        );
    }

    #[test]
    fn bodies_are_drawn_between_fixed_frames() {
        let (p, y) = between([0.0, 0.0, 0.0], 0.0, [10.0, 0.0, 20.0], 1.0, 0.25);
        assert_eq!(p, [2.5, 0.0, 5.0]);
        assert!((y - 0.25).abs() < 1e-6);
        // yaw the short way across +-180 degrees
        let (_, y) = between([0.0; 3], 3.0, [0.0; 3], -3.0, 0.5);
        assert!((y.abs() - std::f32::consts::PI).abs() < 0.2, "{y}");
        // a warp is drawn at once
        let (p, _) = between([0.0; 3], 0.0, [1000.0, 0.0, 0.0], 0.0, 0.1);
        assert_eq!(p, [1000.0, 0.0, 0.0]);
    }

    #[test]
    fn the_follow_camera_keeps_its_distance_and_stops_at_walls() {
        struct Open;
        impl ddsim::Ground for Open {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
                Some(0.0)
            }
        }
        // yaw 0 looks along +z: the eye is behind, at -z
        let (eye, target) = follow_camera(&Open, Vec3::ZERO, 0.0, 0.0);
        assert_eq!(target, Vec3::new(0.0, CAMERA_TARGET_HEIGHT, 0.0));
        assert!((eye - target).length() - CAMERA_DISTANCE < 1e-3);
        assert!(eye.z < 0.0);
        // a wall 100 cm behind found by the ray: the eye stops on it
        let (eye, _) = follow_camera(&Wall, Vec3::ZERO, 0.0, 0.0);
        assert!((eye.z + 100.0).abs() < 1e-3, "{eye:?}");
        // a wall 230 cm behind that only the sphere meets (0x007de570): the
        // sphere (1.3 x near = 20.8) one near plane (16) in front of the eye
        // clears it at z = -230 + 20.8, so the eye stops at -209.2 - 16
        struct Plane;
        impl ddsim::Ground for Plane {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
                Some(0.0)
            }
            fn triangles_in(&self, lo: [f32; 3], _: [f32; 3], out: &mut Vec<ddsim::sweep::Face>) -> bool {
                if lo[2] > -230.0 {
                    return false;
                }
                let z = -230.0;
                out.push(([[-1e4, -1e4, z], [1e4, -1e4, z], [0.0, 1e4, z]], 0));
                true
            }
        }
        let (eye, _) = follow_camera(&Plane, Vec3::ZERO, 0.0, 0.0);
        assert!((eye.z - (-230.0 + 1.3 * CAMERA_NEAR - CAMERA_NEAR)).abs() < 1.0, "{eye:?}");
    }

    #[test]
    fn camera_cast_stops_at_first_thin_wall_from_target() {
        let mut terrain = ddworld::Terrain::empty();
        for z in [-30.0, -110.0] {
            terrain.add([[-1000.0, -1000.0, z], [1000.0, -1000.0, z], [0.0, 1000.0, z]]);
        }
        // End overlaps the second wall, but the cast must meet the first
        // one. Searching backwards from the end can choose the wrong pocket.
        let (stop, _) = camera_sphere_cast(&terrain, Vec3::ZERO, Vec3::new(0.0, 0.0, -100.0), 20.0).unwrap();
        assert!((stop.z + 9.9).abs() < 0.002, "{stop:?}");
        assert!(!sphere_hits(&terrain, stop, 20.0));
    }

    #[test]
    fn camera_prefilters_excluded_material_before_nearest_contact() {
        let mut terrain = ddworld::Terrain::empty();
        for (z, flags) in [(-150.0, 0x10), (-190.0, 0)] {
            terrain.add_flagged([[-1000.0, -1000.0, z], [1000.0, -1000.0, z], [0.0, 1000.0, z]], flags);
        }
        let (eye, _) = follow_camera(&terrain, Vec3::ZERO, 0.0, 0.0);
        assert!((eye.z + 185.1).abs() < 0.002, "{eye:?}");
        let (_, flags) = camera_sphere_cast(&terrain, Vec3::new(0.0, 140.0, 0.0), Vec3::new(0.0, 140.0, -214.0), 20.8).unwrap();
        assert_eq!(flags, 0);
        let mut excluded = ddworld::Terrain::empty();
        excluded.add_flagged([[-1000.0, -1000.0, -190.0], [1000.0, -1000.0, -190.0], [0.0, 1000.0, -190.0]], 0x0080_0000);
        let (eye, _) = follow_camera(&excluded, Vec3::ZERO, 0.0, 0.0);
        assert!((eye.z + CAMERA_DISTANCE).abs() < 0.001, "{eye:?}");
    }

    #[test]
    fn camera_authored_anchor_adapter_converts_viewer_pitch_sign() {
        struct Capture(std::cell::Cell<[f32; 3]>);
        impl ddsim::Ground for Capture {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
            fn ray_with_flags(&self, from: [f32; 3], _: [f32; 3]) -> Option<(ddsim::RayHit, u32)> {
                self.0.set(from); None
            }
        }
        let ground = Capture(std::cell::Cell::new([0.0; 3]));
        let params = mtcamera::params::Normal {
            distance: 300.0, height: 123.0, fov_degrees: 67.0, camera_target: 0,
            screen_adjust_type: 0, screen_adjust_flags: 1, screen_adjust_height: 0.0,
        };
        let feet = Vec3::new(10.0, 20.0, 30.0);
        follow_camera_with_params(&ground, feet, 0.0, 0.8, Some(&params));
        assert_eq!(ground.0.get(), [10.0, 160.0, 30.0]);
        let (eye, target) = follow_camera_with_params(&ground, feet, 0.0, 0.81, Some(&params));
        assert_eq!(ground.0.get(), [10.0, 143.0, 30.0]);
        assert!((eye.distance(target) - 300.0).abs() < 0.001);
    }

    #[test]
    fn camera_query_keeps_view_direction_separate_from_anchor() {
        // Synthetic perpendicular query: the 0x007de570 angular allowance
        // adds 10 cm, while the near-plane offset remains on the view axis.
        let mut terrain = ddworld::Terrain::empty();
        terrain.add([[-1000.0, -1000.0, 90.0], [1000.0, -1000.0, 90.0], [0.0, 1000.0, 90.0]]);
        let desired_eye = Vec3::new(0.0, 0.0, 100.0);
        let target = desired_eye - Vec3::X * 100.0;
        let (eye, corrected_target) = collide_camera_query(&terrain, desired_eye, target,
            Vec3::ZERO, Vec3::X, 16.0);
        assert!((eye.z - 59.1).abs() < 0.002, "{eye:?}");
        // The sphere sweeps toward (ray-hit - view*near), then the eye
        // recovers the same near offset. It must not substitute ray direction.
        assert!(eye.x > 0.0 && eye.x < 16.0, "{eye:?}");
        assert!((corrected_target - eye + Vec3::X * 100.0).length() < 0.001);
    }

    #[test]
    fn camera_sphere_initial_overlap_uses_penetration_normal() {
        let mut terrain = ddworld::Terrain::empty();
        terrain.add([[-1000.0, -1000.0, -10.0], [1000.0, -1000.0, -10.0], [0.0, 1000.0, -10.0]]);
        let (stop, _) = camera_sphere_cast(&terrain, Vec3::ZERO, Vec3::new(0.0, 0.0, -50.0), 20.0).unwrap();
        assert!((stop.z - 10.1).abs() < 0.002, "{stop:?}");
    }

    #[test]
    fn camera_wall_and_roof_recovery_translate_the_view_without_rotating() {
        // 0x007e1400 translates both endpoints after the collision helper.
        // These contacts push perpendicular to the viewing direction,
        // exposing the missing caller response even with a zero-pitch view.
        for triangle in [
            [[-10.0, -1000.0, -1000.0], [-10.0, 1000.0, -1000.0], [-10.0, 0.0, 1000.0]],
            [[-1000.0, 150.0, -1000.0], [1000.0, 150.0, -1000.0], [0.0, 150.0, 1000.0]],
        ] {
            let mut terrain = ddworld::Terrain::empty();
            terrain.add(triangle);
            let desired_target = Vec3::Y * CAMERA_TARGET_HEIGHT;
            let desired_eye = desired_target - Vec3::Z * CAMERA_DISTANCE;
            let (eye, target) = follow_camera(&terrain, Vec3::ZERO, 0.0, 0.0);
            assert!(eye.distance(desired_eye) > 1.0);
            assert!((target - desired_target - (eye - desired_eye)).length() < 0.001);
            assert!((target - eye - Vec3::Z * CAMERA_DISTANCE).length() < 0.001);
        }
    }

    #[test]
    fn camera_active_controller_near_does_not_hit_clear_side_wall_or_roof() {
        // The child uCameraGame's 96 cm near plane incorrectly gave this
        // query a 124.8 cm radius. Active uCameraCtrl uses 16 cm (20.8 cm
        // radius), so a wall 60 cm away or roof 70 cm above is clear.
        let mut terrain = ddworld::Terrain::empty();
        terrain.add([[-60.0, -1000.0, -1000.0], [-60.0, 1000.0, -1000.0], [-60.0, 0.0, 1000.0]]);
        terrain.add([[-1000.0, 210.0, -1000.0], [1000.0, 210.0, -1000.0], [0.0, 210.0, 1000.0]]);
        let (eye, target) = follow_camera(&terrain, Vec3::ZERO, 0.0, 0.0);
        assert_eq!(eye, Vec3::new(0.0, CAMERA_TARGET_HEIGHT, -CAMERA_DISTANCE));
        assert_eq!(target, Vec3::Y * CAMERA_TARGET_HEIGHT);
    }

    #[test]
    #[ignore = "requires private original camera pose samples and game installation"]
    fn camera_compare_private_original_poses() {
        // Each private row: candidate eye, candidate target, actual eye,
        // actual target, active near; optionally anchor and outward direction.
        // No recordings are embedded in repo.
        let path = std::env::var("DDDA_CAMERA_POSES").expect("private pose file");
        let native = std::env::var("DDDA_NATIVE_PC").expect("nativePC path");
        let tolerance = std::env::var("DDDA_CAMERA_TOLERANCE_CM").ok()
            .map(|value| value.parse::<f32>().expect("camera tolerance in cm"));
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let terrain = ddhost::install::stage_collision(&vfs, 200).unwrap();
        for (index, line) in std::fs::read_to_string(path).unwrap().lines().enumerate() {
            let v: Vec<f32> = line.split(',').map(|s| s.parse().unwrap()).collect();
            assert!(v.len() == 13 || v.len() == 19);
            let point = |start| Vec3::new(v[start], v[start + 1], v[start + 2]);
            let (eye, target) = if v.len() == 19 {
                collide_camera_query(&terrain, point(0), point(3), point(13), point(16), v[12])
            } else {
                collide_camera(&terrain, point(0), point(3), v[12])
            };
            let eye_error = eye.distance(point(6));
            let target_error = target.distance(point(9));
            eprintln!("camera sample {index}: eye={eye:?} target={target:?}; errors eye={eye_error} target={target_error}");
            if let Some(tolerance) = tolerance {
                assert!(eye_error <= tolerance && target_error <= tolerance,
                    "sample {index} exceeds {tolerance} cm");
            }
        }
    }

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
