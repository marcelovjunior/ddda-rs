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

/// Weather suffixes of the weather manager's tracks (`mWeatherType` 0
/// fine, 1 cloudy, 2 very cloudy; the darkness weather 3 is not handled).
pub const WEATHERS: [&str; 3] = ["F", "E", "R"];

/// Mie scattering and density of each weather: the weather manager's
/// constructor values (0x004bb200 copies the tables at 0x01823b30,
/// 0x01823ba0 and 0x01823c10); the stage schedulers do not set them.
const WEATHER_MIE: [(f32, f32); 3] = [(7.554_876, 1.0), (7.55, 1.0), (7.554_877, 1.0)];

/// The sky scheduler's light units, as the weather manager drives them.
#[derive(Clone, Debug, Default)]
pub struct SkyLights {
    /// Sun (`uSkyInfiniteLight` with `IntensityScale_*`): `mLightScale`,
    /// the weather's `IntensityScale` and `SaturationScale`.
    pub sun_scale: f32,
    pub sun_intensity: f32,
    pub sun_saturation: f32,
    /// Fill light (`uSkyInfiniteLight` with `DayColor_*`).
    pub fill: Option<BlendLight>,
    /// Hemisphere light (`uSkyHemiSphereLight`).
    pub hemi: Option<BlendLight>,
    /// The night spot light (`uSkySpotLight`).
    pub spot: Option<SkySpot>,
}

/// `uSkySpotLight`: a spot light at `LightOffset` above the player (*assumed*
/// anchor), lit only at night.
#[derive(Clone, Debug, Default)]
pub struct SkySpot {
    pub color: [f32; 3],
    pub offset: [f32; 3],
    /// `mStart`, `mEnd` (cm), `mCone`, `mSpread` (degrees), `mFalloff`.
    pub start: f32,
    pub end: f32,
    pub cone: f32,
    pub spread: f32,
    pub falloff: f32,
}

impl SkySpot {
    /// Its colour at night blend `b`: `LightColor` x clamp((b - 0.8) x 10,
    /// 0, 1) (the zone fade, 1 outside zones, is left out).
    // Translated from 0x00c69460 (DDDA.exe 2017-12-13 Steam)
    pub fn color_at(&self, b: f32) -> [f32; 3] {
        let f = ((b - 0.8) * 9.999_998).clamp(0.0, 1.0);
        self.color.map(|c| c * f)
    }

    /// Shader attenuation parameters (`sLightParam` +0x30..+0x40): start,
    /// 1 / (end - start), cos(cone / 2), 1 / (cos((cone - spread) / 2) -
    /// cos(cone / 2)), and the falloff exponent.
    // Translated from 0x00e19850 (DDDA.exe 2017-12-13 Steam)
    pub fn attenuation(&self) -> ([f32; 4], f32) {
        let deg = std::f32::consts::PI / 180.0;
        let outer = (self.cone * 0.5 * deg).cos();
        let inner = ((self.cone - self.spread) * 0.5 * deg).cos();
        let delta = (inner - outer).max(1.192_092_9e-7);
        ([self.start, 1.0 / (self.end - self.start), outer, 1.0 / delta], self.falloff)
    }
}

/// A light with day and night colours (and a reverse, ground, colour for
/// the hemisphere light).
#[derive(Clone, Debug, Default)]
pub struct BlendLight {
    pub day: [f32; 3],
    pub night: [f32; 3],
    pub day_rev: [f32; 3],
    pub night_rev: [f32; 3],
    pub dir: [f32; 3],
}

impl BlendLight {
    /// Colours at night blend `b`. *Assumed:* a linear blend from day to
    /// night by the weather manager's day/night rate; it matches the 20:21
    /// capture's hemisphere and fill colours within the sun-direction error.
    fn at(&self, b: f32) -> ([f32; 3], [f32; 3]) {
        let mix = |d: [f32; 3], n: [f32; 3]| std::array::from_fn(|i| d[i] + (n[i] - d[i]) * b);
        (mix(self.day, self.night), mix(self.day_rev, self.night_rev))
    }
}

