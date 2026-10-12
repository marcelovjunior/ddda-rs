//! XAudio2 2.7's guest ABI and clocked audio graph.
//!
//! 0x00DCE0E0 creates the engine; 0x00DCE600 creates AudioReverb 2.7.
//! Source formats/callbacks come from 0x00DCE1F0 / 0x00DCE2E0 / 0x00DCE530.
//! Contracts and the call-site census are in the L16 brief. No host audio
//! device is opened. The explicit null endpoint is stereo, 48 kHz.
//! The clock here decides what each source voice consumes per pass and
//! when callbacks fire; `ddaudio` mixes the consumed samples (brief L19).
use crate::dispatch::{Conflict, Registry};
use crate::input::{guid, write_guid, IUNKNOWN};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, call_guest, ret, zero};
use ddnative_base::{mem::Mem, x86::Cpu};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

pub const CLSID_ENGINE: [u32; 4] = [0x5a508685, 0x4fbaa254, 0x249a829b, 0xaf0603b0];
pub const IID_ENGINE: [u32; 4] = [0x8bcf1f58, 0x45839fe7, 0xade2c68a, 0xbbc865c4];
pub const CLSID_REVERB: [u32; 4] = [0x6a93130e, 0x41d11d53, 0x58e7cfa9, 0x79b10b80];
const INVALID: u32 = 0x88960001;
const E_INVALIDARG: u32 = 0x80070057;
const QUANTUM: Duration = Duration::from_millis(10); // XAudio2 2.7 Windows processing quantum
const RATE: u32 = 48000;
const CHANNELS: u32 = 2;

fn state<R>(f: impl FnOnce(&mut Audio) -> R) -> R {
    crate::runtime::with(|r| r.audio.with(f))
}
#[derive(Clone, Debug)]
pub struct AudioBlock {
    pub sample_rate: u32,
    pub channels: u32,
    pub samples: Vec<f32>,
}
/// A bounded output seam: the mastering voice's mix, one block per pass.
/// A slow host drops output blocks, never stalls the guest audio callback.
pub fn output() -> Receiver<AudioBlock> {
    let (tx, rx) = mpsc::sync_channel(8);
    state(|s| s.output = Some(tx));
    rx
}

#[derive(Clone)]
struct Format {
    tag: u16,
    channels: u32,
    rate: u32,
    align: u32,
    bits: u32,
    samples_per_block: u32,
    /// ADPCMWAVEFORMAT coefficient pairs (mmreg.h).
    coefs: Vec<(i16, i16)>,
}
impl Format {
    fn read(m: &dyn Mem, p: u32) -> Result<Self, u32> {
        if p == 0 {
            return Err(E_INVALIDARG);
        }
        let f = Self {
            tag: m.u16(p),
            channels: m.u16(p + 2) as u32,
            rate: m.u32(p + 4),
            align: m.u16(p + 12) as u32,
            bits: m.u16(p + 14) as u32,
            samples_per_block: if m.u16(p) == 2 && m.u16(p + 16) >= 4 {
                m.u16(p + 18) as u32
            } else {
                0
            },
            coefs: if m.u16(p) == 2 && m.u16(p + 16) >= 4 {
                let n = (m.u16(p + 20) as u32).min((m.u16(p + 16) as u32 - 4) / 4);
                (0..n)
                    .map(|i| (m.u16(p + 22 + 4 * i) as i16, m.u16(p + 24 + 4 * i) as i16))
                    .collect()
            } else {
                vec![]
            },
        };
        if f.channels == 0 || f.channels > 64 || !(1000..=200000).contains(&f.rate) || f.align == 0 {
            return Err(INVALID);
        }
        match f.tag {
            1 if matches!(f.bits, 8 | 16 | 24 | 32) && f.align == f.channels * f.bits / 8 => {}
            3 if f.bits == 32 && f.align == f.channels * 4 => {}
            2 if f.samples_per_block >= 2 && f.bits == 4 => {}
            _ => return Err(INVALID),
        }
        Ok(f)
    }
    fn dsp(&self) -> ddaudio::Format {
        ddaudio::Format {
            tag: self.tag,
            channels: self.channels,
            rate: self.rate,
            block_align: self.align,
            bits: self.bits,
            samples_per_block: if self.tag == 2 { self.samples_per_block } else { 1 },
            coefs: self.coefs.clone(),
        }
    }
    fn samples(&self, bytes: u32) -> u64 {
        if self.tag == 2 {
            (bytes / self.align) as u64 * self.samples_per_block as u64
        } else {
            (bytes / self.align) as u64
        }
    }
}
#[derive(Clone)]
struct Buffer {
    context: u32,
    data: u32,
    flags: u32,
    end: u64,
    loop_start: u64,
    loop_end: u64,
    loops: u32,
    cursor: u64,
    started: bool,
}
#[derive(Clone)]
struct Effect {
    object: u32,
    enabled: bool,
    channels: u32,
    parameters: Vec<u8>,
}
#[derive(Clone)]
enum Change {
    Start(bool),
    Volume(f32),
    Channels(Vec<f32>),
    Ratio(f32),
    Filter([u32; 3]),
    OutputFilter(u32, [u32; 3]),
    Matrix(u32, Vec<f32>),
    EffectState(usize, bool),
    EffectParameters(usize, Vec<u8>),
    ExitLoop,
}
struct Voice {
    engine: u32,
    kind: u32,
    format: Format,
    flags: u32,
    stage: u32,
    callback: u32,
    max_ratio: f32,
    running: bool,
    ratio: f32,
    volume: f32,
    volumes: Vec<f32>,
    filter: [u32; 3],
    output_filters: BTreeMap<u32, [u32; 3]>,
    matrices: BTreeMap<u32, Vec<f32>>,
    sends: Vec<u32>,
    effects: Vec<Effect>,
    queue: VecDeque<Buffer>,
    flushed: VecDeque<u32>,
    played: u64,
    fraction: f64,
}
struct Engine {
    refs: u32,
    initialized: bool,
    running: bool,
    worker: bool,
    callbacks: Vec<u32>,
    changes: Vec<(u32, u32, Change)>,
}
#[derive(Default)]
pub(crate) struct Audio {
    engine_vtable: u32,
    source_vtable: u32,
    voice_vtable: u32,
    effect_vtable: u32,
    worker_entry: u32,
    engines: BTreeMap<u32, Engine>,
    voices: BTreeMap<u32, Voice>,
    effects: BTreeMap<u32, u32>,
    output: Option<SyncSender<AudioBlock>>,
    mixer: ddaudio::Mixer,
    /// The guest thread mixing a pass (L65): the mixer is out of this state
    /// meanwhile, and in parallel mode the pass runs without the guest lock.
    rendering: Option<u32>,
    /// Voices destroyed by the mixing thread itself during its pass (from a
    /// guest effect), removed from the mixer when the pass ends.
    destroyed_while_rendering: Vec<u32>,
    /// Guest XAPOs by (voice, effect index, object): the locked IXAPO, or
    /// None when it cannot run (QueryInterface failed, or its code is not
    /// lifted yet).
    xapos: BTreeMap<(u32, usize, u32), Option<u32>>,
    /// A clock the session advances itself ([`use_test_clock`]); None:
    /// real time.
    test_clock: Option<Duration>,
    /// With the test clock: the quanta each running worker has processed.
    test_quanta: BTreeMap<u32, u64>,
}

/// Run every engine created from now on in this session on a clock the
/// caller advances ([`advance_test_clock`]) instead of real time, so that
/// the quanta processed (and the callbacks and samples they produce) are
/// exact. For tests; the game always runs on real time.
pub fn use_test_clock() {
    state(|s| s.test_clock = Some(Duration::ZERO));
}

