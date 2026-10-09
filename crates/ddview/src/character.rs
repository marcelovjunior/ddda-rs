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

use crate::stage::{Loader, RMATERIAL, RMODEL, UNIT};

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
    parts: &[ExtraPart],
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

    // Root in game units (cm), scaled to viewer units (m).
    let root = commands
        .spawn((
            Transform::from_translation(translation).with_scale(Vec3::splat(UNIT)),
            Visibility::default(),
            Name::new(model_path.to_string()),
        ))
        .id();
    let mut joints_by_id: HashMap<u8, Entity> = HashMap::new();
    let joint_entities = bind_skeleton(commands, &model, root, &mut joints_by_id);
    spawn_meshes(commands, loader, meshes, materials, images, bindposes, &model, model_path, &joint_entities, root, visible, base_tint);
    for part in parts {
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
                let Some(&parent) = joints_by_id.get(&joint) else {
                    warn!("{}: joint {joint} missing", part.path);
                    continue;
                };
                let r = rot.map(f32::to_radians);
                let anchor = commands
                    .spawn((
                        // uCnsAttach (0x00859bb0, attach mode 1) builds the row-vector
                        // matrix Rx * Ry * Rz from degrees: X first, then Y,
                        // then Z, i.e. Rz * Ry * Rx for column vectors.
                        Transform::from_translation(Vec3::from(pos)).with_rotation(Quat::from_euler(EulerRot::ZYX, r[2], r[1], r[0])),
                        Visibility::default(),
                        Name::new(format!("attach {}", part.path)),
                    ))
                    .id();
                commands.entity(parent).add_child(anchor);
                // Its own joints: weapon bone ids (0, 1, 2) are not body joints.
                bind_skeleton(commands, &pm, anchor, &mut HashMap::new())
            }
            None => bind_skeleton(commands, &pm, root, &mut joints_by_id),
        };
        let all = |_: u16| true;
        let vis: &dyn Fn(u16) -> bool = if part.attach.is_some() { &all } else { visible };
        spawn_meshes(commands, loader, meshes, materials, images, bindposes, &pm, &part.path, &joints, root, vis, part.tint);
    }
    commands.entity(root).insert(Playback {
        motion,
        joints: joints_by_id,
        hold_frame,
        blend_request: None,
        blend: None,
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
    let mut mats: HashMap<u16, Handle<StandardMaterial>> = HashMap::new();
    for m in &model.meshes {
        if m.lod & 1 == 0 || !visible(m.group) {
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
                let mut sm = StandardMaterial {
                    perceptual_roughness: 0.8,
                    reflectance: 0.3,
                    ..default()
                };
                if let (Some(list), Some(mat)) = (mrl.as_ref(), mrl.as_ref().and_then(|l| l.by_name(&name))) {
                    // The traced material description (mtmrl::shading,
                    // docs/SHADING.md): albedo map and colours, culling,
                    // alpha test or blend. Cloth is mostly two-sided:
                    // culling its back faces opened holes onto the body.
                    let s = mtmrl::shading::describe(mat, list, mfx);
                    if let Some(p) = s.textures.get("tAlbedoMap") {
                        sm.base_color_texture = loader.texture(p, images);
                    }
                    let c: [f32; 3] = std::array::from_fn(|k| s.albedo_color[k] * s.diffuse_color[k]);
                    let alpha = if s.uses_alpha() { s.transparency } else { 1.0 };
                    sm.base_color = Color::linear_rgba(c[0], c[1], c[2], alpha);
                    match s.raster.map(|r| r.cull) {
                        Some(mtshader::Cull::None) => {
                            sm.double_sided = true;
                            sm.cull_mode = None;
                        }
                        Some(mtshader::Cull::Front) => sm.cull_mode = Some(bevy::render::render_resource::Face::Front),
                        _ => sm.cull_mode = Some(bevy::render::render_resource::Face::Back),
                    }
                    let test = s.alpha_test.map(|t| t.reference_f32() + 0.5 / 255.0);
                    sm.alpha_mode = if !s.uses_alpha() {
                        AlphaMode::Opaque
                    } else if s.blends_alpha() {
                        AlphaMode::Blend
                    } else if let Some(cutoff) = test {
                        AlphaMode::Mask(cutoff)
                    } else {
                        AlphaMode::Opaque
                    };
                }
                // *Approximation*: the edit's hair colour multiplies the
                // albedo (the original sets a material constant, 0x008438f0).
                if let Some(c) = tint {
                    let b = sm.base_color.to_linear();
                    sm.base_color = Color::linear_rgba(b.red * c[0], b.green * c[1], b.blue * c[2], b.alpha);
                }
                materials.add(sm)
            })
            .clone();
        let e = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                SkinnedMesh {
                    inverse_bindposes: bindposes_handle.clone(),
                    joints: joints.to_vec(),
                },
            ))
            .id();
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
            // freeze the pose shown now (itself possibly mid-blend)
            p.blend = mtlmt::blend::Interpolation::start(frames).map(|b| {
                let pose = p
                    .joints
                    .values()
                    .filter_map(|&e| {
                        let t = joints.get(e).ok()?;
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
    }
}
