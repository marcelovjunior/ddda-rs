//! A skinned character (MOD with bones) playing one motion (LMT slot).
//!
//! Conventions (Riftstone `docs/animation.md`, measured on the games' data
//! and DDDA.exe's joint routine 0x01051B60): matrices are stored row-major
//! for row vectors, world = local x parent world, local = scale, rotation,
//! translation. Read with `Mat4::from_cols_array`, a stored matrix becomes
//! the column-vector matrix Bevy uses. Quaternions are the same rotation in
//! both conventions.
//!
//! Tracks address joints by joint id (`Bone::anim_map_index`, the first
//! byte of a bone record); 255 is the root/scene track (root motion, not
//! applied here). A keyed channel replaces the joint's rest value; unkeyed
//! channels keep it (*assumed*, see Riftstone's UNKNOWNs).

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use crate::stage::{DdSlot, Loader, PartMaterial, PendingDd, RMATERIAL, RMODEL, UNIT};

pub const RMOTIONLIST: u32 = 0x7682_0D81;

/// Frames per second of motion data. *Assumed* 30 until traced in the exe.
pub const MOTION_FPS: f32 = 30.0;

#[derive(Default)]
pub struct CameraFadeCache {
    thresholds: Option<Option<[f32; 7]>>,
    actors: HashMap<(Entity, Entity), mtcamera::fade::NearFade>,
    dd: HashMap<Entity, (Handle<crate::stage::DdMaterial>, (f32, AlphaMode, Option<u8>, u32))>,
    standard: HashMap<Entity, (Handle<StandardMaterial>, f32, AlphaMode)>,
}

/// Near-camera model opacity from 0x0088bdc0; run after joint propagation.
/// Each process currently renders one followed view. State is keyed by both
/// view and actor; multiple simultaneous views need per-view draw materials.
/// This implements the ordinary distance path, not aiming-mode pawn fading,
/// special-camera horizontal-only distance, or the original global bypasses.
pub fn camera_fade(
    time: Res<Time>, args: Res<crate::Args>,
    state: Option<Res<crate::play::PlayState>>,
    stream: Option<Res<crate::stream::Streamer>>,
    pause: Res<crate::pause::PauseState>,
    cameras: Query<(Entity, &Camera, &GlobalTransform), With<crate::play::FollowCam>>,
    actors: Query<&Playback>, transforms: Query<&GlobalTransform>,
    mut meshes: Query<(Entity, &ChildOf, Option<&mut MeshMaterial3d<crate::stage::DdMaterial>>, Option<&mut MeshMaterial3d<StandardMaterial>>)>,
    mut dd: ResMut<Assets<crate::stage::DdMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<CameraFadeCache>,
) {
    let Some(state) = state else { return };
    let Ok((view, camera, eye)) = cameras.single() else { return };
    if cache.thresholds.is_none() {
        let read = (|| -> Result<[f32; 7], ddexe::Error> {
            let file = ddexe::ExeFile::next_to(&args.native_pc)?;
            let exe = file.image()?;
            // 0x0088bdc0 selects these globals; never embed their table.
            Ok(exe.f32s(0x0187_5de4, 7)?.try_into().unwrap())
        })();
        if let Err(e) = &read { warn!("camera fade disabled: {e}"); }
        cache.thresholds = Some(read.ok());
    }
    let Some(thresholds) = cache.thresholds.flatten() else { return };
    let stage = stream.as_ref().map_or(args.stage, |s| s.stage);
    let frames = if pause.paused { 0.0 } else { time.delta_secs().min(0.1) * MOTION_FPS };
    let mut opacity = HashMap::new();
    for (&seat, &root) in &state.entities {
        let Ok(actor) = actors.get(root) else { continue };
        // uModel::vf22 0x00ff9a20 maps semantic bone ID 1, falling back
        // to the model matrix if missing. These are current morphed joints.
        let joint = actor.joints.get(&1).copied().unwrap_or(root);
        let Ok(position) = transforms.get(joint) else { continue };
        let delta = (eye.translation() - position.translation()) / UNIT;
        let (inner, outer) = if seat >= 4 {
            (thresholds[4], thresholds[3])
        } else if stage == 100 {
            (thresholds[2], 90.0) // Immediate in 0x0088bdc0.
        } else { (thresholds[1], thresholds[0]) };
        let alpha = cache.actors.entry((view, root)).or_default().step(
            delta.length_squared(), delta.x * delta.x + delta.z * delta.z,
            inner, outer, frames, !camera.is_active,
        );
        opacity.insert(root, alpha);
    }
    cache.actors.retain(|&(v, root), _| v == view && opacity.contains_key(&root));
    for (mesh, parent, dd_handle, standard_handle) in &mut meshes {
        let Some(&alpha) = opacity.get(&parent.parent()) else { continue };
        if let Some(mut handle) = dd_handle {
            // Clone only when fading first becomes necessary. A shared stage
            // or another actor's material must never inherit this opacity.
            if alpha < 1.0 && !cache.dd.contains_key(&mesh) {
                if let Some(material) = dd.get(&handle.0).cloned() {
                    let authored = (material.base.base_color.alpha(), material.base.alpha_mode,
                        material.extension.depth, material.extension.params.flags);
                    let own = dd.add(material);
                    handle.0 = own.clone();
                    cache.dd.insert(mesh, (own, authored));
                }
            }
            if let Some((own, authored)) = cache.dd.get(&mesh) {
                let wanted = authored.0 * alpha.min(1.0);
                if dd.get(own).is_some_and(|m| m.base.base_color.alpha() != wanted) {
                    if let Some(mut material) = dd.get_mut(own) {
                        crate::stage::character_opacity(&mut material, alpha, *authored);
                    }
                }
            }
        }
        if let Some(mut handle) = standard_handle {
            if alpha < 1.0 && !cache.standard.contains_key(&mesh) {
                if let Some(material) = standard.get(&handle.0).cloned() {
                    let authored = (material.base_color.alpha(), material.alpha_mode);
                    let own = standard.add(material);
                    handle.0 = own.clone();
                    cache.standard.insert(mesh, (own, authored.0, authored.1));
                }
            }
            if let Some((own, original_alpha, original_mode)) = cache.standard.get(&mesh) {
                let wanted = original_alpha * alpha.min(1.0);
                if standard.get(own).is_some_and(|m| m.base_color.alpha() != wanted) {
                    if let Some(mut material) = standard.get_mut(own) {
                        material.base_color.set_alpha(wanted);
                        material.alpha_mode = if alpha < 1.0 { AlphaMode::Blend } else { *original_mode };
                    }
                }
            }
        }
    }
    cache.dd.retain(|mesh, _| meshes.get(*mesh).is_ok());
    cache.standard.retain(|mesh, _| meshes.get(*mesh).is_ok());
}

