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
use crate::d3d9copy as cp;
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
pub const D3DERR_DEVICELOST: u32 = 0x8876_0868;
pub const D3DERR_DEVICENOTRESET: u32 = 0x8876_0869;
/// D3DCAPS2_FULLSCREENGAMMA (d3d9caps.h)
const CAPS2_FULLSCREENGAMMA: u32 = 0x0002_0000;
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
    /// `gamma`: the ramp SetGammaRamp gave this chain (None: identity)
    SwapChain { device: u32, pp: PresentParameters, back_buffers: Vec<u32>, presents: u64, gamma: Option<Box<d3d9cmd::GammaRamp>> },
    /// `owner`: the texture whose reference count this level surface
    /// shares (0 for standalone surfaces and back buffers)
    Surface { device: u32, desc: SurfaceDesc, container: u32, owner: u32, image: crate::d3d9res::Image },
    Texture { device: u32, data: Box<crate::d3d9res::TextureData> },
    Buffer { device: u32, data: Box<crate::d3d9res::BufferData> },
    /// IDirect3DQuery9: D3DQUERYTYPE, how far it is issued, the sampled value
    Query { device: u32, ty: u32, state: QueryState, value: u64 },
    /// ID3DXConstantTable (D3DX9_43; crate::d3dx)
    ConstantTable { data: Box<crate::d3dx::Table> },
}

/// Where a query is: never issued, between D3DISSUE_BEGIN and
/// D3DISSUE_END (occlusion), or ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryState {
    Created,
    Building,
    Ended,
}

#[derive(Debug)]
pub struct Object {
    pub iface: &'static str,
    pub refs: u32,
    pub kind: Kind,
}

#[derive(Default)]
pub(crate) struct Objects {
    /// objects whose last reference went without the guest lock (L93),
    /// destroyed by the next D3D method that holds it
    deferred: Vec<(u32, Object)>,
    vtables: HashMap<&'static str, u32>,
    /// by guest address (L78: a fast hash; looked up several times a draw)
    pub(crate) live: crate::fasthash::FxMap<u32, Object>,
}

pub(crate) fn objects<R>(f: impl FnOnce(&mut Objects) -> R) -> R {
    crate::runtime::with(|rt| rt.d3d.with(f))
}

// --- the D3D lock (L93) --------------------------------------------------------------
//
// The D3D objects (and the device state in them) are behind a lock of their
// own, not the guest lock, so the render thread's ~25,000 D3D calls a frame
// do not queue behind the other threads' services. A D3D method holds the
// lock for its whole call (dispatch::Binding::Device), so D3D calls are as
// atomic against each other as they were under the guest lock; anything
// else reaching the objects takes it for each access. The hot device
// methods take only this lock: they touch the D3D objects and the backend,
// nothing else. The others (creation, Lock, Reset, the copies, D3DX) take
// the guest lock first, then this one; nothing takes them the other way
// round (threads::unlocked keeps the guest lock while this one is held).

/// A value behind the D3D lock: re-entrant on one thread, so a method can
/// reach the objects through any helper; a RefCell inside still catches a
/// nested mutable borrow.
pub(crate) struct D3dCell<T> {
    lock: std::sync::Mutex<bool>,
    free: std::sync::Condvar,
    /// the holder (a per-thread key; 0: none) and its depth (only the
    /// holder changes it)
    owner: std::sync::atomic::AtomicU64,
    depth: std::cell::Cell<u32>,
    cell: std::cell::RefCell<T>,
}

// SAFETY: `cell` and `depth` are only touched by the thread that holds the
// lock (`owner`), and taking and giving the lock goes through the mutex.
unsafe impl<T: Send> Sync for D3dCell<T> {}

impl<T: Default> Default for D3dCell<T> {
    fn default() -> Self {
        D3dCell {
            lock: std::sync::Mutex::new(false),
            free: std::sync::Condvar::new(),
            owner: std::sync::atomic::AtomicU64::new(0),
            depth: std::cell::Cell::new(0),
            cell: Default::default(),
        }
    }
}

