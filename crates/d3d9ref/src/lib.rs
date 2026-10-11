//! The host's real Direct3D 9 driver as the reference for what a shader
//! computes (the oracle for the shader translator, docs/RUNTIME.md
//! "Graphics"). Windows only; [`Oracle::new`] returns None where D3D9 or a
//! device is unavailable, and callers skip.
//!
//! A pixel shader runs once, on a 1x1 floating-point render target per
//! colour output: a generated vs_3_0 pass-through feeds every input the
//! pixel shader declares a chosen constant value (all four vertices of
//! the quad carry it, so interpolation returns it exactly), the pixel
//! shader's float, integer and boolean constants and its samplers'
//! textures (A32B32G32R32F, point sampling, wrap addressing, no mipmaps)
//! are set from a [`PixelRun`], and the outputs are read back.
use d3d9shader::{op, reg};
use std::ffi::c_void;

type Obj = *mut c_void;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureKind {
    Flat,
    Cube,
    Volume,
}

/// A texture bound to one sampler: `size`^2 texels (x6 faces for cubes,
/// x`size` slices for volumes), row-major, faces/slices in order.
#[derive(Clone, Debug)]
pub struct Texture {
    pub kind: TextureKind,
    pub size: u32,
    pub texels: Vec<[f32; 4]>,
}

/// The inputs of one pixel-shader execution.
#[derive(Clone, Debug, Default)]
pub struct PixelRun {
    /// c0..c223
    pub constants: Vec<[f32; 4]>,
    /// i0..i15
    pub ints: Vec<[i32; 4]>,
    /// b0..b15
    pub bools: Vec<bool>,
    /// The value of each input the shader declares, in declaration order.
    pub varyings: Vec<[f32; 4]>,
    /// The texture for each sampler index (None: unbound).
    pub textures: Vec<Option<Texture>>,
}

/// An input a pixel shader declares: usage, usage index (dcl token) and
/// register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub usage: u32,
    pub index: u32,
    pub register: u32,
}

/// The inputs a pixel shader declares (`dcl_<usage><n> v#`), in order.
pub fn pixel_inputs(s: &d3d9shader::Shader) -> Vec<Input> {
    s.instructions
        .iter()
        .filter(|i| i.opcode == op::DCL)
        .filter_map(|i| {
            let d = i.dst.as_ref()?;
            let t = i.decl?;
            (d.kind == reg::INPUT).then_some(Input { usage: t & 0x1F, index: (t >> 16) & 0xF, register: d.index })
        })
        .collect()
}

/// The samplers a shader declares: (sampler index, texture kind).
pub fn samplers(s: &d3d9shader::Shader) -> Vec<(u32, TextureKind)> {
    s.instructions
        .iter()
        .filter(|i| i.opcode == op::DCL)
        .filter_map(|i| {
            let d = i.dst.as_ref()?;
            (d.kind == reg::SAMPLER).then(|| {
                let kind = match (i.decl.unwrap_or(0) >> 27) & 0xF {
                    3 => TextureKind::Cube,
                    4 => TextureKind::Volume,
                    _ => TextureKind::Flat,
                };
                (d.index, kind)
            })
        })
        .collect()
}

/// The colour outputs a pixel shader writes (oC#), sorted.
pub fn colour_outputs(s: &d3d9shader::Shader) -> Vec<u32> {
    let mut v: Vec<u32> = s.instructions.iter()
        .filter_map(|i| i.dst.as_ref())
        .filter(|d| d.kind == reg::COLOROUT)
        .map(|d| d.index)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The vs_3_0 pass-through for `inputs`: position from v0, output k+1 =
/// constant c_k for the k-th pixel-shader input, with its usage.
pub fn pass_through(inputs: &[Input]) -> Vec<u32> {
    let mut t = vec![0xFFFE_0300];
    let dcl = |t: &mut Vec<u32>, usage: u32, dst: u32| {
        t.extend_from_slice(&[0x0200_001F, 0x8000_0000 | usage, dst]);
    };
    dcl(&mut t, 0, 0x900F_0000); // dcl_position v0
    dcl(&mut t, 0, 0xE00F_0000); // dcl_position o0
    for (k, i) in inputs.iter().enumerate() {
        dcl(&mut t, i.usage | (i.index << 16), 0xE00F_0000 | (k as u32 + 1));
    }
    t.extend_from_slice(&[0x0200_0001, 0xE00F_0000, 0x90E4_0000]); // mov o0, v0
    for k in 0..inputs.len() as u32 {
        t.extend_from_slice(&[0x0200_0001, 0xE00F_0000 | (k + 1), 0xA0E4_0000 | k]); // mov o(k+1), c_k
    }
    t.push(0x0000_FFFF);
    t
}

// --- Win32 and COM plumbing ------------------------------------------------------

#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32,
                       parent: Obj, menu: Obj, inst: Obj, param: Obj) -> Obj;
    fn DestroyWindow(hwnd: Obj) -> i32;
}

