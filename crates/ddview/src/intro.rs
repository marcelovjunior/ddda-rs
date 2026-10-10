//! Presentation of the movie and editor requested by the opening story.
//! Preparation still walks the prologue with scripted stand-ins, but stops
//! at an authored movie event. Playback completion releases that event;
//! the quest's stage-801 jump admits the editor. Confirmation then resumes
//! the remaining dragon stand-in route into Cassardis.
//! Unattended movie completion after three seconds is automation only.
//! Editor automation requires an explicit DDVIEW_CHARACTER_NAME fixture.
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::charaedit::{CharaEdit, EditResult};
use crate::movie::MovieFinished;
use crate::stream::Streamer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Movie,
    Edit,
    Blocked,
}

/// The intro in progress; the opening session waits in it.
#[derive(Resource)]
pub struct Intro {
    phase: Phase,
    // Mutex: the session is Send but not Sync, as a resource must be
    session: std::sync::Mutex<Option<crate::opening::Session>>,
    vfs: &'static ddvfs::Vfs,
    mfx: &'static mtshader::ShaderPackage,
    /// The intro's own camera (2D for the movie, 3D for the editor).
    camera: Entity,
    /// App seconds when the phase began.
    since: Option<f64>,
    // Project co-op policy: local profiles use the shared keyboard in roster
    // order. The story's stage-801 gate is released after every local edit.
    owners: Vec<(ddhost::PlayerId, ddhost::ProfileId)>,
    owner_index: usize,
}

/// Starts the movie that the quest requested; decoding failure blocks the event.
pub fn start(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    _native_pc: &std::path::Path,
    session: crate::opening::Session,
    vfs: &'static ddvfs::Vfs,
    mfx: &'static mtshader::ShaderPackage,
) {
    let movie_path = session
        .intro_movie
        .clone()
        .expect("intro requires awaited movie event");
    let owners = session
        .host
        .roster
        .seats()
        .iter()
        .filter(|seat| seat.control == ddhost::Control::Local)
        .map(|seat| (seat.id, seat.profile))
        .collect();
    let camera = commands.spawn(Camera2d).id();
    let mut intro = Intro {
        phase: Phase::Movie,
        session: std::sync::Mutex::new(Some(session)),
        vfs,
        mfx,
        camera,
        since: None,
        owners,
        owner_index: 0,
    };
    if let Err(e) = crate::movie::start(commands, images, &movie_path) {
        warn!("intro: no movie ({e})");
        // A decode failure is explicit; do not acknowledge an unplayed event.
        error!("intro: blocked at movie event: {e}");
        intro.phase = Phase::Blocked;
    }
    commands.insert_resource(intro);
}

fn begin_edit(commands: &mut Commands, intro: &mut Intro) {
    commands.entity(intro.camera).despawn();
    intro.camera = commands
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 1.2, 3.2).looking_at(Vec3::new(0.0, 0.95, 0.0), Vec3::Y),
        ))
        .id();
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        IntroLight,
    ));
    if intro.owners.is_empty() {
        error!("intro: stage801 has no local profile to edit");
        intro.phase = Phase::Blocked;
        return;
    }
    if let Err(e) = setup_editor(commands, intro) {
        error!("intro: character edit: {e}");
        intro.phase = Phase::Blocked;
    }
}

fn setup_editor(commands: &mut Commands, intro: &mut Intro) -> Result<(), String> {
    let (seat, profile) = intro.owners[intro.owner_index];
    let session = intro.session.get_mut().unwrap().as_ref().unwrap();
    if session.host.roster.seat(seat).map(|s| s.profile) != Some(profile) {
        return Err("character editor's profile is no longer seated".into());
    }
    let saved = session.host.character_edit(seat).cloned();
    crate::charaedit::setup_record(commands, intro.vfs, intro.mfx, saved)
}

#[derive(Component)]
pub struct IntroLight;

