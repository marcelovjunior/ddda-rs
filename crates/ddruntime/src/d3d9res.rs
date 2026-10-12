//! Direct3D 9 resources (docs/RUNTIME.md, "Graphics backend design", step
//! 2): textures (2D, cube, volume, with mip chains), standalone surfaces
//! (render targets, depth-stencil, offscreen plain), vertex and index
//! buffers.
//!
//! Every image (a texture level or cube face level, a surface, a volume
//! level) and every buffer has a shadow in guest memory, allocated on first
//! lock: Lock returns a pointer into it, exactly what the game writes
//! through, and Unlock marks it dirty for the backend's upload. Sizes,
//! pitches and lock offsets follow D3D9's layouts (DXT/ATI formats in 4x4
//! blocks). Creation checks the format against the recorded driver's
//! CheckDeviceFormat answers. The lock rules D3D9 enforces are kept:
//! DEFAULT-pool textures lock only when DYNAMIC, render targets only when
//! created lockable, an image is not locked twice.
//!
//! A texture's level surfaces share the texture's reference count
//! (AddRef/Release on them count on the texture), as in D3D9.
use crate::d3d9::{profile, D3DERR_INVALIDCALL, D3D_OK};
use crate::d3d9dev::{hand_out, new_object, objects, Kind, SurfaceDesc};
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

pub const D3DOK_NOAUTOGEN: u32 = 0x0876_086F;
const USAGE_RENDERTARGET: u32 = 0x1;
const USAGE_DEPTHSTENCIL: u32 = 0x2;
const USAGE_DYNAMIC: u32 = 0x200;
const USAGE_AUTOGENMIPMAP: u32 = 0x400;
const POOL_DEFAULT: u32 = 0;
const POOL_SCRATCH: u32 = 3;
pub const RTYPE_SURFACE: u32 = 1;
pub const RTYPE_VOLUME: u32 = 2;
pub const RTYPE_TEXTURE: u32 = 3;
pub const RTYPE_VOLUMETEXTURE: u32 = 4;
pub const RTYPE_CUBETEXTURE: u32 = 5;
pub const RTYPE_VERTEXBUFFER: u32 = 6;
pub const RTYPE_INDEXBUFFER: u32 = 7;
const FMT_VERTEXDATA: u32 = 100;

fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

/// (block size in pixels, bytes per block) of a format, None if unknown.
pub fn layout(format: u32) -> Option<(u32, u32)> {
    Some(match format {
        21 | 22 | 31 | 32 | 33 | 34 | 35 | 62 | 63 | 64 | 67 | 71 | 75 | 77 | 79 | 82 | 83 | 84 | 102 | 112 | 114 | 117 => (1, 4),
        20 => (1, 3),
        23 | 24 | 25 | 26 | 29 | 30 | 40 | 51 | 60 | 61 | 70 | 73 | 80 | 81 | 101 | 111 => (1, 2),
        27 | 28 | 41 | 50 | 52 | 85 => (1, 1),
        36 | 110 | 113 | 115 => (1, 8),
        116 => (1, 16),
        f if f == fourcc(b"DXT1") || f == fourcc(b"ATI1") => (4, 8),
        f if [b"DXT2", b"DXT3", b"DXT4", b"DXT5", b"ATI2"].iter().any(|c| f == fourcc(c)) => (4, 16),
        f if [b"INTZ", b"RAWZ", b"DF24"].iter().any(|c| f == fourcc(c)) => (1, 4),
        f if f == fourcc(b"DF16") => (1, 2),
        f if f == fourcc(b"NULL") => (1, 0),
        _ => return None,
    })
}

/// The storage of one image or buffer.
#[derive(Clone, Debug, Default)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// bytes per row (of blocks for block formats) and per slice
    pub pitch: u32,
    pub slice: u32,
    pub size: u32,
    pub block: u32,
    pub block_bytes: u32,
    /// guest-memory shadow (0 until first locked)
    pub shadow: u32,
    pub locks: u32,
    pub dirty: bool,
}

