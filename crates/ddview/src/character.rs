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

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use crate::stage::{DdSlot, Loader, PartMaterial, PendingDd, RMATERIAL, RMODEL, UNIT};

pub const RMOTIONLIST: u32 = 0x7682_0D81;

/// Frames per second of motion data. *Assumed* 30 until traced in the exe.
pub const MOTION_FPS: f32 = 30.0;

#[derive(Component)]
pub struct Playback {
    pub motion: mtlmt::Motion,
    /// Joint entity by joint id.
    pub joints: HashMap<u8, Entity>,
    /// Fixed frame to hold (screenshots); `None` plays in a loop.
    pub hold_frame: Option<f32>,
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
    /// +0xa4: 0 plain, 0xb absolute size (also takes `child`), 9 and 0xc
    /// approximated as plain.
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
/// k is the scale for types 0 and 9; for type 0xb it is scale x child
/// divided by the world axis lengths, so those joints end at an absolute
/// size whatever their parents did (0x0086d180). The model scale is the
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
            let k = if e.kind == 0xb {
                let len = Vec3::new(w.x_axis.truncate().length(), w.y_axis.truncate().length(), w.z_axis.truncate().length());
                e.scale * e.child / len.max(Vec3::splat(1.192_092_9e-7))
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
        commands, loader, meshes, materials, images, bindposes, model_path, base_tint, body, face, parts, &[], visible,
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
        ))
        .id();
    let mut joints_by_id: HashMap<u8, Entity> = HashMap::new();
    let joint_entities = bind_skeleton(commands, &model, root, &mut joints_by_id);
    let mut attach = vec![];
    spawn_meshes(commands, loader, meshes, materials, images, bindposes, &model, model_path, &joint_entities, root, visible, base_tint);
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
        spawn_meshes(commands, loader, meshes, materials, images, bindposes, &pm, &part.path, &joints, root, vis, part.tint);
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
        // Body edits (ddlook::morph), then face edits (ddlook::face; the eye
        // joints' special types are not translated and are left out). Joint
        // 0x90 gets both; their order is *assumed*.
        let joint = model
            .bones
            .iter()
            .map(|x| {
                let id = x.anim_map_index;
                let mut v = vec![];
                if let Some((_, m)) = body.and_then(|b| b.joints.iter().find(|(j, _)| *j == id)) {
                    v.push(Edit {
                        kind: m.kind,
                        scale: Vec3::from(m.scale),
                        child: Vec3::from(m.child),
                        offset: Vec3::from(m.offset),
                        rotation: Quat::IDENTITY,
                    });
                }
                if let Some(f) = face.iter().find(|f| f.id == id && ddlook::face::special_kind(id).is_none()) {
                    let r = f.rotation.map(f32::to_radians);
                    v.push(Edit {
                        kind: 0,
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

/// Spawns the skinned meshes of one model on `joints` (one per bone).
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
) {
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
    for m in &model.meshes {
        // uModel::vf26 0x00ea5320: the part bit, the disp flag, the LOD
        // bit (LOD 0 here) and the pass mask: draw_mode & the pass's mask
        // (0x00fedb80 stores it at +0x16a). The scene pass "Common" pushes
        // 1 (0x00daf0a8); ShadowCast pushes 8 or 0x200 (0x00ede86c), so
        // the low-poly shadow bodies (draw_mode 0xe6c8) and the outline
        // and velocity shells (0x1020) never draw in colour.
        if m.draw_mode & 1 == 0 || m.flags & 1 == 0 || m.lod & 1 == 0 || !visible(m.group) {
            continue;
        }
        let Some(layout) = mfx.layout(m.vertex_format) else {
            continue;
        };
        let (Ok(v), Ok(tris)) = (model.decode(m, layout), model.triangles(m)) else {
            continue;
        };
        if tris.is_empty() || v.uvs[0].is_empty() || v.joints.len() != v.positions.len() {
            continue;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, v.positions.clone());
        if !v.normals.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, v.normals.clone());
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
        let material = mats
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
                let shading = mrl.as_ref().and_then(|l| {
                    let mut s = mtmrl::shading::describe(l.by_name(&name)?, l, mfx);
                    if let Some(p) = s.textures.get("tAlbedoMap") {
                        let p = undummy(loader, p);
                        s.textures.insert("tAlbedoMap".into(), p);
                    }
                    Some(s)
                });
                let mut material = loader.character_material(shading.as_ref(), materials, images);
                // *Approximation*: the edit's hair colour multiplies the
                // albedo (the original sets a material constant, 0x008438f0).
                if let (Some(c), PartMaterial::Dd(slot)) = (tint, &material) {
                    let mut m = slot.material.clone();
                    let b = m.base.base_color.to_linear();
                    m.base.base_color = Color::linear_rgba(b.red * c[0], b.green * c[1], b.blue * c[2], b.alpha);
                    material = PartMaterial::Dd(std::sync::Arc::new(DdSlot { material: m, handle: Default::default() }));
                }
                material
            })
            .clone();
        let mut e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            SkinnedMesh {
                inverse_bindposes: bindposes_handle.clone(),
                joints: joints.to_vec(),
            },
        ));
        match material {
            PartMaterial::Standard(h) => e.insert(MeshMaterial3d(h)),
            PartMaterial::Dd(slot) => e.insert(PendingDd(slot)),
        };
        let e = e.id();
        commands.entity(root).add_child(e);
    }
}

/// Samples each playing motion and writes its joints' local transforms,
/// then mixes in the pose frozen at the last motion change while its fade
/// lasts (`mtlmt::blend`).
pub fn animate(time: Res<Time>, mut players: Query<&mut Playback>, mut joints: Query<&mut Transform>) {
    // the model's frame step in motion frames (the same clamp as play::step)
    let step = time.delta_secs().min(0.1) * MOTION_FPS;
    for mut p in &mut players {
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
    spawn_assembled_hands(
        commands,
        loader,
        meshes,
        materials,
        images,
        bindposes,
        &look.base,
        look.skin_color,
        look.body.as_ref(),
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
