//! `--play`: the headless simulation (`ddsim`) driven by local input, with
//! every player shown as a character. Two local players exercise the
//! N-player design before networking exists.
//!
//! | | P1 | P2 |
//! |---|---|---|
//! | move (runs) | W A S D | arrow keys |
//! | walk | Left Ctrl | Right Ctrl |
//! | sprint | Left Shift | Right Shift |
//! | jump (twice: double jump) | Space | Enter |
//!
//! The camera orbits P1 (hold right mouse to turn it). F2 switches every
//! player between the town (relaxed) walk and run and the field ones; the
//! area attribute that picks them in the original is not read yet.

use std::collections::HashMap;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use ddsim::{Input, JumpParam, MoveParam, PlayerId, RootMotion, Sim};

use crate::character::{Playback, RMOTIONLIST};
use crate::stage::{Loader, UNIT};

/// Bank 0 motion list of the male Arisen (docs/ANIMATION.md).
pub const MOTIONS: &str = r"motion\pl\m\m00\m0000_co\m0000_co";
pub const BODY: &str = r"model\pl\m\m_base\m000\m000";
pub const JUMP_PARAMS: &str = r"param\pl\other\pljumpparam";
/// Player tuning floats read by id (ground speeds; docs/MOVEMENT.md).
pub const ADJUST_PARAMS: &str = r"param\pl\other\m_pl000_00";

/// Every motion the controller uses (ddsim::State::motion_id).
const USED_MOTIONS: [u16; 25] = [
    0, 3, 4, 7, 10, 11, 12, 13, 14, 15, 20, 21, 22, 23, 24, 43, 45, 46, 47, 48, 49, 50, 0x54, 0x55, 0x611,
];

/// Motion list of a bank: 0 is the common set; 6 is the equipped weapon
/// set's, here the dagger set's `m0004_co` (bank table at 0x014F1D30;
/// which group is the dagger's is not traced, docs/MOVEMENT.md).
fn bank_list(bank: u16) -> Option<&'static str> {
    match bank {
        0 => Some(MOTIONS),
        6 => Some(r"motion\pl\m\m00\m0004_co\m0004_co"),
        _ => None,
    }
}

#[derive(Resource)]
pub struct PlayState {
    pub sim: Sim,
    pub terrain: ddworld::Terrain,
    /// Stage-frame position (cm) of the viewer's origin.
    pub origin: Vec3,
    pub motions: HashMap<u16, mtlmt::Motion>,
    /// Default interpolation frames per motion slot, by bank: the
    /// `rAdjustParam` stored under each motion list's path
    /// (docs/ANIMATION.md).
    pub blend_tables: HashMap<u16, Vec<f32>>,
    pub entities: HashMap<PlayerId, Entity>,
    pub follow: PlayerId,
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    /// Scripted input for unattended tests: P1 runs and jumps, P2 sprints.
    pub demo: bool,
    /// Motion each character's Playback currently shows.
    shown: HashMap<PlayerId, u16>,
    /// Whether each character has an Arisen to show (network sessions
    /// spawn characters for every seat up front).
    pub visible: HashMap<Entity, bool>,
}

#[derive(Component)]
pub struct FollowCam;

/// Rows `index` of `pljumpparam` from the install.
fn jump_params(loader: &Loader, indices: &[usize]) -> Result<Vec<JumpParam>, String> {
    let bytes = loader.vfs.read(JUMP_PARAMS, mtarc::type_hash("rPlJumpParam"))?;
    let x = mtxfs::parse(&bytes).map_err(|e| e.to_string())?;
    let rows = x.get(&x.root, "mArray").ok_or("pljumpparam: no mArray")?;
    let f = |o: &mtxfs::Object, n: &str| -> Result<f32, String> {
        match x.get(o, n).and_then(|v| v.first()) {
            Some(mtxfs::Value::F(v)) => Ok(v[0] as f32),
            _ => Err(format!("pljumpparam: no {n}")),
        }
    };
    indices
        .iter()
        .map(|&i| {
            let Some(mtxfs::Value::Object(Some(o))) = rows.get(i) else {
                return Err(format!("pljumpparam: no row {i}"));
            };
            Ok(JumpParam {
                speed_y: f(o, "mSpeedY")?,
                speed_z: f(o, "mSpeedZ")?,
                gravity: f(o, "mGravity")?,
                add_move_xz: f(o, "mAddMoveSpeedXZ")?,
                damping_z: f(o, "mDampingZ")?,
            })
        })
        .collect()
}

