//! The oracle: renders the same graph on the host's XAudio2 2.9
//! (`xaudio2_9.dll`, shipped with Windows 10+) and on `ddaudio`, and
//! compares the mastering voice's input sample by sample.
//!
//! The host's mix is captured by a pass-through XAPO on the mastering
//! voice. It records each pass and then silences it, so nothing plays on
//! the speakers. The host renders in real time, so these tests need an
//! audio output device and take about a second each; they are ignored by
//! default:
//!
//!     cargo test -p ddaudio --test oracle -- --ignored --nocapture
//!
//! The game runs XAudio2 2.7; where 2.9 may differ is recorded in the L19
//! brief.
#![cfg(windows)]
#![allow(non_snake_case)]

use ddaudio::format::Format;
use ddaudio::mixer::{Kind, Mixer, NoGuestEffects, Node, Send};
use std::sync::{Arc, Mutex};
use windows::core::{implement, Interface, BOOL, GUID, PCWSTR};
use windows::Win32::Media::Audio::XAudio2::*;
use windows::Win32::Media::Audio::{AudioCategory_GameEffects, WAVEFORMATEX};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemAlloc, COINIT_MULTITHREADED};

const RATE: u32 = 48000;
const PASS: usize = 480;

// ---------------------------------------------------------------------
// Capture XAPO

#[implement(IXAPO, IXAPO27)]
struct Capture {
    data: Arc<Mutex<Vec<f32>>>,
    channels: Mutex<u32>,
}

