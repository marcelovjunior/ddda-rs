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
use bevy::render::render_resource::{AsBindGroup, Extent3d, Face, ShaderType, TextureDimension, TextureFormat};
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

/// Registers the stage material type and the system that hands those
/// materials to their entities.
pub struct ShadingPlugin;

impl Plugin for ShadingPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "dd_material.wgsl");
        app.add_plugins(MaterialPlugin::<DdMaterial>::default())
            .init_resource::<StageLighting>()
            .add_systems(Update, resolve_dd_materials);
    }
}

/// The original's lighting inputs for one moment (docs/SHADING.md). With
/// `enabled` false the stage materials use Bevy's PBR lighting instead.
#[derive(Resource, Clone, Debug, Default)]
pub struct StageLighting {
    pub enabled: bool,
    /// `fSHCoef[7]` as the pixel shaders read it.
    pub sh: [Vec4; 7],
    /// Unit direction to the sun (light) in the stage frame.
    pub sun_dir: Vec3,
    pub sun_color: Vec3,
    pub fog_color: Vec3,
    pub fog_density: f32,
    /// Fog start and end in game units (cm) of view depth.
    pub fog_start: f32,
    pub fog_end: f32,
}

/// Uniform of [`DdExt`]; layout mirrored in `dd_material.wgsl`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct DdParams {
    pub flags: u32,
    pub map_channel: u32,
    pub pad0: u32,
    pub pad1: u32,
    pub sh: [Vec4; 7],
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    pub fog_color: Vec4,
    pub fog_range: Vec4,
}

const FLAG_MAP: u32 = 1;
const FLAG_DD_LIGHTING: u32 = 2;

impl DdParams {
    fn with_lighting(mut self, l: &StageLighting) -> Self {
        if !l.enabled {
            return self;
        }
        self.flags |= FLAG_DD_LIGHTING;
        self.sh = l.sh;
        self.sun_dir = l.sun_dir.normalize_or_zero().extend(0.0);
        self.sun_color = l.sun_color.extend(0.0);
        self.fog_color = l.fog_color.extend(l.fog_density);
        let range = l.fog_end - l.fog_start;
        let inv = if range > 0.0 { 1.0 / range } else { 0.0 };
        self.fog_range = Vec4::new(l.fog_start, inv, 1.0 / UNIT, 0.0);
        self
    }
}

/// `StandardMaterial` plus the original's rules: transparency maps,
/// swing billboards and (with [`StageLighting`]) its lighting and fog.
pub type DdMaterial = ExtendedMaterial<StandardMaterial, DdExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct DdExt {
    #[uniform(100)]
    #[reflect(ignore)]
    pub params: DdParams,
    /// `tTransparencyMap` (FTransparencyMap), sampled on UV 1.
    #[texture(101)]
    #[sampler(102)]
    pub map: Option<Handle<Image>>,
}

impl MaterialExtension for DdExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://ddview/dd_material.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://ddview/dd_material.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "embedded://ddview/dd_material.wgsl".into()
    }
}

/// A stage material built once and shared by every entity that uses it.
pub struct DdSlot {
    pub material: DdMaterial,
    pub handle: OnceLock<Handle<DdMaterial>>,
}

/// Placed on entities whose material is a [`DdMaterial`]; replaced by
/// `MeshMaterial3d` in [`resolve_dd_materials`], which also fills in the
/// lighting (the loader only has the `StandardMaterial` asset store).
#[derive(Component, Clone)]
pub struct PendingDd(pub Arc<DdSlot>);

fn resolve_dd_materials(
    mut commands: Commands,
    pending: Query<(Entity, &PendingDd)>,
    mut assets: ResMut<Assets<DdMaterial>>,
    lighting: Res<StageLighting>,
) {
    for (e, p) in &pending {
        let h = p
            .0
            .handle
            .get_or_init(|| {
                let mut m = p.0.material.clone();
                m.extension.params = m.extension.params.with_lighting(&lighting);
                assets.add(m)
            })
            .clone();
        commands.entity(e).remove::<PendingDd>().insert(MeshMaterial3d(h));
    }
}