thread_local! {
    /// This thread's key for D3dCell::owner, and the D3D locks it holds.
    static THREAD_KEY: u64 = {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    static HELD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

impl<T> D3dCell<T> {
    fn acquire(&self) {
        let me = THREAD_KEY.with(|k| *k);
        if self.owner.load(std::sync::atomic::Ordering::Acquire) != me {
            let mut locked = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            while *locked {
                locked = self.free.wait(locked).unwrap_or_else(|e| e.into_inner());
            }
            *locked = true;
            self.owner.store(me, std::sync::atomic::Ordering::Release);
        }
        self.depth.set(self.depth.get() + 1);
        HELD.with(|h| h.set(h.get() + 1));
    }

    fn release(&self) {
        HELD.with(|h| h.set(h.get() - 1));
        let d = self.depth.get() - 1;
        self.depth.set(d);
        if d == 0 {
            self.owner.store(0, std::sync::atomic::Ordering::Release);
            *self.lock.lock().unwrap_or_else(|e| e.into_inner()) = false;
            self.free.notify_one();
        }
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        struct Release<'a, T>(&'a D3dCell<T>);
        impl<T> Drop for Release<'_, T> {
            fn drop(&mut self) {
                self.0.release();
            }
        }
        self.acquire();
        let _release = Release(self);
        f(&mut self.cell.borrow_mut())
    }
}

/// Whether this thread holds the D3D lock.
pub(crate) fn holds_device_lock() -> bool {
    HELD.with(|h| h.get()) > 0
}

/// The D3D lock for one D3D method (dispatch::Binding::Device). With the
/// guest lock also held, the destructions deferred by methods without it
/// run first.
pub(crate) struct DeviceLock(std::sync::Arc<crate::runtime::Runtime>);

pub(crate) fn device_lock() -> DeviceLock {
    let rt = crate::runtime::current();
    rt.d3d.acquire();
    let lock = DeviceLock(rt);
    // (the deferred list first: asking about the guest lock takes its mutex)
    if objects(|o| !o.deferred.is_empty()) && crate::threads::holds_guest_lock() {
        destroy_deferred();
    }
    lock
}

impl Drop for DeviceLock {
    fn drop(&mut self) {
        self.0.d3d.release();
    }
}

/// Destroy the objects whose last reference went in a method without the
/// guest lock (the guest heap needs it).
fn destroy_deferred() {
    while let Some((object, x)) = objects(|o| o.deferred.pop()) {
        destroy(object, x);
    }
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
        if crate::threads::holds_guest_lock() {
            destroy(object, x);
        } else {
            // a method without the guest lock (L93): the heap blocks are
            // freed, and what the object holds released, by the next D3D
            // method that has it (the object is no longer the guest's)
            objects(|o| o.deferred.push((object, x)));
        }
    }
    left
}

/// The end of an object whose last reference went: the backend forgets it,
/// what it holds is released, its heap blocks freed (under the guest lock).
fn destroy(object: u32, x: Object) {
    {
        // the backend forgets what it keeps for the address
        if matches!(x.kind, Kind::Surface { .. } | Kind::Texture { .. } | Kind::Buffer { .. } | Kind::Query { .. }) {
            crate::d3d9draw::backend(|b| b.release(object));
        }
        let held: Vec<u32> = match x.kind {
            Kind::Device(d) => {
                let d = *d;
                d.swap_chains.into_iter().take(1).chain([d.depth_stencil, d.auto_depth]).chain(d.render_targets).chain(d.state.held())
                    .filter(|&p| p != 0).collect()
            }
            Kind::SwapChain { device, back_buffers, .. } => {
                // an additional swap chain is not counted by its device
                objects(|o| {
                    if let Some(Object { kind: Kind::Device(d), .. }) = o.live.get_mut(&device) {
                        if d.swap_chains.first() != Some(&object) {
                            d.swap_chains.retain(|&c| c != object);
                        }
                    }
                });
                back_buffers
            }
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
            Kind::Shader { .. } | Kind::Declaration { .. } | Kind::Query { .. } => vec![],
            Kind::ConstantTable { data } => {
                data.free();
                vec![]
            }
        };
        for h in held {
            release_of(h);
        }
        with_state(|s| s.heap.free(object)).expect("D3D9 object block");
    }
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
/// format, a zero count one buffer. Full-screen parameters must name one of
/// the adapter's modes (size, and the refresh rate unless it is
/// D3DPRESENT_RATE_DEFAULT) in X8R8G8B8. None if the parameters are
/// invalid.
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
    } else if !crate::d3d9::display().has(pp[0], pp[1], pp[12]) || pp[2] != D3DFMT_X8R8G8B8 {
        return None;
    }
    if pp[3] == 0 {
        pp[3] = 1;
    }
    (pp[3] <= 3).then_some(pp)
}

