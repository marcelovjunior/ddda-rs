//! The Direct3D 9 device's state machine (docs/RUNTIME.md, "Graphics
//! backend design", step 1): everything SetRenderState, SetSamplerState,
//! SetTexture, SetViewport, the shader and constant setters and the
//! stream setters change, answered back by the Get* methods, plus the
//! shader and vertex-declaration objects. Drawing reads it (later steps).
//!
//! Defaults are a fresh device's, recorded from a real driver
//! (data/d3d9_profile.txt) with the two documented exceptions the
//! recording device could not show: ZENABLE is TRUE when the device has an
//! automatic depth-stencil surface, and D3DTSS_TEXCOORDINDEX is the stage
//! number. Set* calls keep a reference to the object they bind and release
//! the one they replace, as D3D9 does.
use crate::d3d9::{profile, D3DERR_INVALIDCALL, D3D_OK};
use crate::d3d9dev::{add_ref_to, hand_out, new_object, objects, release_of, with_device, Kind};
use crate::services::{arg, ret};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use crate::fasthash::FxMap as HashMap;
use std::sync::Arc;

const RS_ZENABLE: u32 = 7;
const TSS_TEXCOORDINDEX: u32 = 11;
pub const VS_FLOATS: u32 = 256;
pub const PS_FLOATS: u32 = 224;

pub use d3d9cmd::Constants;

/// A compact immutable draw view of the device's complete register bank.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConstantSnapshot {
    pub dirty: bool,
    value: Option<Arc<Constants>>,
}

impl ConstantSnapshot {
    fn get(&mut self, source: &Arc<Constants>, count: usize) -> Arc<Constants> {
        let count = count.min(source.f.len());
        if count == source.f.len() { return source.clone(); }
        if let Some(value) = &self.value {
            if value.f.len() == count && (!self.dirty ||
                (value.f == source.f[..count] && value.i == source.i && value.b == source.b)) {
                self.dirty = false;
                return value.clone();
            }
        }
        let value = self.value.get_or_insert_with(|| Arc::new(Constants::default()));
        if let Some(dst) = Arc::get_mut(value) {
            dst.f.clear();
            dst.f.extend_from_slice(&source.f[..count]);
            dst.i = source.i;
            dst.b = source.b;
        } else {
            *value = Arc::new(Constants { f: source.f[..count].to_vec(), i: source.i, b: source.b });
        }
        self.dirty = false;
        value.clone()
    }
}

#[cfg(test)]
mod compact_constant_tests {
    use super::*;

    #[test]
    fn prefixes_preserve_old_draws_and_reuse_unread_changes() {
        let mut source = Arc::new(Constants { f: vec![[0; 4]; 256], ..Default::default() });
        let mut cache = ConstantSnapshot::default();
        let first = cache.get(&source, 8);
        Arc::make_mut(&mut source).f[200] = [7; 4];
        cache.dirty = true;
        assert!(Arc::ptr_eq(&first, &cache.get(&source, 8)));
        let full = cache.get(&source, 256);
        assert_eq!(full.f[200], [7; 4]);
        Arc::make_mut(&mut source).f[7] = [9; 4];
        cache.dirty = true;
        let changed = cache.get(&source, 8);
        assert_eq!(first.f[7], [0; 4]);
        assert_eq!(full.f[7], [0; 4]);
        assert_eq!(changed.f[7], [9; 4]);
        assert_eq!(cache.get(&source, 201).f[200], [7; 4]);
        assert!(cache.get(&source, 0).f.is_empty());
    }
}

