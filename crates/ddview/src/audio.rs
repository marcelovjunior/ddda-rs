//! Installed audio presentation. Decoding is fallible before Bevy sees a source.
//! Mixer fidelity (gain curves, reverb and spatial sends) is not yet reproduced.
use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::Arc, time::Duration};
use bevy::{audio::{AddAudioSource, ChannelCount, SampleRate, Source}, prelude::*};
use ddstory::event::Cue;

pub struct GameAudioPlugin(pub PathBuf);
impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        let vfs = ddvfs::Vfs::open(&self.0).map_err(|e| warn!("audio disabled: {e}")).ok();
        if let Some(vfs) = &vfs {
            match PlayerMotionSounds::load(vfs) {
                Ok(sounds) => { app.insert_resource(sounds); }
                Err(e) => warn!("motion audio disabled: {e}"),
            }
        }
        app.add_audio_source::<DecodedAudio>()
            .insert_resource(AudioState { vfs, pending: Vec::new(), cache: HashMap::new(), failed: HashSet::new() })
            .add_systems(PostUpdate, (queue_motion_sounds, play_pending, sync_pause).chain());
    }
}

#[derive(Resource)]
struct AudioState {
    vfs: Option<ddvfs::Vfs>,
    pending: Vec<Cue>,
    cache: HashMap<String, Handle<DecodedAudio>>,
    failed: HashSet<String>,
}

/// Actor-local sampling inputs from a fixed simulation tick. Presentation
/// interpolation must not call this: it could repeat or skip event edges.
pub struct ActorSoundFrame<'a> {
    pub owner: u32,
    pub layer: u8,
    pub body: &'a ddhost::ddenemyrt::body::Body,
    pub motion: ddhost::ddenemyrt::body::MotionId,
    pub frame: f32,
    pub position: [f32; 3],
    pub yaw: f32,
    /// Known uSoundMotionSe selector. Unknown accepts only player requests
    /// invariant between selectors 0 and 2 (0x00c736f0).
    pub selector: Option<u32>,
    pub primary_model: Option<u32>,
    pub secondary_model: Option<u32>,
}

/// The two independent results used by the surface sound lookup.
pub struct SoundSurface {
    pub material_word: u32,
    pub triangle_attribute: u32,
}

/// Loaded player sound resources and rising bits per actor AND motion layer.
/// The fixed-step owner calls sample; resolved requests can use the same
/// ordinary SREQ playback path as event effects.
#[derive(Resource)]
pub struct PlayerMotionSounds {
    requests: mtsound::motion::MotionRequests,
    surfaces: mtsound::motion::SurfaceRequests,
    attribute_banks: Vec<String>,
    previous: HashMap<(u32, u8), u32>,
    kit_models: HashMap<ddsim::Weapon, u32>,
    pending: Vec<Cue>,
}

impl PlayerMotionSounds {
    pub fn load(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        let requests = mtsound::motion::MotionRequests::parse(&vfs.read(
            r"sound\se\pl_com\pl_com_bmse", mtarc::type_hash("rSoundMotionSe"))?)?;
        let surfaces = mtsound::motion::SurfaceRequests::parse(&vfs.read(
            r"sound\se\mt\mt_attri", mtarc::type_hash("rSoundAttributeSe"))?)?;
        let file = ddexe::ExeFile::next_to(&vfs.root)?;
        let image = file.image()?;
        // Translated from player sound binding 0x00b5f880
        // (DDDA.exe 2017-12-13 Steam). Read bulk paths from the install.
        let attribute_banks = (0..30).map(|i| {
            image.cstr(image.u32(0x0187_5288 + 4 * i)?)
        }).collect::<Result<Vec<_>, ddexe::Error>>()?;
        let items = dditem::itl::ItemList::parse(&vfs.read(r"etc\item\itemList",
            mtarc::type_hash("rItemList"))?).map_err(|e| e.to_string())?;
        // Match the current fight's explicitly limited Fighter/Mage kits.
        // Saved equipment beyond those kits is not yet a combat input.
        use ddhost::ddenemyrt::arisen::{IRON_SWORD, MAGE_STAFF};
        let mut kit_models = HashMap::new();
        for (weapon, item) in [(ddsim::Weapon::Sword, IRON_SWORD), (ddsim::Weapon::Staff, MAGE_STAFF)] {
            kit_models.insert(weapon, items.get(item).ok_or("audio combat-kit item absent")?.equip_model());
        }
        Ok(Self { requests, surfaces, attribute_banks, previous: HashMap::new(),
            kit_models, pending: Vec::new() })
    }