fn new_swap_chain(m: &mut dyn Mem, device: u32, pp: PresentParameters) -> u32 {
    let chain = new_object(m, "IDirect3DSwapChain9", Kind::SwapChain { device, pp, back_buffers: vec![], presents: 0, gamma: None });
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
                auto_depth: 0, not_reset: false,
            })));
            let chain = new_swap_chain(m, device, pp);
            with_device(device, |d| d.swap_chains.push(chain));
            bind_implicit(m, device, chain, pp);
            m.set_u32(out, device);
            D3D_OK
        }
    };
    ret(m, cpu, 28);
}

// --- IDirect3DDevice9 ------------------------------------------------------------

/// The implicit swap chain's first back buffer as render target 0 and a
/// new automatic depth-stencil surface (if the parameters ask for one) as
/// the depth-stencil: at creation and after Reset.
fn bind_implicit(m: &mut dyn Mem, device: u32, chain: u32, pp: PresentParameters) {
    let back = back_buffer_of(chain, 0).expect("a back buffer");
    add_ref_to(back); // render target 0 holds the first back buffer
    let depth = if pp[9] != 0 {
        let desc = SurfaceDesc { format: pp[10], usage: D3DUSAGE_DEPTHSTENCIL, pool: D3DPOOL_DEFAULT, multisample: pp[4], quality: pp[5], width: pp[0], height: pp[1] };
        let s = surface(m, device, desc, device);
        add_ref_to(s); // the device owns it, and it is bound
        s
    } else {
        0
    };
    with_device(device, |d| {
        d.depth_stencil = depth;
        d.auto_depth = depth;
        d.render_targets[0] = back;
    });
}

