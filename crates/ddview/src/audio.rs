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

/// Event owners supply their selected player/edit/pawn records. The default
/// context suppresses conditional cues when those are unknown. Queuing is
/// deferred so event systems need no knowledge of the mixer or device.
pub fn cues_with_voices(commands: &mut Commands, frame: &ddstory::event::EventFrame,
    voices: mtsound::voice::EventVoices) {
    if frame.cues.is_empty() { return; }
    let cues = selected_cues(&frame.cues, voices);
    commands.queue(move |world: &mut World| {
        if let Some(mut audio) = world.get_resource_mut::<AudioState>() {
            audio.pending.extend(cues);
        }
    });
}

fn selected_cues(cues: &[Cue], voices: mtsound::voice::EventVoices) -> Vec<Cue> {
    cues.iter().filter_map(|cue| {
        if let Cue::Se { unit, path, se_id, conditional } = cue {
            if !voices.allows(*conditional) { return None; }
            // The explicit context has already applied the conditional gate.
            return Some(Cue::Se { unit: unit.clone(), path: path.clone(),
                se_id: *se_id, conditional: None });
        }
        Some(cue.clone())
    }).collect()
}

/// Voice context for the save slot used to dress the event's player and pawn.
/// The separate original edit-record condition (4..7) is not inferred from
/// these records, so it remains absent.
pub fn voices_from_save_xml(xml: &str, slot: &str) -> Result<mtsound::voice::EventVoices, String> {
    let doc = ddsave::xml::Document::parse(xml).map_err(|e| e.to_string())?;
    let party = doc.root().child(slot).and_then(|p| p.child("mPlCmcEditAndParam"))
        .ok_or("save slot has no character records")?;
    fn variant(record: ddsave::xml::Node<'_, '_>) -> Option<mtsound::voice::Variant> {
        let edit = record.child("mEdit")?;
        mtsound::voice::Variant::from_edit(edit.get("mGender")?, edit.get("mVoice")?)
    }
    Ok(mtsound::voice::EventVoices {
        player: party.child("mPl").and_then(variant),
        pawn: party.child("mCmc").and_then(|p| p.children().next()).and_then(variant),
        edit: None,
    })
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
#[derive(Component)]
struct OneShotSe;

fn play_pending(mut commands: Commands, mut audio: ResMut<AudioState>, mut assets: ResMut<Assets<DecodedAudio>>, playing: Query<(Entity, &EventSound), Without<OneShotSe>>) {
    // Commands are deferred: include sources queued earlier in this batch so
    // two cues for one unit cannot leave two voices playing simultaneously.
    let mut units: HashMap<String, Vec<Entity>> = HashMap::new();
    for (entity, sound) in &playing { units.entry(sound.0.clone()).or_default().push(entity); }
    for cue in std::mem::take(&mut audio.pending) {
        let (unit, key) = match &cue {
            Cue::Stream { unit, path, stream_id, prepare_id, .. } =>
                (unit, format!("{path}:{stream_id}:{prepare_id}")),
            Cue::Se { unit, path, se_id, conditional: None } =>
                (unit, format!("se:{path}:{se_id}")),
            // Selecting every authored variant would layer multiple voices.
            Cue::Se { conditional: Some(_), .. } => continue,
        };
        if audio.failed.contains(&key) { continue; }
        let handle = if let Some(h) = audio.cache.get(&key) { h.clone() } else {
            let decoded = (|| {
                let v = audio.vfs.as_ref().ok_or("audio VFS unavailable")?;
                let pcm = match &cue {
                Cue::Stream { path, stream_id, prepare_id, .. } => {
                let bytes = v.read(path, mtarc::type_hash("rSoundStreamRequest"))?;
                let requests = mtsound::StreamRequests::parse(&bytes)?;
                let source = requests.event_source(u16::try_from(*stream_id).map_err(|_| "invalid stream ID")?, u16::try_from(*prepare_id).map_err(|_| "invalid prepare ID")?)?;
                let source_path = mtsound::stream_path(source.path)?;
                let bytes = std::fs::read(v.root.join(&source_path)).map_err(|e| format!("{source_path}: {e}"))?;
                let pcm = mtsound::decode::vorbis(&bytes, 40_000_000)?;
                info!("audio resolved {key} -> {source_path} ({} samples)", pcm.samples.len());
                pcm
                }
                Cue::Se { path, se_id, .. } => {
                    let bytes = v.read(path, mtarc::type_hash("rSoundRequest"))?;
                    let requests = mtsound::Requests::parse(&bytes)?;
                    let request = requests.single_wave(u16::try_from(*se_id).map_err(|_| "invalid SE ID")?)?;
                    let source_path = request.source.ok_or("sound request has no package")?;
                    let package_bytes = v.read(source_path, mtarc::type_hash("rSoundPackage"))?;
                    let package = mtsound::Package::parse(&package_bytes)?;
                    // Default program/variation only: runtime overrides, gain,
                    // pitch, delay and spatial sends remain unimplemented.
                    let (program, variation) = request.wave_keys();
                    let wave = package.wave_for(program, variation)?.ok_or("sound wave absent")?;
                    let pcm = mtsound::decode::wave(&wave.riff(), 4_000_000)?;
                    info!("audio resolved {key} -> {source_path} ({} samples)", pcm.samples.len());
                    pcm
                }
                };
                stereo(pcm)
            })();
            match decoded {
                Ok(pcm) => { let h = assets.add(pcm); audio.cache.insert(key.clone(), h.clone()); h }
                Err(e) => { warn!("audio {key}: {e}"); audio.failed.insert(key); continue; }
            }
        };
        if matches!(cue, Cue::Se { .. }) {
            // SEs may overlap. Original priority/group limits still need a
            // mixer; stream-unit replacement is not applicable to them.
            commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN,
                EventSound(unit.clone()), OneShotSe));
            continue;
        }
        for entity in units.remove(unit).unwrap_or_default() { commands.entity(entity).despawn(); }
        let entity = commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN, EventSound(unit.clone()))).id();
        units.insert(unit.clone(), vec![entity]);
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
    fn save_voice_context_keeps_player_and_first_pawn_separate() {
        let xml = r#"<class><class name="manual"><class name="mPlCmcEditAndParam">
            <class name="mPl"><class name="mEdit"><u8 name="mGender" value="0"/>
                <u8 name="mVoice" value="3"/></class></class>
            <array name="mCmc"><class><class name="mEdit"><u8 name="mGender" value="1"/>
                <u8 name="mVoice" value="0"/></class></class>
                <class><class name="mEdit"><u8 name="mGender" value="0"/>
                <u8 name="mVoice" value="6"/></class></class></array>
            </class></class></class>"#;
        let context = voices_from_save_xml(xml, "manual").unwrap();
        let selected: Vec<_> = (0..16).filter(|&n| context.allows(Some(n))).collect();
        assert_eq!(selected, [3, 8, 12]);
        assert!(context.edit.is_none());
        assert!(voices_from_save_xml(xml, "absent").is_err());
    }
    #[test]
    fn event_actor_context_filters_alternative_voice_cues() {
        use mtsound::voice::{EventVoices, Variant};
        let cues: Vec<_> = (0..4).map(|id| Cue::Se {
            unit: format!("voice{id}"), path: format!("voice{id}"),
            se_id: 1, conditional: Some(id),
        }).collect();
        assert!(selected_cues(&cues, EventVoices::default()).is_empty());
        let selected = selected_cues(&cues, EventVoices {
            player: Some(Variant { model_or_gender: 0, voice: 3 }),
            ..Default::default()
        });
        assert_eq!(selected, vec![Cue::Se { unit: "voice3".into(), path: "voice3".into(),
            se_id: 1, conditional: None }]);
    }
    #[test]
    fn dialogue_center_is_present_in_both_outputs() {
        let sound = stereo(mtsound::decode::Pcm { channels: 6, sample_rate: 48000, samples: vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0] }).unwrap();
        assert_eq!(&*sound.samples, &[std::f32::consts::FRAC_1_SQRT_2; 2]);
        assert_eq!(sound.decoder().count(), 2);
    }

    #[test]
    fn unconditional_effects_overlap_and_conditional_variants_are_not_guessed() {
        let mut assets = Assets::<DecodedAudio>::default();
        let handle = assets.add(stereo(mtsound::decode::Pcm {
            channels: 1, sample_rate: 48000, samples: vec![0.0],
        }).unwrap());
        let effect = |conditional| Cue::Se {
            unit: "effect".into(), path: "cached".into(), se_id: 1, conditional,
        };
        let mut app = App::new();
        app.insert_resource(assets).insert_resource(AudioState {
            vfs: None, pending: vec![effect(None), effect(Some(0)), effect(None)],
            cache: HashMap::from([("se:cached:1".into(), handle)]),
            failed: HashSet::new(),
        }).add_systems(Update, play_pending);
        app.update();
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<OneShotSe>>();
        assert_eq!(query.iter(world).count(), 2);
        assert!(world.resource::<AudioState>().failed.is_empty());
    }

    #[test]
    fn repeated_unit_cues_replace_even_before_commands_are_applied() {
        let mut assets = Assets::<DecodedAudio>::default();
        let handle = assets.add(stereo(mtsound::decode::Pcm {
            channels: 1, sample_rate: 48000, samples: vec![0.0],
        }).unwrap());
        let cue = |unit: &str| Cue::Stream {
            unit: unit.into(), path: "cached".into(), stream_id: 1,
            mode: 2, prepare_id: 0,
        };
        let mut app = App::new();
        app.insert_resource(assets).insert_resource(AudioState {
            vfs: None,
            pending: vec![cue("voice"), cue("voice"), cue("music")],
            cache: HashMap::from([("cached:1:0".into(), handle)]),
            failed: HashSet::new(),
        }).add_systems(Update, play_pending);
        app.world_mut().spawn(EventSound("voice".into()));
        app.update();
        let world = app.world_mut();
        let mut query = world.query::<&EventSound>();
        let mut units: Vec<_> = query.iter(world).map(|s| s.0.as_str()).collect();
        units.sort();
        assert_eq!(units, ["music", "voice"]);
    }
}
