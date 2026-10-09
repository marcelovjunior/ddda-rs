//! `ddview`: M1 viewer. Loads a block of stage cells from the user's
//! Dragon's Dogma: Dark Arisen install and shows them with a free camera.
//!
//! ```text
//! ddview <nativePC> [--stage 100] [--cell 58,50] [--radius 2]
//!        [--pos x,y,z] [--look x,y,z] [--shot out.png] [--shot-delay secs]
//! ```
//! `--pos`/`--look` are in game units (cm) in the origin cell's frame, the
//! same frame as the live game's `uPlayer +0x40` (see docs/WORLD.md).
//! With `--shot` the viewer saves one frame after `--shot-delay` seconds
//! (default 10: Bevy compiles pipelines asynchronously and draws nothing
//! until they are ready) and exits.
//!
//! Controls: WASD move, Q/E down/up, Shift faster, hold right mouse to look.

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
    pos: Vec3,
    look: Vec3,
    shot: Option<PathBuf>,
    shot_delay: f32,
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
        pos: Vec3::new(205.4, 41019.0 + 170.0, 3542.5),
        look: Vec3::new(205.4 + 1000.0, 41019.0 + 100.0, 3542.5),
        shot: None,
        shot_delay: 10.0,
    };
    while let Some(k) = it.next() {
        let v = it.next().unwrap_or_else(|| panic!("{k} needs a value"));
        match k.as_str() {
            "--stage" => a.stage = v.parse().unwrap(),
            "--cell" => {
                let p: Vec<i32> = v.split(',').map(|x| x.parse().unwrap()).collect();
                a.cell = (p[0], p[1]);
            }
            "--radius" => a.radius = v.parse().unwrap(),
            "--pos" => a.pos = parse_vec3(&v),
            "--look" => a.look = parse_vec3(&v),
            "--shot" => a.shot = Some(PathBuf::from(v)),
            "--shot-delay" => a.shot_delay = v.parse().unwrap(),
            _ => panic!("unknown option {k}"),
        }
    }
    a
}

fn main() {
    let args = args();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ddview".into(),
                resolution: WindowResolution::new(1600, 900),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(args)
        .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 600.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (fly_camera, shot))
        .run();
}

fn setup(
    mut commands: Commands,
    args: Res<Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
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
        stats: Default::default(),
    };
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
    let s = &loader.stats;
    info!(
        "cells {} meshes {} triangles {} textures {} problems {} ({:.1} s)",
        s.cells,
        s.meshes,
        s.triangles,
        s.textures,
        s.missing.len(),
        t.elapsed().as_secs_f32()
    );
    for m in s.missing.iter().take(10) {
        warn!("{m}");
    }
    // Sun: direction is a placeholder until the stage's light data is read.
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
    ));
    let pos = args.pos * stage::UNIT;
    let look = args.look * stage::UNIT;
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(pos).looking_at(look, Vec3::Y),
        FlyCam,
    ));
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
