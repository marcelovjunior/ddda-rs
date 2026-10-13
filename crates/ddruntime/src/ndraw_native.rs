//! The native path through nDraw's command executor (L119, step N2 of
//! docs/NATIVE_RENDERER.md): a `mtdraw::sink::CommandSink` installed over
//! the executor port.
//!
//! `DDDRAW_NATIVE` selects it:
//!
//! - `census`: takes no command (every one runs the default D3D9 path,
//!   unchanged) and measures, per frame, the commands by type, the draws
//!   into the frame's largest colour target (the main scene passes) and
//!   into other targets, the distinct state, shader-binding, stream and
//!   target objects, and how many scene draws the native path could take
//!   (and why the others not). It prints a summary to stderr every
//!   `DDDRAW_NATIVE_EVERY` seconds (default 10).
//! - `drawcall`: takes the scene draws it can and sends the backend the
//!   `DrawCall` the D3D9 path would have sent for them, decoded straight
//!   from the nDraw objects. The applies, the D3D9 state setters and the
//!   snapshot are skipped; the backend draws them exactly as before.
//! - `1`: takes the same draws as native jobs (d3d9cmd::NativeJob, L126):
//!   consecutive taken draws into the same targets form a pass, and the
//!   passes since the last command left to the default path form a job,
//!   sent before that command (or at the frame's end), so the backend
//!   receives everything in the executor's order. A job the backend
//!   rejects is sent as the draws' DrawCalls instead.
//! - `compare`: sends both decoded DrawCalls to the real D3D9 targets and
//!   NativeJobs to pre-pass shadow targets. This compares backend paths;
//!   the separate decode oracle test covers their shared nDraw decoder.
//!   Original indexed merging is bypassed just as in `drawcall`/`1`, so
//!   every compared record is offered to this sink.
//!
//! How a taken draw is decoded: the four apply functions' semantics
//! (crates/mtstate, verified against the original on 17,928 D3D9 calls)
//! run against a zeroed scratch context, which makes them emit every call
//! of the draw's state (the "nothing applied yet" branches); those calls
//! fold into the state the D3D9 device would hold. A draw is only taken
//! where that equals what the delta path leaves on the device:
//!
//! - every constant register and sampler the shaders read is uploaded or
//!   bound by the draw's own shader binding (otherwise the device's older
//!   registers would show through);
//! - no blend-factor blending (0x00DAA5F0 updates RS193 only when the
//!   BlendState pointer changes, docs/NDRAW.md section 9, so the factor
//!   depends on the command history);
//! - the viewport is not the "invalid" one at 0x018D04E4 (then the apply
//!   makes no SetViewport call).
//!
//! The sampler filtering option at [0x018D1D20]+0x34 overrides filters of
//! anisotropic samplers only when a previous sampler was applied on the
//! stage (0x00DA7250's non-null branch); a taken draw assumes that branch
//! (every use of a stage but the first in a frame). Unresolved: the first
//! use of a stage in a frame with option 1..5; `census` reports the option.
//!
//! A taken command changes neither the D3D9 device nor the executor
//! context (mtdraw::sink), so the default path stays consistent.
//!
//! Layouts read (docs/NDRAW.md, sections 2, 3 and 9): a draw record's +4 is
//! its target binding, +8 its fixed-state binding, +0xC its shader binding,
//! +0x10 its stream binding, +0x14 its element count, +0x18 its start and
//! +0x1C (type 1) its base vertex; a view's +0x14 is its D3D surface, +0x18
//! its width (u16) and +0x1A its height (u16, the top bit an sRGB flag).

use crate::d3d9dev::{objects, Kind};
use crate::fasthash::{FxMap, FxSet};
use d3d9cmd::{
    Constants, DrawCall, Indices, NativeBoolWrite, NativeConstants, NativeDraw, NativeFixedState, NativeFloatWrite,
    NativeIndices, NativeIntWrite, NativeJob, NativeObjectIdentity, NativePass, NativeStateIdentity, NativeStream,
    RenderStateList, Target, TextureBinding, Vertices,
};
use ddnative_base::mem::Mem;
use mtdraw::sink::{CommandSink, Taken};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[path = "ndraw_compare.rs"]
pub mod comparison;

/// What `DDDRAW_NATIVE` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    /// count, take nothing
    Census,
    /// take the scene draws the decoder can, as backend DrawCalls
    DrawCall,
    /// take them as native jobs
    Native,
    /// D3D9 oracle on real targets, native jobs on pre-pass shadows.
    Compare,
}

pub fn mode() -> Mode {
    match std::env::var("DDDRAW_NATIVE").as_deref() {
        Ok("census") => Mode::Census,
        Ok("drawcall") => Mode::DrawCall,
        Ok("1") => Mode::Native,
        Ok("compare") => Mode::Compare,
        _ => Mode::Off,
    }
}

/// Install the sink `mode()` asks for (before the program runs).
pub fn install() {
    let mode = mode();
    if mode == Mode::Off {
        return;
    }
    let every = std::env::var("DDDRAW_NATIVE_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(10u64);
    mtdraw::sink::set_sink(Some(Arc::new(Native::new(mode, Duration::from_secs(every.max(1))))));
}

// --- the executable's values ---------------------------------------------------

/// Pointer to the renderer options; +0x34 is the sampler filtering option
/// 0x00DA7250 reads (docs/NDRAW.md section 9).
const FILTER_OPTIONS: u32 = 0x018D_1D20;
/// The four "invalid" viewport words 0x00DA6FE0 resets its cached viewport
/// to when a target changes (docs/NDRAW.md section 9).
const INVALID_VIEWPORT: u32 = 0x018D_04E4;
/// The scratch context the decoders run against: above any guest object.
const SCRATCH_CONTEXT: u32 = 0xFFFF_E000;
/// The executor context's size (0x00DA6DA0 zeroes 0xE28 bytes).
const CONTEXT_BYTES: u32 = 0xE28;
/// The runtime's register-file sizes (d3d9state VS_FLOATS / PS_FLOATS).
const FLOATS: [usize; 2] = [256, 224];

// --- why a draw is not taken ---------------------------------------------------

/// Why the native path leaves a command to the default path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// instanced draws (types 2, 3): not decoded yet
    Instanced,
    /// not into the frame's scene-sized colour target
    NotScene,
    NullBinding,
    /// mtstate refused the shader binding (an undefined derived constant
    /// or comparison)
    Decode,
    /// no SetViewport would be made (see the module notes)
    ViewportGlobals,
    /// blend-factor blending (see the module notes)
    BlendFactor,
    NoColourTarget,
    NoIndices,
    ZeroStride,
    /// a shader, declaration, texture, surface or buffer is not a live
    /// object of the kind expected
    Missing,
    /// shader bytecode the parser refuses
    Shader,
    /// a register or sampler the shaders read is not set by the binding
    FloatsUncovered,
    IntsUncovered,
    BoolsUncovered,
    SamplersUncovered,
    /// the device holds state the draw would carry otherwise (streams 4..15,
    /// a stream frequency, outside a scene)
    Device,
    /// a primitive count the D3D9 path refuses (INVALIDCALL)
    Primitive,
}

impl Refusal {
    const ALL: [Refusal; 17] = [
        Refusal::Instanced, Refusal::NotScene, Refusal::NullBinding, Refusal::Decode, Refusal::ViewportGlobals,
        Refusal::BlendFactor, Refusal::NoColourTarget, Refusal::NoIndices, Refusal::ZeroStride, Refusal::Missing,
        Refusal::Shader, Refusal::FloatsUncovered, Refusal::IntsUncovered, Refusal::BoolsUncovered,
        Refusal::SamplersUncovered, Refusal::Device, Refusal::Primitive,
    ];
}

// --- the pure decode: a record's bindings to the D3D9 state it sets --------------

/// Guest memory with the scratch context in front of it.
struct Scratch<'a> {
    m: &'a dyn Mem,
    context: Vec<u32>,
}

impl mtstate::Memory for Scratch<'_> {
    fn read32(&self, address: u32) -> u32 {
        let off = address.wrapping_sub(SCRATCH_CONTEXT);
        if off < CONTEXT_BYTES {
            self.context[off as usize / 4]
        } else {
            self.m.u32(address)
        }
    }
    fn write32(&mut self, address: u32, value: u32) {
        let off = address.wrapping_sub(SCRATCH_CONTEXT);
        if off < CONTEXT_BYTES {
            self.context[off as usize / 4] = value;
        }
    }
}

/// The D3D9 state a draw record's four bindings set when nothing was
/// applied before (the calls of mtstate's decoders, folded).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folded {
    /// render-target surfaces 0..3 and the depth-stencil surface
    pub targets: [u32; 4],
    pub depth: u32,
    pub viewport: Option<[u32; 6]>,
    pub scissor: Option<[i32; 4]>,
    /// render states in call order
    pub states: Vec<(u32, u32)>,
    /// (vertex buffer, offset, stride) of streams 0..3
    pub streams: [(u32, u32, u32); 4],
    pub indices: u32,
    pub declaration: u32,
    pub vertex_shader: u32,
    pub pixel_shader: u32,
    /// (sampler, texture) in call order
    pub textures: Vec<(u32, u32)>,
    /// (sampler, state type, value) in call order
    pub sampler_states: Vec<(u32, u32, u32)>,
    /// per stage (0 vertex, 1 pixel): float register writes
    pub floats: [Vec<(u32, [u32; 4])>; 2],
    pub ints: [Vec<(u32, [i32; 4])>; 2],
    pub bools: [Vec<(u32, u32)>; 2],
    /// the streams' vertex extent (the context's +0xE20)
    pub extent: u32,
}