#[derive(Clone, Debug)]
pub struct DeviceState {
    pub(crate) constant_snapshots: [ConstantSnapshot; 2],
    pub(crate) shader_float_counts: [usize; 2],
    pub render: RenderStates,
    /// (sampler, type) -> value; samplers 0..15 and vertex samplers 257..260
    pub sampler: HashMap<(u32, u32), u32>,
    /// (stage, type) -> value
    pub stage: HashMap<(u32, u32), u32>,
    /// stage or vertex sampler -> texture object
    pub textures: HashMap<u32, u32>,
    /// X, Y, Width, Height, MinZ, MaxZ (the floats as bits)
    pub viewport: [u32; 6],
    pub scissor: [i32; 4],
    pub transforms: HashMap<u32, [u32; 16]>,
    pub vs: Arc<Constants>,
    pub ps: Arc<Constants>,
    pub vertex_shader: u32,
    pub pixel_shader: u32,
    pub declaration: u32,
    pub fvf: u32,
    /// (vertex buffer, offset, stride) per stream
    pub streams: [(u32, u32, u32); 16],
    pub stream_freq: [u32; 16],
    pub indices: u32,
    pub in_scene: bool,
}

/// The render states by number, kept sorted: a draw carries them in order
/// as they are (L78: a hash map was collected and sorted for every draw, a
/// BTreeMap walked node by node).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderStates(d3d9cmd::RenderStateList);

impl RenderStates {
    pub fn get(&self, state: &u32) -> Option<&u32> {
        self.0.binary_search_by_key(state, |e| e.0).ok().map(|i| &self.0[i].1)
    }

