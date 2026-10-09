//! The sky dome (docs/SHADING.md, "Sky"): the original's Rayleigh / Mie
//! scattering sky, lit by the sun of the `--light` capture.
//!
//! Inputs come from the stage's sky scheduler `scr\st{stage}\sdl\st{stage}_sky`:
//! the sky unit `mpSky` (its `rSky` resource, height offset, starry sky
//! intensity, scatter map size) and the weather manager's fine-weather
//! values (`SunColor_F`, `Cloudiness_F` ...), which the weather manager
//! writes into the sky unit each frame (0x004c3d40). *Assumed:* fine
//! weather, the first key of each track, and the capture's sun direction as
//! the sky's sun (`uSky` +0x2E0; the two agree at 12:21, elevation 66 deg).
//!
//! The dome is a sphere around the camera just inside the far plane; its
//! shader (`sky.wgsl`) evaluates the scatter per pixel.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::asset::RenderAssetUsages;
use bevy::shader::ShaderRef;

use ddquest::sdl::{Scheduler, Value};
use mtweather::sky::{SkyInputs, DEPTH_MAP_SIZE};

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "sky.wgsl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(PostUpdate, follow_camera.before(TransformSystems::Propagate));
    }
}

/// Uniform of [`SkyMaterial`]; layout mirrored in `sky.wgsl`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct SkyParams {
    /// `CBSky` (44 floats).
    pub cb: [Vec4; 11],
    /// Camera position in game units (cm).
    pub eye: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub params: SkyParams,
    #[texture(1, sample_type = "float", filterable = false)]
    pub rayleigh: Handle<Image>,
    #[texture(2, sample_type = "float", filterable = false)]
    pub mie: Handle<Image>,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://ddview/sky.wgsl".into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Seen from inside.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Marks the dome; it follows the camera.
#[derive(Component)]
pub struct SkyDome(pub Handle<SkyMaterial>);

/// The stage's sky: the `rSky` resource and the sky unit's inputs.
pub struct StageSky {
    pub sky: mtweather::Sky,
    pub inputs: SkyInputs,
}

/// First key of the track `name` under track `parent`.
fn value(s: &Scheduler, parent: usize, name: &str) -> Option<Value> {
    s.tracks
        .iter()
        .find(|t| t.link as usize == parent && t.name == name)
        .and_then(|t| t.keys.first())
        .map(|k| k.value.clone())
}

fn float(s: &Scheduler, parent: usize, name: &str) -> Option<f32> {
    match value(s, parent, name)? {
        Value::Float(f) => Some(f),
        _ => None,
    }
}

fn int(s: &Scheduler, parent: usize, name: &str) -> Option<i32> {
    match value(s, parent, name)? {
        Value::Int(i) => Some(i),
        _ => None,
    }
}

fn vec3(s: &Scheduler, parent: usize, name: &str) -> Option<[f32; 3]> {
    match value(s, parent, name)? {
        Value::Vec4(v) => Some([v[0], v[1], v[2]]),
        _ => None,
    }
}

impl StageSky {
    /// Reads the stage's sky scheduler. `None` if it or its sky is missing.
    pub fn load(vfs: &ddvfs::Vfs, stage: u32) -> Option<StageSky> {
        let path = format!("scr\\st{stage}\\sdl\\st{stage}_sky");
        let bytes = vfs.read(&path, mtarc::type_hash("rScheduler")).ok()?;
        let s = Scheduler::parse(&bytes).ok()?;
        let wm = s.tracks.iter().position(|t| t.name == "WeatherManager")?;
        let sky = s.tracks.iter().position(|t| t.link as usize == wm && t.name == "mpSky")?;
        let Some(Value::Resource(Some((_, res)))) = value(&s, sky, "mpResource") else {
            return None;
        };
        let rsky = vfs.read(&res, mtarc::type_hash("rSky")).ok()?;
        let sky_res = mtweather::Sky::parse(&rsky).ok()?;
        let mut inp = SkyInputs::default();
        // The sky unit's own tracks, then the weather manager's fine weather.
        let own = |name, d| float(&s, sky, name).unwrap_or(d);
        inp.mie_scattering = vec3(&s, sky, "mMieScattering").unwrap_or(inp.mie_scattering);
        inp.mie_density = own("mMieDensity", inp.mie_density);
        inp.height_offset = own("mHeightOffset", inp.height_offset);
        inp.starry_sky_intensity = own("mStarrySkyIntensity", inp.starry_sky_intensity);
        inp.sun_color = vec3(&s, sky, "mBaseSunColor").unwrap_or(inp.sun_color);
        let div = |name, d: u32| int(&s, sky, name).map(|v| v as u32).unwrap_or(d);
        inp.scatter_map_size = [div("mDivX", 32), div("mDivY", 32)];
        let wmf = |name, d| float(&s, wm, name).unwrap_or(d);
        inp.sun_color = vec3(&s, wm, "SunColor_F").unwrap_or(inp.sun_color);
        inp.cloudiness = wmf("Cloudiness_F", own("mCloudiness", inp.cloudiness));
        inp.cloud_height = wmf("CloudHeight_F", own("mCloudHeight", inp.cloud_height));
        inp.cloud_thickness = wmf("CloudThickness_F", own("mCloudThickness", inp.cloud_thickness));
        inp.cloud_scattering = wmf("CloudScattering_F", own("mCloudScattering", inp.cloud_scattering));
        inp.cloud_eccentricity =
            wmf("CloudEccentricity_F", own("mCloudEccentricity", inp.cloud_eccentricity));
        Some(StageSky { sky: sky_res, inputs: inp })
    }

    /// Spawns the dome for a sun direction (unit, towards the sun) with
    /// radius `radius` (viewer units; keep it inside the far plane).
    pub fn spawn(
        &self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<SkyMaterial>,
        sun_dir: Vec3,
        radius: f32,
    ) {
        let cb = mtweather::sky::constants(&self.sky, &self.inputs, sun_dir.to_array());
        let mut map = |top: f32, scale: f32| {
            let data = mtweather::sky::optical_depth_map(top, scale, cb[19]);
            let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
            images.add(Image::new(
                Extent3d {
                    width: DEPTH_MAP_SIZE as u32,
                    height: DEPTH_MAP_SIZE as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                bytes,
                TextureFormat::R32Float,
                RenderAssetUsages::RENDER_WORLD,
            ))
        };
        let rayleigh = map(cb[3], cb[7]);
        let mie = map(cb[11], cb[15]);
        let params = SkyParams {
            cb: std::array::from_fn(|i| Vec4::from_slice(&cb[4 * i..4 * i + 4])),
            eye: Vec4::ZERO,
        };
        let material = materials.add(SkyMaterial { params, rayleigh, mie });
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(radius).mesh().uv(64, 32))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            SkyDome(material),
        ));
    }
}

fn follow_camera(
    cams: Query<&Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut domes: Query<(&mut Transform, &SkyDome)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let Some(cam) = cams.iter().next() else { return };
    for (mut t, dome) in &mut domes {
        t.translation = cam.translation;
        let eye = (cam.translation / crate::stage::UNIT).extend(0.0);
        let changed = materials.get(&dome.0).is_some_and(|m| m.params.eye != eye);
        if changed {
            if let Some(mut m) = materials.get_mut(&dome.0) {
                m.params.eye = eye;
            }
        }
    }
}
