//! d3d9.dll: the Direct3D 9 objects the game drives (docs/RUNTIME.md,
//! "Graphics"). COM methods are stdcall with `this` as the first stack
//! argument. Slot numbers follow d3d9.h's declaration order; the game's own
//! calls confirm it (GetAdapterDisplayMode at +0x20, CreateDevice at +0x40,
//! IDirect3DDevice9::GetAvailableTextureMem at +0x10 and
//! CreateAdditionalSwapChain at +0x34, all in 0x00DB1480).
//!
//! The adapter answers from a capability profile recorded from a real
//! Direct3D 9 driver (scripts/d3d9_profile.py -> data/d3d9_profile.txt):
//! identifier, caps and every format, multisample, depth-stencil and
//! conversion check the game's start-up makes (0x00DA9EC0). The display
//! (desktop and mode list) is the host's, asked from its own Direct3D 9
//! ([`display`]). Device creation and resources come next; the
//! presentation backend (wgpu) after that.
use crate::dispatch::{Conflict, Registry};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::HashMap;

pub const D3D_OK: u32 = 0;
pub const D3DERR_INVALIDCALL: u32 = 0x8876_086C;
pub const D3DERR_NOTAVAILABLE: u32 = 0x8876_086A;
pub const D3DFMT_X8R8G8B8: u32 = 22;
const D3DDEVTYPE_HAL: u32 = 1;

// --- the adapter profile ------------------------------------------------------

/// The recorded adapter (see scripts/d3d9_profile.py for the format).
pub struct Profile {
    pub identifier: Vec<u8>,
    pub caps: Vec<u8>,
    pub device_caps: Vec<u8>,
    pub texture_memory: u32,
    defaults: HashMap<String, u32>,
    /// (usage, rtype) -> format -> result
    formats: HashMap<(u32, u32), HashMap<u32, u32>>,
    /// (format, windowed, type) -> (result, quality levels)
    multisample: HashMap<(u32, u32, u32), (u32, u32)>,
    /// A fresh device's render states, sampler states (sampler 0 and vertex
    /// sampler 257) and texture stage states (stages 0 and 1).
    pub render_states: Vec<(u32, u32)>,
    pub sampler_states: Vec<(u32, u32, u32)>,
    pub stage_states: Vec<(u32, u32, u32)>,
    /// other kinds: (kind, fields) -> result
    checks: HashMap<(String, Vec<u32>), u32>,
}