/// Advance the test clock and wake the audio workers, which process every
/// quantum that has now elapsed.
pub fn advance_test_clock(by: Duration) {
    state(|s| {
        let c = s.test_clock.as_mut().expect("advance_test_clock without use_test_clock");
        *c += by;
    });
    crate::threads::signal_key(crate::threads::key::AUDIO_CLOCK);
}

/// Whether every audio worker has processed all quanta the test clock has
/// reached.
pub fn test_clock_settled() -> bool {
    state(|s| {
        let target = s.test_clock.map_or(0, |c| (c.as_nanos() / QUANTUM.as_nanos()) as u64);
        s.test_quanta.values().all(|&q| q >= target)
    })
}

const ENGINE_NAMES: &[&str] = &[
    "QueryInterface",
    "AddRef",
    "Release",
    "GetDeviceCount",
    "GetDeviceDetails",
    "Initialize",
    "RegisterForCallbacks",
    "UnregisterForCallbacks",
    "CreateSourceVoice",
    "CreateSubmixVoice",
    "CreateMasteringVoice",
    "StartEngine",
    "StopEngine",
    "CommitChanges",
    "GetPerformanceData",
    "SetDebugConfiguration",
];
const VOICE_NAMES: &[&str] = &[
    "GetVoiceDetails",
    "SetOutputVoices",
    "SetEffectChain",
    "EnableEffect",
    "DisableEffect",
    "GetEffectState",
    "SetEffectParameters",
    "GetEffectParameters",
    "SetFilterParameters",
    "GetFilterParameters",
    "SetOutputFilterParameters",
    "GetOutputFilterParameters",
    "SetVolume",
    "GetVolume",
    "SetChannelVolumes",
    "GetChannelVolumes",
    "SetOutputMatrix",
    "GetOutputMatrix",
    "DestroyVoice",
    "Start",
    "Stop",
    "SubmitSourceBuffer",
    "FlushSourceBuffers",
    "Discontinuity",
    "ExitLoop",
    "GetState",
    "SetFrequencyRatio",
    "GetFrequencyRatio",
    "SetSourceSampleRate",
];
const VOICE_METHODS: &[(u32, interfaces::Method)] = &[
    (0, voice::<0>),
    (1, voice::<1>),
    (2, voice::<2>),
    (3, voice::<3>),
    (4, voice::<4>),
    (5, voice::<5>),
    (6, voice::<6>),
    (7, voice::<7>),
    (8, voice::<8>),
    (9, voice::<9>),
    (10, voice::<10>),
    (11, voice::<11>),
    (12, voice::<12>),
    (13, voice::<13>),
    (14, voice::<14>),
    (15, voice::<15>),
    (16, voice::<16>),
    (17, voice::<17>),
    (18, voice::<18>),
    (19, voice::<19>),
    (20, voice::<20>),
    (21, voice::<21>),
    (22, voice::<22>),
    (23, voice::<23>),
    (24, voice::<24>),
    (25, voice::<25>),
    (26, voice::<26>),
    (27, voice::<27>),
    (28, voice::<28>),
];
pub fn install(m: &mut dyn Mem, r: &mut Registry, t: &mut Tokens) -> Result<(), Conflict> {
    let e = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "IXAudio2(2.7)",
            slots: 16,
            names: ENGINE_NAMES,
            methods: &[
                (0, engine::<0>),
                (1, engine::<1>),
                (2, engine::<2>),
                (3, engine::<3>),
                (4, engine::<4>),
                (5, engine::<5>),
                (6, engine::<6>),
                (7, engine::<7>),
                (8, engine::<8>),
                (9, engine::<9>),
                (10, engine::<10>),
                (11, engine::<11>),
                (12, engine::<12>),
                (13, engine::<13>),
                (14, engine::<14>),
                (15, engine::<15>),
            ],
        },
    )?;
    let v = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "IXAudio2Voice(2.7)",
            slots: 19,
            names: VOICE_NAMES,
            methods: VOICE_METHODS,
        },
    )?;
    let s = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "IXAudio2SourceVoice(2.7)",
            slots: 29,
            names: VOICE_NAMES,
            methods: VOICE_METHODS,
        },
    )?;
    let f = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "AudioReverb(2.7)::IUnknown",
            slots: 3,
            names: &["QueryInterface", "AddRef", "Release"],
            methods: &[(0, effect::<0>), (1, effect::<1>), (2, effect::<2>)],
        },
    )?;
    let w = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "XAudio2 runtime worker",
            slots: 1,
            names: &["run"],
            methods: &[(0, worker)],
        },
    )?;
    state(|a| {
        a.engine_vtable = e;
        a.voice_vtable = v;
        a.source_vtable = s;
        a.effect_vtable = f;
        a.worker_entry = m.u32(w);
    });
    Ok(())
}
pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[("ole32.dll", "CoCreateInstance", co_create)];
pub fn co_create(m: &mut dyn Mem, c: &mut Cpu) {
    let (clsid, outer, context, iid, out) = (
        arg(m, c, 0),
        arg(m, c, 1),
        arg(m, c, 2),
        arg(m, c, 3),
        arg(m, c, 4),
    );
    c.eax = if out == 0 || clsid == 0 || iid == 0 {
        E_INVALIDARG
    } else {
        m.set_u32(out, 0);
        let cl = guid(m, clsid);
        let id = guid(m, iid);
        if outer != 0 {
            0x80040110
        } else if context & 1 == 0 {
            0x80040154
        } else if cl == CLSID_ENGINE && (id == IID_ENGINE || id == IUNKNOWN) {
            let o = interfaces::object(m, state(|s| s.engine_vtable), 4);
            state(|s| {
                s.engines.insert(
                    o,
                    Engine {
                        refs: 1,
                        initialized: false,
                        running: false,
                        worker: false,
                        callbacks: vec![],
                        changes: vec![],
                    },
                );
            });
            m.set_u32(out, o);
            0
        } else if cl == CLSID_REVERB && id == IUNKNOWN {
            let o = interfaces::object(m, state(|s| s.effect_vtable), 4);
            state(|s| {
                s.effects.insert(o, 1);
            });
            m.set_u32(out, o);
            0
        } else if cl == CLSID_ENGINE || cl == CLSID_REVERB {
            0x80004002
        } else {
            0x80040154
        }
    };
    ret(m, c, 20);
}
fn effect<const SLOT: u32>(m: &mut dyn Mem, c: &mut Cpu) {
    let this = arg(m, c, 0);
    if SLOT == 0 {
        let (iid, out) = (arg(m, c, 1), arg(m, c, 2));
        c.eax = if iid == 0 || out == 0 {
            E_INVALIDARG
        } else {
            m.set_u32(out, 0);
            if guid(m, iid) == IUNKNOWN {
                state(|s| *s.effects.get_mut(&this).expect("released effect") += 1);
                m.set_u32(out, this);
                0
            } else {
                0x80004002
            }
        };
        ret(m, c, 12);
    } else {
        c.eax = state(|s| {
            let refs = s.effects.get_mut(&this).expect("released effect");
            if SLOT == 1 {
                *refs += 1
            } else {
                *refs -= 1
            }
            let n = *refs;
            if n == 0 {
                s.effects.remove(&this);
            }
            n
        });
        ret(m, c, 4);
    }
}