extern "system" {
    fn LoadLibraryA(name: *const u8) -> Obj;
    fn GetProcAddress(module: Obj, name: *const u8) -> Obj;
}

/// The function at vtable `slot` of COM object `o`.
unsafe fn method(o: Obj, slot: usize) -> *const c_void {
    *(*(o as *const *const *const c_void)).add(slot)
}

macro_rules! com {
    ($o:expr, $slot:expr, ($($t:ty),*) $(, $a:expr)*) => {{
        let f: unsafe extern "system" fn(Obj $(, $t)*) -> i32 = std::mem::transmute(method($o, $slot));
        f($o $(, $a)*)
    }};
}

unsafe fn release(o: Obj) {
    if !o.is_null() {
        let f: unsafe extern "system" fn(Obj) -> u32 = std::mem::transmute(method(o, 2));
        f(o);
    }
}

fn check(hr: i32, what: &str) -> Result<(), String> {
    if hr < 0 { Err(format!("{what} failed: {:#x}", hr as u32)) } else { Ok(()) }
}

// IDirect3DDevice9 vtable slots (d3d9.h order)
const CREATE_TEXTURE: usize = 23;
const CREATE_VOLUME_TEXTURE: usize = 24;
const CREATE_CUBE_TEXTURE: usize = 25;
const CREATE_RENDER_TARGET: usize = 28;
const GET_RENDER_TARGET_DATA: usize = 32;
const CREATE_OFFSCREEN_PLAIN_SURFACE: usize = 36;
const SET_RENDER_TARGET: usize = 37;
const BEGIN_SCENE: usize = 41;
const END_SCENE: usize = 42;
const SET_RENDER_STATE: usize = 57;
const SET_TEXTURE: usize = 65;
const SET_SAMPLER_STATE: usize = 69;
const DRAW_PRIMITIVE_UP: usize = 83;
const CREATE_VERTEX_DECLARATION: usize = 86;
const SET_VERTEX_DECLARATION: usize = 87;
const CREATE_VERTEX_SHADER: usize = 91;
const SET_VERTEX_SHADER: usize = 92;
const SET_VERTEX_SHADER_CONSTANT_F: usize = 94;
const CREATE_PIXEL_SHADER: usize = 106;
const SET_PIXEL_SHADER: usize = 107;
const SET_PIXEL_SHADER_CONSTANT_F: usize = 109;
const SET_PIXEL_SHADER_CONSTANT_I: usize = 111;
const SET_PIXEL_SHADER_CONSTANT_B: usize = 113;

const FMT_A32B32G32R32F: u32 = 116;
const POOL_DEFAULT: u32 = 0;
const POOL_MANAGED: u32 = 1;
const POOL_SYSTEMMEM: u32 = 2;

/// A D3D9 HAL device on a hidden window.
pub struct Oracle {
    d3d: Obj,
    device: Obj,
    hwnd: Obj,
    declaration: Obj,
    targets: Vec<Obj>,
    readback: Vec<Obj>,
}