impl Profile {
    pub fn parse(text: &str) -> Profile {
        let mut p = Profile {
            identifier: Vec::new(), caps: Vec::new(), device_caps: Vec::new(), texture_memory: 0, defaults: HashMap::new(),
            formats: HashMap::new(), multisample: HashMap::new(), checks: HashMap::new(),
            render_states: Vec::new(), sampler_states: Vec::new(), stage_states: Vec::new(),
        };
        let hex = |s: &str| u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("d3d9 profile: bad number {s:?}"));
        let bytes = |s: &str| (0..s.len()).step_by(2).map(|k| u8::from_str_radix(&s[k..k + 2], 16).unwrap()).collect::<Vec<u8>>();
        for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let f: Vec<&str> = line.split(' ').collect();
            match f[0] {
                "identifier" => p.identifier = bytes(f[1]),
                "caps" => p.caps = bytes(f[1]),
                "devicecaps" => p.device_caps = bytes(f[1]),
                "texturememory" => p.texture_memory = hex(f[1]),
                "default" => {
                    p.defaults.insert(f[1].to_string(), hex(f[2]));
                }
                "format" => {
                    let entry = p.formats.entry((hex(f[1]), hex(f[2]))).or_default();
                    for fmt in f[4].split(',') {
                        entry.insert(hex(fmt), hex(f[3]));
                    }
                }
                "renderstate" => p.render_states.push((hex(f[1]), hex(f[2]))),
                "samplerstate" => p.sampler_states.push((hex(f[1]), hex(f[2]), hex(f[3]))),
                "stagestate" => p.stage_states.push((hex(f[1]), hex(f[2]), hex(f[3]))),
                "multisample" => {
                    p.multisample.insert((hex(f[1]), hex(f[2]), hex(f[3])), (hex(f[4]), hex(f[5])));
                }
                kind => {
                    let v: Vec<u32> = f[1..].iter().map(|x| hex(x)).collect();
                    p.checks.insert((kind.to_string(), v[..v.len() - 1].to_vec()), v[v.len() - 1]);
                }
            }
        }
        assert_eq!((p.identifier.len(), p.caps.len(), p.device_caps.len()), (1100, 304, 304), "d3d9 profile: identifier and caps sizes");
        p
    }

    fn default(&self, kind: &str) -> u32 {
        self.defaults[kind]
    }

    /// CheckDeviceFormat(HAL, X8R8G8B8, usage, rtype, format).
    pub fn format(&self, usage: u32, rtype: u32, format: u32) -> u32 {
        assert!(RECORDED_USAGES.contains(&usage),
                "CheckDeviceFormat usage {usage:#x} is not in the recorded profile (scripts/d3d9_profile.py)");
        self.formats.get(&(usage, rtype)).and_then(|m| m.get(&format).copied()).unwrap_or(self.default("format"))
    }

    /// CheckDeviceMultiSampleType(HAL, format, windowed, type) -> (result, quality levels).
    pub fn multisample(&self, format: u32, windowed: u32, kind: u32) -> (u32, u32) {
        self.multisample.get(&(format, windowed.min(1), kind)).copied().unwrap_or((self.default("multisample"), 0))
    }

    /// The other checks ("type", "depth", "conversion") by their fields.
    pub fn check(&self, kind: &str, fields: &[u32]) -> u32 {
        self.checks.get(&(kind.to_string(), fields.to_vec())).copied().unwrap_or(self.default(kind))
    }
}

/// The CheckDeviceFormat usages scripts/d3d9_profile.py records: a base
/// usage (none, render target, depth-stencil, dynamic, autogen-mipmap and
/// combinations) with at most one query flag.
const RECORDED_USAGES: [u32; 64] = {
    const BASE: [u32; 8] = [0, 0x1, 0x2, 0x200, 0x400, 0x401, 0x600, 0x201];
    const QUERY: [u32; 8] = [0, 0x8000, 0x10000, 0x20000, 0x40000, 0x80000, 0x100000, 0x200000];
    let mut out = [0u32; 64];
    let mut k = 0;
    while k < 64 {
        out[k] = BASE[k / 8] | QUERY[k % 8];
        k += 1;
    }
    out
};

pub fn profile() -> &'static Profile {
    static P: std::sync::OnceLock<Profile> = std::sync::OnceLock::new();
    P.get_or_init(|| Profile::parse(include_str!("../data/d3d9_profile.txt")))
}

// --- the display ------------------------------------------------------------

pub use hostdisplay::{Display, Mode};

/// The session's display: the one [`set_display`] gave, else the one a
/// `DDRUNTIME_DISPLAY` file records (hostdisplay's text form), else the
/// host's own Direct3D 9 asked once (hostdisplay::query): the player's
/// monitor's desktop and modes, in the driver's order (brief L34). A
/// session without any stops: the game's resolution list would be made up.
pub fn display() -> Display {
    crate::runtime::with(|rt| rt.display.with(|d| {
        d.get_or_insert_with(|| {
            if let Some(path) = std::env::var_os("DDRUNTIME_DISPLAY") {
                let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("DDRUNTIME_DISPLAY {}: {e}", path.to_string_lossy()));
                return Display::parse(&text).unwrap_or_else(|| panic!("DDRUNTIME_DISPLAY {}: not a display record", path.to_string_lossy()));
            }
            hostdisplay::query().expect("the host's display modes (Direct3D 9 adapter 0); set DDRUNTIME_DISPLAY to a recorded display")
        }).clone()
    }))
}

/// Use this display for the session (tests, replays).
pub fn set_display(d: Display) {
    crate::runtime::with(|rt| rt.display.with(|x| *x = Some(d)));
}

