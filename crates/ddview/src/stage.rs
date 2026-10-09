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

#[path = "post.rs"]
pub mod post;

#[path = "transparent_sort.rs"]
pub mod transparent_sort;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, Mesh, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, CompareFunction, Extent3d, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
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
        bevy::asset::embedded_asset!(app, "dd_post.wgsl");
        // dd_material.wgsl imports the sky scattering (water and the
        // environment reflection), so it loads with the stage shading:
        // --event and --lineup run without the sky plugin.
        bevy::shader::load_shader_library!(app, "sky_scatter.wgsl");
        app.add_plugins(MaterialPlugin::<DdMaterial>::default())
            .add_plugins(transparent_sort::OriginalSortPlugin)
            .add_plugins(bevy::core_pipeline::fullscreen_material::FullscreenMaterialPlugin::<post::DdPost>::default())
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
    /// Game clock (ms into the day) of the capture, if it has one.
    pub clock_ms: Option<u32>,
    /// The night spot light (`uSkySpotLight`), if lit.
    pub spot: Option<SpotLight>,
}

/// A spot light in the stage frame (cm, the viewer's origin cell), with
/// the deferred spot shader's attenuation parameters.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpotLight {
    pub pos: Vec3,
    pub dir: Vec3,
    pub color: Vec3,
    /// start, 1 / (end - start), cos outer, 1 / (cos inner - cos outer)
    pub atten: Vec4,
    pub falloff: f32,
}

/// Uniform of [`DdExt`]; layout mirrored in `dd_material.wgsl`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct DdParams {
    pub flags: u32,
    pub map_channel: u32,
    pub alpha_compare: u32,
    pub alpha_reference: u32,
    pub sh: [Vec4; 7],
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    pub fog_color: Vec4,
    pub fog_range: Vec4,
    /// xyz: fReflectiveColor, w: Fresnel F0 (FReflect* materials).
    pub reflect: Vec4,
    /// xyz: the environment cube's colour (`EnvCubeColors`), w: its index.
    pub env: Vec4,
    /// Spot light: position (viewer units, w falloff), direction, colour
    /// (w 1 when lit), attenuation (see [`SpotLight`]).
    pub spot_pos: Vec4,
    pub spot_dir: Vec4,
    pub spot_color: Vec4,
    pub spot_atten: Vec4,
    /// rgb: the constant light of `FLAG_CONST_LIGHT` materials.
    pub const_light: Vec4,
    /// rgb: `fAlbedoBlendColor.rgb * .w * fDiffuseColor` (`FLAG_ALBEDO_ADD`).
    pub albedo_blend: Vec4,
    /// rgb: `fSpecularColor`, w: `fShininess` (`FLAG_CHARACTER`).
    pub spec: Vec4,
}

const FLAG_MAP: u32 = 1;
const FLAG_DD_LIGHTING: u32 = 2;
const FLAG_REFLECT: u32 = 4;
const FLAG_SPEC_MAP: u32 = 8;
/// Drawn into the shadow map only (draw mode without the main pass).
const FLAG_SHADOW_ONLY: u32 = 16;
/// The environment reflection evaluates the stage sky ([`SkyEnv`]).
const FLAG_SKY_ENV: u32 = 32;
/// The environment reflection samples an environment cube (`env_cube`).
const FLAG_ENV_CUBE: u32 = 64;
/// No light buffer: unlit, the vertex colour is the light (`forward_unlit`).
const FLAG_NO_LIGHT_BUFFER: u32 = 128;
/// Alpha blended: keeps its alpha (others write the view depth there).
const FLAG_BLENDED: u32 = 256;
/// Lit by a constant colour (`const_light`) instead of the stage lights.
const FLAG_CONST_LIGHT: u32 = 512;
/// `FAlbedoMapAdd` with `FUVViewNormal`: the albedo-blend map added on the
/// view-normal UV (`albedo_blend`).
const FLAG_ALBEDO_ADD: u32 = 1024;
/// Character pixel shaders' specular: the sun's Blinn-Phong lobe, the
/// environment cube (`FReflectCubeMapShadowLight`), Schlick Fresnel.
const FLAG_CHARACTER: u32 = 2048;
const FLAG_ALPHA_TEST: u32 = 4096;

/// Marks the sky's cloud domes; they follow the camera horizontally.
#[derive(Component)]
pub struct SkyFollow;

/// Colour of each of the five environment cubes `DDCube<i>_CM`, made from
/// `DDBaseCube<i>_CM` by the weather manager's `mEnvMapBaseColor[i]` x
/// lerp(1, `mEnvMapColor[i]`.rgb, .a) (see `sky::StageSky::env_colors`).
#[derive(Resource, Clone, Debug)]
pub struct EnvCubeColors(pub [Vec3; 5]);

impl DdParams {
    /// The parameters relit by `l`: only materials drawn with the game's
    /// lighting change (the clock's relighting, clock.rs).
    pub(crate) fn relit(self, l: &StageLighting) -> Self {
        if self.flags & FLAG_DD_LIGHTING != 0 {
            self.with_lighting(l)
        } else {
            self
        }
    }

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
        if let Some(s) = l.spot {
            self.spot_pos = (s.pos * UNIT).extend(s.falloff);
            self.spot_dir = s.dir.normalize_or_zero().extend(0.0);
            self.spot_color = s.color.extend(1.0);
            self.spot_atten = s.atten;
        }
        self
    }
}

