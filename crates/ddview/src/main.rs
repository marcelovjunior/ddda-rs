//! `ddview`: M1 viewer. Loads a block of stage cells from the user's
//! Dragon's Dogma: Dark Arisen install and shows them with a free camera.
//!
//! ```text
//! ddview <nativePC> [--stage 100] [--cell 58,50] [--radius 2]
//!        [--pos x,y,z] [--look x,y,z] [--up x,y,z] [--fov deg] [--size WxH]
//!        [--far-cells N] [--camera-fov deg] [--ref refNNN.json] [--light light.json]
//!        [--shot out.png] [--shot-delay secs]
//! ```
//! `--pos`/`--look` are in game units (cm) in the origin cell's frame, the
//! same frame as the live game's `uPlayer +0x40` (see docs/WORLD.md).
//! `--ref` takes the eye, target, up, FOV and near/far planes from a
//! reference pose recorded by the private screenshot watcher
//! (`uCameraGame +0x30..+0x6c`) and its origin cell. `--fov` is the
//! vertical field of view; a reference's fov gets the game's CameraFov
//! setting added (read from the player's config.ini, or `--camera-fov`
//! given before `--ref`). `--light` switches the stage materials to the
//! original's lighting with inputs from a JSON file (docs/SHADING.md).
//! With `--shot` the viewer saves one frame after `--shot-delay` seconds
//! (default 10: Bevy compiles pipelines asynchronously and draws nothing
//! until they are ready) and exits.
//!
//! Controls: WASD move, Q/E down/up, Shift faster, hold right mouse to look.

mod event;
mod character;
mod play;
mod sky;
mod stage;

use std::path::PathBuf;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::WindowResolution;

#[derive(Resource, Clone)]
struct Args {
    native_pc: PathBuf,
    stage: u32,
    cell: (i32, i32),
    radius: i32,
    far_cells: i32,
    pos: Vec3,
    look: Vec3,
    up: Vec3,
    fov_deg: f32,
    near: f32,
    far: f32,
    size: (u32, u32),
    shot: Option<PathBuf>,
    light: Option<stage::StageLighting>,
    /// Game clock in ms into the day (`--clock HH:MM[:SS]`); else the
    /// `--light` capture's clock, else 12:00 (*viewer default*).
    clock_ms: Option<u32>,
    /// Weather (`--weather f|e|r`, index into `sky::WEATHERS`); else the
    /// weather whose fog matches the `--light` capture's, else fine.
    weather: Option<usize>,
    /// The game's [GRAPHICS] CameraFov setting, added to reference fovs.
    camera_fov: f32,
    /// Story progress (`*(sGameSys) + 0x30`) for progress-gated landmarks.
    progress: i32,
    shot_delay: f32,
    character: Option<String>,
    motion: Option<String>,
    slot: usize,
    frame: Option<f32>,
    play: bool,
    demo: bool,
    players: u8,
    spawn: Vec3,
}

fn parse_vec3(s: &str) -> Vec3 {
    let v: Vec<f32> = s.split(',').map(|x| x.trim().parse().expect("number")).collect();
    Vec3::new(v[0], v[1], v[2])
}

