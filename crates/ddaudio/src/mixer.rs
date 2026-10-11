//! The graph renderer: one processing pass (quantum) at a time.
//!
//! The runtime (`ddruntime::audio`) owns the graph state the guest sets
//! (voices, sends, matrices, volumes, effects, buffer queues and their
//! timing). Each pass it feeds the source frames its clock consumed
//! ([`Mixer::feed`]) and hands a snapshot of the graph ([`Node`]s) to
//! [`Mixer::render`], which returns the mastering voice's output.
//!
//! Per-voice order, from Microsoft's "XAudio2 Voices" page: a source voice
//! decodes, then runs its variable-rate SRC to the rate of its destinations;
//! every voice then applies (1) its volume, (2) its effect chain, (3) its
//! channel volumes, (4) one matrix mix per destination. A submix voice runs a
//! fixed-rate SRC on its output when its destinations' rate differs. Submix
//! voices are processed in processing-stage order ("XAudio2 Audio Graph";
//! `CreateSubmixVoice` `ProcessingStage`), then the mastering voice.
//!
//! Not implemented, because the game never uses it (census in the L19
//! brief): voice filters and output filters (no `XAUDIO2_VOICE_USEFILTER`
//! voice, no `XAUDIO2_SEND_USEFILTER` send, no SetFilterParameters /
//! SetOutputFilterParameters call in the sound code).
use crate::resample::Resampler;
use crate::reverb::Reverb;
use std::collections::{BTreeMap, HashMap};

pub type VoiceId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Source,
    Submix,
    Mastering,
}

#[derive(Clone, Debug)]
pub struct Send<'a> {
    pub dest: VoiceId,
    /// `pLevelMatrix[SourceChannels * d + s]`; `None` = XAudio2's default.
    pub matrix: Option<&'a [f32]>,
}

#[derive(Clone, Debug)]
pub enum EffectKind<'a> {
    /// AudioReverb 2.7 with its last `XAUDIO2FX_REVERB_PARAMETERS` bytes
    /// (empty: the effect's defaults).
    Reverb(&'a [u8]),
    /// A guest XAPO, processed by the [`EffectHost`].
    Guest(u32),
}

#[derive(Clone, Debug)]
pub struct Effect<'a> {
    pub kind: EffectKind<'a>,
    pub enabled: bool,
    /// The effect's output channel count (`XAUDIO2_EFFECT_DESCRIPTOR`).
    pub channels: u32,
}

#[derive(Clone, Debug)]
pub struct Node<'a> {
    pub id: VoiceId,
    pub kind: Kind,
    /// Input channels and rate: the source format's, or the submix's /
    /// mastering voice's `InputChannels` / `InputSampleRate`.
    pub channels: u32,
    pub rate: u32,
    pub stage: u32,
    /// Source voices: started. Submix and mastering voices always run.
    pub running: bool,
    pub ratio: f32,
    pub volume: f32,
    /// Empty = all 1.0.
    pub channel_volumes: &'a [f32],
    pub effects: Vec<Effect<'a>>,
    pub sends: Vec<Send<'a>>,
}

impl Node<'_> {
    /// Channels after the effect chain.
    pub fn output_channels(&self) -> u32 {
        self.effects.last().map_or(self.channels, |e| e.channels)
    }
}

/// Runs guest effects (IXAPO::Process). `buffer` is interleaved, `frames`
/// frames of `channels` channels. `valid` is the input's
/// `XAPO_BUFFER_VALID` flag; the return value is the output's.
pub trait EffectHost {
    #[allow(clippy::too_many_arguments)]
    fn process(
        &mut self,
        voice: VoiceId,
        index: usize,
        object: u32,
        buffer: &mut [f32],
        channels: u32,
        frames: u32,
        valid: bool,
        enabled: bool,
    ) -> bool;
}

/// Passes guest effects through unchanged (tests, or before the host binds).
pub struct NoGuestEffects;
impl EffectHost for NoGuestEffects {
    fn process(
        &mut self,
        _: VoiceId,
        _: usize,
        _: u32,
        _: &mut [f32],
        _: u32,
        _: u32,
        valid: bool,
        _: bool,
    ) -> bool {
        valid
    }
}

#[derive(Default)]
struct Dsp {
    channels: u32,
    resampler: Resampler,
    reverbs: BTreeMap<usize, Reverb>,
    /// The levels in effect at the end of the last pass, ramped from.
    volume: Option<f32>,
    channel_volumes: Vec<f32>,
    matrices: HashMap<VoiceId, Vec<f32>>,
}

/// Volume, channel-volume and matrix changes ramp linearly over this many
/// frames from the old to the new level: the host's XAudio2 2.9 does so at
/// 48 kHz, starting at the first pass after the change; the pass's frame 0
/// still has the old level (oracle `level_changes_ramp_like_the_host`).
pub const RAMP_FRAMES: usize = 48;

fn ramp(old: f32, new: f32, k: usize) -> f32 {
    if k >= RAMP_FRAMES || old == new {
        new
    } else {
        old + (new - old) * (k as f32 / RAMP_FRAMES as f32)
    }
}