/// A device's own record.
pub struct DeviceData {
    pub creation: [u32; 4],
    /// the implicit swap chain first (the device holds a reference), then
    /// additional ones (held by the application only)
    pub swap_chains: Vec<u32>,
    pub depth_stencil: u32,
    pub render_targets: [u32; 4],
    pub state: crate::d3d9state::DeviceState,
    /// the automatic depth-stencil surface, which the device owns (it
    /// lives on when another depth-stencil surface is set)
    pub auto_depth: u32,
    /// a Reset failed: the device waits for a successful one
    pub not_reset: bool,
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

/// TestCooperativeLevel(this): the device is never lost on the headless
/// desktop; after a failed Reset it waits for another (D3DERR_DEVICENOTRESET).
fn test_cooperative_level(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = if with_device(arg(m, cpu, 0), |d| d.not_reset) { D3DERR_DEVICENOTRESET } else { D3D_OK };
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
/// GetDisplayMode(this, swap chain, D3DDISPLAYMODE*): the adapter's
/// current mode (the full-screen mode while full screen).
fn get_display_mode(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (chain, out) = (arg(m, cpu, 1), arg(m, cpu, 2));
    let count = with_device(arg(m, cpu, 0), |d| d.swap_chains.len() as u32);
    cpu.eax = if chain < count {
        crate::d3d9::write_mode(m, out, crate::d3d9::current_mode());
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
            m.set_u32(out, chain); // the caller's is the only reference
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
        Some(Object { kind: Kind::SwapChain { device, pp, back_buffers, presents, .. }, .. }) => f(*device, pp, back_buffers, presents),
        _ => panic!("{chain:#x} is not a live IDirect3DSwapChain9"),
    })
}
/// Show a swap chain's first back buffer through its gamma ramp: the ramp
/// applies to a full-screen chain only, the device's caps having
/// D3DCAPS2_FULLSCREENGAMMA and not a windowed gamma (D3D9 SetGammaRamp:
/// an unsupported mode ignores the ramp). D3DERR_DEVICELOST while the
/// device waits for a Reset.
fn present_chain(m: &mut dyn Mem, chain: u32) -> u32 {
    let device = with_chain(chain, |d, _, _, _| d);
    if with_device(device, |d| d.not_reset) {
        return D3DERR_DEVICELOST;
    }
    with_chain(chain, |_, _, _, n| *n += 1);
    let ramp = objects(|o| match o.live.get(&chain).map(|x| &x.kind) {
        Some(Kind::SwapChain { pp, gamma, .. }) if pp[8] == 0 && caps2() & CAPS2_FULLSCREENGAMMA != 0 => gamma.clone(),
        _ => None,
    });
    draw::backend(|b| b.gamma(ramp.as_deref()));
    if let Some(back) = back_buffer_of(chain, 0) {
        draw::present_back_buffer(m, back);
    }
    D3D_OK
}

/// D3DCAPS9::Caps2 of the recorded device.
fn caps2() -> u32 {
    let c = &profile().device_caps;
    u32::from_le_bytes([c[12], c[13], c[14], c[15]])
}

/// Present(this, src, dst, window, dirty, flags): a frame (headless).
fn chain_present(m: &mut dyn Mem, cpu: &mut Cpu) {
    let chain = arg(m, cpu, 0);
    cpu.eax = present_chain(m, chain);
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
        | Some(Kind::Declaration { device, .. }) | Some(Kind::Texture { device, .. }) | Some(Kind::Buffer { device, .. }) | Some(Kind::Query { device, .. }) => *device,
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
    cpu.eax = present_chain(m, chain);
    ret(m, cpu, 20);
}

/// SetGammaRamp(this, swap chain, flags, D3DGAMMARAMP*): returns nothing.
/// The ramp is kept with the chain and shown with its frames when the
/// chain is full-screen (see present_chain). D3DSGR_CALIBRATE is ignored:
/// the device has no D3DCAPS2_CANCALIBRATEGAMMA. An unknown chain is
/// ignored. Called by 0x00DA6D00 (the brightness option).
fn set_gamma_ramp(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, index, p) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 3));
    let mut ramp: d3d9cmd::GammaRamp = [[0; 256]; 3];
    for (c, channel) in ramp.iter_mut().enumerate() {
        for (k, e) in channel.iter_mut().enumerate() {
            *e = m.u16(p + 2 * (256 * c + k) as u32);
        }
    }
    if let Some(chain) = with_device(dev, |d| d.swap_chains.get(index as usize).copied()) {
        objects(|o| {
            if let Some(Object { kind: Kind::SwapChain { gamma, .. }, .. }) = o.live.get_mut(&chain) {
                *gamma = Some(Box::new(ramp));
            }
        });
    }
    ret(m, cpu, 16);
}

/// Whether `object` (of `device`) keeps Reset from succeeding: D3D9 fails a
/// Reset while D3DPOOL_DEFAULT resources other than the implicit ones are
/// alive (Microsoft Learn: IDirect3DDevice9::Reset; Wine d3d9
/// reset_enum_callback, after the device state is unbound). The implicit
/// swap chain, its back buffers and the automatic depth-stencil surface
/// block only while the application holds a reference to them.
fn blocks_reset(o: &Objects, device: u32, object: u32, chain0: u32, auto_depth: u32) -> bool {
    let Some(x) = o.live.get(&object) else { return false };
    match &x.kind {
        Kind::SwapChain { device: d, .. } if *d == device => object != chain0 || x.refs > 1,
        Kind::Surface { device: d, desc, container, owner, .. } if *d == device && *owner == 0 && desc.pool == D3DPOOL_DEFAULT => {
            let implicit = *container == chain0 || object == auto_depth;
            !implicit || x.refs > 1
        }
        Kind::Texture { device: d, data } if *d == device => data.pool == D3DPOOL_DEFAULT,
        Kind::Buffer { device: d, data } if *d == device => data.pool == D3DPOOL_DEFAULT,
        _ => false,
    }
}