impl IXAPO_Impl for Capture_Impl {
    fn GetRegistrationProperties(&self) -> windows::core::Result<*mut XAPO_REGISTRATION_PROPERTIES> {
        unsafe {
            let p = CoTaskMemAlloc(size_of::<XAPO_REGISTRATION_PROPERTIES>())
                as *mut XAPO_REGISTRATION_PROPERTIES;
            let mut r = XAPO_REGISTRATION_PROPERTIES::default();
            r.clsid = GUID::from_u128(0x6ddaa0d1_0000_4000_8000_000000000019);
            r.MajorVersion = 1;
            r.Flags = XAPO_FLAG_CHANNELS_MUST_MATCH
                | XAPO_FLAG_FRAMERATE_MUST_MATCH
                | XAPO_FLAG_BITSPERSAMPLE_MUST_MATCH
                | XAPO_FLAG_BUFFERCOUNT_MUST_MATCH
                | XAPO_FLAG_INPLACE_REQUIRED
                | XAPO_FLAG_INPLACE_SUPPORTED;
            r.MinInputBufferCount = 1;
            r.MaxInputBufferCount = 1;
            r.MinOutputBufferCount = 1;
            r.MaxOutputBufferCount = 1;
            p.write_unaligned(r);
            Ok(p)
        }
    }
    fn IsInputFormatSupported(
        &self,
        _: *const WAVEFORMATEX,
        _: *const WAVEFORMATEX,
        pp: *mut *mut WAVEFORMATEX,
    ) -> windows::core::Result<()> {
        if !pp.is_null() {
            unsafe { *pp = std::ptr::null_mut() };
        }
        Ok(())
    }
    fn IsOutputFormatSupported(
        &self,
        _: *const WAVEFORMATEX,
        _: *const WAVEFORMATEX,
        pp: *mut *mut WAVEFORMATEX,
    ) -> windows::core::Result<()> {
        if !pp.is_null() {
            unsafe { *pp = std::ptr::null_mut() };
        }
        Ok(())
    }
    fn Initialize(&self, _: *const core::ffi::c_void, _: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn Reset(&self) {}
    fn LockForProcess(
        &self,
        _: u32,
        input: *const XAPO_LOCKFORPROCESS_PARAMETERS,
        _: u32,
        _: *const XAPO_LOCKFORPROCESS_PARAMETERS,
    ) -> windows::core::Result<()> {
        unsafe {
            let f = (*input).pFormat;
            *self.channels.lock().unwrap() = std::ptr::addr_of!((*f).nChannels).read_unaligned() as u32;
        }
        Ok(())
    }
    fn UnlockForProcess(&self) {}
    fn Process(
        &self,
        _: u32,
        input: *const XAPO_PROCESS_BUFFER_PARAMETERS,
        _: u32,
        output: *mut XAPO_PROCESS_BUFFER_PARAMETERS,
        _: BOOL,
    ) {
        unsafe {
            let p = input.read_unaligned();
            let n = p.ValidFrameCount as usize * *self.channels.lock().unwrap() as usize;
            let buf = std::slice::from_raw_parts_mut(p.pBuffer as *mut f32, n);
            let mut d = self.data.lock().unwrap();
            let flags = p.BufferFlags;
            if flags == XAPO_BUFFER_VALID {
                d.extend_from_slice(buf);
            } else {
                d.extend(std::iter::repeat_n(0.0, n));
            }
            buf.fill(0.0);
            let mut o = output.read_unaligned();
            o.BufferFlags = XAPO_BUFFER_SILENT;
            o.ValidFrameCount = p.ValidFrameCount;
            output.write_unaligned(o);
        }
    }
    fn CalcInputFrames(&self, n: u32) -> u32 {
        n
    }
    fn CalcOutputFrames(&self, n: u32) -> u32 {
        n
    }
}

// ---------------------------------------------------------------------
// Host engines: XAudio2 2.7 (the game's version, the DirectX June 2010
// redistributable's COM server XAudio2_7.dll) and 2.9 (xaudio2_9.dll).
// Their voice interfaces share one layout; the engine interfaces differ
// (2.7 has GetDeviceCount, GetDeviceDetails and Initialize at slots 3..5,
// and a device index instead of a device id string).

/// IXAudio2, version 2.7 (IID at the game's 0x0142AC40).
#[windows::core::interface("8bcf1f58-9fe7-4583-8ac6-e2adc465c8bb")]
unsafe trait IXAudio27: windows::core::IUnknown {
    unsafe fn GetDeviceCount(&self, count: *mut u32) -> windows::core::HRESULT;
    unsafe fn GetDeviceDetails(&self, index: u32, details: *mut u8) -> windows::core::HRESULT;
    unsafe fn Initialize(&self, flags: u32, processor: u32) -> windows::core::HRESULT;
    unsafe fn RegisterForCallbacks(&self, cb: *mut core::ffi::c_void) -> windows::core::HRESULT;
    unsafe fn UnregisterForCallbacks(&self, cb: *mut core::ffi::c_void);
    unsafe fn CreateSourceVoice(
        &self,
        voice: *mut *mut core::ffi::c_void,
        format: *const WAVEFORMATEX,
        flags: u32,
        max_ratio: f32,
        callback: *mut core::ffi::c_void,
        sends: *const XAUDIO2_VOICE_SENDS,
        effects: *const XAUDIO2_EFFECT_CHAIN,
    ) -> windows::core::HRESULT;
    unsafe fn CreateSubmixVoice(
        &self,
        voice: *mut *mut core::ffi::c_void,
        channels: u32,
        rate: u32,
        flags: u32,
        stage: u32,
        sends: *const XAUDIO2_VOICE_SENDS,
        effects: *const XAUDIO2_EFFECT_CHAIN,
    ) -> windows::core::HRESULT;
    unsafe fn CreateMasteringVoice(
        &self,
        voice: *mut *mut core::ffi::c_void,
        channels: u32,
        rate: u32,
        flags: u32,
        device: u32,
        effects: *const XAUDIO2_EFFECT_CHAIN,
    ) -> windows::core::HRESULT;
    unsafe fn StartEngine(&self) -> windows::core::HRESULT;
    unsafe fn StopEngine(&self);
}

/// IXAPO with the 2.7 IID (game rdata 0x0142AF28); same methods as 2.9's.
#[windows::core::interface("a90bc001-e897-e897-55e4-9e4700000000")]
unsafe trait IXAPO27: windows::core::IUnknown {
    unsafe fn GetRegistrationProperties(
        &self,
        pp: *mut *mut XAPO_REGISTRATION_PROPERTIES,
    ) -> windows::core::HRESULT;
    unsafe fn IsInputFormatSupported(
        &self,
        a: *const WAVEFORMATEX,
        b: *const WAVEFORMATEX,
        pp: *mut *mut WAVEFORMATEX,
    ) -> windows::core::HRESULT;
    unsafe fn IsOutputFormatSupported(
        &self,
        a: *const WAVEFORMATEX,
        b: *const WAVEFORMATEX,
        pp: *mut *mut WAVEFORMATEX,
    ) -> windows::core::HRESULT;
    unsafe fn Initialize(&self, data: *const core::ffi::c_void, size: u32) -> windows::core::HRESULT;
    unsafe fn Reset(&self);
    unsafe fn LockForProcess(
        &self,
        ic: u32,
        input: *const XAPO_LOCKFORPROCESS_PARAMETERS,
        oc: u32,
        output: *const XAPO_LOCKFORPROCESS_PARAMETERS,
    ) -> windows::core::HRESULT;
    unsafe fn UnlockForProcess(&self);
    unsafe fn Process(
        &self,
        ic: u32,
        input: *const XAPO_PROCESS_BUFFER_PARAMETERS,
        oc: u32,
        output: *mut XAPO_PROCESS_BUFFER_PARAMETERS,
        enabled: BOOL,
    );
    unsafe fn CalcInputFrames(&self, n: u32) -> u32;
    unsafe fn CalcOutputFrames(&self, n: u32) -> u32;
}

impl IXAPO27_Impl for Capture_Impl {
    unsafe fn GetRegistrationProperties(
        &self,
        pp: *mut *mut XAPO_REGISTRATION_PROPERTIES,
    ) -> windows::core::HRESULT {
        match IXAPO_Impl::GetRegistrationProperties(self) {
            Ok(p) => {
                unsafe { *pp = p };
                windows::core::HRESULT(0)
            }
            Err(e) => e.code(),
        }
    }
    unsafe fn IsInputFormatSupported(
        &self,
        a: *const WAVEFORMATEX,
        b: *const WAVEFORMATEX,
        pp: *mut *mut WAVEFORMATEX,
    ) -> windows::core::HRESULT {
        IXAPO_Impl::IsInputFormatSupported(self, a, b, pp).into()
    }
    unsafe fn IsOutputFormatSupported(
        &self,
        a: *const WAVEFORMATEX,
        b: *const WAVEFORMATEX,
        pp: *mut *mut WAVEFORMATEX,
    ) -> windows::core::HRESULT {
        IXAPO_Impl::IsOutputFormatSupported(self, a, b, pp).into()
    }
    unsafe fn Initialize(&self, data: *const core::ffi::c_void, size: u32) -> windows::core::HRESULT {
        IXAPO_Impl::Initialize(self, data, size).into()
    }
    unsafe fn Reset(&self) {}
    unsafe fn LockForProcess(
        &self,
        ic: u32,
        input: *const XAPO_LOCKFORPROCESS_PARAMETERS,
        oc: u32,
        output: *const XAPO_LOCKFORPROCESS_PARAMETERS,
    ) -> windows::core::HRESULT {
        IXAPO_Impl::LockForProcess(self, ic, input, oc, output).into()
    }
    unsafe fn UnlockForProcess(&self) {}
    unsafe fn Process(
        &self,
        ic: u32,
        input: *const XAPO_PROCESS_BUFFER_PARAMETERS,
        oc: u32,
        output: *mut XAPO_PROCESS_BUFFER_PARAMETERS,
        enabled: BOOL,
    ) {
        IXAPO_Impl::Process(self, ic, input, oc, output, enabled)
    }
    unsafe fn CalcInputFrames(&self, n: u32) -> u32 {
        n
    }
    unsafe fn CalcOutputFrames(&self, n: u32) -> u32 {
        n
    }
}

/// The XAudio2 versions the oracle renders on.
const VERSIONS: [u32; 2] = [7, 9];

enum Engine {
    V27(IXAudio27),
    V29(IXAudio2),
}

fn voice_from<T: Interface>(raw: *mut core::ffi::c_void) -> T {
    assert!(!raw.is_null());
    unsafe { T::from_raw(raw) }
}

fn chain_ptr(chain: Option<&XAUDIO2_EFFECT_CHAIN>) -> *const XAUDIO2_EFFECT_CHAIN {
    chain.map_or(std::ptr::null(), |c| c as *const _)
}

impl Engine {
    fn new(version: u32) -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            match version {
                7 => {
                    // CLSID_XAudio2 2.7, the game's 0x0142AC50
                    let clsid = GUID::from_u128(0x5a508685_a254_4fba_9b82_9a24b00306af);
                    let e: IXAudio27 = windows::Win32::System::Com::CoCreateInstance(
                        &clsid,
                        None,
                        windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                    )
                    .expect("XAudio2 2.7 (DirectX June 2010 redistributable)");
                    // XAUDIO2_DEFAULT_PROCESSOR in 2.7 is XAUDIO2_ANY_PROCESSOR
                    e.Initialize(0, u32::MAX).ok().unwrap();
                    e.StopEngine();
                    Engine::V27(e)
                }
                _ => {
                    let mut raw = std::ptr::null_mut();
                    // NTDDI_WIN10 = 0x0A000000
                    xaudio2_9()(&mut raw, 0, 1, 0x0A00_0000)
                        .ok()
                        .expect("XAudio2 2.9");
                    let e = IXAudio2::from_raw(raw);
                    e.StopEngine();
                    Engine::V29(e)
                }
            }
        }
    }
    fn mastering(&self, channels: u32, chain: Option<&XAUDIO2_EFFECT_CHAIN>) -> IXAudio2MasteringVoice {
        let mut raw = std::ptr::null_mut();
        unsafe {
            match self {
                Engine::V27(e) => e
                    .CreateMasteringVoice(&mut raw, channels, RATE, 0, 0, chain_ptr(chain))
                    .ok()
                    .expect("a mastering voice (needs an audio output device)"),
                Engine::V29(e) => {
                    let mut v = None;
                    e.CreateMasteringVoice(
                        &mut v,
                        channels,
                        RATE,
                        0,
                        PCWSTR::null(),
                        chain.map(|c| c as *const _),
                        AudioCategory_GameEffects,
                    )
                    .expect("a mastering voice (needs an audio output device)");
                    return v.unwrap();
                }
            }
        }
        voice_from(raw)
    }
    fn submix(
        &self,
        channels: u32,
        stage: u32,
        sends: Option<&XAUDIO2_VOICE_SENDS>,
        chain: Option<&XAUDIO2_EFFECT_CHAIN>,
    ) -> IXAudio2Voice {
        let mut raw = std::ptr::null_mut();
        let sends = sends.map_or(std::ptr::null(), |s| s as *const _);
        unsafe {
            match self {
                Engine::V27(e) => e
                    .CreateSubmixVoice(&mut raw, channels, RATE, 0, stage, sends, chain_ptr(chain))
                    .ok(),
                Engine::V29(e) => {
                    let mut v = None;
                    let r = e.CreateSubmixVoice(
                        &mut v,
                        channels,
                        RATE,
                        0,
                        stage,
                        Some(sends),
                        Some(chain_ptr(chain)),
                    );
                    if let Some(v) = v {
                        raw = v.into_raw();
                    }
                    r
                }
            }
            .unwrap();
        }
        voice_from(raw)
    }
    fn source(
        &self,
        format: &[u8],
        max_ratio: f32,
        sends: Option<&XAUDIO2_VOICE_SENDS>,
    ) -> IXAudio2SourceVoice {
        let mut raw = std::ptr::null_mut();
        let sends = sends.map_or(std::ptr::null(), |s| s as *const _);
        let f = format.as_ptr() as *const WAVEFORMATEX;
        unsafe {
            match self {
                Engine::V27(e) => e
                    .CreateSourceVoice(
                        &mut raw,
                        f,
                        0,
                        max_ratio,
                        std::ptr::null_mut(),
                        sends,
                        std::ptr::null(),
                    )
                    .ok(),
                Engine::V29(e) => {
                    let mut v = None;
                    let r = e.CreateSourceVoice(
                        &mut v,
                        f,
                        0,
                        max_ratio,
                        None::<&IXAudio2VoiceCallback>,
                        Some(sends),
                        None,
                    );
                    if let Some(v) = v {
                        raw = v.into_raw();
                    }
                    r
                }
            }
            .unwrap();
        }
        voice_from(raw)
    }
    /// AudioReverb of this version: CLSID_AudioReverb 2.7 (the game's
    /// 0x0142AC70) through COM, or xaudio2_9.dll's CreateAudioReverb.
    fn reverb(&self) -> windows::core::IUnknown {
        match self {
            Engine::V27(_) => unsafe {
                let clsid = GUID::from_u128(0x6a93130e_1d53_41d1_a9cf_e758800bb179);
                windows::Win32::System::Com::CoCreateInstance(
                    &clsid,
                    None,
                    windows::Win32::System::Com::CLSCTX_INPROC_SERVER,
                )
                .expect("AudioReverb 2.7")
            },
            Engine::V29(_) => create_reverb_2_9(),
        }
    }
    /// The reverb parameter block for this version from the game's 2.7 one.
    fn reverb_parameters(&self, p: &[u8]) -> Vec<u8> {
        match self {
            Engine::V27(_) => p.to_vec(),
            Engine::V29(_) => reverb_params_2_9(p),
        }
    }
    fn start(&self) {
        unsafe {
            match self {
                Engine::V27(e) => e.StartEngine().ok().unwrap(),
                Engine::V29(e) => e.StartEngine().unwrap(),
            }
        }
    }
    fn stop(&self) {
        unsafe {
            match self {
                Engine::V27(e) => e.StopEngine(),
                Engine::V29(e) => e.StopEngine(),
            }
        }
    }
}