fn callback(m: &mut dyn Mem, c: &mut Cpu, object: u32, slot: u32, args: &[u32]) {
    if object != 0 {
        let target = m.u32(m.u32(object) + 4 * slot);
        let mut all = vec![object];
        all.extend_from_slice(args);
        call_guest(m, c, target, &all);
    }
}
// IXAPOParameters is a separate COM interface, not an IXAPO vtable slot.
// xapobase.h / xapo.h: QueryInterface, SetParameters (slot 3), Release.
// The 2.7-era IIDs (DirectX SDK xapo.h), not the Windows SDK's 2.8+ ones:
// the game's XAPO QueryInterface 0x00DCF480 compares with the
// IXAPOParameters IID at 0x0142AF38, its base 0x00DCF350 with the IXAPO
// IID at 0x0142AF28: A90BC001-E897-E897-55E4-9E470000000{1,0}.
const IID_PARAMETERS: [u32; 4] = [0xa90bc001, 0xe897e897, 0x479ee455, 0x01000000];
fn effect_parameters(m: &mut dyn Mem, c: &mut Cpu, this: u32, value: &Change) {
    let Change::EffectParameters(index, data) = value else {
        return;
    };
    let object = state(|s| {
        s.voices
            .get(&this)
            .and_then(|v| v.effects.get(*index))
            .map(|e| e.object)
    });
    let Some(object) = object else { return };
    if state(|s| s.effects.contains_key(&object)) {
        return;
    }
    let saved = c.esp;
    c.esp -= 20 + data.len() as u32;
    let p = c.esp;
    write_guid(m, p, IID_PARAMETERS);
    m.set_u32(p + 16, 0);
    callback(m, c, object, 0, &[p, p + 16]);
    let params = m.u32(p + 16);
    if c.eax == 0 && params != 0 {
        for (i, b) in data.iter().enumerate() {
            m.set_u8(p + 20 + i as u32, *b);
        }
        callback(m, c, params, 3, &[p + 20, data.len() as u32]);
        callback(m, c, params, 2, &[]);
    }
    c.esp = saved;
}
fn apply(v: &mut Voice, change: Change) {
    match change {
        Change::Start(b) => v.running = b,
        Change::Volume(x) => v.volume = x,
        Change::Channels(x) => v.volumes = x,
        Change::Ratio(x) => v.ratio = x,
        Change::Filter(x) => v.filter = x,
        Change::OutputFilter(d, x) => {
            v.output_filters.insert(d, x);
        }
        Change::Matrix(d, x) => {
            v.matrices.insert(d, x);
        }
        Change::EffectState(i, b) => {
            if let Some(e) = v.effects.get_mut(i) {
                e.enabled = b;
            }
        }
        Change::EffectParameters(i, x) => {
            if let Some(e) = v.effects.get_mut(i) {
                e.parameters = x;
            }
        }
        Change::ExitLoop => {
            if let Some(b) = v.queue.front_mut() {
                b.loops = 0;
            }
        }
    }
}
fn change(m: &mut dyn Mem, c: &mut Cpu, this: u32, set: u32, value: Change) {
    if set == 0 {
        effect_parameters(m, c, this, &value);
    }
    state(|s| {
        let e = s.voices[&this].engine;
        if set == 0 {
            apply(s.voices.get_mut(&this).unwrap(), value);
        } else {
            s.engines.get_mut(&e).unwrap().changes.push((set, this, value));
        }
    });
}
fn start_worker(m: &mut dyn Mem, c: &mut Cpu, engine: u32) {
    let start = state(|s| {
        let e = s.engines.get_mut(&engine).unwrap();
        if e.worker {
            return None;
        }
        e.worker = true;
        if let Some(clock) = s.test_clock {
            // counted from the start, before the worker thread first runs
            s.test_quanta.insert(engine, (clock.as_nanos() / QUANTUM.as_nanos()) as u64);
        }
        Some(s.worker_entry)
    });
    if let Some(start) = start {
        let saved = c.esp;
        for a in [0, 0, engine, start, 0, 0].into_iter() {
            c.esp -= 4;
            m.set_u32(c.esp, a);
        }
        c.esp -= 4;
        m.set_u32(c.esp, crate::services::CALLBACK_RETURN);
        crate::threads::create_thread(m, c);
        assert_eq!(c.esp, saved);
        crate::threads::close(c.eax);
    }
}
fn worker(m: &mut dyn Mem, c: &mut Cpu) {
    let engine = arg(m, c, 0);
    if state(|s| s.test_clock.is_some()) {
        return test_worker(m, c, engine);
    }
    let mut next = Instant::now() + QUANTUM;
    loop {
        // woken by the engine's release (or the next quantum)
        crate::threads::block_on(&[crate::threads::key::audio_engine(engine)], Some(next));
        if !state(|s| s.engines.contains_key(&engine)) {
            break;
        }
        let now = Instant::now();
        if now < next {
            continue;
        }
        // One quantum at a time, including delayed quanta: no dropped callbacks.
        while next <= now {
            process(m, c, engine);
            next += QUANTUM;
            if !state(|s| s.engines.contains_key(&engine)) {
                break;
            }
        }
    }
    c.eax = 0;
    ret(m, c, 4);
}

/// The worker on the test clock: the quanta come from the clock the test
/// advances, one at a time as on real time; between advances it waits
/// (and rechecks the engine every quantum of real time, so it ends when
/// the engine goes).
fn test_worker(m: &mut dyn Mem, c: &mut Cpu, engine: u32) {
    let mut done = state(|s| s.test_quanta[&engine]);
    loop {
        if !state(|s| s.engines.contains_key(&engine)) {
            break;
        }
        let target = state(|s| (s.test_clock.unwrap().as_nanos() / QUANTUM.as_nanos()) as u64);
        if done < target {
            process(m, c, engine);
            done += 1;
            state(|s| s.test_quanta.insert(engine, done));
            continue;
        }
        crate::threads::block_on(
            &[crate::threads::key::audio_engine(engine), crate::threads::key::AUDIO_CLOCK],
            Some(Instant::now() + QUANTUM),
        );
    }
    state(|s| s.test_quanta.remove(&engine));
    c.eax = 0;
    ret(m, c, 4);
}

/// Process one 10ms graph quantum on the engine's guest audio thread.
fn process(m: &mut dyn Mem, c: &mut Cpu, engine: u32) {
    let callbacks = state(|s| {
        s.engines
            .get(&engine)
            .filter(|e| e.running)
            .map(|e| e.callbacks.clone())
    });
    let Some(callbacks) = callbacks else { return };
    for &cb in &callbacks {
        callback(m, c, cb, 0, &[]);
    }
    let voices = state(|s| {
        s.voices
            .iter()
            .filter(|(_, v)| v.engine == engine && v.kind == 0)
            .map(|(&a, _)| a)
            .collect::<Vec<_>>()
    });
    for id in voices {
        let info = state(|s| {
            s.voices
                .get(&id)
                .map(|v| (v.callback, v.running, v.format.rate, v.ratio))
        });
        let Some((cb, running, rate, ratio)) = info else {
            continue;
        };
        let flushed = state(|s| {
            s.voices
                .get_mut(&id)
                .map(|v| v.flushed.drain(..).collect::<Vec<_>>())
                .unwrap_or_default()
        });
        for context in flushed {
            if !state(|s| s.voices.contains_key(&id)) {
                break;
            }
            callback(m, c, cb, 4, &[context]);
        }
        if !running || !state(|s| s.voices.contains_key(&id)) {
            continue;
        }
        let required = state(|s| {
            let v = &s.voices[&id];
            let needed = (v.fraction + rate as f64 * ratio as f64 / 100.0).ceil() as u64;
            let queued = v.queue.iter().fold(0u64, |sum, b| {
                if b.loops == 255 {
                    u64::MAX
                } else {
                    sum.saturating_add(b.end.saturating_sub(b.cursor))
                        .saturating_add((b.loop_end - b.loop_start).saturating_mul(b.loops as u64))
                }
            });
            let samples = needed.saturating_sub(queued);
            let bytes = if v.format.tag == 2 {
                samples.div_ceil(v.format.samples_per_block as u64) * v.format.align as u64
            } else {
                samples * v.format.align as u64
            };
            bytes.min(u32::MAX as u64) as u32
        });
        callback(m, c, cb, 0, &[required]);
        let mut samples = state(|s| {
            let Some(v) = s.voices.get_mut(&id) else { return 0 };
            let x = v.fraction + rate as f64 * ratio as f64 / 100.0;
            v.fraction = x.fract();
            x.floor() as u64
        });
        while samples > 0 {
            // Emit one boundary at a time. A callback may submit, stop or
            // destroy a voice; never retain a state borrow across guest code.
            let action = state(|s| {
                let v = s.voices.get_mut(&id)?;
                if !v.running {
                    return None;
                }
                let b = v.queue.front_mut()?;
                if !b.started {
                    b.started = true;
                    return Some((3, b.context));
                }
                let boundary = if b.loops != 0 { b.loop_end } else { b.end };
                let n = samples.min(boundary.saturating_sub(b.cursor));
                // Decode what the clock consumes now: once a buffer-end
                // callback runs, the guest may reuse the memory.
                feed(&mut s.mixer, &*m, id, &v.format, b.data, b.cursor, n);
                b.cursor += n;
                v.played += n;
                samples -= n;
                if b.cursor < boundary {
                    return None;
                }
                if b.loops != 0 {
                    if b.loops != 255 {
                        b.loops -= 1;
                    }
                    b.cursor = b.loop_start;
                    Some((5, b.context))
                } else {
                    let b = v.queue.pop_front().unwrap();
                    if b.flags & 0x40 != 0 {
                        v.played = 0;
                    }
                    Some((if b.flags & 0x40 != 0 { 7 } else { 4 }, b.context))
                }
            });
            match action {
                Some((7, ctx)) => {
                    callback(m, c, cb, 4, &[ctx]);
                    if state(|s| s.voices.contains_key(&id)) {
                        callback(m, c, cb, 2, &[]);
                    }
                }
                Some((slot, ctx)) => callback(m, c, cb, slot, &[ctx]),
                None => break,
            }
        }
        if state(|s| s.voices.contains_key(&id)) {
            callback(m, c, cb, 1, &[]);
        }
    }
    let block = render(m, c, engine);
    state(|s| {
        if let (Some(tx), Some(block)) = (&s.output, block) {
            let _ = tx.try_send(block);
        }
    });
    for cb in callbacks {
        if state(|s| s.engines.get(&engine).is_some_and(|e| e.callbacks.contains(&cb))) {
            callback(m, c, cb, 1, &[]);
        }
    }
}