#[derive(Component)]
pub struct Playback {
    pub motion: mtlmt::Motion,
    /// Joint entity by joint id.
    pub joints: HashMap<u8, Entity>,
    /// Fixed frame to hold (screenshots); `None` plays in a loop.
    pub hold_frame: Option<f32>,
    /// Diagnostic context only: the event timeline differs from motion time.
    pub event_frame: Option<f32>,
    /// Set with `motion` when the motion changes: interpolation frames of
    /// the change. `animate` freezes the shown pose and fades it out over
    /// the new motion (`mtlmt::blend`, docs/ANIMATION.md).
    pub blend_request: Option<f32>,
    /// The fade in progress: the frozen local pose (rotation, translation,
    /// scale) by joint entity.
    pub blend: Option<(mtlmt::blend::Interpolation, Vec<(Entity, Quat, Vec3, Vec3)>)>,
    /// Body morph (ddlook::morph), applied after the motion by
    /// [`apply_morph`].
    pub morph: Option<Morph>,
    /// Weapons drawn: each weapon with a hand joint hangs there instead of
    /// on its sheathed row (0x0080be30's switch).
    pub drawn: bool,
}

/// Original local transform, before motion and character-edit constraints.
/// Added only when the joint diagnostic is enabled; shared part joints keep
/// the rest transform and parent of the skeleton that first created them.
#[derive(Component)]
pub struct JointRest {
    id: u8,
    parent_id: Option<u8>,
    local: Transform,
}

#[derive(Default)]
pub struct JointDump {
    settings: Option<(Option<f32>, Option<f32>)>,
    done: HashSet<Entity>,
}

fn diagnostic_frame(name: &str) -> Option<f32> {
    std::env::var(name).ok()?.parse::<f32>().ok().filter(|f| f.is_finite())
}

fn joint_delta(rest: &Transform, animated: &Transform) -> (f32, f32) {
    // abs(dot) treats q and -q as the same rotation. Distances are already
    // in model cm (the UNIT conversion sits on the actor root).
    let cosine = rest.rotation.normalize().dot(animated.rotation.normalize()).abs().clamp(0.0, 1.0);
    (2.0 * cosine.acos().to_degrees(), rest.translation.distance(animated.translation))
}

#[allow(clippy::too_many_arguments)]
fn dump_joints(
    actor: Entity, model: Option<&Name>, cutscene: Option<&crate::event::Actor>,
    playback: &Playback, frame: f32, state: &mut JointDump,
    rest: &Query<&JointRest>, joints: &Query<&mut Transform>,
) {
    let Some(cutscene) = cutscene else { return };
    let (motion_frame, event_frame) = *state.settings.get_or_insert_with(|| (
        diagnostic_frame("DDVIEW_JOINT_DUMP"), diagnostic_frame("DDVIEW_JOINT_DUMP_EVENT_FRAME"),
    ));
    let matches = motion_frame.is_some_and(|wanted| (wanted - frame).abs() < 0.5)
        || event_frame.zip(playback.event_frame).is_some_and(|(wanted, actual)| (wanted - actual).abs() < 0.5);
    if !matches || !state.done.insert(actor) { return; }
    let mut ids: Vec<_> = playback.joints.iter().collect();
    ids.sort_unstable_by_key(|(id, _)| **id);
    for (_, &entity) in ids {
        let (Ok(original), Ok(animated)) = (rest.get(entity), joints.get(entity)) else { continue };
        let (angle, moved) = joint_delta(&original.local, animated);
        let xyz_degrees = |q: Quat| {
            let (x, y, z) = q.normalize().to_euler(EulerRot::XYZ);
            [x.to_degrees(), y.to_degrees(), z.to_degrees()]
        };
        info!(
            "JOINT{} actor={} model={} event_frame={:?} motion_frame={frame:.3} id={} parent={:?} rest_t_cm={:?} rest_r_xyz_deg={:?} rest_s={:?} animated_t_cm={:?} animated_r_xyz_deg={:?} animated_s={:?} angle_deg={angle:.3} move_cm={moved:.3}",
            if angle > 45.0 || moved > 3.0 { "!!" } else { "" },
            cutscene.name, model.map_or("<unnamed>", Name::as_str), playback.event_frame, original.id,
            original.parent_id, original.local.translation.to_array(), xyz_degrees(original.local.rotation), original.local.scale.to_array(),
            animated.translation.to_array(), xyz_degrees(animated.rotation), animated.scale.to_array(),
        );
    }
}

/// A rigid attachment (a weapon): its anchor under the root, the sheathed
/// placement (bone index, uCnsAttach matrix) and the drawn bone (the
/// weapon joint 0x96 / 0x99, no offset), if it has one.
#[derive(Clone, Copy)]
pub struct Attach {
    pub anchor: Entity,
    pub bone: usize,
    pub local: Mat4,
    pub hand: Option<usize>,
}

/// The base skeleton with its body morph.
pub struct Morph {
    /// Base-model bones in file order (parents first): entity and parent
    /// index.
    pub bones: Vec<(Entity, Option<usize>)>,
    /// Per bone: its uCnsEdit records in order (body, then face).
    pub joint: Vec<Vec<Edit>>,
    /// uModel +0x60.
    pub model_scale: Vec3,
    /// The unmorphed local transforms of the last frame: the motion only
    /// writes the channels it keys, so the rest must come from here.
    pub base: Vec<Transform>,
    /// Rigid attachments (weapons).
    pub attach: Vec<Attach>,
}

/// One uCnsEdit record (0x00860ac0).
#[derive(Clone, Copy)]
pub struct Edit {
    /// Semantic model joint ID, used by type-0xb's joint-2 exception.
    pub joint_id: u8,
    /// Bone index of the semantic eye/ear pivot (0x00812330 +0xac).
    pub pivot: Option<usize>,
    /// +0xa4: 0 plain, 0xb absolute size (also takes `child`), 9 human
    /// hand absolute size; 0xc remains approximated as plain.
    pub kind: u32,
    pub scale: Vec3,
    pub child: Vec3,
    pub offset: Vec3,
    /// +0x70 as a rotation (type 0: X, then Y, then Z).
    pub rotation: Quat,
}

