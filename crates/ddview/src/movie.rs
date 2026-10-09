//! Full-screen movies (`movie\event\*.wmv`, `movie\title\*.wmv`) from the
//! install, decoded at run time by `mtmovie` (Media Foundation).
//!
//! [`start`] opens a movie: one worker thread decodes video frames ahead
//! (a few frames buffered), another decodes the whole soundtrack and folds
//! it to stereo. Playback starts when the sound is ready (or failed), and
//! [`update`] shows the frame due at the elapsed time. When the movie ends
//! or is skipped, [`MovieFinished`] is set for the caller (the opening's
//! q0000 waits on its event).
//!
//! *Viewer choices:* Enter or Esc skips (the original's skip input is not
//! traced). The picture keeps its 16:9 aspect, letterboxed on black.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::audio::DecodedAudio;

/// Decoded frames buffered ahead of the picture.
const AHEAD: usize = 6;

/// The movie file for a name such as `st100ev00` (event movies) or
/// `title_A` (title movies).
pub fn path(native_pc: &Path, name: &str) -> PathBuf {
    let dir = if name.starts_with("title") { "title" } else { "event" };
    native_pc.join("movie").join(dir).join(format!("{name}.wmv"))
}

/// Set by a caller to skip the playing movie (as Enter / Esc do).
#[derive(Resource, Default)]
pub struct MovieSkip(pub bool);

/// Set when the last started movie ended or was skipped.
#[derive(Resource, Default)]
pub struct MovieFinished(pub bool);

#[derive(Resource)]
pub struct MoviePlayer {
    name: String,
    // Mutex: a Receiver is Send but not Sync, as a resource must be
    frames: Mutex<Receiver<mtmovie::Frame>>,
    sound: Option<Mutex<Receiver<Result<(Vec<f32>, u32), String>>>>,
    image: Handle<Image>,
    root: Entity,
    /// Seconds of app time when the movie's clock started.
    start: Option<f64>,
    next: Option<mtmovie::Frame>,
    voice: Option<Entity>,
    ended: bool,
}

#[derive(Component)]
struct MovieRoot;

/// Starts `path` full screen. Fails if the file does not open as a movie.
pub fn start(commands: &mut Commands, images: &mut Assets<Image>, path: &Path) -> Result<(), String> {
    let video = mtmovie::VideoReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let info = video.info;
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    info!("movie {name}: {}x{} at {:.2} fps, {:.1} s", info.width, info.height, info.fps, info.duration);
    // Media Foundation objects stay on the thread that made them: the
    // worker reopens the file.
    let (tx, frames) = sync_channel(AHEAD);
    let vpath = path.to_path_buf();
    drop(video);
    std::thread::spawn(move || {
        let Ok(mut v) = mtmovie::VideoReader::open(&vpath) else { return };
        while let Ok(Some(f)) = v.next() {
            if tx.send(f).is_err() {
                return; // skipped
            }
        }
    });
    let (stx, sound) = sync_channel(1);
    let apath = path.to_path_buf();
    std::thread::spawn(move || {
        let r = (|| {
            let mut a = mtmovie::AudioReader::open(&apath).map_err(|e| e.to_string())?;
            let pcm = a.read_all().map_err(|e| e.to_string())?;
            Ok((stereo(&pcm, a.format.channels), a.format.sample_rate))
        })();
        let _ = stx.send(r);
    });
    let image = images.add(Image::new_fill(
        Extent3d { width: info.width, height: info.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));
    let root = commands
        .spawn((
            MovieRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK),
            GlobalZIndex(100),
        ))
        .with_children(|p| {
            p.spawn((
                ImageNode::new(image.clone()),
                Node {
                    width: Val::Percent(100.0),
                    max_height: Val::Percent(100.0),
                    aspect_ratio: Some(info.width as f32 / info.height as f32),
                    ..default()
                },
            ));
        })
        .id();
    commands.insert_resource(MovieFinished(false));
    commands.insert_resource(MoviePlayer {
        name,
        frames: Mutex::new(frames),
        sound: Some(Mutex::new(sound)),
        image,
        root,
        start: None,
        next: None,
        voice: None,
        ended: false,
    });
    Ok(())
}

/// Folds the soundtrack to stereo. Media Foundation's 5.1 order is FL, FR,
/// FC, LFE, BL, BR (the WAVEFORMATEXTENSIBLE speaker order); centre and
/// back channels go in at -3 dB, LFE is left out (*viewer choice*, not the
/// original's mixer).
fn stereo(pcm: &[f32], channels: u16) -> Vec<f32> {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    match channels {
        1 | 2 => pcm.to_vec(),
        6 => pcm.chunks_exact(6).flat_map(|s| [s[0] + k * (s[2] + s[4]), s[1] + k * (s[2] + s[5])]).collect(),
        n => pcm.chunks_exact(n as usize).flat_map(|s| [s[0], s[1]]).collect(),
    }
}

/// Shows the frame due now, starts the sound, and ends or skips the movie.
pub fn update(
    mut commands: Commands,
    player: Option<ResMut<MoviePlayer>>,
    mut finished: ResMut<MovieFinished>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut skip_request: ResMut<MovieSkip>,
    mut images: ResMut<Assets<Image>>,
    mut sounds: ResMut<Assets<DecodedAudio>>,
) {
    let Some(mut m) = player else { return };
    let now = time.elapsed_secs_f64();
    let skip = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) || std::mem::take(&mut skip_request.0);
    if m.start.is_none() {
        // wait for the soundtrack so picture and sound start together
        let ready = match m.sound.as_mut().map(|r| r.get_mut().unwrap().try_recv()) {
            Some(Ok(Ok((pcm, rate)))) => {
                match crate::audio::play_pcm(&mut commands, &mut sounds, pcm, 2, rate) {
                    Ok(e) => m.voice = Some(e),
                    Err(e) => warn!("movie {}: sound: {e}", m.name),
                }
                true
            }
            Some(Ok(Err(e))) => {
                warn!("movie {}: no sound: {e}", m.name);
                true
            }
            Some(Err(TryRecvError::Disconnected)) | None => true,
            Some(Err(TryRecvError::Empty)) => false,
        };
        if !ready && !skip {
            return;
        }
        m.sound = None;
        m.start = Some(now);
    }
    let elapsed = now - m.start.unwrap_or(now);
    // the newest frame due now (late frames are dropped)
    let mut due = None;
    loop {
        if m.next.is_none() {
            match m.frames.get_mut().unwrap().try_recv() {
                Ok(f) => m.next = Some(f),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    m.ended = true;
                    break;
                }
            }
        }
        if m.next.as_ref().is_some_and(|f| f.time <= elapsed) {
            due = m.next.take();
        } else {
            break;
        }
    }
    if let Some(f) = due {
        if let Some(mut img) = images.get_mut(&m.image) {
            img.data = Some(f.bgra);
        }
    }
    if skip || (m.ended && m.next.is_none()) {
        info!("movie {}: {} at {elapsed:.1} s", m.name, if skip { "skipped" } else { "ended" });
        commands.entity(m.root).despawn();
        if let Some(v) = m.voice {
            commands.entity(v).despawn();
        }
        commands.remove_resource::<MoviePlayer>();
        finished.0 = true;
    }
}
