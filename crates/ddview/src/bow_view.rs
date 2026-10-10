//! Offline bow presentation and platform-to-simulation adapter (S27).
//! The simulation owns phases, shot timing, random cone and damage.
use bevy::prelude::*;
use ddhost::ddenemyrt::fight::{BowContext, Fight};
use ddsim::{Ground, PlayerId, Sim};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Binding { Key(KeyCode), Mouse(MouseButton) }
impl Binding {
    fn down(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Self::Key(k) => keys.pressed(k) || keys.just_pressed(k),
            Self::Mouse(m) => mouse.pressed(m) || mouse.just_pressed(m),
        }
    }
}

/// Config's keyboard values are DirectInput scan codes; mouse buttons occupy
/// bits above the byte. This is a platform mapping, not embedded game data.
fn binding(code: u32) -> Option<Binding> {
    use KeyCode::*;
    let key = match code {
        1 => Escape, 2 => Digit1, 3 => Digit2, 4 => Digit3, 5 => Digit4,
        6 => Digit5, 7 => Digit6, 8 => Digit7, 9 => Digit8, 10 => Digit9,
        11 => Digit0, 12 => Minus, 13 => Equal, 14 => Backspace, 15 => Tab,
        16 => KeyQ, 17 => KeyW, 18 => KeyE, 19 => KeyR, 20 => KeyT,
        21 => KeyY, 22 => KeyU, 23 => KeyI, 24 => KeyO, 25 => KeyP,
        26 => BracketLeft, 27 => BracketRight, 28 => Enter, 29 => ControlLeft,
        30 => KeyA, 31 => KeyS, 32 => KeyD, 33 => KeyF, 34 => KeyG,
        35 => KeyH, 36 => KeyJ, 37 => KeyK, 38 => KeyL, 39 => Semicolon,
        40 => Quote, 41 => Backquote, 42 => ShiftLeft, 43 => Backslash,
        44 => KeyZ, 45 => KeyX, 46 => KeyC, 47 => KeyV, 48 => KeyB,
        49 => KeyN, 50 => KeyM, 51 => Comma, 52 => Period, 53 => Slash,
        54 => ShiftRight, 55 => NumpadMultiply, 56 => AltLeft, 57 => Space,
        58 => CapsLock, 59 => F1, 60 => F2, 61 => F3, 62 => F4, 63 => F5,
        64 => F6, 65 => F7, 66 => F8, 67 => F9, 68 => F10, 69 => NumLock,
        70 => ScrollLock, 71 => Numpad7, 72 => Numpad8, 73 => Numpad9,
        74 => NumpadSubtract, 75 => Numpad4, 76 => Numpad5, 77 => Numpad6,
        78 => NumpadAdd, 79 => Numpad1, 80 => Numpad2, 81 => Numpad3,
        82 => Numpad0, 83 => NumpadDecimal, 87 => F11, 88 => F12,
        156 => NumpadEnter, 157 => ControlRight, 181 => NumpadDivide,
        184 => AltRight, 199 => Home, 200 => ArrowUp, 201 => PageUp,
        203 => ArrowLeft, 205 => ArrowRight, 207 => End, 208 => ArrowDown,
        209 => PageDown, 210 => Insert, 211 => Delete,
        256 => return Some(Binding::Mouse(MouseButton::Left)),
        512 => return Some(Binding::Mouse(MouseButton::Right)),
        1024 => return Some(Binding::Mouse(MouseButton::Middle)),
        _ => return None,
    };
    Some(Binding::Key(key))
}