#[derive(Clone)]
pub enum PartMaterial {
    Standard(Handle<StandardMaterial>),
    Dd(Arc<DdSlot>),
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

/// Vertex colour attribute. Billboard meshes carry their swing pivots
/// (viewer units) with w = -1 for `dd_material.wgsl`. Other meshes carry
/// the layout's `Occlusion` byte in RGB (the pixel shaders multiply all
/// light by it, so it folds into the albedo) and the vertex alpha in A when
/// the material uses alpha. The vertex colour's own RGB is not used by
/// `FDiffuse` (the default variant, all terrain).
fn vertex_colors(
    model: &mtmod::Model<'_>,
    m: &mtmod::Mesh,
    layout: &InputLayout,
    v: &mtmod::Vertices,
    shading: Option<&Shading>,
    billboard: Option<&[([f32; 3], f32)]>,
) -> Option<Vec<[f32; 4]>> {
    if let Some(p) = billboard {
        return Some(p.iter().map(|(q, _)| [q[0] * UNIT, q[1] * UNIT, q[2] * UNIT, -1.0]).collect());
    }
    let n = v.positions.len();
    let occlusion: Option<Vec<f32>> = layout.element("Occlusion", 0).and_then(|e| {
        let raw = model.mesh_vertices(m).ok()?;
        let stride = m.vertex_stride as usize;
        (0..n)
            .map(|i| mtmod::read_component(raw, i * stride + e.offset as usize, e.ty, 0).ok())
            .collect()
    });
    let alpha = shading.is_some_and(Shading::uses_alpha) && v.colors.len() == n;
    if occlusion.is_none() && !alpha {
        return None;
    }
    Some(
        (0..n)
            .map(|i| {
                let o = occlusion.as_ref().map_or(1.0, |o| o[i]);
                [o, o, o, if alpha { v.colors[i][3] } else { 1.0 }]
            })
            .collect(),
    )
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
        let swm_bytes = self.swing_model(path);
        let swm = swm_bytes.as_deref().and_then(|b| mtswm::parse(b).ok());
        // Keyed by (material index, drawn two-sided as a stand-in for an
        // undecoded billboard node).
        let mut mat_cache: HashMap<(u16, bool), (PartMaterial, Option<Shading>)> = HashMap::new();
        let mut parts = Vec::new();
        for (mesh_index, m) in model.meshes.iter().enumerate() {
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
            // nDraw::MaterialNull meshes (e.g. `Scene_Material` proxies in
            // scenery models) draw nothing in the colour pass. *Whether they
            // cast shadows is not traced*; they are skipped.
            let null_class = mtmrl::jamcrc(b"nDraw::MaterialNull") & 0x7FFF_FFFF;
            let name = model.material_names.get(m.material as usize).map(String::as_str).unwrap_or("");
            if mrl
                .as_ref()
                .and_then(|l| l.by_name(name))
                .is_some_and(|mat| mat.class_hash & 0x7FFF_FFFF == null_class)
            {
                continue;
            }
            // Swing billboard pivots of this mesh (rSwingModel node with
            // flag 0x200 and 4-byte records), when the counts agree.
            let node = swm.as_ref().and_then(|s| s.nodes.get(mesh_index).map(|n| (s, n)));
            let billboard = node.filter(|(_, n)| n.is_billboard()).and_then(|(s, n)| {
                s.pivots(mesh_index)
                    .filter(|p| p.len() == v.positions.len() && usize::from(n.vertex_count) == v.positions.len())
            });
            let undecoded_billboard = billboard.is_none() && node.is_some_and(|(_, n)| n.is_billboard());
            let (material, shading) = mat_cache
                .entry((m.material, undecoded_billboard))
                .or_insert_with(|| {
                    let name = model
                        .material_names
                        .get(m.material as usize)
                        .cloned()
                        .unwrap_or_default();
                    let shading = mrl
                        .as_ref()
                        .and_then(|l| l.by_name(&name).map(|mat| mtmrl::shading::describe(mat, l, mfx)));
                    (
                        self.material(shading.as_ref(), undecoded_billboard, materials, images),
                        shading,
                    )
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
            if let Some(colors) = vertex_colors(&model, m, layout, &v, shading.as_ref(), billboard.as_deref()) {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
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

    /// The model's `rSwingModel`: `...\etc\<name>_s00` for scenery
    /// (`...\model\<name>`), else the model's own path (`fmfore_*`).
    fn swing_model(&self, model_path: &str) -> Option<Vec<u8>> {
        let ty = mtarc::type_hash("rSwingModel");
        let (dir, name) = model_path.rsplit_once('\\')?;
        let etc = dir
            .strip_suffix("\\model")
            .map(|d| format!("{d}\\etc\\{name}_s00"));
        etc.and_then(|p| self.vfs.read(&p, ty).ok())
            .or_else(|| self.vfs.read(model_path, ty).ok())
    }

    /// The Bevy material for one MRL material (`None`: no MRL entry).
    /// Rules and their evidence: `mtmrl::shading` and docs/SHADING.md.
    fn material(
        &mut self,
        shading: Option<&Shading>,
        two_sided: bool,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        // Roughness and reflectance only matter without StageLighting
        // (Bevy PBR fallback); they are viewer placeholders.
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
        if two_sided {
            // Billboard cards whose swing records are not decoded (8-byte
            // nodes of the distant fmfore models): not turned to the camera,
            // so drawn from both sides to stay visible.
            sm.double_sided = true;
            sm.cull_mode = None;
        }
        // Alpha test from the material header (Material::alpha_test,
        // traced): discard unless alpha > reference. Bevy keeps
        // alpha >= cutoff, so the cutoff sits half a step above the
        // reference (alpha comes from 8-bit textures and vertex bytes).
        let test = s.alpha_test.map(|t| t.reference_f32() + 0.5 / 255.0);
        sm.alpha_mode = if !s.uses_alpha() {
            AlphaMode::Opaque
        } else if s.blends_alpha() {
            AlphaMode::Blend
        } else if let Some(cutoff) = test {
            AlphaMode::Mask(cutoff)
        } else {
            // Alpha computed but neither tested nor blended: no effect.
            AlphaMode::Opaque
        };
        if let Some([r0, r1]) = s.uv_transform {
            sm.uv_transform = bevy::math::Affine2::from_cols(
                Vec2::new(r0[0], r1[0]),
                Vec2::new(r0[1], r1[1]),
                Vec2::new(r0[3], r1[3]),
            );
        }
        let mut params = DdParams::default();
        let mut map = None;
        if let AlphaSource::Map { texture, channel, .. } = &s.alpha {
            if let Some(h) = self.texture(texture, images) {
                params.flags |= FLAG_MAP;
                params.map_channel = u32::from(*channel);
                map = Some(h);
            }
        }
        PartMaterial::Dd(Arc::new(DdSlot {
            material: DdMaterial {
                base: sm,
                extension: DdExt { params, map },
            },
            handle: OnceLock::new(),
        }))
    }

    fn spawn_parts(&mut self, commands: &mut Commands, parts: &[Part], transform: Transform, name: &str) {
        for p in parts {
            self.stats.meshes += 1;
            self.stats.triangles += p.triangles;
            let mut e = commands.spawn((Mesh3d(p.mesh.clone()), transform, Name::new(name.to_string())));
            match &p.material {
                PartMaterial::Standard(h) => e.insert(MeshMaterial3d(h.clone())),
                PartMaterial::Dd(slot) => e.insert(PendingDd(slot.clone())),
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
            // mAngle is applied about the world X, then Y, then Z axis
            // (extrinsic XYZ, R = Rz * Ry * Rx): measured on om0529 in
            // st100_58m49n (angles 2.23, 0.70, 3.25 rad), whose log only
            // lines up with the live capture m1_lighting_02 in this order
            // (all six intrinsic orders were rendered). *Not traced in the
            // exe.*
            let rotation = Quat::from_euler(EulerRot::ZYX, p.angle[2], p.angle[1], p.angle[0]);
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