    /// Set a state; the value it had.
    pub fn insert(&mut self, state: u32, value: u32) -> Option<u32> {
        match self.0.binary_search_by_key(&state, |e| e.0) {
            // the same value: no copy of a list draws still share
            Ok(i) if self.0[i].1 == value => Some(value),
            Ok(i) => Some(std::mem::replace(&mut self.0[i].1, value)),
            Err(i) => {
                self.0.insert(i, (state, value));
                None
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &u32)> {
        self.0.iter().map(|(k, v)| (k, v))
    }

    /// (state, value) in state order.
    pub fn as_slice(&self) -> &[(u32, u32)] {
        &self.0
    }

    /// The list itself, shared (a draw keeps it; L97).
    pub fn shared(&self) -> d3d9cmd::RenderStateList {
        self.0.clone()
    }
}

impl FromIterator<(u32, u32)> for RenderStates {
    /// As a map collects: a later value of a state replaces an earlier one.
    fn from_iter<I: IntoIterator<Item = (u32, u32)>>(iter: I) -> Self {
        let mut r = RenderStates::default();
        for (k, v) in iter {
            r.insert(k, v);
        }
        r
    }
}

impl DeviceState {
    pub(crate) fn draw_constants(&mut self, counts: [usize; 2]) -> [Arc<Constants>; 2] {
        [self.constant_snapshots[0].get(&self.vs, counts[0]),
         self.constant_snapshots[1].get(&self.ps, counts[1])]
    }
    /// A fresh device's state for a back buffer of `w` x `h`.
    pub fn new(auto_depth: bool, w: u32, h: u32) -> DeviceState {
        let p = profile();
        let mut render: RenderStates = p.render_states.iter().copied().collect();
        render.insert(RS_ZENABLE, auto_depth as u32);
        let mut sampler = HashMap::default();
        for &(s, t, v) in &p.sampler_states {
            // the displacement-map sampler 256 starts like the vertex samplers
            let range: Vec<u32> = if s == 0 { (0..16).collect() } else { (256..261).collect() };
            for k in range {
                sampler.insert((k, t), v);
            }
        }
        let mut stage = HashMap::default();
        for &(st, t, v) in &p.stage_states {
            let range: Vec<u32> = if st == 0 { vec![0] } else { (1..8).collect() };
            for k in range {
                stage.insert((k, t), if t == TSS_TEXCOORDINDEX { k } else { v });
            }
        }
        DeviceState {
            constant_snapshots: Default::default(),
            shader_float_counts: [VS_FLOATS as usize, PS_FLOATS as usize],
            render,
            sampler,
            stage,
            textures: HashMap::default(),
            viewport: [0, 0, w, h, 0f32.to_bits(), 1f32.to_bits()],
            scissor: [0, 0, w as i32, h as i32],
            transforms: HashMap::default(),
            vs: Arc::new(Constants { f: vec![[0; 4]; VS_FLOATS as usize], ..Default::default() }),
            ps: Arc::new(Constants { f: vec![[0; 4]; PS_FLOATS as usize], ..Default::default() }),
            vertex_shader: 0,
            pixel_shader: 0,
            declaration: 0,
            fvf: 0,
            streams: [(0, 0, 0); 16],
            stream_freq: [1; 16],
            indices: 0,
            in_scene: false,
        }
    }

    /// The objects the state holds references to.
    pub fn held(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.textures.values().copied().collect();
        v.extend([self.vertex_shader, self.pixel_shader, self.declaration, self.indices]);
        v.extend(self.streams.iter().map(|s| s.0));
        v.retain(|&o| o != 0);
        v
    }
}

fn state<R>(device: u32, f: impl FnOnce(&mut DeviceState) -> R) -> R {
    with_device(device, |d| f(&mut d.state))
}

/// Bind `new` where `old` was, with D3D9's reference counting.
fn rebind(old: u32, new: u32) {
    if new != 0 {
        add_ref_to(new);
    }
    if old != 0 {
        release_of(old);
    }
}

fn is(object: u32, iface: &str) -> bool {
    objects(|o| o.live.get(&object).is_some_and(|x| x.iface == iface || x.iface.starts_with(iface)))
}

// --- render, sampler and stage states ------------------------------------------

/// SetRenderState(this, state, value).
pub fn set_render_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, st, v) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    state(dev, |s| s.render.insert(st, v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}
/// GetRenderState(this, state, value*).
pub fn get_render_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, st, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = match state(dev, |s| s.render.get(&st).copied()) {
        Some(v) => {
            m.set_u32(out, v);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}
/// SetSamplerState(this, sampler, type, value).
pub fn set_sampler_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, smp, t, v) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    state(dev, |s| s.sampler.insert((smp, t), v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 16);
}
/// GetSamplerState(this, sampler, type, value*).
pub fn get_sampler_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, smp, t, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = match state(dev, |s| s.sampler.get(&(smp, t)).copied()) {
        Some(v) => {
            m.set_u32(out, v);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}
/// SetTextureStageState(this, stage, type, value).
pub fn set_texture_stage_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, st, t, v) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    state(dev, |s| s.stage.insert((st, t), v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 16);
}
/// GetTextureStageState(this, stage, type, value*).
pub fn get_texture_stage_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, st, t, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = match state(dev, |s| s.stage.get(&(st, t)).copied()) {
        Some(v) => {
            m.set_u32(out, v);
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}

// --- textures, viewport, scissor, transforms ---------------------------------------

/// SetTexture(this, stage, texture or NULL): pixel samplers 0..15,
/// D3DDMAPSAMPLER (256, the displacement-map sampler; the game clears it in
/// 0x00DA6EA0) and the vertex samplers 257..260.
pub fn set_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, st, tex) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if st < 16 || (256..261).contains(&st) {
        let old = state(dev, |s| if tex == 0 { s.textures.remove(&st) } else { s.textures.insert(st, tex) }).unwrap_or(0);
        rebind(old, tex);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}
/// GetTexture(this, stage, IDirect3DBaseTexture9**).
pub fn get_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, st, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let tex = state(dev, |s| s.textures.get(&st).copied()).unwrap_or(0);
    hand_out(m, out, tex);
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}
/// SetViewport(this, D3DVIEWPORT9*).
pub fn set_viewport(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let v: [u32; 6] = std::array::from_fn(|k| m.u32(p + 4 * k as u32));
    state(dev, |s| s.viewport = v);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetViewport(this, D3DVIEWPORT9*).
pub fn get_viewport(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let v = state(dev, |s| s.viewport);
    for (k, x) in v.iter().enumerate() {
        m.set_u32(p + 4 * k as u32, *x);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// SetScissorRect(this, RECT*).
pub fn set_scissor_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let r: [i32; 4] = std::array::from_fn(|k| m.u32(p + 4 * k as u32) as i32);
    state(dev, |s| s.scissor = r);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetScissorRect(this, RECT*).
pub fn get_scissor_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let r = state(dev, |s| s.scissor);
    for (k, x) in r.iter().enumerate() {
        m.set_u32(p + 4 * k as u32, *x as u32);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// SetTransform(this, type, D3DMATRIX*): kept for GetTransform (the game's
/// shaders do the transforms; fixed-function vertex processing is unused).
pub fn set_transform(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, t, p) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let v: [u32; 16] = std::array::from_fn(|k| m.u32(p + 4 * k as u32));
    state(dev, |s| s.transforms.insert(t, v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}
/// GetTransform(this, type, D3DMATRIX*): identity until set.
pub fn get_transform(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, t, p) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let one = 1f32.to_bits();
    let v = state(dev, |s| s.transforms.get(&t).copied())
        .unwrap_or([one, 0, 0, 0, 0, one, 0, 0, 0, 0, one, 0, 0, 0, 0, one]);
    for (k, x) in v.iter().enumerate() {
        m.set_u32(p + 4 * k as u32, *x);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}

/// SetRenderTarget(this, index, surface): target 0 cannot be NULL; setting
/// it resets the viewport and scissor rectangle to the surface's size.
pub fn set_render_target(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, index, surf) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if index >= 4 || (index == 0 && surf == 0) {
        D3DERR_INVALIDCALL
    } else {
        let size = objects(|o| match o.live.get(&surf).map(|x| &x.kind) {
            Some(Kind::Surface { desc, .. }) => Some((desc.width, desc.height)),
            _ => None,
        });
        let old = with_device(dev, |d| std::mem::replace(&mut d.render_targets[index as usize], surf));
        rebind(old, surf);
        if index == 0 {
            if let Some((w, h)) = size {
                state(dev, |s| {
                    s.viewport = [0, 0, w, h, 0f32.to_bits(), 1f32.to_bits()];
                    s.scissor = [0, 0, w as i32, h as i32];
                });
            }
        }
        D3D_OK
    };
    ret(m, cpu, 12);
}
/// SetDepthStencilSurface(this, surface or NULL).
pub fn set_depth_stencil_surface(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, surf) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let old = with_device(dev, |d| std::mem::replace(&mut d.depth_stencil, surf));
    rebind(old, surf);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}

// --- scenes and clears ----------------------------------------------------------

/// BeginScene(this): not nested.
pub fn begin_scene(m: &mut dyn Mem, cpu: &mut Cpu) {
    let dev = arg(m, cpu, 0);
    cpu.eax = if state(dev, |s| std::mem::replace(&mut s.in_scene, true)) { D3DERR_INVALIDCALL } else { D3D_OK };
    ret(m, cpu, 4);
}
/// EndScene(this): only inside a scene.
pub fn end_scene(m: &mut dyn Mem, cpu: &mut Cpu) {
    let dev = arg(m, cpu, 0);
    cpu.eax = if state(dev, |s| std::mem::replace(&mut s.in_scene, false)) { D3D_OK } else { D3DERR_INVALIDCALL };
    ret(m, cpu, 4);
}

// --- shaders and constants -------------------------------------------------------

/// A shader's token stream in guest memory, up to and including END.
fn read_tokens(m: &dyn Mem, p: u32) -> Vec<u32> {
    let mut t = vec![m.u32(p)];
    let mut at = 1u32;
    loop {
        let tok = m.u32(p + 4 * at);
        t.push(tok);
        at += 1;
        if tok & 0xFFFF == 0xFFFF {
            break;
        }
        let len = if tok & 0xFFFF == 0xFFFE { (tok >> 16) & 0x7FFF } else { (tok >> 24) & 0xF };
        for _ in 0..len {
            t.push(m.u32(p + 4 * at));
            at += 1;
        }
    }
    t
}

fn create_shader(m: &mut dyn Mem, cpu: &mut Cpu, iface: &'static str, stage_bits: u32) {
    let (dev, p, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let tokens = read_tokens(m, p);
    let parsed = d3d9shader::parse(&tokens);
    cpu.eax = if tokens[0] >> 16 == stage_bits && parsed.is_ok() {
        let float_constants = parsed.unwrap().float_constant_count();
        let o = new_object(m, iface, Kind::Shader { device: dev, tokens: tokens.into(), float_constants });
        m.set_u32(out, o);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}
/// CreateVertexShader(this, DWORD* function, IDirect3DVertexShader9**).
pub fn create_vertex_shader(m: &mut dyn Mem, cpu: &mut Cpu) { create_shader(m, cpu, "IDirect3DVertexShader9", 0xFFFE) }
/// CreatePixelShader(this, DWORD* function, IDirect3DPixelShader9**).
pub fn create_pixel_shader(m: &mut dyn Mem, cpu: &mut Cpu) { create_shader(m, cpu, "IDirect3DPixelShader9", 0xFFFF) }

fn set_bound(m: &mut dyn Mem, cpu: &mut Cpu, iface: &str, slot: fn(&mut DeviceState) -> &mut u32) {
    let (dev, o) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = if o == 0 || is(o, iface) {
        let stage = match iface {
            "IDirect3DVertexShader9" => Some((0, VS_FLOATS as usize)),
            "IDirect3DPixelShader9" => Some((1, PS_FLOATS as usize)),
            _ => None,
        };
        let count = stage.map(|(stage, fallback)| (stage, objects(|objects| {
            match objects.live.get(&o).map(|object| &object.kind) {
                Some(Kind::Shader { float_constants, .. }) => *float_constants,
                _ => fallback,
            }
        })));
        let old = state(dev, |s| {
            if let Some((stage, count)) = count { s.shader_float_counts[stage] = count; }
            std::mem::replace(slot(s), o)
        });
        rebind(old, o);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 8);
}
fn get_bound(m: &mut dyn Mem, cpu: &mut Cpu, slot: fn(&mut DeviceState) -> &mut u32) {
    let (dev, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let o = state(dev, |s| *slot(s));
    hand_out(m, out, o);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
pub fn set_vertex_shader(m: &mut dyn Mem, cpu: &mut Cpu) { set_bound(m, cpu, "IDirect3DVertexShader9", |s| &mut s.vertex_shader) }
pub fn get_vertex_shader(m: &mut dyn Mem, cpu: &mut Cpu) { get_bound(m, cpu, |s| &mut s.vertex_shader) }
pub fn set_pixel_shader(m: &mut dyn Mem, cpu: &mut Cpu) { set_bound(m, cpu, "IDirect3DPixelShader9", |s| &mut s.pixel_shader) }
pub fn get_pixel_shader(m: &mut dyn Mem, cpu: &mut Cpu) { get_bound(m, cpu, |s| &mut s.pixel_shader) }
pub fn set_vertex_declaration(m: &mut dyn Mem, cpu: &mut Cpu) { set_bound(m, cpu, "IDirect3DVertexDeclaration9", |s| &mut s.declaration) }
pub fn get_vertex_declaration(m: &mut dyn Mem, cpu: &mut Cpu) { get_bound(m, cpu, |s| &mut s.declaration) }
pub fn set_indices(m: &mut dyn Mem, cpu: &mut Cpu) { set_bound(m, cpu, "IDirect3DIndexBuffer9", |s| &mut s.indices) }
pub fn get_indices(m: &mut dyn Mem, cpu: &mut Cpu) { get_bound(m, cpu, |s| &mut s.indices) }

/// SetFVF(this, fvf).
pub fn set_fvf(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, v) = (arg(m, cpu, 0), arg(m, cpu, 1));
    state(dev, |s| s.fvf = v);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetFVF(this, fvf*).
pub fn get_fvf(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    m.set_u32(out, state(dev, |s| s.fvf));
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}

/// Set*ShaderConstantF/I/B(this, start, data*, count): count is in vec4
/// registers for F and I, in BOOLs for B.
fn set_constants(m: &mut dyn Mem, cpu: &mut Cpu, vertex: bool, kind: char) {
    let (dev, start, p, count) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let limit = match kind {
        'f' => if vertex { VS_FLOATS } else { PS_FLOATS },
        _ => 16,
    };
    cpu.eax = if start.checked_add(count).is_none_or(|end| end > limit) {
        D3DERR_INVALIDCALL
    } else {
        let words: Vec<u32> = (0..count * if kind == 'b' { 1 } else { 4 }).map(|k| m.u32(p + 4 * k)).collect();
        state(dev, |s| {
            let c = if vertex { &mut s.vs } else { &mut s.ps };
            // A queued or recorded draw owns the old block. Only detach it
            // when an update changes bits; repeated setters keep sharing.
            let changed = (0..count as usize).any(|k| {
                let r = start as usize + k;
                match kind {
                    'f' => c.f[r] != words[4 * k..4 * k + 4],
                    'i' => c.i[r].map(|v| v as u32) != words[4 * k..4 * k + 4],
                    _ => c.b[r] != words[k],
                }
            });
            if !changed { return; }
            let c = Arc::make_mut(c);
            for k in 0..count as usize {
                let r = start as usize + k;
                match kind {
                    'f' => c.f[r] = [words[4 * k], words[4 * k + 1], words[4 * k + 2], words[4 * k + 3]],
                    'i' => c.i[r] = [words[4 * k] as i32, words[4 * k + 1] as i32, words[4 * k + 2] as i32, words[4 * k + 3] as i32],
                    _ => c.b[r] = words[k],
                }
            }
            s.constant_snapshots[if vertex { 0 } else { 1 }].dirty = true;
        });
        D3D_OK
    };
    ret(m, cpu, 16);
}
fn get_constants(m: &mut dyn Mem, cpu: &mut Cpu, vertex: bool, kind: char) {
    let (dev, start, p, count) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let limit = match kind {
        'f' => if vertex { VS_FLOATS } else { PS_FLOATS },
        _ => 16,
    };
    cpu.eax = if start.checked_add(count).is_none_or(|end| end > limit) {
        D3DERR_INVALIDCALL
    } else {
        let words: Vec<u32> = state(dev, |s| {
            let c = if vertex { &s.vs } else { &s.ps };
            (start as usize..(start + count) as usize).flat_map(|r| match kind {
                'f' => c.f[r].to_vec(),
                'i' => c.i[r].iter().map(|&x| x as u32).collect(),
                _ => vec![c.b[r]],
            }).collect()
        });
        for (k, w) in words.iter().enumerate() {
            m.set_u32(p + 4 * k as u32, *w);
        }
        D3D_OK
    };
    ret(m, cpu, 16);
}
pub fn set_vs_f(m: &mut dyn Mem, cpu: &mut Cpu) { set_constants(m, cpu, true, 'f') }
pub fn get_vs_f(m: &mut dyn Mem, cpu: &mut Cpu) { get_constants(m, cpu, true, 'f') }
pub fn set_vs_i(m: &mut dyn Mem, cpu: &mut Cpu) { set_constants(m, cpu, true, 'i') }
pub fn get_vs_i(m: &mut dyn Mem, cpu: &mut Cpu) { get_constants(m, cpu, true, 'i') }
pub fn set_vs_b(m: &mut dyn Mem, cpu: &mut Cpu) { set_constants(m, cpu, true, 'b') }
pub fn get_vs_b(m: &mut dyn Mem, cpu: &mut Cpu) { get_constants(m, cpu, true, 'b') }
pub fn set_ps_f(m: &mut dyn Mem, cpu: &mut Cpu) { set_constants(m, cpu, false, 'f') }
pub fn get_ps_f(m: &mut dyn Mem, cpu: &mut Cpu) { get_constants(m, cpu, false, 'f') }
pub fn set_ps_i(m: &mut dyn Mem, cpu: &mut Cpu) { set_constants(m, cpu, false, 'i') }
pub fn get_ps_i(m: &mut dyn Mem, cpu: &mut Cpu) { get_constants(m, cpu, false, 'i') }
pub fn set_ps_b(m: &mut dyn Mem, cpu: &mut Cpu) { set_constants(m, cpu, false, 'b') }
pub fn get_ps_b(m: &mut dyn Mem, cpu: &mut Cpu) { get_constants(m, cpu, false, 'b') }

/// IDirect3DVertexShader9/IDirect3DPixelShader9::GetFunction(this, data*, UINT* size):
/// the size when data is NULL, else the tokens.
pub fn shader_get_function(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, data, size) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let tokens = objects(|o| match o.live.get(&this).map(|x| &x.kind) {
        Some(Kind::Shader { tokens, .. }) => tokens.clone(),
        _ => panic!("GetFunction on {this:#x}, not a shader"),
    });
    let bytes = 4 * tokens.len() as u32;
    cpu.eax = if data == 0 {
        m.set_u32(size, bytes);
        D3D_OK
    } else if m.u32(size) < bytes {
        D3DERR_INVALIDCALL
    } else {
        for (k, t) in tokens.iter().enumerate() {
            m.set_u32(data + 4 * k as u32, *t);
        }
        m.set_u32(size, bytes);
        D3D_OK
    };
    ret(m, cpu, 12);
}

// --- vertex declarations and streams ----------------------------------------------

/// CreateVertexDeclaration(this, D3DVERTEXELEMENT9*, IDirect3DVertexDeclaration9**):
/// elements up to D3DDECL_END (stream 0xFF).
pub fn create_vertex_declaration(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, p, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let mut elements = Vec::new();
    for k in 0..65u32 {
        let e: [u8; 8] = std::array::from_fn(|b| m.u8(p + 8 * k + b as u32));
        if e[0] == 0xFF {
            break;
        }
        elements.push(e);
    }
    let o = new_object(m, "IDirect3DVertexDeclaration9", Kind::Declaration { device: dev, elements: elements.into() });
    m.set_u32(out, o);
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}

/// IDirect3DVertexDeclaration9::GetDeclaration(this, elements*, UINT* count):
/// the count includes D3DDECL_END; elements NULL asks for the count.
pub fn declaration_get(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, p, count) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let elements = objects(|o| match o.live.get(&this).map(|x| &x.kind) {
        Some(Kind::Declaration { elements, .. }) => elements.clone(),
        _ => panic!("GetDeclaration on {this:#x}, not a declaration"),
    });
    let n = elements.len() as u32 + 1;
    if p != 0 {
        let end = [0xFFu8, 0, 0, 0, 17, 0, 0, 0];
        for (k, e) in elements.iter().chain([&end]).enumerate() {
            for (b, x) in e.iter().enumerate() {
                m.set_u8(p + 8 * k as u32 + b as u32, *x);
            }
        }
    }
    m.set_u32(count, n);
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}

/// SetStreamSource(this, stream, vertex buffer, offset, stride).
pub fn set_stream_source(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, n, vb, off, stride) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    cpu.eax = if n < 16 {
        let old = state(dev, |s| std::mem::replace(&mut s.streams[n as usize], (vb, off, stride)).0);
        rebind(old, vb);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 20);
}
/// GetStreamSource(this, stream, IDirect3DVertexBuffer9**, offset*, stride*).
pub fn get_stream_source(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, n, vb, off, stride) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    cpu.eax = if n < 16 {
        let (b, o, s) = state(dev, |s| s.streams[n as usize]);
        hand_out(m, vb, b);
        m.set_u32(off, o);
        m.set_u32(stride, s);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 20);
}
/// SetStreamSourceFreq(this, stream, setting).
pub fn set_stream_source_freq(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, n, v) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if n < 16 {
        state(dev, |s| s.stream_freq[n as usize] = v);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}
/// GetStreamSourceFreq(this, stream, setting*).
pub fn get_stream_source_freq(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (dev, n, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    cpu.eax = if n < 16 {
        m.set_u32(out, state(dev, |s| s.stream_freq[n as usize]));
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}
