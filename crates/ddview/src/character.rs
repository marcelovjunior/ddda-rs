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
    let mut joint_entities = Vec::with_capacity(model.bones.len());
    for (i, b) in model.bones.iter().enumerate() {
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
            commands.entity(joint_entities[parent]).add_child(e);
        } else {
            commands.entity(root).add_child(e);
        }
        joint_entities.push(e);
    }
    // Inverse binds from the rest pose (world = parent world x local in
    // column-vector form). The stored `inverse_bind` matrices are not used:
    // they carry a uniform scale (270.48 on the male body) and a translation
    // from the model's bounding-box minimum, i.e. they map into the
    // quantised vertex space, while mtmod already returns positions in
    // model space (cm).
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
    let joints_by_id: HashMap<u8, Entity> = model
        .bones
        .iter()
        .zip(&joint_entities)
        .map(|(b, e)| (b.anim_map_index, *e))
        .collect();

    let mrl_bytes = loader.vfs.read(model_path, RMATERIAL).ok();
    let mrl = mrl_bytes.as_deref().and_then(|b| mtmrl::parse(b).ok());
    let mfx = loader.mfx;
    let mut mats: HashMap<u16, Handle<StandardMaterial>> = HashMap::new();
    for m in &model.meshes {
        if m.lod & 1 == 0 {
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
                if let Some(mat) = mrl.as_ref().and_then(|l| l.by_name(&name)) {
                    for c in &mat.commands {
                        if c.kind == mtmrl::CommandKind::Texture
                            && mfx.object(c.object).map(|o| o.name.as_str()) == Some("tAlbedoMap")
                        {
                            if let Some(t) = mrl.as_ref().and_then(|l| l.texture(c)) {
                                let p = t.path.clone();
                                sm.base_color_texture = loader.texture(&p, images);
                            }
                        }
                    }
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
                    joints: joint_entities.clone(),
                },
            ))
            .id();
        commands.entity(root).add_child(e);
    }
    commands.entity(root).insert(Playback {
        motion,
        joints: joints_by_id,
        hold_frame,
    });
    Ok(root)
}

/// Samples each playing motion and writes its joints' local transforms.
pub fn animate(time: Res<Time>, players: Query<&Playback>, mut joints: Query<&mut Transform>) {
    for p in &players {
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
    }
}