/// Decodes frames `first .. first + n` of a source buffer at guest `data`
/// into the voice's mixer input.
fn feed(mixer: &mut ddaudio::Mixer, m: &dyn Mem, voice: u32, f: &Format, data: u32, first: u64, n: u64) {
    if n == 0 {
        return;
    }
    let format = f.dsp();
    let spb = format.samples_per_block as u64;
    let (block0, block1) = (first / spb, (first + n).div_ceil(spb));
    let base = data.wrapping_add((block0 * f.align as u64) as u32);
    let mut bytes = vec![0u8; ((block1 - block0) * f.align as u64) as usize];
    m.read_bytes(base, &mut bytes);
    let mut frames = Vec::new();
    format.decode(&bytes, first - block0 * spb, n, &mut frames);
    mixer.feed(voice, f.channels, &frames);
}

/// One voice of the graph as the mixer sees it, owned so that no runtime
/// borrow is held while guest effects run.
struct Snapshot {
    id: u32,
    kind: ddaudio::Kind,
    channels: u32,
    rate: u32,
    stage: u32,
    running: bool,
    ratio: f32,
    volume: f32,
    volumes: Vec<f32>,
    /// (reverb parameters for the runtime's AudioReverb, object, enabled,
    /// output channels)
    effects: Vec<(Option<Vec<u8>>, u32, bool, u32)>,
    sends: Vec<(u32, Option<Vec<f32>>)>,
}

/// Mixes the pass for `engine`: the mastering voice's output block.
fn render(m: &mut dyn Mem, c: &mut Cpu, engine: u32) -> Option<AudioBlock> {
    const KINDS: [ddaudio::Kind; 3] = [
        ddaudio::Kind::Source,
        ddaudio::Kind::Submix,
        ddaudio::Kind::Mastering,
    ];
    let (snapshot, mut mixer) = state(|s| {
        let snapshot: Vec<Snapshot> = s
            .voices
            .iter()
            .filter(|(_, v)| v.engine == engine)
            .map(|(&id, v)| Snapshot {
                id,
                kind: KINDS[v.kind as usize],
                channels: v.format.channels,
                rate: v.format.rate,
                stage: v.stage,
                running: v.running,
                ratio: v.ratio,
                volume: v.volume,
                volumes: v.volumes.clone(),
                effects: v
                    .effects
                    .iter()
                    .map(|e| {
                        let reverb = s.effects.contains_key(&e.object).then(|| e.parameters.clone());
                        (reverb, e.object, e.enabled, e.channels)
                    })
                    .collect(),
                sends: v
                    .sends
                    .iter()
                    .map(|d| {
                        // A matrix set with a NULL destination applies to
                        // the only destination.
                        let single = v.sends.len() == 1;
                        let m = v
                            .matrices
                            .get(d)
                            .or_else(|| v.matrices.get(&0).filter(|_| single));
                        (*d, m.cloned())
                    })
                    .collect(),
            })
            .collect();
        s.rendering = Some(crate::threads::current_tid());
        (snapshot, std::mem::take(&mut s.mixer))
    });
    // the pass ends (also by a panic out of a guest effect): DestroyVoice
    // waits for this
    struct Pass;
    impl Drop for Pass {
        fn drop(&mut self) {
            state(|s| s.rendering = None);
            crate::threads::signal_key(crate::threads::key::AUDIO_PASS);
        }
    }
    let pass = Pass;
    let master = snapshot
        .iter()
        .find(|n| n.kind == ddaudio::Kind::Mastering)
        .map(|n| (n.channels, n.rate));
    let nodes: Vec<ddaudio::Node> = snapshot
        .iter()
        .map(|n| ddaudio::Node {
            id: n.id,
            kind: n.kind,
            channels: n.channels,
            rate: n.rate,
            stage: n.stage,
            running: n.running,
            ratio: n.ratio,
            volume: n.volume,
            channel_volumes: &n.volumes,
            effects: n
                .effects
                .iter()
                .map(|(reverb, object, enabled, channels)| ddaudio::Effect {
                    kind: match reverb {
                        Some(p) => ddaudio::EffectKind::Reverb(p),
                        None => ddaudio::EffectKind::Guest(*object),
                    },
                    enabled: *enabled,
                    channels: *channels,
                })
                .collect(),
            sends: n
                .sends
                .iter()
                .map(|(d, m)| ddaudio::Send {
                    dest: *d,
                    matrix: m.as_deref(),
                })
                .collect(),
        })
        .collect();
    // L65: mixing is host work on the snapshot and the mixer's own buffers
    // (the guest effects take the lock for the state they read), so in
    // parallel mode the pass runs without the guest lock; voice changes made
    // meanwhile apply from the next pass, as XAudio2 applies them
    let out = crate::threads::unlocked(|| mixer.render(&nodes, &mut GuestEffects { m, c }));
    state(|s| {
        for v in s.destroyed_while_rendering.drain(..) {
            mixer.remove(v);
        }
        s.mixer = mixer;
    });
    drop(pass);
    let (channels, sample_rate) = master?;
    Some(AudioBlock {
        sample_rate,
        channels,
        samples: out?,
    })
}

// IXAPO (xapo.h): QueryInterface 0, Release 2, LockForProcess 8,
// UnlockForProcess 9, Process 10 (the game's vtable 0x0142B138 has Process
// 0x00DCF4F0 at slot 10); the 2.7 IID (see IID_PARAMETERS).
const IID_IXAPO: [u32; 4] = [0xa90bc001, 0xe897e897, 0x479ee455, 0x00000000];
// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT (ksmedia.h)
const SUBTYPE_FLOAT: [u32; 4] = [0x00000003, 0x00100000, 0xaa000080, 0x719b3800];