fn args() -> Args {
    let mut it = std::env::args().skip(1);
    let native_pc = PathBuf::from(it.next().expect("usage: ddview <nativePC> [options]"));
    // Default pose: the live player position read on 2026-10-08 in
    // st100_58m50n (docs/WORLD.md), eye 170 cm above it.
    let mut a = Args {
        native_pc,
        stage: 100,
        cell: (58, 50),
        radius: 2,
        far_cells: 20,
        pos: Vec3::new(205.4, 41019.0 + 170.0, 3542.5),
        look: Vec3::new(205.4 + 1000.0, 41019.0 + 100.0, 3542.5),
        shot: None,
        light: None,
        clock_ms: None,
        weather: None,
        camera_fov: config_camera_fov().unwrap_or(0.0),
        progress: 0,
        shot_delay: 10.0,
        character: None,
        motion: None,
        slot: 0,
        frame: None,
        play: false,
        demo: false,
        players: 2,
        // ref001's recorded player position (origin cell 58m50n)
        spawn: Vec3::new(215.9, 41054.3, 3921.1),
        up: Vec3::Y,
        fov_deg: 45.0,
        near: 10.0,
        far: 2_000_000.0,
        size: (1600, 900),
    };
    while let Some(k) = it.next() {
        if k == "--play" || k == "--demo" {
            a.play = true;
            a.demo |= k == "--demo";
            continue;
        }
        let v = it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--stage" => a.stage = v.parse().unwrap(),
            "--cell" => {
                let p: Vec<i32> = v.split(',').map(|x| x.parse().unwrap()).collect();
                a.cell = (p[0], p[1]);
            }
            "--radius" => a.radius = v.parse().unwrap(),
            "--far-cells" => a.far_cells = v.parse().unwrap(),
            "--pos" => a.pos = parse_vec3(&v),
            "--look" => a.look = parse_vec3(&v),
            "--shot" => a.shot = Some(PathBuf::from(v)),
            "--light" => a.light = Some(load_lighting(&v)),
            "--clock" => {
                let p: Vec<u32> = v.split(':').map(|x| x.parse().expect("--clock HH:MM[:SS]")).collect();
                a.clock_ms = Some((p[0] * 3600 + p.get(1).unwrap_or(&0) * 60 + p.get(2).unwrap_or(&0)) * 1000);
            }
            "--weather" => {
                let w = v.to_uppercase();
                a.weather = Some(sky::WEATHERS.iter().position(|x| *x == w).expect("--weather f|e|r"));
            }
            "--camera-fov" => a.camera_fov = v.parse().unwrap(),
            "--progress" => a.progress = v.parse().unwrap(),
            "--shot-delay" => a.shot_delay = v.parse().unwrap(),
            "--up" => a.up = parse_vec3(&v),
            "--fov" => a.fov_deg = v.parse().unwrap(),
            "--near" => a.near = v.parse().unwrap(),
            "--size" => {
                let p: Vec<u32> = v.split('x').map(|x| x.parse().unwrap()).collect();
                a.size = (p[0], p[1]);
            }
            "--ref" => apply_ref(&mut a, &v),
            "--character" => a.character = Some(v),
            "--motion" => a.motion = Some(v),
            "--slot" => a.slot = v.parse().unwrap(),
            "--frame" => a.frame = Some(v.parse().unwrap()),
            "--players" => a.players = v.parse().unwrap(),
            "--spawn" => a.spawn = parse_vec3(&v),
            _ => panic!("unknown option {k}"),
        }
    }
    a
}

/// Reads a reference pose (JSON written by the screenshot watcher) and its
/// camera into the arguments. Only the fields used here are parsed, with a
/// minimal scanner, to keep the viewer free of a JSON dependency.
fn apply_ref(a: &mut Args, path: &str) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let nums_after = |key: &str, n: usize| -> Vec<f32> {
        let i = text
            .find(&format!("\"{key}\""))
            .unwrap_or_else(|| panic!("{path}: no {key}"));
        text[i + key.len() + 2..]
            .split(|c: char| {
                !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E' || c == '+')
            })
            .filter(|t| !t.is_empty() && t.parse::<f32>().is_ok())
            .take(n)
            .map(|t| t.parse().unwrap())
            .collect()
    };
    let v3 = |key: &str| {
        let v = nums_after(key, 3);
        Vec3::new(v[0], v[1], v[2])
    };
    a.pos = v3("eye");
    a.look = v3("target");
    a.up = v3("up");
    // The recorded fov is uCameraGame.mFov. The game renders with
    // mFov + the CameraFov setting of its config.ini (uCameraBase::vf21,
    // 0x007D3E00, adds the global at *0x018D1D20 + 0x24, which the
    // [GRAPHICS] CameraFov option fills). Checked live: mFov 55 with
    // CameraFov 15 gives the 70 degree projection in sCamera's viewport.
    a.fov_deg = nums_after("fov_deg", 1)[0] + a.camera_fov;
    a.near = nums_after("near", 1)[0];
    a.far = nums_after("far", 1)[0];
    // The recorded floating-origin cell, e.g. "scr\\st100\\model\\m60\\st100_61m52n":
    // the eye and target are in that cell's frame (docs/WORLD.md). A later
    // --stage/--cell on the command line overrides it.
    if let Some((stage, cell)) = origin_cell(&text) {
        a.stage = stage;
        a.cell = cell;
    }
}

