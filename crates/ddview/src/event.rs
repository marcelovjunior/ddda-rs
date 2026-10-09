//! `--event <row>`: plays one cutscene from the event table with the
//! headless player `ddstory::event::EventPlayer` and shows it: the stage
//! models, the actors with their event motions, the event camera, the
//! fade and the subtitles. Stream sound cues go to the audio adapter;
//! cue diagnostics and the end are printed. This file is presentation only: every value comes from
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
//! Characters are assembled by `ddlook` and `character::spawn_assembled`:
//! one skeleton, the part mask, hair colour. NPC actors wear their event
//! looks; with `--save DDDA.sav` the player actor (and pawn actors) wear
//! the save's Arisen and pawns, else the base Arisen body.
//!
//! `ddview <nativePC> --lineup DDDA.sav[,npc:N...] [--pose <rMotionList>[:slot]]
//! [--start-frame F] [--shot out.png]` shows the save's Arisen and party
//! pawns (and NPCs) side by side, in the rest pose or held at frame F of
//! the given motion (e.g. `motion\pl\m\m00\m0000_co\m0000_co:0`).
//!
//! The stage and the characters are lit by the stage's data lighting at
//! `--hour` in fine weather (`stage::data_lighting`), when the stage has a
//! sky scheduler.
//!
//! *Not shown yet:* the sky dome, post filters and effects, body and face morphs,
//! skin and eye colours, IK. The camera's actor-joint adjustments are
//! applied with zero displacement: the viewer has no IK or body morphs, so
//! every joint sits where its local pose puts it.

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
    sex_explicit: bool,
    hour: u32,
    start_frame: u32,
    hold: bool,
    size: (u32, u32),
    shot: Option<PathBuf>,
    shot_delay: f32,
    save: Option<PathBuf>,
    /// `--lineup` entries: `npc:N` or a save file.
    lineup: Vec<String>,
    /// `--pose list[:slot]`: the lineup's motion (default: rest pose).
    pose: Option<(String, usize)>,
}

