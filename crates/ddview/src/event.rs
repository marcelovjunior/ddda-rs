//! `--event <row>`: plays one cutscene from the event table with the
//! headless player `ddstory::event::EventPlayer` and shows it: the stage
//! models, the actors with their event motions, the event camera, the
//! fade and the subtitles. Sound cues and the end are printed (there is
//! no audio yet). This file is presentation only: every value comes from
//! the event player.
//!
//! ```text
//! ddview <nativePC> --event 22 [--exe DDDA.exe] [--lang 1] [--sex 0] [--hour 8]
//!        [--start-frame F] [--hold] [--size WxH] [--shot out.png] [--shot-delay secs]
//! ```
//! `--start-frame` steps the event headless to frame F first; `--hold`
//! freezes it there (for screenshots). Without `--exe` the exe is looked
//! for next to `nativePC`.
//!
//! *Not shown yet:* stage lighting and effects, the camera's
//! actor-joint adjustments and `mOffsetPos`, the actors' looks (every
//! actor uses the base Arisen body as a stand-in), IK.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::WindowResolution;
use ddstory::data::Data;
use ddstory::event::{EventConfig, EventFrame, EventPlayer};

use crate::character::{self, Playback, RMOTIONLIST};
use crate::stage::{self, Loader, PartMaterial, PendingDd, UNIT};

#[derive(Clone)]
struct EventArgs {
    native_pc: PathBuf,
    exe: PathBuf,
    row: usize,
    language: usize,
    sex: u8,
    hour: u32,
    start_frame: u32,
    hold: bool,
    size: (u32, u32),
    shot: Option<PathBuf>,
    shot_delay: f32,
}

fn parse_args() -> Option<EventArgs> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let i = all.iter().position(|a| a == "--event")?;
    let native_pc = PathBuf::from(all.first().expect("usage: ddview <nativePC> --event <row>"));
    let mut a = EventArgs {
        exe: native_pc.parent().map(|p| p.join("DDDA.exe")).unwrap_or_default(),
        native_pc,
        row: all.get(i + 1).and_then(|s| s.parse().ok()).expect("--event needs a row number"),
        language: 1,
        sex: 0,
        hour: 8,
        start_frame: 0,
        hold: false,
        size: (1600, 900),
        shot: None,
        shot_delay: 10.0,
    };
    let mut it = all.iter().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| panic!("{k} needs a value")).clone();
        match k.as_str() {
            "--event" => {
                val();
            }
            "--exe" => a.exe = PathBuf::from(val()),
            "--lang" => a.language = val().parse().unwrap(),
            "--sex" => a.sex = val().parse().unwrap(),
            "--hour" => a.hour = val().parse().unwrap(),
            "--start-frame" => a.start_frame = val().parse().unwrap(),
            "--hold" => a.hold = true,
            "--shot" => a.shot = Some(PathBuf::from(val())),
            "--shot-delay" => a.shot_delay = val().parse().unwrap(),
            "--size" => {
                let v = val();
                let p: Vec<u32> = v.split('x').map(|x| x.parse().unwrap()).collect();
                a.size = (p[0], p[1]);
            }
            _ => {}
        }
    }
    Some(a)
}

/// Runs the event viewer when the command line has `--event`; returns
/// false otherwise (the caller goes on with its own modes).
pub fn run_if_requested() -> bool {
    let Some(args) = parse_args() else { return false };
    let data = ddstory::data::InstallData::open(&args.native_pc, Some(&args.exe)).expect("open install");
    let stage_no = data.event_table().and_then(|t| t.rows.get(args.row)).map(|r| r.stage as u32).unwrap_or(0);
    let cfg = EventConfig { language: args.language, sex: args.sex, hour: args.hour, stage: stage_no };
    let mut player = EventPlayer::open(&data, args.row, cfg).expect("open event");
    // Frames 0..=start_frame, so the first shown frame has its actors.
    let mut last = EventFrame::default();
    for _ in 0..=args.start_frame {
        last = player.step(1.0).expect("event step");
        log_frame(&last);
    }
    info!("event row {} ({}) end frame {}", args.row, player.path, player.end_frame);
    let size = args.size;
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("ddview event {}", args.row),
                resolution: WindowResolution::new(size.0, size.1),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(stage::ShadingPlugin)
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 250.0, ..default() })
        .insert_resource(Playing { player, last, stage: stage_no, args: args.clone() })
        .add_systems(Startup, setup)
        .add_systems(Update, (drive, character::animate, shot).chain())
        .run();
    true
}

#[derive(Resource)]
struct Playing {
    player: EventPlayer,
    last: EventFrame,
    stage: u32,
    args: EventArgs,
}

/// One actor of the timeline and its motion list.
#[derive(Component)]
struct Actor {
    name: String,
    motions: Vec<Option<mtlmt::Motion>>,
    shown: i32,
}

#[derive(Component)]
struct EventCam;
#[derive(Component)]
struct FadePlane;
#[derive(Component)]
struct SubtitleText;
#[derive(Component)]
struct InfoText;