impl Image {
    pub fn new(format: u32, width: u32, height: u32, depth: u32) -> Option<Image> {
        let (block, bytes) = layout(format)?;
        let pitch = width.div_ceil(block) * bytes;
        let slice = pitch * height.div_ceil(block);
        Some(Image { width, height, depth, pitch, slice, size: slice * depth, block, block_bytes: bytes, ..Default::default() })
    }
    pub fn bytes(size: u32) -> Image {
        Image { width: size, height: 1, depth: 1, pitch: size, slice: size, size, block: 1, block_bytes: 1, ..Default::default() }
    }
    /// The shadow, allocated (zeroed) on first use.
    fn shadow(&mut self, m: &mut dyn Mem) -> u32 {
        if self.shadow == 0 {
            let p = with_state(|s| s.heap.alloc(self.size.max(1))).expect("guest heap for a D3D9 resource");
            crate::services::zero(m, p, self.size);
            self.shadow = p;
        }
        self.shadow
    }
    /// Lock (a rectangle or box of) the image: the address of its first byte.
    fn lock(&mut self, m: &mut dyn Mem, left: u32, top: u32, front: u32) -> Option<u32> {
        if self.locks > 0 {
            return None;
        }
        self.locks = 1;
        let base = self.shadow(m);
        Some(base + front * self.slice + (top / self.block) * self.pitch + (left / self.block) * self.block_bytes)
    }
    fn unlock(&mut self) -> bool {
        if self.locks == 0 {
            return false;
        }
        self.locks = 0;
        self.dirty = true;
        true
    }
    pub(crate) fn free(&self) {
        if self.shadow != 0 {
            with_state(|s| s.heap.free(self.shadow)).expect("D3D9 resource shadow");
        }
    }
}

pub use d3d9cmd::TexKind;

/// The texture record (Kind::Texture).
#[derive(Debug)]
pub struct TextureData {
    pub kind: TexKind,
    pub format: u32,
    pub usage: u32,
    pub pool: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub levels: u32,
    /// level surfaces (flat: by level; cube: face * levels + level)
    pub surfaces: Vec<u32>,
    /// volume levels
    pub volumes: Vec<Image>,
    pub lod: u32,
    pub autogen_filter: u32,
}

/// The buffer record (Kind::Buffer).
#[derive(Debug)]
pub struct BufferData {
    pub index: bool,
    pub usage: u32,
    /// FVF for vertex buffers, D3DFMT_INDEX16/32 for index buffers
    pub format: u32,
    pub pool: u32,
    pub image: Image,
}

fn full_chain(w: u32, h: u32, d: u32) -> u32 {
    32 - w.max(h).max(d).max(1).leading_zeros()
}

/// Whether the recorded driver accepts `format` for `usage` and `rtype`.
fn supported(usage: u32, rtype: u32, format: u32) -> bool {
    let base = usage & (USAGE_RENDERTARGET | USAGE_DEPTHSTENCIL | USAGE_DYNAMIC | USAGE_AUTOGENMIPMAP);
    let hr = profile().format(base, rtype, format);
    hr == D3D_OK || hr == D3DOK_NOAUTOGEN
}

fn surface_object(m: &mut dyn Mem, device: u32, desc: SurfaceDesc, container: u32, owner: u32, image: Image) -> u32 {
    new_object(m, "IDirect3DSurface9", Kind::Surface { device, desc, container, owner, image })
}