/// The stage's sky: the `rSky` resource, the sky unit's inputs and the
/// light units, for one weather.
pub struct StageSky {
    pub sky: mtweather::Sky,
    pub inputs: SkyInputs,
    pub lights: SkyLights,
    /// Colour of each environment cube `DDCube<i>_CM`:
    /// `mEnvMapBaseColor[i]` x lerp(1, `mEnvMapColor[i]`.rgb, .a), the
    /// constants of the cube tint pass (sc\dx9\root 0x912060). *Assumed:*
    /// the weather scales (`mEnvMapBaseColorScale_C/_R`, `mEnvMapBaseScale_C/_R`)
    /// are not applied (their use is not traced), the blend factor is 0.
    pub env_colors: [[f32; 3]; 5],
    /// The cloud domes (`uSkyCloudModel`) of this weather.
    pub clouds: Vec<CloudDome>,
}

/// A `uSkyCloudModel` of the sky scheduler.
#[derive(Clone, Debug)]
pub struct CloudDome {
    pub model: String,
    /// `mDayOnly`: hidden by night (its alpha is the day share, 0x00c64620).
    pub day_only: bool,
    pub light: mtweather::sky::CloudUnit,
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
    /// Reads the stage's sky scheduler for weather `weather` (index into
    /// [`WEATHERS`]). `None` if it or its sky is missing.
    pub fn load(vfs: &ddvfs::Vfs, stage: u32, weather: usize) -> Option<StageSky> {
        let w = WEATHERS.get(weather)?;
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
        // The sky unit's own tracks, then the weather manager's values for
        // the weather, which it writes into the sky unit (0x004c3d40).
        let own = |name, d| float(&s, sky, name).unwrap_or(d);
        inp.height_offset = own("mHeightOffset", inp.height_offset);
        inp.starry_sky_intensity = own("mStarrySkyIntensity", inp.starry_sky_intensity);
        let div = |name, d: u32| int(&s, sky, name).map(|v| v as u32).unwrap_or(d);
        inp.scatter_map_size = [div("mDivX", 32), div("mDivY", 32)];
        let wmf = |name: &str, d| float(&s, wm, &format!("{name}_{w}")).unwrap_or(d);
        inp.sun_color = vec3(&s, wm, &format!("SunColor_{w}"))
            .or_else(|| vec3(&s, sky, "mBaseSunColor"))
            .unwrap_or(inp.sun_color);
        (inp.mie_scattering, inp.mie_density) = ([WEATHER_MIE[weather].0; 3], WEATHER_MIE[weather].1);
        inp.cloudiness = wmf("Cloudiness", own("mCloudiness", inp.cloudiness));
        inp.cloud_height = wmf("CloudHeight", own("mCloudHeight", inp.cloud_height));
        inp.cloud_thickness = wmf("CloudThickness", own("mCloudThickness", inp.cloud_thickness));
        inp.cloud_scattering = wmf("CloudScattering", own("mCloudScattering", inp.cloud_scattering));
        inp.cloud_eccentricity = wmf("CloudEccentricity", own("mCloudEccentricity", inp.cloud_eccentricity));
        let lights = Self::light_units(&s, w);
        // The arrays' tracks repeat the name; the type hash field holds the
        // element index.
        let element = |name: &str, i: u32| {
            s.tracks
                .iter()
                .find(|t| t.link as usize == wm && t.name == name && t.type_hash == i)
                .and_then(|t| t.keys.first())
                .and_then(|k| match k.value {
                    Value::Vec4(v) => Some(v),
                    _ => None,
                })
        };
        let env_colors = std::array::from_fn(|i| {
            let base = element("mEnvMapBaseColor", i as u32).unwrap_or([1.0; 4]);
            let c = element("mEnvMapColor", i as u32).unwrap_or([1.0; 4]);
            std::array::from_fn(|k| base[k] * (1.0 + c[3] * (c[k] - 1.0)))
        });
        // Cloud domes whose mWeatherType is this weather (0x00c64620 gives
        // the others alpha 0 once the weather has settled).
        let cloud_type = mtarc::type_hash("uSkyCloudModel");
        let clouds = s
            .tracks
            .iter()
            .enumerate()
            .filter(|(i, t)| t.type_hash == cloud_type && int(&s, *i, "mWeatherType") == Some(weather as i32))
            .filter_map(|(i, _)| {
                let Some(Value::Resource(Some((_, model)))) = value(&s, i, "mpModel") else { return None };
                let f = |name, d| float(&s, i, name).unwrap_or(d);
                Some(CloudDome {
                    model,
                    day_only: matches!(value(&s, i, "mDayOnly"), Some(Value::Bool(true))),
                    light: mtweather::sky::CloudUnit {
                        sun_height: f("mSunColorHeight", 0.0),
                        sun_scale: f("mSunColorScale", 1.0),
                        saturation: f("mSaturationScale", 1.0),
                        intensity: f("mIntensityScale", 1.0),
                        amb_height: f("mAmbColorHeight", 0.0),
                        amb_scale: f("mAmbColorScale", 1.0),
                    },
                })
            })
            .collect();
        Some(StageSky { sky: sky_res, inputs: inp, lights, env_colors, clouds })
    }