#[derive(Default)]
pub struct Controls { toggle: Option<Binding>, held: Option<Binding>, shot: Option<Binding> }
impl Controls {
    /// Names registered by 0x0040A6C0. No guessed default is installed when
    /// a setting is absent or an unsupported input code is configured.
    pub fn parse(text: &str) -> Self {
        let get = |name: &str| text.lines().find_map(|line| {
            let (key, value) = line.trim().split_once('=')?;
            (key.trim() == name).then(|| value.trim().parse().ok().and_then(binding)).flatten()
        });
        Self { toggle: get("STG_TOGGLE_BOW_MODE"), held: get("STG_SUB_WEP"), shot: get("STG_ATTACK_0") }
    }
    pub fn load() -> Self {
        let text = std::env::var_os("LOCALAPPDATA").and_then(|dir| {
            std::fs::read_to_string(std::path::PathBuf::from(dir)
                .join("CAPCOM/DRAGONS DOGMA DARK ARISEN/config.ini")).ok()
        }).unwrap_or_default();
        Self::parse(&text)
    }
    pub fn apply(&self, index: usize, input: &mut ddsim::Input,
        keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>, bow_active: bool) {
        // P2 bindings are explicitly viewer choices, independent of P1's config.
        let (toggle, held) = if index == 0 { (self.toggle, self.held) }
            else { (Some(Binding::Key(KeyCode::NumpadSubtract)), Some(Binding::Key(KeyCode::NumpadDivide))) };
        input.bow_toggle = toggle.is_some_and(|b| b.down(keys, mouse));
        input.bow = held.is_some_and(|b| b.down(keys, mouse));
        if index == 0 && (bow_active || input.bow || input.bow_toggle) {
            input.light |= self.shot.is_some_and(|b| b.down(keys, mouse));
        }
    }
}

/// Installed camera mode selection: 0x007DCF30 picks normal slot 1 when
/// actor +0x273C bit 2 is set by bow enter 0x00B16FE0 (2017-12-13 Steam).
pub struct AimCamera {
    pub normal: mtcamera::params::Normal,
    pub shoulder: f32,
    pub pitch_min: f32,
    pub pitch_max: f32,
}
pub fn aim_camera(vfs: &ddvfs::Vfs) -> Result<AimCamera, String> {
    use mtxfs::Value;
    // 0x00421010 loads this into sCamera+0xD00 for secondary categories
    // 10/11/12. B1E3A0 requests the installed basic row's cameraNo.
    let bow = mtxfs::parse(&vfs.read(ddhost::ddenemyrt::arrow::BOW_PARAM_PATH,
        mtarc::type_hash("rBowActParamList"))?).map_err(|e| e.to_string())?;
    let camera = mtxfs::parse(&vfs.read(r"param\cam\pl\pl06",
        mtarc::type_hash("rCameraParamList"))?).map_err(|e| e.to_string())?;
    fn slots(x: &mtxfs::Xfs) -> Result<&[Value], String> {
        let Some([Value::Object(Some(array))]) = x.get(&x.root, "mParamList") else { return Err("missing parameter array".into()) };
        x.get(array, "mpArray").ok_or("missing parameter slots".into())
    }
    let Some(Value::Object(Some(basic))) = slots(&bow)?.first() else { return Err("missing basic bow row".into()) };
    let index = match bow.get(basic, "cameraNo") {
        Some([Value::S(n)]) => usize::try_from(*n).ok(),
        Some([Value::U(n)]) => usize::try_from(*n).ok(),
        _ => None,
    }.ok_or("basic bow has no enabled camera".to_string())?;
    let Some(Value::Object(Some(record))) = slots(&camera)?.get(index) else { return Err("missing bow camera slot".into()) };
    if camera.class_of(record).name() != Some("cCamExParamTps") { return Err("bow camera is not TPS".into()); }
    let number = |name| match camera.get(record, name) {
        Some([Value::F(v)]) if v.len() == 1 => Ok(v[0] as f32),
        _ => Err(format!("missing camera {name}")),
    };
    let integer = |name| match camera.get(record, name) {
        Some([Value::U(n)]) => Ok(*n as u32),
        Some([Value::S(n)]) => Ok(*n as u32),
        _ => Err(format!("missing camera {name}")),
    };
    if number("mOffsetZ")? != 0. { return Err("actor-axis TPS offset not supported".into()); }
    Ok(AimCamera { normal: mtcamera::params::Normal {
        distance: number("mDistance")?, height: number("mHeight")?, fov_degrees: number("mFov")?,
        camera_target: integer("mCameraTarget")?, screen_adjust_type: integer("mScrAdjustType")?,
        screen_adjust_flags: integer("mScrAdjustFlag")?, screen_adjust_height: number("mScrAdjustHeight")?,
    }, shoulder: number("mOffsetX")?, pitch_min: -number("mAngLimitXMax")?.to_radians(),
        pitch_max: -number("mAngLimitXMin")?.to_radians() })
}

