//! What `--opening` shows before Cassardis (OPENING_SLICE steps 3-4):
//! q0000's movie (`movie\event\st100ev00.wmv`), then the character editor
//! (stage 801's role), then the opening world with the edited Arisen.
//!
//! The quest state does not wait on this: the session was already walked
//! to q0001 before the window opened (`opening::prepare`, the fast-forward
//! stand-in for the prologue and the dragon attack, still in place until
//! those are playable). Only the order of what is shown follows the
//! original: the movie, then the edit, then Cassardis.
//!
//! With `--unattended` it advances by itself (the movie skips after 3 s,
//! the edit confirms after 3 s) so automated runs reach Cassardis.
//!
//! Skipped by `--skip-intro`, `--save` (the save has its Arisen), `--demo`
//! and network sessions.

use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;

use crate::charaedit::{CharaEdit, EditResult};
use crate::movie::MovieFinished;
use crate::stream::Streamer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Movie,
    Edit,
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
}

/// Starts the intro: the movie first (or the editor if the movie fails).
pub fn start(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    native_pc: &std::path::Path,
    session: crate::opening::Session,
    vfs: &'static ddvfs::Vfs,
    mfx: &'static mtshader::ShaderPackage,
) {
    let camera = commands.spawn(Camera2d).id();
    let mut intro = Intro { phase: Phase::Movie, session: std::sync::Mutex::new(Some(session)), vfs, mfx, camera, since: None };
    if let Err(e) = crate::movie::start(commands, images, &crate::movie::path(native_pc, "st100ev00")) {
        warn!("intro: no movie ({e})");
        intro.phase = Phase::Edit;
        begin_edit(commands, &mut intro);
    }
    commands.insert_resource(intro);
}

fn begin_edit(commands: &mut Commands, intro: &mut Intro) {
    commands.entity(intro.camera).despawn();
    intro.camera = commands
        .spawn((Camera3d::default(), Transform::from_xyz(0.0, 1.2, 3.2).looking_at(Vec3::new(0.0, 0.95, 0.0), Vec3::Y)))
        .id();
    commands.spawn((
        DirectionalLight { illuminance: 8000.0, ..default() },
        Transform::from_xyz(1.0, 3.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        IntroLight,
    ));
    if let Err(e) = crate::charaedit::setup(commands, intro.vfs, intro.mfx, 0) {
        error!("intro: character edit: {e}");
    }
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
        Phase::Movie => {
            if args.unattended && waited > 3.0 {
                skip.0 = true;
            }
            if movie.0 {
                intro.phase = Phase::Edit;
                intro.since = Some(now);
                begin_edit(&mut commands, &mut intro);
            }
        }
        Phase::Edit => {
            let (Some(mut edit), Some(mut streamer)) = (edit, streamer) else { return };
            if args.unattended && waited > 3.0 {
                if let Some(r) = result.as_mut() {
                    r.0.get_or_insert_with(|| edit.record());
                }
            }
            let Some(fields) = result.as_ref().and_then(|r| r.0.clone()) else { return };
            edit.close(&mut commands);
            commands.remove_resource::<CharaEdit>();
            commands.remove_resource::<EditResult>();
            commands.entity(intro.camera).despawn();
            for l in &lights {
                commands.entity(l).despawn();
            }
            let arisen = match ddlook::save::edit_look_dressed(intro.vfs, &fields) {
                Ok(look) => Some(look),
                Err(e) => {
                    error!("intro: the edited Arisen: {e:?}");
                    ddlook::save::template_look(intro.vfs, 0).ok()
                }
            };
            let Some(session) = intro.session.get_mut().unwrap().take() else { return };
            info!("intro: done; Cassardis with the edited Arisen");
            crate::opening_world(
                &mut commands,
                &mut streamer.loader,
                &mut meshes,
                &mut materials,
                &mut images,
                &mut bindposes,
                session,
                arisen,
                &args,
                intro.vfs,
            );
            commands.remove_resource::<Intro>();
        }
    }
}