fn spawn_parts(commands: &mut Commands, parts: &[stage::Part], transform: Transform, name: &str) {
    for p in parts {
        let mut e = commands.spawn((Mesh3d(p.mesh.clone()), transform, Name::new(name.to_string())));
        match &p.material {
            PartMaterial::Standard(h) => e.insert(MeshMaterial3d(h.clone())),
            PartMaterial::Dd(slot) => e.insert(PendingDd(slot.clone())),
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    playing: Res<Playing>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    let args = &playing.args;
    let vfs = ddvfs::Vfs::open(&args.native_pc).expect("open nativePC");
    let mfx_path = args.native_pc.join("usershader").join("BbsShaderPackage.mfx");
    let mfx = mtshader::ShaderPackage::parse(&std::fs::read(mfx_path).expect("read mfx")).expect("parse mfx");
    let mut loader = Loader {
        vfs: &vfs,
        mfx: &mfx,
        textures: Default::default(),
        models: Default::default(),
        model_names: Default::default(),
        water: None,
        stats: Default::default(),
    };
    loader.index_models();

    // Stage geometry: every `scr\stSSS\model\stSSS_mdNN` model at the
    // stage origin, and the `_s00` / `_s10` scenery layouts of cell
    // 00m00n at their stored positions. *Assumed*: these single-cell
    // stages use stage coordinates directly (the st100 block offset of
    // stage.rs does not apply); checked only by eye on st200.
    let s = playing.stage;
    let prefix = format!("scr\\st{s}\\model\\st{s}_md").to_lowercase();
    let mut models: Vec<String> =
        vfs.paths_of_type(stage::RMODEL).filter(|p| p.to_lowercase().starts_with(&prefix)).map(str::to_string).collect();
    models.sort();
    for m in &models {
        if let Some(parts) = loader.model(m, &mut meshes, &mut materials, &mut images) {
            // Converted meshes are already in viewer units (stage.rs).
            spawn_parts(&mut commands, &parts, Transform::IDENTITY, m);
        }
    }
    let mut placed = 0;
    for lot in ["s00", "s10"] {
        let lot_path = format!("scr\\st{s}\\etc\\st{s}_00m00n_{lot}");
        let Ok(bytes) = vfs.read(&lot_path, stage::RLAYOUT) else { continue };
        let Ok(records) = mtlot::parse(&bytes) else { continue };
        for r in &records {
            let Some(p) = r.placement() else { continue };
            let id = p.name.split('_').next().unwrap_or("").to_ascii_lowercase();
            let Some(path) = loader.model_names.get(&id).cloned() else { continue };
            let Some(parts) = loader.model(&path, &mut meshes, &mut materials, &mut images) else { continue };
            // *Unresolved:* Euler order of mAngle (XYZ, as stage.rs).
            let t = Transform {
                translation: Vec3::from(p.position) * UNIT,
                rotation: Quat::from_euler(EulerRot::XYZ, p.angle[0], p.angle[1], p.angle[2]),
                scale: Vec3::from(p.scale),
            };
            spawn_parts(&mut commands, &parts, t, &p.name);
            placed += 1;
        }
    }
    info!("stage {s}: {} models, {placed} placed objects", models.len());

    // Actors: the base Arisen body as a stand-in for every look.
    for a in &playing.last.actors {
        let Some(list_path) = a.motion_list.clone() else { continue };
        let motions = match vfs.read(&list_path, RMOTIONLIST).map_err(|e| e.to_string()).and_then(|b| {
            mtlmt::parse(&b).map_err(|e| format!("{list_path}: {e}"))
        }) {
            Ok(l) => l.motions,
            Err(e) => {
                error!("{}: {e}", a.name);
                continue;
            }
        };
        match character::spawn(
            &mut commands,
            &mut loader,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut bindposes,
            crate::play::BODY,
            &list_path,
            a.motion_no.max(0) as usize,
            Vec3::ZERO,
            Some(a.motion_frame),
        ) {
            Ok(root) => {
                commands.entity(root).insert(Actor { name: a.name.clone(), motions, shown: a.motion_no });
            }
            Err(e) => error!("{}: {e}", a.name),
        }
    }

    commands.spawn((
        DirectionalLight { illuminance: 8_000.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 30f32.to_radians(), near: 0.05, far: 2000.0, ..default() }),
        Transform::default(),
        EventCam,
    ));
    // Fade plane (the timeline's image-plane filter) and subtitle.
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::NONE),
        FadePlane,
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Percent(8.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(Text::new(""), TextFont::from_font_size(30.0), TextColor(Color::WHITE), TextLayout::justify(Justify::Center), SubtitleText)],
    ));
    commands.spawn((
        Node { position_type: PositionType::Absolute, top: Val::Px(8.0), left: Val::Px(8.0), ..default() },
        children![(Text::new(""), TextFont::from_font_size(16.0), TextColor(Color::srgb(0.8, 0.8, 0.8)), InfoText)],
    ));
}

