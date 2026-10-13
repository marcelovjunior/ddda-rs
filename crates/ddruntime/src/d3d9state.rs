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
use crate::d3d9dev::{add_ref_to, counted_in, hand_out, new_object, objects, release_of, with_device, Kind, Object};
use crate::guest::View;
use crate::services::{arg, args, ret};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use crate::fasthash::FxMap as HashMap;
use std::sync::Arc;
use crate::d3d9draw::snapshot_groups as groups;

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

/// Dense D3D9 state banks. Presence is separate from value: an unset state
/// still returns INVALIDCALL, while a stored zero is present. The spill map
/// preserves the service's existing behavior for out-of-range state numbers.
#[derive(Clone, Debug)]
pub struct DenseStates<const N: usize, const M: usize> {
    values: [[u32; M]; N],
    present: [[bool; M]; N],
    spill: HashMap<(u32, u32), u32>,
}

impl<const N: usize, const M: usize> Default for DenseStates<N, M> {
    fn default() -> Self {
        Self { values: [[0; M]; N], present: [[false; M]; N], spill: HashMap::default() }
    }
}

impl<const N: usize, const M: usize> DenseStates<N, M> {
    fn slot(slot: u32) -> Option<usize> {
        // D3DDMAPSAMPLER and the four vertex samplers follow pixel slots.
        if N == 21 && (256..261).contains(&slot) { return Some((slot - 240) as usize); }
        (slot < N as u32 && !(N == 21 && (16..21).contains(&slot))).then_some(slot as usize)
    }

    pub fn get(&self, &(slot, state): &(u32, u32)) -> Option<&u32> {
        if let Some(slot) = Self::slot(slot).filter(|_| state < M as u32) {
            self.present[slot][state as usize].then_some(&self.values[slot][state as usize])
        } else {
            self.spill.get(&(slot, state))
        }
    }

    pub fn insert(&mut self, (slot, state): (u32, u32), value: u32) -> Option<u32> {
        if let Some(slot) = Self::slot(slot).filter(|_| state < M as u32) {
            let state = state as usize;
            let old = self.present[slot][state].then_some(self.values[slot][state]);
            self.values[slot][state] = value;
            self.present[slot][state] = true;
            old
        } else {
            self.spill.insert((slot, state), value)
        }
    }

    pub(crate) fn row(&self, slot: u32) -> [u32; M] {
        if let Some(slot) = Self::slot(slot) { self.values[slot] }
        else { std::array::from_fn(|state| self.get(&(slot, state as u32)).copied().unwrap_or(0)) }
    }
}

#[derive(Clone, Debug)]
pub struct DeviceState {
    pub(crate) draw_groups: Option<Box<groups::Groups>>,
    pub(crate) draw_dirty: u8,
    /// Reused staging for Set*ShaderConstant; commands own immutable banks.
    constant_words: Vec<u32>,
    pub(crate) constant_snapshots: [ConstantSnapshot; 2],
    pub(crate) shader_float_counts: [usize; 2],
    pub render: RenderStates,
    /// (sampler, type) -> value; samplers 0..15 and vertex samplers 257..260
    pub sampler: DenseStates<21, 14>,
    /// (stage, type) -> value
    pub stage: DenseStates<8, 33>,
    /// stage or vertex sampler -> texture object
    pub textures: StageTextures,
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

/// The texture bound at each sampler stage: pixel samplers 0..15 and
/// D3DDMAPSAMPLER plus the vertex samplers 256..260 (L144: an array, not a
/// hash map updated by every SetTexture). Iterates (stage, texture) in stage
/// order, bound stages only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StageTextures([u32; 21]);

/// The stage numbers of StageTextures' entries.
const STAGES: [u32; 21] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 256, 257, 258, 259, 260];

impl StageTextures {
    fn index(stage: u32) -> Option<usize> {
        match stage {
            0..16 => Some(stage as usize),
            256..261 => Some(stage as usize - 240),
            _ => None,
        }
    }

    pub fn get(&self, stage: &u32) -> Option<&u32> {
        Self::index(*stage).map(|i| &self.0[i]).filter(|&&t| t != 0)
    }

    /// Bind `texture` (not 0) at `stage` (a valid one); the texture it had.
    pub fn insert(&mut self, stage: u32, texture: u32) -> Option<u32> {
        let i = Self::index(stage).unwrap_or_else(|| panic!("sampler stage {stage}"));
        Some(std::mem::replace(&mut self.0[i], texture)).filter(|&t| t != 0)
    }