impl AimCamera {
    /// Stationary TPS desired eye/target, translated from 0x007CDF70
    /// (2017-12-13 Steam). Viewer yaw = native yaw - pi; pitch reverses.
    /// Base male actor has model scale 1. Actor-height morph correction,
    /// authored camera entry interpolation and recoil remain separate.
    pub fn pose(&self, feet: Vec3, yaw: f32, pitch: f32) -> (Vec3, Vec3) {
        let pitch = pitch.clamp(self.pitch_min, self.pitch_max);
        let target = feet + Vec3::Y * self.normal.height
            + Vec3::new(-yaw.cos(), 0., yaw.sin()) * self.shoulder;
        let direction = Vec3::new(yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos());
        (target - direction * self.normal.distance, target)
    }
}

pub fn aim_range(vfs: &ddvfs::Vfs) -> Result<f32, String> {
    use mtxfs::Value;
    let bytes = vfs.read(ddhost::ddenemyrt::arrow::BOW_PARAM_PATH, mtarc::type_hash("rBowActParamList"))?;
    let x = mtxfs::parse(&bytes).map_err(|e| e.to_string())?;
    let object = |o: &mtxfs::Object, name| match x.get(o, name).and_then(|v| v.first()) {
        Some(Value::Object(Some(o))) => Ok((**o).clone()),
        _ => Err(format!("bow missing {name}")),
    };
    let array = object(&x.root, "mParamList")?;
    let basic = object(&array, "mpArray")?;
    // rBowActParam +0x80, read by B1D010 / B16FE0. Reflected property name
    // is verified against the installed XFS, never a hardcoded shot distance.
    match x.get(&basic, "checkLength").and_then(|v| v.first()) {
        Some(Value::F(v)) if v[0].is_finite() && v[0] > 0. => Ok(v[0] as f32),
        _ => Err("bow missing positive checkLength".into()),
    }
}

/// Translated from the unlocked-camera ray branch of 0x00B1D010
/// (DDDA.exe 2017-12-13 Steam): advance the start to the bow's camera
/// distance, end at the installed aim range, and use a world ray hit.
/// uAimCheck's actor sensor/lock-on path is not implemented by this adapter.
pub fn target(ground: &dyn Ground, eye: Vec3, looking: Vec3, bow_origin: Vec3, range: f32) -> Option<Vec3> {
    let direction = (looking - eye).try_normalize()?;
    let start = eye + direction * eye.distance(bow_origin);
    let end = eye + direction * range;
    if start.distance_squared(end) < 0.01 { return Some(end); }
    Some(ground.ray(start.to_array(), end.to_array()).map_or(end, |hit| Vec3::from(hit.point)))
}