impl Oracle {
    /// The host's D3D9 HAL device, or None (no D3D9, no adapter).
    pub fn new() -> Option<Oracle> {
        unsafe {
            let module = LoadLibraryA(c"d3d9.dll".as_ptr() as *const u8);
            if module.is_null() {
                return None;
            }
            let create = GetProcAddress(module, c"Direct3DCreate9".as_ptr() as *const u8);
            if create.is_null() {
                return None;
            }
            let create: unsafe extern "system" fn(u32) -> Obj = std::mem::transmute(create);
            let d3d = create(32);
            if d3d.is_null() {
                return None;
            }
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(0, class.as_ptr(), class.as_ptr(), 0, 0, 0, 64, 64,
                                       std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
            if hwnd.is_null() {
                release(d3d);
                return None;
            }
            // D3DPRESENT_PARAMETERS, x64 layout: back buffer 1x1 A8R8G8B8,
            // one buffer, discard, the window, windowed, immediate presents
            let mut pp = [0u8; 64];
            let put = |pp: &mut [u8; 64], off: usize, v: u32| pp[off..off + 4].copy_from_slice(&v.to_le_bytes());
            put(&mut pp, 0, 1);
            put(&mut pp, 4, 1);
            put(&mut pp, 8, 21);
            put(&mut pp, 12, 1);
            put(&mut pp, 24, 1);
            pp[32..40].copy_from_slice(&(hwnd as u64).to_le_bytes());
            put(&mut pp, 40, 1);
            put(&mut pp, 60, 0x8000_0000);
            let mut device: Obj = std::ptr::null_mut();
            // HAL, hardware vertex processing | FPU preserve
            let hr = com!(d3d, 16, (u32, u32, Obj, u32, *mut u8, *mut Obj), 0, 1, hwnd, 0x42, pp.as_mut_ptr(), &mut device);
            if hr < 0 || device.is_null() {
                DestroyWindow(hwnd);
                release(d3d);
                return None;
            }
            // vertex declaration: float4 position at offset 0, then D3DDECL_END
            let elements: [u16; 8] = [0, 0, 3, 0, 0x00FF, 0, 17, 0];
            let mut declaration: Obj = std::ptr::null_mut();
            com!(device, CREATE_VERTEX_DECLARATION, (*const u16, *mut Obj), elements.as_ptr(), &mut declaration);
            let mut o = Oracle { d3d, device, hwnd, declaration, targets: vec![], readback: vec![] };
            for _ in 0..4 {
                let (mut rt, mut rb): (Obj, Obj) = (std::ptr::null_mut(), std::ptr::null_mut());
                com!(o.device, CREATE_RENDER_TARGET, (u32, u32, u32, u32, u32, i32, *mut Obj, Obj),
                     1, 1, FMT_A32B32G32R32F, 0, 0, 0, &mut rt, std::ptr::null_mut());
                com!(o.device, CREATE_OFFSCREEN_PLAIN_SURFACE, (u32, u32, u32, u32, *mut Obj, Obj),
                     1, 1, FMT_A32B32G32R32F, POOL_SYSTEMMEM, &mut rb, std::ptr::null_mut());
                if rt.is_null() || rb.is_null() {
                    return None;
                }
                o.targets.push(rt);
                o.readback.push(rb);
            }
            Some(o)
        }
    }

    /// Run pixel shader `ps` (its tokens) once with `run`'s inputs; the
    /// value of each colour output it writes (oC0, oC1, ...).
    pub fn run_pixel(&mut self, ps: &[u32], run: &PixelRun) -> Result<Vec<[f32; 4]>, String> {
        let shader = d3d9shader::parse(ps).map_err(|e| e.to_string())?;
        let inputs = pixel_inputs(&shader);
        let outputs = colour_outputs(&shader);
        unsafe {
            let mut created: Vec<Obj> = Vec::new();
            let r = self.draw(ps, &shader, &inputs, &outputs, run, &mut created);
            for o in created {
                release(o);
            }
            r
        }
    }

    unsafe fn draw(&mut self, ps: &[u32], shader: &d3d9shader::Shader, inputs: &[Input], outputs: &[u32],
                   run: &PixelRun, created: &mut Vec<Obj>) -> Result<Vec<[f32; 4]>, String> {
        let d = self.device;
        let vs_tokens = pass_through(inputs);
        let (mut vs, mut pixel): (Obj, Obj) = (std::ptr::null_mut(), std::ptr::null_mut());
        check(com!(d, CREATE_VERTEX_SHADER, (*const u32, *mut Obj), vs_tokens.as_ptr(), &mut vs), "CreateVertexShader")?;
        created.push(vs);
        check(com!(d, CREATE_PIXEL_SHADER, (*const u32, *mut Obj), ps.as_ptr(), &mut pixel), "CreatePixelShader")?;
        created.push(pixel);
        for (k, &o) in outputs.iter().enumerate() {
            check(com!(d, SET_RENDER_TARGET, (u32, Obj), o, self.targets[k.min(3)]), "SetRenderTarget")?;
        }
        for k in outputs.len() as u32..4 {
            if k > 0 {
                com!(d, SET_RENDER_TARGET, (u32, Obj), k, std::ptr::null_mut::<c_void>());
            }
        }
        // render states: no culling, depth, blending, alpha test or sRGB writes
        for (state, value) in [(22u32, 1u32), (7, 0), (27, 0), (15, 0), (194, 0), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (174, 0)] {
            com!(d, SET_RENDER_STATE, (u32, u32), state, value);
        }
        com!(d, SET_VERTEX_DECLARATION, (Obj), self.declaration);
        com!(d, SET_VERTEX_SHADER, (Obj), vs);
        com!(d, SET_PIXEL_SHADER, (Obj), pixel);
        let mut varyings: Vec<f32> = run.varyings.iter().flatten().copied().collect();
        varyings.resize(inputs.len() * 4, 0.0);
        if !inputs.is_empty() {
            com!(d, SET_VERTEX_SHADER_CONSTANT_F, (u32, *const f32, u32), 0, varyings.as_ptr(), inputs.len() as u32);
        }
        let mut cf: Vec<f32> = run.constants.iter().flatten().copied().collect();
        cf.resize(224 * 4, 0.0);
        com!(d, SET_PIXEL_SHADER_CONSTANT_F, (u32, *const f32, u32), 0, cf.as_ptr(), 224);
        let mut ci: Vec<i32> = run.ints.iter().flatten().copied().collect();
        ci.resize(16 * 4, 0);
        com!(d, SET_PIXEL_SHADER_CONSTANT_I, (u32, *const i32, u32), 0, ci.as_ptr(), 16);
        let mut cb: Vec<i32> = run.bools.iter().map(|&b| b as i32).collect();
        cb.resize(16, 0);
        com!(d, SET_PIXEL_SHADER_CONSTANT_B, (u32, *const i32, u32), 0, cb.as_ptr(), 16);
        for (index, kind) in samplers(shader) {
            let tex = match run.textures.get(index as usize).cloned().flatten() {
                Some(t) => {
                    if t.kind != kind {
                        return Err(format!("sampler s{index} is {kind:?}, the run gives {:?}", t.kind));
                    }
                    let o = self.texture(&t)?;
                    created.push(o);
                    o
                }
                None => std::ptr::null_mut(),
            };
            com!(d, SET_TEXTURE, (u32, Obj), index, tex);
            // point sampling, no mipmaps, wrap, linear (not sRGB) reads
            for (state, value) in [(5u32, 1u32), (6, 1), (7, 0), (1, 1), (2, 1), (3, 1), (11, 0)] {
                com!(d, SET_SAMPLER_STATE, (u32, u32, u32), index, state, value);
            }
        }
        check(com!(d, BEGIN_SCENE, ()), "BeginScene")?;
        let quad: [f32; 16] = [-1.0, -1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, 1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0];
        let hr = com!(d, DRAW_PRIMITIVE_UP, (u32, u32, *const f32, u32), 5, 2, quad.as_ptr(), 16);
        com!(d, END_SCENE, ());
        check(hr, "DrawPrimitiveUP")?;
        let mut values = Vec::new();
        for k in 0..outputs.len() {
            check(com!(d, GET_RENDER_TARGET_DATA, (Obj, Obj), self.targets[k], self.readback[k]), "GetRenderTargetData")?;
            values.push(read_texel(self.readback[k])?);
        }
        Ok(values)
    }

    unsafe fn texture(&mut self, t: &Texture) -> Result<Obj, String> {
        let d = self.device;
        let mut tex: Obj = std::ptr::null_mut();
        let n = t.size as usize;
        match t.kind {
            TextureKind::Flat => {
                check(com!(d, CREATE_TEXTURE, (u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                           t.size, t.size, 1, 0, FMT_A32B32G32R32F, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateTexture")?;
                // IDirect3DTexture9::LockRect(level, D3DLOCKED_RECT*, RECT*, flags) slot 19, UnlockRect 20
                let mut locked = [0u64; 2];
                check(com!(tex, 19, (u32, *mut u64, Obj, u32), 0, locked.as_mut_ptr(), std::ptr::null_mut(), 0), "LockRect")?;
                write_rows(locked, n, &t.texels[..n * n]);
                com!(tex, 20, (u32), 0);
            }
            TextureKind::Cube => {
                check(com!(d, CREATE_CUBE_TEXTURE, (u32, u32, u32, u32, u32, *mut Obj, Obj),
                           t.size, 1, 0, FMT_A32B32G32R32F, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateCubeTexture")?;
                for face in 0..6 {
                    // IDirect3DCubeTexture9::LockRect(face, level, ...) slot 19, UnlockRect(face, level) 20
                    let mut locked = [0u64; 2];
                    check(com!(tex, 19, (u32, u32, *mut u64, Obj, u32), face, 0, locked.as_mut_ptr(), std::ptr::null_mut(), 0), "LockRect")?;
                    let f = face as usize * n * n;
                    write_rows(locked, n, &t.texels[f..f + n * n]);
                    com!(tex, 20, (u32, u32), face, 0);
                }
            }
            TextureKind::Volume => {
                check(com!(d, CREATE_VOLUME_TEXTURE, (u32, u32, u32, u32, u32, u32, u32, *mut Obj, Obj),
                           t.size, t.size, t.size, 1, 0, FMT_A32B32G32R32F, POOL_MANAGED, &mut tex, std::ptr::null_mut()), "CreateVolumeTexture")?;
                // IDirect3DVolumeTexture9::LockBox(level, D3DLOCKED_BOX*, D3DBOX*, flags) slot 19, UnlockBox 20
                let mut locked = [0u64; 2]; // RowPitch, SlicePitch (two INTs), pBits
                let mut boxed = [0u8; 24];
                check(com!(tex, 19, (u32, *mut u8, Obj, u32), 0, boxed.as_mut_ptr(), std::ptr::null_mut(), 0), "LockBox")?;
                let row = i32::from_le_bytes(boxed[0..4].try_into().unwrap()) as usize;
                let slice = i32::from_le_bytes(boxed[4..8].try_into().unwrap()) as usize;
                let bits = u64::from_le_bytes(boxed[8..16].try_into().unwrap()) as *mut u8;
                for z in 0..n {
                    for y in 0..n {
                        let src = &t.texels[(z * n + y) * n..(z * n + y + 1) * n];
                        std::ptr::copy_nonoverlapping(src.as_ptr() as *const u8, bits.add(z * slice + y * row), n * 16);
                    }
                }
                let _ = &mut locked;
                com!(tex, 20, (u32), 0);
            }
        }
        Ok(tex)
    }
}

/// D3DLOCKED_RECT on x64: Pitch (INT, padded to 8), pBits.
unsafe fn write_rows(locked: [u64; 2], n: usize, texels: &[[f32; 4]]) {
    let pitch = locked[0] as u32 as usize;
    let bits = locked[1] as *mut u8;
    for y in 0..n {
        std::ptr::copy_nonoverlapping(texels[y * n..(y + 1) * n].as_ptr() as *const u8, bits.add(y * pitch), n * 16);
    }
}

unsafe fn read_texel(surface: Obj) -> Result<[f32; 4], String> {
    let mut locked = [0u64; 2];
    // IDirect3DSurface9::LockRect(D3DLOCKED_RECT*, RECT*, flags) slot 13 (D3DLOCK_READONLY 0x10), UnlockRect 14
    check(com!(surface, 13, (*mut u64, Obj, u32), locked.as_mut_ptr(), std::ptr::null_mut(), 0x10), "LockRect")?;
    let bits = locked[1] as *const f32;
    let v = [*bits, *bits.add(1), *bits.add(2), *bits.add(3)];
    com!(surface, 14, ());
    Ok(v)
}

impl Drop for Oracle {
    fn drop(&mut self) {
        unsafe {
            for o in self.targets.drain(..).chain(self.readback.drain(..)) {
                release(o);
            }
            release(self.declaration);
            release(self.device);
            release(self.d3d);
            DestroyWindow(self.hwnd);
        }
    }
}