fn fold(calls: &[mtstate::Call], f: &mut Folded) {
    for c in calls {
        let a = &c.args;
        match c.slot {
            37 => f.targets[a[0] as usize & 3] = a[1],
            39 => f.depth = a[0],
            47 => f.viewport = Some(std::array::from_fn(|k| c.data[k])),
            75 => f.scissor = Some(std::array::from_fn(|k| c.data[k] as i32)),
            57 => f.states.push((a[0], a[1])),
            100 => f.streams[a[0] as usize & 3] = (a[1], a[2], a[3]),
            104 => f.indices = a[0],
            87 => f.declaration = a[0],
            92 => f.vertex_shader = a[0],
            107 => f.pixel_shader = a[0],
            65 => f.textures.push((a[0], a[1])),
            69 => f.sampler_states.push((a[0], a[1], a[2])),
            // Set{Vertex,Pixel}ShaderConstant{F,I,B}(register, data, count)
            94 | 109 => {
                let stage = (c.slot == 109) as usize;
                for (k, v) in c.data.chunks_exact(4).enumerate() {
                    f.floats[stage].push((a[0] + k as u32, [v[0], v[1], v[2], v[3]]));
                }
            }
            96 | 111 => {
                let stage = (c.slot == 111) as usize;
                for (k, v) in c.data.chunks_exact(4).enumerate() {
                    f.ints[stage].push((a[0] + k as u32, std::array::from_fn(|j| v[j] as i32)));
                }
            }
            98 | 113 => {
                let stage = (c.slot == 113) as usize;
                for (k, &v) in c.data.iter().enumerate() {
                    f.bools[stage].push((a[0] + k as u32, v));
                }
            }
            _ => {}
        }
    }
}

/// Decode a draw record's bindings (applied in the executor's order:
/// targets, fixed state, streams, shader). `srgb`: sRender's byte +0x31.
pub fn decode(m: &dyn Mem, record: u32, invalid_viewport: [u32; 4], srgb: bool) -> Result<Folded, Refusal> {
    let [targets, fixed, shader, streams] = [4u32, 8, 0xC, 0x10].map(|o| m.u32(record.wrapping_add(o)));
    if targets == 0 || fixed == 0 || shader == 0 || streams == 0 {
        return Err(Refusal::NullBinding);
    }
    let metadata = m.u32(shader.wrapping_add(8));
    if metadata == 0 || m.u32(metadata.wrapping_add(0x10)) == 0 || m.u32(metadata.wrapping_add(0x14)) == 0 {
        // without both shaders the decoder would compare against the
        // scratch context's empty value array (and the D3D9 path refuses)
        return Err(Refusal::Missing);
    }
    for k in 0..4 {
        if m.u32(streams.wrapping_add(4 * k)) != 0 && m.u8(streams.wrapping_add(0x10 + k)) == 0 {
            return Err(Refusal::ZeroStride);
        }
    }
    let mut s = Scratch { m, context: vec![0; CONTEXT_BYTES as usize / 4] };
    let mut f = Folded::default();
    fold(&mtstate::targets(&mut s, SCRATCH_CONTEXT, targets, invalid_viewport), &mut f);
    fold(&mtstate::fixed(&mut s, SCRATCH_CONTEXT, fixed), &mut f);
    fold(&mtstate::streams(&mut s, SCRATCH_CONTEXT, streams), &mut f);
    // the filtering option only acts in the non-null branch, never taken here
    let calls = mtstate::shader(&mut s, SCRATCH_CONTEXT, shader, srgb, 0).map_err(|_| Refusal::Decode)?;
    fold(&calls, &mut f);
    f.extent = s.context[0xE20 / 4];
    Ok(f)
}

/// The draw a record makes: D3DPRIMITIVETYPE, primitive count, start,
/// base vertex and vertex count, as 0x00DAB480 computes them (mtdraw's
/// `primitives`, `draw_primitive`, `draw_indexed`).
pub fn draw_params(m: &dyn Mem, record: u32, kind: u32, extent: u32) -> Result<(u32, u32, u32, i32, u32), Refusal> {
    draw_params_of(&G::new(m), record, kind, extent)
}

// --- what shaders read ---------------------------------------------------------

/// The constant registers and samplers a shader reads (its own DEF
/// registers excluded: those override the device's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub floats: [u64; 4],
    pub ints: u16,
    pub bools: u16,
    pub samplers: u16,
}

pub fn reads(tokens: &[u32]) -> Result<Reads, Refusal> {
    use d3d9shader::{op, reg};
    let shader = d3d9shader::parse(tokens).map_err(|_| Refusal::Shader)?;
    let limit = if shader.stage == d3d9shader::Stage::Vertex { 256 } else { 224 };
    let mut r = Reads::default();
    let (mut def_f, mut def_i, mut def_b) = ([0u64; 4], 0u16, 0u16);
    let set = |bits: &mut [u64; 4], n: usize| {
        if n < 256 {
            bits[n / 64] |= 1 << (n % 64);
        }
    };
    for ins in &shader.instructions {
        if let Some(d) = &ins.dst {
            match (ins.opcode, d.kind) {
                (op::DEF, _) => set(&mut def_f, d.index as usize),
                (op::DEFI, _) => def_i |= 1 << (d.index & 15),
                (op::DEFB, _) => def_b |= 1 << (d.index & 15),
                (op::DCL, reg::SAMPLER) => r.samplers |= 1 << (d.index & 15),
                _ => {}
            }
        }
        for (source, p) in ins.src.iter().enumerate() {
            match p.kind {
                reg::CONST if p.relative.is_none() => {
                    let rows = if source == 1 {
                        match ins.opcode {
                            op::M4X4 | op::M3X4 => 4,
                            op::M4X3 | op::M3X3 => 3,
                            op::M3X2 => 2,
                            _ => 1,
                        }
                    } else {
                        1
                    };
                    for n in p.index as usize..p.index as usize + rows {
                        set(&mut r.floats, n);
                    }
                }
                reg::CONST | reg::CONST2 | reg::CONST3 | reg::CONST4 => {
                    for n in 0..limit {
                        set(&mut r.floats, n);
                    }
                }
                reg::CONSTINT => r.ints |= 1 << (p.index & 15),
                reg::CONSTBOOL => r.bools |= 1 << (p.index & 15),
                _ => {}
            }
        }
    }
    for k in 0..4 {
        r.floats[k] &= !def_f[k];
    }
    r.ints &= !def_i;
    r.bools &= !def_b;
    Ok(r)
}

// --- the DrawCall ---------------------------------------------------------------
//
// A draw's state comes from four bindings. The objects behind them are
// decoded once and cached: the fixed-state objects by identity (address and
// hash at +0xC, raster variant), shader metadata by address (its shaders
// and declaration checked again once a frame), sampler objects by identity.
// The bindings themselves live in the frame's command memory: target,
// stream and shader bindings are cached per frame by address (their values,
// the constants they point at included, do not change while the executor
// runs: the D3D9 path relies on that too, it skips uploads by pointer).
// Per draw: four lookups, the draw's own parameters and the flush.

/// Guest reads through the page table.
#[derive(Clone, Copy)]
struct G<'a> {
    m: &'a dyn Mem,
    pt: Option<std::ptr::NonNull<ddnative_base::mem::PageTable>>,
}

impl<'a> G<'a> {
    fn new(m: &'a dyn Mem) -> Self {
        G { m, pt: m.page_table() }
    }
    fn u32(&self, a: u32) -> u32 {
        self.m.fast_u32(self.pt, a)
    }
    fn u16(&self, a: u32) -> u32 {
        self.m.fast_u16(self.pt, a) as u32
    }
    fn u8(&self, a: u32) -> u32 {
        self.m.fast_u8(self.pt, a) as u32
    }
}

/// The render states the backend reads (crates/ddgpu `derive`/`try_draw`)
/// that the applies never set: taken from the device once a frame.
const DEVICE_STATES: [u32; 3] = [162, 194, 206];

/// The blend, depth-stencil and rasterizer objects of a fixed-state
/// binding: the states the full apply sets (0x00DAA5F0 with nothing
/// applied, mtstate's tables).
pub struct FixedPart {
    states: Vec<(u32, u32)>,
    /// blend-factor blending (see the module notes)
    blend_factor: bool,
}