    pub fn remove(&mut self, stage: &u32) -> Option<u32> {
        Self::index(*stage).and_then(|i| Some(std::mem::take(&mut self.0[i])).filter(|&t| t != 0))
    }

    pub fn values(&self) -> impl Iterator<Item = &u32> {
        self.0.iter().filter(|&&t| t != 0)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &u32)> {
        STAGES.iter().zip(self.0.iter()).filter(|(_, &t)| t != 0)
    }

    pub fn len(&self) -> usize {
        self.values().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a> IntoIterator for &'a StageTextures {
    type Item = (&'a u32, &'a u32);
    type IntoIter = std::iter::Filter<std::iter::Zip<std::slice::Iter<'a, u32>, std::slice::Iter<'a, u32>>, fn(&(&'a u32, &'a u32)) -> bool>;
    fn into_iter(self) -> Self::IntoIter {
        fn bound(e: &(&u32, &u32)) -> bool {
            *e.1 != 0
        }
        STAGES.iter().zip(self.0.iter()).filter(bound as fn(&(&'a u32, &'a u32)) -> bool)
    }
}

/// The render states by number, kept sorted: a draw carries them in order
/// as they are (L78: a hash map was collected and sorted for every draw, a
/// BTreeMap walked node by node).
#[derive(Clone, Debug)]
pub struct RenderStates {
    list: Vec<(u32, u32)>,
    // Index + 1, with zero meaning absent. Unusual state IDs use binary search.
    positions: [u8; 256],
    /// The copy draws share (L97), made by the first draw after a change: a
    /// SetRenderState only edits `list` (L101: a copy-on-write list cost two
    /// atomic operations per call).
    shared: std::cell::OnceCell<d3d9cmd::RenderStateList>,
}

impl Default for RenderStates {
    fn default() -> Self { Self { list: Vec::new(), positions: [0; 256], shared: Default::default() } }
}

impl PartialEq for RenderStates {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list
    }
}

impl Eq for RenderStates {}

impl RenderStates {
    fn position(&self, state: u32) -> Option<usize> {
        if let Some(&index) = self.positions.get(state as usize) {
            if index == 255 { return self.list.binary_search_by_key(&state, |e| e.0).ok(); }
            return (index != 0).then(|| index as usize - 1);
        }
        self.list.binary_search_by_key(&state, |e| e.0).ok()
    }

    pub fn get(&self, state: &u32) -> Option<&u32> {
        self.position(*state).map(|i| &self.list[i].1)
    }