fn create_texture_object(m: &mut dyn Mem, device: u32, kind: TexKind, dims: (u32, u32, u32), levels: u32, usage: u32, format: u32, pool: u32) -> Result<u32, u32> {
    let (w, h, d) = dims;
    let rtype = match kind {
        TexKind::Flat => RTYPE_TEXTURE,
        TexKind::Cube => RTYPE_CUBETEXTURE,
        TexKind::Volume => RTYPE_VOLUMETEXTURE,
    };
    if w == 0 || h == 0 || d == 0 || layout(format).is_none() || !supported(usage, rtype, format) {
        return Err(D3DERR_INVALIDCALL);
    }
    let chain = full_chain(w, h, d);
    let levels = if usage & USAGE_AUTOGENMIPMAP != 0 { 1 } else if levels == 0 { chain } else { levels.min(chain) };
    let iface = match kind {
        TexKind::Flat => "IDirect3DTexture9",
        TexKind::Cube => "IDirect3DCubeTexture9",
        TexKind::Volume => "IDirect3DVolumeTexture9",
    };
    let tex = new_object(m, iface, Kind::Texture { device, data: Box::new(TextureData {
        kind, format, usage, pool, width: w, height: h, depth: d, levels, surfaces: vec![], volumes: vec![], lod: 0, autogen_filter: 2,
    }) });
    let faces = if kind == TexKind::Cube { 6 } else { 1 };
    let mut surfaces = Vec::new();
    let mut volumes = Vec::new();
    for _face in 0..faces {
        for l in 0..levels {
            let (lw, lh, ld) = ((w >> l).max(1), (h >> l).max(1), (d >> l).max(1));
            if kind == TexKind::Volume {
                volumes.push(Image::new(format, lw, lh, ld).unwrap());
            } else {
                let desc = SurfaceDesc { format, usage, pool, multisample: 0, quality: 0, width: lw, height: lh };
                surfaces.push(surface_object(m, device, desc, tex, tex, Image::new(format, lw, lh, 1).unwrap()));
            }
        }
    }
    texture(tex, |t| {
        t.surfaces = surfaces;
        t.volumes = volumes;
    });
    Ok(tex)
}

pub(crate) fn texture<R>(tex: u32, f: impl FnOnce(&mut TextureData) -> R) -> R {
    objects(|o| match o.live.get_mut(&tex).map(|x| &mut x.kind) {
        Some(Kind::Texture { data, .. }) => f(data),
        _ => panic!("{tex:#x} is not a live texture"),
    })
}

fn surface_image<R>(surf: u32, f: impl FnOnce(&SurfaceDesc, &mut Image) -> R) -> R {
    objects(|o| match o.live.get_mut(&surf).map(|x| &mut x.kind) {
        Some(Kind::Surface { desc, image, .. }) => f(desc, image),
        _ => panic!("{surf:#x} is not a live surface"),
    })
}

// --- device methods: creation --------------------------------------------------

/// CreateTexture(this, w, h, levels, usage, format, pool, IDirect3DTexture9**, shared).
pub fn create_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..9).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = match create_texture_object(m, a[0], TexKind::Flat, (a[1], a[2], 1), a[3], a[4], a[5], a[6]) {
        Ok(t) => {
            m.set_u32(a[7], t);
            D3D_OK
        }
        Err(e) => e,
    };
    ret(m, cpu, 36);
}
/// CreateVolumeTexture(this, w, h, d, levels, usage, format, pool, IDirect3DVolumeTexture9**, shared).
pub fn create_volume_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..10).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = match create_texture_object(m, a[0], TexKind::Volume, (a[1], a[2], a[3]), a[4], a[5], a[6], a[7]) {
        Ok(t) => {
            m.set_u32(a[8], t);
            D3D_OK
        }
        Err(e) => e,
    };
    ret(m, cpu, 40);
}
/// CreateCubeTexture(this, edge, levels, usage, format, pool, IDirect3DCubeTexture9**, shared).
pub fn create_cube_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..8).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = match create_texture_object(m, a[0], TexKind::Cube, (a[1], a[1], 1), a[2], a[3], a[4], a[5]) {
        Ok(t) => {
            m.set_u32(a[6], t);
            D3D_OK
        }
        Err(e) => e,
    };
    ret(m, cpu, 32);
}

