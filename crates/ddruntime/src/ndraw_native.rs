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

use crate::d3d9dev::{objects, with_device, Kind};
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
    let code = (m.u32(m.u32(record.wrapping_add(0xC))) >> 21) & 0xFF;
    let count = m.u32(record.wrapping_add(0x14));
    let (primitive, primitives) = match code.wrapping_sub(1) {
        0 => (2, count >> 1),
        1 => (3, count.wrapping_sub(1)),
        2 => (4, count / 3),
        3 => (5, count.wrapping_sub(2)),
        _ => (1, count),
    };
    let vertices = crate::d3d9draw::vertex_count(primitive, primitives).ok_or(Refusal::Primitive)?;
    let start = m.u32(record.wrapping_add(0x18));
    Ok(match kind {
        0 => (primitive, primitives, start, 0, vertices),
        _ => {
            let base = m.u32(record.wrapping_add(0x1C));
            (primitive, primitives, start, base as i32, extent.wrapping_sub(base))
        }
    })
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

/// The render states the backend reads (crates/ddgpu `derive`/`try_draw`).
/// A draw carries the applies' values, and the device's for those the
/// applies did not set (162, 194 and 206 never; the alpha test's 15, 24
/// and 25 when the binding leaves it off).
const BACKEND_STATES: [u32; 37] = [
    7, 8, 14, 15, 19, 20, 22, 23, 24, 25, 27, 52, 53, 54, 55, 56, 57, 58, 59, 162, 168, 171, 174, 175, 185, 186,
    187, 188, 189, 190, 191, 192, 193, 194, 195, 206, 207,
];
const BACKEND_STATES_MORE: [u32; 2] = [208, 209];

/// Per-run caches: the shaders' reads by bytecode, the render-state lists
/// by content (one shared list per distinct state: the backend memoizes
/// draws by the list's address).
#[derive(Default)]
pub struct Caches {
    reads: FxMap<usize, (Arc<[u32]>, Reads)>,
    states: FxMap<Vec<(u32, u32)>, RenderStateList>,
    /// native fixed state by identity and render-state list (its address:
    /// the list is held in `states`)
    fixed: FxMap<(NativeStateIdentity, u64), Arc<NativeFixedState>>,
    /// target surfaces of this frame
    targets: FxMap<u32, Option<Target>>,
    key: Vec<(u32, u32)>,
}