// ---------------------------------------------------------------------
// Graph description shared by both renderers

#[derive(Clone)]
enum Dest {
    Master,
    Submix(usize),
}

#[derive(Clone)]
struct SubmixSpec {
    /// AudioReverb parameters, 2.7 layout (52 bytes), as the only effect.
    reverb: Option<Vec<u8>>,
    channels: u32,
    stage: u32,
    dest: Dest,
    matrix: Option<Vec<f32>>,
    volume: f32,
}

#[derive(Clone)]
struct SourceSpec {
    format_bytes: Vec<u8>,
    data: Vec<u8>,
    ratio: f32,
    dests: Vec<(Dest, Option<Vec<f32>>)>,
    volume: f32,
    channel_volumes: Vec<f32>,
}

#[derive(Clone)]
struct Graph {
    master_channels: u32,
    submixes: Vec<SubmixSpec>,
    sources: Vec<SourceSpec>,
}

fn wave(format: &Format) -> Vec<u8> {
    let mut b = vec![];
    b.extend((format.tag).to_le_bytes());
    b.extend((format.channels as u16).to_le_bytes());
    b.extend(format.rate.to_le_bytes());
    b.extend((format.rate * format.block_align).to_le_bytes());
    b.extend((format.block_align as u16).to_le_bytes());
    b.extend((format.bits as u16).to_le_bytes());
    b.extend(0u16.to_le_bytes());
    b
}

// ---------------------------------------------------------------------
// Host renderer

type CreateFn =
    unsafe extern "system" fn(*mut *mut core::ffi::c_void, u32, u32, u32) -> windows::core::HRESULT;

