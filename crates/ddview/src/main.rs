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
//!
//! `--play [--save DDDA.sav] [--goblin]` plays (play.rs): with a save, at
//! its spot, clock and weather, with its Arisen and main pawn dressed;
//! `--goblin` (repeatable) adds a goblin 6 m ahead. `--demo` scripts it.

mod audio;
mod probe;
mod perf;
mod clock;
mod intro;
mod charaedit;
mod movie;
mod stream;
mod event;
mod character;
mod doors;
mod ambient;
mod net;
mod opening;
mod vocation_menu;
mod quest_camera;
mod play;
mod bow_view;
mod items;
mod skills;
mod hud;
mod level_up_notice;
mod pause;
mod shadows;
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
    /// The reference's player position (`"player"`), else `spawn`.
    player: Option<Vec3>,
    /// `--host <port>` / `--join <addr>`: LAN co-op (net.rs); both imply
    /// `--play`. `--profile <id>` names this player's Arisen.
    host: Option<u16>,
    join: Option<String>,
    profile: Option<u64>,
    /// `--save DDDA.sav`: its XML (`ddsave`), for the party and the spot.
    save_xml: Option<std::sync::Arc<String>>,
    /// `--goblin`: goblins to add.
    goblins: usize,
    /// `--players` was given.
    players_given: bool,
    /// `--opening`: the Cassardis story slice (opening.rs).
    opening: bool,
    /// `--shots <dir>`: the opening's step screenshots.
    shots: Option<PathBuf>,
    /// `--kit fighter|mage`: the local Arisen's fight kit instead of the
    /// save weapon's.
    kit: Option<play::Kit>,
    /// `--cam-pitch rad`: the follow camera's start pitch (negative looks
    /// up from below).
    cam_pitch: Option<f32>,
    /// Start at the viewer pause menu (also useful with --shot).
    paused: bool,
    /// `--movie <name>`: play one of the game's movies (`st100ev00`, `title_A`, ...).
    movie: Option<String>,
    /// `--charaedit` / `--charaedit-female`: the character editor (charaedit.rs).
    charaedit: Option<u8>,
    /// `--skip-intro`: `--opening` starts at Cassardis (no movie, no editor).
    skip_intro: bool,
    /// The lighting was built from the data at `clock_ms` (not a capture),
    /// so the clock may advance it (clock.rs).
    light_from_data: bool,
    /// `--perf`: log frame-time statistics (perf.rs).
    perf: bool,
    /// Script the opening through the gate and a field cell boundary, without captures.
    perf_route: bool,
    /// `--no-vsync` (implied by `--perf` / `--perf-route`): present without
    /// waiting for the display, so frame times are not capped at its
    /// refresh rate.
    no_vsync: bool,
    /// Automated runs never grab the cursor or pause on focus loss.
    unattended: bool,
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
        // 6 draws the same picture as 20 at the 58m50n overlook (0 pixels
        // differ by more than 7 levels; the low models and fog cover the rest)
        // at a 10.9 ms instead of an 18 ms median frame
        far_cells: 6,
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
        host: None,
        join: None,
        profile: None,
        save_xml: None,
        goblins: 0,
        players_given: false,
        opening: false,
        shots: None,
        kit: None,
        cam_pitch: None,
        paused: false,
        movie: None,
        charaedit: None,
        skip_intro: false,
        light_from_data: false,
        perf: false,
        perf_route: false,
        no_vsync: false,
        unattended: false,
        // ref001's recorded player position (origin cell 58m50n)
        spawn: Vec3::new(215.9, 41054.3, 3921.1),
        player: None,
        up: Vec3::Y,
        fov_deg: 45.0,
        near: 10.0,
        far: 2_000_000.0,
        size: (1600, 900),
    };
    while let Some(k) = it.next() {
        if k == "--unattended" { a.unattended = true; continue; }
        if k == "--movie" { a.movie = it.next(); continue; }
        if k == "--charaedit" { a.charaedit = Some(0); continue; }
        if k == "--skip-intro" { a.skip_intro = true; continue; }
        if k == "--perf" { a.perf = true; continue; }
        if k == "--no-vsync" { a.no_vsync = true; continue; }
        if k == "--perf-route" { a.perf_route = true; continue; }
        if k == "--probe" { stage::KEEP_MESHES.store(true, std::sync::atomic::Ordering::Relaxed); continue; }
        if k == "--charaedit-female" { a.charaedit = Some(1); continue; }
        if k == "--paused" {
            a.play = true;
            a.paused = true;
            continue;
        }
        if k == "--play" || k == "--demo" {
            a.play = true;
            a.demo |= k == "--demo";
            continue;
        }
        if k == "--opening" {
            a.opening = true;
            a.play = true;
            continue;
        }
        if k == "--goblin" {
            a.goblins += 1;
            a.play = true;
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
            "--shots" => a.shots = Some(PathBuf::from(v)),
            "--players" => {
                a.players = v.parse().unwrap();
                a.players_given = true;
            }
            "--kit" => {
                a.kit = Some(match v.as_str() {
                    "fighter" => play::Kit::Fighter,
                    "mage" => play::Kit::Mage,
                    "strider" => play::Kit::Strider,
                    _ => panic!("--kit fighter|strider|mage"),
                })
            }
            "--cam-pitch" => a.cam_pitch = Some(v.parse().unwrap()),
            "--save" => {
                let bytes = std::fs::read(&v).unwrap_or_else(|e| panic!("{v}: {e}"));
                let save = ddsave::SaveFile::parse(&bytes).unwrap_or_else(|e| panic!("{v}: {e:?}"));
                a.save_xml = Some(std::sync::Arc::new(save.xml));
                a.play = true;
            }
            "--spawn" => a.spawn = parse_vec3(&v),
            "--host" => {
                a.host = Some(v.parse().unwrap());
                a.play = true;
            }
            "--join" => {
                a.join = Some(v);
                a.play = true;
            }
            "--profile" => a.profile = Some(v.parse().unwrap()),
            _ => panic!("unknown option {k}"),
        }
    }
    if a.perf_route {
        // Screenshots stall frames, so a route with shots is a visual
        // capture, not a timing benchmark (its perf numbers are invalid).
        if a.shot.is_some() || a.shots.is_some() {
            eprintln!("ddview: --perf-route with screenshots: visual capture only, perf numbers invalid");
        }
        a.opening = true;
        a.demo = true;
        a.play = true;
        a.perf = true;
        a.unattended = true;
        a.skip_intro = true;
    }
    if let Some(xml) = a.save_xml.clone() {
        apply_saved_spot(&mut a, &xml);
    }
    a
}

