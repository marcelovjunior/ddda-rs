//! Converts stage cells (MOD + MRL + TEX from the user's install) into
//! Bevy meshes, materials and images, and places each cell's static
//! scenery from its `_s00` layout (`mtlot`).
//!
//! Placement follows `docs/WORLD.md`: cell `st<S>_<A>m<B>n` sits at
//! X = (B - B0) * 10000, Z = (A - A0) * 10000 game units from the origin
//! cell (A0, B0). Game units are taken as centimetres and scaled by 0.01
//! (*unresolved:* unit not yet confirmed). Axes are used as stored (Y up);
//! the reference screenshots show no mirroring.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use ddvfs::Vfs;
use mtshader::ShaderPackage;

pub const RMODEL: u32 = 0x58A1_5856;
pub const RMATERIAL: u32 = 0x2749_C8A8;
pub const RTEXTURE: u32 = 0x241F_5DEB;
pub const RLAYOUT: u32 = 0x1530_2EF4;
pub const UNIT: f32 = 0.01;

#[derive(Default)]
pub struct Stats {
    pub cells: usize,
    pub instances: usize,
    pub instances_without_model: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub textures: usize,
    pub missing: Vec<String>,
}

/// One drawable piece of a converted model.
#[derive(Clone)]
pub struct Part {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub triangles: usize,
}

pub struct Loader<'a> {
    pub vfs: &'a Vfs,
    pub mfx: &'a ShaderPackage,
    pub textures: HashMap<String, Option<Handle<Image>>>,
    /// Converted models by resource path, shared by every instance.
    pub models: HashMap<String, Option<Vec<Part>>>,
    /// Model resource path by its last path component (`om0528`).
    pub model_names: HashMap<String, String>,
    pub stats: Stats,
}

