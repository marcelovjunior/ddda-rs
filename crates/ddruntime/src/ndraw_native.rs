//! The native path through nDraw's command executor (L119, step N2 of
//! docs/NATIVE_RENDERER.md): a `mtdraw::sink::CommandSink` installed over
//! the executor port.
//!
//! `DDDRAW_NATIVE` selects it. The default (unset) is `drawcall`, since
//! `verify` found every decoded DrawCall equal to the D3D9 path's over a
//! gameplay run (combo19, ~420,000 draws per 10 s, no mismatch); `0` or
//! `off` turns the native path off (every command through the D3D9 path).
//! The summary on stderr is printed every `DDDRAW_NATIVE_EVERY` seconds:
//! by default every 10 s when `DDDRAW_NATIVE` is set, never when it is not.
//!
//! - `census`: takes no command (every one runs the default D3D9 path,
//!   unchanged) and measures, per frame, the commands by type, the draws
//!   into the frame's largest colour target (the main scene passes) and
//!   into other targets, the distinct state, shader-binding, stream and
//!   target objects, and how many scene draws the native path could take
//!   (and why the others not).
//! - `drawcall` (the default): takes the scene draws it can and sends the backend the
//!   `DrawCall` the D3D9 path would have sent for them, decoded straight
//!   from the nDraw objects. The applies, the D3D9 state setters and the
//!   snapshot are skipped; the backend draws them exactly as before.
//! - `verify`: takes nothing; for every scene draw it could take, the
//!   DrawCall the D3D9 path then makes is compared with the one it would
//!   have sent (every field the backend reads: render states, the
//!   registers and samplers the shaders read, targets, streams, ...);
//!   the report counts the comparisons and the mismatches by field and
//!   prints the first ones.
//! - `1` or `native`: takes the same draws as native jobs (d3d9cmd::NativeJob, L126):
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
//! Samplers and textures depend on what ran before (combo17's verify run:
//! nearly every draw differed): 0x00DAA8B0 applies a stage's SamplerState
//! only when the context's cached object changes, the filtering option at
//! [0x018D1D20]+0x34 overrides anisotropic samplers only when an old one
//! was cached (0x00DA7250's non-null branch), fields are emitted only where
//! they differ from the old object's raw ones, and a texture is set by its
//! object (a new D3D texture under the same object is not). So a taken
//! draw runs that part of the apply itself, on the executor's context and
//! the runtime's device (`shadow_samplers`, a port of 0x00DAA8B0's sampler
//! section), and takes its textures and sampler states from the device, as
//! the D3D9 path's snapshot does.
//!
//! Otherwise a taken command changes neither the device nor the context.
//! When the default path takes over again, the context's last fixed-state
//! and shader bindings and their objects are cleared (`invalidate`), so the
//! next applies run in full instead of comparing against bindings the taken
//! draws went past, and the constant registers the taken draws uploaded
//! are written to the device: the default path then sees the device the
//! D3D9 path would have left.
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
    /// take nothing; compare the D3D9 path's DrawCalls with the decoded ones
    Verify,
}

pub fn mode() -> Mode {
    mode_of(std::env::var("DDDRAW_NATIVE").ok().as_deref())
}

/// The mode a `DDDRAW_NATIVE` value (None: unset) asks for.
pub fn mode_of(value: Option<&str>) -> Mode {
    match value {
        None | Some("drawcall") => Mode::DrawCall,
        Some("0") | Some("off") => Mode::Off,
        Some("census") => Mode::Census,
        Some("verify") => Mode::Verify,
        Some("1") | Some("native") => Mode::Native,
        Some("compare") => Mode::Compare,
        Some(other) => {
            eprintln!("DDDRAW_NATIVE={other}: not a mode (0/off, drawcall, verify, compare, census, 1/native); using drawcall");
            Mode::DrawCall
        }
    }
}

/// Install the sink `mode()` asks for (before the program runs).
pub fn install() {
    let mode = mode();
    if mode == Mode::Off {
        return;
    }
    // the summary: asked for, or with the mode chosen explicitly
    let every = match std::env::var("DDDRAW_NATIVE_EVERY").ok().and_then(|v| v.parse::<u64>().ok()) {
        Some(n) => Duration::from_secs(n.max(1)),
        None if std::env::var_os("DDDRAW_NATIVE").is_some() => Duration::from_secs(10),
        None => Duration::MAX,
    };
    mtdraw::sink::set_sink(Some(Arc::new(Native::new(mode, every))));
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
    /// inside a span of type-7 commands (a query begun and not ended): the
    /// D3D9 path draws it, so the backend counts its samples
    Query,
}

impl Refusal {
    const ALL: [Refusal; 18] = [
        Refusal::Instanced, Refusal::NotScene, Refusal::NullBinding, Refusal::Decode, Refusal::ViewportGlobals,
        Refusal::BlendFactor, Refusal::NoColourTarget, Refusal::NoIndices, Refusal::ZeroStride, Refusal::Missing,
        Refusal::Shader, Refusal::FloatsUncovered, Refusal::IntsUncovered, Refusal::BoolsUncovered,
        Refusal::SamplersUncovered, Refusal::Device, Refusal::Primitive, Refusal::Query,
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
    /// the descriptor's low 12 bits (the context cache slot)
    index: u32,
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
    let (samplers, uploads) = descriptors(g, metadata)?;
    Ok(MetaPart {
        handles, vs, ps, declaration, floats: [vs_floats, ps_floats], reads: shader_reads, samplers, uploads,
        checked: frame.into(),
    })
}

/// A shader metadata object's sampler and constant descriptors, in
/// 0x00DAA8B0's order (mtstate::shader).
fn descriptors(g: &G, metadata: u32) -> Result<([Vec<SamplerSlot>; 2], [Vec<Upload>; 2]), Refusal> {
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
            samplers[stage].push(SamplerSlot { index: d & 4095, stage: (d & 4095) + if pixel { 0 } else { 257 }, sampler: d >> 12 & 1023, texture: d >> 22 });
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
    Ok((samplers, uploads))
}

/// A shader binding of this frame: its metadata, the alpha test and the
/// constant banks (with the registers they write).
pub struct BindingPart {
    meta: Arc<MetaPart>,
    /// (ALPHAFUNC, ALPHAREF) when the binding enables the alpha test
    alpha: Option<(u32, u32)>,
    constants: [Arc<Constants>; 2],
    /// per stage: float, int and bool registers written
    written: [([u64; 4], u16, u16); 2],
}

/// A shader binding's state as the full apply sets it (0x00DAA8B0 with
/// nothing applied: every sampler, texture and upload).
fn binding_part(g: &G, c: &mut Caches, binding: u32, previous: Option<&Arc<BindingPart>>) -> Result<Arc<BindingPart>, Refusal> {
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
    // the sampler and texture objects must exist (0x00DAA8B0 reads them)
    for slot in meta.samplers.iter().flatten() {
        if value(slot.sampler) == 0 || value(slot.texture) == 0 {
            return Err(Refusal::Missing);
        }
    }
    // constants: the binding's registers over zeros (in reused banks)
    let mut banks = std::mem::take(&mut c.banks);
    for (stage, bank) in banks.iter_mut().enumerate() {
        bank.f.clear();
        bank.f.resize(meta.floats[stage].min(FLOATS[stage]), [0; 4]);
        bank.i = [[0; 4]; 16];
        bank.b = [0; 16];
    }
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
        // every register the shaders read comes from here
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
    }
    // the same as the binding's part of an earlier frame: that part
    let part = match previous {
        Some(p) if Arc::ptr_eq(&p.meta, &meta) && p.alpha == alpha && p.written == written
            && *p.constants[0] == banks[0] && *p.constants[1] == banks[1] => p.clone(),
        _ => Arc::new(BindingPart { meta, alpha, constants: [Arc::new(banks[0].clone()), Arc::new(banks[1].clone())], written }),
    };
    c.banks = banks;
    Ok(part)
}