/// The morph pass (uCnsEdit::vf24 0x00860ac0 on world matrices, parents
/// first, so children follow their morphed parents): world = parent world
/// x local, then `world x Translate(offset) x Scale(k)` (column vectors).
/// k is the scale for type 0; type 9 human hands cancel inherited axis
/// lengths (0x00867b70, +0xb0 set by 0x0081c330). Type 0xb uses child scale
/// and axis-length compensation (0x0086d180), except semantic joint 2
/// compensates only Y and retains model scale there. The model scale is the
/// skeleton's root. The result goes back into the local transforms (shear
/// from non-uniform parent scales is dropped by the TRS decomposition).
fn apply_morph(m: &mut Morph, drawn: bool, joints: &mut Query<&mut Transform>) {
    let n = m.bones.len();
    let root = Mat4::from_scale(m.model_scale);
    let mut world: Vec<Mat4> = Vec::with_capacity(n);
    for i in 0..n {
        let (e, parent) = m.bones[i];
        let local = joints.get(e).copied().unwrap_or_default();
        m.base[i] = local;
        let pw = parent.map_or(root, |p| world[p]);
        let mut w = pw * local.to_matrix();
        for e in &m.joint[i] {
            if matches!(e.kind, 0xd..=0x10) {
                let (z, y, x) = e.rotation.to_euler(EulerRot::ZYX);
                let pivot = e.pivot.and_then(|p| world.get(p)).map(Mat4::to_cols_array);
                if let Some(corrected) = ddlook::face_constraint::apply(
                    e.kind, w.to_cols_array(), pw.to_cols_array(), pivot,
                    e.offset.to_array(), e.scale.to_array(), [x, y, z],
                ) {
                    w = Mat4::from_cols_array(&corrected);
                }
                continue;
            }
            let k = if e.kind == 0xb {
                Vec3::from_array(ddlook::morph::human_absolute_axis_factors(
                    e.joint_id, e.scale.to_array(), e.child.to_array(), m.model_scale.to_array(), [
                        w.x_axis.truncate().length(), w.y_axis.truncate().length(), w.z_axis.truncate().length(),
                    ],
                ))
            } else if e.kind == 9 {
                Vec3::from_array(ddlook::morph::hand_axis_factors(e.scale.to_array(), [
                    w.x_axis.truncate().length(), w.y_axis.truncate().length(), w.z_axis.truncate().length(),
                ]))
            } else {
                e.scale
            };
            // Row-vector C = diag(k) x R with the offset row, W' = C x W:
            // column form W x T(offset) x R x S(k).
            w = w * Mat4::from_translation(e.offset) * Mat4::from_quat(e.rotation) * Mat4::from_scale(k);
        }
        world.push(w);
    }
    // Only bones under an edit are written back: the TRS decomposition
    // would garble the others (mirrored bones, shear) for nothing.
    let mut dirty = vec![false; n];
    for i in 0..n {
        let (e, parent) = m.bones[i];
        dirty[i] = !m.joint[i].is_empty() || parent.map_or(m.model_scale != Vec3::ONE, |p| dirty[p]);
        if !dirty[i] {
            continue;
        }
        // Root bones hang under the entity root, which has no model scale:
        // their local is the world matrix itself.
        let local = parent.map_or(world[i], |p| world[p].inverse() * world[i]);
        if let Ok(mut tr) = joints.get_mut(e) {
            *tr = Transform::from_matrix(local);
        }
    }
    // uCnsAttach::vf24 (0x00859bb0, +0xac clear): the joint's world rows
    // are normalised before the attach matrix goes on them, so weapons
    // keep their size and offset whatever the joint's scale.
    // Drawn (0x00c1b460): the weapon joint with the constraint reset to no
    // rotation, no offset, scale 1 (0x00859b10).
    for at in &m.attach {
        let (i, local) = match (drawn, at.hand) {
            (true, Some(h)) => (h, Mat4::IDENTITY),
            _ => (at.bone, at.local),
        };
        let a = at.anchor;
        let w = world[i];
        let unit = |v: Vec4| {
            let l = v.length();
            if l > 1.192_092_9e-7 { v / l } else { v }
        };
        let n = Mat4::from_cols(unit(w.x_axis), unit(w.y_axis), unit(w.z_axis), w.w_axis);
        if let Ok(mut tr) = joints.get_mut(a) {
            *tr = Transform::from_matrix(n * local);
        }
    }
}

impl Playback {
    /// Shows `motion` from now on, blending from the current pose over
    /// `frames` motion frames (0 snaps).
    pub fn change_motion(&mut self, motion: mtlmt::Motion, frames: f32) {
        self.motion = motion;
        self.blend_request = Some(frames);
    }
}

/// One model of a character beyond the base body (outfit, hair, beard,
/// equipment), with an optional colour multiplied into its materials.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtraPart {
    pub path: String,
    pub tint: Option<[f32; 3]>,
    /// Rigidly attached (weapons): joint id of the base skeleton, rotation
    /// in degrees (applied X, Y, Z: uCnsAttach 0x00859bb0) and offset in cm. The part
    /// keeps its own small skeleton under that joint.
    pub attach: Option<(u8, [f32; 3], [f32; 3])>,
}

/// Spawns `model_path` with its skeleton at `translation` (viewer units),
/// playing `slot` of `motion_path`. Returns the root entity.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    model_path: &str,
    motion_path: &str,
    slot: usize,
    translation: Vec3,
    hold_frame: Option<f32>,
) -> Result<Entity, String> {
    spawn_assembled(
        commands,
        loader,
        meshes,
        materials,
        images,
        bindposes,
        model_path,
        None,
        None,
        &[],
        &[],
        &|_| true,
        motion_path,
        slot,
        translation,
        hold_frame,
    )
}

/// Spawns one character: the base body's skeleton, shared by every part
/// (each part's bones bind to the base joints of the same joint id, the
/// way MT parts reuse the main model's joint matrices; joints a part adds
/// hang under their parent), with only the meshes whose part group
/// `visible` accepts (the character's part mask, ddlook::PartMask).
#[allow(clippy::too_many_arguments)]
pub fn spawn_assembled(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    model_path: &str,
    base_tint: Option<[f32; 3]>,
    body: Option<&ddlook::morph::BodyMorph>,
    face: &[ddlook::face::FaceJoint],
    parts: &[ExtraPart],
    visible: &dyn Fn(u16) -> bool,
    motion_path: &str,
    slot: usize,
    translation: Vec3,
    hold_frame: Option<f32>,
) -> Result<Entity, String> {
    spawn_assembled_hands(
        commands, loader, meshes, materials, images, bindposes, model_path, base_tint, None, body, face, parts, &[], visible,
        motion_path, slot, translation, hold_frame,
    )
}