fn fixed_part(g: &G, id: &NativeStateIdentity) -> FixedPart {
    let words = |a: u32, n: u32| -> Vec<u32> { (0..n).map(|k| g.u32(a.wrapping_add(4 * k))).collect() };
    let raster = id.raster.address.wrapping_add(0x20 + 12 * id.raster_flip as u32);
    let mut calls = mtstate::packed(mtstate::BLEND, &words(id.blend.address.wrapping_add(0x20), 2), None, None, 0);
    calls.extend(mtstate::packed(mtstate::DEPTH, &words(id.depth.address.wrapping_add(0x18), 2), None, None, 0));
    calls.extend(mtstate::packed(mtstate::RASTER, &words(raster, 3), None, None, 0));
    let states: Vec<(u32, u32)> = calls.iter().map(|c| (c.args[0], c.args[1])).collect();
    let state = |s: u32| states.iter().find(|e| e.0 == s).map(|e| e.1);
    // D3DBLEND_BLENDFACTOR / INVBLENDFACTOR
    let blend_factor = state(27) == Some(1) && [19, 20, 207, 208].iter().any(|&s| matches!(state(s), Some(14 | 15)));
    FixedPart { states, blend_factor }
}

/// A sampler descriptor (0x00DAA8B0): the stage, and the binding values
/// holding the SamplerState and the Texture.
#[derive(Clone, Copy, Debug)]
struct SamplerSlot {
    stage: u32,
    sampler: u32,
    texture: u32,
}

/// A constant descriptor (0x00DAA8B0): the section (0 float, 1 int,
/// 2 bool, 3 derived), the binding value, the source dword offset, the
/// first register and count, int-to-float conversion, the derived kind.
#[derive(Clone, Copy, Debug)]
struct Upload {
    section: u8,
    value: u32,
    offset: u32,
    register: u32,
    count: u32,
    convert: bool,
    kind: u32,
}

/// A shader metadata object: its shaders and declaration (handles and the
/// runtime's objects), what the shaders read, and its descriptors.
pub struct MetaPart {
    handles: [u32; 3],
    vs: Arc<[u32]>,
    ps: Arc<[u32]>,
    declaration: Arc<[[u8; 8]]>,
    floats: [usize; 2],
    reads: [Reads; 2],
    samplers: [Vec<SamplerSlot>; 2],
    uploads: [Vec<Upload>; 2],
    /// the frame it was last checked against the runtime's objects
    checked: std::sync::atomic::AtomicU64,
}

/// The runtime's shader bytecode and float count, and declaration, of
/// [vs, ps, declaration] handles.
fn shader_objects(handles: [u32; 3]) -> Option<((Arc<[u32]>, usize), (Arc<[u32]>, usize), Arc<[[u8; 8]]>)> {
    objects(|o| {
        let shader = |h: u32| match o.live.get(&h).map(|x| &x.kind) {
            Some(Kind::Shader { tokens, float_constants, .. }) => Some((tokens.clone(), *float_constants)),
            _ => None,
        };
        let declaration = match o.live.get(&handles[2]).map(|x| &x.kind) {
            Some(Kind::Declaration { elements, .. }) => Some(elements.clone()),
            _ => None,
        };
        Some((shader(handles[0])?, shader(handles[1])?, declaration?))
    })
}

fn meta_part(g: &G, metadata: u32, reads_cache: &mut FxMap<usize, (Arc<[u32]>, Reads)>, frame: u64) -> Result<MetaPart, Refusal> {
    let handles = [0x10, 0x14, 0x18].map(|o| g.u32(metadata.wrapping_add(o)));
    if handles[0] == 0 || handles[1] == 0 {
        return Err(Refusal::Missing);
    }
    let ((vs, vs_floats), (ps, ps_floats), declaration) = shader_objects(handles).ok_or(Refusal::Missing)?;
    let mut shader_reads = [Reads::default(); 2];
    for (k, tokens) in [&vs, &ps].into_iter().enumerate() {
        let key = Arc::as_ptr(tokens) as *const u32 as usize;
        shader_reads[k] = match reads_cache.get(&key) {
            Some((_, r)) => *r,
            None => {
                let r = reads(tokens)?;
                // the Arc is held, so its address is not reused while cached
                reads_cache.insert(key, (tokens.clone(), r));
                r
            }
        };
    }
    // the descriptors, in 0x00DAA8B0's order (mtstate::shader)
    let descriptors = g.u32(metadata.wrapping_add(4));
    let mut samplers: [Vec<SamplerSlot>; 2] = Default::default();
    let mut uploads: [Vec<Upload>; 2] = Default::default();
    for stage in 0..2usize {
        let pixel = stage == 1;
        let summary = g.u32(metadata.wrapping_add(if pixel { 12 } else { 8 }));
        let mut desc = descriptors.wrapping_add(if pixel { (g.u32(metadata.wrapping_add(8)) & 255) * 4 } else { 0 });
        for _ in 0..(summary >> 8 & 15) {
            let d = g.u32(desc);
            desc = desc.wrapping_add(4);
            samplers[stage].push(SamplerSlot { stage: (d & 4095) + if pixel { 0 } else { 257 }, sampler: d >> 12 & 1023, texture: d >> 22 });
        }
        for (section, count) in [(0u8, summary >> 12 & 127), (1, summary >> 19 & 15), (2, summary >> 23 & 31), (3, summary >> 28)] {
            for _ in 0..count {
                let d = g.u32(desc);
                desc = desc.wrapping_add(4);
                let u = Upload {
                    section,
                    value: d >> 1 & if section == 3 { 127 } else { 31 },
                    offset: d >> 6 & 1023,
                    register: d >> 16 & 255,
                    count: d >> 24,
                    convert: section == 0 && d & 1 != 0,
                    kind: d >> 8 & 255,
                };
                // derived constants other than these read undefined stack data
                if section == 3 && (!matches!(u.kind, 3 | 7) || u.count > 1) {
                    return Err(Refusal::Decode);
                }
                uploads[stage].push(u);
            }
        }
    }
    Ok(MetaPart {
        handles, vs, ps, declaration, floats: [vs_floats, ps_floats], reads: shader_reads, samplers, uploads,
        checked: frame.into(),
    })
}

/// A shader binding of this frame: its metadata, the alpha test, the
/// textures with their sampler states and the constant banks.
pub struct BindingPart {
    meta: Arc<MetaPart>,
    /// (ALPHAFUNC, ALPHAREF) when the binding enables the alpha test
    alpha: Option<(u32, u32)>,
    textures: Arc<[TextureBinding]>,
    constants: [Arc<Constants>; 2],
}