fn xaudio2_9() -> CreateFn {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    unsafe {
        let lib = LoadLibraryW(windows::core::w!("xaudio2_9.dll")).expect("xaudio2_9.dll");
        let f = GetProcAddress(lib, windows::core::s!("XAudio2CreateWithVersionInfo"))
            .expect("XAudio2CreateWithVersionInfo");
        std::mem::transmute(f)
    }
}

/// AudioReverb from xaudio2_9.dll (`CreateAudioReverb`).
fn create_reverb_2_9() -> windows::core::IUnknown {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    type Create = unsafe extern "system" fn(*mut *mut core::ffi::c_void) -> windows::core::HRESULT;
    unsafe {
        let lib = LoadLibraryW(windows::core::w!("xaudio2_9.dll")).unwrap();
        let f: Create =
            std::mem::transmute(GetProcAddress(lib, windows::core::s!("CreateAudioReverb")).unwrap());
        let mut raw = std::ptr::null_mut();
        f(&mut raw).ok().unwrap();
        windows::core::IUnknown::from_raw(raw)
    }
}

/// The 2.7 parameter block (52 bytes) in 2.9's layout: SideDelay (unused
/// below 7.1; the 2.7 RearDelay's value, within its [0, 5] range) after
/// RearDelay, DisableLateField FALSE at the end.
fn reverb_params_2_9(p: &[u8]) -> Vec<u8> {
    let mut q = p[..10].to_vec();
    q.push(p[9].min(5));
    q.extend_from_slice(&p[10..52]);
    q.extend(0u32.to_le_bytes());
    q
}

fn as_voice<T: Interface>(v: &T) -> IXAudio2Voice {
    unsafe { IXAudio2Voice::from_raw(v.as_raw()) }
}

fn host_render(version: u32, g: &Graph, passes: usize) -> Vec<f32> {
    host_render_with(version, g, passes, &[])
}

/// A level change applied while the host runs: after `delay` of real time,
/// `apply` gets the source voices and the submix voices.
struct HostChange {
    delay: std::time::Duration,
    apply: Box<dyn Fn(&[IXAudio2SourceVoice], &[IXAudio2Voice])>,
}