#[derive(Default)]
pub struct Mixer {
    dsp: HashMap<VoiceId, Dsp>,
}

/// One voice's input for this pass: interleaved frames and whether any
/// signal arrived (`XAPO_BUFFER_VALID`) or it is silent.
struct Bus {
    channels: u32,
    samples: Vec<f32>,
    valid: bool,
}

/// XAudio2's default matrix when a send has no `SetOutputMatrix`, for the
/// channel counts the game's voices have (1, 2, 6; the endpoint is stereo).
/// Values: `GetOutputMatrix` of the host's XAudio2 2.9 for voices without a
/// custom matrix (oracle `default_matrices_match_the_host`, which also
/// renders them); mono to stereo agrees with "XAudio2 Default Channel
/// Mapping" (both front channels at full scale). The same matrices apply
/// to a stereo mastering voice. Other channel pairs were not measured: they
/// fall back to a channel-for-channel diagonal (an unverified guess; the
/// game creates no such pair).
pub fn default_matrix(src: u32, dst: u32) -> Vec<f32> {
    // -3 dB, as the host reports it (10^(-3/20) rounded to f32).
    const M3DB: f32 = 0.70794576;
    let (s, d) = (src as usize, dst as usize);
    let measured: Option<&[f32]> = match (src, dst) {
        (1, 1) => Some(&[1.0]),
        (2, 1) => Some(&[0.5, 0.5]),
        // The host's value is one ulp below 1.0f32 / 6.0.
        (6, 1) => Some(&[0.16666666; 6]),
        (1, 2) => Some(&[1.0, 1.0]),
        (2, 2) => Some(&[1.0, 0.0, 0.0, 1.0]),
        // FL, FR, FC, LFE, BL, BR: centre and back at -3 dB, LFE dropped.
        (6, 2) => Some(&[1.0, 0.0, M3DB, 0.0, M3DB, 0.0, 0.0, 1.0, M3DB, 0.0, 0.0, M3DB]),
        (1, 6) => Some(&[1.0, 1.0, 0.0, 0.0, 0.0, 0.0]),
        _ => None,
    };
    if let Some(m) = measured {
        return m.to_vec();
    }
    // 2 -> 6 and 6 -> 6 measured as this diagonal too.
    let mut m = vec![0.0; s * d];
    for c in 0..s.min(d) {
        m[s * c + c] = 1.0;
    }
    m
}