fn set_float(bank: &mut Constants, written: &mut ([u64; 4], u16, u16), n: u32, r: [u32; 4]) {
    if n < 256 {
        written.0[n as usize / 64] |= 1 << (n % 64);
    }
    if let Some(x) = bank.f.get_mut(n as usize) {
        *x = r;
    }
}

/// What the sampler section of 0x00DAA8B0 does for a draw: context words
/// written, SetSamplerState and SetTexture calls (made on the device for a
/// taken draw; for `verify` only predicted).
#[derive(Default)]
pub struct Shadow {
    ctx: Vec<(u32, u32)>,
    sampler_states: Vec<(u32, u32, u32)>,
    textures: Vec<(u32, u32)>,
}

impl Shadow {
    fn read(&self, g: &G, a: u32) -> u32 {
        self.ctx.iter().rev().find(|w| w.0 == a).map_or_else(|| g.u32(a), |w| w.1)
    }
    fn write(&mut self, a: u32, v: u32) {
        self.ctx.push((a, v));
    }
    fn is_empty(&self) -> bool {
        self.sampler_states.is_empty() && self.textures.is_empty()
    }
    fn clear(&mut self) {
        self.ctx.clear();
        self.sampler_states.clear();
        self.textures.clear();
    }
    /// Drop what changes nothing: calls setting what the device (`mirror`)
    /// already holds, context words already holding the value written
    /// last. What stays has the same effect.
    fn prune(&mut self, g: &G, mirror: &Mirror) {
        let mut rows = mirror.rows;
        let mut textures = mirror.textures;
        self.sampler_states.retain(|&(t, ty, v)| match (slot_of(t), (ty as usize) < 14) {
            (Some(i), true) => rows[i][ty as usize] != v && {
                rows[i][ty as usize] = v;
                true
            },
            _ => true,
        });
        self.textures.retain(|&(t, h)| match slot_of(t) {
            Some(i) => textures[i] != h && {
                textures[i] = h;
                true
            },
            None => true,
        });
        // the last write of each word, where it changes the word
        let mut last: Vec<(u32, u32)> = Vec::new();
        for &(a, v) in &self.ctx {
            match last.iter_mut().find(|w| w.0 == a) {
                Some(w) => w.1 = v,
                None => last.push((a, v)),
            }
        }
        last.retain(|&(a, v)| g.u32(a) != v);
        self.ctx.clear();
        self.ctx.extend(last);
    }
}

/// The device's sampler states and textures, stages 0..15 and 256..260,
/// as of `epoch` (crate::d3d9state::sampler_epoch: every change bumps it).
#[derive(Clone)]
struct Mirror {
    epoch: u64,
    rows: [[u32; 14]; 21],
    textures: [u32; 21],
}

fn slot_of(stage: u32) -> Option<usize> {
    match stage {
        0..=15 => Some(stage as usize),
        256..=260 => Some(stage as usize - 240),
        _ => None,
    }
}

fn stage_of(slot: usize) -> u32 {
    if slot < 16 { slot as u32 } else { slot as u32 + 240 }
}

impl Mirror {
    fn load(device: u32) -> Option<Mirror> {
        let epoch = crate::d3d9state::sampler_epoch();
        objects(|o| match o.live.get(&device).map(|x| &x.kind) {
            Some(Kind::Device(d)) => {
                let s = &d.state;
                let mut textures = [0; 21];
                for (&t, &h) in &s.textures {
                    if let Some(i) = slot_of(t) {
                        textures[i] = h;
                    }
                }
                Some(Mirror { epoch, rows: std::array::from_fn(|i| s.sampler.row(stage_of(i))), textures })
            }
            _ => None,
        })
    }

    fn apply(&mut self, sh: &Shadow) {
        for &(t, ty, v) in &sh.sampler_states {
            if let (Some(i), true) = (slot_of(t), (ty as usize) < 14) {
                self.rows[i][ty as usize] = v;
            }
        }
        for &(t, h) in &sh.textures {
            if let Some(i) = slot_of(t) {
                self.textures[i] = h;
            }
        }
    }

    /// The bound textures with their sampler states after `sh`, as the
    /// D3D9 path's snapshot builds them: by stage, objects that are not
    /// live textures left out.
    fn textures(&self, sh: &Shadow, c: &mut Caches) -> Arc<[TextureBinding]> {
        let mut m = self.clone();
        m.apply(sh);
        let mut out = Vec::with_capacity(8);
        for i in 0..21 {
            let h = m.textures[i];
            if h != 0 {
                if let Some(info) = c.texture(h) {
                    out.push(TextureBinding { sampler: stage_of(i), resource: h, states: m.rows[i], ..info });
                }
            }
        }
        out.into()
    }
}