/// Reset(this, D3DPRESENT_PARAMETERS*): invalid parameters fail without
/// touching the device. Otherwise the device state goes back to a fresh
/// device's (every binding released, Wine's state_unbind_resources); if
/// D3DPOOL_DEFAULT resources are still alive the call fails and the device
/// waits for another Reset (TestCooperativeLevel answers
/// D3DERR_DEVICENOTRESET); else the implicit swap chain's back buffers and
/// automatic depth-stencil surface are rebuilt from the completed
/// parameters (written back, as CreateDevice does). Reached from
/// 0x00DAF7C0 (display mode changes).
fn reset(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, pp_ptr) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let focus = with_device(dev, |d| d.creation[2]);
    cpu.eax = match complete(read_pp(m, pp_ptr), focus) {
        None => D3DERR_INVALIDCALL,
        Some(pp) => {
            // unbind everything (a fresh state holds nothing)
            let held: Vec<u32> = with_device(dev, |d| {
                let mut h = d.state.held();
                h.extend(std::mem::take(&mut d.render_targets));
                h.push(std::mem::take(&mut d.depth_stencil));
                d.state = crate::d3d9state::DeviceState::new(false, 0, 0);
                h.retain(|&p| p != 0);
                h
            });
            for h in held {
                release_of(h);
            }
            let (chain0, auto_depth) = with_device(dev, |d| (d.swap_chains[0], d.auto_depth));
            let blocked = objects(|o| o.live.keys().any(|&k| blocks_reset(o, dev, k, chain0, auto_depth)));
            if blocked {
                with_device(dev, |d| d.not_reset = true);
                D3DERR_INVALIDCALL
            } else {
                write_pp(m, pp_ptr, &pp);
                // the old implicit surfaces go (the chain and the device held the last references)
                let old = with_chain(chain0, |_, old_pp, backs, _| {
                    *old_pp = pp;
                    std::mem::take(backs)
                });
                for b in old.into_iter().chain([auto_depth]).filter(|&b| b != 0) {
                    release_of(b);
                }
                let backs: Vec<u32> = (0..pp[3]).map(|_| {
                    let desc = SurfaceDesc { format: pp[2], usage: D3DUSAGE_RENDERTARGET, pool: D3DPOOL_DEFAULT, multisample: pp[4], quality: pp[5], width: pp[0], height: pp[1] };
                    surface(m, dev, desc, chain0)
                }).collect();
                with_chain(chain0, |_, _, b, _| *b = backs);
                with_device(dev, |d| {
                    d.state = crate::d3d9state::DeviceState::new(pp[9] != 0, pp[0], pp[1]);
                    d.not_reset = false;
                });
                bind_implicit(m, dev, chain0, pp);
                D3D_OK
            }
        }
    };
    ret(m, cpu, 8);
}

/// How the game presents: its first device's implicit swap chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presentation {
    /// The back buffer: the window's client area when windowed, the
    /// display mode when full screen.
    pub width: u32,
    pub height: u32,
    pub windowed: bool,
    /// FullScreen_RefreshRateInHz (0 when windowed, or the adapter's default).
    pub refresh: u32,
    /// PresentationInterval: D3DPRESENT_INTERVAL_IMMEDIATE (0x80000000) for
    /// VSYNC=OFF, D3DPRESENT_INTERVAL_ONE (1) for VSYNC=ON (0x00DB1480, the
    /// table at 0x01429878).
    pub interval: u32,
}