/// Runs the game's own XAPOs the way XAudio2 does: QueryInterface for
/// IXAPO, LockForProcess once with the voice's float format (the
/// WAVEFORMATEXTENSIBLE, channel masks and 480-frame maximum the host's
/// XAudio2 passes, measured in the L19 brief), then in-place Process each
/// pass with the input's buffer flag.
struct GuestEffects<'a> {
    m: &'a mut dyn Mem,
    c: &'a mut Cpu,
}

impl GuestEffects<'_> {
    /// Calls slot `slot` of `object`; `None` when the call reaches code that
    /// is not lifted (the effect then passes audio through, reported once).
    fn call(&mut self, object: u32, slot: u32, args: &[u32]) -> Option<u32> {
        let saved = (self.c.esp, self.c.ebx, self.c.ebp, self.c.esi, self.c.edi);
        let target = self.m.u32(self.m.u32(object) + 4 * slot);
        let mut all = vec![object];
        all.extend_from_slice(args);
        let (m, c) = (&mut *self.m, &mut *self.c);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call_guest(m, c, target, &all))) {
            Ok(r) => Some(r),
            Err(p) => match p.downcast::<crate::dispatch::Unbound>() {
                Ok(u) => {
                    (self.c.esp, self.c.ebx, self.c.ebp, self.c.esi, self.c.edi) = saved;
                    eprintln!("audio: guest XAPO {object:#010x} passes audio through: {u}");
                    None
                }
                Err(p) => std::panic::resume_unwind(p),
            },
        }
    }

    fn lock(&mut self, object: u32, channels: u32, rate: u32) -> Option<u32> {
        let saved = self.c.esp;
        self.c.esp -= 96;
        let p = self.c.esp;
        write_guid(self.m, p, IID_IXAPO);
        self.m.set_u32(p + 16, 0);
        let qi = self.call(object, 0, &[p, p + 16]);
        let xapo = self.m.u32(p + 16);
        let result = if qi == Some(0) && xapo != 0 {
            // WAVEFORMATEXTENSIBLE at p + 24 (40 bytes).
            let f = p + 24;
            let mask = match channels {
                1 => 0x4,
                2 => 0x3,
                6 => 0x60f,
                _ => 0,
            };
            for (o, v) in [
                (0, 0xfffe),
                (2, channels),
                (12, channels * 4),
                (14, 32),
                (16, 22),
                (18, 32),
            ] {
                self.m.set_u16(f + o, v as u16);
            }
            self.m.set_u32(f + 4, rate);
            self.m.set_u32(f + 8, rate * channels * 4);
            self.m.set_u32(f + 20, mask);
            write_guid(self.m, f + 24, SUBTYPE_FLOAT);
            // XAPO_LOCKFORPROCESS_BUFFER_PARAMETERS, input and output.
            for q in [p + 64, p + 72] {
                self.m.set_u32(q, f);
                self.m.set_u32(q + 4, rate / 100);
            }
            match self.call(xapo, 8, &[1, p + 64, 1, p + 72]) {
                Some(0) => Some(xapo),
                _ => {
                    // Drop the reference QueryInterface added.
                    self.call(xapo, 2, &[]);
                    None
                }
            }
        } else {
            None
        };
        self.c.esp = saved;
        result
    }
}

impl ddaudio::EffectHost for GuestEffects<'_> {
    fn process(
        &mut self,
        voice: u32,
        index: usize,
        object: u32,
        buffer: &mut [f32],
        channels: u32,
        frames: u32,
        valid: bool,
        enabled: bool,
    ) -> bool {
        let key = (voice, index, object);
        // the pass may run without the guest lock (render): runtime state
        // only under it
        let xapo = match crate::threads::with_lock(|| state(|s| s.xapos.get(&key).copied())) {
            Some(x) => x,
            None => {
                let x = self.lock(object, channels, frames * 100);
                crate::threads::with_lock(|| state(|s| s.xapos.insert(key, x)));
                x
            }
        };
        let Some(xapo) = xapo else { return valid };
        let bytes = buffer.len() as u32 * 4;
        let saved = self.c.esp;
        self.c.esp = (self.c.esp - bytes - 32) & !15;
        let (data, params) = (self.c.esp, self.c.esp + bytes);
        for (i, x) in buffer.iter().enumerate() {
            self.m.set_u32(data + 4 * i as u32, x.to_bits());
        }
        // XAPO_PROCESS_BUFFER_PARAMETERS, input and output: pBuffer (the
        // same memory: in-place), BufferFlags, ValidFrameCount.
        for q in [params, params + 12] {
            self.m.set_u32(q, data);
            self.m.set_u32(q + 4, valid as u32);
            self.m.set_u32(q + 8, frames);
        }
        let out_valid = match self.call(xapo, 10, &[1, params, 1, params + 12, enabled as u32]) {
            Some(_) => {
                for (i, x) in buffer.iter_mut().enumerate() {
                    *x = f32::from_bits(self.m.u32(data + 4 * i as u32));
                }
                self.m.u32(params + 16) != 0
            }
            None => {
                crate::threads::with_lock(|| state(|s| s.xapos.insert(key, None)));
                valid
            }
        };
        self.c.esp = saved;
        out_valid
    }
}

