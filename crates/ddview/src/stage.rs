//! Converts stage cells (MOD + MRL + TEX from the user's install) into
//! Bevy meshes, materials and images, and places each cell's static
//! scenery from its `_s00` layout (`mtlot`).
//!
//! Placement follows `docs/WORLD.md`: cell `st<S>_<A>m<B>n` sits at
//! X = (B - B0) * 10000, Z = (A - A0) * 10000 game units from the origin
//! cell (A0, B0). Game units are taken as centimetres and scaled by 0.01
//! (*unresolved:* unit not yet confirmed). Axes are used as stored (Y up);
//! the reference screenshots show no mirroring.
//!
//! Materials follow the original's shading rules as far as they are traced
//! (`mtmrl::shading`, docs/SHADING.md): albedo and diffuse colour
//! constants, UV offsets, cull mode from the rasterizer state, and the
//! terrain layers. A cell's terrain is opaque base meshes plus overlay
//! meshes on exactly the same triangles (checked on four cells: every
//! overlay triangle coincides with a base triangle) drawn with
//! `BSBlendAlpha` (SrcAlpha, InvSrcAlpha), depth test without write, and
//! alpha = albedo alpha x vertex alpha [x transparency map] x
//! `fTransparency`. *Difference:* the original blends in sRGB (its pixel
//! shaders encode to sRGB before the blend), Bevy blends in linear.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, Face, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use ddvfs::Vfs;
use mtmrl::shading::{AlphaSource, Shading};
use mtshader::{Cull, InputLayout, ShaderPackage};

pub const RMODEL: u32 = 0x58A1_5856;
pub const RMATERIAL: u32 = 0x2749_C8A8;
pub const RTEXTURE: u32 = 0x241F_5DEB;
pub const RLAYOUT: u32 = 0x1530_2EF4;
pub const UNIT: f32 = 0.01;

#[derive(Default)]
pub struct Stats {
    pub cells: usize,
    pub far_cells: usize,
    pub water_models: usize,
    pub instances: usize,
    pub instances_without_model: usize,
    pub meshes: usize,
    pub triangles: usize,
    pub textures: usize,
    pub missing: Vec<String>,
}

/// Registers the material type for transparency-map layers and the
/// system that hands those materials to their entities.
pub struct ShadingPlugin;

impl Plugin for ShadingPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "transparency_map.wgsl");
        app.add_plugins(MaterialPlugin::<LayerMaterial>::default())
            .add_systems(Update, resolve_layer_materials);
    }
}

/// `StandardMaterial` whose alpha is also multiplied by a transparency map
/// (`FTransparencyMap`) sampled with the mesh's second UV set.
pub type LayerMaterial = ExtendedMaterial<StandardMaterial, TransparencyMap>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TransparencyMap {
    /// Channel of the map that scales the alpha (0 = R).
    #[uniform(100)]
    pub channel: u32,
    #[texture(101)]
    #[sampler(102)]
    pub map: Handle<Image>,
}

impl MaterialExtension for TransparencyMap {
    fn fragment_shader() -> ShaderRef {
        "embedded://ddview/transparency_map.wgsl".into()
    }
}

/// A layer material built once and shared by every entity that uses it.
pub struct LayerSlot {
    pub material: LayerMaterial,
    pub handle: OnceLock<Handle<LayerMaterial>>,
}

/// Placed on entities whose material is a [`LayerMaterial`]; replaced by
/// `MeshMaterial3d` in [`resolve_layer_materials`] (the loader only has
/// the `StandardMaterial` asset store).
#[derive(Component, Clone)]
pub struct PendingLayer(pub Arc<LayerSlot>);

fn resolve_layer_materials(
    mut commands: Commands,
    pending: Query<(Entity, &PendingLayer)>,
    mut assets: ResMut<Assets<LayerMaterial>>,
) {
    for (e, p) in &pending {
        let h = p.0.handle.get_or_init(|| assets.add(p.0.material.clone())).clone();
        commands.entity(e).remove::<PendingLayer>().insert(MeshMaterial3d(h));
    }
}

#[derive(Clone)]
pub enum PartMaterial {
    Standard(Handle<StandardMaterial>),
    Layer(Arc<LayerSlot>),
}

/// One drawable piece of a converted model.
#[derive(Clone)]
pub struct Part {
    pub mesh: Handle<Mesh>,
    pub material: PartMaterial,
    pub triangles: usize,
}

/// Index into `Vertices::uvs` (the layout's `TexCoord` elements in order)
/// of a UV alias such as `UV_Extend`: the `TexCoord` at the alias's offset.
fn uv_slot(layout: &InputLayout, alias: &str) -> Option<usize> {
    let offset = layout.elements.iter().find(|e| e.name == alias)?.offset;
    layout
        .elements
        .iter()
        .filter(|e| e.name == "TexCoord")
        .position(|e| e.offset == offset)
}