/// Reads the original's lighting inputs for one moment from a JSON file
/// (written from live reads of the game's light units, docs/SHADING.md):
/// `hemisphere_count` and `hemispheres` (sky rgb, ground rgb, direction per
/// light), `light_count` and `lights` (colour rgb, direction to the light
/// per light; the first is the sun, drawn with its shadow map, the others
/// are added to the SH), `fog_color`, `fog_density`, `fog_start`,
/// `fog_end` (cm), optionally `clock` with `"+0x4c ms"` (game clock, for
/// the sky's sun). Only lights of the stage's light group belong in it.
fn load_lighting(path: &str) -> stage::StageLighting {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let nums = |key: &str, n: usize| -> Vec<f32> {
        let i = text
            .find(&format!("\"{key}\""))
            .unwrap_or_else(|| panic!("{path}: no {key}"));
        let v: Vec<f32> = text[i + key.len() + 2..]
            .split(|c: char| {
                !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E' || c == '+')
            })
            .filter(|t| !t.is_empty() && t.parse::<f32>().is_ok())
            .take(n)
            .map(|t| t.parse().unwrap())
            .collect();
        assert_eq!(v.len(), n, "{path}: {key} needs {n} numbers");
        v
    };
    let hemis = nums("hemisphere_count", 1)[0] as usize;
    let h = nums("hemispheres", 9 * hemis);
    let count = nums("light_count", 1)[0] as usize;
    let l = nums("lights", 6 * count);
    let f3 = |key: &str| {
        let v = nums(key, 3);
        [v[0], v[1], v[2]]
    };
    let fog = mtweather::FogValues {
        color: f3("fog_color"),
        density: nums("fog_density", 1)[0],
        start: nums("fog_start", 1)[0],
        end: nums("fog_end", 1)[0],
    };
    let mut out = lighting_from_units(
        h.as_chunks::<9>().0,
        l.as_chunks::<6>().0,
        &fog,
    );
    out.clock_ms = text.contains("\"+0x4c ms\"").then(|| nums("+0x4c ms", 1)[0] as u32);
    out
}

/// Lighting from the light units' values: hemisphere lights (sky rgb,
/// ground rgb, axis) and infinite lights (colour, direction to the light;
/// the first is the sun, drawn with its shadow map, the others go into
/// the SH), and the fog.
fn lighting_from_units(hemis: &[[f32; 9]], lights: &[[f32; 6]], fog: &mtweather::FogValues) -> stage::StageLighting {
    use mtweather::lighting::{pack_sh, sh_add, sh_directional, sh_hemisphere};
    let v3 = |v: &[f32]| [v[0], v[1], v[2]];
    let mut sh = [[0.0f32; 9]; 3];
    for x in hemis {
        sh_add(&mut sh, &sh_hemisphere(v3(&x[6..]), v3(x), v3(&x[3..])));
    }
    for x in lights.iter().skip(1) {
        sh_add(&mut sh, &sh_directional(v3(&x[3..]), v3(x)));
    }
    let (sun_color, sun_dir) = match lights.first() {
        Some(x) => (Vec3::from_slice(&x[0..3]), Vec3::from_slice(&x[3..6])),
        None => (Vec3::ZERO, Vec3::Y),
    };
    stage::StageLighting {
        enabled: true,
        sh: pack_sh(&sh).map(Vec4::from),
        sun_dir,
        sun_color,
        fog_color: Vec3::from(fog.color),
        fog_density: fog.density,
        fog_start: fog.start,
        fog_end: fog.end,
        clock_ms: None,
    }
}