/// A shader binding's state as the full apply sets it (0x00DAA8B0 with
/// nothing applied: every sampler, texture and upload).
fn binding_part(g: &G, c: &mut Caches, info: &FrameInfo, binding: u32) -> Result<BindingPart, Refusal> {
    let flags = g.u32(binding);
    let alpha = if flags & 0x18_0000 == 0x18_0000 {
        let comparison = flags >> 3 & 255;
        // table 0x01445A70 has eight entries
        if comparison >= 8 {
            return Err(Refusal::Decode);
        }
        Some((comparison + 1, flags >> 11 & 255))
    } else {
        None
    };
    let metadata = g.u32(binding.wrapping_add(8));
    if metadata == 0 {
        return Err(Refusal::Missing);
    }
    let meta = c.meta(g, metadata)?;
    let value = |n: u32| g.u32(binding.wrapping_add(12 + 4 * n));
    // textures and sampler states (a later descriptor on a stage wins)
    let mut bound: Vec<(u32, u32, [u32; 14])> = Vec::new();
    for slot in meta.samplers.iter().flatten() {
        let (sampler, texture) = (value(slot.sampler), value(slot.texture));
        if sampler == 0 || texture == 0 {
            return Err(Refusal::Missing);
        }
        let mut row = c.frame_row(slot.stage);
        for (ty, v) in c.sampler(g, sampler) {
            row[ty as usize] = v;
        }
        if (1..=5).contains(&info.filtering) && row[6] == 3 {
            // 0x00DA7250's non-null branch (see the module notes)
            if info.filtering == 1 {
                row[5] = 2;
                row[6] = 2;
                row[7] = 2;
            } else {
                row[10] = 1 << (info.filtering - 1);
            }
        }
        if slot.stage < 16 && info.srgb {
            row[11] = (g.u32(texture.wrapping_add(0x20)) & 0x4000_0000 != 0 && g.u32(texture.wrapping_add(0x2C)) & 0x8_0000 == 0) as u32;
        }
        let handle = g.u32(texture.wrapping_add(0x34));
        match bound.iter_mut().find(|b| b.0 == slot.stage) {
            Some(b) => *b = (slot.stage, handle, row),
            None => bound.push((slot.stage, handle, row)),
        }
    }
    bound.retain(|b| b.1 != 0);
    bound.sort_unstable_by_key(|b| b.0);
    let mut textures = Vec::with_capacity(bound.len());
    for &(stage, handle, states) in &bound {
        let t = c.texture(handle).ok_or(Refusal::Missing)?;
        textures.push(TextureBinding { sampler: stage, resource: handle, states, ..t });
    }
    // constants: the binding's registers over zeros
    let mut banks = [
        Constants { f: vec![[0; 4]; meta.floats[0].min(FLOATS[0])], ..Default::default() },
        Constants { f: vec![[0; 4]; meta.floats[1].min(FLOATS[1])], ..Default::default() },
    ];
    let mut written = [([0u64; 4], 0u16, 0u16); 2];
    for stage in 0..2 {
        let bank = &mut banks[stage];
        let w = &mut written[stage];
        for u in &meta.uploads[stage] {
            let v = value(u.value);
            match u.section {
                3 => {
                    if u.count == 0 {
                        continue;
                    }
                    let dims = [g.u16(v.wrapping_add(0x20)), g.u16(v.wrapping_add(0x22)) & 0x3FFF, g.u16(v.wrapping_add(0x26)) & 0x7FF];
                    let mut r = [0u32; 4];
                    for k in 0..3 {
                        r[k] = if u.kind == 3 { (1.0f32 / dims[k] as f32).to_bits() } else { (dims[k] as f32).to_bits() };
                    }
                    set_float(bank, w, u.register, r);
                }
                0 => {
                    let at = (v & !15).wrapping_add(u.offset * 4);
                    let count = if v & 15 != 0 { u.count.min((v & 15) * 16) } else { u.count };
                    for i in 0..count {
                        let r: [u32; 4] = std::array::from_fn(|j| g.u32(at.wrapping_add(i * 16 + j as u32 * 4)));
                        let r = if u.convert { r.map(|x| (x as i32 as f32).to_bits()) } else { r };
                        set_float(bank, w, u.register + i, r);
                    }
                }
                1 => {
                    let at = (v & !15).wrapping_add(u.offset * 4);
                    for i in 0..u.count {
                        let n = u.register + i;
                        if n < 16 {
                            bank.i[n as usize] = std::array::from_fn(|j| g.u32(at.wrapping_add(i * 16 + j as u32 * 4)) as i32);
                            w.1 |= 1 << n;
                        }
                    }
                }
                _ => {
                    let at = (v & !15).wrapping_add(u.offset * 4);
                    for i in 0..u.count {
                        let n = u.register + i;
                        if n < 16 {
                            bank.b[n as usize] = g.u32(at.wrapping_add(i * 4));
                            w.2 |= 1 << n;
                        }
                    }
                }
            }
        }
        // every register and sampler the shaders read comes from here
        let r = &meta.reads[stage];
        if (0..4).any(|k| r.floats[k] & !w.0[k] != 0) {
            return Err(Refusal::FloatsUncovered);
        }
        if r.ints & !w.1 != 0 {
            return Err(Refusal::IntsUncovered);
        }
        if r.bools & !w.2 != 0 {
            return Err(Refusal::BoolsUncovered);
        }
        let base = if stage == 0 { 257 } else { 0 };
        if (0..16).any(|s| r.samplers >> s & 1 != 0 && !bound.iter().any(|b| b.0 == base + s)) {
            return Err(Refusal::SamplersUncovered);
        }
    }
    let [vs, ps] = banks;
    Ok(BindingPart { meta, alpha, textures: textures.into(), constants: [Arc::new(vs), Arc::new(ps)] })
}

fn set_float(bank: &mut Constants, written: &mut ([u64; 4], u16, u16), n: u32, r: [u32; 4]) {
    if n < 256 {
        written.0[n as usize / 64] |= 1 << (n % 64);
    }
    if let Some(x) = bank.f.get_mut(n as usize) {
        *x = r;
    }
}

/// A stream binding of this frame (0x00DAA7C0 with nothing applied).
pub struct StreamPart {
    streams: Arc<[(u32, u32, u32, u32, u32)]>,
    indices: u32,
    extent: u32,
}

fn stream_part(g: &G, binding: u32) -> Result<StreamPart, Refusal> {
    let mut streams = Vec::with_capacity(4);
    let mut extent = 0u32;
    for k in 0..4u32 {
        let vb = g.u32(binding.wrapping_add(4 * k));
        if vb == 0 {
            continue;
        }
        let offset = g.u32(binding.wrapping_add(0x14 + 4 * k));
        let stride = g.u8(binding.wrapping_add(0x10 + k));
        if stride == 0 {
            return Err(Refusal::ZeroStride);
        }
        extent = extent.max(g.u32(vb.wrapping_add(0x14)).wrapping_sub(offset) / stride);
        let handle = g.u32(vb.wrapping_add(0x20));
        if handle != 0 {
            // non-instanced: frequency 1
            streams.push((k, handle, offset, stride, 1));
        }
    }
    let ib = g.u32(binding.wrapping_add(0x28));
    Ok(StreamPart { streams: streams.into(), indices: if ib == 0 { 0 } else { g.u32(ib.wrapping_add(0x20)) }, extent })
}

/// A target binding of this frame (0x00DA6FE0 with nothing applied).
pub struct TargetPart {
    targets: [Option<Target>; 4],
    depth: Option<Target>,
    viewport: [u32; 6],
    scissor: [i32; 4],
}

fn target_part(g: &G, c: &mut Caches, info: &FrameInfo, binding: u32) -> Result<TargetPart, Refusal> {
    let views: [u32; 5] = std::array::from_fn(|k| g.u32(binding.wrapping_add(4 * k as u32)));
    if views[0] == 0 {
        return Err(Refusal::NoColourTarget);
    }
    let surface = |v: u32| if v == 0 { 0 } else { g.u32(v.wrapping_add(0x14)) };
    let targets: [Option<Target>; 4] = std::array::from_fn(|k| c.target(surface(views[k])));
    if targets[0].is_none() {
        return Err(Refusal::Missing);
    }
    let depth = c.target(surface(views[4]));
    let rect = |a: u32| -> [i32; 4] { std::array::from_fn(|k| g.u32(a.wrapping_add(4 * k as u32)) as i32) };
    let normalize = |r: &mut [i32; 4]| {
        if r[2] < r[0] {
            r[0] = 0;
            r[2] = 0;
        }
        if r[3] < r[1] {
            r[1] = 0;
            r[3] = 0;
        }
    };
    let alternate = (g.u32(binding.wrapping_add(0x14)) >> 9 & 1) * 16;
    let mut v = rect(binding.wrapping_add(0x18 + alternate));
    v[0] = v[0].max(0);
    v[1] = v[1].max(0);
    v[2] = v[2].min(g.u16(binding.wrapping_add(0x58)) as i32);
    v[3] = v[3].min(g.u16(binding.wrapping_add(0x5A)) as i32);
    normalize(&mut v);
    // a target was set, so the cached viewport is the invalid one: equal,
    // no SetViewport is made
    if v == info.invalid_viewport.map(|x| x as i32) {
        return Err(Refusal::ViewportGlobals);
    }
    let mut s = rect(binding.wrapping_add(0x38 + alternate));
    s[0] = s[0].max(v[0]);
    s[1] = s[1].max(v[1]);
    s[2] = s[2].min(v[2]);
    s[3] = s[3].min(v[3]);
    normalize(&mut s);
    let viewport = [v[0] as u32, v[1] as u32, (v[2] - v[0]) as u32, (v[3] - v[1]) as u32, 0, 1f32.to_bits()];
    Ok(TargetPart { targets, depth, viewport, scissor: s })
}

/// What a texture binding takes from the runtime's texture object.
fn texture_info(handle: u32) -> Option<TextureBinding> {
    objects(|o| match o.live.get(&handle).map(|x| &x.kind) {
        Some(Kind::Texture { data, .. }) => Some(TextureBinding {
            sampler: 0, resource: handle, states: [0; 14],
            kind: data.kind, format: data.format, width: data.width, height: data.height,
            depth: data.depth, levels: data.levels, usage: data.usage,
        }),
        _ => None,
    })
}

/// The device state a frame's taken draws take over: what the applies do
/// not set.
#[derive(Clone, Default)]
struct DeviceFrame {
    usable: bool,
    /// DEVICE_STATES, then ALPHAREF (24) and ALPHAFUNC (25), as present
    states: Vec<(u32, u32)>,
    /// sampler rows of stages 0..15 and 257..260
    rows: FxMap<u32, [u32; 14]>,
}

/// A render-state list: the fixed part, stencil reference, blend factor,
/// alpha test and the device's states.
#[derive(Clone, PartialEq, Eq, Hash)]
struct StatesKey {
    fixed: usize,
    stencil: u32,
    factor: u32,
    alpha: Option<(u32, u32)>,
    device: u64,
}

/// The caches (see above): across frames by identity, and this frame's.
#[derive(Default)]
pub struct Caches {
    frame: u64,
    device: DeviceFrame,
    /// a generation of the device's states (part of the render-state keys)
    device_generation: u64,
    reads: FxMap<usize, (Arc<[u32]>, Reads)>,
    fixed: FxMap<NativeStateIdentity, Arc<FixedPart>>,
    metas: FxMap<u32, Arc<MetaPart>>,
    samplers: FxMap<(u32, u32), Vec<(u32, u32)>>,
    states: FxMap<StatesKey, RenderStateList>,
    /// native fixed state by identity and render-state list (its address:
    /// the list is held in `states`)
    native_fixed: FxMap<(NativeStateIdentity, u64), Arc<NativeFixedState>>,
    // this frame's
    bindings: FxMap<u32, Result<Arc<BindingPart>, Refusal>>,
    streams: FxMap<u32, Result<Arc<StreamPart>, Refusal>>,
    target_bindings: FxMap<u32, Result<Arc<TargetPart>, Refusal>>,
    targets: FxMap<u32, Option<Target>>,
    textures: FxMap<u32, Option<TextureBinding>>,
    index_formats: FxMap<u32, Option<u32>>,
}