// Engine and voice control methods follow below.
fn sends(m: &dyn Mem, p: u32, engine: u32) -> Result<Vec<u32>, u32> {
    if p == 0 {
        return Ok(state(|s| {
            s.voices
                .iter()
                .filter(|(_, v)| v.engine == engine && v.kind == 2)
                .map(|(&a, _)| a)
                .collect()
        }));
    }
    let n = m.u32(p);
    let a = m.u32(p + 4);
    if n > 64 || n != 0 && a == 0 {
        return Err(INVALID);
    }
    let mut out = vec![];
    for i in 0..n {
        let flags = m.u32(a + 8 * i);
        let dest = m.u32(a + 8 * i + 4);
        if flags & !0x80 != 0
            || out.contains(&dest)
            || !state(|s| {
                s.voices
                    .get(&dest)
                    .is_some_and(|v| v.engine == engine && v.kind != 0)
            })
        {
            return Err(INVALID);
        }
        out.push(dest);
    }
    Ok(out)
}
fn send_filters(m: &dyn Mem, p: u32) -> BTreeMap<u32, [u32; 3]> {
    let mut filters = BTreeMap::new();
    if p != 0 {
        let a = m.u32(p + 4);
        for i in 0..m.u32(p) {
            if m.u32(a + i * 8) & 0x80 != 0 {
                filters.insert(m.u32(a + i * 8 + 4), [0, 1.0f32.to_bits(), 1.0f32.to_bits()]);
            }
        }
    }
    filters
}
fn effects(m: &dyn Mem, p: u32) -> Result<Vec<Effect>, u32> {
    if p == 0 {
        return Ok(vec![]);
    }
    let n = m.u32(p);
    let a = m.u32(p + 4);
    if n > 64 || n != 0 && a == 0 {
        return Err(INVALID);
    }
    (0..n)
        .map(|i| {
            let object = m.u32(a + 12 * i);
            let enabled = m.u32(a + 12 * i + 4) != 0;
            let channels = m.u32(a + 12 * i + 8);
            if object == 0 || channels == 0 || channels > 64 {
                Err(INVALID)
            } else {
                Ok(Effect {
                    object,
                    enabled,
                    channels,
                    parameters: vec![],
                })
            }
        })
        .collect()
}
fn retain_effects(m: &mut dyn Mem, c: &mut Cpu, fx: &[Effect], release: bool) {
    for e in fx {
        let slot = if release { 2 } else { 1 };
        callback(m, c, e.object, slot, &[]);
    }
}
fn destroy(m: &mut dyn Mem, c: &mut Cpu, this: u32) {
    // XAudio2 unlocks and releases a destroyed voice's effects.
    let locked: Vec<u32> = state(|s| {
        let keys: Vec<_> = s.xapos.keys().filter(|k| k.0 == this).copied().collect();
        keys.iter().filter_map(|k| s.xapos.remove(k).flatten()).collect()
    });
    for xapo in locked {
        callback(m, c, xapo, 9, &[]);
        callback(m, c, xapo, 2, &[]);
    }
    // XAudio2's DestroyVoice waits until the audio thread is not processing
    // the voice; here, until a pass in progress (without the guest lock,
    // L65) ends. The mixing thread itself (a guest effect destroying a
    // voice) cannot wait for its own pass: the voice leaves the mixer when
    // the pass ends.
    let me = crate::threads::current_tid();
    while state(|s| s.rendering.is_some_and(|t| t != me)) {
        crate::threads::block_on(&[crate::threads::key::AUDIO_PASS], None);
    }
    let old = state(|s| {
        if s.rendering.is_some() {
            s.destroyed_while_rendering.push(this);
        } else {
            s.mixer.remove(this);
        }
        let old = s.voices.remove(&this);
        for e in s.engines.values_mut() {
            e.changes.retain(|(_, id, _)| *id != this);
        }
        old
    });
    if let Some(v) = old {
        retain_effects(m, c, &v.effects, true);
    }
}
fn engine<const SLOT: u32>(m: &mut dyn Mem, c: &mut Cpu) {
    let this = arg(m, c, 0);
    assert!(
        state(|s| s.engines.contains_key(&this)),
        "released XAudio2 engine"
    );
    let cleanup = match SLOT {
        0 | 4 | 5 | 15 => 12,
        1 | 2 | 11 | 12 => 4,
        3 | 6 | 7 | 13 | 14 => 8,
        8 | 9 => 32,
        10 => 28,
        _ => unreachable!(),
    };
    c.eax = match SLOT {
        0 => {
            let (iid, out) = (arg(m, c, 1), arg(m, c, 2));
            if out == 0 || iid == 0 {
                E_INVALIDARG
            } else {
                m.set_u32(out, 0);
                if matches!(guid(m, iid), IUNKNOWN | IID_ENGINE) {
                    state(|s| s.engines.get_mut(&this).unwrap().refs += 1);
                    m.set_u32(out, this);
                    0
                } else {
                    0x80004002
                }
            }
        }
        1 => state(|s| {
            let e = s.engines.get_mut(&this).unwrap();
            e.refs += 1;
            e.refs
        }),
        2 => {
            let refs = state(|s| {
                let e = s.engines.get_mut(&this).unwrap();
                e.refs -= 1;
                e.refs
            });
            if refs == 0 {
                let voices = state(|s| {
                    s.voices
                        .iter()
                        .filter(|(_, v)| v.engine == this)
                        .map(|(&a, _)| a)
                        .collect::<Vec<_>>()
                });
                for v in voices {
                    destroy(m, c, v);
                }
                state(|s| {
                    s.engines.remove(&this);
                });
                crate::threads::signal_key(crate::threads::key::audio_engine(this));
            }
            refs
        }
        3 => {
            let p = arg(m, c, 1);
            if p == 0 {
                E_INVALIDARG
            } else {
                m.set_u32(p, 1);
                0
            }
        }
        4 => {
            let (index, p) = (arg(m, c, 1), arg(m, c, 2));
            if index != 0 || p == 0 {
                E_INVALIDARG
            } else {
                // XAUDIO2_DEVICE_DETAILS (2.7): WCHAR ID[256], name[256],
                // role, WAVEFORMATEXTENSIBLE. A declared null endpoint profile.
                zero(m, p, 1068);
                for (off, name) in [(0, "ddruntime:null"), (512, "DDDA headless audio")] {
                    for (i, w) in name.encode_utf16().enumerate() {
                        m.set_u16(p + off + 2 * i as u32, w);
                    }
                }
                m.set_u32(p + 1024, 0xf);
                let f = p + 1028;
                m.set_u16(f, 0xfffe);
                m.set_u16(f + 2, CHANNELS as u16);
                m.set_u32(f + 4, RATE);
                m.set_u32(f + 8, RATE * CHANNELS * 4);
                m.set_u16(f + 12, (CHANNELS * 4) as u16);
                m.set_u16(f + 14, 32);
                m.set_u16(f + 16, 22);
                m.set_u16(f + 18, 32);
                m.set_u32(f + 20, 3);
                write_guid(m, f + 24, [3, 0x00100000, 0xaa000080, 0x719b3800]);
                0
            }
        }
        5 => {
            let (flags, processor) = (arg(m, c, 1), arg(m, c, 2));
            if flags & !1 != 0 || processor == 0 {
                INVALID
            } else if state(|s| s.engines[&this].initialized) {
                INVALID
            } else {
                state(|s| {
                    let e = s.engines.get_mut(&this).unwrap();
                    e.initialized = true;
                    e.running = true;
                });
                start_worker(m, c, this);
                0
            }
        }
        6 => {
            let cb = arg(m, c, 1);
            if cb == 0 {
                E_INVALIDARG
            } else {
                state(|s| {
                    let list = &mut s.engines.get_mut(&this).unwrap().callbacks;
                    if !list.contains(&cb) {
                        list.push(cb);
                    }
                });
                0
            }
        }
        7 => {
            let cb = arg(m, c, 1);
            state(|s| s.engines.get_mut(&this).unwrap().callbacks.retain(|&a| a != cb));
            0
        }
        8 | 9 | 10 => create_voice(m, c, this, SLOT),
        11 => {
            state(|s| s.engines.get_mut(&this).unwrap().running = true);
            0
        }
        12 => {
            state(|s| s.engines.get_mut(&this).unwrap().running = false);
            0
        }
        13 => {
            let set = arg(m, c, 1);
            let changes = state(|s| std::mem::take(&mut s.engines.get_mut(&this).unwrap().changes));
            for (op, id, value) in changes {
                if set == 0 || op == set {
                    effect_parameters(m, c, id, &value);
                    state(|s| {
                        if let Some(v) = s.voices.get_mut(&id) {
                            apply(v, value);
                        }
                    });
                } else {
                    state(|s| s.engines.get_mut(&this).unwrap().changes.push((op, id, value)));
                }
            }
            0
        }
        14 => {
            let p = arg(m, c, 1);
            if p != 0 {
                zero(m, p, 64);
                let counts = state(|s| {
                    (
                        s.voices
                            .values()
                            .filter(|v| v.engine == this && v.kind == 0 && v.running)
                            .count(),
                        s.voices
                            .values()
                            .filter(|v| v.engine == this && v.kind == 0)
                            .count(),
                        s.voices
                            .values()
                            .filter(|v| v.engine == this && v.kind == 1)
                            .count(),
                    )
                });
                m.set_u32(p + 36, counts.0 as u32);
                m.set_u32(p + 40, counts.1 as u32);
                m.set_u32(p + 44, counts.2 as u32);
            }
            0
        }
        15 => 0,
        _ => unreachable!(),
    };
    ret(m, c, cleanup);
}
fn create_voice(m: &mut dyn Mem, c: &mut Cpu, engine: u32, slot: u32) -> u32 {
    let out = arg(m, c, 1);
    if out == 0 {
        return E_INVALIDARG;
    }
    m.set_u32(out, 0);
    if !state(|s| s.engines[&engine].initialized) {
        return INVALID;
    }
    let (kind, format, flags, stage, cb, ratio, send_ptr, effect_ptr) = if slot == 8 {
        let f = match Format::read(m, arg(m, c, 2)) {
            Ok(f) => f,
            Err(e) => return e,
        };
        (
            0,
            f,
            arg(m, c, 3),
            0,
            arg(m, c, 5),
            f32::from_bits(arg(m, c, 4)),
            arg(m, c, 6),
            arg(m, c, 7),
        )
    } else {
        let channels = match arg(m, c, 2) {
            0 => CHANNELS,
            n => n,
        };
        let rate = match arg(m, c, 3) {
            0 => RATE,
            n => n,
        };
        let f = Format {
            tag: 3,
            channels,
            rate,
            align: channels * 4,
            bits: 32,
            samples_per_block: 0,
            coefs: vec![],
        };
        (
            if slot == 9 { 1 } else { 2 },
            f,
            arg(m, c, 4),
            if slot == 9 { arg(m, c, 5) } else { 0 },
            0,
            1.0,
            if slot == 9 { arg(m, c, 6) } else { 0 },
            if slot == 9 { arg(m, c, 7) } else { arg(m, c, 6) },
        )
    };
    if format.channels == 0
        || format.channels > 64
        || !(1000..=200000).contains(&format.rate)
        || !ratio.is_finite()
        || !(1.0..=1024.0).contains(&ratio)
    {
        return INVALID;
    }
    if slot == 10
        && (arg(m, c, 5) != 0 || state(|s| s.voices.values().any(|v| v.engine == engine && v.kind == 2)))
    {
        return INVALID;
    }
    if (kind == 0 && flags & !0x1a != 0) || (kind == 1 && flags & !8 != 0) || (kind == 2 && flags != 0) {
        return INVALID;
    }
    let sends = if kind == 2 {
        vec![]
    } else {
        match sends(m, send_ptr, engine) {
            Ok(x) => x,
            Err(e) => return e,
        }
    };
    let effects = match effects(m, effect_ptr) {
        Ok(x) => x,
        Err(e) => return e,
    };
    retain_effects(m, c, &effects, false);
    let table = state(|s| {
        if kind == 0 {
            s.source_vtable
        } else {
            s.voice_vtable
        }
    });
    let object = interfaces::object(m, table, 4);
    let channels = format.channels;
    let output_filters = send_filters(m, send_ptr);
    state(|s| {
        s.voices.insert(
            object,
            Voice {
                engine,
                kind,
                format,
                flags,
                stage,
                callback: cb,
                max_ratio: ratio,
                running: kind != 0,
                ratio: 1.0,
                volume: 1.0,
                volumes: vec![1.0; channels as usize],
                filter: [0, 1.0f32.to_bits(), 1.0f32.to_bits()],
                output_filters,
                matrices: BTreeMap::new(),
                sends,
                effects,
                queue: VecDeque::new(),
                flushed: VecDeque::new(),
                played: 0,
                fraction: 0.0,
            },
        );
    });
    m.set_u32(out, object);
    0
}
fn submit(m: &dyn Mem, this: u32, p: u32, wma: u32) -> u32 {
    if p == 0 {
        return E_INVALIDARG;
    }
    if wma != 0 {
        return INVALID;
    }
    let flags = m.u32(p);
    let bytes = m.u32(p + 4);
    let data = m.u32(p + 8);
    let start = m.u32(p + 12) as u64;
    let length = m.u32(p + 16) as u64;
    let loop_start = m.u32(p + 20) as u64;
    let loop_length = m.u32(p + 24) as u64;
    let loops = m.u32(p + 28);
    let context = m.u32(p + 32);
    state(|s| {
        let v = s.voices.get_mut(&this).unwrap();
        let total = v.format.samples(bytes);
        let end = if length == 0 { total } else { start + length };
        let loop_end = if loop_length == 0 {
            end
        } else {
            loop_start + loop_length
        };
        if v.kind != 0
            || v.queue.len() >= 64
            || flags & !0x40 != 0
            || bytes == 0
            || bytes > 0x80000000
            || data == 0
            || bytes % v.format.align != 0
            || end > total
            || end <= start
            || length == 0 && start != 0
            || loops > 255
            || loops == 0 && (loop_start != 0 || loop_length != 0)
            || loops != 0
                && (loop_start >= end || loop_end <= start || loop_end > end || loop_end <= loop_start)
        {
            return INVALID;
        }
        if v.format.tag == 2
            && [start, end, loop_start, loop_end]
                .iter()
                .any(|n| n % v.format.samples_per_block as u64 != 0)
        {
            return INVALID;
        }
        v.queue.push_back(Buffer {
            context,
            data,
            flags,
            end,
            loop_start,
            loop_end,
            loops,
            cursor: start,
            started: false,
        });
        0
    })
}
fn floats(m: &dyn Mem, p: u32, n: u32) -> Result<Vec<f32>, u32> {
    if p == 0 || n > 4096 {
        return Err(INVALID);
    }
    let x = (0..n)
        .map(|i| f32::from_bits(m.u32(p + 4 * i)))
        .collect::<Vec<_>>();
    if x.iter().any(|x| !x.is_finite() || x.abs() > 16777216.0) {
        Err(INVALID)
    } else {
        Ok(x)
    }
}
fn voice<const SLOT: u32>(m: &mut dyn Mem, c: &mut Cpu) {
    let this = arg(m, c, 0);
    assert!(state(|s| s.voices.contains_key(&this)), "destroyed XAudio2 voice");
    // 2.7 GetState has only the state pointer (2.8 adds flags).
    let cleanup = match SLOT {
        0 | 1 | 2 | 9 | 13 | 18 | 22 | 23 | 24 | 25 | 27 | 28 => {
            if matches!(SLOT, 18 | 22 | 23) {
                4
            } else {
                8
            }
        }
        3 | 4 | 5 | 8 | 11 | 12 | 15 | 19 | 20 | 21 | 26 => 12,
        7 | 10 | 14 => 16,
        6 | 17 => 20,
        16 => 24,
        _ => unreachable!(),
    };
    c.eax = match SLOT {
        0 => {
            let p = arg(m, c, 1);
            state(|s| {
                let v = &s.voices[&this];
                m.set_u32(p, v.flags);
                m.set_u32(p + 4, v.format.channels);
                m.set_u32(p + 8, v.format.rate);
            });
            0
        }
        1 => {
            let p = arg(m, c, 1);
            let engine = state(|s| s.voices[&this].engine);
            match sends(m, p, engine) {
                Ok(x) => {
                    let valid = state(|s| {
                        let v = &s.voices[&this];
                        x.iter().all(|dest| {
                            let d = &s.voices[dest];
                            *dest != this && (v.kind != 1 || d.kind == 2 || d.stage > v.stage)
                        })
                    });
                    if valid {
                        let filters = send_filters(m, p);
                        state(|s| {
                            let v = s.voices.get_mut(&this).unwrap();
                            v.sends = x;
                            v.output_filters = filters;
                            // "A voice's current send levels will be replaced
                            // by a default send matrix" (SetOutputVoices).
                            v.matrices.clear();
                        });
                        0
                    } else {
                        INVALID
                    }
                }
                Err(e) => e,
            }
        }
        2 => {
            let p = arg(m, c, 1);
            match effects(m, p) {
                Ok(x) => {
                    retain_effects(m, c, &x, false);
                    let old = state(|s| std::mem::replace(&mut s.voices.get_mut(&this).unwrap().effects, x));
                    retain_effects(m, c, &old, true);
                    0
                }
                Err(e) => e,
            }
        }
        3 | 4 => {
            let (index, set) = (arg(m, c, 1) as usize, arg(m, c, 2));
            if state(|s| index < s.voices[&this].effects.len()) {
                change(m, c, this, set, Change::EffectState(index, SLOT == 3));
                0
            } else {
                INVALID
            }
        }
        5 => {
            let (index, p) = (arg(m, c, 1) as usize, arg(m, c, 2));
            let enabled = state(|s| s.voices[&this].effects.get(index).map(|e| e.enabled));
            if let Some(enabled) = enabled {
                m.set_u32(p, u32::from(enabled));
                0
            } else {
                INVALID
            }
        }
        6 | 7 => {
            let (index, p, n) = (arg(m, c, 1) as usize, arg(m, c, 2), arg(m, c, 3));
            if p == 0
                || n > 65536
                || !state(|s| {
                    s.voices[&this]
                        .effects
                        .get(index)
                        .is_some_and(|e| !s.effects.contains_key(&e.object) || n == 52)
                })
            {
                INVALID
            } else if SLOT == 6 {
                let data = (0..n).map(|i| m.u8(p + i)).collect();
                let set = arg(m, c, 4);
                change(m, c, this, set, Change::EffectParameters(index, data));
                0
            } else {
                let data = state(|s| s.voices[&this].effects[index].parameters.clone());
                if data.len() != n as usize {
                    INVALID
                } else {
                    for (i, b) in data.into_iter().enumerate() {
                        m.set_u8(p + i as u32, b);
                    }
                    0
                }
            }
        }
        8 | 10 => {
            let (p, set, dest) = if SLOT == 8 {
                (arg(m, c, 1), arg(m, c, 2), 0)
            } else {
                (arg(m, c, 2), arg(m, c, 3), arg(m, c, 1))
            };
            let dest = if SLOT == 10 && dest == 0 {
                state(|s| {
                    let v = &s.voices[&this];
                    if v.sends.len() == 1 {
                        v.sends[0]
                    } else {
                        0
                    }
                })
            } else {
                dest
            };
            let available = state(|s| {
                if SLOT == 8 {
                    s.voices[&this].flags & 8 != 0
                } else {
                    s.voices[&this].output_filters.contains_key(&dest)
                }
            });
            if p == 0 || !available {
                INVALID
            } else {
                let f = [m.u32(p), m.u32(p + 4), m.u32(p + 8)];
                let freq = f32::from_bits(f[1]);
                let q = f32::from_bits(f[2]);
                if f[0] > 5
                    || !freq.is_finite()
                    || !(0.0..=1.0).contains(&freq)
                    || !q.is_finite()
                    || !(0.0..=1.5).contains(&q)
                    || q == 0.0
                {
                    INVALID
                } else {
                    change(
                        m,
                        c,
                        this,
                        set,
                        if SLOT == 8 {
                            Change::Filter(f)
                        } else {
                            Change::OutputFilter(dest, f)
                        },
                    );
                    0
                }
            }
        }
        9 | 11 => {
            let (p, dest) = if SLOT == 9 {
                (arg(m, c, 1), 0)
            } else {
                (arg(m, c, 2), arg(m, c, 1))
            };
            let f = state(|s| {
                let v = &s.voices[&this];
                if SLOT == 9 {
                    v.filter
                } else {
                    v.output_filters
                        .get(&dest)
                        .copied()
                        .unwrap_or([0, 1.0f32.to_bits(), 1.0f32.to_bits()])
                }
            });
            for (i, x) in f.into_iter().enumerate() {
                m.set_u32(p + 4 * i as u32, x);
            }
            0
        }
        12 => {
            let value = f32::from_bits(arg(m, c, 1));
            if !value.is_finite() || value.abs() > 16777216.0 {
                INVALID
            } else {
                let set = arg(m, c, 2);
                change(m, c, this, set, Change::Volume(value));
                0
            }
        }
        13 => {
            m.set_u32(arg(m, c, 1), state(|s| s.voices[&this].volume.to_bits()));
            0
        }
        14 | 15 => {
            let (n, p) = (arg(m, c, 1), arg(m, c, 2));
            if n != state(|s| s.voices[&this].format.channels) || p == 0 {
                INVALID
            } else if SLOT == 14 {
                match floats(m, p, n) {
                    Ok(x) => {
                        let set = arg(m, c, 3);
                        change(m, c, this, set, Change::Channels(x));
                        0
                    }
                    Err(e) => e,
                }
            } else {
                let values = state(|s| s.voices[&this].volumes.clone());
                for (i, x) in values.into_iter().enumerate() {
                    m.set_u32(p + 4 * i as u32, x.to_bits());
                }
                0
            }
        }
        16 | 17 => {
            let (dest, src, dst, p) = (arg(m, c, 1), arg(m, c, 2), arg(m, c, 3), arg(m, c, 4));
            let channels = state(|s| {
                let v = &s.voices[&this];
                let d = if dest == 0 {
                    v.sends.first().copied().unwrap_or(0)
                } else {
                    dest
                };
                (
                    v.effects.last().map_or(v.format.channels, |e| e.channels),
                    s.voices.get(&d).map(|v| v.format.channels),
                )
            });
            if src != channels.0 || Some(dst) != channels.1 || p == 0 {
                INVALID
            } else if SLOT == 16 {
                match floats(m, p, src * dst) {
                    Ok(x) => {
                        let set = arg(m, c, 5);
                        change(m, c, this, set, Change::Matrix(dest, x));
                        0
                    }
                    Err(e) => e,
                }
            } else {
                // Without SetOutputMatrix: XAudio2's default matrix.
                let matrix = state(|s| s.voices[&this].matrices.get(&dest).cloned())
                    .unwrap_or_else(|| ddaudio::mixer::default_matrix(src, dst));
                for (i, x) in matrix.into_iter().enumerate() {
                    m.set_u32(p + 4 * i as u32, x.to_bits());
                }
                0
            }
        }
        18 => {
            destroy(m, c, this);
            0
        }
        19 | 20 => {
            let (flags, set) = (arg(m, c, 1), arg(m, c, 2));
            if flags != 0 && !(SLOT == 20 && flags == 0x20) {
                INVALID
            } else {
                change(m, c, this, set, Change::Start(SLOT == 19));
                0
            }
        }
        21 => submit(m, this, arg(m, c, 1), arg(m, c, 2)),
        22 => {
            state(|s| {
                let v = s.voices.get_mut(&this).unwrap();
                let keep = usize::from(v.running && v.queue.front().is_some_and(|b| b.started));
                while v.queue.len() > keep {
                    let b = v.queue.remove(keep).unwrap();
                    v.flushed.push_back(b.context);
                }
                if keep == 0 {
                    v.played = 0;
                    v.fraction = 0.0;
                }
            });
            0
        }
        23 => {
            state(|s| {
                if let Some(b) = s.voices.get_mut(&this).unwrap().queue.back_mut() {
                    b.flags |= 0x40;
                }
            });
            0
        }
        24 => {
            let set = arg(m, c, 1);
            change(m, c, this, set, Change::ExitLoop);
            0
        }
        25 => {
            let p = arg(m, c, 1);
            state(|s| {
                let v = &s.voices[&this];
                m.set_u32(
                    p,
                    v.queue
                        .front()
                        .filter(|b| v.running && b.started)
                        .map_or(0, |b| b.context),
                );
                m.set_u32(p + 4, v.queue.len() as u32);
                m.set_u32(p + 8, v.played as u32);
                m.set_u32(p + 12, (v.played >> 32) as u32);
            });
            0
        }
        26 => {
            let ratio = f32::from_bits(arg(m, c, 1));
            if !ratio.is_finite() || ratio < 1.0 / 1024.0 || ratio > state(|s| s.voices[&this].max_ratio) {
                INVALID
            } else {
                let set = arg(m, c, 2);
                change(m, c, this, set, Change::Ratio(ratio));
                0
            }
        }
        27 => {
            m.set_u32(arg(m, c, 1), state(|s| s.voices[&this].ratio.to_bits()));
            0
        }
        28 => {
            let rate = arg(m, c, 1);
            state(|s| {
                let v = s.voices.get_mut(&this).unwrap();
                if !(1000..=200000).contains(&rate) || !v.queue.is_empty() || !v.flushed.is_empty() {
                    INVALID
                } else {
                    v.format.rate = rate;
                    0
                }
            })
        }
        _ => unreachable!(),
    };
    ret(m, c, cleanup);
}