impl Mixer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends source frames the runtime's clock consumed this pass.
    pub fn feed(&mut self, voice: VoiceId, channels: u32, frames: &[f32]) {
        let d = self.dsp.entry(voice).or_default();
        if d.channels != channels {
            d.channels = channels;
            d.resampler = Resampler::default();
        }
        d.resampler.push(frames);
    }

    /// Forgets a destroyed voice's DSP state.
    pub fn remove(&mut self, voice: VoiceId) {
        self.dsp.remove(&voice);
    }

    /// Processes one pass. Returns the mastering voice's interleaved output
    /// (`channels * rate / 100` samples), or `None` without one.
    pub fn render(&mut self, nodes: &[Node], host: &mut dyn EffectHost) -> Option<Vec<f32>> {
        let by_id: HashMap<VoiceId, &Node> = nodes.iter().map(|n| (n.id, n)).collect();
        let mut buses: HashMap<VoiceId, Bus> = HashMap::new();
        for n in nodes.iter().filter(|n| n.kind != Kind::Source) {
            buses.insert(
                n.id,
                Bus {
                    channels: n.channels,
                    samples: vec![0.0; (n.channels * n.rate / 100) as usize],
                    valid: false,
                },
            );
        }
        let mut order: Vec<&Node> = nodes.iter().collect();
        order.sort_by_key(|n| (n.kind != Kind::Source, n.kind == Kind::Mastering, n.stage, n.id));
        let mut master = None;
        for n in order {
            let dest_rate = n.sends.first().and_then(|s| by_id.get(&s.dest)).map(|d| d.rate);
            let (mut samples, valid, rate) = match n.kind {
                Kind::Source => {
                    if !n.running {
                        continue;
                    }
                    let Some(rate) = dest_rate else {
                        // Nothing to hear: still consume in real time.
                        self.drain(n, n.rate);
                        continue;
                    };
                    let (s, valid) = self.drain(n, rate);
                    (s, valid, rate)
                }
                _ => {
                    let bus = buses.remove(&n.id).unwrap();
                    (bus.samples, bus.valid, n.rate)
                }
            };
            let frames = (samples.len() / n.channels.max(1) as usize) as u32;
            // (1) volume
            {
                let dsp = self.dsp.entry(n.id).or_default();
                let old = dsp.volume.unwrap_or(n.volume);
                for (k, frame) in samples.chunks_mut(n.channels.max(1) as usize).enumerate() {
                    let g = ramp(old, n.volume, k);
                    frame.iter_mut().for_each(|x| *x *= g);
                }
                dsp.volume = Some(n.volume);
            }
            // (2) effect chain
            let (mut samples, mut channels, mut valid) = (samples, n.channels, valid);
            for (index, e) in n.effects.iter().enumerate() {
                match &e.kind {
                    EffectKind::Guest(object) => {
                        valid = host.process(
                            n.id,
                            index,
                            *object,
                            &mut samples,
                            channels,
                            frames,
                            valid,
                            e.enabled,
                        );
                    }
                    EffectKind::Reverb(params) => {
                        let dsp = self.dsp.entry(n.id).or_default();
                        let r = dsp
                            .reverbs
                            .entry(index)
                            .or_insert_with(|| Reverb::new(rate, channels, e.channels));
                        r.set_parameters(params);
                        let out = r.process(&samples, frames, e.enabled);
                        samples = out;
                        valid = true;
                    }
                }
                if channels != e.channels && samples.len() != (e.channels * frames) as usize {
                    samples.resize((e.channels * frames) as usize, 0.0);
                }
                channels = e.channels;
            }
            // (3) channel volumes
            {
                let ch = channels.max(1) as usize;
                let new: Vec<f32> = (0..ch)
                    .map(|c| n.channel_volumes.get(c).copied().unwrap_or(1.0))
                    .collect();
                let dsp = self.dsp.entry(n.id).or_default();
                let old = if dsp.channel_volumes.len() == ch {
                    std::mem::take(&mut dsp.channel_volumes)
                } else {
                    new.clone()
                };
                if old != new || new.iter().any(|g| *g != 1.0) {
                    for (k, frame) in samples.chunks_mut(ch).enumerate() {
                        for (c, x) in frame.iter_mut().enumerate() {
                            *x *= ramp(old[c], new[c], k);
                        }
                    }
                }
                dsp.channel_volumes = new;
            }
            if n.kind == Kind::Mastering {
                master = Some(samples);
                continue;
            }
            if !valid {
                continue;
            }
            // Submix voices resample their output when the destination
            // rate differs (fixed-rate SRC).
            let samples = match (n.kind, dest_rate) {
                (Kind::Submix, Some(d)) if d != rate => {
                    let dsp = self.dsp.entry(n.id).or_default();
                    dsp.channels = channels;
                    dsp.resampler.push(&samples);
                    dsp.resampler
                        .render(channels, rate as f64 / d as f64, (d / 100) as usize)
                        .0
                }
                _ => samples,
            };
            // (4) one matrix mix per destination
            let dsp = self.dsp.entry(n.id).or_default();
            dsp.matrices.retain(|d, _| n.sends.iter().any(|s| s.dest == *d));
            for send in &n.sends {
                let Some(bus) = buses.get_mut(&send.dest) else {
                    continue;
                };
                let m = match send.matrix {
                    Some(m) => m.to_vec(),
                    None => default_matrix(channels, bus.channels),
                };
                let old = dsp
                    .matrices
                    .remove(&send.dest)
                    .filter(|o| o.len() == m.len())
                    .unwrap_or_else(|| m.clone());
                mix_ramped(&samples, channels, &old, &m, &mut bus.samples, bus.channels);
                dsp.matrices.insert(send.dest, m);
                bus.valid = true;
            }
        }
        master
    }

    /// Resamples a source voice's consumed frames to `rate`.
    fn drain(&mut self, n: &Node, rate: u32) -> (Vec<f32>, bool) {
        let d = self.dsp.entry(n.id).or_default();
        if d.channels != n.channels {
            d.channels = n.channels;
            d.resampler = Resampler::default();
        }
        let step = n.rate as f64 * n.ratio as f64 / rate as f64;
        d.resampler.render(n.channels, step, (rate / 100) as usize)
    }
}

/// [`mix`], with the matrix ramping from `old` to `new` over the first
/// [`RAMP_FRAMES`] frames.
pub fn mix_ramped(input: &[f32], src: u32, old: &[f32], new: &[f32], out: &mut [f32], dst: u32) {
    let (s, d) = (src as usize, dst as usize);
    let head = if old == new {
        0
    } else {
        RAMP_FRAMES.min(input.len() / s.max(1))
    };
    let mut m = vec![0.0; new.len()];
    for k in 0..head {
        for (j, x) in m.iter_mut().enumerate() {
            *x = ramp(old[j], new[j], k);
        }
        mix(
            &input[k * s..(k + 1) * s],
            src,
            &m,
            &mut out[k * d..(k + 1) * d],
            dst,
        );
    }
    mix(&input[head * s..], src, new, &mut out[head * d..], dst);
}

/// `out[d] += in[s] * m[src * d + s]` for every frame.
pub fn mix(input: &[f32], src: u32, m: &[f32], out: &mut [f32], dst: u32) {
    let (s, d) = (src as usize, dst as usize);
    for (fi, fo) in input.chunks_exact(s).zip(out.chunks_exact_mut(d)) {
        for (j, o) in fo.iter_mut().enumerate() {
            let row = &m[s * j..s * j + s];
            let mut acc = 0.0f32;
            for (x, w) in fi.iter().zip(row) {
                acc += x * w;
            }
            *o += acc;
        }
    }
}