fn create_buffer(m: &mut dyn Mem, cpu: &mut Cpu, index: bool) {
    // (this, length, usage, fvf | format, pool, buffer**, shared)
    let a: Vec<u32> = (0..7).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = if a[1] == 0 || (index && !matches!(a[3], 101 | 102)) {
        D3DERR_INVALIDCALL
    } else {
        let iface = if index { "IDirect3DIndexBuffer9" } else { "IDirect3DVertexBuffer9" };
        let b = new_object(m, iface, Kind::Buffer {
            device: a[0],
            data: Box::new(BufferData { index, usage: a[2], format: a[3], pool: a[4], image: Image::bytes(a[1]) }),
        });
        m.set_u32(a[5], b);
        D3D_OK
    };
    ret(m, cpu, 28);
}
/// CreateVertexBuffer(this, length, usage, fvf, pool, IDirect3DVertexBuffer9**, shared).
pub fn create_vertex_buffer(m: &mut dyn Mem, cpu: &mut Cpu) { create_buffer(m, cpu, false) }
/// CreateIndexBuffer(this, length, usage, format, pool, IDirect3DIndexBuffer9**, shared).
pub fn create_index_buffer(m: &mut dyn Mem, cpu: &mut Cpu) { create_buffer(m, cpu, true) }

fn create_surface(m: &mut dyn Mem, cpu: &mut Cpu, usage: u32, rtype_usage: u32) {
    // CreateRenderTarget / CreateDepthStencilSurface:
    // (this, w, h, format, multisample, quality, lockable | discard, surface**, shared)
    let a: Vec<u32> = (0..9).map(|k| arg(m, cpu, k)).collect();
    // a multisampled surface needs the type offered for the format and a
    // quality below its level count (CheckDeviceMultiSampleType, the same
    // answers windowed and full-screen in the recorded profile); a
    // multisampled render target cannot be lockable (Microsoft Learn,
    // IDirect3DDevice9::CreateRenderTarget)
    let multisample_ok = a[4] == 0 || {
        let (hr, levels) = crate::d3d9::profile().multisample(a[3], 1, a[4]);
        hr == 0 && a[5] < levels && !(usage == USAGE_RENDERTARGET && a[6] != 0)
    };
    cpu.eax = match Image::new(a[3], a[1], a[2], 1) {
        Some(image) if a[1] > 0 && a[2] > 0 && multisample_ok && supported(rtype_usage, RTYPE_SURFACE, a[3]) => {
            // a render target is lockable only when created lockable
            let usage = if usage == USAGE_RENDERTARGET && a[6] == 0 { usage | NOT_LOCKABLE } else { usage };
            let desc = SurfaceDesc { format: a[3], usage, pool: POOL_DEFAULT, multisample: a[4], quality: a[5], width: a[1], height: a[2] };
            let s = surface_object(m, a[0], desc, a[0], 0, image);
            m.set_u32(a[7], s);
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 36);
}
/// Marks a surface created without lockable access (kept in its usage bits
/// above any D3DUSAGE value; GetDesc masks it out).
pub const NOT_LOCKABLE: u32 = 0x8000_0000;
pub fn create_render_target(m: &mut dyn Mem, cpu: &mut Cpu) { create_surface(m, cpu, USAGE_RENDERTARGET, USAGE_RENDERTARGET) }
pub fn create_depth_stencil_surface(m: &mut dyn Mem, cpu: &mut Cpu) { create_surface(m, cpu, USAGE_DEPTHSTENCIL, USAGE_DEPTHSTENCIL) }

/// CreateOffscreenPlainSurface(this, w, h, format, pool, surface**, shared).
pub fn create_offscreen_plain_surface(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..7).map(|k| arg(m, cpu, k)).collect();
    cpu.eax = match Image::new(a[3], a[1], a[2], 1) {
        Some(image) if a[1] > 0 && a[2] > 0 => {
            let desc = SurfaceDesc { format: a[3], usage: 0, pool: a[4], multisample: 0, quality: 0, width: a[1], height: a[2] };
            let s = surface_object(m, a[0], desc, a[0], 0, image);
            m.set_u32(a[5], s);
            D3D_OK
        }
        _ => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 28);
}