fn host_render_with(version: u32, g: &Graph, passes: usize, changes: &[HostChange]) -> Vec<f32> {
    unsafe {
        let xa = Engine::new(version);
        let data = Arc::new(Mutex::new(Vec::new()));
        let capture: IXAPO = Capture {
            data: data.clone(),
            channels: Mutex::new(0),
        }
        .into();
        let unknown: windows::core::IUnknown = capture.cast().unwrap();
        let mut fx = [XAUDIO2_EFFECT_DESCRIPTOR {
            pEffect: std::mem::ManuallyDrop::new(Some(unknown.clone())),
            InitialState: true.into(),
            OutputChannels: g.master_channels,
        }];
        let chain = XAUDIO2_EFFECT_CHAIN {
            EffectCount: 1,
            pEffectDescriptors: fx.as_mut_ptr(),
        };
        let master = xa.mastering(g.master_channels, Some(&chain));
        let master_v = as_voice(&master);
        let mut subs: Vec<IXAudio2Voice> = vec![];
        let dest_voice = |d: &Dest, subs: &[IXAudio2Voice]| match d {
            Dest::Master => master_v.clone(),
            Dest::Submix(i) => subs[*i].clone(),
        };
        for s in &g.submixes {
            let mut sends = [XAUDIO2_SEND_DESCRIPTOR {
                Flags: 0,
                pOutputVoice: std::mem::ManuallyDrop::new(Some(dest_voice(&s.dest, &subs))),
            }];
            let list = XAUDIO2_VOICE_SENDS {
                SendCount: 1,
                pSends: sends.as_mut_ptr(),
            };
            let reverb = s.reverb.as_ref().map(|_| xa.reverb());
            let mut fx = reverb
                .iter()
                .map(|r| XAUDIO2_EFFECT_DESCRIPTOR {
                    pEffect: std::mem::ManuallyDrop::new(Some(r.clone())),
                    InitialState: true.into(),
                    OutputChannels: s.channels,
                })
                .collect::<Vec<_>>();
            let chain = XAUDIO2_EFFECT_CHAIN {
                EffectCount: fx.len() as u32,
                pEffectDescriptors: fx.as_mut_ptr(),
            };
            let v = xa.submix(
                s.channels,
                s.stage,
                Some(&list),
                (!fx.is_empty()).then_some(&chain),
            );
            let d = dest_voice(&s.dest, &subs);
            if let Some(m) = &s.matrix {
                let dc = m.len() as u32 / s.channels;
                v.SetOutputMatrix(&d, s.channels, dc, m.as_ptr(), 0).unwrap();
            }
            v.SetVolume(s.volume, 0).unwrap();
            if let Some(p) = &s.reverb {
                let p = xa.reverb_parameters(p);
                v.SetEffectParameters(0, p.as_ptr() as *const _, p.len() as u32, 0)
                    .unwrap();
            }
            subs.push(v);
        }
        let mut sources = vec![];
        for s in &g.sources {
            let format = Format::parse(&s.format_bytes).unwrap();
            let mut sends: Vec<XAUDIO2_SEND_DESCRIPTOR> = s
                .dests
                .iter()
                .map(|(d, _)| XAUDIO2_SEND_DESCRIPTOR {
                    Flags: 0,
                    pOutputVoice: std::mem::ManuallyDrop::new(Some(dest_voice(d, &subs))),
                })
                .collect();
            let list = XAUDIO2_VOICE_SENDS {
                SendCount: sends.len() as u32,
                pSends: sends.as_mut_ptr(),
            };
            let v = xa.source(&s.format_bytes, 4.0, Some(&list));
            let vv = as_voice(&v);
            for (d, m) in &s.dests {
                if let Some(m) = m {
                    let dc = m.len() as u32 / format.channels;
                    vv.SetOutputMatrix(&dest_voice(d, &subs), format.channels, dc, m.as_ptr(), 0)
                        .unwrap();
                }
            }
            vv.SetVolume(s.volume, 0).unwrap();
            if !s.channel_volumes.is_empty() {
                vv.SetChannelVolumes(&s.channel_volumes, 0).unwrap();
            }
            v.SetFrequencyRatio(s.ratio, 0).unwrap();
            let buffer = XAUDIO2_BUFFER {
                Flags: XAUDIO2_END_OF_STREAM,
                AudioBytes: s.data.len() as u32,
                pAudioData: s.data.as_ptr(),
                ..Default::default()
            };
            v.SubmitSourceBuffer(&buffer, None).unwrap();
            v.Start(0, 0).unwrap();
            sources.push(v);
        }
        data.lock().unwrap().clear();
        xa.start();
        for c in changes {
            std::thread::sleep(c.delay);
            (c.apply)(&sources, &subs);
        }
        let want = passes * PASS * g.master_channels as usize;
        while data.lock().unwrap().len() < want {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        xa.stop();
        for v in sources {
            v.DestroyVoice();
        }
        for v in subs.into_iter().rev() {
            v.DestroyVoice();
        }
        master.DestroyVoice();
        drop(xa);
        let mut d = data.lock().unwrap().clone();
        d.truncate(want);
        d
    }
}

// ---------------------------------------------------------------------
// ddaudio renderer, fed the way ddruntime's clock consumes buffers:
// floor(fraction + rate * ratio / 100) frames per pass.

fn mixer_render(g: &Graph, passes: usize) -> Vec<f32> {
    mixer_render_with(g, passes, &[])
}

/// Renders `g`, switching to `changes[i].1` from pass `changes[i].0` on.
fn mixer_render_with(g: &Graph, passes: usize, changes: &[(usize, Graph)]) -> Vec<f32> {
    let mut mixer = Mixer::new();
    let master = 1u32;
    let sub_id = |i: usize| 100 + i as u32;
    let src_id = |i: usize| 1000 + i as u32;
    let dest_id = |d: &Dest| match d {
        Dest::Master => master,
        Dest::Submix(i) => sub_id(*i),
    };
    let formats: Vec<Format> = g
        .sources
        .iter()
        .map(|s| Format::parse(&s.format_bytes).unwrap())
        .collect();
    let mut cursor = vec![0u64; g.sources.len()];
    let mut fraction = vec![0f64; g.sources.len()];
    let mut out = vec![];
    for pass in 0..passes {
        let g = changes
            .iter()
            .rev()
            .find(|(p, _)| *p <= pass)
            .map_or(g, |(_, g)| g);
        for (i, s) in g.sources.iter().enumerate() {
            let f = &formats[i];
            let x = fraction[i] + f.rate as f64 * s.ratio as f64 / 100.0;
            fraction[i] = x.fract();
            let total = f.frames(s.data.len() as u64);
            let n = (x.floor() as u64).min(total - cursor[i]);
            let mut frames = vec![];
            f.decode(&s.data, cursor[i], n, &mut frames);
            cursor[i] += n;
            mixer.feed(src_id(i), f.channels, &frames);
        }
        let mut nodes = vec![Node {
            id: master,
            kind: Kind::Mastering,
            channels: g.master_channels,
            rate: RATE,
            stage: 0,
            running: true,
            ratio: 1.0,
            volume: 1.0,
            channel_volumes: &[],
            effects: vec![],
            sends: vec![],
        }];
        for (i, s) in g.submixes.iter().enumerate() {
            nodes.push(Node {
                id: sub_id(i),
                kind: Kind::Submix,
                channels: s.channels,
                rate: RATE,
                stage: s.stage,
                running: true,
                ratio: 1.0,
                volume: s.volume,
                channel_volumes: &[],
                effects: s
                    .reverb
                    .iter()
                    .map(|p| ddaudio::Effect {
                        kind: ddaudio::EffectKind::Reverb(p),
                        enabled: true,
                        channels: s.channels,
                    })
                    .collect(),
                sends: vec![Send {
                    dest: dest_id(&s.dest),
                    matrix: s.matrix.as_deref(),
                }],
            });
        }
        for (i, s) in g.sources.iter().enumerate() {
            nodes.push(Node {
                id: src_id(i),
                kind: Kind::Source,
                channels: formats[i].channels,
                rate: formats[i].rate,
                stage: 0,
                running: true,
                ratio: s.ratio,
                volume: s.volume,
                channel_volumes: &s.channel_volumes,
                effects: vec![],
                sends: s
                    .dests
                    .iter()
                    .map(|(d, m)| Send {
                        dest: dest_id(d),
                        matrix: m.as_deref(),
                    })
                    .collect(),
            });
        }
        out.extend(mixer.render(&nodes, &mut NoGuestEffects).unwrap());
    }
    out
}

// ---------------------------------------------------------------------
// Signals and comparison

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

fn pcm16_signal(channels: u32, frames: usize, seed: u64) -> Vec<u8> {
    let mut r = Lcg(seed);
    let mut b = vec![];
    for i in 0..frames {
        for c in 0..channels {
            let t = i as f64 / RATE as f64;
            let tone = (2.0 * std::f64::consts::PI * (220.0 * (c + 1) as f64) * t).sin() * 12000.0;
            let noise = (r.next() % 8000) as f64 - 4000.0;
            b.extend(((tone + noise) as i16).to_le_bytes());
        }
    }
    b
}

/// A tone-plus-noise signal encoded as mono MS ADPCM blocks (70 bytes,
/// 128 samples) with the standard coefficients: a greedy encoder picks the
/// coefficient pair with the smallest first-step error and, per sample, the
/// nibble closest to the target. Bounded step sizes, like real encoder output.
fn adpcm_signal(blocks: usize, seed: u64) -> Vec<u8> {
    const ADAPTATION: [i32; 16] = [
        230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
    ];
    const COEF: [(i32, i32); 7] = [
        (256, 0),
        (512, -256),
        (0, 0),
        (192, 64),
        (240, 0),
        (460, -208),
        (392, -232),
    ];
    let mut r = Lcg(seed);
    let pcm: Vec<i32> = (0..blocks * 128)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            ((2.0 * std::f64::consts::PI * 330.0 * t).sin() * 9000.0 + (r.next() % 6000) as f64 - 3000.0)
                as i32
        })
        .collect();
    let mut b = vec![];
    for blk in pcm.chunks(128) {
        let encode = |p: usize, out: &mut Vec<u8>| -> i64 {
            let (c1, c2) = COEF[p];
            let mut delta = 16.max((blk[2] - blk[1]).abs() / 4);
            out.push(p as u8);
            out.extend((delta as i16).to_le_bytes());
            out.extend((blk[1] as i16).to_le_bytes());
            out.extend((blk[0] as i16).to_le_bytes());
            let (mut s1, mut s2, mut err) = (blk[1], blk[0], 0i64);
            let mut nibbles = vec![];
            for &x in &blk[2..] {
                let predict = (s1 * c1 + s2 * c2) >> 8;
                let n = ((x - predict) as f64 / delta as f64).round().clamp(-8.0, 7.0) as i32;
                let sample = (predict + n * delta).clamp(-32768, 32767);
                err += ((x - sample) as i64).pow(2);
                s2 = s1;
                s1 = sample;
                delta = ((ADAPTATION[(n & 15) as usize] * delta) >> 8).max(16);
                nibbles.push((n & 15) as u8);
            }
            for pair in nibbles.chunks(2) {
                out.push(pair[0] << 4 | pair[1]);
            }
            err
        };
        let best = (0..7).min_by_key(|&p| encode(p, &mut vec![])).unwrap();
        encode(best, &mut b);
    }
    b
}