/// `--save`: the stage, the origin cell around the saved position, the
/// spawn, the camera start, the clock and the weather (each unless given
/// on the command line; options after `--save` override it).
fn apply_saved_spot(a: &mut Args, xml: &str) {
    let Some(spot) = play::saved_spot(xml) else {
        eprintln!("--save: no mPlayerDataManual spot; keeping the defaults");
        return;
    };
    a.stage = spot.stage;
    // the position is in the stage frame (origin at cell 50m50n); its cell
    // is (A, B) = (50 + z / 10000, 50 + x / 10000) (docs/WORLD.md)
    let cell = (50 + (spot.pos.z / 10000.0).floor() as i32, 50 + (spot.pos.x / 10000.0).floor() as i32);
    a.cell = cell;
    let origin = Vec3::new((cell.1 - 50) as f32 * 10000.0, 0.0, (cell.0 - 50) as f32 * 10000.0);
    a.spawn = spot.pos - origin;
    a.player = Some(a.spawn);
    // the camera starts behind the Arisen
    let back = Vec3::new(-spot.yaw.sin(), 0.0, -spot.yaw.cos());
    a.pos = a.spawn + back * 450.0 + Vec3::Y * 220.0;
    a.look = a.spawn + Vec3::Y * 150.0;
    a.clock_ms.get_or_insert(spot.clock_ms);
    if a.weather.is_none() && (spot.weather as usize) < sky::WEATHERS.len() {
        a.weather = Some(spot.weather as usize);
    }
    if !a.players_given {
        a.players = 1;
    }
    eprintln!(
        "--save: stage {} cell {}m{}n at ({:.0}, {:.0}, {:.0}), clock {:02}:{:02}, weather {}",
        spot.stage,
        cell.0,
        cell.1,
        spot.pos.x,
        spot.pos.y,
        spot.pos.z,
        spot.clock_ms / 3_600_000,
        spot.clock_ms / 60_000 % 60,
        spot.weather
    );
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
    if text.contains("\"player\"") {
        a.player = Some(v3("player"));
    }
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
        spot: None,
    }
}