    /// Only the request banks with traced bindings are available here.
    fn bank(&self, slot: mtsound::motion::BankSlot, actor: &ActorSoundFrame<'_>) -> Option<String> {
        use mtsound::motion::{BankSlot, weapon_banks};
        Some(match slot {
            BankSlot::Common => r"sound\se\pl_com\pl_com".into(),
            BankSlot::Bound(2) => weapon_banks(actor.primary_model?, false)?.common,
            BankSlot::Bound(3) => weapon_banks(actor.primary_model?, false)?.specific,
            BankSlot::Bound(4) => weapon_banks(actor.secondary_model?, true)?.common,
            BankSlot::Bound(5) => weapon_banks(actor.secondary_model?, true)?.specific,
            BankSlot::Attribute(index) => self.attribute_banks.get(index as usize)?.clone(),
            // Clothing, voice, other bound slots and motion streams unconnected.
            _ => return None,
        })
    }

    pub fn sample(&mut self, actor: ActorSoundFrame<'_>,
        mut surface: impl FnMut([f32; 3], [f32; 3]) -> Option<SoundSurface>) -> Vec<Cue> {
        use mtsound::motion::{attribute_bank, direct_bank, rising_ids, surface_index};
        if actor.layer >= 8 { return Vec::new(); }
        let key = (actor.owner, actor.layer);
        let Some(events) = actor.body.events(actor.motion) else {
            self.previous.insert(key, 0);
            return Vec::new();
        };
        let group = &events.groups[3];
        let wrapped = actor.body.wrap_frame(actor.motion, actor.frame);
        let frame = wrapped as i32;
        let bits = ddcombat::events::event_bits(group, frame, frame);
        let previous = self.previous.insert(key, bits).unwrap_or(0);
        let ids = rising_ids(previous, bits, &group.remap);
        if ids.is_empty() { return Vec::new(); }
        let pose = actor.body.pose(actor.motion, wrapped, actor.position, actor.yaw);
        let mut cues = Vec::new();
        for id in ids {
            let Some(request) = self.requests.get(id) else { continue; };
            let selected = match actor.selector {
                Some(selector) => request.request(selector),
                None if request.requests[0] == request.requests[2] => request.requests[0],
                None => continue,
            };
            if selected < 0 { continue; }
            let matrix = if request.joint < 0 { Some(&pose.model) } else {
                pose.joints.iter().find(|(id, _)| i32::from(*id) == request.joint).map(|(_, m)| m)
            };
            let Some(matrix) = matrix else { continue; };
            let position = ddcombat::volumes::transform_point(matrix, request.offset);
            let (slot, selected) = if request.surface {
                let Some(row) = self.surfaces.0.get(selected as usize).and_then(Option::as_ref) else { continue; };
                // Cached unit-floor lookup (UseFindIntersect=false) unconnected.
                if !row.find_intersection { continue; }
                let from = [position[0], position[1] + request.up_margin, position[2]];
                let to = [position[0], position[1] + request.down_margin, position[2]];
                let Some(hit) = surface(from, to) else { continue; };
                let selected = row.request(surface_index(hit.material_word));
                // 0x00c72b60 / getter0x00423be0: independent bank override.
                let index = if hit.triangle_attribute & 2 != 0 { 2 } else { 0 };
                (attribute_bank(row.request_type, index), selected)
            } else {
                (direct_bank(request.request_type), selected as u32)
            };
            let Ok(se_id) = u16::try_from(selected) else { continue; };
            if se_id == u16::MAX { continue; }
            let Some(path) = slot.and_then(|slot| self.bank(slot, &actor)) else { continue; };
            cues.push(Cue::Se { unit: format!("motion:{}:{}:{id}", actor.owner, actor.layer),
                path, se_id: i32::from(se_id), conditional: None });
        }
        cues
    }

    pub fn forget_actor(&mut self, owner: u32) {
        self.previous.retain(|(actor, _), _| *actor != owner);
    }