/// [`spawn_assembled`], with each attached part's drawn joint id (by part
/// index; missing or `None`: stays on its row when drawn).
#[allow(clippy::too_many_arguments)]
pub fn spawn_assembled_hands(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    model_path: &str,
    base_tint: Option<[f32; 3]>,
    eyebrow: Option<ddlook::eyebrow::Eyebrow>,
    body: Option<&ddlook::morph::BodyMorph>,
    face: &[ddlook::face::FaceJoint],
    parts: &[ExtraPart],
    hands: &[Option<u8>],
    visible: &dyn Fn(u16) -> bool,
    motion_path: &str,
    slot: usize,
    translation: Vec3,
    hold_frame: Option<f32>,
) -> Result<Entity, String> {
    let bytes = loader.vfs.read(model_path, RMODEL)?;
    let model = mtmod::parse(&bytes).map_err(|e| format!("{model_path}: {e}"))?;
    if model.bones.is_empty() {
        return Err(format!("{model_path} has no bones"));
    }
    // No motion path: hold the rest pose (an empty motion keys nothing).
    let motion = if motion_path.is_empty() {
        mtlmt::Motion {
            frame_count: 1,
            loop_frame: 0,
            end_position: [0.0; 4],
            end_rotation: [0.0; 4],
            flags: 0,
            tracks: Vec::new(),
        }
    } else {
        let lmt_bytes = loader.vfs.read(motion_path, RMOTIONLIST)?;
        let list = mtlmt::parse(&lmt_bytes).map_err(|e| format!("{motion_path}: {e}"))?;
        list.motions
            .get(slot)
            .cloned()
            .flatten()
            .ok_or_else(|| format!("{motion_path}: slot {slot} is empty or missing"))?
    };

    // Root in game units (cm), scaled to viewer units (m). The body
    // morph's model scale sits at the skeleton's root (apply_morph).
    let root = commands
        .spawn((
            Transform::from_translation(translation).with_scale(Vec3::splat(UNIT)),
            Visibility::default(),
            Name::new(model_path.to_string()),
            crate::stage::transparent_sort::ModelSortCenter(Vec3::from_slice(&model.sphere[..3])),
        ))
        .id();
    let mut joints_by_id: HashMap<u8, Entity> = HashMap::new();
    let joint_entities = bind_skeleton(commands, &model, root, &mut joints_by_id);
    let mut attach = vec![];
    let mut diagnostic_groups = spawn_meshes(commands, loader, meshes, materials, images, bindposes, &model, model_path, &joint_entities, root, visible, base_tint, eyebrow);
    for (k, part) in parts.iter().enumerate() {
        let pb = match loader.vfs.read(&part.path, RMODEL) {
            Ok(b) => b,
            Err(e) => {
                warn!("{}: {e}", part.path);
                continue;
            }
        };
        let pm = match mtmod::parse(&pb) {
            Ok(m) => m,
            Err(e) => {
                warn!("{}: {e}", part.path);
                continue;
            }
        };
        let joints = match part.attach {
            Some((joint, rot, pos)) => {
                let Some(bone) = model.bones.iter().position(|b| b.anim_map_index == joint) else {
                    warn!("{}: joint {joint} missing", part.path);
                    continue;
                };
                let r = rot.map(f32::to_radians);
                // uCnsAttach (0x00859bb0, attach mode 1) builds the row-vector
                // matrix Rx * Ry * Rz from degrees: X first, then Y, then Z,
                // i.e. Rz * Ry * Rx for column vectors. The anchor hangs
                // under the root; apply_morph places it on the joint.
                let local = Transform::from_translation(Vec3::from(pos)).with_rotation(Quat::from_euler(EulerRot::ZYX, r[2], r[1], r[0]));
                let anchor = commands.spawn((local, Visibility::default(), Name::new(format!("attach {}", part.path)))).id();
                commands.entity(root).add_child(anchor);
                let hand = hands
                    .get(k)
                    .copied()
                    .flatten()
                    .and_then(|h| model.bones.iter().position(|b| b.anim_map_index == h));
                attach.push(Attach { anchor, bone, local: local.to_matrix(), hand });
                // Its own joints: weapon bone ids (0, 1, 2) are not body joints.
                bind_skeleton(commands, &pm, anchor, &mut HashMap::new())
            }
            None => bind_skeleton(commands, &pm, root, &mut joints_by_id),
        };
        let all = |_: u16| true;
        let vis: &dyn Fn(u16) -> bool = if part.attach.is_some() { &all } else { visible };
        diagnostic_groups.extend(spawn_meshes(commands, loader, meshes, materials, images, bindposes, &pm, &part.path, &joints, root, vis, part.tint, None));
    }
    if std::env::var_os("DDVIEW_GROUP_COLOURS").is_some() {
        diagnostic_groups.sort_unstable();
        diagnostic_groups.dedup();
        let legend = diagnostic_groups.iter().map(|&group| {
            let rgb = diagnostic_group_colour(group).to_srgba();
            format!("{group} -> #{:02X}{:02X}{:02X}", (rgb.red * 255.0).round() as u8,
                (rgb.green * 255.0).round() as u8, (rgb.blue * 255.0).round() as u8)
        }).collect::<Vec<_>>().join(", ");
        eprintln!("[group-colours] actor={root:?} model={model_path} motion={motion_path} {legend}");
    }
    // Every assembled character runs the morph pass (it also places the
    // attachments); without edit data it is the identity.
    let morph = Some({
        let bones: Vec<(Entity, Option<usize>)> = model
            .bones
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let p = usize::from(x.parent);
                (joint_entities[i], (x.parent != 255 && p < i).then_some(p))
            })
            .collect();
        // Body edits (ddlook::morph), then face edits (ddlook::face). Joint
        // 0x90 gets both; their order is *assumed*.
        let joint = model
            .bones
            .iter()
            .map(|x| {
                let id = x.anim_map_index;
                let mut v = vec![];
                if let Some((_, m)) = body.and_then(|b| b.joints.iter().find(|(j, _)| *j == id)) {
                    v.push(Edit {
                        joint_id: id,
                        pivot: None,
                        kind: m.kind,
                        scale: Vec3::from(m.scale),
                        child: Vec3::from(m.child),
                        offset: Vec3::from(m.offset),
                        rotation: Quat::IDENTITY,
                    });
                }
                // 0x00812820 resets +0x61, then 0x0081e590/0x0081a7c0/
                // 0x0081a700 activate nondefault records only. Preserve
                // face_morph's gating; an absent identity edit stays off.
                if let Some(f) = face.iter().find(|f| f.id == id) {
                    let r = f.rotation.map(f32::to_radians);
                    let (kind, pivot_id) = ddlook::face_constraint::binding(id).unwrap_or((0, None));
                    v.push(Edit {
                        joint_id: id,
                        pivot: pivot_id.and_then(|pivot| model.bones.iter().position(|b| b.anim_map_index == pivot)),
                        kind,
                        scale: Vec3::from(f.scale),
                        child: Vec3::ONE,
                        offset: Vec3::from(f.offset),
                        rotation: Quat::from_euler(EulerRot::ZYX, r[2], r[1], r[0]),
                    });
                }
                v
            })
            .collect();
        let base = model.bones.iter().map(|x| Transform::from_matrix(Mat4::from_cols_array(&x.local))).collect();
        let model_scale = body.map_or(Vec3::ONE, |b| Vec3::from(b.model_scale));
        Morph { bones, joint, model_scale, base, attach }
    });
    commands.entity(root).insert(Playback {
        motion,
        joints: joints_by_id,
        hold_frame,
        event_frame: None,
        blend_request: None,
        blend: None,
        morph,
        drawn: false,
    });
    Ok(root)
}