    /// The light units of the scheduler by their tracks: the infinite
    /// light with `IntensityScale_<w>` is the sun, the one with
    /// `DayColor_<w>` the fill light, the hemisphere light has both colours.
    fn light_units(s: &Scheduler, w: &str) -> SkyLights {
        let infinite = mtarc::type_hash("uSkyInfiniteLight");
        let spot = mtarc::type_hash("uSkySpotLight");
        let hemisphere = mtarc::type_hash("uSkyHemiSphereLight");
        let mut out = SkyLights::default();
        for (i, t) in s.tracks.iter().enumerate() {
            let day = vec3(s, i, &format!("DayColor_{w}"));
            if t.type_hash == spot {
                out.spot = Some(SkySpot {
                    color: vec3(s, i, "LightColor").unwrap_or_default(),
                    offset: vec3(s, i, "LightOffset").unwrap_or([0.0, 0.0, 0.0]),
                    start: float(s, i, "mStart").unwrap_or(0.0),
                    end: float(s, i, "mEnd").unwrap_or(1.0),
                    cone: float(s, i, "mCone").unwrap_or(90.0),
                    spread: float(s, i, "mSpread").unwrap_or(0.0),
                    falloff: float(s, i, "mFalloff").unwrap_or(1.0),
                });
            } else if t.type_hash == infinite {
                if let Some(intensity) = float(s, i, &format!("IntensityScale_{w}")) {
                    out.sun_scale = float(s, i, "mLightScale").unwrap_or(1.0);
                    out.sun_intensity = intensity;
                    out.sun_saturation = float(s, i, &format!("SaturationScale_{w}")).unwrap_or(1.0);
                } else if let Some(day) = day {
                    out.fill = Some(BlendLight {
                        day,
                        night: vec3(s, i, "NightColor").unwrap_or_default(),
                        dir: vec3(s, i, "LightDir").unwrap_or([0.0, 1.0, 0.0]),
                        ..Default::default()
                    });
                }
            } else if t.type_hash == hemisphere {
                if let Some(day) = day {
                    out.hemi = Some(BlendLight {
                        day,
                        night: vec3(s, i, "NightColor").unwrap_or_default(),
                        day_rev: vec3(s, i, &format!("DayRevColor_{w}")).unwrap_or_default(),
                        night_rev: vec3(s, i, "NightRevColor").unwrap_or_default(),
                        dir: vec3(s, i, "LightDir").unwrap_or([0.0, 1.0, 0.0]),
                    });
                }
            }
        }
        out
    }

    /// The light units for a sun direction: hemisphere lights as (sky rgb,
    /// ground rgb, axis) and infinite lights as (colour, direction to the
    /// light), the sun first, as in the `--light` JSON.
    pub fn light_values(&self, sun_dir: [f32; 3]) -> (Vec<[f32; 9]>, Vec<[f32; 6]>) {
        let b = mtweather::sky::night_blend(sun_dir);
        let l = &self.lights;
        let sun = mtweather::sky::sun_light_color(
            &self.sky,
            &self.inputs,
            sun_dir,
            l.sun_scale,
            l.sun_intensity,
            l.sun_saturation,
        );
        let mut lights = vec![[sun[0], sun[1], sun[2], sun_dir[0], sun_dir[1], sun_dir[2]]];
        if let Some(f) = &l.fill {
            let (c, _) = f.at(b);
            lights.push([c[0], c[1], c[2], f.dir[0], f.dir[1], f.dir[2]]);
        }
        let mut hemis = Vec::new();
        if let Some(h) = &l.hemi {
            let (sky, ground) = h.at(b);
            hemis.push([sky[0], sky[1], sky[2], ground[0], ground[1], ground[2], h.dir[0], h.dir[1], h.dir[2]]);
        }
        (hemis, lights)
    }
}