fn game_adpcm_format() -> Vec<u8> {
    // The ADPCMWAVEFORMAT 0x00DCE1F0 builds on its stack.
    let mut b = vec![];
    for w in [2u16, 1] {
        b.extend(w.to_le_bytes());
    }
    b.extend(48000u32.to_le_bytes());
    b.extend(0x668au32.to_le_bytes());
    for w in [70u16, 4, 32, 128, 7] {
        b.extend(w.to_le_bytes());
    }
    for (a, c) in [
        (256, 0),
        (512, -256),
        (0, 0),
        (192, 64),
        (240, 0),
        (460, -208),
        (392, -232),
    ] {
        b.extend((a as i16).to_le_bytes());
        b.extend((c as i16).to_le_bytes());
    }
    b
}

struct Compare {
    lag: isize,
    max_error: f32,
    ser_db: f64,
}

/// Best alignment of `ours` against `host` within +-8 frames (`ours`
/// delayed by `lag` frames), its maximum absolute error and its
/// signal-to-error ratio, over the frames both cover.
fn compare(host: &[f32], ours: &[f32], channels: usize) -> Compare {
    let frames = host.len() / channels;
    let mut best: Option<(f64, Compare)> = None;
    for lag in -8isize..=8 {
        let (mut sig, mut err, mut max) = (0f64, 0f64, 0f32);
        for f in 0..frames {
            let g = f as isize + lag;
            if g < 0 || g as usize >= frames {
                continue;
            }
            for c in 0..channels {
                let h = host[f * channels + c];
                let o = ours[g as usize * channels + c];
                sig += (h as f64).powi(2);
                err += ((h - o) as f64).powi(2);
                max = max.max((h - o).abs());
            }
        }
        let ser_db = if err == 0.0 { f64::INFINITY } else { 10.0 * (sig / err).log10() };
        if best.as_ref().is_none_or(|(e, _)| err < *e) {
            best = Some((err, Compare { lag, max_error: max, ser_db }));
        }
    }
    best.unwrap().1
}

/// Renders `g` on each host version and on ddaudio. Returns the worst
/// comparison (largest error, lowest SER; lag 99 when the versions differ).
fn run(name: &str, g: &Graph, passes: usize) -> Compare {
    run_on(name, g, passes, &VERSIONS)
}

fn run_on(name: &str, g: &Graph, passes: usize, versions: &[u32]) -> Compare {
    let ours = mixer_render(g, passes);
    let mut worst: Option<Compare> = None;
    for &version in versions {
        let host = host_render(version, g, passes);
        let energy: f64 = host.iter().map(|x| (*x as f64).powi(2)).sum();
        assert!(energy > 1e-6, "{name}: XAudio2 2.{version} rendered silence");
        let c = compare(&host, &ours, g.master_channels as usize);
        println!(
            "{name} vs 2.{version}: lag {} frames, max |error| {:e}, SER {:.1} dB",
            c.lag, c.max_error, c.ser_db
        );
        worst = Some(match worst {
            None => c,
            Some(w) => Compare {
                lag: if w.lag == c.lag { c.lag } else { 99 },
                max_error: w.max_error.max(c.max_error),
                ser_db: w.ser_db.min(c.ser_db),
            },
        });
    }
    worst.unwrap()
}

fn mono_source(
    format: Vec<u8>,
    data: Vec<u8>,
    ratio: f32,
    dest: Dest,
    matrix: Option<Vec<f32>>,
) -> SourceSpec {
    SourceSpec {
        format_bytes: format,
        data,
        ratio,
        dests: vec![(dest, matrix)],
        volume: 1.0,
        channel_volumes: vec![],
    }
}

#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn pcm16_unit_ratio_is_exact() {
    let g = Graph {
        master_channels: 2,
        submixes: vec![],
        sources: vec![mono_source(
            wave(&Format::pcm16(1, RATE)),
            pcm16_signal(1, 48000, 1),
            1.0,
            Dest::Master,
            Some(vec![1.0, 1.0]),
        )],
    };
    let c = run("pcm16 mono, ratio 1", &g, 60);
    assert_eq!((c.lag, c.max_error), (0, 0.0));
}

#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn adpcm_unit_ratio_is_exact() {
    let g = Graph {
        master_channels: 2,
        submixes: vec![],
        sources: vec![mono_source(
            game_adpcm_format(),
            adpcm_signal(300, 2),
            1.0,
            Dest::Master,
            Some(vec![1.0, 0.0]),
        )],
    };
    let c = run("game MS ADPCM, ratio 1", &g, 60);
    assert_eq!((c.lag, c.max_error), (0, 0.0));
}

/// Random nibbles drive the step size past 32 bits; the host wraps it
/// (two's complement), which only `wrapping_mul` on i32 reproduces.
#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn adpcm_step_overflow_matches_the_host() {
    let mut r = Lcg(9);
    let mut data = vec![];
    for _ in 0..300 {
        data.push((r.next() % 7) as u8);
        data.extend((16 + (r.next() % 600) as i16).to_le_bytes());
        data.extend(((r.next() % 20000) as i16 - 10000).to_le_bytes());
        data.extend(((r.next() % 20000) as i16 - 10000).to_le_bytes());
        for _ in 0..63 {
            data.push(r.next() as u8);
        }
    }
    let g = Graph {
        master_channels: 2,
        submixes: vec![],
        sources: vec![mono_source(
            game_adpcm_format(),
            data,
            1.0,
            Dest::Master,
            Some(vec![1.0, 0.0]),
        )],
    };
    let c = run("MS ADPCM, overflowing step sizes", &g, 60);
    assert_eq!((c.lag, c.max_error), (0, 0.0));
}