/// The game's presentation, once it has a device: what a host window
/// follows (size, windowed or full screen, refresh rate, vsync).
pub fn presentation() -> Option<Presentation> {
    objects(|o| {
        let device = o.live.iter().filter_map(|(&k, v)| match &v.kind { Kind::Device(d) => Some((k, d.swap_chains.first().copied()?)), _ => None }).min()?;
        match &o.live.get(&device.1)?.kind {
            Kind::SwapChain { pp, .. } => Some(Presentation { width: pp[0], height: pp[1], windowed: pp[8] != 0, refresh: pp[12], interval: pp[13] }),
            _ => None,
        }
    })
}

thread_local! {
    static PRESENTED: std::cell::Cell<Option<Presentation>> = const { std::cell::Cell::new(None) };
}

/// The presentation of the frame this thread is presenting: for a
/// presentation host's sink, which runs inside Present without the guest
/// lock in parallel mode (L64), so it must not read the session's state
/// ([`presentation`]) itself.
pub fn presented() -> Option<Presentation> {
    PRESENTED.with(std::cell::Cell::get)
}

pub(crate) fn set_presented(p: Option<Presentation>) {
    PRESENTED.with(|c| c.set(p));
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
            (16, reset), (17, present), (18, get_back_buffer), (21, set_gamma_ramp), (23, res::create_texture), (24, res::create_volume_texture),
            (25, res::create_cube_texture), (26, res::create_vertex_buffer), (27, res::create_index_buffer),
            (28, res::create_render_target), (29, res::create_depth_stencil_surface),
            (30, cp::update_surface), (31, cp::update_texture), (32, cp::get_render_target_data),
            (34, cp::stretch_rect), (35, cp::color_fill), (118, cp::create_query),
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
        name: "IDirect3DQuery9",
        slots: 8,
        methods: &[
            (0, query_interface), (1, add_ref), (2, release), (3, get_device), (4, cp::query_get_type),
            (5, cp::query_get_data_size), (6, cp::query_issue), (7, cp::query_get_data),
        ],
        names: &["QueryInterface", "AddRef", "Release", "GetDevice", "GetType", "GetDataSize", "Issue", "GetData"],
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

/// The IDirect3DDevice9 methods that take no guest lock (L93): each was
/// audited to touch only the D3D objects (a last Release is deferred, see
/// release_of), guest memory and the backend, never the guest heap, the
/// display, windows or guest code. AddRef, Release, TestCooperativeLevel,
/// Present, Get/SetRenderTarget, Get/SetDepthStencilSurface, Begin/EndScene,
/// Clear, Set/GetTransform, Set/GetViewport, Set/GetRenderState,
/// Get/SetTexture, Get/SetTextureStageState, Get/SetSamplerState,
/// Set/GetScissorRect, the four draws, Set/GetVertexDeclaration,
/// Set/GetFVF, Set/GetVertexShader, the vertex constants, Set/Get
/// StreamSource(Freq), Set/GetIndices, Set/GetPixelShader, the pixel
/// constants.
const DEVICE_ONLY_SLOTS: &[u32] = &[
    1, 2, 3, 17, 37, 38, 39, 40, 41, 42, 43, 44, 45, 47, 48, 57, 58, 64, 65, 66, 67, 68, 69, 75, 76,
    81, 82, 83, 84, 87, 88, 89, 90, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105,
    107, 108, 109, 110, 111, 112, 113, 114,
];

/// Build the vtables (before the registry is frozen); objects are created
/// at run time.
pub fn install(m: &mut dyn Mem, registry: &mut Registry, tokens: &mut Tokens) -> Result<(), Conflict> {
    objects(|o| *o = Objects::default());
    for spec in INTERFACES.iter().chain(crate::d3dx::INTERFACES) {
        let v = interfaces::vtable(m, registry, tokens, spec)?;
        // L93: every D3D method holds the D3D lock; the hot device methods
        // that touch only the D3D objects and the backend take no guest lock
        for &(slot, f) in spec.methods {
            let only_d3d = spec.name == "IDirect3DDevice9" && DEVICE_ONLY_SLOTS.contains(&slot)
                || spec.name == "IDirect3DSwapChain9" && slot == 3;
            registry.replace(m.u32(v + 4 * slot), crate::dispatch::Binding::Device(f, !only_d3d));
        }
        // (L82 bound Clear and the two draws as ServiceTail, the guest lock
        // given up for their backend tail; L93 runs them without it)
        objects(|o| o.vtables.insert(spec.name, v));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GuestMemory, READ, WRITE};
    use crate::session::{Outcome, Session};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;

    const SCRATCH: u32 = 0x7EFF_0000;
    const PP: u32 = SCRATCH + 0x200;
    const OUT: u32 = SCRATCH + 0x300;

    fn token(s: &Session, name: &str) -> u32 {
        s.imports.iter().find(|i| i.full_name() == name).unwrap().token
    }

    fn call(s: &mut Session, target: u32, args: &[u32]) -> u32 {
        match s.call(target, 0, args) {
            Outcome::Returned(c) => c.eax,
            other => panic!("{other:?}"),
        }
    }

    fn com(s: &mut Session, object: u32, slot: u32, args: &[u32]) -> u32 {
        let target = s.mem.u32(s.mem.u32(object) + slot * 4);
        call(s, target, &[&[object], args].concat())
    }

    /// A guest thread's COM calls, through the registry on its own stack.
    struct Caller {
        r: Arc<crate::dispatch::Registry>,
        m: GuestMemory,
        top: u32,
        out: u32,
    }

    impl Caller {
        fn com(&mut self, object: u32, slot: u32, args: &[u32]) -> u32 {
            let target = self.m.u32(self.m.u32(object) + 4 * slot);
            let mut c = Cpu::default();
            c.esp = self.top;
            for a in [object].iter().chain(args).rev() {
                c.esp -= 4;
                self.m.set_u32(c.esp, *a);
            }
            c.esp -= 4;
            self.m.set_u32(c.esp, 0xFFFF_0000);
            self.r.call(&mut self.m, &mut c, target).unwrap();
            c.eax
        }
    }

    /// L93: a render thread binds and draws (D3D lock only) while a loader
    /// thread creates textures and drops its own references (guest + D3D
    /// lock) and a third thread keeps taking the guest lock. The render
    /// thread's SetTexture makes the last release of each texture without
    /// the guest lock, so its destruction is deferred and done by the
    /// loader's next creation. 300 textures: no panic, every draw
    /// recorded, every texture destroyed, nothing left deferred.
    #[test]
    fn a_render_thread_draws_while_another_creates_and_releases_textures() {
        let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
        if ddexe::ExeFile::open(&path).is_err() {
            eprintln!("skipped: no installed game image");
            return;
        }
        let mut s = Session::new(&path).unwrap();
        crate::threads::set_parallel(true);
        let name = SCRATCH + 0x100;
        s.mem.set_u16(name, b'W' as u16);
        s.mem.set_u32(SCRATCH + 8, token(&s, "USER32.dll!DefWindowProcW"));
        s.mem.set_u32(SCRATCH + 40, name);
        let f = token(&s, "USER32.dll!RegisterClassExW");
        call(&mut s, f, &[SCRATCH]);
        let f = token(&s, "USER32.dll!CreateWindowExW");
        let hwnd = call(&mut s, f, &[0, name, 0, 0x00CA_0000, 0, 0, 70, 93, 0, 0, 0x0040_0000, 0]);
        let f = token(&s, "d3d9.dll!Direct3DCreate9");
        let d3d = call(&mut s, f, &[32]);
        for (k, v) in [64, 64, 21, 1, 0, 0, 1, hwnd, 1, 0, 0, 0, 0, 0].into_iter().enumerate() {
            s.mem.set_u32(PP + 4 * k as u32, v);
        }
        assert_eq!(com(&mut s, d3d, 16, &[0, 1, hwnd, 0x46, PP, OUT]), 0);
        let dev = s.mem.u32(OUT);
        assert_eq!(com(&mut s, dev, 41, &[]), 0); // BeginScene
        let draws_before = crate::d3d9draw::recorded(|r| r.draws.len()).unwrap();
        let last_error = token(&s, "KERNEL32.dll!GetLastError");

        let textures = 300u32;
        let next = Arc::new(AtomicU32::new(0));
        let bound = Arc::new(AtomicU32::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let rt = crate::runtime::current();
        let draws = Arc::new(AtomicU32::new(0));
        crate::threads::without_lock(|| {
            let spawn = |k: u32, body: Box<dyn FnOnce(&mut Caller) + Send>| {
                let (rt, r, mut m) = (rt.clone(), s.registry.clone(), s.mem.share());
                std::thread::spawn(move || {
                    crate::runtime::enter(rt);
                    crate::threads::set_test_tid(0x6000 + 4 * k);
                    let stack = 0x0500_0000 + k * 0x1_0000;
                    m.map(stack, 0x1_0000, READ | WRITE).unwrap();
                    let mut caller = Caller { r, m, top: stack + 0xF000, out: stack + 0xF800 };
                    body(&mut caller);
                })
            };
            let (next1, bound1, done1, draws1) = (next.clone(), bound.clone(), done.clone(), draws.clone());
            let render = spawn(0, Box::new(move |g: &mut Caller| {
                let mut current = 0;
                let mut k = 0u32;
                while !done1.load(Ordering::SeqCst) || next1.load(Ordering::SeqCst) != current {
                    let t = next1.load(Ordering::SeqCst);
                    if t != current {
                        assert_eq!(g.com(dev, 65, &[0, t]), 0); // SetTexture
                        current = t;
                        bound1.store(t, Ordering::SeqCst);
                    }
                    assert_eq!(g.com(dev, 81, &[4, 0, 1]), 0); // DrawPrimitive
                    draws1.fetch_add(1, Ordering::SeqCst);
                    k += 1;
                    if k % 100 == 0 {
                        assert_eq!(g.com(dev, 17, &[0, 0, 0, 0]), 0); // Present
                    }
                }
                assert_eq!(g.com(dev, 65, &[0, 0]), 0); // unbind the last texture
            }));
            let (next2, bound2, done2) = (next.clone(), bound.clone(), done.clone());
            let loader = spawn(1, Box::new(move |g: &mut Caller| {
                for _ in 0..textures {
                    let out = g.out;
                    assert_eq!(g.com(dev, 23, &[16, 16, 1, 0, 21, 1, out, 0]), 0); // CreateTexture
                    let t = g.m.u32(out);
                    next2.store(t, Ordering::SeqCst);
                    while bound2.load(Ordering::SeqCst) != t {
                        std::thread::yield_now();
                    }
                    assert_eq!(g.com(t, 2, &[]), 1, "the device holds the bound texture"); // Release
                }
                done2.store(true, Ordering::SeqCst);
            }));
            let done3 = done.clone();
            let traffic = spawn(2, Box::new(move |g: &mut Caller| {
                while !done3.load(Ordering::SeqCst) {
                    let mut c = Cpu::default();
                    c.esp = g.top;
                    c.esp -= 4;
                    g.m.set_u32(c.esp, 0xFFFF_0000);
                    g.r.call(&mut g.m, &mut c, last_error).unwrap();
                }
            }));
            for t in [render, loader, traffic] {
                t.join().unwrap();
            }
        });
        // a method holding the guest lock destroys what is still deferred
        assert_eq!(com(&mut s, dev, 4, &[]) > 0, true); // GetAvailableTextureMem
        let (textures_live, deferred) = objects(|o| {
            (o.live.values().filter(|x| matches!(x.kind, Kind::Texture { .. })).count(), o.deferred.len())
        });
        let recorded = crate::d3d9draw::recorded(|r| r.draws.len()).unwrap() - draws_before;
        eprintln!("{} draws, {textures} textures created and destroyed", draws.load(Ordering::SeqCst));
        // (the default recorder keeps the first 10,000 draws)
        assert_eq!(recorded as u32, draws.load(Ordering::SeqCst).min(10_000 - draws_before as u32), "every draw recorded");
        assert_eq!((textures_live, deferred), (0, 0), "every texture destroyed, nothing left deferred");
    }
}