/// Fills in the clock, the weather and, without `--light`, the lighting
/// from the game data (docs/SHADING.md, "Lighting from the data"): the
/// sky scheduler's light units at the clock's sun and the weather's fog.
fn resolve_lighting(args: &mut Args) {
    if args.character.is_some() || args.play {
        return;
    }
    let Ok(vfs) = ddvfs::Vfs::open(&args.native_pc) else { return };
    let clock = args.clock_ms.or(args.light.as_ref().and_then(|l| l.clock_ms)).unwrap_or(12 * 3_600_000);
    args.clock_ms = Some(clock);
    let minute = clock / 60_000;
    if args.weather.is_none() {
        args.weather = Some(
            args.light
                .as_ref()
                .and_then(|l| sky::weather_of_fog(&vfs, args.stage, minute, l.fog_color.to_array()))
                .unwrap_or(0),
        );
    }
    if args.light.is_some() {
        return;
    }
    let weather = args.weather.unwrap_or(0);
    let (Some(sky), Some(fog)) =
        (sky::StageSky::load(&vfs, args.stage, weather), sky::weather_fog(&vfs, args.stage, weather, minute))
    else {
        return;
    };
    let sun = mtweather::sky::sun_direction(&sky.sky, clock);
    let (hemis, lights) = sky.light_values(sun);
    let mut l = lighting_from_units(&hemis, &lights, &fog);
    l.clock_ms = Some(clock);
    args.light = Some(l);
}

/// `CameraFov` from the player's own game settings
/// (`%LOCALAPPDATA%\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini`), if present.
fn config_camera_fov() -> Option<f32> {
    let dir = std::env::var_os("LOCALAPPDATA")?;
    let path = PathBuf::from(dir).join("CAPCOM").join("DRAGONS DOGMA DARK ARISEN").join("config.ini");
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix("CameraFov="))
        .and_then(|v| v.trim().parse().ok())
}

/// `ViewRange` from the player's game settings as the config object stores
/// it (written by 0x00eddb20: NORMAL 1, FAR 2, FARTHEST 3); NORMAL if absent.
fn config_view_range() -> u32 {
    let Some(dir) = std::env::var_os("LOCALAPPDATA") else { return 1 };
    let path = PathBuf::from(dir).join("CAPCOM").join("DRAGONS DOGMA DARK ARISEN").join("config.ini");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    match text.lines().find_map(|l| l.trim().strip_prefix("ViewRange=")).map(str::trim) {
        Some("FAR") => 2,
        Some("FARTHEST") => 3,
        _ => 1,
    }
}

/// Stage and cell (A, B) of the `origin_cell` path in a reference JSON.
fn origin_cell(json: &str) -> Option<(u32, (i32, i32))> {
    let i = json.find("\"origin_cell\"")?;
    let rest = &json[i + "\"origin_cell\"".len()..];
    let value = rest.split('"').nth(1)?;
    let name = value.rsplit('\\').next()?; // st100_61m52n
    let (stage, cell) = name.strip_prefix("st")?.split_once('_')?;
    let (a, b) = cell.strip_suffix('n')?.split_once('m')?;
    Some((stage.parse().ok()?, (a.parse().ok()?, b.parse().ok()?)))
}

fn main() {
    if event::run_if_requested() {
        return;
    }
    let mut args = args();
    resolve_lighting(&mut args);
    let size = args.size;
    // Under --light the sky dome (sky.rs) covers the background; where a
    // stage has no sky scheduler it takes the fog colour, what full fog
    // fades to at the horizon.
    let clear = match &args.light {
        Some(l) => Color::linear_rgb(l.fog_color.x, l.fog_color.y, l.fog_color.z),
        None => Color::srgb(0.55, 0.68, 0.85),
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ddview".into(),
                resolution: WindowResolution::new(size.0, size.1),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(stage::ShadingPlugin)
        .add_plugins(sky::SkyPlugin)
        .insert_resource(args.light.clone().unwrap_or_default())
        .insert_resource(args)
        .insert_resource(ClearColor(clear))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 250.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (fly_camera, shot, play::step, character::animate).chain())
        .run();
}

fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut sky_materials: ResMut<Assets<sky::SkyMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    let t = std::time::Instant::now();
    let vfs = ddvfs::Vfs::open(&args.native_pc).expect("open nativePC");
    let mfx_path = args.native_pc.join("usershader").join("BbsShaderPackage.mfx");
    let mfx = mtshader::ShaderPackage::parse(&std::fs::read(mfx_path).expect("read mfx")).expect("parse mfx");
    info!(
        "indexed {} resources in {:.1} s",
        vfs.len(),
        t.elapsed().as_secs_f32()
    );
    let mut loader = stage::Loader {
        vfs: &vfs,
        mfx: &mfx,
        textures: Default::default(),
        models: Default::default(),
        model_names: Default::default(),
        water: None,
        stats: Default::default(),
        lod_eye: Some(args.pos * stage::UNIT),
        view_range: config_view_range(),
    };
    loader.index_models();
    if let Some(model) = &args.character {
        let motion = args.motion.clone().unwrap_or_default();
        match character::spawn(
            &mut commands,
            &mut loader,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut bindposes,
            model,
            &motion,
            args.slot,
            Vec3::ZERO,
            args.frame,
        ) {
            Ok(_) => info!("character {model} motion {motion} slot {}", args.slot),
            Err(e) => error!("character: {e}"),
        }
        spawn_camera_and_light(&mut commands, &args, None);
        return;
    }
    let (a0, b0) = args.cell;
    for da in -args.radius..=args.radius {
        for db in -args.radius..=args.radius {
            let offset = Vec3::new(db as f32 * 100.0, 0.0, da as f32 * 100.0);
            loader.spawn_cell(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                args.stage,
                a0 + da,
                b0 + db,
                offset,
            );
        }
    }
    // Distant ring: simplified models of the cells outside the detailed radius.
    for da in -args.far_cells..=args.far_cells {
        for db in -args.far_cells..=args.far_cells {
            if da.abs() <= args.radius && db.abs() <= args.radius {
                continue;
            }
            let offset = Vec3::new(db as f32 * 100.0, 0.0, da as f32 * 100.0);
            loader.spawn_far_cell(
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut images,
                args.stage,
                a0 + da,
                b0 + db,
                offset,
            );
        }
    }
    if args.stage == 100 {
        let eye = args.pos + Vec3::new((b0 - 50) as f32 * 10000.0, 0.0, (a0 - 50) as f32 * 10000.0);
        let n = loader.spawn_landmarks(&mut commands, &mut meshes, &mut materials, &mut images, (a0, b0), eye, args.progress);
        info!("landmarks {n}");
        let n = loader.spawn_low_models(&mut commands, &mut meshes, &mut materials, &mut images, (a0, b0), loader.view_range);
        info!("low models {n}");
    }
    loader.spawn_water(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut images,
        args.stage,
        (a0, b0),
    );
    let s = &loader.stats;
    info!("far cells {} water models {}", s.far_cells, s.water_models);
    info!(
        "cells {} instances {} (no model {}) meshes {} triangles {} textures {} problems {} ({:.1} s)",
        s.cells,
        s.instances,
        s.instances_without_model,
        s.meshes,
        s.triangles,
        s.textures,
        s.missing.len(),
        t.elapsed().as_secs_f32()
    );
    for m in s.missing.iter().take(10) {
        warn!("{m}");
    }
    if args.play {
        // Stage frame (cm) of the viewer origin: cell (a0, b0) relative to 50m50n.
        let origin = Vec3::new((b0 - 50) as f32 * 10000.0, 0.0, (a0 - 50) as f32 * 10000.0);
        // Ground and walls from the cells' collision meshes (st<S>h_/e_..._mrg00).
        let mut terrain = ddworld::Terrain::load_collision(&vfs, args.stage, a0, b0, args.radius);
        let with_collision = terrain.add_scenery(&vfs, args.stage, a0, b0, args.radius);
        info!("scenery instances with collision: {with_collision}");
        info!(
            "terrain {} cells {} triangles",
            terrain.cells,
            terrain.triangles()
        );
        match play::setup(
            &mut commands,
            &mut loader,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut bindposes,
            terrain,
            origin,
            origin + args.spawn,
            args.players,
        ) {
            Ok(mut state) => {
                state.demo = args.demo;
                commands.insert_resource(state)
            }
            Err(e) => error!("play: {e}"),
        }
    }
    // The stage's filter chain (vignette, tone curve) goes with the
    // original's lighting.
    info!(
        "clock {:?} ms, weather {:?}, lighting {}",
        args.clock_ms,
        args.weather.map(|w| sky::WEATHERS[w]),
        if args.light.is_some() { "on" } else { "Bevy PBR" }
    );
    let post = args.light.as_ref().and_then(|_| stage::post::DdPost::load(&vfs, args.stage));
    if let Some(light) = &args.light {
        match sky::StageSky::load(&vfs, args.stage, args.weather.unwrap_or(0)) {
            Some(sky) => sky.spawn(
                &mut commands,
                &mut meshes,
                &mut images,
                &mut sky_materials,
                // The sky's own sun from the capture's clock; without one,
                // the capture's light (the moon at night).
                args.clock_ms
                    .or(light.clock_ms)
                    .map(|ms| Vec3::from(mtweather::sky::sun_direction(&sky.sky, ms)))
                    .unwrap_or(light.sun_dir.normalize_or(Vec3::Y)),
                // Just inside the far plane.
                args.far * stage::UNIT * 0.9,
            ),
            None => warn!("no sky scheduler for stage {}", args.stage),
        }
    }
    spawn_camera_and_light(&mut commands, &args, post);
}