impl Caches {
    /// A new frame: what the device holds that the applies do not set
    /// (with the D3D lock held).
    pub fn begin_frame(&mut self, info: &FrameInfo) {
        self.frame += 1;
        self.bindings.clear();
        self.streams.clear();
        self.target_bindings.clear();
        self.targets.clear();
        self.textures.clear();
        self.index_formats.clear();
        let device = objects(|o| match o.live.get(&info.device).map(|x| &x.kind) {
            Some(Kind::Device(d)) => {
                let s = &d.state;
                let states: Vec<(u32, u32)> =
                    DEVICE_STATES.iter().chain(&[24, 25]).filter_map(|&k| s.render.get(&k).map(|&v| (k, v))).collect();
                let rows = (0..16).chain(257..261).map(|t| (t, s.sampler.row(t))).collect();
                // a taken draw carries streams 0..3 only
                let usable = s.in_scene && s.streams[4..].iter().all(|x| x.0 == 0);
                Some(DeviceFrame { usable, states, rows })
            }
            _ => None,
        })
        .unwrap_or_default();
        if device.states != self.device.states {
            self.device_generation += 1;
        }
        self.device = device;
        if self.states.len() > 4096 {
            self.states.clear();
            self.native_fixed.clear();
        }
    }

    fn frame_row(&self, stage: u32) -> [u32; 14] {
        self.device.rows.get(&stage).copied().unwrap_or([0; 14])
    }

    fn target(&mut self, surface: u32) -> Option<Target> {
        if surface == 0 {
            return None;
        }
        self.targets.entry(surface).or_insert_with(|| crate::d3d9draw::target(surface)).clone()
    }

    fn texture(&mut self, handle: u32) -> Option<TextureBinding> {
        self.textures.entry(handle).or_insert_with(|| texture_info(handle)).clone()
    }

    /// A SamplerState's (type, value) pairs (0x00DA7250, null old payload).
    fn sampler(&mut self, g: &G, sampler: u32) -> Vec<(u32, u32)> {
        let key = (sampler, g.u32(sampler.wrapping_add(0xC)));
        if let Some(v) = self.samplers.get(&key) {
            return v.clone();
        }
        let words: Vec<u32> = (0..3).map(|k| g.u32(sampler.wrapping_add(0x30 + 4 * k))).collect();
        let v: Vec<(u32, u32)> =
            mtstate::packed(mtstate::SAMPLER, &words, None, Some(0), 0).iter().map(|c| (c.args[1], c.args[2])).collect();
        self.samplers.insert(key, v.clone());
        v
    }

    fn meta(&mut self, g: &G, metadata: u32) -> Result<Arc<MetaPart>, Refusal> {
        if let Some(m) = self.metas.get(&metadata) {
            if m.checked.load(std::sync::atomic::Ordering::Relaxed) == self.frame {
                return Ok(m.clone());
            }
            // once a frame: the same shader and declaration objects
            let handles = [0x10, 0x14, 0x18].map(|o| g.u32(metadata.wrapping_add(o)));
            if handles == m.handles {
                if let Some(((vs, _), (ps, _), d)) = shader_objects(handles) {
                    if Arc::ptr_eq(&vs, &m.vs) && Arc::ptr_eq(&ps, &m.ps) && Arc::ptr_eq(&d, &m.declaration) {
                        m.checked.store(self.frame, std::sync::atomic::Ordering::Relaxed);
                        return Ok(m.clone());
                    }
                }
            }
        }
        let m = Arc::new(meta_part(g, metadata, &mut self.reads, self.frame)?);
        self.metas.insert(metadata, m.clone());
        Ok(m)
    }
}

/// What a taken draw needs from the frame: sRender's device, the options.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub device: u32,
    pub srgb: bool,
    pub filtering: u32,
    pub invalid_viewport: [u32; 4],
}

impl FrameInfo {
    pub fn read(m: &dyn Mem, render: u32) -> FrameInfo {
        let options = m.u32(FILTER_OPTIONS);
        FrameInfo {
            device: m.u32(render.wrapping_add(0x98)),
            srgb: m.u8(render.wrapping_add(0x31)) != 0,
            filtering: if options == 0 { 0 } else { m.u32(options.wrapping_add(0x34)) },
            invalid_viewport: std::array::from_fn(|k| m.u32(INVALID_VIEWPORT + 4 * k as u32)),
        }
    }
}

/// Look up `key` in a per-frame map, building it on a miss.
fn part<T>(map: &mut FxMap<u32, Result<Arc<T>, Refusal>>, key: u32, build: impl FnOnce() -> Result<T, Refusal>) -> Result<Arc<T>, Refusal> {
    if let Some(r) = map.get(&key) {
        return r.clone();
    }
    let r = build().map(Arc::new);
    map.insert(key, r.clone());
    r
}

/// The DrawCall the D3D9 path makes for draw record `record` (type 0 or
/// 1), with the D3D lock held and `caches.begin_frame` called for this
/// frame. The resources it reads are `reads_of` it.
pub fn draw_call(m: &dyn Mem, info: &FrameInfo, c: &mut Caches, record: u32, kind: u32) -> Result<DrawCall, Refusal> {
    if !c.device.usable {
        return Err(Refusal::Device);
    }
    let g = G::new(m);
    let [targets, fixed, shader, streams] = [4u32, 8, 0xC, 0x10].map(|o| g.u32(record.wrapping_add(o)));
    if targets == 0 || fixed == 0 || shader == 0 || streams == 0 {
        return Err(Refusal::NullBinding);
    }
    let t = {
        let mut map = std::mem::take(&mut c.target_bindings);
        let r = part(&mut map, targets, || target_part(&g, c, info, targets));
        c.target_bindings = map;
        r?
    };
    let s = part(&mut c.streams, streams, || stream_part(&g, streams))?;
    let b = {
        let mut map = std::mem::take(&mut c.bindings);
        let r = part(&mut map, shader, || binding_part(&g, c, info, shader));
        c.bindings = map;
        r?
    };
    // the fixed state: the objects by identity, the dynamic values
    let id = identity_of(&g, fixed);
    if id.blend.address == 0 || id.depth.address == 0 || id.raster.address == 0 {
        return Err(Refusal::NullBinding);
    }
    let fp = match c.fixed.get(&id) {
        Some(f) => f.clone(),
        None => {
            let f = Arc::new(fixed_part(&g, &id));
            c.fixed.insert(id, f.clone());
            f
        }
    };
    if fp.blend_factor {
        return Err(Refusal::BlendFactor);
    }
    let rgba = g.u32(fixed);
    let key = StatesKey {
        fixed: Arc::as_ptr(&fp) as usize,
        stencil: g.u8(fixed.wrapping_add(4)),
        factor: (rgba & 0xFF00_FF00) | (rgba & 255) << 16 | (rgba >> 16 & 255),
        alpha: b.alpha,
        device: c.device_generation,
    };
    let render_states = match c.states.get(&key) {
        Some(l) => l.clone(),
        None => {
            let mut list = fp.states.clone();
            list.push((193, key.factor));
            list.push((57, key.stencil));
            // the alpha test: context init (0x00DA6DA0) sets 15, 24 and 25
            // each frame, so the device holds what the last apply set
            list.push((15, key.alpha.is_some() as u32));
            if let Some((func, reference)) = key.alpha {
                list.push((25, func));
                list.push((24, reference));
            }
            for &(s, v) in &c.device.states {
                if !list.iter().any(|e| e.0 == s) {
                    list.push((s, v));
                }
            }
            // later calls win, then sorted by state
            let mut sorted: Vec<(u32, u32)> = Vec::with_capacity(list.len());
            for (s, v) in list {
                match sorted.iter_mut().find(|e| e.0 == s) {
                    Some(e) => e.1 = v,
                    None => sorted.push((s, v)),
                }
            }
            sorted.sort_unstable_by_key(|e| e.0);
            let l: RenderStateList = sorted.into();
            c.states.insert(key, l.clone());
            l
        }
    };
    // the draw
    if kind == 1 && s.indices == 0 {
        return Err(Refusal::NoIndices);
    }
    let (primitive, primitives, start, base_vertex, vertex_count) = draw_params_of(&g, record, kind, s.extent)?;
    let indices = if kind == 1 {
        let ib = s.indices;
        let format = *c.index_formats.entry(ib).or_insert_with(|| objects(|o| match o.live.get(&ib).map(|x| &x.kind) {
            Some(Kind::Buffer { data, .. }) => Some(data.format),
            _ => None,
        }));
        Indices::Buffer(ib, format.ok_or(Refusal::Missing)?)
    } else {
        Indices::None
    };
    Ok(DrawCall {
        primitive, primitives, start, base_vertex, min_index: 0, vertex_count,
        vertices: Vertices::Streams(s.streams.clone()),
        indices,
        vertex_shader: Some(b.meta.vs.clone()), pixel_shader: Some(b.meta.ps.clone()),
        declaration: b.meta.declaration.clone(),
        // the runtime's FVF stays 0 once declarations are used
        fvf: 0,
        vs_constants: b.constants[0].clone(), ps_constants: b.constants[1].clone(),
        textures: b.textures.clone(),
        render_states,
        viewport: t.viewport,
        scissor: t.scissor,
        targets: t.targets.clone(),
        depth: t.depth.clone(),
    })
}

