//! `--play`: the headless simulation (`ddsim`) driven by local input, with
//! every player shown as a character. Two local players exercise the N-player
//! design before networking exists: P1 on WASD + left Shift, P2 on the arrow
//! keys + right Shift. The camera orbits P1 (hold right mouse to turn it).

use std::collections::HashMap;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use ddsim::{Action, Input, PlayerId, RootMotion, Sim};

use crate::character::{Playback, RMOTIONLIST};
use crate::stage::{Loader, UNIT};

/// Bank 0 motion list of the male Arisen (docs/ANIMATION.md).
pub const MOTIONS: &str = r"motion\pl\m\m00\m0000_co\m0000_co";
pub const BODY: &str = r"model\pl\m\m_base\m000\m000";

#[derive(Resource)]
pub struct PlayState {
    pub sim: Sim,
    pub terrain: ddworld::Terrain,
    /// Stage-frame position (cm) of the viewer's origin.
    pub origin: Vec3,
    pub motions: HashMap<Action, mtlmt::Motion>,
    pub entities: HashMap<PlayerId, Entity>,
    pub follow: PlayerId,
    pub cam_yaw: f32,
    pub cam_pitch: f32,
    /// Scripted input for unattended tests: P1 walks forward, P2 runs.
    pub demo: bool,
    /// Action each character's Playback currently shows.
    shown: HashMap<PlayerId, Action>,
}

#[derive(Component)]
pub struct FollowCam;

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
    for action in [Action::Wait, Action::Walk, Action::Run] {
        let slot = usize::from(action.motion_id() & 0xFF);
        let m = list
            .motions
            .get(slot)
            .cloned()
            .flatten()
            .ok_or(format!("{MOTIONS}: slot {slot} missing"))?;
        roots.insert(action, RootMotion::from_motion(&m));
        motions.insert(action, m);
    }
    let mut sim = Sim::new(roots);
    let mut entities = HashMap::new();
    for id in 0..players {
        // players side by side, 1.5 m apart
        let pos = spawn + Vec3::new(150.0 * f32::from(id), 0.0, 0.0);
        sim.add_player(id, pos.to_array(), 0.0);
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
    // Camera yaw in the sim's convention (0 looks along +Z).
    let cam_yaw = st.cam_yaw;
    let mut inputs = HashMap::new();
    inputs.insert(
        0,
        Input {
            stick: stick(&keys, KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD),
            run: keys.pressed(KeyCode::ShiftLeft),
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
            run: keys.pressed(KeyCode::ShiftRight),
            camera_yaw: cam_yaw,
        },
    );
    if st.demo {
        for (id, run) in [(0u8, false), (1u8, true)] {
            inputs.insert(
                id,
                Input {
                    stick: [0.0, 1.0],
                    run,
                    camera_yaw: cam_yaw,
                },
            );
        }
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
        if st.shown.get(&p.id) != Some(&p.action) {
            if let Some(m) = st.motions.get(&p.action) {
                pb.motion = m.clone();
            }
            st.shown.insert(p.id, p.action);
        }
        pb.hold_frame = Some(p.frame);
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