// --- locking -------------------------------------------------------------------

/// Whether an image of this usage/pool may be locked (D3D9's rules).
fn lockable(usage: u32, pool: u32) -> bool {
    if usage & NOT_LOCKABLE != 0 || usage & USAGE_DEPTHSTENCIL != 0 {
        return false;
    }
    pool != POOL_DEFAULT || usage & USAGE_DYNAMIC != 0 || usage & USAGE_RENDERTARGET != 0 || pool == POOL_SCRATCH
}

/// Write a D3DLOCKED_RECT {Pitch, pBits} for a lock of `image` at `rect`.
fn lock_rect(m: &mut dyn Mem, image: &mut Image, locked: u32, rect: u32) -> u32 {
    let (left, top) = if rect != 0 { (m.u32(rect), m.u32(rect + 4)) } else { (0, 0) };
    match image.lock(m, left, top, 0) {
        Some(p) => {
            m.set_u32(locked, image.pitch);
            m.set_u32(locked + 4, p);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    }
}

/// IDirect3DSurface9::LockRect(this, D3DLOCKED_RECT*, RECT*, flags).
pub fn surface_lock_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, locked, rect) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let (usage, pool) = surface_image(this, |d, _| (d.usage, d.pool));
    cpu.eax = if !lockable(usage, pool) {
        D3DERR_INVALIDCALL
    } else {
        let mut img = surface_image(this, |_, i| std::mem::take(i));
        let r = lock_rect(m, &mut img, locked, rect);
        surface_image(this, |_, i| *i = img);
        r
    };
    ret(m, cpu, 16);
}
/// IDirect3DSurface9::UnlockRect(this).
pub fn surface_unlock_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = arg(m, cpu, 0);
    cpu.eax = if surface_image(this, |_, i| i.unlock()) { D3D_OK } else { D3DERR_INVALIDCALL };
    ret(m, cpu, 4);
}

/// The surface for (face, level) of a flat or cube texture.
fn level_surface(tex: u32, face: u32, level: u32) -> Option<u32> {
    texture(tex, |t| {
        if level >= t.levels || face >= 6 || (t.kind != TexKind::Cube && face > 0) {
            return None;
        }
        t.surfaces.get((face * t.levels + level) as usize).copied()
    })
}

fn texture_lock(m: &mut dyn Mem, tex: u32, face: u32, level: u32, locked: u32, rect: u32) -> u32 {
    let (usage, pool) = texture(tex, |t| (t.usage, t.pool));
    match level_surface(tex, face, level) {
        Some(s) if lockable(usage, pool) => {
            let mut img = surface_image(s, |_, i| std::mem::take(i));
            let r = lock_rect(m, &mut img, locked, rect);
            surface_image(s, |_, i| *i = img);
            r
        }
        _ => D3DERR_INVALIDCALL,
    }
}
fn texture_unlock(tex: u32, face: u32, level: u32) -> u32 {
    match level_surface(tex, face, level) {
        Some(s) if surface_image(s, |_, i| i.unlock()) => D3D_OK,
        _ => D3DERR_INVALIDCALL,
    }
}