/// The resources a DrawCall reads, as the D3D9 path flushes them:
/// textures, streams, indices, render targets.
pub fn reads_of(d: &DrawCall) -> impl Iterator<Item = u32> + '_ {
    let streams = match &d.vertices {
        Vertices::Streams(s) => &s[..],
        Vertices::Inline { .. } => &[][..],
    };
    let indices = match d.indices {
        Indices::Buffer(r, _) => Some(r),
        _ => None,
    };
    d.textures.iter().map(|t| t.resource)
        .chain(streams.iter().map(|s| s.1))
        .chain(indices)
        .chain(d.targets.iter().flatten().map(|t| t.surface))
}

fn identity_of(g: &G, fixed: u32) -> NativeStateIdentity {
    let id = |o: u32| {
        let address = g.u32(fixed.wrapping_add(o));
        NativeObjectIdentity { address, hash: if address == 0 { 0 } else { g.u32(address.wrapping_add(0xC)) } }
    };
    NativeStateIdentity { blend: id(8), depth: id(0xC), raster: id(0x10), raster_flip: g.u32(fixed.wrapping_add(4)) >> 8 & 1 != 0 }
}

fn draw_params_of(g: &G, record: u32, kind: u32, extent: u32) -> Result<(u32, u32, u32, i32, u32), Refusal> {
    let code = (g.u32(g.u32(record.wrapping_add(0xC))) >> 21) & 0xFF;
    let count = g.u32(record.wrapping_add(0x14));
    let (primitive, primitives) = match code.wrapping_sub(1) {
        0 => (2, count >> 1),
        1 => (3, count.wrapping_sub(1)),
        2 => (4, count / 3),
        3 => (5, count.wrapping_sub(2)),
        _ => (1, count),
    };
    let vertices = crate::d3d9draw::vertex_count(primitive, primitives).ok_or(Refusal::Primitive)?;
    let start = g.u32(record.wrapping_add(0x18));
    Ok(match kind {
        0 => (primitive, primitives, start, 0, vertices),
        _ => {
            let base = g.u32(record.wrapping_add(0x1C));
            (primitive, primitives, start, base as i32, extent.wrapping_sub(base))
        }
    })
}

/// The fixed-state objects of a draw record: address and hash (+0xC), and
/// the rasterizer variant (fixed binding +4 bit 8).
pub fn identity(m: &dyn Mem, record: u32) -> NativeStateIdentity {
    let g = G::new(m);
    identity_of(&g, g.u32(record.wrapping_add(8)))
}

/// A taken draw as a native draw: the DrawCall's state; the constants as
/// one write of each bank the DrawCall carries (every register the shaders
/// read, see draw_call).
pub fn native_draw(d: &DrawCall, identity: NativeStateIdentity, caches: &mut Caches) -> Result<NativeDraw, Refusal> {
    let Vertices::Streams(streams) = &d.vertices else { return Err(Refusal::Device) };
    // non-instanced: every stream steps once per vertex
    if streams.iter().any(|s| s.4 != 1) {
        return Err(Refusal::Device);
    }
    let (Some(vertex_shader), Some(pixel_shader)) = (d.vertex_shader.clone(), d.pixel_shader.clone()) else {
        return Err(Refusal::Missing);
    };
    let state = caches.native_fixed.entry((identity, d.render_states.address())).or_insert_with(|| {
        Arc::new(NativeFixedState { identity, render_states: d.render_states.iter().copied().collect::<Vec<_>>().into() })
    }).clone();
    let constants = |c: &Constants| NativeConstants {
        floats: vec![NativeFloatWrite { first: 0, registers: c.f.as_slice().into() }],
        ints: vec![NativeIntWrite { first: 0, registers: c.i.as_slice().into() }],
        bools: vec![NativeBoolWrite { first: 0, registers: c.b.as_slice().into() }],
    };
    let scissor = d.render_states.iter().any(|&(s, v)| s == 174 && v != 0);
    Ok(NativeDraw {
        state,
        vertex_shader,
        pixel_shader,
        declaration: d.declaration.clone(),
        textures: d.textures.clone(),
        streams: streams.iter().map(|s| NativeStream { stream: s.0, resource: s.1, offset: s.2, stride: s.3 }).collect::<Vec<_>>().into(),
        indices: match d.indices {
            Indices::Buffer(resource, format) => Some(NativeIndices { resource, format }),
            _ => None,
        },
        vs_constants: constants(&d.vs_constants),
        ps_constants: constants(&d.ps_constants),
        viewport: d.viewport,
        scissor: scissor.then_some(d.scissor),
        primitive: d.primitive,
        primitives: d.primitives,
        start: d.start,
        base_vertex: d.base_vertex,
        min_index: d.min_index,
        vertex_count: d.vertex_count,
        instances: 1,
    })
}

/// The native draws taken since the last command left to the default
/// path, and their DrawCalls (sent instead if the backend rejects the job).
#[derive(Default)]
pub struct Pending {
    passes: Vec<NativePass>,
    calls: Vec<DrawCall>,
}

impl Pending {
    /// Add a draw: to the last pass if it renders into the same targets.
    pub fn push(&mut self, native: NativeDraw, call: DrawCall) {
        match self.passes.last_mut() {
            Some(p) if p.colours == call.targets && p.depth == call.depth => p.draws.push(native),
            _ => self.passes.push(NativePass {
                colours: call.targets.clone(), depth: call.depth.clone(), draws: vec![native], ..Default::default()
            }),
        }
        self.calls.push(call);
    }

    /// The job, and the DrawCalls to send if the backend rejects it.
    pub fn take(&mut self) -> Option<(NativeJob, Vec<DrawCall>)> {
        if self.passes.is_empty() {
            return None;
        }
        Some((NativeJob { passes: std::mem::take(&mut self.passes) }, std::mem::take(&mut self.calls)))
    }

    /// Send the job; the draws' count, and whether the backend took it.
    pub fn send(&mut self) -> Option<(u64, bool)> {
        let (job, calls) = self.take()?;
        let n = calls.len() as u64;
        crate::d3d9draw::count_draws(n);
        let accepted = crate::d3d9draw::backend_unlocked(|b| match b.native(job) {
            Ok(()) => true,
            Err(_) => {
                for c in &calls {
                    b.draw(c);
                }
                false
            }
        });
        Some((n, accepted))
    }
}

// --- the sink ------------------------------------------------------------------

/// One frame's counts.
#[derive(Default)]
struct Frame {
    kinds: [u64; 16],
    /// draws (types 0..3) by their first colour target's size
    draws_by_size: FxMap<(u32, u32), u64>,
    states: FxSet<u32>,
    shaders: FxSet<u32>,
    streams: FxSet<u32>,
    targets: FxSet<u32>,
}

/// Totals since the last report.
#[derive(Default)]
struct Totals {
    frames: u64,
    kinds: [u64; 16],
    scene_draws: u64,
    other_draws: u64,
    states: u64,
    shaders: u64,
    streams: u64,
    targets: u64,
    sizes: FxMap<(u32, u32), u64>,
    /// scene draws the native path takes (or, census, could take)
    taken: u64,
    refused: FxMap<Refusal, u64>,
    /// time spent deciding and taking scene draws
    decode: Duration,
    filtering: u32,
    /// native jobs sent, and their draws the backend took or rejected
    jobs: u64,
    native: u64,
    rejected: u64,
}

struct Inner {
    frame: Frame,
    totals: Totals,
    since: Instant,
    info: FrameInfo,
    /// the scene size: the largest colour target of the previous frame
    scene: (u32, u32),
    caches: Caches,
    pending: Pending,
    comparison: comparison::Comparison,
}

struct Native {
    mode: Mode,
    inner: Mutex<Inner>,
    every: Duration,
}

impl Native {
    fn new(mode: Mode, every: Duration) -> Self {
        Native {
            mode,
            inner: Mutex::new(Inner {
                frame: Frame::default(), totals: Totals::default(), since: Instant::now(),
                info: FrameInfo::default(), scene: (0, 0), caches: Caches::default(), pending: Pending::default(),
                comparison: comparison::Comparison::default(),
            }),
            every,
        }
    }
}