/// The stage's lighting from the data at a game clock and weather, as
/// main.rs's `resolve_lighting` builds it without a capture: the sky
/// scheduler's light units at the clock's sun and the weather's fog
/// (docs/SHADING.md, "Lighting from the data"). No night spot light (it
/// follows the player). `None` when the stage has no sky scheduler.
pub fn data_lighting(vfs: &Vfs, stage: u32, clock_ms: u32, weather: usize) -> Option<StageLighting> {
    let sky = crate::sky::StageSky::load(vfs, stage, weather)?;
    let fog = crate::sky::weather_fog(vfs, stage, weather, clock_ms / 60_000)?;
    let sun = mtweather::sky::sun_direction(&sky.sky, clock_ms);
    let (hemis, lights) = sky.light_values(sun);
    let mut l = crate::lighting_from_units(&hemis, &lights, &fog);
    l.clock_ms = Some(clock_ms);
    Some(l)
}

/// The colour the albedo-blend map adds with (`FAlbedoMapAdd` on
/// `FUVViewNormal`): `fAlbedoBlendColor.rgb * .w`, times `fDiffuseColor`
/// (which scales the lit albedo; the base colour carries it for the albedo
/// map, so the added term takes it too). `None` when it adds nothing, as
/// for most `model\pl` materials (w or rgb 0).
fn albedo_add(s: &Shading) -> Option<Vec4> {
    let b = s.albedo_blend_color;
    let d = s.diffuse_color;
    (s.feature("FUVAlbedoBlendMap") == Some("FUVViewNormal") && b[3] != 0.0 && b[..3] != [0.0; 3])
        .then(|| Vec4::new(b[0] * b[3] * d[0], b[1] * b[3] * d[1], b[2] * b[3] * d[2], 0.0))
}

/// `StandardMaterial` plus the original's rules: transparency maps,
/// swing billboards and (with [`StageLighting`]) its lighting and fog.
pub type DdMaterial = ExtendedMaterial<StandardMaterial, DdExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(DdPipelineKey)]
pub struct DdExt {
    /// Authored test/write/compare bits (MFX depth state word zero).
    pub depth: Option<u8>,
    #[uniform(100)]
    #[reflect(ignore)]
    pub params: DdParams,
    /// `tTransparencyMap` (FTransparencyMap), sampled on UV 1.
    #[texture(101)]
    #[sampler(102)]
    pub map: Option<Handle<Image>>,
    /// `tSpecularMap` (FSpecularMap), its G channel masks the reflection.
    #[texture(103)]
    #[sampler(104)]
    pub spec: Option<Handle<Image>>,
    /// The sky for the environment reflection (see [`SkyEnv`]).
    #[uniform(105)]
    #[reflect(ignore)]
    pub sky: SkyEnvParams,
    #[texture(106, sample_type = "float", filterable = false)]
    pub sky_rayleigh: Handle<Image>,
    #[texture(107, sample_type = "float", filterable = false)]
    pub sky_mie: Handle<Image>,
    /// `DDBaseCube<i>_CM` for an `FReflectCubeMap` on `DDCube<i>_CM`.
    #[texture(108, dimension = "cube")]
    #[sampler(109)]
    pub env_cube: Option<Handle<Image>>,
    /// `tAlbedoBlendMap` (`FLAG_ALBEDO_ADD`).
    #[texture(110)]
    #[sampler(111)]
    pub albedo_blend: Option<Handle<Image>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DdPipelineKey {
    depth: Option<u8>,
}

impl From<&DdExt> for DdPipelineKey {
    fn from(material: &DdExt) -> Self { Self { depth: material.depth } }
}

/// MFX compare is forward Z; the viewer's depth buffer is reverse Z.
fn imported_depth(bits: u8) -> (bool, CompareFunction) {
    use CompareFunction::*;
    let compare = if bits & 1 == 0 { Always } else {
        [Never, Greater, Equal, GreaterEqual, Less, NotEqual, LessEqual, Always][((bits >> 2) & 7) as usize]
    };
    (bits & 2 != 0, compare)
}

/// `CBSky` and the camera for the sky evaluated in the stage materials;
/// layout mirrored in `dd_material.wgsl`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct SkyEnvParams {
    pub cb: [Vec4; 11],
    pub eye: Vec4,
}

/// The stage's sky (inserted by the sky dome's spawn): the stage materials
/// reflect it (`FReflectCubeMap`, the sky's reflection cube).
#[derive(Resource, Clone, Default)]
pub struct SkyEnv {
    pub params: SkyEnvParams,
    pub rayleigh: Handle<Image>,
    pub mie: Handle<Image>,
}