fn log_frame(f: &EventFrame) {
    for c in &f.cues {
        info!("frame {:?}: sound cue {c:?}", f.frame);
    }
    if f.finished {
        info!("event finished at frame {:?}: the host now reports EventFinished to ddstory (SceNo raise, party placement)", f.frame);
    }
}

#[allow(clippy::too_many_arguments)]
fn drive(
    time: Res<Time>,
    mut playing: ResMut<Playing>,
    mut acc: Local<f32>,
    mut cams: Query<(&mut Transform, &mut Projection), With<EventCam>>,
    mut actors: Query<(&mut Actor, &mut Playback, &mut Visibility, &mut Transform), Without<EventCam>>,
    mut fade: Query<&mut BackgroundColor, With<FadePlane>>,
    mut subs: Query<&mut Text, (With<SubtitleText>, Without<InfoText>)>,
    mut info_text: Query<&mut Text, (With<InfoText>, Without<SubtitleText>)>,
    windows: Query<&Window>,
) {
    // mDeltaTime: frames at 30 per second.
    if !playing.args.hold {
        *acc += time.delta_secs().min(0.1) * 30.0;
        while *acc >= 1.0 && !playing.player.finished() {
            *acc -= 1.0;
            let f = playing.player.step(1.0).expect("event step");
            log_frame(&f);
            playing.last = f;
        }
    }
    let f = &playing.last;
    if let (Some(c), Ok((mut t, mut proj))) = (&f.camera, cams.single_mut()) {
        if let Some(p) = c.pose {
            let eye = Vec3::from(p.position) * UNIT;
            let target = Vec3::from(p.target) * UNIT;
            *t = Transform::from_translation(eye).looking_at(target, Vec3::from(p.up));
            let aspect = windows.single().map(|w| w.width() / w.height().max(1.0)).unwrap_or(16.0 / 9.0);
            let mut fov = p.fov_degrees.clamp(0.01, 179.99).to_radians();
            // FOV type 1 is horizontal (uMotionCamera::vf21 0x00faa920).
            if c.fov_type != 0 {
                fov = 2.0 * ((fov * 0.5).tan() / aspect).atan();
            }
            if let Projection::Perspective(pp) = proj.as_mut() {
                pp.fov = fov;
            }
        }
    }
    for (mut a, mut pb, mut vis, mut root) in &mut actors {
        let Some(v) = f.actors.iter().find(|v| v.name == a.name) else { continue };
        if v.motion_no != a.shown {
            if let Some(Some(m)) = a.motions.get(v.motion_no.max(0) as usize) {
                pb.motion = m.clone();
                pb.blend = None;
            }
            a.shown = v.motion_no;
        }
        pb.hold_frame = Some(v.motion_frame);
        // Joint 255 carries the actor's placement in stage coordinates
        // (character::animate skips it); *assumed* from the data: the
        // event motions' root tracks hold absolute positions near the
        // event camera. mOffsetPos is added on top (*assumed*).
        let off = v.offset_pos.map(|o| Vec3::new(o[0], o[1], o[2])).unwrap_or(Vec3::ZERO);
        for t in pb.motion.tracks.iter().filter(|t| t.joint == 255) {
            let x = t.sample(v.motion_frame);
            match t.usage {
                mtlmt::Usage::LocalPosition | mtlmt::Usage::AbsolutePosition => {
                    root.translation = (Vec3::new(x[0], x[1], x[2]) + off) * UNIT
                }
                mtlmt::Usage::LocalRotation | mtlmt::Usage::AbsoluteRotation => {
                    root.rotation = Quat::from_xyzw(x[0], x[1], x[2], x[3])
                }
                _ => {}
            }
        }
        *vis = if v.draw { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut bg) = fade.single_mut() {
        bg.0 = match f.fade {
            Some(c) => Color::srgba(c[0], c[1], c[2], c[3].clamp(0.0, 1.0)),
            None => Color::NONE,
        };
    }
    if let Ok(mut s) = subs.single_mut() {
        s.0 = f.subtitle.as_ref().and_then(|s| s.text.clone()).unwrap_or_default().replace('\r', "");
    }
    if let Ok(mut i) = info_text.single_mut() {
        let cut = f.camera.as_ref().map(|c| c.cut).unwrap_or(-1);
        let label = f.subtitle.as_ref().map(|s| s.label.as_str()).unwrap_or("-");
        i.0 = format!(
            "event row {} frame {} / {}  cut {cut}  subtitle {label}{}",
            playing.args.row,
            f.frame.map(|x| x as i64).unwrap_or(-1),
            playing.player.end_frame,
            if f.finished { "  (finished)" } else { "" }
        );
    }
}

fn shot(
    mut commands: Commands,
    playing: Res<Playing>,
    time: Res<Time>,
    mut taken: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &playing.args.shot else { return };
    let now = time.elapsed_secs();
    match *taken {
        None if now >= playing.args.shot_delay => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            *taken = Some(now);
        }
        Some(t) if now >= t + 1.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
