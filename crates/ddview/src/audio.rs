//! Installed audio presentation. Decoding is fallible before Bevy sees a source.
//! Mixer fidelity (gain curves, reverb and spatial sends) is not yet reproduced.
use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::Arc, time::Duration};
use bevy::{audio::{AddAudioSource, ChannelCount, SampleRate, Source}, prelude::*};
use ddstory::event::Cue;

pub struct GameAudioPlugin(pub PathBuf);
impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        let vfs = ddvfs::Vfs::open(&self.0).map_err(|e| warn!("audio disabled: {e}")).ok();
        app.add_audio_source::<DecodedAudio>()
            .insert_resource(AudioState { vfs, pending: Vec::new(), cache: HashMap::new(), failed: HashSet::new() })
            .add_systems(PostUpdate, (play_pending, sync_pause).chain());
    }
}

#[derive(Resource)]
struct AudioState {
    vfs: Option<ddvfs::Vfs>,
    pending: Vec<Cue>,
    cache: HashMap<String, Handle<DecodedAudio>>,
    failed: HashSet<String>,
}

/// Deferred so event systems need no knowledge of the mixer or device.
pub fn cues(commands: &mut Commands, frame: &ddstory::event::EventFrame) {
    if frame.cues.is_empty() { return; }
    let cues = frame.cues.clone();
    commands.queue(move |world: &mut World| {
        if let Some(mut audio) = world.get_resource_mut::<AudioState>() {
            audio.pending.extend(cues);
        }
    });
}

#[derive(Asset, TypePath, Clone)]
struct DecodedAudio {
    samples: Arc<[f32]>,
    channels: ChannelCount,
    rate: SampleRate,
}
struct Samples { audio: DecodedAudio, at: usize }
impl Iterator for Samples {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let sample = self.audio.samples.get(self.at).copied()?;
        self.at += 1; Some(sample)
    }
}
impl Source for Samples {
    fn current_span_len(&self) -> Option<usize> { Some(self.audio.samples.len().saturating_sub(self.at)) }
    fn channels(&self) -> ChannelCount { self.audio.channels }
    fn sample_rate(&self) -> SampleRate { self.audio.rate }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.audio.samples.len() as f64 / self.audio.channels.get() as f64 / self.audio.rate.get() as f64))
    }
}
impl Decodable for DecodedAudio {
    type Decoder = Samples;
    fn decoder(&self) -> Samples { Samples { audio: self.clone(), at: 0 } }
}

/// Provisional presentation downmix, not a translation of DDDA's mixer.
/// Vorbis order is FL,C,FR,RL,RR,LFE (Xiph Vorbis I section 4.3.9).
/// Use the standard equal-power center/surround stereo fold, omit LFE.
/// This avoids Rodio's channel converter simply discarding the dialogue.
fn stereo(mut pcm: mtsound::decode::Pcm) -> Result<DecodedAudio, String> {
    if pcm.channels == 6 {
        pcm.samples = pcm.samples.chunks_exact(6).flat_map(|s| {
            let k = std::f32::consts::FRAC_1_SQRT_2;
            [s[0] + k * (s[1] + s[3]), s[2] + k * (s[1] + s[4])]
        }).collect();
        pcm.channels = 2;
    }
    if !matches!(pcm.channels, 1 | 2) { return Err("unsupported channel layout".into()); }
    Ok(DecodedAudio { samples: pcm.samples.into(), channels: ChannelCount::new(pcm.channels).ok_or("zero channels")?, rate: SampleRate::new(pcm.sample_rate).ok_or("zero sample rate")? })
}

#[derive(Component)]
struct EventSound(String);

fn play_pending(mut commands: Commands, mut audio: ResMut<AudioState>, mut assets: ResMut<Assets<DecodedAudio>>, playing: Query<(Entity, &EventSound)>) {
    // Commands are deferred: include sources queued earlier in this batch so
    // two cues for one unit cannot leave two voices playing simultaneously.
    let mut units: HashMap<String, Vec<Entity>> = HashMap::new();
    for (entity, sound) in &playing { units.entry(sound.0.clone()).or_default().push(entity); }
    for cue in std::mem::take(&mut audio.pending) {
        let Cue::Stream { unit, path, stream_id, prepare_id, .. } = cue else {
            // Conditional Arisen voice selection and SREQ programs still being traced.
            continue;
        };
        let key = format!("{path}:{stream_id}:{prepare_id}");
        if audio.failed.contains(&key) { continue; }
        let handle = if let Some(h) = audio.cache.get(&key) { h.clone() } else {
            let decoded = (|| {
                let v = audio.vfs.as_ref().ok_or("audio VFS unavailable")?;
                let bytes = v.read(&path, mtarc::type_hash("rSoundStreamRequest"))?;
                let requests = mtsound::StreamRequests::parse(&bytes)?;
                let source = requests.event_source(u16::try_from(stream_id).map_err(|_| "invalid stream ID")?, u16::try_from(prepare_id).map_err(|_| "invalid prepare ID")?)?;
                let source_path = mtsound::stream_path(source.path)?;
                let bytes = std::fs::read(v.root.join(&source_path)).map_err(|e| format!("{source_path}: {e}"))?;
                let pcm = mtsound::decode::vorbis(&bytes, 40_000_000)?;
                info!("audio resolved {key} -> {source_path} ({} samples)", pcm.samples.len());
                stereo(pcm)
            })();
            match decoded {
                Ok(pcm) => { let h = assets.add(pcm); audio.cache.insert(key.clone(), h.clone()); h }
                Err(e) => { warn!("audio {key}: {e}"); audio.failed.insert(key); continue; }
            }
        };
        for entity in units.remove(&unit).unwrap_or_default() { commands.entity(entity).despawn(); }
        let entity = commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN, EventSound(unit.clone()))).id();
        units.insert(unit, vec![entity]);
    }
}

fn sync_pause(pause: Option<Res<crate::pause::PauseState>>, args: Option<Res<crate::Args>>, sinks: Query<&AudioSink, With<EventSound>>) {
    let paused = pause.is_some_and(|p| p.paused) && args.is_some_and(|a| a.host.is_none() && a.join.is_none());
    for sink in &sinks { if paused { sink.pause(); } else { sink.play(); } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dialogue_center_is_present_in_both_outputs() {
        let sound = stereo(mtsound::decode::Pcm { channels: 6, sample_rate: 48000, samples: vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0] }).unwrap();
        assert_eq!(&*sound.samples, &[std::f32::consts::FRAC_1_SQRT_2; 2]);
        assert_eq!(sound.decoder().count(), 2);
    }
}