#[derive(Default)]
pub struct Aiming {
    pub controls: Controls,
    pub camera: Option<AimCamera>,
    pub range: Option<f32>,
    random: HashMap<PlayerId, ddpawn::rift::XorShift128>,
    models: HashMap<PlayerId, BowModel>,
    /// Explicit unattended screenshot fixture only, never ordinary input.
    capture: Option<Capture>,
}
struct BowModel { entity: Entity, mount: ddlook::weapon::WeaponPart }
#[derive(Default)]
struct Capture { shot: bool, frames: u32, frozen: bool }
impl Aiming {
    pub fn load(vfs: &ddvfs::Vfs) -> Self {
        let camera = aim_camera(vfs).map_err(|e| warn!("bow camera: {e}")).ok();
        let range = aim_range(vfs).map_err(|e| warn!("bow aim: {e}")).ok();
        Self { controls: Controls::load(), camera, range, random: HashMap::new(), models: HashMap::new(),
            capture: std::env::var_os("DDVIEW_BOW_CAPTURE").map(|_| Capture::default()) }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_models(&mut self, commands: &mut Commands, loader: &mut crate::stage::Loader,
        meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>,
        bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>, fight: &Fight) -> Result<(), String> {
        for (&seat, actor) in &fight.arisen {
            if actor.kit != ddhost::ddenemyrt::fight::Kit::Strider { continue; }
            // First-choice Strider secondary item, 0x00673740, same item as
            // BowData's fresh kit valuation; model and mount read by ddlook.
            let mount = ddlook::save::weapons_for_items(loader.vfs, -1, 0x1F8).map_err(|e| e.to_string())?
                .into_iter().next().ok_or("missing shortbow model")?;
            let entity = crate::character::spawn(commands, loader, meshes, materials, images, bindposes,
                &mount.path, r"motion\wp\bow\bow", 10, Vec3::ZERO, Some(0.))?;
            self.models.insert(seat, BowModel { entity, mount });
        }
        Ok(())
    }
    /// Supplies source pose, camera ray, fresh kit secondary valuation and
    /// an explicit host stream before Fight consumes queued shot events.
    /// The per-seat seed is a port session choice; the original stream is global.
    pub fn prepare(&mut self, sim: &Sim, fight: &mut Fight, ground: &dyn Ground,
        views: &[(PlayerId, Vec3, Vec3)]) {
        use ddcombat::volumes::Pose;
        use ddpawn::rift::PawnRng;
        for &(seat, eye, looking) in views {
            let Some(player) = sim.player(seat).filter(|p| p.bow.is_some()) else { continue };
            let Ok(pose) = posed_bow(fight, sim, seat) else { continue };
            let Some(matrix) = pose.joint_matrix(0) else { continue };
            let Some(range) = self.range else { continue };
            let Some(target) = target(ground, eye, looking, Vec3::from_slice(&matrix[3][..3]), range) else { continue };
            let rng = self.random.entry(seat).or_insert_with(|| ddpawn::rift::XorShift128::seeded(u32::from(seat)));
            // Advance only for queued rows, not every rendered frame. Empty
            // initial context is still retained for post-launch valuation.
            let words = player.bow_shots.len() * 4;
            let random = (0..words).map(|_| rng.next_u32()).collect();
            fight.bow_contexts.insert(seat, BowContext { pose, target: target.to_array(), random,
                augments: [u32::MAX; 6], // fresh --kit has no equipped augments
                secondary_attacker: fight.data.bow.starting_attacker });
        }
    }
}

/// Diagnostic chooses a viewport ray toward the first --goblin actor,
/// then drives ordinary held-bow/light input. It does not inject a muzzle,
/// arrow, victim HP, random words or completion. Stop after a real launch
/// so GPU shader warmup cannot miss the brief projectile-flight interval.
pub fn capture_input(st: &mut crate::play::PlayState, inputs: &mut HashMap<PlayerId, ddsim::Input>) {
    use ddcombat::volumes::Volume;
    let Some(capture) = &mut st.bow_view.capture else { return };
    let Some(player) = st.sim.player(st.follow) else { return };
    let Some(fight) = &st.fight else { return };
    let Some(goblin) = fight.goblins.first() else { return };
    let Some(hurt) = goblin.fighter.hurts.first() else { return };
    let center = match hurt.volume { Volume::Sphere { c, .. } => Vec3::from(c), Volume::Capsule { a,b,.. } => (Vec3::from(a)+Vec3::from(b))*0.5 };
    let Some(camera) = &st.bow_view.camera else { return };
    let feet = Vec3::from(player.position);
    let delta = center-feet;
    let distance = delta.x.hypot(delta.z);
    if distance > camera.shoulder.abs() {
        st.cam_yaw = delta.x.atan2(delta.z) + (camera.shoulder/distance).asin();
        let (_, flat_target) = camera.pose(feet,st.cam_yaw,0.);
        let aim = center-flat_target;
        st.cam_pitch = (-aim.y).atan2(aim.x.hypot(aim.z));
    }
    capture.frames += 1;
    let shoot = !capture.shot && capture.frames >= 30
        && player.bow.is_some_and(|b| b.phase == ddsim::BowPhase::Ready);
    capture.shot |= shoot;
    inputs.insert(st.follow, ddsim::Input { bow: true, light: shoot, camera_yaw: st.cam_yaw, ..Default::default() });
}
pub fn freeze_capture(st: &mut crate::play::PlayState) -> bool {
    let Some(capture) = &mut st.bow_view.capture else { return false };
    if !capture.frozen && st.fight.as_ref().is_some_and(|fight| fight.arrows.iter().any(|a| a.distance > 0.)) {
        capture.frozen = true;
        info!("S27 bow capture: real input shot, bank9 phase {:?}, live arrows={}; holding source snapshot for screenshot",
            st.sim.player(st.follow).and_then(|p| p.bow).map(|b| b.phase), st.fight.as_ref().map_or(0, |f| f.arrows.len()));
    }
    capture.frozen
}
pub fn capture_frozen(st: &crate::play::PlayState) -> bool { st.bow_view.capture.as_ref().is_some_and(|capture| capture.frozen) }

/// Stationary body layers before presentation interpolation. 0x01061DD0's
/// keyed-channel priority is represented by sampling lower0 then upper1;
/// B16FE0 / B1C7D0 / B1CAC0 request independent motion clocks.
fn layered_motion(lower: &mtlmt::Motion, lower_frame: f32, upper: &mtlmt::Motion, upper_frame: f32) -> mtlmt::Motion {
    let mut motion = lower.clone();
    motion.tracks = lower.tracks.iter().map(|track| (track, lower_frame))
        .chain(upper.tracks.iter().map(|track| (track, upper_frame)))
        .map(|(track, frame)| { let mut sampled = track.clone(); sampled.reference = track.sample(frame); sampled.keys.clear(); sampled }).collect();
    motion
}
pub fn posed_bow(fight: &Fight, sim: &Sim, seat: PlayerId) -> Result<ddhost::ddenemyrt::body::Posed, String> {
    use ddhost::ddenemyrt::body::{MotionId, Posed, model_matrix};
    let player = sim.player(seat).ok_or("missing bow player")?;
    let bow = player.bow.ok_or("no bow phase")?;
    let data = &fight.data.bow;
    let list = &data.player_bank.list;
    let motion = |id: u16| list.motions.get(usize::from(id & 255)).and_then(Option::as_ref).ok_or("missing bow motion");
    let layered = layered_motion(motion(bow.lower.motion)?, bow.lower.frame, motion(bow.upper.motion)?, bow.upper.frame);
    let body = &fight.kit(fight.arisen.get(&seat).ok_or("missing bow fighter")?.kit).body;
    let matrices = body.skeleton.pose(Some(&layered), 0.);
    let index = body.skeleton.index_of(0x99).ok_or("missing drawn joint0x99")?;
    let model = model_matrix(player.position, player.yaw);
    let mut anchor = mtlmt::pose::mul(&matrices[index], &model);
    // Drawn secondary mount reset normalizes axes, 859B10 / 859BB0.
    for axis in anchor.iter_mut().take(3) {
        let length = axis.iter().map(|v| v*v).sum::<f32>().sqrt();
        if length > f32::EPSILON { for value in axis { *value /= length; } }
    }
    let slot = match bow.phase {
        ddsim::BowPhase::Draw => 10,
        ddsim::BowPhase::Ready => data.wait_weapon,
        ddsim::BowPhase::Shot if player.bow_shot.wait.is_some() => data.wait_weapon,
        ddsim::BowPhase::Shot => data.shot_weapon,
        ddsim::BowPhase::Released => 13,
        ddsim::BowPhase::Change => return Err("unsupported bow change".into()),
    };
    let id = MotionId::new(0, slot.into());
    if data.weapon_body.motion(id).is_none() { return Err("missing bow model motion".into()); }
    let weapon = data.weapon_body.pose(id, bow.upper.frame, [0.;3], 0.);
    Ok(Posed { model: anchor, joints: weapon.joints.into_iter().map(|(id, joint)| (id, mtlmt::pose::mul(&joint, &anchor))).collect() })
}

/// Pose the real bow model, including its own string/limb motion. Idle
/// bows use the source sheathed mount; bow mode uses drawn joint0x99.
pub fn show_models(st: &crate::play::PlayState,
    roots: &mut Query<(&mut Transform, &mut crate::character::Playback), Without<crate::play::FollowCam>>) {
    use ddcombat::volumes::Pose;
    let Some(fight) = &st.fight else { return };
    for (&seat, visual) in &st.bow_view.models {
        let Some(player) = st.sim.player(seat) else { continue };
        let Ok((mut root, mut playback)) = roots.get_mut(visual.entity) else { continue };
        if let Some(bow) = player.bow {
            let Ok(pose) = posed_bow(fight, &st.sim, seat) else { continue };
            let slot = match bow.phase {
                ddsim::BowPhase::Draw => 10,
                ddsim::BowPhase::Ready => fight.data.bow.wait_weapon,
                ddsim::BowPhase::Shot if player.bow_shot.wait.is_some() => fight.data.bow.wait_weapon,
                ddsim::BowPhase::Shot => fight.data.bow.shot_weapon,
                ddsim::BowPhase::Released => 13,
                ddsim::BowPhase::Change => continue,
            };
            if let Some(motion) = fight.data.bow.weapon_body.motion(ddhost::ddenemyrt::body::MotionId::new(0, slot.into())) {
                // The weapon's layer is separate from both body layers.
                playback.motion = motion.clone(); playback.hold_frame = Some(bow.upper.frame);
            }
            set_root(&mut root, pose.model, st.origin);
        } else {
            let actor = &fight.arisen[&seat];
            let id = ddhost::ddenemyrt::body::MotionId::new((player.state.motion_id() >> 8) as u8, player.state.motion_id() & 255);
            let pose = fight.kit(actor.kit).body.pose(id, player.frame, player.position, player.yaw);
            let Some(anchor) = pose.joint_matrix(i32::from(visual.mount.joint)) else { continue };
            let r = visual.mount.rotation.map(f32::to_radians);
            let local = Transform::from_translation(Vec3::from(visual.mount.offset))
                .with_rotation(Quat::from_euler(EulerRot::ZYX, r[2], r[1], r[0])).to_matrix();
            let anchor = Mat4::from_cols_array_2d(&anchor);
            let mut transform = Transform::from_matrix(anchor * local);
            transform.translation = (transform.translation - st.origin) * crate::stage::UNIT;
            transform.scale *= crate::stage::UNIT;
            *root = transform;
            playback.hold_frame = Some(0.);
        }
    }
}
fn set_root(root: &mut Transform, matrix: ddcombat::volumes::Mat4, origin: Vec3) {
    *root = Transform::from_matrix(Mat4::from_cols_array_2d(&matrix));
    root.translation = (root.translation - origin) * crate::stage::UNIT;
    root.scale *= crate::stage::UNIT;
}

/// Flight markers are a viewer presentation proxy, not a port of the bow
/// particle list. Each line is the authoritative previous/current segment;
/// the tip sphere uses its installed collision radius, not invented speed.
pub fn draw_arrows(state: Option<Res<crate::play::PlayState>>, mut gizmos: Gizmos) {
    let Some(st) = state else { return };
    let Some(fight) = &st.fight else { return };
    for arrow in &fight.arrows {
        let point = |p: [f32;3]| (Vec3::from(p) - st.origin) * crate::stage::UNIT;
        let end = point(arrow.position);
        gizmos.line(point(arrow.previous_position), end, Color::srgb(1.,0.9,0.5));
        if let Some(radius) = arrow.volumes(&fight.data.arrow).first().map(|v| match v.volume {
            ddcombat::volumes::Volume::Sphere { r, .. } | ddcombat::volumes::Volume::Capsule { r, .. } => r,
        }) {
            gizmos.sphere(Isometry3d::from_translation(end), radius * crate::stage::UNIT, Color::srgb(1.,0.9,0.5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_inputs_and_second_seat_are_independent() {
        let c = Controls::parse("STG_TOGGLE_BOW_MODE=16\nSTG_SUB_WEP=56\nSTG_ATTACK_0=256\n");
        let mut keys = ButtonInput::default(); let mut mouse = ButtonInput::default();
        keys.press(KeyCode::KeyQ); mouse.press(MouseButton::Left);
        let mut first = ddsim::Input::default(); let mut second = first;
        c.apply(0, &mut first, &keys, &mouse, false); c.apply(1, &mut second, &keys, &mouse, false);
        assert!(first.bow_toggle && first.light && !first.bow);
        assert!(!second.bow_toggle && !second.light && !second.bow);
        assert_eq!(binding(0), None); assert_eq!(binding(u32::MAX), None);
    }
    #[test]
    fn ray_starts_at_bow_depth_and_uses_world_hit_or_authored_range() {
        struct World;
        impl Ground for World {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
            fn ray(&self, from: [f32;3], to: [f32;3]) -> Option<ddsim::RayHit> {
                assert_eq!(from, [0.,0.,50.]); assert_eq!(to, [0.,0.,200.]);
                Some(ddsim::RayHit { point: [0.,0.,125.], normal: [0.,0.,-1.] })
            }
        }
        assert_eq!(target(&World, Vec3::ZERO, Vec3::Z, Vec3::new(30.,0.,40.), 200.), Some(Vec3::new(0.,0.,125.)));
    }
    #[test]
    fn installed_basic_bow_requests_tps_and_uses_its_authored_camera() {
        let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !native.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let camera = aim_camera(&vfs).unwrap();
        assert!(aim_range(&vfs).unwrap() > camera.normal.distance);
        let (eye, target) = camera.pose(Vec3::ZERO, 0., 0.);
        assert!((target.x + camera.shoulder).abs() < 1e-5);
        assert!((target.y - camera.normal.height).abs() < 1e-5);
        assert!((eye.z + camera.normal.distance).abs() < 1e-5);
        let (eye, target) = camera.pose(Vec3::new(100.,200.,300.), std::f32::consts::FRAC_PI_2, 0.);
        assert!((target.z - 300. - camera.shoulder).abs() < 1e-4);
        assert!((target.x - eye.x - camera.normal.distance).abs() < 1e-4);
    }
    #[test]
    fn installed_viewer_keys_camera_pose_and_rng_launch_an_arrow_that_hits() {
        use ddcombat::volumes::{Pose, Volume};
        let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !native.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let data = crate::play::fight_data(&vfs, native).unwrap();
        struct Flat;
        impl Ground for Flat { fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.) } }
        let mut sim = Sim::new(Default::default()); let mut fight = Fight::new(data);
        for seat in [3,7] { sim.add_player(seat, [f32::from(seat-3)*300.,0.,0.],0.); fight.add_arisen_as(seat, ddhost::ddenemyrt::fight::Kit::Strider); fight.equip(&mut sim, seat); }
        fight.add_goblin([0.,0.,1000.],std::f32::consts::PI); fight.volumes(&sim);
        let mut adapter = Aiming::load(&vfs);
        adapter.controls = Controls::parse("STG_TOGGLE_BOW_MODE=16\nSTG_SUB_WEP=56\nSTG_ATTACK_0=256");
        let mut keys = ButtonInput::default(); let mut mouse = ButtonInput::default();
        let mut launched = false; let mut hit = false;
        for frame in 0..150 {
            if frame == 0 { keys.press(KeyCode::KeyQ); } else { keys.release(KeyCode::KeyQ); keys.clear(); }
            if frame == 90 { mouse.press(MouseButton::Left); } else { mouse.release(MouseButton::Left); mouse.clear(); }
            let mut input = ddsim::Input::default();
            adapter.controls.apply(0, &mut input, &keys, &mouse, sim.player(3).unwrap().bow.is_some());
            fight.hits(1.);
            hit |= fight.events.iter().any(|e| e.victim == fight.goblins[0].fighter.owner && e.total > 0.);
            sim.step(1./30., &[(3,input)].into(), &Flat);
            if sim.player(3).unwrap().bow.is_some() {
                let center = match fight.goblins[0].fighter.hurts[0].volume { Volume::Sphere { c, .. } => Vec3::from(c), _ => panic!() };
                let camera = adapter.camera.as_ref().unwrap();
                let feet = Vec3::from(sim.player(3).unwrap().position);
                let delta = center-feet;
                // This test deliberately chooses the viewport's orbit to
                // point its ray at the installed hurt centre. Production
                // does not choose this target or a nearest enemy.
                let yaw = delta.x.atan2(delta.z) + (camera.shoulder/delta.x.hypot(delta.z)).asin();
                let (_, flat_target) = camera.pose(feet,yaw,0.);
                let d = center-flat_target;
                let pitch = (-d.y).atan2(d.x.hypot(d.z));
                let (eye,looking) = camera.pose(feet,yaw,pitch);
                adapter.prepare(&sim,&mut fight,&Flat,&[(3,eye,looking)]);
                assert!(!fight.bow_contexts.contains_key(&7));
            }
            fight.players_moved(&mut sim, &Flat, 1.); fight.volumes(&sim);
            if let Some(arrow) = fight.arrows.first() {
                launched = true;
                assert_eq!(arrow.shooter,3);
                let muzzle = fight.bow_contexts[&3].pose.joint_matrix(0).unwrap()[3];
                assert!(Vec3::from_slice(&muzzle[..3]).distance(Vec3::from(sim.player(3).unwrap().position)) > 10.);
            }
            if hit { break; }
        }
        assert!(launched, "viewer input did not launch; errors={:?}",fight.bow_errors);
        assert!(hit,"camera-directed arrow did not hit");
    }
}