/// IDirect3DTexture9::LockRect(this, level, D3DLOCKED_RECT*, RECT*, flags).
pub fn texture_lock_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level, locked, rect) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = texture_lock(m, this, 0, level, locked, rect);
    ret(m, cpu, 20);
}
/// IDirect3DTexture9::UnlockRect(this, level).
pub fn texture_unlock_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = texture_unlock(this, 0, level);
    ret(m, cpu, 8);
}
/// IDirect3DCubeTexture9::LockRect(this, face, level, D3DLOCKED_RECT*, RECT*, flags).
pub fn cube_lock_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, face, level, locked, rect) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    cpu.eax = texture_lock(m, this, face, level, locked, rect);
    ret(m, cpu, 24);
}
/// IDirect3DCubeTexture9::UnlockRect(this, face, level).
pub fn cube_unlock_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, face, level) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = texture_unlock(this, face, level);
    ret(m, cpu, 12);
}
/// IDirect3DVolumeTexture9::LockBox(this, level, D3DLOCKED_BOX*, D3DBOX*, flags):
/// {RowPitch, SlicePitch, pBits}.
pub fn volume_lock_box(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level, locked, boxp) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let (left, top, front) = if boxp != 0 { (m.u32(boxp), m.u32(boxp + 4), m.u32(boxp + 16)) } else { (0, 0, 0) };
    let (usage, pool, levels) = texture(this, |t| (t.usage, t.pool, t.levels));
    cpu.eax = if level >= levels || !lockable(usage, pool) {
        D3DERR_INVALIDCALL
    } else {
        let mut img = texture(this, |t| std::mem::take(&mut t.volumes[level as usize]));
        let r = match img.lock(m, left, top, front) {
            Some(p) => {
                m.set_u32(locked, img.pitch);
                m.set_u32(locked + 4, img.slice);
                m.set_u32(locked + 8, p);
                D3D_OK
            }
            None => D3DERR_INVALIDCALL,
        };
        texture(this, |t| t.volumes[level as usize] = img);
        r
    };
    ret(m, cpu, 20);
}
/// IDirect3DVolumeTexture9::UnlockBox(this, level).
pub fn volume_unlock_box(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = texture(this, |t| match t.volumes.get_mut(level as usize) {
        Some(i) => if i.unlock() { D3D_OK } else { D3DERR_INVALIDCALL },
        None => D3DERR_INVALIDCALL,
    });
    ret(m, cpu, 8);
}

// --- texture queries -------------------------------------------------------------

/// GetLevelCount(this).
pub fn texture_get_level_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = texture(arg(m, cpu, 0), |t| t.levels);
    ret(m, cpu, 4);
}
/// GetType(this) for any resource.
pub fn resource_get_type(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = arg(m, cpu, 0);
    cpu.eax = objects(|o| match o.live.get(&this).map(|x| &x.kind) {
        Some(Kind::Texture { data: t, .. }) => match t.kind {
            TexKind::Flat => RTYPE_TEXTURE,
            TexKind::Cube => RTYPE_CUBETEXTURE,
            TexKind::Volume => RTYPE_VOLUMETEXTURE,
        },
        Some(Kind::Surface { .. }) => RTYPE_SURFACE,
        Some(Kind::Buffer { data, .. }) => if data.index { RTYPE_INDEXBUFFER } else { RTYPE_VERTEXBUFFER },
        _ => panic!("GetType on {this:#x}"),
    });
    ret(m, cpu, 4);
}
/// SetPriority(this, priority) -> previous; GetPriority: 0 (no residency
/// management on this device).
pub fn resource_set_priority(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 8);
}
pub fn resource_get_priority(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 4);
}
/// PreLoad(this): the backend uploads on use.
pub fn resource_preload(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 4);
}
/// SetLOD(this, lod) -> previous (managed textures only; others 0).
pub fn texture_set_lod(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, lod) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = texture(this, |t| if t.pool == 1 { std::mem::replace(&mut t.lod, lod.min(t.levels - 1)) } else { 0 });
    ret(m, cpu, 8);
}
pub fn texture_get_lod(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = texture(arg(m, cpu, 0), |t| t.lod);
    ret(m, cpu, 4);
}
/// SetAutoGenFilterType(this, filter) / GetAutoGenFilterType(this).
pub fn texture_set_autogen_filter(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, f) = (arg(m, cpu, 0), arg(m, cpu, 1));
    texture(this, |t| t.autogen_filter = f);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
pub fn texture_get_autogen_filter(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = texture(arg(m, cpu, 0), |t| t.autogen_filter);
    ret(m, cpu, 4);
}
/// GenerateMipSubLevels(this): the backend generates them from level 0.
pub fn texture_generate_mips(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 4);
}