/// Fills in the clock, the weather and, without `--light`, the lighting
/// from the game data (docs/SHADING.md, "Lighting from the data"): the
/// sky scheduler's light units at the clock's sun and the weather's fog.
fn resolve_lighting(args: &mut Args) {
    // --play keeps Bevy's lighting unless a save, a clock or a capture
    // asks for the game's.
    if args.character.is_some() || args.play && args.save_xml.is_none() && args.clock_ms.is_none() && args.light.is_none() {
        return;
    }
    let Ok(vfs) = ddvfs::Vfs::open(&args.native_pc) else { return };
    let clock = args.clock_ms.or(args.light.as_ref().and_then(|l| l.clock_ms)).unwrap_or(12 * 3_600_000);
    args.clock_ms = Some(clock);
    let minute = clock / 60_000;
    // The night spot light: not part of the --light captures (spot units
    // were not read), so it is added from the data in both cases.
    let spot = |sky: &sky::StageSky, player: Vec3| -> Option<stage::SpotLight> {
        let s = sky.lights.spot.as_ref()?;
        let sun = mtweather::sky::sun_direction(&sky.sky, clock);
        let color = Vec3::from(s.color_at(mtweather::sky::night_blend(sun)));
        if color == Vec3::ZERO {
            return None;
        }
        let (atten, falloff) = s.attenuation();
        let offset = Vec3::from(s.offset);
        Some(stage::SpotLight { pos: player + offset, dir: -offset, color, atten: Vec4::from(atten), falloff })
    };
    let player = args.player.unwrap_or(args.spawn);
    if args.weather.is_none() {
        args.weather = Some(
            args.light
                .as_ref()
                .and_then(|l| sky::weather_of_fog(&vfs, args.stage, minute, l.fog_color.to_array()))
                .unwrap_or(0),
        );
    }
    if let Some(l) = &mut args.light {
        if let Some(sky) = sky::StageSky::load(&vfs, args.stage, args.weather.unwrap_or(0)) {
            l.spot = spot(&sky, player);
        }
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
    l.spot = spot(&sky, player);
    args.light = Some(l);
    args.light_from_data = true;
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

/// `DofFilter` from the player's game settings (ON unless it says OFF).
fn config_dof_filter() -> bool {
    let Some(dir) = std::env::var_os("LOCALAPPDATA") else { return true };
    let path = PathBuf::from(dir).join("CAPCOM").join("DRAGONS DOGMA DARK ARISEN").join("config.ini");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().find_map(|l| l.trim().strip_prefix("DofFilter=")).map(str::trim) != Some("OFF")
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
    args = net::prejoin(args);
    // --opening: stand-ins walk to the awaited movie (or q0001 when the
    // intro is bypassed); stage, clock and camera start come from the host.
    let session = args.opening.then(|| {
        let exe = args.native_pc.parent().map(|p| p.join("DDDA.exe")).unwrap_or_default();
        let mut s = opening::prepare(&args.native_pc, &exe, if args.host.is_some() || args.join.is_some() { 1 } else { args.players }, args.demo, args.shots.clone(), !args.skip_intro && args.save_xml.is_none() && !args.demo && args.host.is_none() && args.join.is_none())
            .unwrap_or_else(|e| panic!("--opening: {e}"));
        if args.perf_route { s.enable_perf_route(); }
        let w = &s.host.story.as_ref().unwrap().world;
        args.stage = opening::STAGE;
        args.cell = (50, 50);
        args.clock_ms.get_or_insert((w.hour * 3600 + w.minute * 60) * 1000);
        args.weather.get_or_insert(0);
        let p = Vec3::from(s.host.sim.player(0).map(|p| p.position).unwrap_or([0.0; 3]));
        args.spawn = p;
        args.player = Some(p);
        args.pos = p + Vec3::new(0.0, 250.0, -300.0);
        args.look = p + Vec3::Y * 140.0;
        s
    });
    resolve_lighting(&mut args);
    let size = args.size;
    // Under --light the sky dome (sky.rs) covers the background; where a
    // stage has no sky scheduler it takes the fog colour, what full fog
    // fades to at the horizon.
    let clear = match &args.light {
        Some(l) => Color::linear_rgb(l.fog_color.x, l.fog_color.y, l.fog_color.z),
        None => Color::srgb(0.55, 0.68, 0.85),
    };
    let perf_enabled = args.perf;
    let no_vsync = args.no_vsync || args.perf || args.perf_route;
    let unattended = args.unattended;
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ddview".into(),
                resolution: WindowResolution::new(size.0, size.1),
                present_mode: if no_vsync {
                    bevy::window::PresentMode::AutoNoVsync
                } else {
                    bevy::window::PresentMode::AutoVsync
                },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(stage::ShadingPlugin)
        .add_plugins(audio::GameAudioPlugin(args.native_pc.clone()))
        .add_plugins(sky::SkyPlugin)
        .insert_resource(args.light.clone().unwrap_or_default())
        .insert_resource(args)
        .insert_resource(ClearColor(clear))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 250.0,
            ..default()
        })
        .init_resource::<play::Hud>()
        .add_plugins(level_up_notice::LevelUpPlugin)
        .insert_resource(OpeningSlot(std::sync::Mutex::new(session)))
        .init_resource::<pause::PauseState>()
        .init_resource::<movie::MovieFinished>()
        .init_resource::<movie::MovieSkip>()
        .add_systems(Startup, (setup, pause::setup).chain())
        .add_systems(Update, (pause::update, fly_camera, shot, (perf::update, probe::update, event::lod_update, sky_shadow), clock::update, movie::update, charaedit::update, intro::update, play::step, stream::play_hookup, stream::update, net::step, opening::step, ambient::update, doors::update, (opening::cutscene, quest_camera::show).chain(),
            character::animate.run_if(pause::animate_world.or_else(|args: Res<Args>| args.opening && (args.host.is_some() || args.join.is_some()))), bow_view::draw_arrows, shadows::update, (hud::update, vocation_menu::update).chain()).chain());

    app.add_systems(PostUpdate, character::camera_fade.after(bevy::transform::TransformSystems::Propagate));
    // Diagnostics are opt-in: render queries themselves have a cost, so
    // ordinary play must not pay for a profiling run.
    if unattended {
        // Keep scripted captures/profiling independent of desktop focus.
        // Bevy's default game settings throttle unfocused updates to 60 Hz.
        app.insert_resource(bevy::winit::WinitSettings::continuous());
    }
    if perf_enabled {
        app.add_plugins((
            bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
            bevy::diagnostic::LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(5),
                ..default()
            },
        ));
    }
    // DDVIEW_TIME_SCALE (unattended tests only): run the virtual clock N
    // times faster. Every Res<Time> consumer (the 30 Hz host frames, event
    // frames, camera blends) speeds up together; the simulation itself is
    // unchanged and deterministic, only more host frames run per rendered
    // frame. Not for play: audio and movies keep real time.
    if let Some(scale) = Some(time_scale()).filter(|v| *v != 1.0) {
        app.add_systems(Startup, move |mut time: ResMut<Time<Virtual>>| {
            time.set_relative_speed(scale);
            time.set_max_delta(std::time::Duration::from_secs_f32(0.25 * scale.max(1.0)));
            info!("ddview: virtual time x{scale} (DDVIEW_TIME_SCALE)");
        });
    }
    // an AppExit::Error (e.g. "opening failed") must fail the process, so
    // unattended runs and harnesses see it
    if let AppExit::Error(code) = app.run() {
        std::process::exit(i32::from(code.get()));
    }
}