/// The adapter's current display mode: the full-screen swap chain's mode
/// while the game's device is full screen (D3D9 changes the display mode),
/// else the desktop's.
pub fn current_mode() -> Mode {
    let d = display();
    match crate::d3d9dev::presentation() {
        Some(p) if !p.windowed => Mode { width: p.width, height: p.height, refresh: full_screen_refresh(&d, p.width, p.height, p.refresh), format: D3DFMT_X8R8G8B8 },
        _ => d.desktop,
    }
}

/// The refresh rate a full-screen mode runs at: the one asked for, or for
/// D3DPRESENT_RATE_DEFAULT (0) the desktop's when that mode has it, else
/// the first listed rate for the size (runtime choice: D3D9 leaves the
/// default rate to the driver).
pub fn full_screen_refresh(d: &Display, width: u32, height: u32, asked: u32) -> u32 {
    if asked != 0 {
        return asked;
    }
    let sized = |m: &&Mode| m.width == width && m.height == height;
    if d.modes.iter().filter(sized).any(|m| m.refresh == d.desktop.refresh) {
        return d.desktop.refresh;
    }
    d.modes.iter().find(sized).map_or(d.desktop.refresh, |m| m.refresh)
}

pub const IDIRECT3D9: &[&str] = &[
    "QueryInterface", "AddRef", "Release", "RegisterSoftwareDevice", "GetAdapterCount",
    "GetAdapterIdentifier", "GetAdapterModeCount", "EnumAdapterModes", "GetAdapterDisplayMode",
    "CheckDeviceType", "CheckDeviceFormat", "CheckDeviceMultiSampleType", "CheckDepthStencilMatch",
    "CheckDeviceFormatConversion", "GetDeviceCaps", "GetAdapterMonitor", "CreateDevice",
];

pub const IDIRECT3DDEVICE9: &[&str] = &[
    "QueryInterface", "AddRef", "Release", "TestCooperativeLevel", "GetAvailableTextureMem",
    "EvictManagedResources", "GetDirect3D", "GetDeviceCaps", "GetDisplayMode", "GetCreationParameters",
    "SetCursorProperties", "SetCursorPosition", "ShowCursor", "CreateAdditionalSwapChain", "GetSwapChain",
    "GetNumberOfSwapChains", "Reset", "Present", "GetBackBuffer", "GetRasterStatus", "SetDialogBoxMode",
    "SetGammaRamp", "GetGammaRamp", "CreateTexture", "CreateVolumeTexture", "CreateCubeTexture",
    "CreateVertexBuffer", "CreateIndexBuffer", "CreateRenderTarget", "CreateDepthStencilSurface",
    "UpdateSurface", "UpdateTexture", "GetRenderTargetData", "GetFrontBufferData", "StretchRect",
    "ColorFill", "CreateOffscreenPlainSurface", "SetRenderTarget", "GetRenderTarget",
    "SetDepthStencilSurface", "GetDepthStencilSurface", "BeginScene", "EndScene", "Clear",
    "SetTransform", "GetTransform", "MultiplyTransform", "SetViewport", "GetViewport", "SetMaterial",
    "GetMaterial", "SetLight", "GetLight", "LightEnable", "GetLightEnable", "SetClipPlane",
    "GetClipPlane", "SetRenderState", "GetRenderState", "CreateStateBlock", "BeginStateBlock",
    "EndStateBlock", "SetClipStatus", "GetClipStatus", "GetTexture", "SetTexture",
    "GetTextureStageState", "SetTextureStageState", "GetSamplerState", "SetSamplerState",
    "ValidateDevice", "SetPaletteEntries", "GetPaletteEntries", "SetCurrentTexturePalette",
    "GetCurrentTexturePalette", "SetScissorRect", "GetScissorRect", "SetSoftwareVertexProcessing",
    "GetSoftwareVertexProcessing", "SetNPatchMode", "GetNPatchMode", "DrawPrimitive",
    "DrawIndexedPrimitive", "DrawPrimitiveUP", "DrawIndexedPrimitiveUP", "ProcessVertices",
    "CreateVertexDeclaration", "SetVertexDeclaration", "GetVertexDeclaration", "SetFVF", "GetFVF",
    "CreateVertexShader", "SetVertexShader", "GetVertexShader", "SetVertexShaderConstantF",
    "GetVertexShaderConstantF", "SetVertexShaderConstantI", "GetVertexShaderConstantI",
    "SetVertexShaderConstantB", "GetVertexShaderConstantB", "SetStreamSource", "GetStreamSource",
    "SetStreamSourceFreq", "GetStreamSourceFreq", "SetIndices", "GetIndices", "CreatePixelShader",
    "SetPixelShader", "GetPixelShader", "SetPixelShaderConstantF", "GetPixelShaderConstantF",
    "SetPixelShaderConstantI", "GetPixelShaderConstantI", "SetPixelShaderConstantB",
    "GetPixelShaderConstantB", "DrawRectPatch", "DrawTriPatch", "DeletePatch", "CreateQuery",
];