fn write_surface_desc(m: &mut dyn Mem, out: u32, d: &SurfaceDesc) {
    for (k, v) in [d.format, RTYPE_SURFACE, d.usage & !NOT_LOCKABLE, d.pool, d.multisample, d.quality, d.width, d.height].into_iter().enumerate() {
        m.set_u32(out + 4 * k as u32, v);
    }
}
/// IDirect3DTexture9::GetLevelDesc(this, level, D3DSURFACE_DESC*).
pub fn texture_get_level_desc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = match level_surface(this, 0, level) {
        Some(s) => {
            let d = surface_image(s, |d, _| d.clone());
            write_surface_desc(m, out, &d);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}
/// IDirect3DVolumeTexture9::GetLevelDesc(this, level, D3DVOLUME_DESC*):
/// Format, Type, Usage, Pool, Width, Height, Depth.
pub fn volume_get_level_desc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let r = texture(this, |t| t.volumes.get(level as usize).map(|i| [t.format, RTYPE_VOLUME, t.usage, t.pool, i.width, i.height, i.depth]));
    cpu.eax = match r {
        Some(v) => {
            for (k, x) in v.into_iter().enumerate() {
                m.set_u32(out + 4 * k as u32, x);
            }
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}
/// IDirect3DTexture9::GetSurfaceLevel(this, level, IDirect3DSurface9**).
pub fn texture_get_surface_level(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, level, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = match level_surface(this, 0, level) {
        Some(s) => {
            hand_out(m, out, s);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}
/// IDirect3DCubeTexture9::GetCubeMapSurface(this, face, level, IDirect3DSurface9**).
pub fn cube_get_surface(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, face, level, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = match level_surface(this, face, level) {
        Some(s) => {
            hand_out(m, out, s);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}
/// AddDirtyRect / AddDirtyBox: the whole level is uploaded when dirty, so
/// these only mark it.
pub fn texture_add_dirty(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
pub fn cube_add_dirty(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}

// --- buffers ---------------------------------------------------------------------

fn buffer<R>(b: u32, f: impl FnOnce(&mut BufferData) -> R) -> R {
    objects(|o| match o.live.get_mut(&b).map(|x| &mut x.kind) {
        Some(Kind::Buffer { data, .. }) => f(data),
        _ => panic!("{b:#x} is not a live buffer"),
    })
}

/// Lock(this, offset, size, void**, flags): size 0 is the whole buffer;
/// buffers may be locked again while locked (D3D9 counts the locks).
pub fn buffer_lock(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, offset, size, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let mut img = buffer(this, |b| std::mem::take(&mut b.image));
    cpu.eax = if offset.checked_add(size).is_none_or(|end| end > img.size) {
        D3DERR_INVALIDCALL
    } else {
        let base = img.shadow(m);
        img.locks += 1;
        m.set_u32(out, base + offset);
        D3D_OK
    };
    buffer(this, |b| b.image = img);
    ret(m, cpu, 20);
}
/// Unlock(this).
pub fn buffer_unlock(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = arg(m, cpu, 0);
    cpu.eax = buffer(this, |b| {
        if b.image.locks == 0 {
            return D3DERR_INVALIDCALL;
        }
        b.image.locks -= 1;
        b.image.dirty = true;
        D3D_OK
    });
    ret(m, cpu, 4);
}
/// GetDesc(this, desc*): vertex buffers {Format VERTEXDATA, Type, Usage,
/// Pool, Size, FVF}; index buffers {Format, Type, Usage, Pool, Size}.
pub fn buffer_get_desc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let v = buffer(this, |b| if b.index {
        vec![b.format, RTYPE_INDEXBUFFER, b.usage, b.pool, b.image.size]
    } else {
        vec![FMT_VERTEXDATA, RTYPE_VERTEXBUFFER, b.usage, b.pool, b.image.size, b.format]
    });
    for (k, x) in v.into_iter().enumerate() {
        m.set_u32(out + 4 * k as u32, x);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