/// The first colour target's (width, height) of a draw record; (0, 0)
/// without one.
fn target_size(m: &dyn Mem, record: u32) -> (u32, u32) {
    let g = G::new(m);
    let binding = g.u32(record.wrapping_add(4));
    if binding == 0 {
        return (0, 0);
    }
    let view = g.u32(binding);
    if view == 0 {
        return (0, 0);
    }
    (g.u16(view.wrapping_add(0x18)), g.u16(view.wrapping_add(0x1A)) & 0x7FFF)
}

impl CommandSink for Native {
    fn frame_begin(&self, m: &mut dyn Mem, render: u32, _context: u32) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let g = &mut *g;
        g.info = FrameInfo::read(m, render);
        if self.mode != Mode::Off {
            let _lock = crate::d3d9dev::device_lock();
            g.caches.begin_frame(&g.info);
        }
    }

    fn command(&self, m: &mut dyn Mem, _context: u32, record: u32, kind: u32) -> Taken {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let g = &mut *g;
        let taken = self.decide(g, m, record, kind);
        if taken == Taken::Default {
            // what the native path took goes before this command
            g.send();
        }
        taken
    }

    fn frame_end(&self, _m: &mut dyn Mem, _render: u32, _context: u32) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let g = &mut *g;
        g.send();
        g.end_frame(self.mode, self.every);
    }
}

impl Inner {
    fn send(&mut self) {
        if self.comparison.is_active() {
            crate::d3d9draw::backend_unlocked(|b| self.comparison.finish(b));
        }
        if let Some((n, accepted)) = self.pending.send() {
            self.totals.jobs += 1;
            if accepted {
                self.totals.native += n;
            } else {
                self.totals.rejected += n;
            }
        }
    }
}

impl Native {
    fn decide(&self, g: &mut Inner, m: &mut dyn Mem, record: u32, kind: u32) -> Taken {
        let f = &mut g.frame;
        f.kinds[kind as usize & 15] += 1;
        if kind > 3 {
            return Taken::Default;
        }
        let size = target_size(m, record);
        *f.draws_by_size.entry(size).or_default() += 1;
        if self.mode == Mode::Census {
            f.targets.insert(m.u32(record.wrapping_add(4)));
            f.states.insert(m.u32(record.wrapping_add(8)));
            f.shaders.insert(m.u32(record.wrapping_add(0xC)));
            f.streams.insert(m.u32(record.wrapping_add(0x10)));
        }
        if size != g.scene || size == (0, 0) {
            return Taken::Default;
        }
        let t = &mut g.totals;
        if kind >= 2 {
            *t.refused.entry(Refusal::Instanced).or_default() += 1;
            return Taken::Default;
        }
        let started = Instant::now();
        let lock = crate::d3d9dev::device_lock();
        let decoded = draw_call(m, &g.info, &mut g.caches, record, kind);
        let taken = match decoded {
            Err(r) => {
                *t.refused.entry(r).or_default() += 1;
                Taken::Default
            }
            Ok(_) if self.mode == Mode::Census => {
                t.taken += 1;
                Taken::Default
            }
            Ok(d) if self.mode == Mode::DrawCall => {
                crate::d3d9draw::flush_reads(m, reads_of(&d));
                crate::d3d9draw::backend_unlocked(|b| b.draw(&d));
                crate::d3d9draw::count_draws(1);
                t.taken += 1;
                Taken::Handled
            }
            Ok(d) => match native_draw(&d, identity(m, record), &mut g.caches) {
                Err(r) => {
                    *t.refused.entry(r).or_default() += 1;
                    Taken::Default
                }
                Ok(native) => {
                    if self.mode == Mode::Compare {
                        // Finish before any upload can replace resources used
                        // by an earlier draw in the pending shadow pass.
                        let dirty = objects(|o| reads_of(&d).any(|r| o.dirty.contains(&r)));
                        if dirty {
                            crate::d3d9draw::backend_unlocked(|b| g.comparison.finish(b));
                        }
                        crate::d3d9draw::flush_reads(m, reads_of(&d));
                        crate::d3d9draw::backend_unlocked(|b| g.comparison.draw(b, native, &d));
                        t.taken += 1;
                        drop(lock);
                        t.decode += started.elapsed();
                        return Taken::Handled;
                    }
                    crate::d3d9draw::flush_reads(m, reads_of(&d));
                    g.pending.push(native, d);
                    t.taken += 1;
                    Taken::Handled
                }
            },
        };
        drop(lock);
        t.decode += started.elapsed();
        taken
    }
}

impl Inner {
    fn end_frame(&mut self, mode: Mode, every: Duration) {
        let g = self;
        let f = std::mem::take(&mut g.frame);
        let t = &mut g.totals;
        t.frames += 1;
        t.filtering = g.info.filtering;
        for k in 0..16 {
            t.kinds[k] += f.kinds[k];
        }
        // the main scene passes: the frame's largest colour target
        let scene = f.draws_by_size.keys().copied().max_by_key(|&(w, h)| w as u64 * h as u64);
        for (&size, &n) in &f.draws_by_size {
            if Some(size) == scene {
                t.scene_draws += n;
            } else {
                t.other_draws += n;
            }
            *t.sizes.entry(size).or_default() += n;
        }
        g.scene = scene.unwrap_or((0, 0));
        t.states += f.states.len() as u64;
        t.shaders += f.shaders.len() as u64;
        t.streams += f.streams.len() as u64;
        t.targets += f.targets.len() as u64;
        if g.since.elapsed() >= every {
            report(mode, t);
            if mode == Mode::Compare {
                let c = std::mem::take(&mut g.comparison.stats);
                eprintln!("ndraw native compare: begun={} submitted={} rejected={} oracle_draws={} shadow_draws={} (GPU verification reported asynchronously)",
                    c.begun, c.submitted, c.rejected, c.oracle_draws, c.shadow_draws);
            }
            *t = Totals::default();
            g.since = Instant::now();
        }
    }
}