/// Builds a Bevy image from a TEX file: base colour, so sRGB (*assumed*:
/// the exe's sRGB rule is not traced yet).
pub fn tex_to_image(bytes: &[u8]) -> Result<Image, String> {
    let t = mttex::parse(bytes).map_err(|e| e.to_string())?;
    let h = &t.header;
    if h.shape != mttex::Shape::Flat {
        return Err(format!("{:?} textures not supported in the viewer", h.shape));
    }
    let format = match h.format {
        mttex::D3dFormat::Dxt1 => TextureFormat::Bc1RgbaUnormSrgb,
        mttex::D3dFormat::Dxt3 => TextureFormat::Bc2RgbaUnormSrgb,
        mttex::D3dFormat::Dxt5 => TextureFormat::Bc3RgbaUnormSrgb,
        mttex::D3dFormat::A8R8G8B8 | mttex::D3dFormat::X8R8G8B8 => TextureFormat::Bgra8UnormSrgb,
        f => return Err(format!("format {f:?} not supported in the viewer")),
    };
    if format != TextureFormat::Bgra8UnormSrgb && (h.width % 4 != 0 || h.height % 4 != 0) {
        return Err(format!(
            "{}x{} not a multiple of the block size",
            h.width, h.height
        ));
    }
    let data: Vec<u8> = t.surfaces.iter().flat_map(|s| s.data.iter().copied()).collect();
    let mut img = Image::new_uninit(
        Extent3d {
            width: h.width,
            height: h.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = h.mip_count.max(1);
    img.data = Some(data);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    Ok(img)
}

impl Loader<'_> {
    fn texture(&mut self, path: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(h) = self.textures.get(path) {
            return h.clone();
        }
        let h = match self.vfs.read(path, RTEXTURE).and_then(|b| tex_to_image(&b)) {
            Ok(img) => {
                self.stats.textures += 1;
                Some(images.add(img))
            }
            Err(e) => {
                self.stats.missing.push(format!("texture {path}: {e}"));
                None
            }
        };
        self.textures.insert(path.to_string(), h.clone());
        h
    }

    /// Index of model resources by their last path component.
    pub fn index_models(&mut self) {
        for p in self.vfs.paths_of_type(RMODEL) {
            let name = p.rsplit('\\').next().unwrap_or(p).to_string();
            self.model_names.entry(name).or_insert_with(|| p.to_string());
        }
    }

    /// Converts a model (MOD + its MRL) once; later calls reuse the result.
    pub fn model(
        &mut self,
        path: &str,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Vec<Part>> {
        if let Some(parts) = self.models.get(path) {
            return parts.clone();
        }
        let parts = self.convert_model(path, meshes, materials, images);
        self.models.insert(path.to_string(), parts.clone());
        parts
    }

    fn convert_model(
        &mut self,
        path: &str,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Option<Vec<Part>> {
        let mfx = self.mfx;
        let mod_bytes = self.vfs.read(path, RMODEL).ok()?;
        let model = match mtmod::parse(&mod_bytes) {
            Ok(m) => m,
            Err(e) => {
                self.stats.missing.push(format!("{path}: {e}"));
                return None;
            }
        };
        let mrl_bytes = self.vfs.read(path, RMATERIAL).ok();
        let mrl = mrl_bytes.as_deref().and_then(|b| mtmrl::parse(b).ok());
        let mut mat_cache: HashMap<u16, Handle<StandardMaterial>> = HashMap::new();
        let mut parts = Vec::new();
        for m in &model.meshes {
            // LOD byte: bit 0 = highest detail (*assumed*), 255 = all.
            if m.lod & 1 == 0 {
                continue;
            }
            let Some(layout) = mfx.layout(m.vertex_format) else {
                continue;
            };
            let Ok(v) = model.decode(m, layout) else { continue };
            let Ok(tris) = model.triangles(m) else { continue };
            if v.uvs[0].is_empty() || tris.is_empty() {
                continue;
            }
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                v.positions
                    .iter()
                    .map(|p| [p[0] * UNIT, p[1] * UNIT, p[2] * UNIT])
                    .collect::<Vec<_>>(),
            );
            if !v.normals.is_empty() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, v.normals.clone());
            }
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, v.uvs[0].clone());
            mesh.insert_indices(Indices::U32(
                tris.iter().flatten().map(|&i| u32::from(i)).collect(),
            ));
            let material = mat_cache
                .entry(m.material)
                .or_insert_with(|| {
                    let name = model
                        .material_names
                        .get(m.material as usize)
                        .cloned()
                        .unwrap_or_default();
                    let mut sm = StandardMaterial {
                        perceptual_roughness: 0.9,
                        metallic: 0.0,
                        reflectance: 0.2,
                        ..default()
                    };
                    if let Some(mat) = mrl.as_ref().and_then(|l| l.by_name(&name)) {
                        for c in &mat.commands {
                            let obj = mfx.object(c.object).map(|o| o.name.as_str());
                            match (c.kind, obj) {
                                (mtmrl::CommandKind::Texture, Some("tAlbedoMap")) => {
                                    if let Some(t) = mrl.as_ref().and_then(|l| l.texture(c)) {
                                        let p = t.path.clone();
                                        sm.base_color_texture = self.texture(&p, images);
                                    }
                                }
                                (mtmrl::CommandKind::Feature, Some("FTransparency")) => {
                                    let v = mfx.object(c.value).map(|o| o.name.as_str());
                                    if matches!(v, Some("FTransparencyAlpha") | Some("FTransparencyMap")) {
                                        // Alpha test vs blend not yet traced; cut-out
                                        // at 0.5 is a placeholder.
                                        sm.alpha_mode = AlphaMode::Mask(0.5);
                                        sm.double_sided = true;
                                        sm.cull_mode = None;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    materials.add(sm)
                })
                .clone();
            parts.push(Part {
                mesh: meshes.add(mesh),
                material,
                triangles: tris.len(),
            });
        }
        Some(parts)
    }

    fn spawn_parts(&mut self, commands: &mut Commands, parts: &[Part], transform: Transform, name: &str) {
        for p in parts {
            self.stats.meshes += 1;
            self.stats.triangles += p.triangles;
            commands.spawn((
                Mesh3d(p.mesh.clone()),
                MeshMaterial3d(p.material.clone()),
                transform,
                Name::new(name.to_string()),
            ));
        }
    }

    /// Spawns one cell's terrain model and its static scenery (`_s00`
    /// layout). `offset` is the cell's position in viewer units. Returns
    /// false if the cell has no model.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_cell(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        stage: u32,
        a: i32,
        b: i32,
        offset: Vec3,
    ) -> bool {
        // The folder is the cell's A (m) number rounded down to tens, e.g.
        // scr\st100\model\m50\st100_58m49n (checked with ddtool arc-ls).
        let path = format!("scr\\st{stage}\\model\\m{}\\st{stage}_{a}m{b}n", a / 10 * 10);
        let Some(parts) = self.model(&path, meshes, materials, images) else {
            return false;
        };
        self.stats.cells += 1;
        let cell = format!("st{stage}_{a}m{b}n");
        self.spawn_parts(commands, &parts, Transform::from_translation(offset), &cell);
        self.spawn_scenery(commands, meshes, materials, images, stage, a, b, offset);
        true
    }

    /// Static models placed by the cell's `_s00` layout. Placement
    /// positions are in a fixed stage frame whose origin is cell 50m50n:
    /// x = (B - 50) * 10000 + local, z = (A - 50) * 10000 + local (checked
    /// on cells 58m50n, 61m51n and 50m40n; see docs/WORLD.md).
    #[allow(clippy::too_many_arguments)]
    fn spawn_scenery(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        stage: u32,
        a: i32,
        b: i32,
        offset: Vec3,
    ) {
        let lot_path = format!("scr\\st{stage}\\etc\\st{stage}_{a}m{b}n_s00");
        let Ok(bytes) = self.vfs.read(&lot_path, RLAYOUT) else {
            return;
        };
        let records = match mtlot::parse(&bytes) {
            Ok(r) => r,
            Err(e) => {
                self.stats.missing.push(format!("{lot_path}: {e}"));
                return;
            }
        };
        let block = Vec3::new((b - 50) as f32 * 10000.0, 0.0, (a - 50) as f32 * 10000.0);
        for r in &records {
            let Some(p) = r.placement() else { continue };
            let id = p.name.split('_').next().unwrap_or("").to_ascii_lowercase();
            let Some(model_path) = self.model_names.get(&id).cloned() else {
                self.stats.instances_without_model += 1;
                continue;
            };
            let Some(parts) = self.model(&model_path, meshes, materials, images) else {
                self.stats.instances_without_model += 1;
                continue;
            };
            let local = Vec3::from(p.position) - block;
            // *Unresolved:* Euler order of mAngle; XYZ assumed until traced in the exe.
            let rotation = Quat::from_euler(EulerRot::XYZ, p.angle[0], p.angle[1], p.angle[2]);
            let transform = Transform {
                translation: offset + local * UNIT,
                rotation,
                scale: Vec3::from(p.scale),
            };
            self.stats.instances += 1;
            self.spawn_parts(commands, &parts, transform, &p.name);
        }
    }
}