/// The joint entity of every bone of `model`: an existing joint with the
/// same id, else a new one under the bone's parent (or the root) with the
/// bone's rest transform.
fn bind_skeleton(
    commands: &mut Commands,
    model: &mtmod::Model,
    root: Entity,
    joints_by_id: &mut HashMap<u8, Entity>,
) -> Vec<Entity> {
    let mut out = Vec::with_capacity(model.bones.len());
    for (i, b) in model.bones.iter().enumerate() {
        if let Some(&e) = joints_by_id.get(&b.anim_map_index) {
            out.push(e);
            continue;
        }
        let local = Mat4::from_cols_array(&b.local);
        let e = commands
            .spawn((
                Transform::from_matrix(local),
                Visibility::default(),
                Name::new(format!("joint{}", b.anim_map_index)),
            ))
            .id();
        let parent = usize::from(b.parent);
        if diagnostic_frame("DDVIEW_JOINT_DUMP").is_some() || diagnostic_frame("DDVIEW_JOINT_DUMP_EVENT_FRAME").is_some() {
            commands.entity(e).insert(JointRest {
                id: b.anim_map_index,
                parent_id: (b.parent != 255 && parent < i).then(|| model.bones[parent].anim_map_index),
                local: Transform::from_matrix(local),
            });
        }
        if b.parent != 255 && parent < i {
            commands.entity(out[parent]).add_child(e);
        } else {
            commands.entity(root).add_child(e);
        }
        joints_by_id.insert(b.anim_map_index, e);
        out.push(e);
    }
    out
}

// Viewer diagnostic palette, not an original-game material rule.
fn diagnostic_group_colour(group: u16) -> Color {
    Color::hsl((f32::from(group) * 0.13).fract() * 360.0, 1.0, 0.5)
}