/// `DDVIEW_TIME_SCALE` (unattended tests): the virtual clock's speed, 1
/// when unset. Catch-up caps scale with it so faster runs don't stall.
pub fn time_scale() -> f32 {
    static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| {
        std::env::var("DDVIEW_TIME_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(1.0)
    })
}

/// The opening session until setup moves it into its resource.
#[derive(Resource)]
struct OpeningSlot(std::sync::Mutex<Option<opening::Session>>);

fn setup(
    mut commands: Commands,
    args: Res<Args>,
    slot: Res<OpeningSlot>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut sky_materials: ResMut<Assets<sky::SkyMaterial>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    if let Some(name) = &args.movie {
        commands.spawn(Camera2d);
        if let Err(e) = movie::start(&mut commands, &mut images, &movie::path(&args.native_pc, name)) {
            error!("movie: {e}");
        }
        return;
    }
    if args.perf {
        commands.insert_resource(perf::Perf::default());
    }
    let t = std::time::Instant::now();
    // Kept for the whole run: the field streamer loads cells from them later.
    let vfs: &'static ddvfs::Vfs = Box::leak(Box::new(ddvfs::Vfs::open(&args.native_pc).expect("open nativePC")));
    let mfx_path = args.native_pc.join("usershader").join("BbsShaderPackage.mfx");
    let mfx: &'static mtshader::ShaderPackage = Box::leak(Box::new(
        mtshader::ShaderPackage::parse(&std::fs::read(mfx_path).expect("read mfx")).expect("parse mfx"),
    ));
    info!(
        "indexed {} resources in {:.1} s",
        vfs.len(),
        t.elapsed().as_secs_f32()
    );
    let mut loader = stage::Loader {
        vfs,
        mfx,
        textures: Default::default(),
        models: Default::default(),
        model_names: Default::default(),
        water: None,
        stats: Default::default(),
        lod_eye: Some(args.pos * stage::UNIT),
        view_range: config_view_range(),
        tag: None,
    };
    loader.index_models();
    shadows::prepare(&mut commands, vfs, &mut meshes, &mut images);
    // The sun's shadow as the stage's sky scheduler sets it (sky.rs
    // SkyShadow; applied to the directional light by sky_shadow).
    let shadow_stage = if args.opening { opening::STAGE } else { args.stage };
    commands.insert_resource(SkyShadowSetting(
        sky::StageSky::load(vfs, shadow_stage, args.weather.unwrap_or(0)).and_then(|s| s.shadow),
    ));
    if let Some(sex) = args.charaedit {
        // facing the body from the front, about chest high
        commands.spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 1.2, 3.2).looking_at(Vec3::new(0.0, 0.95, 0.0), Vec3::Y),
        ));
        commands.spawn((DirectionalLight { illuminance: 8000.0, ..default() }, Transform::from_xyz(1.0, 3.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)));
        if let Err(e) = charaedit::setup(&mut commands, vfs, mfx, sex) {
            error!("charaedit: {e}");
        }
        return;
    }
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
    if let Some(session) = slot.0.lock().unwrap().take() {
        // Present the awaited movie and editor unless the run supplies its
        // own Arisen, bypasses the intro, or is networked.
        let intro = !args.skip_intro && args.save_xml.is_none() && !args.demo && args.host.is_none() && args.join.is_none();
        if intro {
            intro::start(&mut commands, &mut images, &args.native_pc, session, vfs, mfx);
        } else {
            // seat 0 wears the save's Arisen, else a stand-in
            let saved = args.save_xml.as_deref().and_then(|x| ddlook::save::save_looks(vfs, x, "mPlayerDataManual").ok());
            let arisen = saved
                .as_ref()
                .and_then(|v| v.iter().find(|s| s.who == "arisen").map(|s| s.look.clone()))
                .or_else(|| ddlook::save::template_look(vfs, 0).ok());
            opening_world(&mut commands, &mut loader, &mut meshes, &mut materials, &mut images, &mut bindposes, session, arisen, &args, vfs);
        }
        // The world handoff for the opening's JumpArea (stream.rs): its host
        // drives the focus; the stage models it spawned are StageEntity.
        match stream::Streamer::field_grid(&args.native_pc) {
            Ok(grid) => commands.insert_resource(stream::Streamer::new(
                loader,
                vfs,
                grid,
                opening::STAGE,
                stream::Driver::External,
                (50, 50),
                args.far_cells,
                Default::default(),
            )),
            Err(e) => warn!("stream: off ({e})"),
        }
        return;
    }
    let (a0, b0) = args.cell;
    // What is spawned here, tagged for the field streamer (stream.rs).
    let mut present = std::collections::HashSet::new();
    let tag = |m: i32, n: i32, part: stage::CellPart| stage::StreamCell { m, n, part };
    for da in -args.radius..=args.radius {
        for db in -args.radius..=args.radius {
            let offset = Vec3::new(db as f32 * 100.0, 0.0, da as f32 * 100.0);
            loader.tag = Some(tag(a0 + da, b0 + db, stage::CellPart::Model));
            present.insert(tag(a0 + da, b0 + db, stage::CellPart::Model));
            present.insert(tag(a0 + da, b0 + db, stage::CellPart::Layout));
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
            loader.tag = Some(tag(a0 + da, b0 + db, stage::CellPart::Far));
            present.insert(tag(a0 + da, b0 + db, stage::CellPart::Far));
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
    loader.tag = None;
    if args.stage == 100 {
        let eye = args.pos + Vec3::new((b0 - 50) as f32 * 10000.0, 0.0, (a0 - 50) as f32 * 10000.0);
        let n = loader.spawn_landmarks(&mut commands, &mut meshes, &mut materials, &mut images, (a0, b0), eye, args.progress);
        info!("landmarks {n}");
        // with field streaming (--play offline) the streamer keeps them by cell
        if !(args.play && args.host.is_none() && args.join.is_none()) {
            let n = loader.spawn_low_models(&mut commands, &mut meshes, &mut materials, &mut images, (a0, b0), loader.view_range);
            info!("low models {n}");
        }
    }
    if !args.play {
        // Static nonfield reference captures need the authored stage bundle.
        if args.stage != 100 {
            event::spawn_stage_models(&mut commands,&mut loader,&mut meshes,&mut materials,&mut images,args.stage);
        }
        let cells:Vec<_>=(-args.radius..=args.radius).flat_map(|dm|
            (-args.radius..=args.radius).filter_map(move |dn|
                Some((u16::try_from(a0+dm).ok()?,u16::try_from(b0+dn).ok()?)))).collect();
        let world=ddhost::ddspawn::World{scene:args.progress,hour:args.clock_ms.unwrap_or(43_200_000)/3_600_000,flag:&|_|false};
        let origin=if args.stage==100 {Vec3::new((b0-50) as f32*10000.0,0.0,(a0-50) as f32*10000.0)}else{Vec3::ZERO};
        if let Err(e)=doors::preview(&mut commands,&mut loader,&mut meshes,&mut materials,&mut images,
            &mut bindposes,args.stage,&cells,&world,origin){warn!("door preview: {e}");}
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
        let mut terrain = ddworld::Terrain::load_collision(vfs, args.stage, a0, b0, args.radius);
        let with_collision = terrain.add_scenery(vfs, args.stage, a0, b0, args.radius);
        info!("scenery instances with collision: {with_collision}");
        info!(
            "terrain {} cells {} triangles",
            terrain.cells,
            terrain.triangles()
        );
        let party = party(vfs, &args, origin);
        let data = match play::fight_data(vfs, &args.native_pc) {
            Ok(d) => Some(d),
            Err(e) => {
                error!("play: fight data: {e}");
                None
            }
        };
        match play::setup(
            &mut commands,
            &mut loader,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut bindposes,
            terrain,
            origin,
            party,
            data,
        ) {
            Ok(mut state) => {
                state.demo = args.demo;
                if args.host.is_none() && args.join.is_none() {
                    skills::equip(&mut state.sim, &state.locals, args.save_xml.as_deref().map(String::as_str));
                    info!("skills: slots 1-6 (P1), Numpad4-9 (P2); base-level supported actions");
                    if let Some(xml) = args.save_xml.as_deref() {
                        match items::QuickItems::load(vfs, xml, &state.locals) {
                            Ok(items) => {
                                info!("quick-use: U (P1), Numpad0 (P2); first useful saved restorative");
                                commands.insert_resource(items);
                            }
                            Err(e) => error!("quick-use: {e}"),
                        }
                    }
                }
                if let Some(p) = args.cam_pitch {
                    state.cam_pitch = p;
                }
                let mode = match (&args.host, &args.join) {
                    (Some(port), _) => Some(net::Mode::Host { port: *port }),
                    (None, Some(addr)) => Some(net::Mode::Join { addr: addr.clone() }),
                    _ => None,
                };
                if let Some(mode) = mode {
                    let place = (args.stage, a0, b0, args.radius);
                    let spawn = origin + args.spawn;
                    let yaw = args.save_xml.as_deref().and_then(|x| play::saved_spot(x)).map_or(0.0, |s| s.yaw);
                    if let Err(e) =
                        net::setup(&mut commands, vfs, &args.native_pc, mode, args.profile, place, (spawn, yaw), &mut state)
                    {
                        error!("net: {e}");
                    }
                }
                commands.insert_resource(state);
                if let Err(e)=hud::spawn(&mut commands,&vfs,&mut images){error!("HUD: {e}");}
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
    let post = args.light.as_ref().and_then(|_| stage::post::DdPost::load(vfs, args.stage)).map(|p| {
        // DOFFilter runs only with the player's DofFilter=ON (0x0050adc0).
        match stage::post::DdPost::dof_filter(vfs, args.stage).filter(|_| config_dof_filter()) {
            Some(dof) => p.with_dof(&dof, args.near, args.far, args.size),
            None => p,
        }
    });
    if let Some(light) = &args.light {
        let sky = sky::StageSky::load(vfs, args.stage, args.weather.unwrap_or(0));
        if let (Some(sky), Some(ms)) = (&sky, args.clock_ms.or(light.clock_ms)) {
            // The cloud domes, at the sky's height offset above the camera's
            // ground position.
            let sun = mtweather::sky::sun_direction(&sky.sky, ms);
            let day = 1.0 - mtweather::sky::night_blend(sun);
            let pos = Vec3::new(args.pos.x, sky.inputs.height_offset, args.pos.z) * stage::UNIT;
            for c in &sky.clouds {
                let alpha = if c.day_only { day } else { 1.0 };
                if alpha <= 0.0 {
                    continue;
                }
                let (s, a) = mtweather::sky::cloud_light(&sky.sky, &sky.inputs, sun, &c.light);
                let light = Vec3::from(s) + Vec3::from(a);
                loader.spawn_sky_model(&mut commands, &mut meshes, &mut materials, &mut images, &c.model, light, alpha, pos);
            }
        }
        match sky {
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
                args.pos,
            ),
            None => warn!("no sky scheduler for stage {}", args.stage),
        }
    }
    spawn_camera_and_light(&mut commands, &args, post);
    // The clock advances in play when the lighting comes from the data.
    if args.play && args.light_from_data {
        if let Some(ms) = args.clock_ms {
            commands.insert_resource(clock::GameClock::new(vfs, args.stage, args.weather.unwrap_or(0), ms));
        }
    }
    // Field streaming: offline --play on stage 100 (the grid descriptor is
    // read from the installed exe).
    if args.play && args.stage == 100 && args.host.is_none() && args.join.is_none() {
        match stream::Streamer::field_grid(&args.native_pc) {
            Ok(grid) => {
                info!("stream: on ({} cells tagged at start)", present.len());
                commands.insert_resource(stream::Streamer::new(
                    loader,
                    vfs,
                    grid,
                    args.stage,
                    stream::Driver::Play,
                    (a0, b0),
                    args.far_cells,
                    present,
                ));
            }
            Err(e) => warn!("stream: off ({e})"),
        }
    }
}

/// The opening's world: its stage, the party (seat 0 wears `arisen`), the
/// HUD and the camera. Called at start, or by the intro once the edit is
/// confirmed.
#[allow(clippy::too_many_arguments)]
pub fn opening_world(
    commands: &mut Commands,
    loader: &mut stage::Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
    session: opening::Session,
    arisen: Option<ddlook::Look>,
    args: &Args,
    vfs: &ddvfs::Vfs,
) {
    let res = opening::OpeningRes(std::sync::Mutex::new(session));
    match vocation_menu::View::load(vfs, &args.native_pc, images) {
        Ok(view) => commands.insert_resource(view),
        Err(error) => panic!("opening vocation menu: {error}"),
    };
    let looks = if args.join.is_some() { vec![None, arisen] } else { vec![arisen] };
    if let Some(state) = opening::setup(commands, loader, meshes, materials, images, bindposes, &res, looks, args.demo, args.host.is_some() || args.join.is_some()) {
        commands.insert_resource(state);
        if let Err(e) = hud::spawn(commands, vfs, images) {
            error!("HUD: {e}");
        }
    }
    commands.insert_resource(res);
    spawn_camera_and_light(commands, args, None);
}

/// Who plays (play.rs): with `--save`, its Arisen (seat 0) and main pawn,
/// dressed; more seats and the network's other seats are stand-ins
/// (`ddlook::save::template_look`); without a save every seat is the base
/// body. A network guest's own seat (1) wears its save.
fn party(vfs: &ddvfs::Vfs, args: &Args, origin: Vec3) -> play::Party {
    let saved = args.save_xml.as_deref().map(|xml| {
        ddlook::save::save_looks(vfs, xml, "mPlayerDataManual").unwrap_or_else(|e| {
            error!("--save: looks: {e}");
            vec![]
        })
    });
    let spot = args.save_xml.as_deref().and_then(|x| play::saved_spot(x));
    let pick = |who: &str| saved.as_ref().and_then(|v| v.iter().find(|s| s.who == who)).map(|s| s.look.clone());
    let stand_in = |sex: u8| ddlook::save::template_look(vfs, sex).ok();
    let joining = args.join.is_some();
    let net = args.host.is_some() || joining;
    let seats = if net { args.players.max(2) } else { args.players.max(1) };
    let mut arisen = vec![];
    let mut kits = std::collections::HashMap::new();
    for id in 0..seats {
        // players side by side, 1.5 m apart
        let pos = origin + args.spawn + Vec3::new(150.0 * f32::from(id), 0.0, 0.0);
        let own = if joining { id == 1 } else { id == 0 };
        let look = match (&saved, own) {
            (Some(_), true) => pick("arisen"),
            (Some(_), false) => stand_in(0),
            (None, _) if net && !own => stand_in(0),
            _ => None,
        };
        let kit = if own { args.kit.unwrap_or_else(|| play::kit_of(look.as_ref())) } else { play::Kit::Fighter };
        kits.insert(id, kit);
        arisen.push((id, look, pos, spot.as_ref().map_or(0.0, |s| s.yaw)));
    }
    // The main pawn serves seat 0: the host's (or a single player's) save
    // pawn; a guest sees a stand-in.
    let pawn = match (&saved, &spot) {
        (Some(_), Some(s)) if !joining => Some((pick("pawn0"), s.pawn.map_or(s.pos + Vec3::X * 150.0, |p| p.0), s.pawn.map_or(s.yaw, |p| p.1))),
        _ if joining => Some((stand_in(0), origin + args.spawn + Vec3::X * 300.0, 0.0)),
        _ => None,
    };
    play::Party { arisen, pawn, goblins: if joining { 0 } else { args.goblins }, kits }
}

/// The stage's sun shadow settings, once read.
#[derive(Resource)]
struct SkyShadowSetting(Option<sky::SkyShadow>);

/// The original constructor 0x00eefc60 enables three cascades; 0x00ee5ce0
/// splits their ranges using mShadowDistribution. We apply those ranges to
/// Bevy's projection; the original projection fitting, quality-dependent
/// atlas and receiver filtering remain separate fidelity work.
fn sky_shadow(
    mut commands: Commands,
    setting: Option<Res<SkyShadowSetting>>,
    cameras: Query<&Projection, With<Camera3d>>,
    mut lights: Query<(Entity, &mut Transform, &mut DirectionalLight), Without<ShadowSet>>,
) {
    let Some(setting) = setting else { return };
    let Some(sh) = setting.0 else { return };
    let Ok(Projection::Perspective(camera)) = cameras.single() else { return };
    let near = camera.near;
    let far = camera.far.min(sh.view_distance * stage::UNIT);
    // Three automatic cascades, ctor0x00eefc60 (+0x170/+0x174/+0x180).
    // These settings are not exposed by the stable-shadow reflection.
    let Some(bounds) = mtweather::shadow::cascade_boundaries(near, far, 3, sh.distribution) else { return };
    for (e, mut transform, mut light) in &mut lights {
        // The custom stage material retains the direct sun direction.
        // This Bevy light provides its shadow map: weather0x004c1110 and
        // uSkyStableShadow0x00c6c970 use the separately elevated direction.
        let sun = transform.rotation * Vec3::Z;
        let shadow = Vec3::from(mtweather::sky::shadow_direction(sun.to_array(), sh.direction_limit));
        transform.rotation = Transform::IDENTITY.looking_to(-shadow, Vec3::Y).rotation;
        // Root shader VS0x0a75e0 displaces the caster away from light by
        // depth-bias cm;0x00edf810 supplies -shadow direction. Bevy's
        // equivalent receiver bias is towards light, in world units.
        // Its separate normal bias still approximates the source slope bias.
        light.shadow_depth_bias = sh.depth_bias_cm * stage::UNIT;
        let config = bevy::light::CascadeShadowConfig {
            bounds: bounds.clone(),
            minimum_distance: near,
            overlap_proportion: 0.0,
        };
        commands.entity(e).insert((config, ShadowSet));
    }
}

#[derive(Component)]
struct ShadowSet;

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
            // DDVIEW_NO_SHADOWS: performance experiments only
            shadow_maps_enabled: std::env::var_os("DDVIEW_NO_SHADOWS").is_none(),
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
            // A float frame: the depth of field reads the view depth from
            // its alpha.
            commands.entity(cam).insert((post, bevy::camera::Hdr));
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
