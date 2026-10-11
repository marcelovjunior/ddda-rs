//! The Direct3D 9 device and the objects it creates (docs/RUNTIME.md,
//! "Graphics": a D3D9 object boundary, backed by wgpu later).
//!
//! Every object is a guest COM object (crate::interfaces) with a reference
//! count the runtime keeps: AddRef/Release follow COM, and the last Release
//! frees the guest block. Behaviour follows the documented D3D9 contract
//! (Microsoft Learn: IDirect3D9::CreateDevice, D3DPRESENT_PARAMETERS,
//! IDirect3DDevice9, IDirect3DSwapChain9, IDirect3DSurface9). The device is
//! headless for now: Present counts frames and nothing is drawn; methods
//! are added as the game reaches them, the rest stop with their name.
use crate::d3d9::{profile, D3DERR_INVALIDCALL, D3DFMT_X8R8G8B8, D3D_OK};
use crate::d3d9draw as draw;
use crate::d3d9res as res;
use crate::d3d9state as st;
use crate::dispatch::{Conflict, Registry};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::HashMap;

pub const D3DERR_NOTFOUND: u32 = 0x8876_0866;
const D3DFMT_UNKNOWN: u32 = 0;
const D3DRTYPE_SURFACE: u32 = 1;
const D3DUSAGE_RENDERTARGET: u32 = 0x1;
const D3DUSAGE_DEPTHSTENCIL: u32 = 0x2;
const D3DPOOL_DEFAULT: u32 = 0;

/// D3DPRESENT_PARAMETERS (x86 layout, 56 bytes) as 14 words: BackBufferWidth,
/// BackBufferHeight, BackBufferFormat, BackBufferCount, MultiSampleType,
/// MultiSampleQuality, SwapEffect, hDeviceWindow, Windowed,
/// EnableAutoDepthStencil, AutoDepthStencilFormat, Flags,
/// FullScreen_RefreshRateInHz, PresentationInterval.
pub type PresentParameters = [u32; 14];