/// Spawns the skinned meshes of one model on `joints` (one per bone).
/// Returns the drawn groups only when the diagnostic is enabled.
#[allow(clippy::too_many_arguments)]
fn spawn_meshes(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    model: &mtmod::Model,
    model_path: &str,
    joints: &[Entity],
    root: Entity,
    visible: &dyn Fn(u16) -> bool,
    tint: Option<[f32; 3]>,
    eyebrow: Option<ddlook::eyebrow::Eyebrow>,
) -> Vec<u16> {
    let group_colours = std::env::var_os("DDVIEW_GROUP_COLOURS").is_some();
    let mut drawn_groups = Vec::new();
    let mut group_materials = HashMap::new();
    // Inverse binds from the rest pose (world = parent world x local in
    // column-vector form). The stored `inverse_bind` matrices are not used:
    // they carry a uniform scale (270.48 on the male body) and a translation
    // from the model's bounding-box minimum, i.e. they map into the
    // quantised vertex space, while mtmod already returns positions in
    // model space (cm). A part's rest pose is its own (its bones carry the
    // same joint ids as the base body's).
    let mut world: Vec<Mat4> = Vec::with_capacity(model.bones.len());
    for (i, b) in model.bones.iter().enumerate() {
        let local = Mat4::from_cols_array(&b.local);
        let parent = usize::from(b.parent);
        world.push(if b.parent != 255 && parent < i {
            world[parent] * local
        } else {
            local
        });
    }
    let inverse: Vec<Mat4> = world.iter().map(Mat4::inverse).collect();
    let bindposes_handle = bindposes.add(SkinnedMeshInverseBindposes::from(inverse));

    let mrl_bytes = loader.vfs.read(model_path, RMATERIAL).ok();
    let mrl = mrl_bytes.as_deref().and_then(|b| mtmrl::parse(b).ok());
    let mfx = loader.mfx;
    let mut mats: HashMap<u16, PartMaterial> = HashMap::new();
    let mut bound_offset = 0;
    let brow_debug = std::env::var_os("DDVIEW_BROW_DEBUG").is_some();
    let part_log = std::env::var_os("DDVIEW_PART_LOG").is_some();
    for (mesh_index, m) in model.meshes.iter().enumerate() {
        // Diagnostic only: skipped meshes never build a material, so report
        // that explicitly rather than constructing one just for logging.
        let log_part = |outcome: &str, alpha: &str| {
            if part_log {
                let name = model.material_names.get(m.material as usize).map(String::as_str).unwrap_or("<missing>");
                info!("PART actor={root:?} model={model_path} mesh={mesh_index} group={} lod={} draw_mode={:#x} material={name} alpha={alpha} vertices={} {outcome}", m.group, m.lod, m.draw_mode, m.vertex_count);
            }
        };
        let first_bound = (m.weight_bound_count > 0)
            .then(|| model.weight_bounds.get(bound_offset..bound_offset + 144))
            .flatten();
        bound_offset += usize::from(m.weight_bound_count) * 144;
        // uModel::vf26 0x00ea5320: the part bit, the disp flag, the LOD
        // bit (LOD 0 here) and the pass mask: draw_mode & the pass's mask
        // (0x00fedb80 stores it at +0x16a). The scene pass "Common" pushes
        // 1 (0x00daf0a8); ShadowCast pushes 8 or 0x200 (0x00ede86c), so
        // the low-poly shadow bodies (draw_mode 0xe6c8) and the outline
        // and velocity shells (0x1020) never draw in colour.
        let skip = if m.draw_mode & 1 == 0 { Some("skipped:no-colour-pass") }
            else if m.flags & 1 == 0 { Some("skipped:display-disabled") }
            else if m.lod & 1 == 0 { Some("skipped:lod0-disabled") }
            else if !visible(m.group) { Some("skipped:part-group-hidden") }
            else { None };
        if let Some(reason) = skip {
            log_part(reason, "not-built");
            continue;
        }
        let Some(layout) = mfx.layout(m.vertex_format) else {
            log_part("skipped:vertex-layout-missing", "not-built");
            continue;
        };
        let decoded = model.decode(m, layout);
        let triangles = model.triangles(m);
        let (v, tris) = match (decoded, triangles) {
            (Ok(v), Ok(tris)) => (v, tris),
            (Err(_), _) => { log_part("skipped:vertex-decode-error", "not-built"); continue; }
            (_, Err(_)) => { log_part("skipped:triangle-decode-error", "not-built"); continue; }
        };
        let skip = if tris.is_empty() { Some("skipped:empty-triangles") }
            else if v.uvs[0].is_empty() { Some("skipped:uv0-missing") }
            else if v.joints.len() != v.positions.len() { Some("skipped:joint-count-mismatch") }
            else { None };
        if let Some(reason) = skip {
            log_part(reason, "not-built");
            continue;
        }
        let usage = if crate::stage::KEEP_MESHES.load(std::sync::atomic::Ordering::Relaxed) {
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD
        } else {
            RenderAssetUsages::RENDER_WORLD
        };
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, v.positions.clone());
        if !v.normals.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, v.normals.clone());
        }
        if !v.tangents.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, v.tangents.clone());
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, v.uvs[0].clone());
        // Bevy skins with four influences; layouts with up to eight keep the
        // four largest, renormalised (an approximation of the original).
        let mut idx = Vec::with_capacity(v.joints.len());
        let mut wts = Vec::with_capacity(v.joints.len());
        for (j, w) in v.joints.iter().zip(&v.weights) {
            let mut pairs: Vec<(u16, f32)> = j.iter().copied().zip(w.iter().copied()).collect();
            pairs.sort_by(|a, b| b.1.total_cmp(&a.1));
            pairs.truncate(4);
            let sum: f32 = pairs.iter().map(|p| p.1).sum();
            let n = if sum > 0.0 { sum } else { 1.0 };
            idx.push(std::array::from_fn::<u16, 4, _>(|k| pairs[k].0));
            wts.push(std::array::from_fn::<f32, 4, _>(|k| {
                if sum > 0.0 {
                    pairs[k].1 / n
                } else if k == 0 {
                    1.0
                } else {
                    0.0
                }
            }));
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(idx));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, wts);
        mesh.insert_indices(Indices::U32(
            tris.iter().flatten().map(|&i| u32::from(i)).collect(),
        ));
        let material = if group_colours {
            drawn_groups.push(m.group);
            PartMaterial::Standard(group_materials.entry(m.group).or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: diagnostic_group_colour(m.group),
                    unlit: true,
                    alpha_mode: AlphaMode::Opaque,
                    cull_mode: None,
                    double_sided: true,
                    ..Default::default()
                })
            }).clone())
        } else { mats
            .entry(m.material)
            .or_insert_with(|| {
                let name = model
                    .material_names
                    .get(m.material as usize)
                    .cloned()
                    .unwrap_or_default();
                // The stage's material rules (stage::Loader::material: the
                // traced albedo, colours, culling, alpha test or blend) and
                // its lighting (dd_material.wgsl: the SH ambient, the sun
                // with its shadow, the fog), so characters are lit as the
                // stage is. Cloth is mostly two-sided: culling its back
                // faces opened holes onto the body.
                let mut is_eyebrow = false;
                let mut is_brow_overlay = false;
                let shading = mrl.as_ref().and_then(|l| {
                    let mat = l.by_name(&name)?;
                    let mut s = mtmrl::shading::describe(mat, l, mfx);
                    // rMaterial::vf09 0x0112e4d0 copies these bits unchanged
                    // to draw material +0x18. 0x00843940 identifies colour
                    // groups 5/6 here; group 6 also has eyebrow texture/UVs.
                    let group = (mat.bits[0] >> 23) & 0xff;
                    is_brow_overlay = group == 6;
                    if let Some(brow) = eyebrow.filter(|_| group == 5 || group == 6) {
                        is_eyebrow = true;
                        s.albedo_color = brow.color;
                        if group == 6 {
                            if let Some(path) = brow.albedo_path() { s.textures.insert("tAlbedoMap".into(), path); }
                            // 0x01067570 / 0x010675f0 replace only the
                            // translation and diagonal of CBMaterial's UV rows.
                            let mut uv = s.uv_transform.unwrap_or([
                                [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0],
                            ]);
                            uv[0][0] = brow.uv_scale[0]; uv[1][1] = brow.uv_scale[1];
                            uv[0][3] = brow.uv_offset[0]; uv[1][3] = brow.uv_offset[1];
                            s.uv_transform = Some(uv);
                        }
                    }
                    if let Some(p) = s.textures.get("tAlbedoMap") {
                        let p = undummy(loader, p);
                        s.textures.insert("tAlbedoMap".into(), p);
                    }
                    Some(s)
                });
                let mut material = loader.character_material(shading.as_ref(), materials, images);
                // *Approximation*: the edit's hair colour multiplies the
                // albedo (the original sets a material constant, 0x008438f0).
                if let (Some(c), PartMaterial::Dd(slot)) = (tint.filter(|_| !is_eyebrow), &material) {
                    let mut m = slot.material.clone();
                    let b = m.base.base_color.to_linear();
                    m.base.base_color = Color::linear_rgba(b.red * c[0], b.green * c[1], b.blue * c[2], b.alpha);
                    material = PartMaterial::Dd(std::sync::Arc::new(DdSlot { material: m, handle: Default::default() }));
                }
                if brow_debug && is_brow_overlay {
                    // Diagnostic viewer choice: isolate the overlay geometry
                    // from texture alpha and lighting. Keep authored culling
                    // and all mesh/group admission rules unchanged.
                    let cull_mode = match &material {
                        PartMaterial::Standard(h) => materials.get(h).and_then(|m| m.cull_mode),
                        PartMaterial::Dd(slot) => slot.material.base.cull_mode,
                    };
                    material = PartMaterial::Standard(materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.0, 0.0),
                        unlit: true,
                        alpha_mode: AlphaMode::Opaque,
                        depth_bias: 5.0,
                        cull_mode,
                        ..default()
                    }));
                    info!("BROW_DEBUG actor={root:?} model={model_path} material={name} opaque-red depth_bias=5");
                }
                material
            })
            .clone() };
        if part_log {
            let alpha = match &material {
                PartMaterial::Standard(h) => materials.get(h).map(|m| format!("{:?}", m.alpha_mode)).unwrap_or_else(|| "missing-material".into()),
                PartMaterial::Dd(slot) => format!("{:?}", slot.material.base.alpha_mode),
            };
            log_part("spawned", &alpha);
        }
        let mut e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            SkinnedMesh {
                inverse_bindposes: bindposes_handle.clone(),
                joints: joints.to_vec(),
            },
        ));
        // 0x00ffc350 uses the first weight bound's bone-space sphere center
        // when primitive flag bit2 selects per-primitive transparent sorting.
        let bound = first_bound.and_then(|bytes| {
            let bone = u32::from_le_bytes(bytes[..4].try_into().ok()?) as usize;
            let xyz = std::array::from_fn::<_, 3, _>(|i| {
                f32::from_le_bytes(bytes[16 + i * 4..20 + i * 4].try_into().unwrap())
            });
            Some((*joints.get(bone)?, Vec3::from(xyz)))
        });
        if m.flags & 4 == 0 || bound.is_some() {
            e.insert(crate::stage::transparent_sort::PrimitiveSort {
                model: root,
                radius_cm: model.sphere[3],
                priority: m.alpha_priority,
                bound: if m.flags & 4 != 0 { bound } else { None },
            });
        }
        match material {
            PartMaterial::Standard(h) => e.insert(MeshMaterial3d(h)),
            PartMaterial::Dd(slot) => e.insert(PendingDd(slot)),
        };
        let e = e.id();
        commands.entity(root).add_child(e);
    }
    drawn_groups
}