/// The weather's fog at `minute` of the day
/// (`scr\st<S>\etc\st<S>_fog_<w>`, sampled as 0x00cead20 does).
pub fn weather_fog(vfs: &ddvfs::Vfs, stage: u32, weather: usize, minute: u32) -> Option<mtweather::FogValues> {
    let w = WEATHERS.get(weather)?.to_lowercase();
    let path = format!("scr\\st{stage}\\etc\\st{stage}_fog_{w}");
    let bytes = vfs.read(&path, mtarc::type_hash("rWeatherFogParam")).ok()?;
    mtweather::WeatherFog::parse(&bytes).ok()?.sample(minute)
}

/// The weather whose fog at `minute` equals a captured fog colour (within
/// 1 %), e.g. the refs-lit series' fog is `st100_fog_e`'s.
pub fn weather_of_fog(vfs: &ddvfs::Vfs, stage: u32, minute: u32, color: [f32; 3]) -> Option<usize> {
    (0..WEATHERS.len()).find(|&w| {
        weather_fog(vfs, stage, w, minute).is_some_and(|f| {
            (0..3).all(|i| (f.color[i] - color[i]).abs() <= color[i].abs() * 0.01 + 1e-4)
        })
    })
}

impl StageSky {
    /// Spawns the dome for a sun direction (unit, towards the sun) with
    /// radius `radius` (viewer units; keep it inside the far plane), and
    /// inserts [`crate::stage::SkyEnv`] for the materials' reflections seen
    /// from `eye` (game units).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<SkyMaterial>,
        sun_dir: Vec3,
        radius: f32,
        eye: Vec3,
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
        commands.insert_resource(crate::stage::EnvCubeColors(self.env_colors.map(Vec3::from)));
        commands.insert_resource(crate::stage::SkyEnv {
            params: crate::stage::SkyEnvParams { cb: params.cb, eye: eye.extend(0.0) },
            rayleigh: rayleigh.clone(),
            mie: mie.clone(),
        });
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

#[allow(clippy::type_complexity)]
fn follow_camera(
    cams: Query<&Transform, (With<Camera3d>, Without<SkyDome>, Without<crate::stage::SkyFollow>)>,
    mut clouds: Query<&mut Transform, (With<crate::stage::SkyFollow>, Without<Camera3d>, Without<SkyDome>)>,
    mut domes: Query<(&mut Transform, &SkyDome)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let Some(cam) = cams.iter().next() else { return };
    for mut t in &mut clouds {
        t.translation.x = cam.translation.x;
        t.translation.z = cam.translation.z;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// st100_sky's SkySpotLight: LightColor (0.022, 0.05672, 0.078), start
    /// 2000, end 3000, cone 90, spread 60, falloff 1.
    #[test]
    fn sky_spot_light() {
        let s = SkySpot {
            color: [0.022, 0.05672, 0.078],
            offset: [0.0, 2000.0, 0.1],
            start: 2000.0,
            end: 3000.0,
            cone: 90.0,
            spread: 60.0,
            falloff: 1.0,
        };
        assert_eq!(s.color_at(0.5), [0.0; 3]);
        let c = s.color_at(1.0);
        assert!((c[0] - 0.022).abs() < 1e-6 && (c[2] - 0.078).abs() < 1e-6);
        let (a, f) = s.attenuation();
        assert!(a[0] == 2000.0 && (a[1] - 0.001).abs() < 1e-9 && f == 1.0);
        // cos 45 deg and 1 / (cos 15 deg - cos 45 deg)
        assert!((a[2] - 0.707_106_8).abs() < 1e-6 && (a[3] - 3.863_7).abs() < 1e-3);
    }
}