#[derive(Clone, Debug)]
pub struct SurfaceDesc {
    pub format: u32,
    pub usage: u32,
    pub pool: u32,
    pub multisample: u32,
    pub quality: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub enum Kind {
    Device(Box<DeviceData>),
    /// IDirect3DVertexShader9 / IDirect3DPixelShader9: the bytecode
    Shader { device: u32, tokens: Vec<u32> },
    /// IDirect3DVertexDeclaration9: the elements (8 bytes each, END excluded)
    Declaration { device: u32, elements: Vec<[u8; 8]> },
    SwapChain { device: u32, pp: PresentParameters, back_buffers: Vec<u32>, presents: u64 },
    /// `owner`: the texture whose reference count this level surface
    /// shares (0 for standalone surfaces and back buffers)
    Surface { device: u32, desc: SurfaceDesc, container: u32, owner: u32, image: crate::d3d9res::Image },
    Texture { device: u32, data: Box<crate::d3d9res::TextureData> },
    Buffer { device: u32, data: Box<crate::d3d9res::BufferData> },
}

#[derive(Debug)]
pub struct Object {
    pub iface: &'static str,
    pub refs: u32,
    pub kind: Kind,
}

#[derive(Default)]
pub(crate) struct Objects {
    vtables: HashMap<&'static str, u32>,
    pub(crate) live: HashMap<u32, Object>,
}

pub(crate) fn objects<R>(f: impl FnOnce(&mut Objects) -> R) -> R {
    crate::runtime::with(|rt| rt.d3d.with(f))
}

/// The interface and reference count of a live object (tests, diagnostics).
pub fn refs(object: u32) -> Option<(&'static str, u32)> {
    objects(|o| o.live.get(&object).map(|x| (x.iface, x.refs)))
}

/// A new guest object of `iface` with one reference.
pub(crate) fn new_object(m: &mut dyn Mem, iface: &'static str, kind: Kind) -> u32 {
    let vtable = objects(|o| o.vtables.get(iface).copied()).unwrap_or_else(|| panic!("no vtable for {iface}"));
    let p = interfaces::object(m, vtable, 16);
    objects(|o| o.live.insert(p, Object { iface, refs: 1, kind }));
    p
}

pub(crate) fn add_ref_to(object: u32) -> u32 {
    let object = counted(object);
    objects(|o| {
        let x = o.live.get_mut(&object).unwrap_or_else(|| panic!("AddRef on {object:#x}, not a live D3D9 object"));
        x.refs += 1;
        x.refs
    })
}

/// Release one reference; at zero the object goes, releasing what it holds
/// (a swap chain its back buffers, a device its swap chains and surfaces).
pub(crate) fn release_of(object: u32) -> u32 {
    let object = counted(object);
    let (left, gone) = objects(|o| {
        let x = o.live.get_mut(&object).unwrap_or_else(|| panic!("Release on {object:#x}, not a live D3D9 object"));
        x.refs -= 1;
        if x.refs == 0 { (0, o.live.remove(&object)) } else { (x.refs, None) }
    });
    if let Some(x) = gone {
        let held: Vec<u32> = match x.kind {
            Kind::Device(d) => {
                let d = *d;
                d.swap_chains.into_iter().chain([d.depth_stencil]).chain(d.render_targets).chain(d.state.held())
                    .filter(|&p| p != 0).collect()
            }
            Kind::SwapChain { back_buffers, .. } => back_buffers,
            Kind::Surface { image, .. } => {
                image.free();
                vec![]
            }
            Kind::Texture { data, .. } => {
                // the level surfaces go with the texture (they share its count)
                for s in &data.surfaces {
                    if let Some(x) = objects(|o| o.live.remove(s)) {
                        if let Kind::Surface { image, .. } = x.kind {
                            image.free();
                        }
                        with_state(|st| st.heap.free(*s)).expect("surface object block");
                    }
                }
                for v in &data.volumes {
                    v.free();
                }
                vec![]
            }
            Kind::Buffer { data, .. } => {
                data.image.free();
                vec![]
            }
            Kind::Shader { .. } | Kind::Declaration { .. } => vec![],
        };
        for h in held {
            release_of(h);
        }
        with_state(|s| s.heap.free(object)).expect("D3D9 object block");
    }
    left
}

fn add_ref(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = add_ref_to(arg(m, cpu, 0));
    ret(m, cpu, 4);
}
fn release(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = release_of(arg(m, cpu, 0));
    ret(m, cpu, 4);
}

/// Hand an object to the guest through an out pointer, with a reference.
pub(crate) fn hand_out(m: &mut dyn Mem, out: u32, object: u32) {
    if object != 0 {
        add_ref_to(object);
    }
    m.set_u32(out, object);
}

// --- device creation (IDirect3D9::CreateDevice) ----------------------------------

/// The back buffers and auto depth-stencil surface a swap chain's present
/// parameters describe.
fn surface(m: &mut dyn Mem, device: u32, desc: SurfaceDesc, container: u32) -> u32 {
    let image = crate::d3d9res::Image::new(desc.format, desc.width, desc.height, 1).unwrap_or_default();
    new_object(m, "IDirect3DSurface9", Kind::Surface { device, desc, container, owner: 0, image })
}

/// The object whose reference count `object` uses (a texture's level
/// surfaces count on the texture).
fn counted(object: u32) -> u32 {
    objects(|o| match o.live.get(&object).map(|x| &x.kind) {
        Some(Kind::Surface { owner, .. }) if *owner != 0 => *owner,
        _ => object,
    })
}

/// Complete windowed present parameters as D3D9 does: a zero back buffer
/// size is the window's client size, an unknown format the display
/// format, a zero count one buffer. Fullscreen parameters must name a mode
/// of the desktop. None if the parameters are invalid.
fn complete(mut pp: PresentParameters, focus: u32) -> Option<PresentParameters> {
    if pp[7] == 0 {
        pp[7] = focus;
    }
    if pp[8] != 0 {
        if pp[12] != 0 {
            return None; // a refresh rate is only valid in fullscreen
        }
        if pp[0] == 0 || pp[1] == 0 {
            let w = crate::user32::window(pp[7])?;
            let (cw, ch) = crate::user32::client_size_of(&w);
            if pp[0] == 0 {
                pp[0] = cw as u32;
            }
            if pp[1] == 0 {
                pp[1] = ch as u32;
            }
        }
        if pp[2] == D3DFMT_UNKNOWN {
            pp[2] = D3DFMT_X8R8G8B8;
        }
    } else if !crate::d3d9::MODES.contains(&(pp[0], pp[1])) || !matches!(pp[12], 0 | 60) {
        return None;
    }
    if pp[3] == 0 {
        pp[3] = 1;
    }
    (pp[3] <= 3).then_some(pp)
}

fn new_swap_chain(m: &mut dyn Mem, device: u32, pp: PresentParameters) -> u32 {
    let chain = new_object(m, "IDirect3DSwapChain9", Kind::SwapChain { device, pp, back_buffers: vec![], presents: 0 });
    let buffers: Vec<u32> = (0..pp[3]).map(|_| {
        let desc = SurfaceDesc { format: pp[2], usage: D3DUSAGE_RENDERTARGET, pool: D3DPOOL_DEFAULT, multisample: pp[4], quality: pp[5], width: pp[0], height: pp[1] };
        surface(m, device, desc, chain)
    }).collect();
    objects(|o| {
        if let Some(Object { kind: Kind::SwapChain { back_buffers, .. }, .. }) = o.live.get_mut(&chain) {
            *back_buffers = buffers;
        }
    });
    chain
}

fn read_pp(m: &dyn Mem, p: u32) -> PresentParameters {
    std::array::from_fn(|k| m.u32(p + 4 * k as u32))
}
fn write_pp(m: &mut dyn Mem, p: u32, pp: &PresentParameters) {
    for (k, v) in pp.iter().enumerate() {
        m.set_u32(p + 4 * k as u32, *v);
    }
}

/// IDirect3D9::CreateDevice(this, adapter, devtype, focus window, flags,
/// D3DPRESENT_PARAMETERS*, IDirect3DDevice9**). The completed parameters
/// are written back, as D3D9 does.
pub fn create_device(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..7).map(|k| arg(m, cpu, k)).collect();
    let (adapter, devtype, focus, flags, pp_ptr, out) = (a[1], a[2], a[3], a[4], a[5], a[6]);
    let pp = if adapter == 0 && devtype == 1 { complete(read_pp(m, pp_ptr), focus) } else { None };
    cpu.eax = match pp {
        None => D3DERR_INVALIDCALL,
        Some(pp) => {
            write_pp(m, pp_ptr, &pp);
            let state = crate::d3d9state::DeviceState::new(pp[9] != 0, pp[0], pp[1]);
            let device = new_object(m, "IDirect3DDevice9", Kind::Device(Box::new(DeviceData {
                creation: [adapter, devtype, focus, flags], swap_chains: vec![], depth_stencil: 0, render_targets: [0; 4], state,
            })));
            let chain = new_swap_chain(m, device, pp);
            let back = objects(|o| match &o.live[&chain].kind { Kind::SwapChain { back_buffers, .. } => back_buffers[0], _ => unreachable!() });
            add_ref_to(back); // render target 0 holds the first back buffer
            let depth = if pp[9] != 0 {
                let desc = SurfaceDesc { format: pp[10], usage: D3DUSAGE_DEPTHSTENCIL, pool: D3DPOOL_DEFAULT, multisample: pp[4], quality: pp[5], width: pp[0], height: pp[1] };
                surface(m, device, desc, device)
            } else {
                0
            };
            objects(|o| {
                if let Some(Object { kind: Kind::Device(d), .. }) = o.live.get_mut(&device) {
                    d.swap_chains.push(chain);
                    d.depth_stencil = depth;
                    d.render_targets[0] = back;
                }
            });
            m.set_u32(out, device);
            D3D_OK
        }
    };
    ret(m, cpu, 28);
}