/// Sun and camera, shared by the stage and character modes.
fn spawn_camera_and_light(commands: &mut Commands, args: &Args, post: Option<stage::post::DdPost>) {
    // Sun. Without --light its direction and strength are placeholders
    // for Bevy's PBR. With --light the stage materials do their own
    // lighting and use this light only for its shadow map, pointed along
    // the original's sun direction.
    let to_light = args
        .light
        .as_ref()
        .map(|l| l.sun_dir)
        .filter(|d| d.length_squared() > 0.0)
        .unwrap_or(Vec3::new(0.4, 1.0, 0.3));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(-to_light, Vec3::Y),
    ));
    let pos = args.pos * stage::UNIT;
    let look = args.look * stage::UNIT;
    let cam = commands
        .spawn((
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: args.fov_deg.to_radians(),
                near: args.near * stage::UNIT,
                far: args.far * stage::UNIT,
                ..default()
            }),
            Transform::from_translation(pos).looking_at(look, args.up),
        ))
        .id();
    if args.light.is_some() {
        // The stage shader outputs the original's colour; no tone mapping
        // (the original's post-processing is not traced).
        commands
            .entity(cam)
            .insert(bevy::core_pipeline::tonemapping::Tonemapping::None);
        if let Some(post) = post {
            commands.entity(cam).insert(post);
        }
    }
    if args.play {
        commands.entity(cam).insert(play::FollowCam);
    } else {
        commands.entity(cam).insert(FlyCam);
    }
}

#[derive(Component)]
struct FlyCam;

fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut q: Query<&mut Transform, With<FlyCam>>,
) {
    let Ok(mut t) = q.single_mut() else { return };
    if buttons.pressed(MouseButton::Right) && motion.delta != Vec2::ZERO {
        let (mut yaw, mut pitch, _) = t.rotation.to_euler(EulerRot::YXZ);
        yaw -= motion.delta.x * 0.003;
        pitch = (pitch - motion.delta.y * 0.003).clamp(-1.54, 1.54);
        t.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }
    let mut d = Vec3::ZERO;
    let (f, r) = (*t.forward(), *t.right());
    if keys.pressed(KeyCode::KeyW) {
        d += f;
    }
    if keys.pressed(KeyCode::KeyS) {
        d -= f;
    }
    if keys.pressed(KeyCode::KeyD) {
        d += r;
    }
    if keys.pressed(KeyCode::KeyA) {
        d -= r;
    }
    if keys.pressed(KeyCode::KeyE) {
        d += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        d -= Vec3::Y;
    }
    let speed = if keys.pressed(KeyCode::ShiftLeft) {
        60.0
    } else {
        12.0
    };
    t.translation += d.normalize_or_zero() * speed * time.delta_secs();
}

/// With `--shot`: wait `shot_delay` seconds, save one frame, exit once the
/// screenshot has been written (one second later).
fn shot(
    mut commands: Commands,
    args: Res<Args>,
    time: Res<Time>,
    mut taken: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &args.shot else { return };
    let now = time.elapsed_secs();
    match *taken {
        None if now >= args.shot_delay => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            *taken = Some(now);
        }
        Some(t) if now >= t + 1.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