    /// Set a state; the value it had.
    pub fn insert(&mut self, state: u32, value: u32) -> Option<u32> {
        match self.position(state) {
            // the same value: the shared copy stays valid
            Some(i) if self.list[i].1 == value => Some(value),
            Some(i) => {
                self.shared.take();
                Some(std::mem::replace(&mut self.list[i].1, value))
            }
            None => {
                self.shared.take();
                let i = self.list.binary_search_by_key(&state, |e| e.0).unwrap_err();
                self.list.insert(i, (state, value));
                for (i, &(key, _)) in self.list.iter().enumerate() {
                    if let Some(pos) = self.positions.get_mut(key as usize) { *pos = (i + 1).min(255) as u8; }
                }
                None
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &u32)> {
        self.list.iter().map(|(k, v)| (k, v))
    }

    /// (state, value) in state order.
    pub fn as_slice(&self) -> &[(u32, u32)] {
        &self.list
    }

    /// The list itself, shared (a draw keeps it; L97).
    pub fn shared(&self) -> d3d9cmd::RenderStateList {
        self.shared.get_or_init(|| self.list.clone().into()).clone()
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
        let mut sampler = DenseStates::default();
        for &(s, t, v) in &p.sampler_states {
            // the displacement-map sampler 256 starts like the vertex samplers
            let range: Vec<u32> = if s == 0 { (0..16).collect() } else { (256..261).collect() };
            for k in range {
                sampler.insert((k, t), v);
            }
        }
        let mut stage = DenseStates::default();
        for &(st, t, v) in &p.stage_states {
            let range: Vec<u32> = if st == 0 { vec![0] } else { (1..8).collect() };
            for k in range {
                stage.insert((k, t), if t == TSS_TEXCOORDINDEX { k } else { v });
            }
        }
        DeviceState {
            draw_groups: None,
            draw_dirty: u8::MAX,
            constant_words: Vec::new(),
            constant_snapshots: Default::default(),
            shader_float_counts: [VS_FLOATS as usize, PS_FLOATS as usize],
            render,
            sampler,
            stage,
            textures: StageTextures::default(),
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
    let [dev, st, v] = args(m, cpu);
    state(dev, |s| s.render.insert(st, v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}
/// GetRenderState(this, state, value*).
pub fn get_render_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, st, out] = args(m, cpu);
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
    let [dev, smp, t, v] = args(m, cpu);
    device_set_sampler_state(dev, smp, t, v);
    cpu.eax = D3D_OK;
    ret(m, cpu, 16);
}
/// SetSamplerState's effect, for host callers (the native path, L119).
pub(crate) fn device_set_sampler_state(dev: u32, smp: u32, t: u32, v: u32) {
    state(dev, |s| {
        if s.sampler.insert((smp, t), v) != Some(v) {
            s.draw_dirty |= groups::TEXTURES;
            SAMPLER_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
}

/// Bumped by every change of a sampler state or a texture binding
/// (between Resets, which rebuild the state outside a frame): the native
/// path's mirror of them is current while it is unchanged (L119).
static SAMPLER_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn sampler_epoch() -> u64 {
    SAMPLER_EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}
/// GetSamplerState(this, sampler, type, value*).
pub fn get_sampler_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, smp, t, out] = args(m, cpu);
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
    let [dev, st, t, v] = args(m, cpu);
    state(dev, |s| s.stage.insert((st, t), v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 16);
}
/// GetTextureStageState(this, stage, type, value*).
pub fn get_texture_stage_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, st, t, out] = args(m, cpu);
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
    let [dev, st, tex] = args(m, cpu);
    cpu.eax = if device_set_texture(dev, st, tex) { D3D_OK } else { D3DERR_INVALIDCALL };
    ret(m, cpu, 12);
}
/// SetTexture's effect, for host callers too (the native path, L119):
/// false for a stage D3D9 refuses.
pub(crate) fn device_set_texture(dev: u32, st: u32, tex: u32) -> bool {
    if st < 16 || (256..261).contains(&st) {
        // L101: the binding and both reference counts in one access to the
        // objects; a release to zero goes through release_of (destroyed, or
        // deferred without the guest lock), as before
        let last = objects(|o| {
            let old = match o.live.get_mut(&dev) {
                Some(Object { kind: Kind::Device(d), .. }) => {
                    let s = &mut d.state;
                    let old = if tex == 0 { s.textures.remove(&st) } else { s.textures.insert(st, tex) }.unwrap_or(0);
                    if old != tex {
                        s.draw_dirty |= groups::TEXTURES;
                        SAMPLER_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    old
                }
                _ => panic!("{dev:#x} is not a live IDirect3DDevice9"),
            };
            // the same texture again: its AddRef and Release cancel out
            // (the stage's own reference keeps it above zero throughout)
            if old == tex {
                return None;
            }
            if tex != 0 {
                let t = counted_in(o, tex);
                let x = o.live.get_mut(&t).unwrap_or_else(|| panic!("AddRef on {t:#x}, not a live D3D9 object"));
                x.refs += 1;
            }
            if old == 0 {
                return None;
            }
            let t = counted_in(o, old);
            let x = o.live.get_mut(&t).unwrap_or_else(|| panic!("Release on {t:#x}, not a live D3D9 object"));
            if x.refs > 1 {
                x.refs -= 1;
                None
            } else {
                Some(old)
            }
        });
        if let Some(old) = last {
            release_of(old);
        }
        true
    } else {
        false
    }
}
/// GetTexture(this, stage, IDirect3DBaseTexture9**).
pub fn get_texture(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, st, out] = args(m, cpu);
    let tex = state(dev, |s| s.textures.get(&st).copied()).unwrap_or(0);
    hand_out(m, out, tex);
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}
/// SetViewport(this, D3DVIEWPORT9*).
pub fn set_viewport(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, p] = args(m, cpu);
    let v: [u32; 6] = View::new(m).words(p);
    state(dev, |s| s.viewport = v);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetViewport(this, D3DVIEWPORT9*).
pub fn get_viewport(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, p] = args(m, cpu);
    let v = state(dev, |s| s.viewport);
    for (k, x) in v.iter().enumerate() {
        m.set_u32(p + 4 * k as u32, *x);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// SetScissorRect(this, RECT*).
pub fn set_scissor_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, p] = args(m, cpu);
    let r: [i32; 4] = View::new(m).words::<4>(p).map(|x| x as i32);
    state(dev, |s| s.scissor = r);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetScissorRect(this, RECT*).
pub fn get_scissor_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, p] = args(m, cpu);
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
    let [dev, t, p] = args(m, cpu);
    let v: [u32; 16] = View::new(m).words(p);
    state(dev, |s| s.transforms.insert(t, v));
    cpu.eax = D3D_OK;
    ret(m, cpu, 12);
}
/// GetTransform(this, type, D3DMATRIX*): identity until set.
pub fn get_transform(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, t, p] = args(m, cpu);
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
    let [dev, index, surf] = args(m, cpu);
    cpu.eax = if index >= 4 || (index == 0 && surf == 0) {
        D3DERR_INVALIDCALL
    } else {
        let size = objects(|o| match o.live.get(&surf).map(|x| &x.kind) {
            Some(Kind::Surface { desc, .. }) => Some((desc.width, desc.height)),
            _ => None,
        });
        let old = with_device(dev, |d| {
            d.state.draw_dirty |= groups::TARGETS;
            std::mem::replace(&mut d.render_targets[index as usize], surf)
        });
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
    let [dev, surf] = args(m, cpu);
    let old = with_device(dev, |d| {
        d.state.draw_dirty |= groups::TARGETS;
        std::mem::replace(&mut d.depth_stencil, surf)
    });
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
    let [dev, p, out] = args(m, cpu);
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
    let [dev, o] = args(m, cpu);
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
            let old = std::mem::replace(slot(s), o);
            if old != o {
                s.draw_dirty |= match iface {
                    "IDirect3DVertexShader9" | "IDirect3DPixelShader9" => groups::SHADERS,
                    "IDirect3DVertexDeclaration9" => groups::DECLARATION,
                    "IDirect3DIndexBuffer9" => groups::INDICES,
                    _ => 0,
                };
            }
            old
        });
        rebind(old, o);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 8);
}
fn get_bound(m: &mut dyn Mem, cpu: &mut Cpu, slot: fn(&mut DeviceState) -> &mut u32) {
    let [dev, out] = args(m, cpu);
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
    let [dev, v] = args(m, cpu);
    state(dev, |s| s.fvf = v);
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}
/// GetFVF(this, fvf*).
pub fn get_fvf(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, out] = args(m, cpu);
    m.set_u32(out, state(dev, |s| s.fvf));
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}