/// XAudio2's resampler is undocumented; the brief allowed a tolerance, but
/// 2.9's turns out to be linear interpolation that ddaudio reproduces
/// bit for bit (frequency ratios as SetFrequencyRatio at 0x00DC6D8F sets
/// them, up to the game's maximum of 2).
#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn resampling_matches_the_host() {
    for ratio in [0.5f32, 0.75, 0.9439, 1.0595, 1.5, 2.0] {
        let g = Graph {
            master_channels: 2,
            submixes: vec![],
            sources: vec![mono_source(
                wave(&Format::pcm16(1, RATE)),
                pcm16_signal(1, 96000, 3),
                ratio,
                Dest::Master,
                Some(vec![1.0, 1.0]),
            )],
        };
        let c = run(&format!("pcm16 mono, ratio {ratio}"), &g, 40);
        assert_eq!((c.lag, c.max_error), (0, 0.0), "ratio {ratio}");
    }
}

/// The host's default send matrices (`GetOutputMatrix` without a prior
/// `SetOutputMatrix`), source channels 1/2/6 into submix voices of 1/2/6
/// channels and into the stereo mastering voice.
fn host_default_matrices(version: u32) -> Vec<((u32, u32, bool), Vec<f32>)> {
    unsafe {
        let xa = Engine::new(version);
        let master = xa.mastering(2, None);
        let mut out = vec![];
        for dst in [1u32, 2, 6, 0] {
            let dest: IXAudio2Voice = if dst == 0 {
                as_voice(&master)
            } else {
                xa.submix(dst, 0, None, None)
            };
            let dc = if dst == 0 { 2 } else { dst };
            for src in [1u32, 2, 6] {
                let mut sends = [XAUDIO2_SEND_DESCRIPTOR {
                    Flags: 0,
                    pOutputVoice: std::mem::ManuallyDrop::new(Some(dest.clone())),
                }];
                let list = XAUDIO2_VOICE_SENDS {
                    SendCount: 1,
                    pSends: sends.as_mut_ptr(),
                };
                let v = xa.source(&wave(&Format::pcm16(src, RATE)), 2.0, Some(&list));
                let mut m = vec![0f32; (src * dc) as usize];
                let voice = as_voice(&v);
                let vt = Interface::vtable(&voice);
                (vt.GetOutputMatrix)(voice.as_raw(), dest.as_raw(), src, dc, m.as_mut_ptr());
                out.push(((src, dc, dst == 0), m));
                v.DestroyVoice();
            }
            if dst != 0 {
                dest.DestroyVoice();
            }
        }
        master.DestroyVoice();
        out
    }
}

#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn default_matrices_match_the_host() {
    // 2.9's 6 -> 2 default: front at full scale, centre and back at -3 dB
    // (10^(-3/20) as f32), no LFE.
    const M3DB: f32 = 0.70794576;
    let v29_6_2 = [1.0, 0.0, M3DB, 0.0, M3DB, 0.0, 0.0, 1.0, M3DB, 0.0, 0.0, M3DB];
    for version in VERSIONS {
        for ((src, dst, master), m) in host_default_matrices(version) {
            let kind = if master { " (mastering)" } else { "" };
            println!("2.{version} {src} -> {dst}{kind}: {m:?}");
            if version == 9 && (src, dst) == (6, 2) {
                assert_eq!(m, v29_6_2, "2.9 6 -> 2{kind}");
            } else {
                assert_eq!(
                    m,
                    ddaudio::mixer::default_matrix(src, dst),
                    "2.{version} {src} -> {dst}{kind}"
                );
            }
        }
    }
    // And they are what the host mixes with: 1, 2 and 6 channel sources
    // into the stereo mastering voice, and 6 channels through a mono and a
    // 6-channel submix, all without SetOutputMatrix.
    let src = |ch: u32, seed, dest| SourceSpec {
        format_bytes: wave(&Format::pcm16(ch, RATE)),
        data: pcm16_signal(ch, 48000, seed),
        ratio: 1.0,
        dests: vec![(dest, None)],
        volume: 0.25,
        channel_volumes: vec![],
    };
    let sub = |channels, stage| SubmixSpec {
        reverb: None,
        channels,
        stage,
        dest: Dest::Master,
        matrix: None,
        volume: 1.0,
    };
    let g = Graph {
        master_channels: 2,
        submixes: vec![sub(1, 0), sub(6, 1)],
        sources: vec![
            src(1, 11, Dest::Master),
            src(2, 12, Dest::Master),
            src(6, 13, Dest::Master),
            src(6, 14, Dest::Submix(0)),
            src(2, 15, Dest::Submix(1)),
        ],
    };
    let c = run_on("default matrices", &g, 50, &[7]);
    assert_eq!(c.lag, 0);
    assert!(c.max_error <= 1e-6, "{}", c.max_error);
}

/// The game's shape (census in the L19 brief): mono/stereo/5.1 sources
/// with explicit 1-to-6 (and 2/6-to-6) matrices into 6-channel stage-2
/// submixes, into a stage-3 submix, into a stage-4 submix, into the
/// mastering voice; plus a mono send through a stage-1 mono submix.
/// Voice volumes and channel volumes on top.
#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn matrices_volumes_and_stages_match_the_host() {
    let pan = |l: f32, r: f32, c: f32, lfe: f32, bl: f32, br: f32| vec![l, r, c, lfe, bl, br];
    let src =
        |ch: u32, seed, dests: Vec<(Dest, Option<Vec<f32>>)>, volume, channel_volumes: Vec<f32>| SourceSpec {
            format_bytes: wave(&Format::pcm16(ch, RATE)),
            data: pcm16_signal(ch, 48000, seed),
            ratio: 1.0,
            dests,
            volume,
            channel_volumes,
        };
    let g = Graph {
        master_channels: 2,
        submixes: vec![
            // 0: stage 4 -> master, 6->2 matrix
            SubmixSpec {
                reverb: None,
                channels: 6,
                stage: 4,
                dest: Dest::Master,
                matrix: Some(vec![0.9, 0.0, 0.5, 0.1, 0.6, 0.0, 0.0, 0.9, 0.5, 0.1, 0.0, 0.6]),
                volume: 0.8,
            },
            // 1: stage 3 -> 0
            SubmixSpec {
                reverb: None,
                channels: 6,
                stage: 3,
                dest: Dest::Submix(0),
                matrix: None,
                volume: 1.0,
            },
            // 2: stage 2 -> 1
            SubmixSpec {
                reverb: None,
                channels: 6,
                stage: 2,
                dest: Dest::Submix(1),
                matrix: None,
                volume: 0.7,
            },
            // 3: stage 1 mono -> 1 with a 1->6 matrix
            SubmixSpec {
                reverb: None,
                channels: 1,
                stage: 1,
                dest: Dest::Submix(1),
                matrix: Some(pan(0.3, 0.3, 0.2, 0.0, 0.4, 0.4)),
                volume: 1.0,
            },
        ],
        sources: vec![
            src(
                1,
                21,
                vec![
                    (Dest::Submix(2), Some(pan(0.8, 0.2, 0.3, 0.05, 0.4, 0.1))),
                    (Dest::Submix(3), Some(vec![0.35])),
                ],
                0.6,
                vec![],
            ),
            src(
                2,
                22,
                vec![(
                    Dest::Submix(2),
                    Some(vec![0.7, 0.0, 0.0, 0.7, 0.3, 0.3, 0.0, 0.0, 0.5, 0.0, 0.0, 0.5]),
                )],
                1.0,
                vec![0.5, 1.2],
            ),
            src(
                6,
                23,
                vec![(Dest::Submix(1), None)],
                0.4,
                vec![1.0, 0.9, 0.8, 0.7, 0.6, 0.5],
            ),
            src(1, 24, vec![(Dest::Master, Some(vec![-0.3, 0.6]))], 1.0, vec![]),
        ],
    };
    let c = run("matrices, volumes, stages", &g, 50);
    assert_eq!(c.lag, 0);
    assert!(c.max_error <= 1e-6, "{}", c.max_error);
}