fn parse_args() -> Option<EventArgs> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    // `--event` wins when both are given; `--lineup <save>` next to it
    // then names the save that dresses the player (as `--save`).
    let ev = all.iter().position(|a| a == "--event");
    let li = all.iter().position(|a| a == "--lineup");
    let i = ev.or(li)?;
    let lineup = ev.is_none();
    let lineup_arg = li.and_then(|k| all.get(k + 1)).cloned();
    let native_pc = PathBuf::from(all.first().expect("usage: ddview <nativePC> --event <row>"));
    let mut a = EventArgs {
        exe: native_pc.parent().map(|p| p.join("DDDA.exe")).unwrap_or_default(),
        native_pc,
        row: if lineup { usize::MAX } else { all.get(i + 1).and_then(|s| s.parse().ok()).expect("--event needs a row number") },
        language: 1,
        sex: 0,
        sex_explicit: false,
        hour: 8,
        start_frame: 0,
        hold: false,
        size: (1600, 900),
        shot: None,
        shot_delay: 10.0,
        save: None,
        lineup: lineup_arg.iter().flat_map(|v| v.split(',')).map(str::to_string).collect(),
        pose: None,
    };
    if !lineup {
        // `--event N --lineup save` (the head smoke run): use the save.
        a.save = a.lineup.iter().find(|e| !e.starts_with("npc:")).map(PathBuf::from);
    }
    let mut it = all.iter().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| panic!("{k} needs a value")).clone();
        match k.as_str() {
            "--event" | "--lineup" => {
                val();
            }
            "--save" => a.save = Some(PathBuf::from(val())),
            "--exe" => a.exe = PathBuf::from(val()),
            "--lang" => a.language = val().parse().unwrap(),
            "--sex" => { a.sex = val().parse().unwrap(); a.sex_explicit = true; },
            "--hour" => a.hour = val().parse().unwrap(),
            "--start-frame" => a.start_frame = val().parse().unwrap(),
            "--hold" => a.hold = true,
            "--pose" => {
                let v = val();
                a.pose = Some(match v.rsplit_once(':') {
                    Some((l, n)) if n.parse::<usize>().is_ok() => (l.to_string(), n.parse().unwrap()),
                    _ => (v, 0),
                });
            }
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
    let Some(mut args) = parse_args() else { return false };
    if args.row == usize::MAX {
        run_lineup(args);
        return true;
    }
    let data = ddstory::data::InstallData::open(&args.native_pc, Some(&args.exe)).expect("open install");
    let vfs = ddvfs::Vfs::open(&args.native_pc).expect("open nativePC");
    let (saved, voices) = args.save.as_ref().map(|p| saved_characters(&vfs, p)).unwrap_or_default();
    // Keep the event's sex-specific sound scheduler aligned with the saved
    // actor, while preserving an explicitly requested --sex diagnostic.
    if !args.sex_explicit {
        if let Some(player) = voices.player { args.sex = player.model_or_gender as u8; }
    }
    let stage_no = data.event_table().and_then(|t| t.rows.get(args.row)).map(|r| r.stage as u32).unwrap_or(0);
    let cfg = EventConfig { language: args.language, sex: args.sex, hour: args.hour, stage: stage_no };
    let mut player = EventPlayer::open(&data, args.row, cfg).expect("open event");
    // Actor looks: NPCs from their edit data; the player and pawns from
    // the save when one is given, else the base body.
    let mut looks = std::collections::HashMap::new();
    {
        let mut probe = EventPlayer::open(&data, args.row, cfg).expect("open event");
        let f = probe.step(1.0).expect("event step");
        for a in &f.actors {
            let look = match a.unit_type {
                2 => match ddlook::npc_look(&vfs, a.unit_no as u32, a.unit_no as u32, args.row as u32) {
                    Ok(l) => Some(l.look),
                    Err(e) => {
                        eprintln!("{}: look: {e}", a.name);
                        None
                    }
                },
                // The save's Arisen, else a stand-in (ddlook::save::template_look).
                0 => saved.iter().find(|s| s.who == "arisen").map(|s| s.look.clone()).or_else(|| {
                    ddlook::save::template_look(&vfs, args.sex).map_err(|e| eprintln!("player stand-in: {e}")).ok()
                }),
                _ => saved.iter().find(|s| s.who == format!("pawn{}", a.unit_no)).map(|s| s.look.clone()),
            };
            if let Some(l) = look {
                looks.insert(a.name.clone(), l);
            }
        }
    }
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
        .add_plugins(crate::audio::GameAudioPlugin(args.native_pc.clone()))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 250.0, ..default() })
        .insert_resource(Playing { player, last, stage: stage_no, args: args.clone(), looks, voices })
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
    /// Looks per actor name (absent: the base Arisen body).
    looks: std::collections::HashMap<String, ddlook::Look>,
    voices: mtsound::voice::EventVoices,
}

/// One actor of the timeline and its motion list.
#[derive(Component)]
pub struct Actor {
    pub name: String,
    motions: Vec<Option<mtlmt::Motion>>,
    shown: i32,
    /// The event row it belongs to (the opening keeps several events'
    /// actors spawned).
    pub row: usize,
}

#[derive(Component)]
pub struct EventCam;
#[derive(Component)]
pub struct FadePlane;
#[derive(Component)]
pub struct SubtitleText;
#[derive(Component)]
pub struct InfoText;

/// The original's per-frame LOD choice for whole-stage models and their
/// placed objects (`stage::lod_level`, 0x00ea5320): a part is drawn only
/// while its mask has the level for the camera's distance to its model.
/// *Assumed:* the model's centre is its parts' bounding-box centre (the
/// original measures to the model's bounding sphere).
#[derive(Component)]
pub struct Lod {
    pub mask: u8,
    pub mid: f32,
    pub low: f32,
    /// Viewer units.
    pub centre: Vec3,
    pub view_range: u32,
}