impl Caches {
    fn reads(&mut self, tokens: &Arc<[u32]>) -> Result<Reads, Refusal> {
        let key = Arc::as_ptr(tokens) as *const u32 as usize;
        if let Some((_, r)) = self.reads.get(&key) {
            return Ok(*r);
        }
        let r = reads(tokens)?;
        if self.reads.len() > 8192 {
            self.reads.clear();
        }
        // the Arc is held, so its address is not reused while cached
        self.reads.insert(key, (tokens.clone(), r));
        Ok(r)
    }
    fn target(&mut self, surface: u32) -> Option<Target> {
        if surface == 0 {
            return None;
        }
        self.targets.entry(surface).or_insert_with(|| crate::d3d9draw::target(surface)).clone()
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

fn mask_has(bits: &[u64; 4], n: u32) -> bool {
    n < 256 && bits[n as usize / 64] >> (n % 64) & 1 != 0
}

/// The DrawCall the D3D9 path makes for draw record `record` (type 0 or
/// 1), and the resources it reads (for flush_reads); with the D3D lock
/// held.
pub fn draw_call(m: &dyn Mem, info: &FrameInfo, caches: &mut Caches, record: u32, kind: u32) -> Result<(DrawCall, Vec<u32>), Refusal> {
    let f = decode(m, record, info.invalid_viewport, info.srgb)?;
    let viewport = f.viewport.ok_or(Refusal::ViewportGlobals)?;
    if f.targets[0] == 0 {
        return Err(Refusal::NoColourTarget);
    }
    if kind == 1 && f.indices == 0 {
        return Err(Refusal::NoIndices);
    }
    let (primitive, primitives, start, base_vertex, vertex_count) = draw_params(m, record, kind, f.extent)?;
    // the objects behind the handles
    let (vs, ps, declaration, index_format) = objects(|o| {
        let shader = |h: u32| match o.live.get(&h).map(|x| &x.kind) {
            Some(Kind::Shader { tokens, float_constants, .. }) => Some((tokens.clone(), *float_constants)),
            _ => None,
        };
        let declaration = match o.live.get(&f.declaration).map(|x| &x.kind) {
            Some(Kind::Declaration { elements, .. }) => Some(elements.clone()),
            _ => None,
        };
        let index_format = match o.live.get(&f.indices).map(|x| &x.kind) {
            Some(Kind::Buffer { data, .. }) => Some(data.format),
            _ => None,
        };
        (shader(f.vertex_shader), shader(f.pixel_shader), declaration, index_format)
    });
    let ((vs, vs_floats), (ps, ps_floats)) = (vs.ok_or(Refusal::Missing)?, ps.ok_or(Refusal::Missing)?);
    let declaration = declaration.ok_or(Refusal::Missing)?;
    // every register and sampler the shaders read comes from this binding
    let shader_reads = [caches.reads(&vs)?, caches.reads(&ps)?];
    for stage in 0..2 {
        let r = &shader_reads[stage];
        let mut floats = [0u64; 4];
        for &(n, _) in &f.floats[stage] {
            if n < 256 {
                floats[n as usize / 64] |= 1 << (n % 64);
            }
        }
        if (0..4).any(|k| r.floats[k] & !floats[k] != 0) {
            return Err(Refusal::FloatsUncovered);
        }
        let ints = f.ints[stage].iter().fold(0u16, |b, &(n, _)| b | 1u16.checked_shl(n).unwrap_or(0));
        if r.ints & !ints != 0 {
            return Err(Refusal::IntsUncovered);
        }
        let bools = f.bools[stage].iter().fold(0u16, |b, &(n, _)| b | 1u16.checked_shl(n).unwrap_or(0));
        if r.bools & !bools != 0 {
            return Err(Refusal::BoolsUncovered);
        }
        let base = if stage == 0 { 257 } else { 0 };
        for s in 0..16 {
            if r.samplers >> s & 1 != 0 && !f.textures.iter().any(|&(t, h)| t == base + s && h != 0) {
                return Err(Refusal::SamplersUncovered);
            }
        }
    }
    // render states: the applies' values, then the device's own
    let key = &mut caches.key;
    key.clear();
    for &(s, v) in &f.states {
        match key.iter_mut().find(|e| e.0 == s) {
            Some(e) => e.1 = v,
            None => key.push((s, v)),
        }
    }
    let state = |key: &Vec<(u32, u32)>, s: u32| key.iter().find(|e| e.0 == s).map(|e| e.1);
    // blend-factor blending (D3DBLEND_BLENDFACTOR / INVBLENDFACTOR): the
    // factor on the device depends on the command history
    if state(key, 27) == Some(1) && [19, 20, 207, 208].iter().any(|&s| matches!(state(key, s), Some(14 | 15))) {
        return Err(Refusal::BlendFactor);
    }
    let stages: Vec<u32> = {
        let mut v: Vec<u32> = f.textures.iter().map(|t| t.0).chain(f.sampler_states.iter().map(|s| s.0)).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let (device_ok, device_states, freq, fvf, rows) = with_device(info.device, |d| {
        let s = &d.state;
        let ok = s.in_scene && s.streams[4..].iter().all(|x| x.0 == 0);
        let states: Vec<(u32, u32)> = BACKEND_STATES.iter().chain(&BACKEND_STATES_MORE).filter_map(|&k| s.render.get(&k).map(|&v| (k, v))).collect();
        let rows: Vec<[u32; 14]> = stages.iter().map(|&t| s.sampler.row(t)).collect();
        (ok, states, s.stream_freq, s.fvf, rows)
    });
    if !device_ok {
        return Err(Refusal::Device);
    }
    for (s, v) in device_states {
        if state(key, s).is_none() {
            key.push((s, v));
        }
    }
    key.sort_unstable_by_key(|e| e.0);
    let render_states = match caches.states.get(key.as_slice()) {
        Some(l) => l.clone(),
        None => {
            if caches.states.len() > 4096 {
                caches.states.clear();
                caches.fixed.clear();
            }
            let l: RenderStateList = key.clone().into();
            caches.states.insert(key.clone(), l.clone());
            l
        }
    };
    // textures with their sampler states: the device's row, the binding's
    // states over it
    let mut rows = rows;
    for &(t, ty, v) in &f.sampler_states {
        let k = stages.iter().position(|&x| x == t).unwrap();
        rows[k][ty as usize % 14] = v;
    }
    if (1..=5).contains(&info.filtering) {
        // 0x00DA7250's non-null branch (see the module notes)
        for row in rows.iter_mut() {
            if row[6] == 3 {
                if info.filtering == 1 {
                    row[5] = 2;
                    row[6] = 2;
                    row[7] = 2;
                } else {
                    row[10] = 1 << (info.filtering - 1);
                }
            }
        }
    }
    let mut bound: Vec<(u32, u32)> = Vec::with_capacity(f.textures.len());
    for &(t, h) in &f.textures {
        match bound.iter_mut().find(|b| b.0 == t) {
            Some(b) => b.1 = h,
            None => bound.push((t, h)),
        }
    }
    bound.retain(|b| b.1 != 0);
    bound.sort_unstable_by_key(|b| b.0);
    let textures: Option<Vec<TextureBinding>> = objects(|o| {
        bound.iter().map(|&(t, h)| match o.live.get(&h).map(|x| &x.kind) {
            Some(Kind::Texture { data, .. }) => Some(TextureBinding {
                sampler: t, resource: h, states: rows[stages.iter().position(|&x| x == t).unwrap()],
                kind: data.kind, format: data.format, width: data.width, height: data.height,
                depth: data.depth, levels: data.levels, usage: data.usage,
            }),
            _ => None,
        }).collect()
    });
    let textures = textures.ok_or(Refusal::Missing)?;
    // constants: the binding's registers over zeros (nothing else is read)
    let constants = |stage: usize, count: usize| {
        let mut c = Constants { f: vec![[0; 4]; count.min(FLOATS[stage])], ..Default::default() };
        for &(n, v) in &f.floats[stage] {
            if let Some(r) = c.f.get_mut(n as usize) {
                *r = v;
            }
        }
        for &(n, v) in &f.ints[stage] {
            if let Some(r) = c.i.get_mut(n as usize) {
                *r = v;
            }
        }
        for &(n, v) in &f.bools[stage] {
            if let Some(r) = c.b.get_mut(n as usize) {
                *r = v;
            }
        }
        Arc::new(c)
    };
    let streams: Vec<(u32, u32, u32, u32, u32)> = (0..4u32)
        .filter(|&k| f.streams[k as usize].0 != 0)
        .map(|k| { let (h, off, stride) = f.streams[k as usize]; (k, h, off, stride, freq[k as usize]) })
        .collect();
    let indices = if kind == 1 { Indices::Buffer(f.indices, index_format.ok_or(Refusal::Missing)?) } else { Indices::None };
    let targets: [Option<Target>; 4] = std::array::from_fn(|k| caches.target(f.targets[k]));
    if targets[0].is_none() {
        return Err(Refusal::Missing);
    }
    let depth = caches.target(f.depth);
    // what the D3D9 path flushes: textures, streams, indices, targets
    let mut reads: Vec<u32> = textures.iter().map(|t| t.resource).collect();
    reads.extend(streams.iter().map(|s| s.1));
    if kind == 1 {
        reads.push(f.indices);
    }
    reads.extend(f.targets.iter().copied().filter(|&t| t != 0));
    let d = DrawCall {
        primitive, primitives, start, base_vertex, min_index: 0, vertex_count,
        vertices: Vertices::Streams(streams.into()),
        indices,
        vertex_shader: Some(vs), pixel_shader: Some(ps),
        declaration, fvf,
        vs_constants: constants(0, vs_floats), ps_constants: constants(1, ps_floats),
        textures: textures.into(),
        render_states,
        viewport,
        scissor: f.scissor.unwrap_or([0; 4]),
        targets, depth,
    };
    Ok((d, reads))
}

/// The fixed-state objects of a draw record: address and hash (+0xC), and
/// the rasterizer variant (fixed binding +4 bit 8).
pub fn identity(m: &dyn Mem, record: u32) -> NativeStateIdentity {
    let fixed = m.u32(record.wrapping_add(8));
    let id = |o: u32| {
        let address = m.u32(fixed.wrapping_add(o));
        NativeObjectIdentity { address, hash: if address == 0 { 0 } else { m.u32(address.wrapping_add(0xC)) } }
    };
    NativeStateIdentity { blend: id(8), depth: id(0xC), raster: id(0x10), raster_flip: m.u32(fixed.wrapping_add(4)) >> 8 & 1 != 0 }
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
    let state = caches.fixed.entry((identity, d.render_states.address())).or_insert_with(|| {
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
    let binding = m.u32(record.wrapping_add(4));
    if binding == 0 {
        return (0, 0);
    }
    let view = m.u32(binding);
    if view == 0 {
        return (0, 0);
    }
    (m.u16(view.wrapping_add(0x18)) as u32, m.u16(view.wrapping_add(0x1A)) as u32 & 0x7FFF)
}

impl CommandSink for Native {
    fn frame_begin(&self, m: &mut dyn Mem, render: u32, _context: u32) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.info = FrameInfo::read(m, render);
        g.caches.targets.clear();
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
        f.targets.insert(m.u32(record.wrapping_add(4)));
        f.states.insert(m.u32(record.wrapping_add(8)));
        f.shaders.insert(m.u32(record.wrapping_add(0xC)));
        f.streams.insert(m.u32(record.wrapping_add(0x10)));
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
            Ok((d, reads)) if self.mode == Mode::DrawCall => {
                crate::d3d9draw::flush_reads(m, reads);
                crate::d3d9draw::backend_unlocked(|b| b.draw(&d));
                t.taken += 1;
                Taken::Handled
            }
            Ok((d, reads)) => match native_draw(&d, identity(m, record), &mut g.caches) {
                Err(r) => {
                    *t.refused.entry(r).or_default() += 1;
                    Taken::Default
                }
                Ok(native) => {
                    if self.mode == Mode::Compare {
                        // Finish before any upload can replace resources used
                        // by an earlier draw in the pending shadow pass.
                        let dirty = objects(|o| reads.iter().any(|r| o.dirty.contains(r)));
                        if dirty {
                            crate::d3d9draw::backend_unlocked(|b| g.comparison.finish(b));
                        }
                        crate::d3d9draw::flush_reads(m, reads);
                        crate::d3d9draw::backend_unlocked(|b| g.comparison.draw(b, native, &d));
                        t.taken += 1;
                        drop(lock);
                        t.decode += started.elapsed();
                        return Taken::Handled;
                    }
                    crate::d3d9draw::flush_reads(m, reads);
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