pub struct Loader<'a> {
    pub vfs: &'a Vfs,
    pub mfx: &'a ShaderPackage,
    pub textures: HashMap<String, Option<Handle<Image>>>,
    /// Converted models by resource path, shared by every instance.
    pub models: HashMap<String, Option<Vec<Part>>>,
    /// Model resource path by its last path component (`om0528`).
    pub model_names: HashMap<String, String>,
    /// Material for every water model. *Placeholder*: the water shader
    /// (`XfB_N__EW...` materials) is not read yet.
    pub water: Option<Handle<StandardMaterial>>,
    pub stats: Stats,
}

/// Builds a Bevy image from a TEX file, sRGB for the formats the exe
/// flags as colour (`mttex::is_srgb`), linear otherwise.
pub fn tex_to_image(bytes: &[u8]) -> Result<Image, String> {
    let t = mttex::parse(bytes).map_err(|e| e.to_string())?;
    let h = &t.header;
    if h.shape != mttex::Shape::Flat {
        return Err(format!("{:?} textures not supported in the viewer", h.shape));
    }
    let srgb = mttex::is_srgb(h.format_code);
    let format = match (h.format, srgb) {
        (mttex::D3dFormat::Dxt1, true) => TextureFormat::Bc1RgbaUnormSrgb,
        (mttex::D3dFormat::Dxt1, false) => TextureFormat::Bc1RgbaUnorm,
        (mttex::D3dFormat::Dxt3, true) => TextureFormat::Bc2RgbaUnormSrgb,
        (mttex::D3dFormat::Dxt3, false) => TextureFormat::Bc2RgbaUnorm,
        (mttex::D3dFormat::Dxt5, true) => TextureFormat::Bc3RgbaUnormSrgb,
        (mttex::D3dFormat::Dxt5, false) => TextureFormat::Bc3RgbaUnorm,
        (mttex::D3dFormat::A8R8G8B8 | mttex::D3dFormat::X8R8G8B8, true) => TextureFormat::Bgra8UnormSrgb,
        (mttex::D3dFormat::A8R8G8B8 | mttex::D3dFormat::X8R8G8B8, false) => TextureFormat::Bgra8Unorm,
        (f, _) => return Err(format!("format {f:?} not supported in the viewer")),
    };
    let uncompressed = matches!(format, TextureFormat::Bgra8UnormSrgb | TextureFormat::Bgra8Unorm);
    if !uncompressed && (h.width % 4 != 0 || h.height % 4 != 0) {
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
    pub fn texture(&mut self, path: &str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
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
        let mut mat_cache: HashMap<u16, (PartMaterial, Option<Shading>)> = HashMap::new();
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
            let (material, shading) = mat_cache
                .entry(m.material)
                .or_insert_with(|| {
                    let name = model
                        .material_names
                        .get(m.material as usize)
                        .cloned()
                        .unwrap_or_default();
                    let shading = mrl
                        .as_ref()
                        .and_then(|l| l.by_name(&name).map(|mat| mtmrl::shading::describe(mat, l, mfx)));
                    (self.material(shading.as_ref(), materials, images), shading)
                })
                .clone();
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
            // UV set of the albedo map (FUVAlbedoMap) as UV 0; the
            // transparency map's set (FUVTransparencyMap) as UV 1.
            let slot = |alias: &str| uv_slot(layout, alias).filter(|&k| !v.uvs[k].is_empty());
            let albedo = shading.as_ref().and_then(|s| slot(&s.albedo_uv)).unwrap_or(0);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, v.uvs[albedo].clone());
            if let Some(AlphaSource::Map { uv, .. }) = shading.as_ref().map(|s| &s.alpha) {
                let k = slot(uv).unwrap_or(albedo);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, v.uvs[k].clone());
            }
            // Vertex alpha scales the alpha of materials that use it; the
            // vertex colour's RGB is not used by `FDiffuse` (the default
            // variant, all terrain), so it is left white here.
            if shading.as_ref().is_some_and(Shading::uses_alpha) && !v.colors.is_empty() {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_COLOR,
                    v.colors.iter().map(|c| [1.0, 1.0, 1.0, c[3]]).collect::<Vec<_>>(),
                );
            }
            mesh.insert_indices(Indices::U32(
                tris.iter().flatten().map(|&i| u32::from(i)).collect(),
            ));
            parts.push(Part {
                mesh: meshes.add(mesh),
                material,
                triangles: tris.len(),
            });
        }
        Some(parts)
    }

    /// The Bevy material for one MRL material (`None`: no MRL entry).
    /// Rules and their evidence: `mtmrl::shading` and docs/SHADING.md.
    fn material(
        &mut self,
        shading: Option<&Shading>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        // Roughness and reflectance are viewer placeholders: the original
        // lights with its own model (light pre-pass buffer, SH ambient),
        // not a PBR one.
        let mut sm = StandardMaterial {
            perceptual_roughness: 0.9,
            metallic: 0.0,
            reflectance: 0.2,
            ..default()
        };
        let Some(s) = shading else {
            return PartMaterial::Standard(materials.add(sm));
        };
        if let Some(p) = s.textures.get("tAlbedoMap") {
            sm.base_color_texture = self.texture(p, images);
        }
        // colour = albedo * fAlbedoColor * light * fDiffuseColor: both
        // constants scale the same product, so they combine into the base
        // colour (linear).
        let c: [f32; 3] = std::array::from_fn(|k| s.albedo_color[k] * s.diffuse_color[k]);
        let alpha = if s.uses_alpha() { s.transparency } else { 1.0 };
        sm.base_color = Color::linear_rgba(c[0], c[1], c[2], alpha);
        if s.feature("FSpecular") == Some("FSpecularDisable") {
            // The pixel shaders of this variant have no specular term.
            sm.reflectance = 0.0;
        }
        match s.raster.map(|r| r.cull) {
            Some(Cull::None) => {
                // The original flips the normal of back faces (vFace).
                sm.double_sided = true;
                sm.cull_mode = None;
            }
            Some(Cull::Front) => sm.cull_mode = Some(Face::Front),
            _ => sm.cull_mode = Some(Face::Back),
        }
        sm.alpha_mode = if !s.uses_alpha() {
            AlphaMode::Opaque
        } else if s.blends_alpha() {
            AlphaMode::Blend
        } else {
            // Alpha test: the reference value is CBROPTest at run time,
            // not traced; 0.5 is a placeholder.
            // Alpha-tested foliage (`*_NUKI` leaf crowns) is RSMesh (cull
            // back), but its cards all lie in parallel planes facing +Z
            // (checked on om0500, om0501, om0531, om0532): the original
            // turns them to the camera, which is not traced or done here.
            // Drawn two-sided instead so they stay visible, as the
            // references show them from above (ref001) and below (ref004).
            sm.double_sided = true;
            sm.cull_mode = None;
            AlphaMode::Mask(0.5)
        };
        if let Some([r0, r1]) = s.uv_transform {
            sm.uv_transform = bevy::math::Affine2::from_cols(
                Vec2::new(r0[0], r1[0]),
                Vec2::new(r0[1], r1[1]),
                Vec2::new(r0[3], r1[3]),
            );
        }
        if let AlphaSource::Map { texture, channel, .. } = &s.alpha {
            if let Some(map) = self.texture(texture, images) {
                return PartMaterial::Layer(Arc::new(LayerSlot {
                    material: LayerMaterial {
                        base: sm,
                        extension: TransparencyMap {
                            channel: u32::from(*channel),
                            map,
                        },
                    },
                    handle: OnceLock::new(),
                }));
            }
        }
        PartMaterial::Standard(materials.add(sm))
    }

    fn spawn_parts(&mut self, commands: &mut Commands, parts: &[Part], transform: Transform, name: &str) {
        for p in parts {
            self.stats.meshes += 1;
            self.stats.triangles += p.triangles;
            let mut e = commands.spawn((Mesh3d(p.mesh.clone()), transform, Name::new(name.to_string())));
            match &p.material {
                PartMaterial::Standard(h) => e.insert(MeshMaterial3d(h.clone())),
                PartMaterial::Layer(slot) => e.insert(PendingLayer(slot.clone())),
            };
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

    /// The simplified distant model of a cell (`fmfore_<A>m<B>n`, cell-local
    /// like the terrain), for cells outside the detailed radius.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_far_cell(
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
        let path = format!("scr\\st{stage}\\model\\fm_m{}\\fmfore_{a}m{b}n", a / 10 * 10);
        let Some(parts) = self.model(&path, meshes, materials, images) else {
            return false;
        };
        self.stats.far_cells += 1;
        self.spawn_parts(commands, &parts, Transform::from_translation(offset), &path);
        true
    }

    /// Every water model of the stage (`scr\st<S>\model\water\`: the sea plane
    /// and the river, lake and wave models), all in the stage frame (origin
    /// cell 50m50n). `origin` is the viewer's origin cell (A0, B0).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_water(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        stage: u32,
        origin: (i32, i32),
    ) {
        let prefix = format!("scr\\st{stage}\\model\\water\\");
        let mut paths: Vec<String> = self
            .vfs
            .paths_of_type(RMODEL)
            .filter(|p| p.starts_with(&prefix))
            .map(str::to_string)
            .collect();
        paths.sort();
        let water = self
            .water
            .get_or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.06, 0.16, 0.22, 0.9),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 0.08,
                    reflectance: 0.5,
                    ..default()
                })
            })
            .clone();
        let shift = -Vec3::new(
            (origin.1 - 50) as f32 * 10000.0,
            0.0,
            (origin.0 - 50) as f32 * 10000.0,
        ) * UNIT;
        for p in &paths {
            let Some(parts) = self.model(p, meshes, materials, images) else {
                continue;
            };
            let parts: Vec<Part> = parts
                .into_iter()
                .map(|q| Part {
                    material: PartMaterial::Standard(water.clone()),
                    ..q
                })
                .collect();
            self.stats.water_models += 1;
            self.spawn_parts(commands, &parts, Transform::from_translation(shift), p);
        }
    }
}