/// Samples each playing motion and writes its joints' local transforms,
/// then mixes in the pose frozen at the last motion change while its fade
/// lasts (`mtlmt::blend`).
pub fn animate(
    time: Res<Time>, mut players: Query<(Entity, Option<&Name>, Option<&crate::event::Actor>, &mut Playback)>,
    mut joints: Query<&mut Transform>, rest: Query<&JointRest>, mut dump: Local<JointDump>,
) {
    let _timing = crate::perf::Section::start("character::animate");
    // the model's frame step in motion frames (the same clamp as play::step)
    let step = time.delta_secs().min(0.1) * MOTION_FPS;
    for (entity, name, actor, mut p) in &mut players {
        if let Some(frames) = p.blend_request.take() {
            // freeze the pose shown now (itself possibly mid-blend), before
            // the morph: the morph pass goes on top of the blend each frame,
            // so freezing the morphed locals would morph them again at every
            // motion change (a chain of attacks blew joints up)
            let unmorphed: HashMap<Entity, Transform> = p
                .morph
                .as_ref()
                .map(|m| m.bones.iter().zip(&m.base).map(|(&(e, _), &t)| (e, t)).collect())
                .unwrap_or_default();
            p.blend = mtlmt::blend::Interpolation::start(frames).map(|b| {
                let pose = p
                    .joints
                    .values()
                    .filter_map(|&e| {
                        let t = unmorphed.get(&e).copied().or_else(|| joints.get(e).ok().copied())?;
                        Some((e, t.rotation, t.translation, t.scale))
                    })
                    .collect();
                (b, pose)
            });
        }
        if let Some((b, _)) = &mut p.blend {
            if !b.advance(step) {
                p.blend = None;
            }
        }
        // Morphed skeletons start each frame from the unmorphed locals, so
        // channels the motion does not key are not morphed twice.
        if let Some(m) = &p.morph {
            for (i, &(e, _)) in m.bones.iter().enumerate() {
                if let Ok(mut tr) = joints.get_mut(e) {
                    *tr = m.base[i];
                }
            }
        }
        let n = p.motion.frame_count.max(1) as f32;
        let frame = p.hold_frame.unwrap_or((time.elapsed_secs() * MOTION_FPS) % n);
        for t in &p.motion.tracks {
            if t.joint == 255 {
                continue;
            }
            let Some(&e) = p.joints.get(&t.joint) else {
                continue;
            };
            let Ok(mut tr) = joints.get_mut(e) else { continue };
            let v = t.sample(frame);
            match t.usage {
                mtlmt::Usage::LocalRotation => tr.rotation = Quat::from_xyzw(v[0], v[1], v[2], v[3]),
                mtlmt::Usage::LocalPosition => tr.translation = Vec3::new(v[0], v[1], v[2]),
                mtlmt::Usage::LocalScale => tr.scale = Vec3::new(v[0], v[1], v[2]),
                _ => {}
            }
        }
        if let Some((b, pose)) = &p.blend {
            use mtlmt::blend::{mix_rotation, mix_vector};
            for &(e, r, t, s) in pose {
                let Ok(mut tr) = joints.get_mut(e) else { continue };
                tr.rotation = Quat::from_array(mix_rotation(tr.rotation.to_array(), r.to_array(), b.weight));
                tr.translation = Vec3::from(mix_vector(tr.translation.to_array(), t.to_array(), b.weight));
                tr.scale = Vec3::from(mix_vector(tr.scale.to_array(), s.to_array(), b.weight));
            }
        }
        let drawn = p.drawn;
        if let Some(m) = &mut p.morph {
            apply_morph(m, drawn, &mut joints);
        }
        dump_joints(entity, name, actor, &p, frame, &mut dump, &rest, &joints);
    }
}

/// Spawns a `ddlook` look (base body, parts with hair and beard colours,
/// sheathed weapons, part mask, morphs) playing `slot` of `motion_path`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_look(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<SkinnedMeshInverseBindposes>,
    look: &ddlook::Look,
    motion_path: &str,
    slot: usize,
    at: Vec3,
    hold: Option<f32>,
) -> Result<Entity, String> {
    let mut parts: Vec<ExtraPart> = look
        .parts
        .iter()
        .map(|p| ExtraPart {
            path: p.path.clone(),
            tint: if p.hair {
                look.hair_color
            } else if p.beard {
                look.beard_color
            } else {
                None
            },
            attach: None,
        })
        .collect();
    parts.extend(look.weapons.iter().map(|w| ExtraPart {
        path: w.path.clone(),
        tint: None,
        attach: Some((w.joint, w.rotation, w.offset)),
    }));
    let mask = look.mask;
    // the weapons' drawn joints, by part index (the parts come first)
    let mut hands = vec![None; parts.len() - look.weapons.len()];
    hands.extend(look.weapons.iter().map(|w| w.hand));
    // cBodyEdit is separate from cHumanEdit. Its joints (0xaa..0xce,
    // 0x0080ad20) do not overlap the human or face controller joints.
    // Read bulk body-shape tables from the player's installed executable.
    let mut body = look.body.clone();
    if let Some(shape) = &look.body_shape {
        let file = ddexe::ExeFile::next_to(&loader.vfs.root).map_err(|e| e.to_string())?;
        let exe = file.image().map_err(|e| e.to_string())?;
        let tables = ddlook::body::Tables::read(&exe).map_err(|e| e.to_string())?;
        let extra = tables.joints(look.sex, shape).map_err(|e| e.to_string())?;
        body.get_or_insert_with(|| ddlook::morph::BodyMorph {
            model_scale: [1.0; 3], joints: vec![],
        }).joints.extend(extra);
    }
    spawn_assembled_hands(
        commands,
        loader,
        meshes,
        materials,
        images,
        bindposes,
        &look.base,
        look.skin_color,
        look.eyebrow,
        body.as_ref(),
        &look.face,
        &parts,
        &hands,
        &move |g| mask.visible(g),
        motion_path,
        slot,
        at,
        hold,
    )
}

