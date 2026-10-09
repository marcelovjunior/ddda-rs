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
//! The camera orbits P1 (hold right mouse to turn it).

use std::collections::HashMap;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use ddsim::{Input, JumpParam, PlayerId, RootMotion, Sim};

use crate::character::{Playback, RMOTIONLIST};
use crate::stage::{Loader, UNIT};

/// Bank 0 motion list of the male Arisen (docs/ANIMATION.md).
pub const MOTIONS: &str = r"motion\pl\m\m00\m0000_co\m0000_co";
pub const BODY: &str = r"model\pl\m\m_base\m000\m000";
pub const JUMP_PARAMS: &str = r"param\pl\other\pljumpparam";

/// Every motion the controller uses (ddsim::State::motion_id).
const USED_MOTIONS: [u16; 16] = [0, 3, 4, 7, 10, 11, 12, 13, 14, 15, 20, 22, 23, 43, 47, 48];

#[derive(Resource)]
pub struct PlayState {
    pub sim: Sim,
    pub terrain: ddworld::Terrain,
    /// Stage-frame position (cm) of the viewer's origin.
    pub origin: Vec3,
    pub motions: HashMap<u16, mtlmt::Motion>,
    pub entities: HashMap<PlayerId, Entity>,
    pub follow: PlayerId,
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    /// Scripted input for unattended tests: P1 runs and jumps, P2 sprints.
    pub demo: bool,
    /// Motion each character's Playback currently shows.
    shown: HashMap<PlayerId, u16>,
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
    let bytes = loader.vfs.read(MOTIONS, RMOTIONLIST)?;
    let list = mtlmt::parse(&bytes).map_err(|e| e.to_string())?;
    let mut motions = HashMap::new();
    let mut roots = HashMap::new();
    for id in USED_MOTIONS {
        let slot = usize::from(id & 0xFF);
        let m = list
            .motions
            .get(slot)
            .cloned()
            .flatten()
            .ok_or(format!("{MOTIONS}: slot {slot} missing"))?;
        roots.insert(id, RootMotion::from_motion(&m));
        motions.insert(id, m);
    }
    let mut sim = Sim::new(roots);
    // Normal jump = set 0, sprint jump = set 9, read from the install.
    let jp = jump_params(loader, &[0, 9])?;
    sim.jump_normal = jp[0];
    sim.jump_sprint = jp[1];
    info!(
        "jump params from {JUMP_PARAMS}: normal {:?}, sprint {:?}",
        jp[0], jp[1]
    );
    let mut entities = HashMap::new();
    for id in 0..players {
        // players side by side, 1.5 m apart
        let pos = spawn + Vec3::new(150.0 * f32::from(id), 0.0, 0.0);
        sim.add_player(id, pos.to_array(), 0.0);
        // as a dagger vocation: the second jump is available
        sim.player_mut(id).unwrap().double_jump = true;
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
        entities,
        follow: 0,
        cam_yaw: 0.0,
        cam_pitch: -0.25,
        demo: false,
        shown: HashMap::new(),
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

/// One tick: input -> simulation -> character transforms and motions.
#[allow(clippy::type_complexity)]
pub fn step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
) {
    let Some(mut st) = state else { return };
    if buttons.pressed(MouseButton::Right) {
        st.cam_yaw -= mouse.delta.x * 0.004;
        st.cam_pitch = (st.cam_pitch - mouse.delta.y * 0.004).clamp(-1.2, 0.4);
    }
    let cam_yaw = st.cam_yaw;
    let mut inputs = HashMap::new();
    inputs.insert(
        0,
        Input {
            stick: stick(&keys, KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD),
            walk: keys.pressed(KeyCode::ControlLeft),
            sprint: keys.pressed(KeyCode::ShiftLeft),
            jump: keys.pressed(KeyCode::Space),
            camera_yaw: cam_yaw,
        },
    );
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
    for p in &st.sim.players {
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
                pb.motion = m.clone();
            }
            st.shown.insert(p.id, id);
        }
        let shown = st.motions.get(&id).map_or(p.frame, |m| display_frame(m, p.frame));
        pb.hold_frame = Some(shown);
    }
    // Follow camera: behind P1 at the orbit angles, looking at the chest.
    if let (Some(p), Ok(mut cam)) = (st.sim.player(st.follow), cams.single_mut()) {
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