/// Level changes ramp over 48 frames on the host. The host applies a
/// change at the next pass after the call, so the test finds that pass in
/// the host's output and replays the change there.
#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn level_changes_ramp_like_the_host() {
    let ms = std::time::Duration::from_millis;
    let base = Graph {
        master_channels: 2,
        submixes: vec![SubmixSpec {
            reverb: None,
            channels: 6,
            stage: 0,
            dest: Dest::Master,
            matrix: None,
            volume: 1.0,
        }],
        sources: vec![SourceSpec {
            format_bytes: wave(&Format::pcm16(1, RATE)),
            data: pcm16_signal(1, 48000, 31),
            ratio: 1.0,
            dests: vec![(Dest::Submix(0), Some(vec![1.0, 0.5, 0.2, 0.0, 0.3, 0.1]))],
            volume: 1.0,
            channel_volumes: vec![],
        }],
    };
    let mut a = base.clone();
    a.sources[0].dests[0].1 = Some(vec![0.1, 0.9, 0.0, 0.0, 0.6, -0.4]);
    let mut b = a.clone();
    b.sources[0].volume = 0.3;
    let mut c = b.clone();
    c.submixes[0].volume = 0.5;
    let wanted = [a, b, c];
    let changes = vec![
        HostChange {
            delay: ms(120),
            apply: Box::new(|src, sub| unsafe {
                let m = [0.1f32, 0.9, 0.0, 0.0, 0.6, -0.4];
                as_voice(&src[0])
                    .SetOutputMatrix(&sub[0], 1, 6, m.as_ptr(), 0)
                    .unwrap();
            }),
        },
        HostChange {
            delay: ms(120),
            apply: Box::new(|src, _| unsafe { as_voice(&src[0]).SetVolume(0.3, 0).unwrap() }),
        },
        HostChange {
            delay: ms(120),
            apply: Box::new(|_, sub| unsafe { sub[0].SetVolume(0.5, 0).unwrap() }),
        },
    ];
    let passes = 60;
    for version in VERSIONS {
        let host = host_render_with(version, &base, passes, &changes);
        let mut at: Vec<(usize, Graph)> = vec![];
        for g in &wanted {
            let ours = mixer_render_with(&base, passes, &at);
            let frame = (0..host.len())
                .find(|&i| (host[i] - ours[i]).abs() > 1e-6)
                .expect("the change is audible")
                / 2;
            println!("change landed at pass {} (+{})", frame / PASS, frame % PASS);
            at.push((frame / PASS, g.clone()));
        }
        let ours = mixer_render_with(&base, passes, &at);
        let c = compare(&host, &ours, 2);
        println!(
            "level ramps vs 2.{version}: lag {}, max |error| {:e}",
            c.lag, c.max_error
        );
        // 2.9 differs by its default 6 -> 2 matrix (the submix's send here).
        if version == 7 {
            assert_eq!(c.lag, 0);
            assert!(c.max_error <= 1e-6, "{}", c.max_error);
        }
    }
}

/// XAUDIO2FX_REVERB_PARAMETERS, 2.7 layout (52 bytes, pack 1).
#[allow(clippy::too_many_arguments)]
fn reverb_params(
    wet_dry: f32,
    reflections_delay: u32,
    reverb_delay: u8,
    bytes: [u8; 11],
    floats: [f32; 8],
) -> Vec<u8> {
    let mut p = vec![];
    p.extend(wet_dry.to_le_bytes());
    p.extend(reflections_delay.to_le_bytes());
    p.push(reverb_delay);
    p.extend(bytes);
    for f in floats {
        p.extend(f.to_le_bytes());
    }
    assert_eq!(p.len(), 52);
    p
}

/// The SDK defaults (xaudio2fx.h XAUDIO2FX_REVERB_DEFAULT_*), with a wet/dry mix.
fn default_reverb(wet_dry: f32) -> Vec<u8> {
    // RearDelay 5, positions 6 6 27 27, diffusion 8 8, low EQ 8 4, high EQ 8 4
    reverb_params(
        wet_dry,
        5,
        5,
        [5, 6, 6, 27, 27, 8, 8, 8, 4, 8, 4],
        [5000.0, 0.0, 0.0, 0.0, 0.0, 1.0, 100.0, 100.0],
    )
}

fn impulse_graph(reverb: Vec<u8>) -> Graph {
    let mut data = vec![0u8; 2 * 96000];
    data[0..2].copy_from_slice(&16384i16.to_le_bytes());
    Graph {
        master_channels: 2,
        submixes: vec![SubmixSpec {
            reverb: Some(reverb),
            channels: 1,
            stage: 0,
            dest: Dest::Master,
            matrix: Some(vec![1.0, 0.0]),
            volume: 1.0,
        }],
        sources: vec![mono_source(
            wave(&Format::pcm16(1, RATE)),
            data,
            1.0,
            Dest::Submix(0),
            None,
        )],
    }
}

/// With WetDryMix 0 the reverb passes its input unchanged (both versions).
#[test]
#[ignore = "needs the host's XAudio2 2.7 and 2.9 and an audio output device"]
fn reverb_dry_path_is_exact() {
    let c = run("reverb, wet/dry 0", &impulse_graph(default_reverb(0.0)), 50);
    assert_eq!((c.lag, c.max_error), (0, 0.0));
}