/// The centre (model space, viewer units) of a model's parts.
fn model_centre(parts: &[stage::Part], meshes: &Assets<Mesh>) -> Vec3 {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in parts {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) =
            meshes.get(&p.mesh).and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION))
        else {
            continue;
        };
        for q in v {
            lo = lo.min(Vec3::from(*q));
            hi = hi.max(Vec3::from(*q));
        }
    }
    if lo.x > hi.x { Vec3::ZERO } else { (lo + hi) * 0.5 }
}

/// Shows each [`Lod`] part only at its level.
pub fn lod_update(cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>, mut parts: Query<(&Lod, &mut Visibility)>) {
    let _timing = crate::perf::Section::start("event::lod_update");
    let Some((_, cam)) = cams.iter().find(|(c, _)| c.is_active) else { return };
    let eye = cam.translation();
    for (l, mut v) in &mut parts {
        let level = stage::lod_level((eye - l.centre).length() / UNIT, l.mid, l.low, l.view_range);
        let want = if l.mask & level != 0 { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

fn spawn_parts(
    commands: &mut Commands,
    parts: &[stage::Part],
    transform: Transform,
    name: &str,
    centre: Vec3,
    view_range: u32,
) {
    let centre = transform.transform_point(centre);
    for p in parts {
        let mut e = commands.spawn((Mesh3d(p.mesh.clone()), transform, Name::new(name.to_string()), stage::StageEntity));
        if p.lod != 255 {
            e.insert((Lod { mask: p.lod, mid: p.lod_dist[0], low: p.lod_dist[1], centre, view_range }, Visibility::Hidden));
        }
        // only parts whose draw mode has the shadow-cast pass (as stage.rs's
        // cells): Cassardis drawn into every cascade cost 40 ms a frame
        if !p.casts_shadow {
            e.insert(bevy::light::NotShadowCaster);
        }
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
    if !args.hold && args.start_frame == 0 {
        crate::audio::cues_with_voices(&mut commands, &playing.last, playing.voices);
    }
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
        lod_eye: None,
        view_range: 1,
        tag: None,
    };
    loader.index_models();
    // The stage's lighting from the data at the event's hour (weather F),
    // as `--play --save` uses it: the stage and character materials light
    // themselves (dd_material.wgsl); the Bevy sun below then only casts
    // the shadow map, along the data's sun. Stages without a sky
    // scheduler keep Bevy's lighting.
    let light = stage::data_lighting(&vfs, playing.stage, args.hour * 3_600_000, 0);
    info!("event lighting: {}", if light.is_some() { "stage data" } else { "Bevy PBR (no sky scheduler)" });
    if let Some(l) = &light {
        commands.insert_resource(l.clone());
    }

    spawn_stage_models(&mut commands, &mut loader, &mut meshes, &mut materials, &mut images, playing.stage);
    spawn_event_actors(
        &mut commands,
        &mut loader,
        &mut meshes,
        &mut materials,
        &mut images,
        &mut bindposes,
        &playing.last,
        &playing.looks,
        args.row,
    );

    let to_light = light.as_ref().map(|l| l.sun_dir).filter(|d| d.length_squared() > 0.0).unwrap_or(Vec3::new(0.4, 1.0, 0.3));
    commands.spawn((
        DirectionalLight { illuminance: 8_000.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(-to_light, Vec3::Y),
    ));
    let cam = commands
        .spawn((
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 30f32.to_radians(), near: ddstory::event::DEFAULT_NEAR_CM * UNIT,
                far: ddstory::event::DEFAULT_FAR_CM * UNIT, ..default()
            }),
            Transform::default(),
            EventCam,
        ))
        .id();
    if light.is_some() {
        // The materials output the original's colour (as main.rs).
        commands.entity(cam).insert(bevy::core_pipeline::tonemapping::Tonemapping::None);
    }
    spawn_event_ui(&mut commands);
}

/// Stage geometry of a single-model stage: every
/// `scr\stSSS\model\stSSS_mdNN` model at the stage origin, and the `_s00`
/// / `_s10` scenery layouts of cell 00m00n at their stored positions.
/// *Assumed*: these single-cell stages use stage coordinates directly (the
/// st100 block offset of stage.rs does not apply); checked only by eye on
/// st200. Returns the number of placed objects.
pub fn spawn_stage_models(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    s: u32,
) -> usize {
    let vfs = loader.vfs;
    let prefix = format!("scr\\st{s}\\model\\st{s}_md").to_lowercase();
    let mut models: Vec<String> =
        vfs.paths_of_type(stage::RMODEL).filter(|p| p.to_lowercase().starts_with(&prefix)).map(str::to_string).collect();
    models.sort();
    for m in &models {
        if let Some(parts) = loader.model(m, meshes, materials, images) {
            // Converted meshes are already in viewer units (stage.rs).
            // *Interim:* the whole-stage models keep the near level (as the
            // view range FARTHEST does). Measured from their centre, which
            // can be tens of metres from the ground under the player, they
            // dropped near ground (maintainer's probe, st200_md06).
            // `uBaseModel.mLODType` (+0x108 bits 9-12, a fixed level when
            // not negative) is the original's override; who sets it for
            // stage models is not traced.
            let centre = model_centre(&parts, meshes);
            spawn_parts(commands, &parts, Transform::IDENTITY, m, centre, 3);
        }
    }
    let mut placed = 0;
    let mut centres: std::collections::HashMap<String, Vec3> = Default::default();
    for lot in ["s00", "s10"] {
        let lot_path = format!("scr\\st{s}\\etc\\st{s}_00m00n_{lot}");
        let Ok(bytes) = vfs.read(&lot_path, stage::RLAYOUT) else { continue };
        let Ok(records) = mtlot::parse(&bytes) else { continue };
        for r in &records {
            let Some(p) = r.placement() else { continue };
            let id = p.name.split('_').next().unwrap_or("").to_ascii_lowercase();
            let Some(path) = loader.model_names.get(&id).cloned() else { continue };
            let Some(parts) = loader.model(&path, meshes, materials, images) else { continue };
            // *Unresolved:* Euler order of mAngle (XYZ, as stage.rs).
            let t = Transform {
                translation: Vec3::from(p.position) * UNIT,
                rotation: Quat::from_euler(EulerRot::XYZ, p.angle[0], p.angle[1], p.angle[2]),
                scale: Vec3::from(p.scale),
            };
            let centre = *centres.entry(path.clone()).or_insert_with(|| model_centre(&parts, meshes));
            spawn_parts(commands, &parts, t, &p.name, centre, loader.view_range);
            placed += 1;
        }
    }
    info!("stage {s}: {} models, {placed} placed objects", models.len());
    placed
}

/// The actors of an event's first frame `first`: one skeleton per model of
/// the look, all driven by the same motion and placement. `looks` by actor
/// name (absent: the base Arisen body). Each root gets an [`Actor`] of
/// `row`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_event_actors(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
    first: &EventFrame,
    looks: &std::collections::HashMap<String, ddlook::Look>,
    row: usize,
) -> Vec<Entity> {
    let vfs = loader.vfs;
    let mut out = vec![];
    for a in &first.actors {
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
        // A list holds motions only for the cuts the actor is drawn in
        // (npc001_00 has none for cut 1): start from the cut's motion or
        // the first one present.
        let Some(slot) = present_slot(&motions, a.motion_no) else {
            error!("{}: {list_path} has no motions", a.name);
            continue;
        };
        let r = match looks.get(&a.name) {
            Some(look) => character::spawn_look(
                commands,
                loader,
                meshes,
                materials,
                images,
                bindposes,
                look,
                &list_path,
                slot,
                Vec3::ZERO,
                Some(a.motion_frame),
            ),
            None => character::spawn(
                commands,
                loader,
                meshes,
                materials,
                images,
                bindposes,
                crate::play::BODY,
                &list_path,
                slot,
                Vec3::ZERO,
                Some(a.motion_frame),
            ),
        };
        match r {
            Ok(root) => {
                commands.entity(root).insert(Actor { name: a.name.clone(), motions, shown: a.motion_no, row });
                out.push(root);
            }
            Err(e) => error!("{}: {e}", a.name),
        }
    }
    out
}

/// The fade plane (the timeline's image-plane filter), the subtitle line
/// and the info line.
pub fn spawn_event_ui(commands: &mut Commands) {
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

/// The save's Arisen and party pawns (slot `mPlayerDataManual`).
fn saved_characters(vfs: &ddvfs::Vfs, path: &std::path::Path)
    -> (Vec<ddlook::save::SavedLook>, mtsound::voice::EventVoices) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let save = ddsave::SaveFile::parse(&bytes).expect("save file");
    let looks = ddlook::save::save_looks(vfs, &save.xml, "mPlayerDataManual").expect("save looks");
    let voices = crate::audio::voices_from_save_xml(&save.xml, "mPlayerDataManual")
        .unwrap_or_else(|e| { warn!("event voice context unavailable: {e}"); Default::default() });
    (looks, voices)
}

/// One character from a look: base body skeleton, parts, mask, hair tint.
#[allow(clippy::too_many_arguments)]
fn spawn_look(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
    look: &ddlook::Look,
    motion_path: &str,
    slot: usize,
    at: Vec3,
    hold: Option<f32>,
) -> Result<Entity, String> {
    let mut parts: Vec<character::ExtraPart> = look
        .parts
        .iter()
        .map(|p| character::ExtraPart {
            path: p.path.clone(),
            tint: if p.hair {
                look.hair_color
            } else if p.beard {
                look.beard_color
            } else {
                None
            },
            attach: None,
        })
        .collect();
    parts.extend(look.weapons.iter().map(|w| character::ExtraPart {
        path: w.path.clone(),
        tint: None,
        attach: Some((w.joint, w.rotation, w.offset)),
    }));
    let mask = look.mask;
    character::spawn_assembled(
        commands,
        loader,
        meshes,
        materials,
        images,
        bindposes,
        &look.base,
        look.skin_color,
        look.body.as_ref(),
        &look.face,
        &parts,
        &move |g| mask.visible(g),
        motion_path,
        slot,
        at,
        hold,
    )
}

/// `--lineup`: the save's Arisen and pawns side by side, rest pose.
fn run_lineup(args: EventArgs) {
    let size = args.size;
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ddview lineup".into(),
                resolution: WindowResolution::new(size.0, size.1),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(stage::ShadingPlugin)
        .insert_resource(ClearColor(Color::srgb(0.35, 0.38, 0.42)))
        .insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 400.0, ..default() })
        .insert_resource(Lineup(args))
        .add_systems(Startup, setup_lineup)
        .add_systems(Update, (character::animate, lineup_shot).chain())
        .run();
}