impl MaterialExtension for DdExt {
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let (Some(bits), Some(depth)) = (key.bind_group_data.depth, descriptor.depth_stencil.as_mut()) {
            let (write, compare) = imported_depth(bits);
            depth.depth_write_enabled = Some(write);
            depth.depth_compare = Some(compare);
        }
        Ok(())
    }
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
    sky: Option<Res<SkyEnv>>,
    env_colors: Option<Res<EnvCubeColors>>,
) {
    for (e, p) in &pending {
        let h = p
            .0
            .handle
            .get_or_init(|| {
                let mut m = p.0.material.clone();
                m.extension.params = m.extension.params.with_lighting(&lighting);
                if m.extension.params.flags & FLAG_ENV_CUBE != 0 {
                    let i = (m.extension.params.env.w as usize).min(4);
                    let c = env_colors.as_ref().map_or(Vec3::ONE, |e| e.0[i]);
                    m.extension.params.env = c.extend(i as f32);
                }
                if let Some(sky) = &sky {
                    m.extension.params.flags |= FLAG_SKY_ENV;
                    m.extension.sky = sky.params;
                    m.extension.sky_rayleigh = sky.rayleigh.clone();
                    m.extension.sky_mie = sky.mie.clone();
                }
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
    /// The primitive's LOD mask (bit 0 near, 1 middle, 2 far; 255 all).
    pub lod: u8,
    /// Drawn into the shadow map (draw mode has a shadow-cast pass bit).
    pub casts_shadow: bool,
    /// The model's middle and far LOD distances (cm).
    pub lod_dist: [f32; 2],
}

/// Pass mask of the scene's main colour pass (`FUN_00fedb80` pass masks:
/// 1 for the scene and most named passes, also the reset value in
/// 0x00fed710; 2 reflections, 0x10 environment cubes, 8 or 0x200 the
/// shadow casters).
const PASS_MAIN: u16 = 0x1;
const PASS_SHADOW: u16 = 0x8 | 0x200;

/// LOD level (1 near, 2 middle, 4 far) of a model at `dist` cm from the
/// camera: the far level beyond its far distance, the middle one beyond
/// its middle distance, both scaled by 2 for ViewRange FAR; FARTHEST
/// always draws the near level.
// Translated from 0x00ea5320 (DDDA.exe 2017-12-13 Steam)
pub fn lod_level(dist: f32, mid: f32, low: f32, view_range: u32) -> u8 {
    if view_range == 3 {
        return 1;
    }
    let k = if view_range == 2 { 2.0 } else { 1.0 };
    if low * k < dist {
        4
    } else if mid * k < dist {
        2
    } else {
        1
    }
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

/// Materials drawn without the light buffer: every stage100 pixel shader
/// with the sea's sampler set (albedo, normal, detail normal, specular,
/// environment cube; e.g. file offset 0x4A0980 of sc\dx9\stage100) and
/// every blended water shader (FTransparencyVolume, with tDepthMap) reads no
/// tLightAccumulationTexture. Their colour is albedo x fAlbedoColor x
/// fDiffuseColor x v1.rgb x v2.w plus the reflection x mean(v1.rgb) x v2.w;
/// no vertex shader outside the grass reads fSHCoef or a light, so v1 and
/// v2.w are *inferred* to be the layout's VertexColor and Occlusion
/// (IANonSkinTBNC carries both): unlit, baked colours.
fn forward_unlit(s: &Shading) -> bool {
    (s.blends_alpha() && s.feature("FTransparency") == Some("FTransparencyVolume"))
        || (s.feature("FBump") == Some("FBumpDetailNormalMap")
            && s.feature("FReflect") == Some("FReflectCubeMap")
            && !s.textures.keys().any(|k| k.starts_with("tDDMaterial")))
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
    // Unlit forward materials multiply by the vertex colour's RGB as well.
    let rgb = shading.is_some_and(forward_unlit) && v.colors.len() == n;
    if occlusion.is_none() && !alpha && !rgb {
        return None;
    }
    Some(
        (0..n)
            .map(|i| {
                let o = occlusion.as_ref().map_or(1.0, |o| o[i]);
                let c = if rgb { v.colors[i] } else { [1.0; 4] };
                [c[0] * o, c[1] * o, c[2] * o, if alpha { v.colors[i][3] } else { 1.0 }]
            })
            .collect(),
    )
}

/// One landmark of the table at 0x01873550 (15 entries of 0x40 bytes):
/// +0x08 switch radius, +0x10 position (stage frame, cm), +0x30 / +0x34 an
/// optional gate on the story progress `*(sGameSys) + 0x30`: mode 1 draws
/// when progress >= value, mode 2 when progress < value (entries 7 and 5,
/// two versions of the same landmark). The float at +0x24 of entries 11-13
/// is not read by the placement code.
// Translated from 0x00c61bd0 (DDDA.exe 2017-12-13 Steam)
struct Landmark {
    radius: f32,
    position: [f32; 3],
    gate: Option<(i32, u8)>,
}

const LANDMARKS: [Landmark; 15] = [
    Landmark { radius: 25000.0, position: [45000.0, 30000.0, 145000.0], gate: None },
    Landmark { radius: 25000.0, position: [20000.0, 30000.0, 135000.0], gate: None },
    Landmark { radius: 20000.0, position: [0.0, 35000.0, 105000.0], gate: None },
    Landmark { radius: 30000.0, position: [-17000.0, 33000.0, 170000.0], gate: None },
    Landmark { radius: 22000.0, position: [-93000.0, 46000.0, 111000.0], gate: None },
    Landmark { radius: 35000.0, position: [35800.0, 35200.0, 51400.0], gate: Some((0x1E78, 2)) },
    Landmark { radius: 35000.0, position: [14800.0, 35200.0, 51400.0], gate: None },
    Landmark { radius: 35000.0, position: [35800.0, 35200.0, 51400.0], gate: Some((0x1E78, 1)) },
    Landmark { radius: 20000.0, position: [-43000.0, 39000.0, 53000.0], gate: None },
    Landmark { radius: 30000.0, position: [33000.0, 38000.0, -10000.0], gate: None },
    Landmark { radius: 20000.0, position: [32500.0, 40400.0, -31250.0], gate: None },
    Landmark { radius: 25000.0, position: [63500.0, 41500.0, -54200.0], gate: None },
    Landmark { radius: 43000.0, position: [141200.0, 33800.0, 22800.0], gate: None },
    Landmark { radius: 20000.0, position: [-76439.0, 47200.0, 22666.0], gate: None },
    Landmark { radius: 20000.0, position: [-101490.0, 53800.0, -1200.0], gate: None },
];

/// Area of each stage-100 cell for the low models: row `A - 40`, column
/// `B - 30` (checked against the cell grid's outline), '.' none, '3' / 'a' /
/// 'c' / 'd' / 'z' the values 3 / 10 / 12 / 13 / 0x23.
// Translated from the table at 0x014e8d00 (DDDA.exe 2017-12-13 Steam)
const LOW_MODEL_AREAS: [&str; 32] = [
    "........................................",
    "........................................",
    ".......................ddddd............",
    ".......................ddddd............",
    "......................ddddddd...........",
    ".....................ddddddddd..........",
    ".....................ddddddddd..........",
    ".....ccccccc.........ddddddddd..........",
    ".....ccccccc.......ddddddd..ddd.........",
    ".....cccccccccccccddddddd...ddddd.......",
    ".....cccccccccccccddddddd....ddddd......",
    "......ccccccccccccddddddd......dd.......",
    "......ccccccccccccaaaaaaa......dd.......",
    "......ccccccccccccaaaaaaaa......d.......",
    ".......cccccccccccaaaaaaaa..............",
    ".......cccccccccaaaaaaaaaa..............",
    ".........cccc.ccaaaaaaaaaa..............",
    "..................aaaaaaa...............",
    "..................aaaaaa................",
    "..................333333................",
    "..........zzzzzz..3333333...............",
    "..........zzzzzzzz33333333..............",
    "..........zzzzzzzz33333333..............",
    "..........zzzzzzzz33333333..............",
    "..........zzzzzzzz33333333..............",
    ".........zzzzzzzzz33333333..............",
    ".........zzzzzzzzz3333..................",
    ".........zzzzzzzzz3333..................",
    ".........zzzzzzzzz3333..................",
    ".........zzzzzzzzz3333..................",
    ".........zzzzzzzzz3333..................",
    ".........zzzz...........................",
];

/// `st100_area%02d` numbers of the six low-model slots.
// Translated from the table at 0x014e9228 (DDDA.exe 2017-12-13 Steam)
const LOW_MODEL_NUMBERS: [u32; 6] = [0, 3, 5, 10, 20, 30];

/// Which low-model slots use the high model for the player's cell.
// Translated from 0x00c5d7a0 (DDDA.exe 2017-12-13 Steam)
fn low_model_high_slots(cell: (i32, i32)) -> [bool; 6] {
    let (row, col) = (cell.0 - 40, cell.1 - 30);
    let area = usize::try_from(row)
        .ok()
        .zip(usize::try_from(col).ok())
        .and_then(|(r, c)| LOW_MODEL_AREAS.get(r)?.as_bytes().get(c).copied())
        .unwrap_or(b'.');
    let mut high = [false; 6];
    let slots: &[usize] = match area {
        b'3' => &[0, 1],
        b'a' => &[3, 0],
        b'c' => &[3, 4],
        b'd' => &[3, 5],
        b'z' => &[1, 2],
        _ => &[],
    };
    for &i in slots {
        high[i] = true;
    }
    high
}

/// Path of the first `rModel` entry of an archive.
fn arc_model_path(arc: &std::path::Path) -> Option<String> {
    let (index, _) = mtarc::Archive::open(arc).ok()?;
    index.entries.iter().find(|e| e.type_hash == RMODEL).map(|e| e.path.clone())
}

/// A model of a whole-stage area (`event::spawn_stage_models`), dropped
/// when the party leaves the stage (`crate::stream::Streamer::enter`).
#[derive(Component, Debug, Clone, Copy)]
pub struct StageEntity;

/// What a streamed cell entity belongs to (`crate::stream`): the cell
/// `(m, n)` and which of its parts.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamCell {
    pub m: i32,
    pub n: i32,
    pub part: CellPart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellPart {
    /// The cell's terrain model.
    Model,
    /// Its `_s00` layout (placed scenery).
    Layout,
    /// The viewer's distant stand-in (terrain plus `fmfore`).
    Far,
    /// A stage-100 low model: `m` is the slot (-1 for `st100_all00`), `n`
    /// is 1 for the `_h` variant.
    Low,
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
    /// Viewer eye (viewer units) for the models' LOD choice; `None` draws
    /// the near level.
    pub lod_eye: Option<Vec3>,
    /// The config's ViewRange (1 NORMAL, 2 FAR, 3 FARTHEST).
    pub view_range: u32,
    /// Tag for the entities spawned now (streamed cells).
    pub tag: Option<StreamCell>,
}

/// Builds a Bevy image from a TEX file, sRGB for the formats the exe
/// flags as colour (`mttex::is_srgb`), linear otherwise.
pub fn tex_to_image(bytes: &[u8]) -> Result<Image, String> {
    let t = mttex::parse(bytes).map_err(|e| e.to_string())?;
    let h = &t.header;
    let cube = h.shape == mttex::Shape::Cube;
    if h.shape == mttex::Shape::Volume {
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
            depth_or_array_layers: if cube { 6 } else { 1 },
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = h.mip_count.max(1);
    if cube {
        // Faces in D3D order (+X, -X, +Y, -Y, +Z, -Z), each with its mips:
        // wgpu's layer-major order.
        img.texture_view_descriptor = Some(bevy::render::render_resource::TextureViewDescriptor {
            dimension: Some(bevy::render::render_resource::TextureViewDimension::Cube),
            ..Default::default()
        });
    }
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
        let mut mat_cache: HashMap<(u16, bool, bool), (PartMaterial, Option<Shading>)> = HashMap::new();
        let mut parts = Vec::new();
        let lod_dist = [model.mid_dist as f32, model.low_dist as f32];
        for (mesh_index, m) in model.meshes.iter().enumerate() {
            // The draw mode is a mask of the render passes that draw the
            // primitive (0x00ea5320 tests it against the pass mask); meshes
            // without the main pass (e.g. the white shadow cards of the
            // om0570 cypresses, draw mode 0xE6C8) only cast shadows.
            let shadow_only = m.draw_mode & PASS_MAIN == 0;
            if shadow_only && m.draw_mode & PASS_SHADOW == 0 {
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
                .entry((m.material, undecoded_billboard, shadow_only))
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
                        self.material(shading.as_ref(), undecoded_billboard, shadow_only, materials, images),
                        shading,
                    )
                })
                .clone();
            if shadow_only && matches!(material, PartMaterial::Standard(_)) {
                continue;
            }
            // Albedo maps under effect\rtt\ are render targets of the effect
            // system (e.g. the coast's animated foam, e_st100_10_BM), not
            // in the data: without them the mesh would draw white, so it is
            // left out (*not done*: the foam effect).
            if shading.as_ref().and_then(|s| s.textures.get("tAlbedoMap")).is_some_and(|t| {
                t.starts_with("effect\\rtt\\") && self.vfs.read(t, RTEXTURE).is_err()
            }) {
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
                lod: m.lod,
                casts_shadow: m.draw_mode & PASS_SHADOW != 0,
                lod_dist,
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
    pub fn material(
        &mut self,
        shading: Option<&Shading>,
        two_sided: bool,
        shadow_only: bool,
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
        } else if s.blends_alpha() && s.feature("FTransparency") == Some("FTransparencyVolume") {
            // DDMaterialWater: the original fades by water depth over
            // fTransparencyVolume (not done); blended at fTransparency.
            sm.base_color.set_alpha(s.transparency);
            AlphaMode::Blend
        } else {
            // Alpha computed but neither tested nor blended: no effect.
            AlphaMode::Opaque
        };
        // The original uses forward Z; Bevy uses reverse Z (positive bias
        // brings the surface forward). Preserve rasterizer bias with that
        // sign conversion (mtshader::RasterizerState, SHADING.md).
        // No additional bias is invented for coplanar alpha overlays.
        sm.depth_bias = -s.raster.map_or(0.0, |r| r.depth_bias as f32);
        if let Some([r0, r1]) = s.uv_transform {
            sm.uv_transform = bevy::math::Affine2::from_cols(
                Vec2::new(r0[0], r1[0]),
                Vec2::new(r0[1], r1[1]),
                Vec2::new(r0[3], r1[3]),
            );
        }
        let mut params = DdParams::default();
        // Blend and alpha test are independent in the original. Bevy's
        // AlphaMode::Blend alone would silently omit the latter.
        if s.blends_alpha() {
            if let Some(test) = s.alpha_test {
                params.flags |= FLAG_ALPHA_TEST;
                params.alpha_compare = u32::from(test.compare);
                params.alpha_reference = u32::from(test.reference);
            }
        }
        if shadow_only {
            params.flags |= FLAG_SHADOW_ONLY;
        }
        let mut map = None;
        let mut spec = None;
        let mut env_cube = None;
        if forward_unlit(s) {
            params.flags |= FLAG_NO_LIGHT_BUFFER;
        }
        if sm.alpha_mode == AlphaMode::Blend {
            params.flags |= FLAG_BLENDED;
        }
        if matches!(s.feature("FReflect"), Some("FReflectCubeMap") | Some("FReflectWaterMap")) {
            // Reflection of the sky render (DDCube0_CM) or of the planar
            // water reflection; both are run-time render targets, see the
            // shader for the stand-in.
            params.flags |= FLAG_REFLECT;
            let f0 = if s.feature("FFresnel") == Some("FFresnelSchlick") { s.fresnel } else { 1.0 };
            params.reflect = Vec3::from(s.reflective_color).extend(f0);
            // The sky's environment cubes are run-time copies of the static
            // DDBaseCube<i>_CM tinted by the weather manager (pixel shader at
            // file offset 0x912060 of sc\dx9\root, sEnvMapManager's
            // SystemEnvCube pass 0x00434be0).
            if let Some((h, i)) = self.env_cube(s, images) {
                env_cube = Some(h);
                params.flags |= FLAG_ENV_CUBE;
                params.env = Vec4::new(1.0, 1.0, 1.0, i as f32);
            }
            if s.feature("FSpecular") == Some("FSpecularMap") {
                if let Some(p) = s.textures.get("tSpecularMap") {
                    spec = self.texture(p, images);
                    if spec.is_some() {
                        params.flags |= FLAG_SPEC_MAP;
                    }
                }
            }
        }
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
                extension: DdExt {
                    depth: s.depth.map(|d| (d.raw[0] & 31) as u8),
                    params,
                    map,
                    spec,
                    sky: SkyEnvParams::default(),
                    sky_rayleigh: Handle::default(),
                    sky_mie: Handle::default(),
                    env_cube,
                    albedo_blend: None,
                },
            },
            handle: OnceLock::new(),
        }))
    }

    /// The stage sky's environment cube a material's `tEnvMap` names
    /// (`scr\sky\DDCube<i>_CM`): the static `DDBaseCube<i>_CM` and `i`.
    fn env_cube(&mut self, s: &Shading, images: &mut Assets<Image>) -> Option<(Handle<Image>, u32)> {
        let i = s
            .textures
            .get("tEnvMap")
            .and_then(|t| t.strip_prefix("scr\\sky\\DDCube"))
            .and_then(|t| t.strip_suffix("_CM"))
            .and_then(|i| i.parse::<u32>().ok())?;
        Some((self.texture(&format!("scr\\sky\\DDBaseCube{i}_CM"), images)?, i))
    }

    /// A character part's material: [`Self::material`] plus the terms of
    /// the character pixel shaders (file offsets 0x31BEA8 and 0x40C494 of
    /// `sc\dx9\root`, the `FAlbedoMapAdd` / `FUVViewNormal` /
    /// `FReflectCubeMapShadowLight` / `FSpecularMap` variants most
    /// `model\pl` materials use; docs/SHADING.md, "Characters"):
    /// - albedo = albedo x fAlbedoColor + blend.rgb x blend.a x
    ///   fAlbedoBlendColor.rgb x .w, blend = `tAlbedoBlendMap` on the
    ///   view-normal UV;
    /// - plus (sun x shadow x (N.H)^fShininess x fSpecularColor + env) x
    ///   specular map x lerp(F0, 1, (1 - N.V)^5).
    pub fn character_material(
        &mut self,
        shading: Option<&Shading>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> PartMaterial {
        let base = self.material(shading, false, false, materials, images);
        let (Some(s), PartMaterial::Dd(slot)) = (shading, &base) else { return base };
        let mut m = slot.material.clone();
        if let Some(c) = albedo_add(s) {
            if let Some(h) = s.textures.get("tAlbedoBlendMap").and_then(|t| self.texture(t, images)) {
                m.extension.params.flags |= FLAG_ALBEDO_ADD;
                m.extension.params.albedo_blend = c;
                m.extension.albedo_blend = Some(h);
            }
        }
        if s.feature("FSpecular") != Some("FSpecularDisable") {
            let p = &mut m.extension.params;
            p.flags |= FLAG_CHARACTER;
            p.spec = Vec3::from(s.specular_color).extend(s.shininess);
            let f0 = if s.feature("FFresnel") == Some("FFresnelSchlick") { s.fresnel } else { 1.0 };
            p.reflect = Vec3::from(s.reflective_color).extend(f0);
            if s.feature("FReflect") == Some("FReflectCubeMapShadowLight") {
                if let Some((h, i)) = self.env_cube(s, images) {
                    m.extension.env_cube = Some(h);
                    m.extension.params.flags |= FLAG_ENV_CUBE;
                    m.extension.params.env = Vec4::new(1.0, 1.0, 1.0, i as f32);
                }
            }
            if s.feature("FSpecular") == Some("FSpecularMap") && m.extension.spec.is_none() {
                if let Some(h) = s.textures.get("tSpecularMap").and_then(|p| self.texture(p, images)) {
                    m.extension.spec = Some(h);
                    m.extension.params.flags |= FLAG_SPEC_MAP;
                }
            }
        }
        PartMaterial::Dd(Arc::new(DdSlot { material: m, handle: OnceLock::new() }))
    }

    /// A sky cloud dome (`uSkyCloudModel`): the model at `pos` (viewer
    /// units), lit by the constant `light` (the unit's sun plus ambient
    /// colour; *assumed*, its shader is not traced) with its alpha times
    /// `alpha`, following the camera horizontally.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_sky_model(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        path: &str,
        light: Vec3,
        alpha: f32,
        pos: Vec3,
    ) -> bool {
        let Some(parts) = self.model(path, meshes, materials, images) else { return false };
        for p in &parts {
            let PartMaterial::Dd(slot) = &p.material else { continue };
            let mut m = slot.material.clone();
            m.extension.params.flags |= FLAG_CONST_LIGHT;
            m.extension.params.const_light = light.extend(0.0);
            let a = m.base.base_color.alpha() * alpha;
            m.base.base_color.set_alpha(a);
            let slot = Arc::new(DdSlot { material: m, handle: OnceLock::new() });
            commands.spawn((
                Mesh3d(p.mesh.clone()),
                Transform::from_translation(pos),
                Name::new(path.to_string()),
                PendingDd(slot),
                SkyFollow,
                bevy::light::NotShadowCaster,
                bevy::light::NotShadowReceiver,
            ));
        }
        true
    }

    fn spawn_parts(&mut self, commands: &mut Commands, parts: &[Part], transform: Transform, name: &str) {
        // The LOD level from the viewer's start eye (the original picks it
        // per frame from the camera to the model position).
        let level = match self.lod_eye {
            Some(eye) => {
                let dist = (transform.translation - eye).length() / UNIT;
                parts.first().map_or(1, |p| lod_level(dist, p.lod_dist[0], p.lod_dist[1], self.view_range))
            }
            None => 1,
        };
        for p in parts {
            if p.lod & level == 0 {
                continue;
            }
            self.stats.meshes += 1;
            self.stats.triangles += p.triangles;
            let mut e = commands.spawn((Mesh3d(p.mesh.clone()), transform, Name::new(name.to_string())));
            if let Some(tag) = self.tag {
                e.insert(tag);
            }
            match &p.material {
                PartMaterial::Standard(h) => e.insert(MeshMaterial3d(h.clone())),
                PartMaterial::Dd(slot) => e.insert(PendingDd(slot.clone())),
            };
            if !p.casts_shadow {
                e.insert(bevy::light::NotShadowCaster);
            }
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
        let tag = self.tag;
        self.tag = tag.map(|t| StreamCell { part: CellPart::Model, ..t });
        self.spawn_parts(commands, &parts, Transform::from_translation(offset), &cell);
        self.tag = tag.map(|t| StreamCell { part: CellPart::Layout, ..t });
        self.spawn_scenery(commands, meshes, materials, images, stage, a, b, offset);
        self.tag = tag;
        true
    }

    /// Only the cell's terrain model (streaming loads models and layouts
    /// with their own windows).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_cell_model(
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
        let path = format!("scr\\st{stage}\\model\\m{}\\st{stage}_{a}m{b}n", a / 10 * 10);
        let Some(parts) = self.model(&path, meshes, materials, images) else {
            return false;
        };
        self.stats.cells += 1;
        self.spawn_parts(commands, &parts, Transform::from_translation(offset), &format!("st{stage}_{a}m{b}n"));
        true
    }

    /// Only the cell's `_s00` layout.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_cell_layout(
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
        self.spawn_scenery(commands, meshes, materials, images, stage, a, b, offset);
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
        // The cell's own terrain under its forest: without it the fmfore
        // trees float. (The cell's scenery layout is not loaded at this
        // distance.)
        let terrain = format!("scr\\st{stage}\\model\\m{}\\st{stage}_{a}m{b}n", a / 10 * 10);
        let mut any = false;
        if let Some(parts) = self.model(&terrain, meshes, materials, images) {
            self.spawn_parts(commands, &parts, Transform::from_translation(offset), &terrain);
            any = true;
        }
        let path = format!("scr\\st{stage}\\model\\fm_m{}\\fmfore_{a}m{b}n", a / 10 * 10);
        if let Some(parts) = self.model(&path, meshes, materials, images) {
            self.spawn_parts(commands, &parts, Transform::from_translation(offset), &path);
            any = true;
        }
        if any {
            self.stats.far_cells += 1;
        }
        any
    }

    /// The stage-100 landmarks (`uFmSetBase`): 15 far models placed by the
    /// exe, not by a layout. Each switches between the model of
    /// `rom\stage\stage100\fm\fm_index<i>_high.arc` within its radius of the
    /// player and `fm_index<i>_low.arc` beyond it. `eye` is the viewer's
    /// eye in the stage frame (origin cell 50m50n, cm); `origin` is the
    /// viewer's origin cell (A0, B0).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_landmarks(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        origin: (i32, i32),
        eye: Vec3,
        progress: i32,
    ) -> usize {
        let shift = -Vec3::new((origin.1 - 50) as f32 * 10000.0, 0.0, (origin.0 - 50) as f32 * 10000.0);
        let mut n = 0;
        for (i, l) in LANDMARKS.iter().enumerate() {
            match l.gate {
                Some((v, 1)) if progress < v => continue,
                Some((v, 2)) if progress >= v => continue,
                _ => {}
            }
            let pos = Vec3::from(l.position);
            let near = Vec2::new(eye.x - pos.x, eye.z - pos.z).length() < l.radius;
            let file = format!("fm_index{i:02}_{}.arc", if near { "high" } else { "low" });
            let arc = self.vfs.root.join("rom").join("stage").join("stage100").join("fm").join(file);
            let Some(path) = arc_model_path(&arc) else {
                self.stats.missing.push(format!("landmark {i}: no model in {}", arc.display()));
                continue;
            };
            let Some(parts) = self.model(&path, meshes, materials, images) else { continue };
            self.spawn_parts(commands, &parts, Transform::from_translation((pos + shift) * UNIT), &path);
            n += 1;
        }
        n
    }

    /// The low models for the player's cell `(m, n)`: `(-1, false)` for the
    /// backdrop, then each area slot with whether it uses `_h`
    /// (see [`Loader::spawn_low_models`]).
    pub fn low_models_for(&self, cell: (i32, i32)) -> Vec<(i32, bool)> {
        let high = low_model_high_slots(cell);
        let mut out = vec![(-1, false)];
        out.extend((0..LOW_MODEL_NUMBERS.len()).map(|i| (i as i32, high[i] || self.view_range >= 2)));
        out
    }

    /// One low model (`slot` -1 is the backdrop) for a viewer whose origin
    /// cell is `origin`.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_low_model(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        origin: (i32, i32),
        slot: i32,
        high: bool,
    ) -> bool {
        let path = match usize::try_from(slot).ok().and_then(|i| LOW_MODEL_NUMBERS.get(i)) {
            Some(n) => format!("scr\\st100\\model\\st100_area{n:02}_{}", if high { "h" } else { "l" }),
            None => "scr\\st100\\model\\st100_all00".to_string(),
        };
        let shift = -Vec3::new((origin.1 - 50) as f32 * 10000.0, 0.0, (origin.0 - 50) as f32 * 10000.0);
        let Some(parts) = self.model(&path, meshes, materials, images) else {
            return false;
        };
        self.spawn_parts(commands, &parts, Transform::from_translation(shift * UNIT), &path);
        true
    }

    /// The stage-100 low models (`uStageLowMdl`, created by the split
    /// controller, 0x00c5c2d0 and 0x00c5d7a0): the whole-map backdrop
    /// `scr\st100\model\st100_all00` and six area models
    /// `st100_area<NN>_h` / `_l`, all in the stage frame. An area slot uses
    /// `_h` when the player's cell's area marks it (`low_model_high_slots`)
    /// or when the config's ViewRange is FAR or FARTHEST (+0x38 >= 2,
    /// 0x00c58d60), `_l` otherwise. `origin` is the viewer's origin cell
    /// (A0, B0). *Assumed:* the models sit at the stage origin (their
    /// `cSetInfoCoord` is not set by the creator) and are drawn without the
    /// fade the unit runs while streaming in.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_low_models(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        origin: (i32, i32),
        view_range: u32,
    ) -> usize {
        let shift = -Vec3::new((origin.1 - 50) as f32 * 10000.0, 0.0, (origin.0 - 50) as f32 * 10000.0);
        let high = low_model_high_slots(origin);
        let mut paths = vec!["scr\\st100\\model\\st100_all00".to_string()];
        for (i, n) in LOW_MODEL_NUMBERS.iter().enumerate() {
            let suffix = if high[i] || view_range >= 2 { "h" } else { "l" };
            paths.push(format!("scr\\st100\\model\\st100_area{n:02}_{suffix}"));
        }
        let mut count = 0;
        for path in paths {
            let Some(parts) = self.model(&path, meshes, materials, images) else {
                self.stats.missing.push(format!("low model {path}"));
                continue;
            };
            self.spawn_parts(commands, &parts, Transform::from_translation(shift * UNIT), &path);
            count += 1;
        }
        count
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
            // The water models' own materials (MaterialStdEst sea,
            // DDMaterialWater rivers and lakes); the placeholder is only
            // used for parts without one.
            let parts: Vec<Part> = parts
                .into_iter()
                .map(|q| match q.material {
                    PartMaterial::Dd(_) => q,
                    _ => Part {
                        material: PartMaterial::Standard(water.clone()),
                        ..q
                    },
                })
                .collect();
            self.stats.water_models += 1;
            self.spawn_parts(commands, &parts, Transform::from_translation(shift), p);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn authored_depth_states_survive_blending_and_reverse_z() {
        // Encoded MFX test/write/comparison combinations and reverse Z.
        use bevy::render::render_resource::CompareFunction;
        assert_eq!(super::imported_depth(15), (true, CompareFunction::GreaterEqual));
        assert_eq!(super::imported_depth(13), (false, CompareFunction::GreaterEqual));
        assert_eq!(super::imported_depth(30), (true, CompareFunction::Always));
        assert_eq!(super::imported_depth(7), (true, CompareFunction::Greater));
        assert_eq!(super::imported_depth(19), (true, CompareFunction::Less));
        let path = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC\usershader\BbsShaderPackage.mfx");
        if !path.is_file() { return; }
        let p = ShaderPackage::parse(&std::fs::read(path).unwrap()).unwrap();
        for (name, write) in [("DSZTestWrite", true), ("DSZTest", false)] {
            let entry = p.entries.values().find(|e| e.name == name).unwrap();
            let state = p.depth_stencil_states[&entry.index];
            assert_eq!(super::imported_depth((state.raw[0] & 31) as u8), (write, CompareFunction::GreaterEqual));
        }
    }
    use super::*;

    fn character_shading(features: &[(&str, &str)], blend: [f32; 4], diffuse: [f32; 3]) -> Shading {
        Shading {
            features: features.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            textures: HashMap::new(),
            albedo_uv: "UV_Primary".into(),
            albedo_color: [1.0; 3],
            albedo_blend_color: blend,
            specular_color: [1.0; 3],
            shininess: 16.0,
            diffuse_color: diffuse,
            transparency: 1.0,
            reflective_color: [0.0; 3],
            fresnel: 1.0,
            uv_transform: None,
            alpha: AlphaSource::None,
            blend: None,
            depth: None,
            raster: None,
            alpha_test: None,
        }
    }

    /// FAlbedoMapAdd: the blend map adds fAlbedoBlendColor.rgb x .w (x
    /// fDiffuseColor) on the view-normal UV; the (0,0,0,0) and (1,1,1,0)
    /// colours of most `model\pl` materials add nothing.
    #[test]
    fn albedo_add_colour() {
        let vn = [("FAlbedo", "FAlbedoMapAdd"), ("FUVAlbedoBlendMap", "FUVViewNormal")];
        assert_eq!(albedo_add(&character_shading(&vn, [0.0; 4], [1.0; 3])), None);
        assert_eq!(albedo_add(&character_shading(&vn, [1.0, 1.0, 1.0, 0.0], [1.0; 3])), None);
        assert_eq!(
            albedo_add(&character_shading(&vn, [0.25, 0.5, 0.75, 1.0], [0.5; 3])),
            Some(Vec4::new(0.125, 0.25, 0.375, 0.0))
        );
        let secondary = [("FAlbedo", "FAlbedoMapAdd"), ("FUVAlbedoBlendMap", "FUVSecondary")];
        assert_eq!(albedo_add(&character_shading(&secondary, [1.0; 4], [1.0; 3])), None);
    }

    #[test]
    fn lod_levels_follow_the_distances() {
        // om0570: middle 3000 cm, far 8000 cm.
        assert_eq!(lod_level(2000.0, 3000.0, 8000.0, 1), 1);
        assert_eq!(lod_level(5000.0, 3000.0, 8000.0, 1), 2);
        assert_eq!(lod_level(9000.0, 3000.0, 8000.0, 1), 4);
        // FAR doubles both distances, FARTHEST keeps the near level.
        assert_eq!(lod_level(9000.0, 3000.0, 8000.0, 2), 2);
        assert_eq!(lod_level(90000.0, 3000.0, 8000.0, 3), 1);
    }

    #[test]
    fn low_model_slots_by_cell() {
        // ref001's cell 58m50n is in area 10: slots area10 and area00 high.
        assert_eq!(low_model_high_slots((58, 50)), [true, false, false, true, false, false]);
        // 42m53n (row 2, column 23) is area 13: area10 and area30.
        assert_eq!(low_model_high_slots((42, 53)), [false, false, false, true, false, true]);
        // Outside the map: all low.
        assert_eq!(low_model_high_slots((10, 10)), [false; 6]);
    }
}