    /// Sample every represented actor once per fixed simulation tick, after
    /// its motion advances. Layer zero only: upper-body/blended motion layers
    /// are not yet represented by the headless player simulation.
    pub fn sample_sim(&mut self, sim: &ddsim::Sim,
        data: &ddhost::ddenemyrt::fight::FightData, terrain: &ddworld::Terrain) {
        let present: HashSet<_> = sim.players.iter().map(|p| u32::from(p.id)).collect();
        self.previous.retain(|(actor, _), _| present.contains(actor));
        for player in &sim.players {
            let body = player.skills.active.and_then(|a| data.skills.0.get(&a.id))
                .map(|skill| &skill.body).unwrap_or_else(|| {
                    if player.weapon == ddsim::Weapon::Staff { &data.mage.body } else { &data.arisen.body }
                });
            let motion = player.motion_id();
            let cues = self.sample(ActorSoundFrame {
                owner: u32::from(player.id), layer: 0, body,
                motion: ddhost::ddenemyrt::body::MotionId::new((motion >> 8) as u8, motion & 255),
                frame: player.frame, position: player.position, yaw: player.yaw,
                selector: None,
                primary_model: self.kit_models.get(&player.weapon).copied(),
                secondary_model: None,
            }, |from, to| {
                let hit = terrain.ray_with_material(from, to)?;
                Some(SoundSurface { material_word: hit.material?[1], triangle_attribute: hit.attribute? })
            });
            self.pending.extend(cues);
        }
    }
}

fn queue_motion_sounds(sounds: Option<ResMut<PlayerMotionSounds>>, mut audio: ResMut<AudioState>) {
    if let Some(mut sounds) = sounds { audio.pending.append(&mut sounds.pending); }
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
    fn installed_motion_sounds_keep_actor_edges_separate_and_resolve_weapon_waves() {
        use ddhost::ddenemyrt::{arisen::ArisenData, body::MotionId};
        let root = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !root.is_dir() { eprintln!("SKIP: no install"); return; }
        let vfs = ddvfs::Vfs::open(root).unwrap();
        let data = ArisenData::fighter(&vfs).unwrap();
        let items = dditem::itl::ItemList::parse(&vfs.read(r"etc\item\itemList",
            mtarc::type_hash("rItemList")).unwrap()).unwrap();
        let model = items.get(ddhost::ddenemyrt::arisen::IRON_SWORD).unwrap().equip_model();
        let mut audio = PlayerMotionSounds::load(&vfs).unwrap();
        let input = |owner, layer, motion, frame| ActorSoundFrame {
            owner, layer, body: &data.body, motion, frame,
            position: [0.0; 3], yaw: 0.0, selector: None,
            primary_model: Some(model), secondary_model: None,
        };
        let surface = |_: [f32; 3], _: [f32; 3]| Some(SoundSurface {
            material_word: 1, triangle_attribute: 0,
        });
        let mut footstep = None;
        'motions: for slot in 0..data.body.banks[0].as_ref().unwrap().list.motions.len() {
            let motion = MotionId::new(0, slot as u16);
            audio.forget_actor(10);
            for frame in 0..data.body.frames(motion) {
                let cues = audio.sample(input(10, 0, motion, frame as f32), surface);
                if cues.iter().any(|c| matches!(c, Cue::Se { path, .. } if path.contains(r"\mt\"))) {
                    footstep = Some((motion, frame as f32, cues));
                    break 'motions;
                }
            }
        }
        let (motion, frame, first) = footstep.expect("installed common motion has a surface sound");
        assert!(audio.sample(input(10, 0, motion, frame), surface).is_empty());
        let other = audio.sample(input(11, 0, motion, frame), surface);
        let layer = audio.sample(input(10, 1, motion, frame), surface);
        assert!(!other.is_empty()); assert!(!layer.is_empty());
        assert_ne!(first, other); // owner retained in source identity
        audio.forget_actor(10);
        assert_eq!(audio.sample(input(10, 0, motion, frame), surface), first);

        let attack = MotionId::new(7, 0);
        let mut decoded = 0;
        audio.forget_actor(10);
        for frame in 0..data.body.frames(attack) {
            for cue in audio.sample(input(10, 0, attack, frame as f32), surface) {
                let Cue::Se { path, se_id, .. } = cue else { unreachable!() };
                if !path.contains(r"\wep\") { continue; }
                let bytes = vfs.read(&path, mtarc::type_hash("rSoundRequest")).unwrap();
                let requests = mtsound::Requests::parse(&bytes).unwrap();
                let Ok(request) = requests.single_wave(se_id as u16) else { continue; };
                let bytes = vfs.read(request.source.unwrap(), mtarc::type_hash("rSoundPackage")).unwrap();
                let package = mtsound::Package::parse(&bytes).unwrap();
                let (program, variation) = request.wave_keys();
                let wave = package.wave_for(program, variation).unwrap().unwrap();
                let pcm = mtsound::decode::wave(&wave.riff(), 4_000_000).unwrap();
                assert!(pcm.samples.iter().any(|v| v.abs() > 0.001));
                decoded += 1;
            }
        }
        assert!(decoded > 0, "sword motion must resolve at least one weapon wave");
    }
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