#[derive(Resource)]
struct Lineup(EventArgs);

fn setup_lineup(
    mut commands: Commands,
    lineup: Res<Lineup>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
) {
    let args = &lineup.0;
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
        lod_eye: None,
        view_range: 1,
        tag: None,
    };
    loader.index_models();
    let mut looks: Vec<(String, ddlook::Look)> = vec![];
    for e in &args.lineup {
        if let Some(n) = e.strip_prefix("npc:") {
            let id: u32 = n.parse().expect("npc:<id>");
            match ddlook::npc_look(&vfs, id, id, 0) {
                Ok(l) => looks.push((format!("npc{id}"), l.look)),
                Err(e) => error!("npc {id}: {e}"),
            }
        } else {
            looks.extend(saved_characters(&vfs, std::path::Path::new(e)).0.into_iter().map(|s| (s.who, s.look)));
        }
    }
    let n = looks.len() as f32;
    for (i, s) in looks.iter().enumerate() {
        // 90 cm apart, centred; empty motion: the rest pose.
        let x = (i as f32 - (n - 1.0) * 0.5) * 0.9;
        let (motion, slot) = args.pose.as_ref().map_or(("", 0), |(l, n)| (l.as_str(), *n));
        match spawn_look(
            &mut commands,
            &mut loader,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut bindposes,
            &s.1,
            motion,
            slot,
            Vec3::new(x, 0.0, 0.0),
            Some(args.start_frame as f32),
        ) {
            Ok(_) => info!("{}: {} parts, base {}", s.0, s.1.parts.len(), s.1.base),
            Err(e) => error!("{}: {e}", s.0),
        }
    }
    commands.spawn((
        DirectionalLight { illuminance: 6_000.0, ..default() },
        Transform::default().looking_to(Vec3::new(-0.3, -0.6, -1.0), Vec3::Y),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 35f32.to_radians(), near: ddstory::event::DEFAULT_NEAR_CM * UNIT, far: 100.0, ..default() }),
        Transform::from_xyz(0.0, 1.1, 4.2).looking_at(Vec3::new(0.0, 0.95, 0.0), Vec3::Y),
    ));
}