// --- IDirect3DDevice9 ------------------------------------------------------------

/// A device's own record.
pub struct DeviceData {
    pub creation: [u32; 4],
    pub swap_chains: Vec<u32>,
    pub depth_stencil: u32,
    pub render_targets: [u32; 4],
    pub state: crate::d3d9state::DeviceState,
}

impl std::fmt::Debug for DeviceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DeviceData {{ creation: {:?}, swap_chains: {:?} }}", self.creation, self.swap_chains)
    }
}

pub(crate) fn with_device<R>(device: u32, f: impl FnOnce(&mut DeviceData) -> R) -> R {
    objects(|o| match o.live.get_mut(&device) {
        Some(Object { kind: Kind::Device(d), .. }) => f(d),
        _ => panic!("{device:#x} is not a live IDirect3DDevice9"),
    })
}

/// TestCooperativeLevel(this): the device is never lost on the headless desktop.
fn test_cooperative_level(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = D3D_OK;
    ret(m, cpu, 4);
}
/// GetAvailableTextureMem(this): the recorded driver's answer.
fn get_available_texture_mem(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = profile().texture_memory;
    ret(m, cpu, 4);
}
/// EvictManagedResources(this).
fn evict_managed_resources(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = D3D_OK;
    ret(m, cpu, 4);
}
/// GetDeviceCaps(this, D3DCAPS9*): the recorded device caps.
fn get_device_caps(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 1);
    for (k, &b) in profile().device_caps.iter().enumerate() {
        m.set_u8(out + k as u32, b);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetDisplayMode(this, swap chain, D3DDISPLAYMODE*): the desktop's mode.
fn get_display_mode(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (chain, out) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let count = with_device(arg(m, cpu, 0), |d| d.swap_chains.len() as u32);
    cpu.eax = if chain < count {
        for (k, v) in [crate::win32::SCREEN_W, crate::win32::SCREEN_H, 60, D3DFMT_X8R8G8B8].into_iter().enumerate() {
            m.set_u32(out + 4 * k as u32, v);
        }
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}
/// GetCreationParameters(this, D3DDEVICE_CREATION_PARAMETERS*).
fn get_creation_parameters(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 1);
    let c = with_device(arg(m, cpu, 0), |d| d.creation);
    for (k, v) in c.into_iter().enumerate() {
        m.set_u32(out + 4 * k as u32, v);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// CreateAdditionalSwapChain(this, D3DPRESENT_PARAMETERS*, IDirect3DSwapChain9**):
/// windowed only (D3D9's rule for additional swap chains).
fn create_additional_swap_chain(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (device, pp_ptr, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let focus = with_device(device, |d| d.creation[2]);
    let pp = read_pp(m, pp_ptr);
    cpu.eax = match complete(pp, focus).filter(|pp| pp[8] != 0) {
        Some(pp) => {
            write_pp(m, pp_ptr, &pp);
            let chain = new_swap_chain(m, device, pp);
            with_device(device, |d| d.swap_chains.push(chain));
            hand_out(m, out, chain); // the device keeps its own reference
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}
/// GetSwapChain(this, index, IDirect3DSwapChain9**).
fn get_swap_chain(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (index, out) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let chain = with_device(arg(m, cpu, 0), |d| d.swap_chains.get(index as usize).copied());
    cpu.eax = match chain {
        Some(c) => {
            hand_out(m, out, c);
            D3D_OK
        }
        None => {
            m.set_u32(out, 0);
            D3DERR_INVALIDCALL
        }
    };
    ret(m, cpu, 12);
}
/// GetNumberOfSwapChains(this).
fn get_number_of_swap_chains(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = with_device(arg(m, cpu, 0), |d| d.swap_chains.len() as u32);
    ret(m, cpu, 4);
}
fn back_buffer_of(chain: u32, index: u32) -> Option<u32> {
    objects(|o| match o.live.get(&chain) {
        Some(Object { kind: Kind::SwapChain { back_buffers, .. }, .. }) => back_buffers.get(index as usize).copied(),
        _ => None,
    })
}
/// GetBackBuffer(this, swap chain, index, type, IDirect3DSurface9**).
fn get_back_buffer(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (chain, index, out) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 4));
    let chain = with_device(arg(m, cpu, 0), |d| d.swap_chains.get(chain as usize).copied());
    cpu.eax = match chain.and_then(|c| back_buffer_of(c, index)) {
        Some(s) => {
            hand_out(m, out, s);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 20);
}
/// GetRenderTarget(this, index, IDirect3DSurface9**).
fn get_render_target(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (index, out) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let rt = with_device(arg(m, cpu, 0), |d| d.render_targets.get(index as usize).copied());
    cpu.eax = match rt {
        Some(s) if s != 0 => {
            hand_out(m, out, s);
            D3D_OK
        }
        Some(_) => {
            m.set_u32(out, 0);
            D3DERR_NOTFOUND
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}
/// GetDepthStencilSurface(this, IDirect3DSurface9**).
fn get_depth_stencil_surface(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 1);
    let ds = with_device(arg(m, cpu, 0), |d| d.depth_stencil);
    cpu.eax = if ds != 0 {
        hand_out(m, out, ds);
        D3D_OK
    } else {
        m.set_u32(out, 0);
        D3DERR_NOTFOUND
    };
    ret(m, cpu, 8);
}
/// GetDirect3D(this, IDirect3D9**): the runtime's IDirect3D9.
fn get_direct3d(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 1);
    m.set_u32(out, with_state(|s| s.objects.get("IDirect3D9").copied()).expect("IDirect3D9"));
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// QueryInterface(this, riid, void**): the runtime's objects expose only
/// the interface they were created as; IUnknown and that one are answered
/// with the object itself, which suffices for the AddRef/Release pattern.
fn query_interface(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, out) = (arg(m, cpu, 0), arg(m, cpu, 2));
    let iid: Vec<u8> = (0..16).map(|k| m.u8(arg(m, cpu, 1) + k)).collect();
    // IID_IUnknown {00000000-0000-0000-C000-000000000046}
    let unknown = [0, 0, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];
    cpu.eax = if iid == unknown {
        hand_out(m, out, this);
        D3D_OK
    } else {
        panic!("QueryInterface on {:?} for an interface the runtime does not model: {iid:02x?}", refs(this).map(|r| r.0));
    };
    ret(m, cpu, 12);
}

// --- IDirect3DSwapChain9 ---------------------------------------------------------

fn with_chain<R>(chain: u32, f: impl FnOnce(u32, &mut PresentParameters, &mut Vec<u32>, &mut u64) -> R) -> R {
    objects(|o| match o.live.get_mut(&chain) {
        Some(Object { kind: Kind::SwapChain { device, pp, back_buffers, presents }, .. }) => f(*device, pp, back_buffers, presents),
        _ => panic!("{chain:#x} is not a live IDirect3DSwapChain9"),
    })
}
/// Present(this, src, dst, window, dirty, flags): a frame (headless).
fn chain_present(m: &mut dyn Mem, cpu: &mut Cpu) {
    let chain = arg(m, cpu, 0);
    with_chain(chain, |_, _, _, n| *n += 1);
    if let Some(back) = back_buffer_of(chain, 0) {
        draw::present_back_buffer(m, back);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 24);
}
/// GetBackBuffer(this, index, type, IDirect3DSurface9**).
fn chain_get_back_buffer(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (chain, index, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 3));
    cpu.eax = match back_buffer_of(chain, index) {
        Some(s) => {
            hand_out(m, out, s);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}
/// GetDevice(this, IDirect3DDevice9**) for swap chains and surfaces.
fn get_device(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let device = objects(|o| match o.live.get(&this).map(|x| &x.kind) {
        Some(Kind::SwapChain { device, .. }) | Some(Kind::Surface { device, .. }) | Some(Kind::Shader { device, .. })
        | Some(Kind::Declaration { device, .. }) | Some(Kind::Texture { device, .. }) | Some(Kind::Buffer { device, .. }) => *device,
        _ => panic!("GetDevice on {this:#x}"),
    });
    hand_out(m, out, device);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetPresentParameters(this, D3DPRESENT_PARAMETERS*).
fn chain_get_present_parameters(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 1);
    let pp = with_chain(arg(m, cpu, 0), |_, pp, _, _| *pp);
    write_pp(m, out, &pp);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// IDirect3DDevice9::Present(this, src, dst, window, dirty): the implicit chain's.
fn present(m: &mut dyn Mem, cpu: &mut Cpu) {
    let chain = with_device(arg(m, cpu, 0), |d| d.swap_chains[0]);
    with_chain(chain, |_, _, _, n| *n += 1);
    if let Some(back) = back_buffer_of(chain, 0) {
        draw::present_back_buffer(m, back);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 20);
}

/// Frames presented on a swap chain (tests, diagnostics).
pub fn presents(chain: u32) -> u64 {
    with_chain(chain, |_, _, _, n| *n)
}

// --- IDirect3DSurface9 -----------------------------------------------------------

/// GetDesc(this, D3DSURFACE_DESC*): Format, Type, Usage, Pool,
/// MultiSampleType, MultiSampleQuality, Width, Height.
fn surface_get_desc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let d = objects(|o| match o.live.get(&this).map(|x| &x.kind) {
        Some(Kind::Surface { desc, .. }) => desc.clone(),
        _ => panic!("GetDesc on {this:#x}, not a surface"),
    });
    for (k, v) in [d.format, D3DRTYPE_SURFACE, d.usage & !crate::d3d9res::NOT_LOCKABLE, d.pool, d.multisample, d.quality, d.width, d.height].into_iter().enumerate() {
        m.set_u32(out + 4 * k as u32, v);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}

pub const ISWAPCHAIN9: &[&str] = &[
    "QueryInterface", "AddRef", "Release", "Present", "GetFrontBufferData", "GetBackBuffer",
    "GetRasterStatus", "GetDisplayMode", "GetDevice", "GetPresentParameters",
];
pub const ISURFACE9: &[&str] = &[
    "QueryInterface", "AddRef", "Release", "GetDevice", "SetPrivateData", "GetPrivateData",
    "FreePrivateData", "SetPriority", "GetPriority", "PreLoad", "GetType", "GetContainer", "GetDesc",
    "LockRect", "UnlockRect", "GetDC", "ReleaseDC",
];

pub const ITEXTURE9: &[&str] = &[
    "QueryInterface", "AddRef", "Release", "GetDevice", "SetPrivateData", "GetPrivateData", "FreePrivateData",
    "SetPriority", "GetPriority", "PreLoad", "GetType", "SetLOD", "GetLOD", "GetLevelCount", "SetAutoGenFilterType",
    "GetAutoGenFilterType", "GenerateMipSubLevels", "GetLevelDesc", "GetSurfaceLevel", "LockRect", "UnlockRect",
    "AddDirtyRect",
];
pub const IBUFFER9: &[&str] = &[
    "QueryInterface", "AddRef", "Release", "GetDevice", "SetPrivateData", "GetPrivateData", "FreePrivateData",
    "SetPriority", "GetPriority", "PreLoad", "GetType", "Lock", "Unlock", "GetDesc",
];

pub const INTERFACES: &[Spec] = &[
    Spec {
        name: "IDirect3DDevice9",
        slots: 119,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, test_cooperative_level),
            (4, get_available_texture_mem), (5, evict_managed_resources), (6, get_direct3d),
            (7, get_device_caps), (8, get_display_mode), (9, get_creation_parameters),
            (13, create_additional_swap_chain), (14, get_swap_chain), (15, get_number_of_swap_chains),
            (17, present), (18, get_back_buffer), (23, res::create_texture), (24, res::create_volume_texture),
            (25, res::create_cube_texture), (26, res::create_vertex_buffer), (27, res::create_index_buffer),
            (28, res::create_render_target), (29, res::create_depth_stencil_surface),
            (36, res::create_offscreen_plain_surface), (37, st::set_render_target), (38, get_render_target),
            (39, st::set_depth_stencil_surface), (40, get_depth_stencil_surface), (41, st::begin_scene),
            (42, st::end_scene), (43, draw::clear), (44, st::set_transform), (45, st::get_transform),
            (47, st::set_viewport), (48, st::get_viewport), (57, st::set_render_state), (58, st::get_render_state),
            (64, st::get_texture), (65, st::set_texture), (66, st::get_texture_stage_state),
            (67, st::set_texture_stage_state), (68, st::get_sampler_state), (69, st::set_sampler_state),
            (75, st::set_scissor_rect), (81, draw::draw_primitive), (82, draw::draw_indexed_primitive),
            (83, draw::draw_primitive_up), (84, draw::draw_indexed_primitive_up), (76, st::get_scissor_rect), (86, st::create_vertex_declaration),
            (87, st::set_vertex_declaration), (88, st::get_vertex_declaration), (89, st::set_fvf), (90, st::get_fvf),
            (91, st::create_vertex_shader), (92, st::set_vertex_shader), (93, st::get_vertex_shader),
            (94, st::set_vs_f), (95, st::get_vs_f), (96, st::set_vs_i), (97, st::get_vs_i), (98, st::set_vs_b),
            (99, st::get_vs_b), (100, st::set_stream_source), (101, st::get_stream_source),
            (102, st::set_stream_source_freq), (103, st::get_stream_source_freq), (104, st::set_indices),
            (105, st::get_indices), (106, st::create_pixel_shader), (107, st::set_pixel_shader),
            (108, st::get_pixel_shader), (109, st::set_ps_f), (110, st::get_ps_f), (111, st::set_ps_i),
            (112, st::get_ps_i), (113, st::set_ps_b), (114, st::get_ps_b),
        ],
        names: crate::d3d9::IDIRECT3DDEVICE9,
    },
    Spec {
        name: "IDirect3DSwapChain9",
        slots: 10,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, chain_present), (5, chain_get_back_buffer),
            (8, get_device), (9, chain_get_present_parameters),
        ],
        names: ISWAPCHAIN9,
    },
    Spec {
        name: "IDirect3DTexture9",
        slots: 22,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (7, res::resource_set_priority),
            (8, res::resource_get_priority), (9, res::resource_preload), (10, res::resource_get_type),
            (11, res::texture_set_lod), (12, res::texture_get_lod), (13, res::texture_get_level_count),
            (14, res::texture_set_autogen_filter), (15, res::texture_get_autogen_filter), (16, res::texture_generate_mips),
            (17, res::texture_get_level_desc), (18, res::texture_get_surface_level), (19, res::texture_lock_rect),
            (20, res::texture_unlock_rect), (21, res::texture_add_dirty),
        ],
        names: ITEXTURE9,
    },
    Spec {
        name: "IDirect3DCubeTexture9",
        slots: 22,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (7, res::resource_set_priority),
            (8, res::resource_get_priority), (9, res::resource_preload), (10, res::resource_get_type),
            (11, res::texture_set_lod), (12, res::texture_get_lod), (13, res::texture_get_level_count),
            (14, res::texture_set_autogen_filter), (15, res::texture_get_autogen_filter), (16, res::texture_generate_mips),
            (18, res::cube_get_surface), (19, res::cube_lock_rect), (20, res::cube_unlock_rect), (21, res::cube_add_dirty),
        ],
        names: &[
            "QueryInterface", "AddRef", "Release", "GetDevice", "SetPrivateData", "GetPrivateData", "FreePrivateData",
            "SetPriority", "GetPriority", "PreLoad", "GetType", "SetLOD", "GetLOD", "GetLevelCount",
            "SetAutoGenFilterType", "GetAutoGenFilterType", "GenerateMipSubLevels", "GetLevelDesc",
            "GetCubeMapSurface", "LockRect", "UnlockRect", "AddDirtyRect",
        ],
    },
    Spec {
        name: "IDirect3DVolumeTexture9",
        slots: 22,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (7, res::resource_set_priority),
            (8, res::resource_get_priority), (9, res::resource_preload), (10, res::resource_get_type),
            (11, res::texture_set_lod), (12, res::texture_get_lod), (13, res::texture_get_level_count),
            (14, res::texture_set_autogen_filter), (15, res::texture_get_autogen_filter), (16, res::texture_generate_mips),
            (17, res::volume_get_level_desc), (19, res::volume_lock_box), (20, res::volume_unlock_box), (21, res::texture_add_dirty),
        ],
        names: &[
            "QueryInterface", "AddRef", "Release", "GetDevice", "SetPrivateData", "GetPrivateData", "FreePrivateData",
            "SetPriority", "GetPriority", "PreLoad", "GetType", "SetLOD", "GetLOD", "GetLevelCount",
            "SetAutoGenFilterType", "GetAutoGenFilterType", "GenerateMipSubLevels", "GetLevelDesc",
            "GetVolumeLevel", "LockBox", "UnlockBox", "AddDirtyBox",
        ],
    },
    Spec {
        name: "IDirect3DVertexBuffer9",
        slots: 14,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (7, res::resource_set_priority),
            (8, res::resource_get_priority), (9, res::resource_preload), (10, res::resource_get_type),
            (11, res::buffer_lock), (12, res::buffer_unlock), (13, res::buffer_get_desc),
        ],
        names: IBUFFER9,
    },
    Spec {
        name: "IDirect3DIndexBuffer9",
        slots: 14,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (7, res::resource_set_priority),
            (8, res::resource_get_priority), (9, res::resource_preload), (10, res::resource_get_type),
            (11, res::buffer_lock), (12, res::buffer_unlock), (13, res::buffer_get_desc),
        ],
        names: IBUFFER9,
    },
    Spec {
        name: "IDirect3DVertexShader9",
        slots: 5,
        methods: &[(0, query_interface), (1, add_ref), (2, release), (3, get_device), (4, st::shader_get_function)],
        names: &["QueryInterface", "AddRef", "Release", "GetDevice", "GetFunction"],
    },
    Spec {
        name: "IDirect3DPixelShader9",
        slots: 5,
        methods: &[(0, query_interface), (1, add_ref), (2, release), (3, get_device), (4, st::shader_get_function)],
        names: &["QueryInterface", "AddRef", "Release", "GetDevice", "GetFunction"],
    },
    Spec {
        name: "IDirect3DVertexDeclaration9",
        slots: 5,
        methods: &[(0, query_interface), (1, add_ref), (2, release), (3, get_device), (4, st::declaration_get)],
        names: &["QueryInterface", "AddRef", "Release", "GetDevice", "GetDeclaration"],
    },
    Spec {
        name: "IDirect3DSurface9",
        slots: 17,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (7, res::resource_set_priority),
            (8, res::resource_get_priority), (9, res::resource_preload), (10, res::resource_get_type),
            (12, surface_get_desc), (13, res::surface_lock_rect), (14, res::surface_unlock_rect),
        ],
        names: ISURFACE9,
    },
];

/// Build the vtables (before the registry is frozen); objects are created
/// at run time.
pub fn install(m: &mut dyn Mem, registry: &mut Registry, tokens: &mut Tokens) -> Result<(), Conflict> {
    objects(|o| *o = Objects::default());
    for spec in INTERFACES {
        let v = interfaces::vtable(m, registry, tokens, spec)?;
        objects(|o| o.vtables.insert(spec.name, v));
    }
    Ok(())
}