fn report(mode: Mode, t: &Totals) {
    if t.frames == 0 {
        return;
    }
    let per = |n: u64| n as f64 / t.frames as f64;
    let names = ["draw", "indexed", "instanced", "instanced indexed", "clear", "copy", "nested", "span"];
    let mut out = format!("ndraw native ({mode:?}): {} frames, per frame:\n", t.frames);
    for (k, name) in names.iter().enumerate() {
        out += &format!("  {name:>18}: {:8.1}\n", per(t.kinds[k]));
    }
    let other: u64 = t.kinds[8..].iter().sum();
    out += &format!("  {:>18}: {:8.1}\n", "types 8-15", per(other));
    out += &format!("  draws into the largest colour target {:.1}, into others {:.1}\n", per(t.scene_draws), per(t.other_draws));
    out += &format!("  distinct objects: states {:.1}, shader bindings {:.1}, stream bindings {:.1}, target bindings {:.1}\n",
                    per(t.states), per(t.shaders), per(t.streams), per(t.targets));
    let decided = t.taken + t.refused.values().sum::<u64>();
    out += &format!("  scene draws {} {:.1}, refused {:.1}; {:.2} us per decided draw; filtering option {}\n",
                    if mode == Mode::Census { "takeable" } else { "taken" }, per(t.taken), per(decided - t.taken),
                    if decided == 0 { 0.0 } else { t.decode.as_secs_f64() * 1e6 / decided as f64 }, t.filtering);
    if mode == Mode::Native {
        out += &format!("  native jobs {:.1}, draws taken by the backend {:.1}, rejected (sent as DrawCalls) {:.1}\n",
                        per(t.jobs), per(t.native), per(t.rejected));
    }
    out += "  refused:";
    for r in Refusal::ALL {
        if let Some(&n) = t.refused.get(&r) {
            out += &format!(" {r:?} {:.1};", per(n));
        }
    }
    out += "\n  draws by first colour target size:";
    let mut sizes: Vec<_> = t.sizes.iter().collect();
    sizes.sort_by(|a, b| b.1.cmp(a.1));
    for ((w, h), n) in sizes.into_iter().take(8) {
        out += &format!(" {w}x{h} {:.1};", per(*n));
    }
    eprintln!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestMem(HashMap<u32, u8>);

    impl Mem for TestMem {
        fn u8(&self, a: u32) -> u8 {
            self.0.get(&a).copied().unwrap_or(0)
        }
        fn set_u8(&mut self, a: u32, v: u8) {
            self.0.insert(a, v);
        }
    }

    /// A draw record with a target binding whose first view is w x h
    /// (the height's top bit set, as an sRGB view has it).
    fn record(m: &mut TestMem, at: u32, (w, h): (u16, u16), state: u32) -> u32 {
        let (binding, view) = (at + 0x100, at + 0x200);
        m.set_u32(at + 4, binding);
        m.set_u32(at + 8, state);
        m.set_u32(binding, view);
        m.set_u16(view + 0x18, w);
        m.set_u16(view + 0x1A, h | 0x8000);
        at
    }

    #[test]
    fn the_census_takes_nothing_and_counts_scene_draws() {
        let mut m = TestMem::default();
        let scene = record(&mut m, 0x1000, (1920, 1080), 0x50);
        let shadow = record(&mut m, 0x2000, (2048, 512), 0x60);
        let small = record(&mut m, 0x3000, (480, 270), 0x50);
        assert_eq!(target_size(&m, scene), (1920, 1080));
        let census = Native::new(Mode::Census, Duration::from_secs(3600));
        for (r, kind) in [(scene, 1), (scene, 1), (shadow, 0), (small, 3), (scene, 4)] {
            assert_eq!(census.command(&mut m, 0, r, kind), Taken::Default);
        }
        census.frame_end(&mut m, 0, 0);
        let g = census.inner.lock().unwrap();
        let t = &g.totals;
        assert_eq!(t.frames, 1);
        assert_eq!((t.kinds[0], t.kinds[1], t.kinds[3], t.kinds[4]), (1, 2, 1, 1));
        // the largest colour target here is 1920x1080: two indexed draws
        assert_eq!((t.scene_draws, t.other_draws), (2, 2));
        assert_eq!(t.states, 2);
        // which becomes the next frame's scene size
        assert_eq!(g.scene, (1920, 1080));
    }

    /// vs_3_0: dcl_position v0; dcl_2d s1; m4x4 o0, v0, c4; texldl; mov
    /// using c10 (relative-free), i2 by a loop, b3 by an if, c11 defined.
    #[test]
    fn a_shader_reads_the_registers_its_instructions_name_minus_its_defs() {
        let t = |op: u32, len: u32| op | len << 24;
        let src = |kind: u32, n: u32| 0x8000_0000 | n | (kind & 7) << 28 | (kind >> 3) << 11 | 0xE4 << 16;
        let dst = |kind: u32, n: u32| 0x8000_0000 | n | (kind & 7) << 28 | (kind >> 3) << 11 | 0xF << 16;
        let tokens = vec![
            0xFFFE_0300,
            t(31, 2), 0x8000_0000, dst(1, 0),                     // dcl_position v0
            t(31, 2), 0x9000_0000, dst(10, 1),                    // dcl_2d s1
            t(81, 5), dst(2, 11), 0, 0, 0, 0,                     // def c11, 0,0,0,0
            t(20, 3), dst(6, 0), src(1, 0), src(2, 4),            // m4x4 o0, v0, c4
            t(1, 2), dst(0, 0), src(2, 10),                       // mov r0, c10
            t(1, 2), dst(0, 1), src(2, 11),                       // mov r1, c11 (defined)
            t(38, 1), src(7, 2),                                  // rep i2
            t(39, 0),                                             // endrep
            t(40, 1), src(14, 3),                                 // if b3
            t(43, 0),                                             // endif
            0x0000_FFFF,
        ];
        let r = reads(&tokens).expect("parses");
        assert_eq!(r.floats, [0xF0 | 1 << 10, 0, 0, 0]);
        assert_eq!((r.ints, r.bools, r.samplers), (1 << 2, 1 << 3, 1 << 1));
    }

    /// Bindings for one indexed scene draw, built as the constructors lay
    /// them out (docs/NDRAW.md sections 2, 3 and 5).
    fn scene_record(m: &mut TestMem) -> u32 {
        let (record, targets, fixed, shader, streams) = (0x1_0000, 0x1_1000, 0x1_2000, 0x1_3000, 0x1_4000);
        let (rtv, dsv, blend, depth, raster) = (0x2_0000, 0x2_1000, 0x2_2000, 0x2_3000, 0x2_4000);
        let (meta, descs, vb, ib, values) = (0x3_0000, 0x3_1000, 0x3_2000, 0x3_3000, 0x3_4000);
        let (sampler, texture) = (0x3_5000, 0x3_6000);
        m.set_u16(record + 2, 1);
        for (o, v) in [(4, targets), (8, fixed), (0xC, shader), (0x10, streams), (0x14, 300), (0x18, 6), (0x1C, 2)] {
            m.set_u32(record + o, v);
        }
        // targets: RTV 0 and the DSV; viewport and scissor rectangles; size
        m.set_u32(targets, rtv);
        m.set_u32(targets + 0x10, dsv);
        m.set_u32(rtv + 0x14, 0xA000);
        m.set_u32(dsv + 0x14, 0xA100);
        for (k, v) in [0, 0, 1920, 1080].into_iter().enumerate() {
            m.set_u32(targets + 0x18 + 4 * k as u32, v);
            m.set_u32(targets + 0x38 + 4 * k as u32, v);
        }
        m.set_u16(targets + 0x58, 1920);
        m.set_u16(targets + 0x5A, 1080);
        // fixed state: factor, stencil reference 5, the three objects
        m.set_u32(fixed, 0x1122_3344);
        m.set_u32(fixed + 4, 5);
        m.set_u32(fixed + 8, blend);
        m.set_u32(fixed + 0xC, depth);
        m.set_u32(fixed + 0x10, raster);
        // blend on, SRCALPHA / INVSRCALPHA add; write mask 15
        m.set_u32(blend + 0x20, 1 | 5 << 1 | 6 << 6 | 1 << 11 | 2 << 14 | 1 << 19 | 1 << 24 | 15 << 27);
        // depth on, write, LESSEQUAL
        m.set_u32(depth + 0x18, 1 | 1 << 1 | 4 << 2);
        // solid, cull CCW, scissor on
        m.set_u32(raster + 0x20, 3 | 3 << 8 | 1 << 17);
        // streams: one VB (stride 32, 3200 bytes), an IB
        m.set_u32(streams, vb);
        m.set_u8(streams + 0x10, 32);
        m.set_u32(streams + 0x28, ib);
        m.set_u32(vb + 0x14, 3200);
        m.set_u32(vb + 0x20, 0xB000);
        m.set_u32(ib + 0x20, 0xB100);
        // shader binding: triangle list, metadata, two values
        m.set_u32(shader, 3 << 21);
        m.set_u32(shader + 8, meta);
        m.set_u32(shader + 12, sampler);
        m.set_u32(shader + 16, texture);
        m.set_u32(shader + 20, values);
        m.set_u32(meta + 4, descs);
        m.set_u32(meta + 0x10, 0xC000);
        m.set_u32(meta + 0x14, 0xC100);
        m.set_u32(meta + 0x18, 0xC200);
        // VS: one float upload (c4, 4 registers from value 2); PS: sampler
        // 0 (sampler value 0, texture value 1)
        m.set_u32(meta + 8, 1 | 1 << 12);
        m.set_u32(meta + 12, 1 | 1 << 8);
        m.set_u32(descs, 2 << 1 | 4 << 16 | 4 << 24);
        m.set_u32(descs + 4, 0 | 1 << 22);
        // a float value is the address of its registers
        for k in 0..16 {
            m.set_u32(values + 4 * k, 0x3F80_0000 + k);
        }
        m.set_u32(sampler + 0x30, 3 | 3 << 3 | 3 << 6 | 2 << 9 | 2 << 13 | 2 << 17);
        m.set_u32(texture + 0x34, 0xD000);
        record
    }

    #[test]
    fn a_record_decodes_to_the_state_its_bindings_set() {
        let mut m = TestMem::default();
        let record = scene_record(&mut m);
        let f = decode(&m, record, [0, 0, 0, 0], false).expect("decodes");
        assert_eq!((f.targets, f.depth), ([0xA000, 0, 0, 0], 0xA100));
        assert_eq!(f.viewport, Some([0, 0, 1920, 1080, 0, 1f32.to_bits()]));
        assert_eq!(f.scissor, Some([0, 0, 1920, 1080]));
        let get = |s| f.states.iter().rev().find(|e| e.0 == s).map(|e| e.1);
        assert_eq!((get(27), get(19), get(20), get(171), get(168)), (Some(1), Some(5), Some(6), Some(1), Some(15)));
        assert_eq!((get(7), get(14), get(23), get(57)), (Some(1), Some(1), Some(4), Some(5)));
        assert_eq!((get(8), get(22), get(174)), (Some(3), Some(3), Some(1)));
        // the factor with red and blue swapped (ARGB)
        assert_eq!(get(193), Some(0x1144_3322));
        assert_eq!((f.streams[0], f.indices), ((0xB000, 0, 32), 0xB100));
        assert_eq!(f.extent, 100);
        assert_eq!((f.vertex_shader, f.pixel_shader, f.declaration), (0xC000, 0xC100, 0xC200));
        assert_eq!(f.textures, vec![(0, 0xD000)]);
        assert_eq!(f.floats[0].iter().map(|e| e.0).collect::<Vec<_>>(), vec![4, 5, 6, 7]);
        assert_eq!(f.floats[0][1].1, [0x3F80_0004, 0x3F80_0005, 0x3F80_0006, 0x3F80_0007]);
        let ty = |t| f.sampler_states.iter().find(|e| e.0 == 0 && e.1 == t).map(|e| e.2);
        assert_eq!((ty(1), ty(5), ty(6), ty(7)), (Some(3), Some(2), Some(2), Some(2)));
        // indexed: start index 6, base vertex 2, 100 - 2 vertices, 100 triangles
        assert_eq!(draw_params(&m, record, 1, f.extent), Ok((4, 100, 6, 2, 98)));
    }
}