fn lineup_shot(
    mut commands: Commands,
    lineup: Res<Lineup>,
    time: Res<Time>,
    mut taken: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &lineup.0.shot else { return };
    let now = time.elapsed_secs();
    match *taken {
        None if now >= lineup.0.shot_delay => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            *taken = Some(now);
        }
        Some(t) if now >= t + 1.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// The motion slot to show: the requested one if the list has it, else the
/// first present one.
fn present_slot(motions: &[Option<mtlmt::Motion>], want: i32) -> Option<usize> {
    match motions.get(want.max(0) as usize) {
        Some(Some(_)) => Some(want.max(0) as usize),
        _ => motions.iter().position(|m| m.is_some()),
    }
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
    mut commands: Commands,
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
            crate::audio::cues_with_voices(&mut commands, &f, playing.voices);
            playing.last = f;
        }
    }
    let f = &playing.last;
    let aspect = windows.single().map(|w| w.width() / w.height().max(1.0)).unwrap_or(16.0 / 9.0);
    if let Ok((mut t, mut proj)) = cams.single_mut() {
        apply_camera(f, aspect, &mut t, &mut proj);
    }
    apply_actors(f, playing.args.row, &mut actors);
    apply_ui(f, &mut fade, &mut subs);
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

/// The event camera of frame `f` on a camera (`aspect` width / height).
pub fn apply_camera(f: &EventFrame, aspect: f32, t: &mut Transform, proj: &mut Projection) {
    if let Some(c) = &f.camera {
        // Zero joint displacements (see the module note).
        if let Ok(Some(p)) = c.adjusted(&[]) {
            let eye = Vec3::from(p.position) * UNIT;
            let target = Vec3::from(p.target) * UNIT;
            *t = Transform::from_translation(eye).looking_at(target, Vec3::from(p.up));
            // This class uses mZoom, not uCameraBase's CameraFov option.
            // Its vtable slot 21 points to uMotionCamera::vf21 0x00faa920.
            let fov = ddstory::event::vertical_fov_radians(p.fov_degrees, c.zoom_degrees, c.fov_type, aspect);
            if let Projection::Perspective(pp) = proj {
                pp.fov = fov;
                pp.near = c.near_cm * UNIT;
                pp.far = c.far_cm * UNIT;
            }
        }
    }
}

/// The fade and the subtitle of frame `f`.
pub fn apply_ui(
    f: &EventFrame,
    fade: &mut Query<&mut BackgroundColor, With<FadePlane>>,
    subs: &mut Query<&mut Text, (With<SubtitleText>, Without<InfoText>)>,
) {
    if let Ok(mut bg) = fade.single_mut() {
        bg.0 = match f.fade {
            Some(c) => Color::srgba(c[0], c[1], c[2], c[3].clamp(0.0, 1.0)),
            None => Color::NONE,
        };
    }
    if let Ok(mut s) = subs.single_mut() {
        s.0 = f.subtitle.as_ref().and_then(|s| s.text.clone()).unwrap_or_default().replace('\r', "");
    }
}

/// Actors of event `row` at frame `f`: motion, frame, placement, drawn.
#[allow(clippy::type_complexity)]
pub fn apply_actors(
    f: &EventFrame,
    row: usize,
    actors: &mut Query<(&mut Actor, &mut Playback, &mut Visibility, &mut Transform), Without<EventCam>>,
) {
    for (mut a, mut pb, mut vis, mut root) in actors.iter_mut() {
        if a.row != row {
            continue;
        }
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