// Translated from 0x00DAA8B0 (DDDA.exe 2017-12-13 Steam): its sampler and
// texture section, as mtstate::shader decodes it (verified against the
// original), on the executor context `ctx`.
fn shadow_samplers(g: &G, ctx: u32, binding: u32, meta: &MetaPart, info: &FrameInfo, sh: &mut Shadow) {
    let value = |n: u32| g.u32(binding.wrapping_add(12 + 4 * n));
    for stage in 0..2 {
        let pixel = stage == 1;
        let mut used = [false; 4];
        for slot in &meta.samplers[stage] {
            let (sampler, texture) = (value(slot.sampler), value(slot.texture));
            let cache = ctx.wrapping_add(if pixel { 0xD8 } else { 0x98 } + slot.index * 4);
            let old = sh.read(g, cache);
            if sampler != old {
                if old == 0 || g.u32(old.wrapping_add(12)) != g.u32(sampler.wrapping_add(12)) {
                    let words = |o: u32| -> Vec<u32> { (0..3).map(|k| g.u32(o.wrapping_add(0x30 + 4 * k))).collect() };
                    let new = words(sampler);
                    let old_words = (old != 0).then(|| words(old));
                    for c in mtstate::packed(mtstate::SAMPLER, &new, old_words.as_deref(), Some(slot.stage), info.filtering) {
                        sh.sampler_states.push((c.args[0], c.args[1], c.args[2]));
                    }
                }
                sh.write(cache, sampler);
            }
            if sh.read(g, cache.wrapping_add(0x80)) != texture {
                sh.write(cache.wrapping_add(0x80), texture);
                sh.textures.push((slot.stage, g.u32(texture.wrapping_add(0x34))));
            }
            if pixel && info.srgb {
                let v = (g.u32(texture.wrapping_add(0x20)) & 0x4000_0000 != 0 && g.u32(texture.wrapping_add(0x2C)) & 0x8_0000 == 0) as u32;
                sh.sampler_states.push((slot.stage, 11, v));
                let mask = 1u32 << (slot.index & 31);
                let bits = sh.read(g, ctx.wrapping_add(0x198));
                sh.write(ctx.wrapping_add(0x198), if v != 0 { bits | mask } else { bits & !mask });
            }
            if !pixel && slot.index < 4 {
                used[slot.index as usize] = sh.read(g, cache.wrapping_add(0x80)) != 0;
            }
        }
        if !pixel {
            // vertex stages this binding does not use, with a sampler cached
            for index in 0..4u32 {
                if !used[index as usize] && sh.read(g, ctx.wrapping_add(0x98 + index * 4)) != 0 {
                    sh.write(ctx.wrapping_add(0x118 + index * 4), 0);
                    sh.textures.push((index + 257, 0));
                }
            }
        }
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
    states: FxMap<StatesKey, RenderStateList>,
    /// native fixed state by identity and render-state list (its address:
    /// the list is held in `states`)
    native_fixed: FxMap<(NativeStateIdentity, u64), Arc<NativeFixedState>>,
    /// native register arrays by constant bank (held)
    native_constants: FxMap<usize, (Arc<Constants>, NativeConstants)>,
    // this frame's
    /// shader bindings by address, with the frame they were last built
    /// or checked in (kept across frames: an unchanged binding keeps its
    /// part, no allocation)
    bindings: FxMap<u32, (u64, Result<Arc<BindingPart>, Refusal>)>,
    /// the banks a binding's constants are read into
    banks: [Constants; 2],
    streams: FxMap<u32, Result<Arc<StreamPart>, Refusal>>,
    target_bindings: FxMap<u32, Result<Arc<TargetPart>, Refusal>>,
    targets: FxMap<u32, Option<Target>>,
    textures: FxMap<u32, Option<TextureBinding>>,
    index_formats: FxMap<u32, Option<u32>>,
    /// the device's sampler states and textures this frame, and its
    /// texture list at an epoch (crate::d3d9state::sampler_epoch)
    mirror: Option<Mirror>,
    texture_list: Option<(u64, Arc<[TextureBinding]>)>,
    /// the sampler section's buffers, reused
    scratch: Shadow,
}

impl Caches {
    /// A new frame: what the device holds that the applies do not set
    /// (with the D3D lock held).
    pub fn begin_frame(&mut self, info: &FrameInfo) {
        self.frame += 1;
        if self.bindings.len() > 16384 {
            self.bindings.clear();
        }
        self.streams.clear();
        self.target_bindings.clear();
        self.targets.clear();
        self.textures.clear();
        self.index_formats.clear();
        self.mirror = None;
        self.texture_list = None;
        let device = objects(|o| match o.live.get(&info.device).map(|x| &x.kind) {
            Some(Kind::Device(d)) => {
                let s = &d.state;
                let states: Vec<(u32, u32)> =
                    DEVICE_STATES.iter().chain(&[24, 25]).filter_map(|&k| s.render.get(&k).map(|&v| (k, v))).collect();
                // a taken draw carries streams 0..3 only
                let usable = s.in_scene && s.streams[4..].iter().all(|x| x.0 == 0);
                Some(DeviceFrame { usable, states })
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

    fn target(&mut self, surface: u32) -> Option<Target> {
        if surface == 0 {
            return None;
        }
        self.targets.entry(surface).or_insert_with(|| crate::d3d9draw::target(surface)).clone()
    }

    fn texture(&mut self, handle: u32) -> Option<TextureBinding> {
        self.textures.entry(handle).or_insert_with(|| texture_info(handle)).clone()
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
/// 1) with the executor context `context`, with the D3D lock held and
/// `caches.begin_frame` called for this frame; and the sampler section's
/// effects, to `commit` if the draw is taken. The resources it reads are
/// `reads_of` it.
pub fn draw_call(m: &dyn Mem, info: &FrameInfo, c: &mut Caches, context: u32, record: u32, kind: u32) -> Result<Decoded, Refusal> {
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
    let b = match c.bindings.get(&shader) {
        Some((frame, r)) if *frame == c.frame => r.clone()?,
        other => {
            let previous = match other {
                Some((_, Ok(p))) => Some(p.clone()),
                _ => None,
            };
            let r = binding_part(&g, c, shader, previous.as_ref());
            c.bindings.insert(shader, (c.frame, r.clone()));
            r?
        }
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
    // samplers and textures: what the apply would do on this context
    let mut sh = std::mem::take(&mut c.scratch);
    sh.clear();
    shadow_samplers(&g, context, shader, &b.meta, info, &mut sh);
    // the device as it is (anything else that set a sampler state or a
    // texture since: the default path's applies, moves the epoch)
    if c.mirror.as_ref().is_none_or(|m| m.epoch != crate::d3d9state::sampler_epoch()) {
        c.mirror = Some(Mirror::load(info.device).ok_or(Refusal::Device)?);
    }
    let mirror = c.mirror.take().unwrap();
    sh.prune(&g, &mirror);
    let textures = match &c.texture_list {
        Some((epoch, list)) if *epoch == mirror.epoch && sh.is_empty() => list.clone(),
        _ => {
            let list = mirror.textures(&sh, c);
            if sh.is_empty() {
                c.texture_list = Some((mirror.epoch, list.clone()));
            }
            list
        }
    };
    c.mirror = Some(mirror);
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
    let call = DrawCall {
        primitive, primitives, start, base_vertex, min_index: 0, vertex_count,
        vertices: Vertices::Streams(s.streams.clone()),
        indices,
        vertex_shader: Some(b.meta.vs.clone()), pixel_shader: Some(b.meta.ps.clone()),
        declaration: b.meta.declaration.clone(),
        // the runtime's FVF stays 0 once declarations are used
        fvf: 0,
        vs_constants: b.constants[0].clone(), ps_constants: b.constants[1].clone(),
        textures,
        render_states,
        viewport: t.viewport,
        scissor: t.scissor,
        targets: t.targets.clone(),
        depth: t.depth.clone(),
    };
    Ok(Decoded { call, shadow: sh, binding: b })
}

/// A decoded draw: its DrawCall, the sampler section's effects and the
/// shader binding (its constant uploads).
pub struct Decoded {
    pub call: DrawCall,
    pub shadow: Shadow,
    pub binding: Arc<BindingPart>,
}

/// Make a taken draw's sampler section effects: the context words and the
/// device calls.
pub fn commit(m: &mut dyn Mem, info: &FrameInfo, c: &mut Caches, mut sh: Shadow, textures: &Arc<[TextureBinding]>) {
    for &(a, v) in &sh.ctx {
        m.set_u32(a, v);
    }
    if !sh.is_empty() {
        for &(t, ty, v) in &sh.sampler_states {
            crate::d3d9state::device_set_sampler_state(info.device, t, ty, v);
        }
        for &(t, h) in &sh.textures {
            crate::d3d9state::device_set_texture(info.device, t, h);
        }
        // the device is what the mirror and the draw's list say now
        if let Some(mirror) = c.mirror.as_mut() {
            mirror.apply(&sh);
            mirror.epoch = crate::d3d9state::sampler_epoch();
            c.texture_list = Some((mirror.epoch, textures.clone()));
        }
    }
    sh.clear();
    c.scratch = sh;
}

/// The shader bindings taken draws used since the default path last ran,
/// in order (a binding repeated by the next draw kept once): their
/// constant uploads, the last value of each register, reach the device
/// when the default path takes over. Per taken draw this costs a pointer
/// compare (a register-by-register record cost 2.3 us for a binding of
/// 128 + 64 registers, release, `bench_uploads_per_taken_draw`).
#[derive(Default)]
pub struct Uploads {
    bindings: Vec<Arc<BindingPart>>,
}

impl Uploads {
    fn add(&mut self, b: &Arc<BindingPart>) {
        if !self.bindings.last().is_some_and(|l| Arc::ptr_eq(l, b)) {
            self.bindings.push(b.clone());
        }
    }
    fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
    /// Each register's last value, newest binding first: `f(stage, kind
    /// (0 float, 1 int, 2 bool), register, the binding's bank)`.
    fn each_last(&self, mut f: impl FnMut(usize, u8, usize, &Constants)) {
        let mut done = [([0u64; 4], 0u16, 0u16); 2];
        for b in self.bindings.iter().rev() {
            for stage in 0..2 {
                let (w, i, bo) = b.written[stage];
                let d = &mut done[stage];
                let bank = &b.constants[stage];
                for word in 0..4 {
                    let mut new = w[word] & !d.0[word];
                    d.0[word] |= new;
                    while new != 0 {
                        let n = word * 64 + new.trailing_zeros() as usize;
                        new &= new - 1;
                        f(stage, 0, n, bank);
                    }
                }
                let mut new = i & !d.1;
                d.1 |= new;
                while new != 0 {
                    f(stage, 1, new.trailing_zeros() as usize, bank);
                    new &= new - 1;
                }
                let mut new = bo & !d.2;
                d.2 |= new;
                while new != 0 {
                    f(stage, 2, new.trailing_zeros() as usize, bank);
                    new &= new - 1;
                }
            }
        }
    }
    /// The float registers' last values, vertex then pixel, by register.
    #[cfg(test)]
    fn registers(&self) -> (Vec<(u32, [u32; 4])>, Vec<(u32, [u32; 4])>) {
        let mut out = (Vec::new(), Vec::new());
        self.each_last(|stage, kind, n, bank| {
            if let (0, Some(&r)) = (kind, bank.f.get(n)) {
                if stage == 0 { &mut out.0 } else { &mut out.1 }.push((n as u32, r));
            }
        });
        out.0.sort_unstable_by_key(|e| e.0);
        out.1.sort_unstable_by_key(|e| e.0);
        out
    }
}

/// The default path takes over after taken draws: the device gets the
/// registers they uploaded, and the context forgets its last fixed-state
/// binding (+0xC) and objects (+0x18, +0x1C, +0x20), its last shader
/// binding (+0x10) and vertex shader (+0x24), so the next applies run in
/// full (docs/NDRAW.md section 3, the context layout).
pub fn invalidate(m: &mut dyn Mem, device: u32, context: u32, uploads: &mut Uploads) {
    for off in [0xC, 0x10, 0x18, 0x1C, 0x20, 0x24] {
        m.set_u32(context.wrapping_add(off), 0);
    }
    flush_uploads(device, uploads);
}

/// Write the registers taken draws uploaded to the device.
pub fn flush_uploads(device: u32, uploads: &mut Uploads) {
    if uploads.is_empty() {
        return;
    }
    let u = std::mem::take(uploads);
    objects(|o| {
        if let Some(Kind::Device(d)) = o.live.get_mut(&device).map(|x| &mut x.kind) {
            let s = &mut d.state;
            let (vs, ps) = (Arc::make_mut(&mut s.vs), Arc::make_mut(&mut s.ps));
            u.each_last(|stage, kind, n, bank| {
                let c = if stage == 0 { &mut *vs } else { &mut *ps };
                match kind {
                    0 => {
                        if let (Some(r), Some(&v)) = (c.f.get_mut(n), bank.f.get(n)) {
                            *r = v;
                        }
                    }
                    1 => c.i[n] = bank.i[n],
                    _ => c.b[n] = bank.b[n],
                }
            });
            s.constant_snapshots[0].dirty = true;
            s.constant_snapshots[1].dirty = true;
        }
    });
}

/// Flush what `d` reads (the D3D9 path's flush_reads; the benchmark).
pub fn flush_draw(m: &dyn Mem, d: &DrawCall) {
    crate::d3d9draw::flush_reads(m, reads_of(d));
}

/// Send `d` to the backend alone (the benchmark).
pub fn send_draw(d: &DrawCall) {
    crate::d3d9draw::backend_unlocked(|b| b.draw(d));
}

/// Send a run of taken draws as one command (the benchmark).
pub fn send_draws(draws: Vec<DrawCall>) {
    let mut batch = draws;
    send_batch(&mut batch);
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
    // one set of register arrays per constant bank (banks are shared by the
    // draws of a binding, across frames while unchanged), so the worker can
    // tell an unchanged bank by its pointer
    let mut constants = |c: &Arc<Constants>| {
        let key = Arc::as_ptr(c) as usize;
        if let Some((_, n)) = caches.native_constants.get(&key) {
            return n.clone();
        }
        if caches.native_constants.len() > 8192 {
            caches.native_constants.clear();
        }
        let n = NativeConstants {
            floats: vec![NativeFloatWrite { first: 0, registers: c.f.as_slice().into() }],
            ints: vec![NativeIntWrite { first: 0, registers: c.i.as_slice().into() }],
            bools: vec![NativeBoolWrite { first: 0, registers: c.b.as_slice().into() }],
        };
        // the bank is held, so its address is not reused while cached
        caches.native_constants.insert(key, (c.clone(), n.clone()));
        n
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

    /// Send the job, with its draws' DrawCalls: the backend makes those
    /// instead, once, in the same place in the FIFO, if it rejects the job
    /// (d3d9cmd native_or_draw, L133). The draws' count.
    pub fn send(&mut self) -> Option<u64> {
        let (job, calls) = self.take()?;
        let n = calls.len() as u64;
        crate::d3d9draw::count_draws(n);
        crate::d3d9draw::backend_unlocked(|b| b.native_or_draw(job, calls));
        Some(n)
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
    /// native jobs sent, and their draws
    jobs: u64,
    native: u64,
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
    /// draws taken since the default path last ran, and their uploads
    handled: bool,
    uploads: Uploads,
    /// taken draws not sent yet (`drawcall`): one backend command a run
    batch: Vec<DrawCall>,
    comparison: comparison::Comparison,
    /// the objects of type-7 spans begun and not ended this frame
    spans: FxSet<u32>,
    /// the backend takes native jobs (`1`; otherwise those draws go as
    /// DrawCalls, as `drawcall`)
    native: bool,
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
                info: FrameInfo::default(), scene: (0, 0), caches: Caches::default(), pending: Pending::default(), native: false,
                handled: false, uploads: Uploads::default(), batch: Vec::new(),
                comparison: comparison::Comparison::default(),
                spans: FxSet::default(),
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
        g.handled = false;
        g.spans.clear();
        if self.mode != Mode::Off {
            let _lock = crate::d3d9dev::device_lock();
            g.caches.begin_frame(&g.info);
        }
        if matches!(self.mode, Mode::Native | Mode::Compare) {
            // stable for the backend's lifetime, but the backend can be replaced
            g.native = crate::d3d9draw::backend(|b| b.native_supported());
        }
    }

    fn command(&self, m: &mut dyn Mem, context: u32, record: u32, kind: u32) -> Taken {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let g = &mut *g;
        let taken = self.decide(g, m, context, record, kind);
        if taken == Taken::Default {
            // what the native path took goes before this command
            g.send();
            if g.handled {
                let _lock = crate::d3d9dev::device_lock();
                invalidate(m, g.info.device, context, &mut g.uploads);
                g.handled = false;
            }
        }
        taken
    }

    fn frame_end(&self, _m: &mut dyn Mem, _render: u32, _context: u32) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let g = &mut *g;
        g.send();
        if g.handled {
            // the next frame's default path sees the registers too
            let _lock = crate::d3d9dev::device_lock();
            flush_uploads(g.info.device, &mut g.uploads);
            g.handled = false;
        }
        g.end_frame(self.mode, self.every);
    }
}

impl Inner {
    fn send(&mut self) {
        send_batch(&mut self.batch);
        if self.comparison.is_active() {
            crate::d3d9draw::backend_unlocked(|b| self.comparison.finish(b));
        }
        if let Some(n) = self.pending.send() {
            self.totals.jobs += 1;
            self.totals.native += n;
        }
    }
}

/// Send the taken draws not sent yet, as one backend command.
fn send_batch(batch: &mut Vec<DrawCall>) {
    if !batch.is_empty() {
        let draws = std::mem::replace(batch, Vec::with_capacity(64));
        crate::d3d9draw::backend_unlocked(|b| b.draws(draws));
    }
}

impl Native {
    fn decide(&self, g: &mut Inner, m: &mut dyn Mem, context: u32, record: u32, kind: u32) -> Taken {
        let f = &mut g.frame;
        f.kinds[kind as usize & 15] += 1;
        if kind == 7 {
            // 0x00DAB480's type 7: the object at +8 (none: nothing), begun
            // (0x011481B0) when +4 is 0, else ended (0x011481F0)
            let object = m.u32(record.wrapping_add(8));
            if object != 0 {
                if m.u32(record.wrapping_add(4)) == 0 {
                    g.spans.insert(object);
                } else {
                    g.spans.remove(&object);
                }
            }
        }
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
        // native jobs do not count samples for queries (a DrawCall does)
        if matches!(self.mode, Mode::Native | Mode::Compare) && g.native && !g.spans.is_empty() {
            *t.refused.entry(Refusal::Query).or_default() += 1;
            return Taken::Default;
        }
        if kind >= 2 {
            *t.refused.entry(Refusal::Instanced).or_default() += 1;
            return Taken::Default;
        }
        let started = Instant::now();
        let lock = crate::d3d9dev::device_lock();
        let decoded = draw_call(m, &g.info, &mut g.caches, context, record, kind);
        let taken = match decoded {
            Err(r) => {
                *t.refused.entry(r).or_default() += 1;
                Taken::Default
            }
            Ok(Decoded { call: d, .. }) if self.mode == Mode::Verify => {
                let reads = [&d.vertex_shader, &d.pixel_shader].map(|t| {
                    let key = t.as_ref().map_or(0, |t| Arc::as_ptr(t) as *const u32 as usize);
                    g.caches.reads.get(&key).map(|r| r.1).unwrap_or_default()
                });
                expect(Expected { call: d, reads });
                t.taken += 1;
                Taken::Default
            }
            Ok(_) if self.mode == Mode::Census => {
                t.taken += 1;
                Taken::Default
            }
            Ok(Decoded { call: d, shadow, binding }) if self.mode == Mode::DrawCall || !g.native => {
                commit(m, &g.info, &mut g.caches, shadow, &d.textures);
                g.uploads.add(&binding);
                g.handled = true;
                // an upload goes after the draws taken before it
                if objects(|o| !o.dirty.is_empty() && reads_of(&d).any(|r| o.dirty.contains(&r))) {
                    send_batch(&mut g.batch);
                }
                crate::d3d9draw::flush_reads(m, reads_of(&d));
                g.batch.push(d);
                crate::d3d9draw::count_draws(1);
                t.taken += 1;
                Taken::Handled
            }
            Ok(Decoded { call: d, shadow, binding }) => match native_draw(&d, identity(m, record), &mut g.caches) {
                Err(r) => {
                    *t.refused.entry(r).or_default() += 1;
                    Taken::Default
                }
                Ok(native) => {
                    commit(m, &g.info, &mut g.caches, shadow, &d.textures);
                    g.uploads.add(&binding);
                    g.handled = true;
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
        out += &format!("  native jobs {:.1}, draws in them {:.1} (a job the backend rejects goes as its DrawCalls)\n",
                        per(t.jobs), per(t.native));
    }
    if mode == Mode::Verify {
        out += &verify_report();
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

    /// Native jobs leave draws made inside a type-7 span (a query begun,
    /// not ended) to the D3D9 path, which counts their samples; drawcall
    /// takes them (a DrawCall is counted by the backend).
    #[test]
    fn draws_inside_a_query_span_stay_on_the_d3d9_path_for_native_jobs() {
        let mut m = TestMem::default();
        let scene = record(&mut m, 0x1000, (1920, 1080), 0x50);
        let (begin, end) = (0x5000, 0x5100);
        m.set_u32(begin + 8, 0x7777);
        m.set_u32(end + 4, 1);
        m.set_u32(end + 8, 0x7777);
        for (mode, inside) in [(Mode::Native, Some(Refusal::Query)), (Mode::DrawCall, None)] {
            let sink = Native::new(mode, Duration::from_secs(3600));
            {
                let mut g = sink.inner.lock().unwrap();
                g.scene = (1920, 1080);
                g.native = true;
            }
            let refused = |sink: &Native, r: Refusal| sink.inner.lock().unwrap().totals.refused.get(&r).copied().unwrap_or(0);
            assert_eq!(sink.decide(&mut sink.inner.lock().unwrap(), &mut m, 0, begin, 7), Taken::Default);
            assert_eq!(sink.decide(&mut sink.inner.lock().unwrap(), &mut m, 0, scene, 1), Taken::Default);
            // native: refused for the span; drawcall: decided (no device here)
            match inside {
                Some(r) => assert_eq!((refused(&sink, r), refused(&sink, Refusal::Device)), (1, 0)),
                None => assert_eq!((refused(&sink, Refusal::Query), refused(&sink, Refusal::Device)), (0, 1)),
            }
            assert_eq!(sink.decide(&mut sink.inner.lock().unwrap(), &mut m, 0, end, 7), Taken::Default);
            assert_eq!(sink.decide(&mut sink.inner.lock().unwrap(), &mut m, 0, scene, 1), Taken::Default);
            assert_eq!(refused(&sink, Refusal::Query), inside.is_some() as u64);
        }
    }

    #[test]
    fn drawcall_is_the_default_and_0_or_off_turns_it_off() {
        assert_eq!(mode_of(None), Mode::DrawCall);
        assert_eq!(mode_of(Some("0")), Mode::Off);
        assert_eq!(mode_of(Some("off")), Mode::Off);
        assert_eq!(mode_of(Some("drawcall")), Mode::DrawCall);
        assert_eq!(mode_of(Some("verify")), Mode::Verify);
        assert_eq!(mode_of(Some("compare")), Mode::Compare);
        assert_eq!(mode_of(Some("census")), Mode::Census);
        assert_eq!(mode_of(Some("1")), Mode::Native);
        assert_eq!(mode_of(Some("native")), Mode::Native);
        assert_eq!(mode_of(Some("bogus")), Mode::DrawCall);
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

    /// mtstate's view of the test memory.
    struct Words<'a>(&'a mut TestMem);
    impl mtstate::Memory for Words<'_> {
        fn read32(&self, a: u32) -> u32 {
            self.0.u32(a)
        }
        fn write32(&mut self, a: u32, v: u32) {
            self.0.set_u32(a, v)
        }
    }

    /// The sampler section of a taken draw against mtstate::shader (the
    /// apply verified against the original): the same SetSamplerState and
    /// SetTexture calls in order, the same context words, on random
    /// contexts, bindings, objects, sRGB and filtering options.
    #[test]
    fn the_sampler_section_does_what_the_apply_does() {
        let mut seed = 0x1234_5678u32;
        let mut rnd = move |n: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) % n.max(1)
        };
        let (ctx, binding, meta, descs) = (0x5_0000u32, 0x6_0000u32, 0x6_1000u32, 0x6_2000u32);
        let samplers: Vec<u32> = (0..4).map(|k| 0x7_0000 + 0x100 * k).collect();
        let textures: Vec<u32> = (0..4).map(|k| 0x8_0000 + 0x100 * k).collect();
        for case in 0..3000 {
            let mut m = TestMem::default();
            // objects: sampler payloads (address, filters, anisotropy, mip
            // level) and hashes, texture flags and handles
            for &o in &samplers {
                let w0 = rnd(512) | (1 + rnd(3)) << 9 | (1 + rnd(3)) << 13 | rnd(3) << 17 | rnd(32) << 21 | rnd(16) << 26;
                m.set_u32(o + 12, rnd(3));
                m.set_u32(o + 0x30, w0);
                m.set_u32(o + 0x34, rnd(2) * 0xFF00_0000);
                m.set_u32(o + 0x38, rnd(4) * 0x3F00_0000);
            }
            for &o in &textures {
                m.set_u32(o + 0x20, rnd(2) << 30);
                m.set_u32(o + 0x2C, rnd(2) << 19);
                m.set_u32(o + 0x34, 0xD000 + rnd(3));
            }
            // a binding: up to 3 vertex and 4 pixel samplers on random stages
            let (nv, np) = (rnd(4), rnd(5));
            m.set_u32(binding, 0);
            m.set_u32(binding + 8, meta);
            for k in 0..8 {
                m.set_u32(binding + 12 + 4 * k, if k < 4 { samplers[rnd(4) as usize] } else { textures[rnd(4) as usize] });
            }
            m.set_u32(meta + 4, descs);
            m.set_u32(meta + 8, nv | nv << 8);
            m.set_u32(meta + 12, np | np << 8);
            m.set_u32(meta + 0x10, 0xC000);
            m.set_u32(meta + 0x14, 0xC100);
            for k in 0..nv + np {
                let index = if k < nv { rnd(4) } else { rnd(16) };
                m.set_u32(descs + 4 * k, index | rnd(4) << 12 | (4 + rnd(4)) << 22);
            }
            // a context left by earlier applies
            for k in 0..4 {
                m.set_u32(ctx + 0x98 + 4 * k, [0, samplers[rnd(4) as usize]][rnd(2) as usize]);
                m.set_u32(ctx + 0x118 + 4 * k, [0, textures[rnd(4) as usize]][rnd(2) as usize]);
            }
            for k in 0..16 {
                m.set_u32(ctx + 0xD8 + 4 * k, [0, samplers[rnd(4) as usize]][rnd(2) as usize]);
                m.set_u32(ctx + 0x158 + 4 * k, [0, textures[rnd(4) as usize]][rnd(2) as usize]);
            }
            m.set_u32(ctx + 0x198, rnd(0x1_0000));
            let info = FrameInfo { device: 0, srgb: rnd(2) == 1, filtering: rnd(7), invalid_viewport: [0; 4] };
            let region = |m: &TestMem| (0x98..0x19C).step_by(4).map(|o| m.u32(ctx + o)).collect::<Vec<_>>();
            // the apply
            let mut a = TestMem(m.0.clone());
            let calls = mtstate::shader(&mut Words(&mut a), ctx, binding, info.srgb, info.filtering).expect("decodes");
            let states: Vec<(u32, u32, u32)> = calls.iter().filter(|c| c.slot == 69).map(|c| (c.args[0], c.args[1], c.args[2])).collect();
            let set: Vec<(u32, u32)> = calls.iter().filter(|c| c.slot == 65).map(|c| (c.args[0], c.args[1])).collect();
            // the section
            let g = G::new(&m);
            let (slots, uploads) = descriptors(&g, meta).unwrap();
            let part = MetaPart {
                handles: [0; 3], vs: Arc::from(vec![]), ps: Arc::from(vec![]), declaration: Arc::from(vec![]),
                floats: [0; 2], reads: Default::default(), samplers: slots, uploads, checked: 0.into(),
            };
            let mut sh = Shadow::default();
            shadow_samplers(&g, ctx, binding, &part, &info, &mut sh);
            assert_eq!(sh.sampler_states, states, "case {case}: sampler states");
            assert_eq!(sh.textures, set, "case {case}: textures");
            let mut b = TestMem(m.0.clone());
            for (at, v) in sh.ctx {
                b.set_u32(at, v);
            }
            assert_eq!(region(&b), region(&a), "case {case}: context");
        }
    }

    /// Pruning keeps exactly the calls and words that change something:
    /// applying what is left to the device gives what applying everything
    /// gives.
    #[test]
    fn pruning_drops_only_what_changes_nothing() {
        let mut m = TestMem::default();
        m.set_u32(0x100, 7);
        m.set_u32(0x104, 9);
        let mut rows = [[0u32; 14]; 21];
        rows[0][10] = 4;
        rows[17][5] = 2;
        let mut textures = [0u32; 21];
        textures[0] = 0xD000;
        let mirror = Mirror { epoch: 1, rows, textures };
        let mut sh = Shadow::default();
        // the same value; a change; a change back (stays: it is the last
        // word for the field); a vertex stage; textures likewise
        sh.sampler_states.extend([(0, 10, 4), (0, 10, 16), (0, 11, 1), (257, 5, 2), (257, 6, 3)]);
        sh.textures.extend([(0, 0xD000), (1, 0xD100), (257, 0)]);
        sh.ctx.extend([(0x100, 7), (0x104, 1), (0x104, 9), (0x108, 3)]);
        let mut all = mirror.clone();
        all.apply(&sh);
        let g = G::new(&m);
        sh.prune(&g, &mirror);
        assert_eq!(sh.sampler_states, vec![(0, 10, 16), (0, 11, 1), (257, 6, 3)]);
        assert_eq!(sh.textures, vec![(1, 0xD100)]);
        assert_eq!(sh.ctx, vec![(0x108, 3)]);
        let mut pruned = mirror.clone();
        pruned.apply(&sh);
        assert_eq!((pruned.rows, pruned.textures), (all.rows, all.textures));
        // nothing left when nothing changes
        let mut again = Shadow::default();
        again.sampler_states.extend([(0, 10, 16)]);
        again.ctx.push((0x100, 7));
        again.prune(&g, &pruned);
        assert!(again.is_empty() && again.ctx.is_empty());
    }

    /// A shader binding writing `vs` vertex and `ps` pixel float
    /// registers.
    fn binding_writing(vs: usize, ps: usize, seed: u32) -> Arc<BindingPart> {
        let bank = |n: usize, floats: usize| {
            let mut c = Constants { f: vec![[0; 4]; floats], ..Default::default() };
            for (k, r) in c.f.iter_mut().take(n).enumerate() {
                *r = [seed, k as u32, 0, 0];
            }
            c
        };
        let mask = |n: usize| -> [u64; 4] { std::array::from_fn(|w| if n >= 64 * (w + 1) { !0 } else if n > 64 * w { (1u64 << (n - 64 * w)) - 1 } else { 0 }) };
        Arc::new(BindingPart {
            meta: Arc::new(MetaPart {
                handles: [0; 3], vs: Arc::from(vec![]), ps: Arc::from(vec![]), declaration: Arc::from(vec![]),
                floats: [256, 224], reads: Default::default(), samplers: Default::default(), uploads: Default::default(),
                checked: 0.into(),
            }),
            alpha: None,
            constants: [Arc::new(bank(vs, 256)), Arc::new(bank(ps, 224))],
            written: [(mask(vs), 0, 0), (mask(ps), 0, 0)],
        })
    }

    /// The registers taken draws upload reach the device as the last value
    /// of each, whatever the bookkeeping.
    #[test]
    fn uploads_keep_the_last_value_of_each_register() {
        let (a, b) = (binding_writing(8, 4, 1), binding_writing(4, 8, 2));
        let mut u = Uploads::default();
        for x in [&a, &b, &b, &a, &b] {
            u.add(x);
        }
        let (vs, ps) = u.registers();
        // b wrote last: its 4 / 8 registers; a's beyond them
        assert_eq!(vs.len(), 8);
        assert_eq!(ps.len(), 8);
        assert!(vs.iter().all(|&(n, r)| r[0] == if n < 4 { 2 } else { 1 }));
        assert!(ps.iter().all(|&(_, r)| r[0] == 2));
    }

    /// Release: `cargo test --release -p ddruntime --no-default-features
    /// --lib bench_ -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_uploads_per_taken_draw() {
        let parts: Vec<_> = (0..8).map(|k| binding_writing(128, 64, k)).collect();
        let n = 200_000;
        let mut u = Uploads::default();
        let t = Instant::now();
        for k in 0..n {
            // runs of the same binding, as consecutive draws of a material
            u.add(&parts[(k / 4) % parts.len()]);
            if k % 1000 == 999 {
                std::hint::black_box(u.registers());
                u = Uploads::default();
            }
        }
        eprintln!("Uploads::add (128 VS + 64 PS registers): {:.3} us per draw", t.elapsed().as_secs_f64() * 1e6 / n as f64);
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

// --- verify --------------------------------------------------------------------

/// A decoded DrawCall waiting for the D3D9 path's, and what its shaders read.
struct Expected {
    call: DrawCall,
    reads: [Reads; 2],
}

thread_local! {
    static EXPECTED: std::cell::RefCell<Option<Expected>> = const { std::cell::RefCell::new(None) };
}
static VERIFYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
struct VerifyStats {
    compared: u64,
    equal: u64,
    /// the D3D9 path drew more primitives (a merged draw, sRender+0x30)
    merged: u64,
    /// expected draws the D3D9 path did not make
    missed: u64,
    fields: FxMap<&'static str, u64>,
    printed: FxMap<&'static str, u32>,
}
static VERIFY: Mutex<Option<VerifyStats>> = Mutex::new(None);

fn expect(e: Expected) {
    VERIFYING.store(true, std::sync::atomic::Ordering::Relaxed);
    EXPECTED.with(|x| {
        if x.borrow_mut().replace(e).is_some() {
            VERIFY.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).missed += 1;
        }
    });
}

/// The D3D9 draw services' DrawCall (d3d9draw): compared with the expected
/// one in `verify` mode.
pub(crate) fn observe(d: &DrawCall) {
    if !VERIFYING.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(e) = EXPECTED.with(|x| x.borrow_mut().take()) else { return };
    let mut wrong: Vec<(&'static str, String)> = Vec::new();
    compare(&e, d, &mut wrong);
    let mut g = VERIFY.lock().unwrap_or_else(|e| e.into_inner());
    let v = g.get_or_insert_with(Default::default);
    v.compared += 1;
    if d.primitives > e.call.primitives {
        v.merged += 1;
    }
    if wrong.is_empty() {
        v.equal += 1;
    }
    for (field, detail) in &wrong {
        *v.fields.entry(field).or_default() += 1;
        let printed = v.printed.entry(field).or_default();
        if *printed < 5 {
            *printed += 1;
            eprintln!("ndraw native verify: {field} differs: {detail}");
        }
    }
}

/// The fields the backend reads where `expected` (as `drawcall` would send
/// it, its shaders reading `reads`) and the D3D9 path's `actual` differ.
pub fn differences(expected: &DrawCall, reads: [Reads; 2], actual: &DrawCall) -> Vec<(&'static str, String)> {
    let mut wrong = Vec::new();
    compare(&Expected { call: expected.clone(), reads }, actual, &mut wrong);
    wrong
}

/// Every field the backend reads, as `drawcall` would send it vs the D3D9
/// path's.
fn compare(e: &Expected, d: &DrawCall, wrong: &mut Vec<(&'static str, String)>) {
    let x = &e.call;
    let mut check = |field: &'static str, ok: bool, detail: &dyn Fn() -> String| {
        if !ok {
            wrong.push((field, detail()));
        }
    };
    check("draw", x.primitive == d.primitive && x.start == d.start && x.base_vertex == d.base_vertex
        && x.min_index == d.min_index && x.vertex_count == d.vertex_count && d.primitives >= x.primitives,
        &|| format!("({}, {}, {}, {}, {}, {}) vs ({}, {}, {}, {}, {}, {})", x.primitive, x.primitives, x.start, x.base_vertex,
                    x.min_index, x.vertex_count, d.primitive, d.primitives, d.start, d.base_vertex, d.min_index, d.vertex_count));
    check("vertices", x.vertices == d.vertices, &|| format!("{:?} vs {:?}", x.vertices, d.vertices));
    check("indices", x.indices == d.indices, &|| format!("{:?} vs {:?}", x.indices, d.indices));
    check("shaders", x.vertex_shader == d.vertex_shader && x.pixel_shader == d.pixel_shader, &|| "bytecode".into());
    check("declaration", x.declaration == d.declaration, &|| format!("{:?} vs {:?}", x.declaration, d.declaration));
    let state = |s: u32| x.render_states.iter().find(|e| e.0 == s).map(|e| e.1);
    let alpha_on = state(15).is_some_and(|v| v != 0);
    let factor_used = state(27) == Some(1) && [19, 20, 207, 208].iter().any(|&s| matches!(state(s), Some(14 | 15)));
    for &(s, v) in x.render_states.iter() {
        // the alpha function and reference only count with the test on,
        // the blend factor only with blend-factor blending
        if ((s == 24 || s == 25) && !alpha_on) || (s == 193 && !factor_used) {
            continue;
        }
        let on_device = d.render_states.binary_search_by_key(&s, |e| e.0).ok().map(|i| d.render_states[i].1);
        check("render state", on_device == Some(v), &|| format!("{s}: {v:#x} vs {on_device:x?}"));
    }
    for stage in 0..2 {
        let (a, b) = if stage == 0 { (&x.vs_constants, &d.vs_constants) } else { (&x.ps_constants, &d.ps_constants) };
        let r = &e.reads[stage];
        for n in 0..256usize {
            if r.floats[n / 64] >> (n % 64) & 1 != 0 {
                check("float register", a.f.get(n) == b.f.get(n), &|| format!("{} c{n}: {:x?} vs {:x?}", ["vs", "ps"][stage], a.f.get(n), b.f.get(n)));
            }
        }
        for n in 0..16usize {
            if r.ints >> n & 1 != 0 {
                check("int register", a.i[n] == b.i[n], &|| format!("{} i{n}", ["vs", "ps"][stage]));
            }
            if r.bools >> n & 1 != 0 {
                check("bool register", a.b[n] == b.b[n], &|| format!("{} b{n}", ["vs", "ps"][stage]));
            }
            if r.samplers >> n & 1 != 0 {
                let slot = n as u32 + if stage == 0 { 257 } else { 0 };
                let (ta, tb) = (x.textures.iter().find(|t| t.sampler == slot), d.textures.iter().find(|t| t.sampler == slot));
                let same = match (ta, tb) {
                    (Some(p), Some(q)) => {
                        p.resource == q.resource && p.kind == q.kind && p.format == q.format && p.width == q.width
                            && p.height == q.height && p.depth == q.depth && p.levels == q.levels && p.usage == q.usage
                            && p.states[1..] == q.states[1..]
                    }
                    _ => false,
                };
                check("texture", same, &|| format!("s{slot}: {ta:?} vs {tb:?}"));
            }
        }
    }
    check("viewport", x.viewport == d.viewport, &|| format!("{:?} vs {:?}", x.viewport, d.viewport));
    if x.render_states.iter().any(|&(s, v)| s == 174 && v != 0) {
        check("scissor", x.scissor == d.scissor, &|| format!("{:?} vs {:?}", x.scissor, d.scissor));
    }
    check("targets", x.targets == d.targets, &|| format!("{:?} vs {:?}", x.targets, d.targets));
    check("depth", x.depth == d.depth, &|| format!("{:?} vs {:?}", x.depth, d.depth));
}

fn verify_report() -> String {
    let mut g = VERIFY.lock().unwrap_or_else(|e| e.into_inner());
    let Some(v) = g.take() else { return "  verify: nothing compared\n".into() };
    let mut fields: Vec<_> = v.fields.iter().collect();
    fields.sort();
    format!("  verify: {} compared, {} equal, {} merged by the D3D9 path, {} not drawn by it; mismatches: {:?}\n",
            v.compared, v.equal, v.merged, v.missed, fields)
}
