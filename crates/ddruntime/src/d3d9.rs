//! d3d9.dll: the Direct3D 9 objects the game drives (docs/RUNTIME.md,
//! "Graphics"). COM methods are stdcall with `this` as the first stack
//! argument. Slot numbers follow d3d9.h's declaration order; the game's own
//! calls confirm it (GetAdapterDisplayMode at +0x20, CreateDevice at +0x40,
//! IDirect3DDevice9::GetAvailableTextureMem at +0x10 and
//! CreateAdditionalSwapChain at +0x34, all in 0x00DB1480).
//!
//! This is the headless start: an adapter that answers queries for a
//! 1920x1080, 60 Hz X8R8G8B8 display. Device creation and resources come
//! next; the presentation backend (wgpu) after that.
use crate::dispatch::{Conflict, Registry};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

pub const D3D_OK: u32 = 0;
pub const D3DERR_INVALIDCALL: u32 = 0x8876_086C;
pub const D3DFMT_X8R8G8B8: u32 = 22;

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
/// RefreshRate, Format.
fn get_adapter_display_mode(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (adapter, mode) = (arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if adapter == 0 {
        m.set_u32(mode, crate::win32::SCREEN_W);
        m.set_u32(mode + 4, crate::win32::SCREEN_H);
        m.set_u32(mode + 8, 60);
        m.set_u32(mode + 12, D3DFMT_X8R8G8B8);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}

pub const INTERFACES: &[Spec] = &[Spec {
    name: "IDirect3D9",
    slots: 17,
    methods: &[(1, add_ref), (2, release), (4, get_adapter_count), (8, get_adapter_display_mode)],
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
    Ok(())
}