// --- exports ----------------------------------------------------------------

/// Direct3DCreate9(sdk_version): the IDirect3D9 object (the game asks for
/// D3D_SDK_VERSION 32).
pub fn direct3d_create9(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = with_state(|s| s.objects.get("IDirect3D9").copied()).expect("IDirect3D9 created at install");
    ret(m, cpu, 4);
}
/// D3DPERF_SetOptions(options): accepted (PIX is not attached).
pub fn d3dperf_set_options(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 4);
}
/// D3DPERF_GetStatus(): 0, no profiler attached (the game reports an error
/// otherwise, 0x00DB14A0).
pub fn d3dperf_get_status(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 0);
}

// --- IDirect3D9 (stdcall, this first) ----------------------------------------

/// AddRef/Release on objects the runtime owns: reference counts are not
/// tracked yet; AddRef answers 2, Release 1 (never reaching 0, the object
/// stays alive, which is what the guest may rely on).
fn add_ref(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 2;
    ret(m, cpu, 4);
}
fn release(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 4);
}
fn get_adapter_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 4);
}
/// GetAdapterDisplayMode(this, adapter, D3DDISPLAYMODE*): Width, Height,
/// RefreshRate, Format of the current mode.
fn get_adapter_display_mode(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (adapter, mode) = (arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if adapter == 0 {
        write_mode(m, mode, current_mode());
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}

pub(crate) fn write_mode(m: &mut dyn Mem, at: u32, d: Mode) {
    for (k, v) in [d.width, d.height, d.refresh, d.format].into_iter().enumerate() {
        m.set_u32(at + 4 * k as u32, v);
    }
}

fn write_bytes(m: &mut dyn Mem, at: u32, data: &[u8]) {
    for (k, &b) in data.iter().enumerate() {
        m.set_u8(at + k as u32, b);
    }
}

/// The checks are answered for adapter 0 and the HAL device on the
/// X8R8G8B8 desktop, which is what was recorded. Another adapter is an
/// invalid call; the reference rasterizer is not installed (as on a normal
/// Windows install: D3DERR_NOTAVAILABLE); another adapter format was not
/// recorded and stops.
fn adapter_check(adapter: u32, devtype: u32, adapter_format: Option<u32>) -> Option<u32> {
    if adapter != 0 {
        return Some(D3DERR_INVALIDCALL);
    }
    if devtype != D3DDEVTYPE_HAL {
        return Some(D3DERR_NOTAVAILABLE);
    }
    if let Some(f) = adapter_format {
        assert_eq!(f, D3DFMT_X8R8G8B8, "IDirect3D9 check with adapter format {f} (only X8R8G8B8 is recorded)");
    }
    None
}

/// GetAdapterIdentifier(this, adapter, flags, D3DADAPTER_IDENTIFIER9*).
fn get_adapter_identifier(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (adapter, out) = (arg(m, cpu, 1), arg(m, cpu, 3));
    cpu.eax = adapter_check(adapter, D3DDEVTYPE_HAL, None).unwrap_or_else(|| {
        write_bytes(m, out, &profile().identifier);
        D3D_OK
    });
    ret(m, cpu, 16);
}
/// GetAdapterModeCount(this, adapter, format).
fn get_adapter_mode_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (adapter, format) = (arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if adapter == 0 && format == D3DFMT_X8R8G8B8 { display().modes.len() as u32 } else { 0 };
    ret(m, cpu, 12);
}
/// EnumAdapterModes(this, adapter, format, index, D3DDISPLAYMODE*).
fn enum_adapter_modes(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (adapter, format, index, out) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    cpu.eax = match display().modes.get(index as usize) {
        Some(&mode) if adapter == 0 && format == D3DFMT_X8R8G8B8 => {
            write_mode(m, out, mode);
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 20);
}
/// CheckDeviceType(this, adapter, devtype, adapter format, back buffer format, windowed).
fn check_device_type(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (1..6).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = adapter_check(a[0], a[1], Some(a[2])).unwrap_or_else(|| profile().check("type", &[a[3], a[4].min(1)]));
    ret(m, cpu, 24);
}
/// CheckDeviceFormat(this, adapter, devtype, adapter format, usage, rtype, format).
fn check_device_format(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (1..7).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = adapter_check(a[0], a[1], Some(a[2])).unwrap_or_else(|| profile().format(a[3], a[4], a[5]));
    ret(m, cpu, 28);
}
/// CheckDeviceMultiSampleType(this, adapter, devtype, format, windowed, type, DWORD* quality levels).
fn check_device_multi_sample_type(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (1..7).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = adapter_check(a[0], a[1], None).unwrap_or_else(|| {
        let (hr, quality) = profile().multisample(a[2], a[3], a[4]);
        if a[5] != 0 && hr == D3D_OK {
            m.set_u32(a[5], quality);
        }
        hr
    });
    ret(m, cpu, 28);
}
/// CheckDepthStencilMatch(this, adapter, devtype, adapter format, render target format, depth format).
fn check_depth_stencil_match(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (1..6).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = adapter_check(a[0], a[1], Some(a[2])).unwrap_or_else(|| profile().check("depth", &[a[3], a[4]]));
    ret(m, cpu, 24);
}
/// CheckDeviceFormatConversion(this, adapter, devtype, source, target).
fn check_device_format_conversion(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (1..5).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = adapter_check(a[0], a[1], None).unwrap_or_else(|| profile().check("conversion", &[a[2], a[3]]));
    ret(m, cpu, 20);
}
/// GetDeviceCaps(this, adapter, devtype, D3DCAPS9*).
fn get_device_caps(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (adapter, devtype, out) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = adapter_check(adapter, devtype, None).unwrap_or_else(|| {
        write_bytes(m, out, &profile().caps);
        D3D_OK
    });
    ret(m, cpu, 16);
}
/// GetAdapterMonitor(this, adapter): the desktop's monitor handle.
pub const MONITOR: u32 = 0x7FC0_1000;
fn get_adapter_monitor(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = if arg(m, cpu, 1) == 0 { MONITOR } else { 0 };
    ret(m, cpu, 8);
}

pub const INTERFACES: &[Spec] = &[Spec {
    name: "IDirect3D9",
    slots: 17,
    methods: &[
        (1, add_ref), (2, release), (4, get_adapter_count), (5, get_adapter_identifier),
        (6, get_adapter_mode_count), (7, enum_adapter_modes), (8, get_adapter_display_mode),
        (9, check_device_type), (10, check_device_format), (11, check_device_multi_sample_type),
        (12, check_depth_stencil_match), (13, check_device_format_conversion), (14, get_device_caps),
        (15, get_adapter_monitor), (16, crate::d3d9dev::create_device),
    ],
    names: IDIRECT3D9,
}];

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("d3d9.dll", "Direct3DCreate9", direct3d_create9),
    ("d3d9.dll", "D3DPERF_SetOptions", d3dperf_set_options),
    ("d3d9.dll", "D3DPERF_GetStatus", d3dperf_get_status),
];

/// Create the singletons (the IDirect3D9 object) before the registry is frozen.
pub fn install(m: &mut dyn Mem, registry: &mut Registry, tokens: &mut Tokens) -> Result<(), Conflict> {
    for spec in INTERFACES {
        let o = interfaces::create(m, registry, tokens, spec)?;
        with_state(|s| s.objects.insert(spec.name, o));
    }
    crate::d3d9dev::install(m, registry, tokens)
}