/// Moves from the movie to the editor, and from the confirmed edit into the
/// opening world.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    intro: Option<ResMut<Intro>>,
    movie: Res<MovieFinished>,
    mut skip: ResMut<crate::movie::MovieSkip>,
    time: Res<Time<Real>>,
    mut result: Option<ResMut<EditResult>>,
    edit: Option<ResMut<CharaEdit>>,
    streamer: Option<ResMut<Streamer>>,
    lights: Query<Entity, With<IntroLight>>,
    args: Res<crate::Args>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(mut intro) = intro else { return };
    let now = time.elapsed_secs_f64();
    let waited = now - *intro.since.get_or_insert(now);
    match intro.phase {
        Phase::Blocked => {}
        Phase::Movie => {
            if args.unattended && waited > 3.0 {
                skip.0 = true;
            }
            if movie.0 {
                let advanced = intro
                    .session
                    .get_mut()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .intro_movie_done();
                if let Err(e) = advanced {
                    error!("intro: movie completion: {e}");
                    intro.phase = Phase::Blocked;
                    return;
                }
                intro.phase = Phase::Edit;
                intro.since = Some(now);
                begin_edit(&mut commands, &mut intro);
            }
        }
        Phase::Edit => {
            let (Some(mut edit), Some(mut streamer)) = (edit, streamer) else {
                return;
            };
            if args.unattended && waited > 3.0 {
                if let Some(r) = result.as_mut() {
                    let (seat, profile) = intro.owners[intro.owner_index];
                    if let Ok(name) = std::env::var(format!("DDVIEW_CHARACTER_NAME_{seat}"))
                        .or_else(|_| std::env::var("DDVIEW_CHARACTER_NAME"))
                    {
                        if r.0.is_none() {
                            match edit.automate(&mut commands, &name, now, seat, profile) {
                                Ok(record) => r.0 = record,
                                Err(e) => {
                                    error!("intro: unattended character creation: {e}");
                                    intro.phase = Phase::Blocked;
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            let Some(record) = result.as_ref().and_then(|r| r.0.clone()) else {
                return;
            };
            let fields: Vec<_> = ddpawn::cmc::EDIT_U8_FIELDS
                .iter()
                .zip(record.fields)
                .map(|(name, value)| ((*name).to_owned(), u32::from(value)))
                .collect();
            if let Err(e) = ddlook::save::edit_look_dressed(intro.vfs, &fields) {
                error!("intro: the edited Arisen cannot produce its installed look: {e:?}");
                intro.phase = Phase::Blocked;
                return;
            }
            let (seat, profile) = intro.owners[intro.owner_index];
            let session = intro.session.get_mut().unwrap().as_mut().unwrap();
            if session.host.roster.seat(seat).map(|s| s.profile) != Some(profile) {
                error!("intro: confirmed profile is no longer seated");
                intro.phase = Phase::Blocked;
                return;
            }
            let sex = record.get("mGender").unwrap();
            let name = record.name_str();
            if let Err(e) = session.host.confirm_character(seat, record) {
                error!("intro: character confirmation: {e}");
                intro.phase = Phase::Blocked;
                return;
            }
            info!("intro: stage801 confirmed seat {seat} profile {profile} sex {sex} name {name:?}");
            edit.close(&mut commands);
            if intro.owner_index + 1 < intro.owners.len() {
                intro.owner_index += 1;
                intro.since = Some(now);
                if let Some(r) = result.as_mut() {
                    r.0 = None;
                }
                if let Err(e) = setup_editor(&mut commands, &mut intro) {
                    error!("intro: next profile editor: {e}");
                    intro.phase = Phase::Blocked;
                }
                return;
            }
            let Some(mut session) = intro.session.get_mut().unwrap().take() else {
                return;
            };
            if let Err(e) = session.intro_edit_done() {
                error!("intro: editor completion: {e}");
                *intro.session.get_mut().unwrap() = Some(session);
                intro.phase = Phase::Blocked;
                return;
            }
            // Explicit project capture checkpoint, after every local profile
            // confirmed and the story admitted the opening. Never a native save.
            if args.unattended {
                let checkpoint = (|| -> Result<(), String> {
                    let Some(dir) = crate::charaedit::capture_dir()? else {
                        return Ok(());
                    };
                    let file = session.host.save().map_err(|e| e.to_string())?;
                    let path = dir.join("S56-confirmed.ddhs");
                    std::fs::write(&path, file.to_bytes()).map_err(|e| e.to_string())?;
                    info!(
                        "intro: stage801 all {} local profiles confirmed; checkpoint {} hash {:016x}",
                        intro.owners.len(),
                        path.display(),
                        session.host.saved_hash()
                    );
                    Ok(())
                })();
                if let Err(e) = checkpoint {
                    error!("intro: character checkpoint: {e}");
                    *intro.session.get_mut().unwrap() = Some(session);
                    intro.phase = Phase::Blocked;
                    return;
                }
            }
            commands.remove_resource::<CharaEdit>();
            commands.remove_resource::<EditResult>();
            commands.entity(intro.camera).despawn();
            for l in &lights {
                commands.entity(l).despawn();
            }
            info!("intro: done; Cassardis with the edited Arisen");
            crate::opening_world(
                &mut commands,
                &mut streamer.loader,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut bindposes,
                session,
                None, // opening_world restores every profile's confirmed look.
                &args,
                intro.vfs,
            );
            commands.remove_resource::<Intro>();
        }
    }
}