/// Enemy materials name 8x8 placeholder maps `d_<name>` (e.g.
/// `model\em\e01\e0100\d_e0100_all_BM`); the full map `<name>` sits next
/// to it. The game's swap is not traced (*viewer choice*): use the full
/// map when the install has it.
fn undummy(loader: &Loader, path: &str) -> String {
    let Some((dir, file)) = path.rsplit_once('\\') else { return path.to_string() };
    let Some(rest) = file.strip_prefix("d_").or_else(|| file.strip_prefix("D_")) else { return path.to_string() };
    let full = format!("{dir}\\{rest}");
    if loader.vfs.locate(&full, crate::stage::RTEXTURE).is_some() {
        full
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod eyebrow_install_tests {
    #[test]
    fn eyelid_constraint_uses_current_eye_pivot_without_accumulating_morphs() {
        use super::*;
        use bevy::ecs::system::SystemState;
        let mut world = World::new();
        let parent_pose = Transform::from_translation(Vec3::new(10.0, -5.0, 8.0))
            .with_rotation(Quat::from_rotation_z(0.37)).with_scale(Vec3::new(1.3, 0.7, 1.1));
        let eye_pose = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0))
            .with_rotation(Quat::from_rotation_z(0.4)).with_scale(Vec3::new(1.1, 0.8, 1.3));
        let lid_pose = Transform::from_translation(Vec3::new(2.0, 3.0, 4.0))
            .with_rotation(Quat::from_rotation_z(-0.2)).with_scale(Vec3::new(0.9, 1.2, 0.8));
        let parent = world.spawn(parent_pose).id();
        let eye = world.spawn(eye_pose).id();
        let lid = world.spawn(lid_pose).id();
        let mut morph = Morph {
            bones: vec![(parent, None), (eye, Some(0)), (lid, Some(0))],
            joint: vec![vec![], vec![], vec![Edit {
                joint_id: 82, pivot: Some(1), kind: 0xd,
                scale: Vec3::new(1.2, 0.85, 1.1), child: Vec3::ONE,
                offset: Vec3::new(0.2, -0.3, 0.5), rotation: Quat::IDENTITY,
            }]],
            model_scale: Vec3::ONE, base: vec![parent_pose, eye_pose, lid_pose], attach: vec![],
        };
        let mut state: SystemState<Query<&mut Transform>> = SystemState::new(&mut world);
        let mut first = None;
        for _ in 0..3 {
            let mut joints = state.get_mut(&mut world).unwrap();
            // Same motion-only restore as animate, before each morph pass.
            for (i, &(entity, _)) in morph.bones.iter().enumerate() {
                *joints.get_mut(entity).unwrap() = morph.base[i];
            }
            apply_morph(&mut morph, false, &mut joints);
            let local = *joints.get(lid).unwrap();
            let pose = parent_pose.to_matrix() * local.to_matrix();
            // Full vf24 0x00860ac0 synthetic fixture: type d target position
            // is independent of its rotation. This also checks pivot mapping.
            assert!(pose.w_axis.truncate().distance(Vec3::new(12.238449, -2.356826, 13.115)) < 0.000_01);
            if let Some(previous) = first {
                assert_eq!(local, previous);
            } else { first = Some(local); }
            assert_eq!(morph.base[2], lid_pose);
        }
    }

    #[test]
    fn near_fade_restores_authored_material_and_does_not_change_its_source() {
        use super::*;
        use crate::stage::{DdExt, DdMaterial, DdParams, SkyEnvParams};
        let source = DdMaterial {
            base: StandardMaterial { base_color: Color::srgba(0.5, 0.4, 0.3, 0.8),
                alpha_mode: AlphaMode::Mask(0.5), ..default() },
            extension: DdExt { depth: Some(15), params: DdParams::default(),
                map: None, spec: None, sky: SkyEnvParams::default(),
                sky_rayleigh: default(), sky_mie: default(), env_cube: None, albedo_blend: None, normal: None },
        };
        let mut private = source.clone();
        let authored = (0.8, source.base.alpha_mode, source.extension.depth, source.extension.params.flags);
        crate::stage::character_opacity(&mut private, 0.25, authored);
        assert_eq!(private.base.base_color.alpha(), 0.2);
        assert_eq!(private.base.alpha_mode, AlphaMode::Blend);
        assert_eq!(private.extension.depth, Some(13));
        assert_eq!(source.base.base_color.alpha(), 0.8);
        private.extension.params.flags |= 32; // A concurrent sky update survives restoration.
        crate::stage::character_opacity(&mut private, 1.25, authored);
        assert_eq!(private.base.base_color.alpha(), 0.8);
        assert_eq!(private.base.alpha_mode, AlphaMode::Mask(0.5));
        assert_eq!(private.extension.depth, Some(15));
        assert_eq!(private.extension.params.flags, 32);
    }

    #[test]
    fn npc_eyebrow_materials_are_selected_from_installed_mrl() {
        let path = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !path.is_dir() { return; }
        let v = ddvfs::Vfs::open(path).unwrap();
        let mfx = mtshader::ShaderPackage::parse(&std::fs::read(path.join("usershader/BbsShaderPackage.mfx")).unwrap()).unwrap();
        for n in 1..=2 {
            let look = ddlook::npc_look(&v, n, n, 22).unwrap().look;
            let bytes = v.read(&look.base, super::RMATERIAL).unwrap();
            let mrl = mtmrl::parse(&bytes).unwrap();
            assert!(look.eyebrow.is_some());
            let model_bytes = v.read(&look.base, super::RMODEL).unwrap();
            let model = mtmod::parse(&model_bytes).unwrap();
            let mut brows = 0;
            for mesh in &model.meshes {
                let name = &model.material_names[mesh.material as usize];
                let mat = mrl.by_name(name).unwrap();
                let group = (mat.bits[0] >> 23) & 0xff;
                if group == 6 && mesh.draw_mode & 1 != 0 {
                    brows += 1;
                    assert_eq!(look.mask.visible(mesh.group), n == 2);
                    let s = mtmrl::shading::describe(mat, &mrl, &mfx);
                    assert!(s.blends_alpha());
                    assert!(!s.depth.unwrap().depth_write);
                    assert_eq!(s.raster.unwrap().depth_bias, 0);
                    assert!(s.uv_transform.is_some());
                }
            }
            assert_eq!(brows, 1);
        }
    }
}
