//! AudioReverb 2.7 (CLSID_AudioReverb version 7, created by the game at
//! 0x00DCE664 and parameterised through ReverbConvertI3DL2ToNative at
//! 0x00DC5C70).
//!
//! The reverb's DSP is proprietary and its impulse response is not
//! derivable from the SDK documentation, so it is not re-implemented. On
//! Windows the platform's own effect runs instead: the AudioReverb 2.7
//! XAPO of the DirectX redistributable (`XAudio2_7.dll`, COM), or failing
//! that the AudioReverb of `xaudio2_9.dll`, called through IXAPO directly
//! (LockForProcess once, Process per pass), the same way XAudio2 hosts it.
//! The oracle compares its output with XAudio2's (tests/oracle.rs,
//! `reverb_matches_the_host`). Without a platform reverb the effect passes
//! its input through unchanged (wet signal missing), reported once.

pub struct Reverb {
    in_channels: u32,
    out_channels: u32,
    #[cfg(windows)]
    host: Option<host::HostReverb>,
}

impl Reverb {
    pub fn new(rate: u32, in_channels: u32, out_channels: u32) -> Self {
        #[cfg(windows)]
        let host = host::HostReverb::new(rate, in_channels, out_channels)
            .map_err(|e| {
                warn_once(&format!(
                    "audio: no platform AudioReverb ({e}); reverb passes dry"
                ))
            })
            .ok();
        #[cfg(not(windows))]
        {
            let _ = rate;
            warn_once("audio: no platform AudioReverb on this OS; reverb passes dry");
        }
        Self {
            in_channels,
            out_channels,
            #[cfg(windows)]
            host,
        }
    }

    /// Which platform effect runs: 7 (AudioReverb 2.7), 9 (2.9), or None.
    pub fn host_version(&self) -> Option<u32> {
        #[cfg(windows)]
        return self.host.as_ref().map(|h| h.version);
        #[cfg(not(windows))]
        None
    }

    /// `XAUDIO2FX_REVERB_PARAMETERS` in the 2.7 layout (52 bytes); empty
    /// keeps the effect's defaults.
    pub fn set_parameters(&mut self, bytes: &[u8]) {
        #[cfg(windows)]
        if let Some(h) = &mut self.host {
            h.set_parameters(bytes);
        }
        #[cfg(not(windows))]
        let _ = bytes;
    }

    /// Processes one pass. `valid` is the input's XAPO_BUFFER_VALID flag;
    /// returns the output and its flag.
    pub fn process(&mut self, input: &[f32], frames: u32, valid: bool, enabled: bool) -> (Vec<f32>, bool) {
        #[cfg(windows)]
        if let Some(h) = &mut self.host {
            return h.process(input, frames, valid, enabled);
        }
        let _ = enabled;
        let (i, o) = (self.in_channels as usize, self.out_channels as usize);
        let mut out = vec![0.0; frames as usize * o];
        for f in 0..frames as usize {
            for c in 0..o.min(i) {
                out[f * o + c] = input[f * i + c];
            }
        }
        (out, valid)
    }
}

fn warn_once(message: &str) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| eprintln!("{message}"));
}

#[cfg(windows)]
mod host {
    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Media::Audio::XAudio2::{
        XAPO_BUFFER_SILENT, XAPO_BUFFER_VALID, XAPO_FLAG_INPLACE_REQUIRED, XAPO_LOCKFORPROCESS_PARAMETERS,
        XAPO_PROCESS_BUFFER_PARAMETERS, XAPO_REGISTRATION_PROPERTIES,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };

    // IXAPO and IXAPOParameters vtables (xapo.h): the same methods in 2.7
    // and 2.9, under different IIDs.
    #[repr(C)]
    struct XapoVtbl {
        query: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *const GUID,
            *mut *mut core::ffi::c_void,
        ) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
        release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
        registration: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *mut *mut XAPO_REGISTRATION_PROPERTIES,
        ) -> HRESULT,
        _input_supported: usize,
        _output_supported: usize,
        _initialize: usize,
        _reset: usize,
        lock: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            u32,
            *const XAPO_LOCKFORPROCESS_PARAMETERS,
            u32,
            *const XAPO_LOCKFORPROCESS_PARAMETERS,
        ) -> HRESULT,
        unlock: unsafe extern "system" fn(*mut core::ffi::c_void),
        process: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            u32,
            *const XAPO_PROCESS_BUFFER_PARAMETERS,
            u32,
            *mut XAPO_PROCESS_BUFFER_PARAMETERS,
            i32,
        ),
    }
    #[repr(C)]
    struct ParametersVtbl {
        _query: usize,
        _add_ref: usize,
        release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
        set: unsafe extern "system" fn(*mut core::ffi::c_void, *const core::ffi::c_void, u32),
    }

    // 2.7: CLSID_AudioReverb version 7 (the game's 0x0142AC70); IXAPO and
    // IXAPOParameters as the game's own XAPOs use them (0x0142AF28/38).
    const CLSID_REVERB_27: GUID = GUID::from_u128(0x6a93130e_1d53_41d1_a9cf_e758800bb179);
    const IID_XAPO_27: GUID = GUID::from_u128(0xa90bc001_e897_e897_55e4_9e4700000000);
    const IID_PARAMETERS_27: GUID = GUID::from_u128(0xa90bc001_e897_e897_55e4_9e4700000001);
    // 2.9 (Windows SDK xapo.h)
    const IID_XAPO_29: GUID = GUID::from_u128(0xa410b984_9839_4819_a0be_2856ae6b3adb);
    const IID_PARAMETERS_29: GUID = GUID::from_u128(0x26d95c66_80f2_499a_ad54_5ae7f01c6d98);

    pub struct HostReverb {
        pub version: u32,
        xapo: *mut core::ffi::c_void,
        parameters: *mut core::ffi::c_void,
        in_channels: u32,
        out_channels: u32,
        in_place: bool,
        last: Vec<u8>,
        _formats: Box<[[u8; 40]; 2]>,
    }

    // The XAPO is free-threaded (XAudio2 creates it on one thread and runs
    // it on its processing thread).
    unsafe impl Send for HostReverb {}

    fn format(channels: u32, rate: u32) -> [u8; 40] {
        // WAVEFORMATEXTENSIBLE, IEEE float, the masks XAudio2 itself passes
        // (measured: 1 -> FRONT_CENTER, 2 -> FL|FR, 6 -> 5.1 side).
        let mut f = [0u8; 40];
        let mask: u32 = match channels {
            1 => 0x4,
            2 => 0x3,
            6 => 0x60f,
            _ => 0,
        };
        f[0..2].copy_from_slice(&0xfffeu16.to_le_bytes());
        f[2..4].copy_from_slice(&(channels as u16).to_le_bytes());
        f[4..8].copy_from_slice(&rate.to_le_bytes());
        f[8..12].copy_from_slice(&(rate * channels * 4).to_le_bytes());
        f[12..14].copy_from_slice(&(channels as u16 * 4).to_le_bytes());
        f[14..16].copy_from_slice(&32u16.to_le_bytes());
        f[16..18].copy_from_slice(&22u16.to_le_bytes());
        f[18..20].copy_from_slice(&32u16.to_le_bytes());
        f[20..24].copy_from_slice(&mask.to_le_bytes());
        // KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        f[24..40].copy_from_slice(&GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71).to_bytes_le());
        f
    }

    trait ToBytes {
        fn to_bytes_le(&self) -> [u8; 16];
    }
    impl ToBytes for GUID {
        fn to_bytes_le(&self) -> [u8; 16] {
            let mut b = [0u8; 16];
            b[0..4].copy_from_slice(&self.data1.to_le_bytes());
            b[4..6].copy_from_slice(&self.data2.to_le_bytes());
            b[6..8].copy_from_slice(&self.data3.to_le_bytes());
            b[8..16].copy_from_slice(&self.data4);
            b
        }
    }

    unsafe fn vtbl<T>(object: *mut core::ffi::c_void) -> &'static T {
        unsafe { &**(object as *mut *const T) }
    }

    fn create() -> Result<(u32, windows::core::IUnknown), String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(u) =
                CoCreateInstance::<_, windows::core::IUnknown>(&CLSID_REVERB_27, None, CLSCTX_INPROC_SERVER)
            {
                return Ok((7, u));
            }
            use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
            type Create = unsafe extern "system" fn(*mut *mut core::ffi::c_void) -> HRESULT;
            let lib = LoadLibraryW(windows::core::w!("xaudio2_9.dll")).map_err(|e| e.to_string())?;
            let f =
                GetProcAddress(lib, windows::core::s!("CreateAudioReverb")).ok_or("no CreateAudioReverb")?;
            let f: Create = std::mem::transmute(f);
            let mut raw = std::ptr::null_mut();
            f(&mut raw).ok().map_err(|e| e.to_string())?;
            Ok((9, windows::core::IUnknown::from_raw(raw)))
        }
    }

    impl HostReverb {
        pub fn new(rate: u32, in_channels: u32, out_channels: u32) -> Result<Self, String> {
            let (version, unknown) = create()?;
            let (iid_xapo, iid_parameters) = if version == 7 {
                (IID_XAPO_27, IID_PARAMETERS_27)
            } else {
                (IID_XAPO_29, IID_PARAMETERS_29)
            };
            unsafe {
                let mut xapo = std::ptr::null_mut();
                unknown
                    .query(&iid_xapo, &mut xapo)
                    .ok()
                    .map_err(|e| e.to_string())?;
                let mut parameters = std::ptr::null_mut();
                unknown
                    .query(&iid_parameters, &mut parameters)
                    .ok()
                    .map_err(|e| e.to_string())?;
                let v = vtbl::<XapoVtbl>(xapo);
                let mut props = std::ptr::null_mut();
                (v.registration)(xapo, &mut props)
                    .ok()
                    .map_err(|e| e.to_string())?;
                let flags = std::ptr::addr_of!((*props).Flags).read_unaligned();
                CoTaskMemFree(Some(props as *const _));
                let formats = Box::new([format(in_channels, rate), format(out_channels, rate)]);
                let lock = |i: usize| XAPO_LOCKFORPROCESS_PARAMETERS {
                    pFormat: formats[i].as_ptr() as *const _,
                    MaxFrameCount: rate / 100,
                };
                let (input, output) = (lock(0), lock(1));
                (v.lock)(xapo, 1, &input, 1, &output)
                    .ok()
                    .map_err(|e| format!("LockForProcess: {e}"))?;
                Ok(Self {
                    version,
                    xapo,
                    parameters,
                    in_channels,
                    out_channels,
                    in_place: flags & XAPO_FLAG_INPLACE_REQUIRED != 0,
                    last: vec![],
                    _formats: formats,
                })
            }
        }

        pub fn set_parameters(&mut self, bytes: &[u8]) {
            if bytes.is_empty() || bytes == self.last.as_slice() {
                return;
            }
            self.last = bytes.to_vec();
            // 2.9's structure adds SideDelay after RearDelay and
            // DisableLateField at the end (xaudio2fx.h).
            let block = if self.version == 7 || bytes.len() != 52 {
                bytes.to_vec()
            } else {
                let mut q = bytes[..10].to_vec();
                q.push(bytes[9].min(5));
                q.extend_from_slice(&bytes[10..]);
                q.extend(0u32.to_le_bytes());
                q
            };
            unsafe {
                (vtbl::<ParametersVtbl>(self.parameters).set)(
                    self.parameters,
                    block.as_ptr() as *const _,
                    block.len() as u32,
                )
            };
        }

        pub fn process(
            &mut self,
            input: &[f32],
            frames: u32,
            valid: bool,
            enabled: bool,
        ) -> (Vec<f32>, bool) {
            let mut a = input.to_vec();
            let mut b = vec![0.0f32; (frames * self.out_channels) as usize];
            let out_ptr = if self.in_place && self.in_channels == self.out_channels {
                a.as_mut_ptr()
            } else {
                b.as_mut_ptr()
            };
            let i = XAPO_PROCESS_BUFFER_PARAMETERS {
                pBuffer: a.as_mut_ptr() as *mut _,
                BufferFlags: if valid {
                    XAPO_BUFFER_VALID
                } else {
                    XAPO_BUFFER_SILENT
                },
                ValidFrameCount: frames,
            };
            let mut o = XAPO_PROCESS_BUFFER_PARAMETERS {
                pBuffer: out_ptr as *mut _,
                BufferFlags: XAPO_BUFFER_VALID,
                ValidFrameCount: frames,
            };
            unsafe { (vtbl::<XapoVtbl>(self.xapo).process)(self.xapo, 1, &i, 1, &mut o, enabled as i32) };
            let flags = o.BufferFlags;
            let out = if out_ptr == a.as_mut_ptr() { a } else { b };
            if flags == XAPO_BUFFER_SILENT {
                return (vec![0.0; out.len()], false);
            }
            (out, true)
        }
    }

    impl Drop for HostReverb {
        fn drop(&mut self) {
            unsafe {
                (vtbl::<XapoVtbl>(self.xapo).unlock)(self.xapo);
                (vtbl::<ParametersVtbl>(self.parameters).release)(self.parameters);
                (vtbl::<XapoVtbl>(self.xapo).release)(self.xapo);
            }
        }
    }
}