/// Set*ShaderConstantF/I/B(this, start, data*, count): count is in vec4
/// registers for F and I, in BOOLs for B.
fn set_constants(m: &mut dyn Mem, cpu: &mut Cpu, vertex: bool, kind: char) {
    let [dev, start, p, count] = args(m, cpu);
    let limit = match kind {
        'f' => if vertex { VS_FLOATS } else { PS_FLOATS },
        _ => 16,
    };
    cpu.eax = if start.checked_add(count).is_none_or(|end| end > limit) {
        D3DERR_INVALIDCALL
    } else {
        let g = View::new(m);
        state(dev, |s| {
            let words = &mut s.constant_words;
            words.clear();
            g.extend_words(p, count * if kind == 'b' { 1 } else { 4 }, words);
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
    let [dev, start, p, count] = args(m, cpu);
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
    let [this, data, size] = args(m, cpu);
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
    let [dev, p, out] = args(m, cpu);
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
    let [this, p, count] = args(m, cpu);
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
    let [dev, n, vb, off, stride] = args(m, cpu);
    cpu.eax = if n < 16 {
        let old = state(dev, |s| {
            let old = std::mem::replace(&mut s.streams[n as usize], (vb, off, stride));
            if old != (vb, off, stride) { s.draw_dirty |= groups::STREAMS; }
            old.0
        });
        rebind(old, vb);
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 20);
}
/// GetStreamSource(this, stream, IDirect3DVertexBuffer9**, offset*, stride*).
pub fn get_stream_source(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, n, vb, off, stride] = args(m, cpu);
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
    let [dev, n, v] = args(m, cpu);
    cpu.eax = if n < 16 {
        state(dev, |s| {
            if std::mem::replace(&mut s.stream_freq[n as usize], v) != v { s.draw_dirty |= groups::STREAMS; }
        });
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}
/// GetStreamSourceFreq(this, stream, setting*).
pub fn get_stream_source_freq(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [dev, n, out] = args(m, cpu);
    cpu.eax = if n < 16 {
        m.set_u32(out, state(dev, |s| s.stream_freq[n as usize]));
        D3D_OK
    } else {
        D3DERR_INVALIDCALL
    };
    ret(m, cpu, 12);
}

#[cfg(test)]
mod dense_state_tests {
    use super::*;

    fn compare<const N: usize, const M: usize>() {
        let mut dense = DenseStates::<N, M>::default();
        let mut old = HashMap::default();
        let slots = [0, 7, 8, 15, 16, 20, 21, 255, 256, 257, 260, 261, u32::MAX];
        let states = [0, 1, 13, 14, 32, 33, u32::MAX];
        for pass in 0..3 {
            for slot in slots {
                for state in states {
                    let key = (slot, state);
                    assert_eq!(dense.get(&key), old.get(&key));
                    let value = if pass == 0 { 0 } else { slot.wrapping_mul(state).wrapping_add(pass) };
                    assert_eq!(dense.insert(key, value), old.insert(key, value));
                    assert_eq!(dense.get(&key), old.get(&key));
                }
                assert_eq!(dense.row(slot), std::array::from_fn(|state| old.get(&(slot, state as u32)).copied().unwrap_or(0)));
            }
        }
        let copy = dense.clone();
        for (key, value) in old { assert_eq!(copy.get(&key), Some(&value)); }
    }

    #[test]
    fn render_index_matches_sorted_map_and_keeps_old_snapshots() {
        let mut states = RenderStates::default();
        let mut old = std::collections::BTreeMap::new();
        for key in (0..300).rev().chain([u32::MAX, 0, 7, 255, 299]) {
            let saved = states.shared();
            let expected: Vec<_> = old.iter().map(|(&k, &v)| (k, v)).collect();
            assert_eq!(states.insert(key, key.wrapping_add(17)), old.insert(key, key.wrapping_add(17)));
            assert_eq!(&*saved, &expected);
            for key in 0..302 { assert_eq!(states.get(&key), old.get(&key)); }
            assert_eq!(states.as_slice(), old.iter().map(|(&k, &v)| (k, v)).collect::<Vec<_>>());
        }
    }

    #[test]
    fn dense_banks_match_maps_including_absent_zero_and_out_of_range_keys() {
        compare::<21, 14>();
        compare::<8, 33>();
    }
}
#[cfg(test)]
mod tests {
    use super::RenderStates;

    /// L101: the position index follows insertions anywhere in the list, and
    /// the copy draws share is made again only after a change.
    #[test]
    fn render_states_index_and_shared_copy() {
        let mut r: RenderStates = [(20, 1), (7, 2), (200, 3)].into_iter().collect();
        assert_eq!(r.as_slice(), &[(7, 2), (20, 1), (200, 3)]);
        for (s, v) in [(3, 9), (150, 8), (7, 5), (300, 4), (0, 6)] {
            r.insert(s, v);
        }
        let want = [(0, 6), (3, 9), (7, 5), (20, 1), (150, 8), (200, 3), (300, 4)];
        assert_eq!(r.as_slice(), &want);
        for &(s, v) in &want {
            assert_eq!(r.get(&s), Some(&v), "state {s}");
        }
        assert_eq!(r.get(&8), None);
        assert_eq!(r.get(&1000), None);
        let same = |x: &d3d9cmd::RenderStateList, y: &d3d9cmd::RenderStateList| std::ptr::eq(x.as_ptr(), y.as_ptr());
        let a = r.shared();
        assert!(same(&a, &r.shared()), "no change: the same copy");
        assert_eq!(r.insert(20, 1), Some(1));
        assert!(same(&a, &r.shared()), "the same value again: still the same copy");
        r.insert(20, 2);
        let b = r.shared();
        assert!(!same(&a, &b));
        assert_eq!(a.iter().find(|e| e.0 == 20), Some(&(20, 1)), "a draw's copy keeps its values");
        assert_eq!(b.iter().find(|e| e.0 == 20), Some(&(20, 2)));
    }
}