/// The float table of an `rAdjustParam` (docs/FORMATS.md): "ajp\0", u32
/// version, u32 count, then `count` f32.
fn adjust_table(loader: &Loader, path: &str) -> Result<Vec<f32>, String> {
    let b = loader.vfs.read(path, mtarc::type_hash("rAdjustParam"))?;
    if b.len() < 12 || &b[..4] != b"ajp\0" {
        return Err(format!("{path}: not an ajp file"));
    }
    let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
    let body = b
        .get(12..12 + 4 * n)
        .ok_or(format!("{path}: {n} floats do not fit"))?;
    Ok(body
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

/// Builds the simulation around `spawn` (stage frame, cm) and spawns one
/// character per player.
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
    spawn: Vec3,
    players: u8,
) -> Result<PlayState, String> {
    let mut lists: HashMap<u16, mtlmt::MotionList> = HashMap::new();
    let mut blend_tables = HashMap::new();
    let mut motions = HashMap::new();
    let mut roots = HashMap::new();
    for id in USED_MOTIONS {
        let bank = id >> 8;
        let path = bank_list(bank).ok_or(format!("motion bank {bank} not mapped"))?;
        if !lists.contains_key(&bank) {
            let bytes = loader.vfs.read(path, RMOTIONLIST)?;
            lists.insert(bank, mtlmt::parse(&bytes).map_err(|e| e.to_string())?);
            // the bank table (0x014F1D30) loads the same path as both
            // rMotionList and rAdjustParam (0x00B7FF30)
            blend_tables.insert(bank, adjust_table(loader, path)?);
        }
        let slot = usize::from(id & 0xFF);
        let m = lists[&bank]
            .motions
            .get(slot)
            .cloned()
            .flatten()
            .ok_or(format!("{path}: slot {slot} missing"))?;
        // curves drive the sprint stop, sprint ramp and ledge climb; events
        // drive the stamina hold (as ddaccept::new_sim)
        roots.insert(
            id,
            RootMotion::from_motion(&m)
                .with_curves(lists[&bank].curves[slot].clone())
                .with_events(lists[&bank].events[slot].clone()),
        );
        motions.insert(id, m);
    }
    let mut sim = Sim::new(roots);
    // Normal jump = set 0, sprint jump = set 9, read from the install.
    let jp = jump_params(loader, &[0, 9])?;
    sim.jump_normal = jp[0];
    sim.jump_sprint = jp[1];
    let adjust = adjust_table(loader, ADJUST_PARAMS)?;
    sim.move_param = MoveParam::from_adjust(&adjust).ok_or(format!("{ADJUST_PARAMS}: too short"))?;
    sim.ledge_param =
        ddsim::LedgeParam::from_adjust(&adjust).ok_or(format!("{ADJUST_PARAMS}: no ledge params"))?;
    info!("ground speeds from {ADJUST_PARAMS}: {:?}", sim.move_param);
    info!(
        "jump params from {JUMP_PARAMS}: normal {:?}, sprint {:?}",
        jp[0], jp[1]
    );
    let mut entities = HashMap::new();
    for id in 0..players {
        // players side by side, 1.5 m apart
        let pos = spawn + Vec3::new(150.0 * f32::from(id), 0.0, 0.0);
        sim.add_player(id, pos.to_array(), 0.0);
        let p = sim.player_mut(id).unwrap();
        // as a dagger vocation: the second jump is available
        p.double_jump = true;
        // the start area was recorded with the relaxed run (docs/MOVEMENT.md)
        p.relaxed = true;
        let e = crate::character::spawn(
            commands,
            loader,
            meshes,
            materials,
            images,
            bindposes,
            BODY,
            MOTIONS,
            0,
            (pos - origin) * UNIT,
            Some(0.0),
        )?;
        entities.insert(id, e);
    }
    Ok(PlayState {
        sim,
        terrain,
        origin,
        motions,
        blend_tables,
        entities,
        follow: 0,
        cam_yaw: 0.0,
        cam_pitch: -0.25,
        demo: false,
        shown: HashMap::new(),
        visible: HashMap::new(),
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
fn interpolation_frames(tables: &HashMap<u16, Vec<f32>>, motion: u16) -> f32 {
    // cPlCliffHang::vf05 (0x00ADCC50) requests 0 frames; every other action
    // ddsim models requests -1, i.e. the motion's default
    // (docs/ANIMATION.md)
    if motion == 0x2F {
        return 0.0;
    }
    tables
        .get(&(motion >> 8))
        .and_then(|t| mtlmt::blend::default_frames(t, motion))
        .unwrap_or(0.0)
}

/// One tick: input -> simulation -> character transforms and motions.
#[allow(clippy::type_complexity)]
pub fn step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    net: Option<Res<crate::net::NetRes>>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
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
        info!(
            "{} walk and run",
            if relaxed { "town (relaxed)" } else { "field" }
        );
    }
    orbit(&mut st, &buttons, &mouse);
    let cam_yaw = st.cam_yaw;
    let mut inputs = HashMap::new();
    inputs.insert(0, p1_input(&keys, cam_yaw));
    inputs.insert(
        1,
        Input {
            stick: stick(
                &keys,
                KeyCode::ArrowUp,
                KeyCode::ArrowDown,
                KeyCode::ArrowLeft,
                KeyCode::ArrowRight,
            ),
            walk: keys.pressed(KeyCode::ControlRight),
            sprint: keys.pressed(KeyCode::ShiftRight),
            jump: keys.pressed(KeyCode::Enter) || keys.pressed(KeyCode::NumpadEnter),
            camera_yaw: cam_yaw,
            ..Default::default()
        },
    );
    if st.demo {
        // P1 runs and jumps every 2 s; P2 sprints
        let t = time.elapsed_secs();
        inputs.insert(
            0,
            Input {
                stick: [0.0, 1.0],
                jump: t % 2.0 < 0.1,
                camera_yaw: cam_yaw,
                ..Default::default()
            },
        );
        inputs.insert(
            1,
            Input {
                stick: [0.0, 1.0],
                sprint: true,
                camera_yaw: cam_yaw,
                ..Default::default()
            },
        );
    }
    let dt = time.delta_secs().min(0.1);
    let st = &mut *st;
    st.sim.step(dt, &inputs, &st.terrain);
    let shown: Vec<Shown> = st.sim.players.iter().map(Shown::of).collect();
    show(st, &shown, &mut roots, &mut cams);
}

/// Turns the follow camera while the right mouse button is held.
pub fn orbit(st: &mut PlayState, buttons: &ButtonInput<MouseButton>, mouse: &AccumulatedMouseMotion) {
    if buttons.pressed(MouseButton::Right) {
        st.cam_yaw -= mouse.delta.x * 0.004;
        st.cam_pitch = (st.cam_pitch - mouse.delta.y * 0.004).clamp(-1.2, 0.4);
    }
}

/// Player one's controls (W A S D, Ctrl, Shift, Space).
pub fn p1_input(keys: &ButtonInput<KeyCode>, cam_yaw: f32) -> Input {
    Input {
        stick: stick(keys, KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD),
        walk: keys.pressed(KeyCode::ControlLeft),
        sprint: keys.pressed(KeyCode::ShiftLeft),
        jump: keys.pressed(KeyCode::Space),
        camera_yaw: cam_yaw,
        ..Default::default()
    }
}

/// What the view needs of one Arisen.
#[derive(Debug, Clone, Copy)]
pub struct Shown {
    pub id: PlayerId,
    pub position: [f32; 3],
    pub yaw: f32,
    pub state: ddsim::State,
    pub frame: f32,
}

impl Shown {
    pub fn of(p: &ddsim::Player) -> Shown {
        Shown { id: p.id, position: p.position, yaw: p.yaw, state: p.state, frame: p.frame }
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
        if st.shown.get(&p.id) != Some(&id) {
            if let Some(m) = st.motions.get(&id) {
                // the first motion of a model does not blend (uModel::vf30)
                let frames = if st.shown.contains_key(&p.id) {
                    interpolation_frames(&st.blend_tables, id)
                } else {
                    0.0
                };
                debug!("player {}: motion {:#x}, blend {frames} frames", p.id, id);
                pb.change_motion(m.clone(), frames);
            }
            st.shown.insert(p.id, id);
        }
        let shown = st.motions.get(&id).map_or(p.frame, |m| display_frame(m, p.frame));
        pb.hold_frame = Some(shown);
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
