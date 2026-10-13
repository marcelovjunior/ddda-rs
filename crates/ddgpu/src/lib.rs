//! The runtime's graphics backend on wgpu (docs/RUNTIME.md, "Graphics
//! backend design", steps 3-5): executes the Direct3D 9 commands of
//! `d3d9cmd` (uploads, draws, clears, presents) on Vulkan/DX12/Metal.
//!
//! Resources are created on first use: textures from their binding (kind,
//! size, levels, format) with every pending level upload written, render
//! targets from their target description. Shaders are the game's bytecode
//! translated by `d3d9wgsl` (pixel inputs linked to the vertex outputs by
//! usage and index, D3D9's half-pixel offset, the alpha test). Render
//! states map to wgpu pipeline state. Formats wgpu lacks or samples
//! differently are converted on upload to what D3D9 returns when sampled
//! (L8 as (L, L, L, 1), R32F as (R, 1, 1, 1), X8R8G8B8 with alpha 1, ...).
//!
//! One render pass per draw and per clear. What a draw builds from its
//! state is cached by that state (src/cache.rs: shader inputs, layouts,
//! pipelines, texture bind groups, target views); constants and
//! DrawPrimitiveUP data go into ring buffers; draws are recorded into one
//! command encoder and submitted every 128 draws or before anything else
//! uses the queue (tests/cache.rs: a long-lived backend draws every frame
//! like a fresh one).
use d3d9cmd::{ClearCall, CopyCall, DrawCall, FillCall, GammaRamp, GraphicsBackend, Indices, Target, TargetTexture, TexKind, TextureBinding, USAGE_DEPTHSTENCIL, USAGE_RENDERTARGET, Upload, Vertices};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

mod cache;
mod compute_quads;
mod backend_data;
mod encode;
pub mod vk;
pub mod native;
pub mod native_compare;
mod native_job;
pub use native_job::NativeJobStats;
mod pipelines;
mod shader_cache;
mod pipeline_log;
mod warmup;
pub mod present;
pub use pipelines::{default_cache_dir, pipeline_stats, report_pipeline_stats_every, save_pipeline_caches, PipelineStats};
use cache::{ModuleKey, PipelineKey, Ring, SamplerKinds, TargetKey, TexGroupKey, DRAWS_PER_SUBMIT, INLINE_RING, PS_BLOCK, UNIFORM_ALIGN, UNIFORM_RING, VS_BLOCK};
mod shader_identity;
use std::future::Future;

/// Pack raw register bits without a per-component loop on little-endian
/// hosts. Arrays of four u32 have no padding and every input byte is valid.
#[cfg(test)]
use backend_data::copy_registers as copy_float_registers;

#[cfg(test)]
mod constant_bytes_tests {
    #[test]
    fn raw_register_bits_match_little_endian_words() {
        let registers = [[0, 0x8000_0000, 0x7fc0_1234, u32::MAX], [1, 2, 3, 4]];
        let mut bytes = [0u8; 32];
        super::copy_float_registers(&mut bytes, &registers);
        let expected: Vec<u8> = registers.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(bytes.as_slice(), expected);
        super::copy_float_registers(&mut [], &[]);
    }
}

fn block_on<F: Future>(f: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

/// How a D3D9 format is held on the GPU for sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Conversion {
    /// the bytes as they are
    Direct,
    /// 32-bit BGRA/RGBA with alpha forced to 1
    OpaqueAlpha,
    /// 16-bit packed colours expanded to RGBA8
    R5G6B5,
    X1R5G5B5,
    A1R5G5B5,
    A4R4G4B4,
    /// luminance/alpha formats expanded to RGBA8
    L8,
    A8L8,
    A8,
    /// float formats widened to four channels, missing ones 1
    R32F,
    G32R32F,
    R16F,
    G16R16F,
    /// 16-bit unsigned/signed normalised channels widened to float, as
    /// D3D9 samples them (missing channels 1)
    G16R16,
    A16B16G16R16,
    L16,
    V16U16,
    Q16W16V16U16,
}

/// (wgpu format, conversion, bytes per texel or per 4x4 block, block size)
fn sampled_format(d3d: u32) -> Option<(wgpu::TextureFormat, Conversion, u32, u32)> {
    use wgpu::TextureFormat as F;
    use Conversion::*;
    Some(match d3d {
        21 => (F::Bgra8Unorm, Direct, 4, 1),
        22 => (F::Bgra8Unorm, OpaqueAlpha, 4, 1),
        32 => (F::Rgba8Unorm, Direct, 4, 1),
        33 => (F::Rgba8Unorm, OpaqueAlpha, 4, 1),
        23 => (F::Rgba8Unorm, R5G6B5, 2, 1),
        24 => (F::Rgba8Unorm, X1R5G5B5, 2, 1),
        25 => (F::Rgba8Unorm, A1R5G5B5, 2, 1),
        26 => (F::Rgba8Unorm, A4R4G4B4, 2, 1),
        50 => (F::Rgba8Unorm, L8, 1, 1),
        51 => (F::Rgba8Unorm, A8L8, 2, 1),
        28 => (F::Rgba8Unorm, A8, 1, 1),
        113 => (F::Rgba16Float, Direct, 8, 1),
        116 => (F::Rgba32Float, Direct, 16, 1),
        114 => (F::Rgba32Float, R32F, 4, 1),
        115 => (F::Rgba32Float, G32R32F, 8, 1),
        111 => (F::Rgba16Float, R16F, 2, 1),
        112 => (F::Rgba16Float, G16R16F, 4, 1),
        31 => (F::Rgb10a2Unorm, Direct, 4, 1),
        34 => (F::Rgba32Float, G16R16, 4, 1),
        36 => (F::Rgba32Float, A16B16G16R16, 8, 1),
        81 => (F::Rgba32Float, L16, 2, 1),
        63 => (F::Rgba8Snorm, Direct, 4, 1),
        64 => (F::Rgba32Float, V16U16, 4, 1),
        110 => (F::Rgba32Float, Q16W16V16U16, 8, 1),
        f if f == fourcc(b"DXT1") => (F::Bc1RgbaUnorm, Direct, 8, 4),
        f if f == fourcc(b"DXT2") || f == fourcc(b"DXT3") => (F::Bc2RgbaUnorm, Direct, 16, 4),
        f if f == fourcc(b"DXT4") || f == fourcc(b"DXT5") => (F::Bc3RgbaUnorm, Direct, 16, 4),
        _ => return None,
    })
}

/// The format a D3D9 render target or depth-stencil surface renders as.
fn target_format(d3d: u32, srgb: bool) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as F;
    Some(match d3d {
        21 | 22 => if srgb { F::Bgra8UnormSrgb } else { F::Bgra8Unorm },
        32 | 33 => if srgb { F::Rgba8UnormSrgb } else { F::Rgba8Unorm },
        113 => F::Rgba16Float,
        116 => F::Rgba32Float,
        114 => F::R32Float,
        112 => F::Rg16Float,
        111 => F::R16Float,
        115 => F::Rg32Float,
        34 => F::Rg16Unorm,
        36 => F::Rgba16Unorm,
        31 => F::Rgb10a2Unorm,
        75 | 77 | 79 => F::Depth24PlusStencil8,
        80 => F::Depth16Unorm,
        71 | 82 => F::Depth32Float,
        f if f == fourcc(b"INTZ") || f == fourcc(b"DF24") => F::Depth24PlusStencil8,
        f if f == fourcc(b"DF16") => F::Depth16Unorm,
        _ => return None,
    })
}

/// Convert one row-pitched image to the GPU layout (tight rows).
fn convert(conv: Conversion, bytes: &[u8], width: u32, rows: u32, pitch: u32, bpp_src: u32) -> Vec<u8> {
    let (w, h, pitch) = (width as usize, rows as usize, pitch as usize);
    let src_row = |y: usize| &bytes[(y * pitch).min(bytes.len())..(y * pitch + w * bpp_src as usize).min(bytes.len())];
    let mut out = Vec::new();
    let u16le = |b: &[u8], k: usize| u16::from_le_bytes([b[2 * k], b[2 * k + 1]]);
    let f32le = |b: &[u8], k: usize| f32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap());
    let one16 = half_one();
    for y in 0..h {
        let r = src_row(y);
        match conv {
            Conversion::Direct => out.extend_from_slice(r),
            Conversion::OpaqueAlpha => {
                for px in r.chunks_exact(4) {
                    out.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
            Conversion::R5G6B5 | Conversion::X1R5G5B5 | Conversion::A1R5G5B5 | Conversion::A4R4G4B4 => {
                for k in 0..w {
                    let v = u16le(r, k) as u32;
                    let ex = |bits: u32, n: u32| ((bits * 255 + ((1 << n) - 1) / 2) / ((1 << n) - 1)) as u8;
                    let px = match conv {
                        Conversion::R5G6B5 => [ex(v >> 11, 5), ex((v >> 5) & 63, 6), ex(v & 31, 5), 255],
                        Conversion::X1R5G5B5 => [ex((v >> 10) & 31, 5), ex((v >> 5) & 31, 5), ex(v & 31, 5), 255],
                        Conversion::A1R5G5B5 => [ex((v >> 10) & 31, 5), ex((v >> 5) & 31, 5), ex(v & 31, 5), if v >> 15 != 0 { 255 } else { 0 }],
                        _ => [ex((v >> 8) & 15, 4), ex((v >> 4) & 15, 4), ex(v & 15, 4), ex(v >> 12, 4)],
                    };
                    out.extend_from_slice(&px);
                }
            }
            Conversion::L8 => {
                for &l in &r[..w] {
                    out.extend_from_slice(&[l, l, l, 255]);
                }
            }
            Conversion::A8L8 => {
                for px in r.chunks_exact(2) {
                    out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
                }
            }
            Conversion::A8 => {
                for &a in &r[..w] {
                    out.extend_from_slice(&[0, 0, 0, a]);
                }
            }
            Conversion::R32F | Conversion::G32R32F => {
                for k in 0..w {
                    let (r0, g0) = if conv == Conversion::R32F { (f32le(r, k), 1.0) } else { (f32le(r, 2 * k), f32le(r, 2 * k + 1)) };
                    for v in [r0, g0, 1.0, 1.0] {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
            Conversion::G16R16 | Conversion::A16B16G16R16 | Conversion::L16 | Conversion::V16U16 | Conversion::Q16W16V16U16 => {
                let un = |k: usize| u16le(r, k) as f32 / 65535.0;
                let sn = |k: usize| (u16le(r, k) as i16 as f32 / 32767.0).max(-1.0);
                for k in 0..w {
                    let v = match conv {
                        Conversion::G16R16 => [un(2 * k), un(2 * k + 1), 1.0, 1.0],
                        Conversion::A16B16G16R16 => [un(4 * k), un(4 * k + 1), un(4 * k + 2), un(4 * k + 3)],
                        Conversion::L16 => [un(k), un(k), un(k), 1.0],
                        Conversion::V16U16 => [sn(2 * k), sn(2 * k + 1), 1.0, 1.0],
                        _ => [sn(4 * k), sn(4 * k + 1), sn(4 * k + 2), sn(4 * k + 3)],
                    };
                    for x in v {
                        out.extend_from_slice(&x.to_le_bytes());
                    }
                }
            }
            Conversion::R16F | Conversion::G16R16F => {
                for k in 0..w {
                    let (r0, g0) = if conv == Conversion::R16F { (u16le(r, k), one16) } else { (u16le(r, 2 * k), u16le(r, 2 * k + 1)) };
                    for v in [r0, g0, one16, one16] {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
            }
        }
    }
    out
}

/// 1.0 as an IEEE half.
fn half_one() -> u16 {
    0x3C00
}

/// StretchRect and ColorFill: a quad over the destination rectangle (the
/// viewport) that samples the source rectangle, or outputs a colour.
const BLIT: &str = "
struct Params { src: vec4<f32>, colour: vec4<f32>, mode: vec4<u32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  let c = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  return V(vec4<f32>(c * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), p.src.xy + c * p.src.zw);
}
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
  if (p.mode.x == 1u) { return p.colour; }
  return textureSampleLevel(tex, smp, v.uv, 0.0);
}
";

struct Tex {
    texture: wgpu::Texture,
    kind: TexKind,
    format: u32,
    gpu_format: wgpu::TextureFormat,
    levels: u32,
    width: u32,
    height: u32,
    depth: u32,
    /// created with D3DUSAGE_RENDERTARGET or D3DUSAGE_DEPTHSTENCIL: stored
    /// in the target format, a render attachment from creation
    target: bool,
}

/// The channels (bit k: channel k of RGBA) a render-target texture of
/// D3D9 format `format` lacks in its wgpu target format, which D3D9 samples
/// as 1 (as the sampled formats' conversions fill them; compared with the
/// driver in tests/targets.rs render_to_texture).
fn missing_channels(format: u32) -> u8 {
    match format {
        // X8R8G8B8, X8B8G8R8: alpha
        22 | 33 => 0b1000,
        // R32F, R16F (wgpu samples G and B as 0)
        114 | 111 => 0b1110,
        // G32R32F, G16R16F, G16R16
        115 | 112 | 34 => 0b1100,
        _ => 0,
    }
}

struct Rt {
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}

/// The wgpu sample count of a target's D3DMULTISAMPLE_TYPE. The game asks
/// for 2, 4 or 8 samples at quality 0 (the table at 0x01440450 through
/// 0x011487C0, gated by the recorded driver's answers); NVIDIA's CSAA
/// qualities and D3DMULTISAMPLE_NONMASKABLE have no wgpu equivalent.
pub fn sample_count(t: &Target) -> Result<u32, String> {
    match (t.samples, t.quality) {
        (0, _) => Ok(1),
        (n @ (2 | 4 | 8 | 16), 0) => Ok(n),
        (1, q) => Err(format!("D3DMULTISAMPLE_NONMASKABLE (quality {q}) is not implemented")),
        (n, q) => Err(format!("{n} samples at quality {q} (coverage sampling) is not implemented")),
    }
}

/// A presented frame, read back (headless sessions and tests).
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, rows top to bottom
    pub rgba: Vec<u8>,
}

pub struct WgpuBackend {
    native_worker: Option<native_job::Worker>,
    native_stats: NativeJobStats,
    native_comparisons: Option<native_job::Comparisons>,
    shader_identities: shader_identity::ShaderIdentities,
    /// Derived draw state by retained object identity and full value equality.
    memo: backend_data::DrawMemo<DrawMemo>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    textures: cache::FxMap<u32, Tex>,
    /// image uploads for textures not created yet: resource -> (index, upload)
    pending: HashMap<u32, Vec<Upload>>,
    buffers: cache::FxMap<u32, wgpu::Buffer>,
    /// standalone surfaces used as targets (back buffers, render targets,
    /// depth-stencil)
    surfaces: cache::FxMap<u32, Rt>,
    /// translated shaders by (shader identity, linked vertex identity, depth
    /// bias, channels sampled as 1)
    modules: cache::FxMap<ModuleKey, (wgpu::ShaderModule, std::sync::Arc<d3d9wgsl::Interface>)>,
    samplers: cache::FxMap<[u32; 14], wgpu::Sampler>,
    blits: HashMap<(wgpu::TextureFormat, u32), (wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    /// clears of part of a target, by (colour formats per slot, depth
    /// format, depth written, stencil written)
    clears: HashMap<ClearKey, (wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
    /// the last presented frame, if reading frames back is enabled
    pub last_frame: Option<Frame>,
    pub read_frames: bool,
    pub errors: Vec<String>,
    /// Called with every presented frame, read back (headless capture).
    pub on_present: Option<Box<dyn FnMut(&Frame) + Send>>,
    /// Shown every presented back buffer on the GPU (a window host).
    pub present_sink: Option<Box<dyn PresentSink>>,
    /// occlusion counting: wgpu query sets of OCCLUSION_SET slots each;
    /// one slot per draw made while guest queries count
    occlusion_sets: Vec<wgpu::QuerySet>,
    free_slots: Vec<u32>,
    /// slot -> how many guest queries include it
    slot_users: HashMap<u32, u32>,
    /// guest query -> the slots of the draws it counted
    query_slots: HashMap<u32, Vec<u32>>,
    /// guest query -> its resolved count
    query_counts: HashMap<u32, u64>,
    /// L120: occlusion resolves in flight (read-back buffer, map state:
    /// 0 pending, 1 mapped, 2 failed, the slot count), with async_queries
    pending_queries: HashMap<u32, (wgpu::Buffer, std::sync::Arc<std::sync::atomic::AtomicU8>, usize)>,
    /// L120: query_result never waits for the GPU (None until the count is
    /// back); off by default, where it resolves and waits as before
    pub async_queries: bool,
    /// L124: the finisher thread (set_async_submit)
    finisher: Option<Finisher>,
    encode_threads: usize,
    encode_profile: encode::Profile,
    chunks: RefCell<Vec<vk::Chunk>>,
    chunk_start: Cell<u32>,
    /// Draws (and clears and copies) recorded before a submission
    /// (DRAWS_PER_SUBMIT by default; ddplay lowers it with async submits,
    /// so encoding overlaps recording within a frame)
    pub draws_per_submit: u32,
    /// guest queries between D3DISSUE_BEGIN and D3DISSUE_END
    active_queries: Vec<u32>,
    gamma: Option<Box<GammaRamp>>,
    /// the adapter drawing (its backend decides some rasteriser details)
    pub adapter: wgpu::AdapterInfo,
    /// the render pass of the open encoder that draws are recorded into,
    /// kept open while consecutive draws use the same attachments (L72)
    pass: RefCell<Option<OpenPass>>,
    /// L113: the open compute pass of quad dispatches (compute_quads)
    cpass: RefCell<Option<wgpu::ComputePass<'static>>>,
    /// DDGPU_NATIVE_SIM=1: eligible full-target quad draws run as dispatches
    native_sim: bool,
    quads: compute_quads::QuadCache,
    /// CPU copies of small vertex buffers (a quad's vertices, compute_quads)
    small_buffers: cache::FxMap<u32, Vec<u8>>,
    /// draws recorded, and render passes begun for them (L72)
    draws_recorded: Cell<u64>,
    passes_begun: Cell<u64>,
    submissions: Cell<u64>,
    /// draws recorded and not yet submitted; submitted (with the ring
    /// bytes) before anything else touches the queue
    open: RefCell<Option<wgpu::CommandEncoder>>,
    open_draws: Cell<u32>,
    /// buffers and textures the open draws read
    open_uses: RefCell<backend_data::ResourceUses>,
    draw_buffers: Vec<(wgpu::Buffer, u64, u64, Option<u32>)>,
    instance_streams: Vec<u32>,
    /// per stage (0 pixel, 1 vertex): the last block's constants and extra
    /// vec4s, its uniform ring offset and the ring generation it was
    /// written in (L78)
    last_constants: RefCell<[(std::sync::Arc<d3d9cmd::Constants>, Vec<[u32; 4]>, u32, u64, usize); 2]>,
    uniforms: RefCell<Ring>,
    inline: RefCell<Ring>,
    /// group 0: the constant blocks in the uniform ring (dynamic offsets)
    layout0: wgpu::BindGroupLayout,
    group0: wgpu::BindGroup,
    /// the vertex shader's inputs and the pixel shader's input declarations
    /// (usage, index), by token hash
    vs_inputs: cache::FxMap<u64, Vec<(u32, u32)>>,
    ps_inputs: cache::FxMap<u64, Vec<(u32, u32)>>,
    tex_layouts: cache::FxMap<(bool, SamplerKinds), wgpu::BindGroupLayout>,
    pipeline_layouts: cache::FxMap<(SamplerKinds, SamplerKinds), wgpu::PipelineLayout>,
    pipelines: cache::FxMap<backend_data::Hashed<PipelineKey>, wgpu::RenderPipeline>,
    vk: Option<vk::Context>,
    vk_pass: RefCell<Option<vk::Pass>>,
    tex_groups: cache::FxMap<TexGroupKey, wgpu::BindGroup>,
    /// per stage (0 pixel, 1 vertex): the last draw's bind group key and
    /// group (L94b: a draw binding what the previous one bound takes no
    /// lookup), and a reused buffer the key is built in
    last_group: [(Vec<cache::TexGroupEntry>, Option<wgpu::BindGroup>); 2],
    group_key: [Vec<cache::TexGroupEntry>; 2],
    target_views: cache::FxMap<TargetKey, Option<(wgpu::TextureView, wgpu::TextureFormat)>>,
    /// the draws' pipelines are created through this cache, kept on disk
    /// ([`enable_pipeline_cache`](Self::enable_pipeline_cache))
    pipeline_cache: Option<std::sync::Arc<pipelines::Persistent>>,
    warmup: Option<warmup::Warmup>,
    warm_hits: u64,
    created_modules: u64,
    created_pipelines: u64,
}

/// Where a host shows presented frames without reading them back: called
/// at every Present, after the frame's commands were submitted, on the
/// backend's device and queue (submissions run in order, so the back
/// buffer holds the complete frame for whatever the sink submits now).
pub trait PresentSink: Send {
    fn present(&mut self, frame: &Presented);
}

/// The back buffer of one Present, as a sink gets it.
pub struct Presented<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// the back buffer surface's texture (TEXTURE_BINDING | COPY_SRC)
    pub texture: &'a wgpu::Texture,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
    /// the ramp set by SetGammaRamp, applied to each 8-bit channel (the
    /// entry's high byte), if any
    pub gamma: Option<&'a GammaRamp>,
}

/// Occlusion query slots per wgpu query set.
const OCCLUSION_SET: u32 = 256;

/// What a draw's render pass draws into: the colour attachments (one per
/// slot up to the last one written), the depth-stencil attachment, the
/// occlusion query set.
type PassKey = (Vec<Option<wgpu::TextureView>>, Option<wgpu::TextureView>, Option<wgpu::QuerySet>);

/// A render pass left open for the next draw (L72). Every attachment is
/// loaded and stored, so consecutive draws in one pass draw what they drew
/// in a pass each: a draw sets all the pass state it uses (pipeline, bind
/// groups, viewport, stencil reference, blend constant, buffers), and the
/// scissor rectangle when its scissor test is on.
/// L124: a thread that finishes command encoders and submits them in the
/// order given.
struct Finisher {
    send: Option<std::sync::mpsc::Sender<Vec<vk::Chunk>>>,
    sent: Cell<u64>,
    done: Arc<(std::sync::Mutex<(u64, bool)>, std::sync::Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Finisher {
    fn new(queue: wgpu::Queue, threads: usize, profile: encode::Profile) -> Finisher {
        let (send, recv) = std::sync::mpsc::channel::<Vec<vk::Chunk>>();
        let done = Arc::new((std::sync::Mutex::new((0u64, false)), std::sync::Condvar::new()));
        let d = done.clone();
        let thread = std::thread::Builder::new().name("wgpu-finish".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let pool = encode::Pool::new(threads, profile.clone(), vk::finish);
                for encoders in recv {
                    let buffers = pool.finish(encoders);
                    let start = profile.start();
                    vk::submit(&queue, buffers);
                    profile.end(8, start);
                    let (m, c) = &*d;
                    m.lock().unwrap().0 += 1;
                    c.notify_all();
                }
            }));
            if result.is_err() {
                let (m, c) = &*d;
                m.lock().unwrap().1 = true;
                c.notify_all();
            }
        }).expect("start the finisher thread");
        Finisher { send: Some(send), sent: Cell::new(0), done, thread: Some(thread) }
    }

    fn submit(&self, enc: wgpu::CommandEncoder) {
        self.submit_batch(vec![vk::Chunk::Wgpu(enc)]);
    }

    fn submit_batch(&self, encoders: Vec<vk::Chunk>) {
        self.sent.set(self.sent.get() + 1);
        self.send.as_ref().unwrap().send(encoders).expect("the finisher thread stopped");
    }

    fn wait(&self) {
        let (m, c) = &*self.done;
        let mut submitted = m.lock().unwrap();
        while submitted.0 < self.sent.get() && !submitted.1 {
            submitted = c.wait(submitted).unwrap();
        }
        assert!(!submitted.1, "the finisher thread failed during encoding/submission");
    }
}

impl Drop for Finisher {
    fn drop(&mut self) {
        drop(self.send.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The draw memo's size: one slot per (shaders, declaration, render-state
/// list) hash; a collision derives again.

/// What try_draw derives before binding resources (L99).
struct Derived {
    vk_pipeline: Option<Arc<vk::Pipeline>>,
    ps_kinds: Vec<(u32, u8)>,
    vs_kinds: Vec<(u32, u8)>,
    layout1: wgpu::BindGroupLayout,
    layout2: wgpu::BindGroupLayout,
    depth_bias: bool,
    streams: Vec<u32>,
    colour: Vec<Option<(wgpu::TextureView, wgpu::TextureFormat)>>,
    depth: Option<(wgpu::TextureView, wgpu::TextureFormat)>,
    max_out: usize,
    pipeline: wgpu::RenderPipeline,
    /// float4 constant registers each stage's shader reads (L90)
    vs_floats: usize,
    ps_floats: usize,
}

/// A memo entry: the inputs a Derived was derived from, and it.
struct DrawMemo {
    vs: Arc<[u32]>,
    ps: Arc<[u32]>,
    declaration: Arc<[[u8; 8]]>,
    states: d3d9cmd::RenderStateList,
    primitive: u32,
    index_format: u32,
    ones: u64,
    instanced: Vec<u32>,
    targets: [Option<Target>; 4],
    depth: Option<Target>,
    strides: Vec<(u32, u32)>,
    derived: Arc<Derived>,
}

struct OpenPass {
    pass: wgpu::RenderPass<'static>,
    key: PassKey,
    /// a draw set the scissor rectangle: a draw without the scissor test
    /// starts a new pass (whose scissor is the whole attachment)
    scissored: bool,
    /// What the pass has set (L80): a draw records only the commands whose
    /// values differ, so wgpu validates and encodes fewer commands per draw.
    /// Equal to recording them all: a render pass keeps its pipeline, bind
    /// groups, buffers and dynamic state until they are set again.
    last: PassState,
}

#[derive(Default)]
struct PassState {
    pipeline: Option<wgpu::RenderPipeline>,
    group0: Option<[u32; 2]>,
    group1: Option<wgpu::BindGroup>,
    group2: Option<wgpu::BindGroup>,
    viewport: Option<[u32; 6]>,
    scissor: Option<[i32; 4]>,
    stencil_ref: Option<u32>,
    blend: Option<u32>,
    vertex: Vec<Option<(wgpu::Buffer, u64, u64)>>,
    index: Option<(wgpu::Buffer, u64, u64, wgpu::IndexFormat)>,
}

impl WgpuBackend {
    /// The device features the backend needs: hosts that create the
    /// device themselves (a window sharing it with its surface) request
    /// these.
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::TEXTURE_COMPRESSION_BC
    }

    /// Features used where the adapter has them: D3DTADDRESS_BORDER, 16-bit
    /// normalised targets (G16R16, A16B16G16R16), D3DFILL_WIREFRAME/POINT,
    /// and blending into 32-bit float targets (the D3D9 driver blends every
    /// render-target format: POSTPIXELSHADER_BLENDING in the recorded
    /// profile). Without one, the draws that need it report an error.
    pub fn optional_features() -> wgpu::Features {
        wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER | wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::POLYGON_MODE_LINE | wgpu::Features::POLYGON_MODE_POINT | wgpu::Features::FLOAT32_BLENDABLE
            // render attachments of formats beyond WebGPU's baseline
            // (G16R16 and A16B16G16R16 as Rg16Unorm/Rgba16Unorm targets)
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            // a pipeline cache kept between runs (Vulkan; L71)
            | wgpu::Features::PIPELINE_CACHE
    }

    /// A backend on the default adapter without a window.
    pub fn headless() -> Option<WgpuBackend> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        // Do not initialize the unused OpenGL backend alongside Vulkan/DX12.
        // On the NVIDIA Windows driver, parallel headless instance creation
        // can deadlock Vulkan device/pipeline creation against WGL's instance
        // thread exiting inside the driver (L96, tests/cache.rs). Native
        // backends remain available; WGPU_BACKEND can explicitly opt into GL.
        descriptor.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let instance = wgpu::Instance::new(descriptor);
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let needed = Self::required_features();
        if !adapter.features().contains(needed) {
            return None;
        }
        let optional = Self::optional_features();
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: needed | (adapter.features() & optional),
            ..Default::default()
        })).ok()?;
        Some(Self::new(device, queue, adapter.get_info()))
    }

    /// A backend on a device the host created (with
    /// [`required_features`](Self::required_features)), e.g. the one a
    /// window's surface is configured with, so presenting needs no copy
    /// between devices; `adapter` is the device's adapter. `read_frames`
    /// starts on, as in headless mode; a host that shows frames through a
    /// [`PresentSink`] turns it off.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, adapter: wgpu::AdapterInfo) -> WgpuBackend {
        let uniforms = Ring::new(&device, UNIFORM_RING, wgpu::BufferUsages::UNIFORM, "constants");
        let inline = Ring::new(&device, INLINE_RING, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX, "inline data");
        // L113: with DDGPU_NATIVE_SIM the pixel stage's bindings are also
        // the quad dispatches' (compute_quads)
        let native_sim = compute_quads::enabled();
        let ps_visibility = if native_sim { wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE } else { wgpu::ShaderStages::FRAGMENT };
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding, visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None },
            count: None,
        };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[uniform(0, ps_visibility), uniform(2, wgpu::ShaderStages::VERTEX)],
        });
        let block = |size| wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &uniforms.buffer, offset: 0, size: std::num::NonZeroU64::new(size) });
        let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout0,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: block(PS_BLOCK) }, wgpu::BindGroupEntry { binding: 2, resource: block(VS_BLOCK) }],
        });
        let vk = vk::enabled().then(|| vk::Context::new(&device, &uniforms.buffer)
            .map_err(|e| eprintln!("DDGPU_VK disabled: {e}"))).and_then(Result::ok);
        let mut backend = WgpuBackend {
            vk,
            vk_pass: RefCell::new(None),
            native_worker: None,
            native_stats: Default::default(),
            native_comparisons: None,
            device,
            queue,
            textures: Default::default(),
            pending: HashMap::new(),
            buffers: Default::default(),
            surfaces: Default::default(),
            modules: Default::default(),
            samplers: Default::default(),
            blits: HashMap::new(),
            clears: HashMap::new(),
            last_frame: None,
            read_frames: true,
            errors: Vec::new(),
            on_present: None,
            present_sink: None,
            occlusion_sets: Vec::new(),
            free_slots: Vec::new(),
            slot_users: HashMap::new(),
            query_slots: HashMap::new(),
            query_counts: HashMap::new(),
            pending_queries: HashMap::new(),
            async_queries: false,
            finisher: None,
            encode_threads: encode::threads_from_env(),
            encode_profile: encode::Profile::from_env(),
            chunks: RefCell::new(Vec::new()),
            chunk_start: Cell::new(0),
            draws_per_submit: DRAWS_PER_SUBMIT,
            active_queries: Vec::new(),
            gamma: None,
            adapter,
            pass: RefCell::new(None),
            cpass: RefCell::new(None),
            native_sim,
            quads: Default::default(),
            small_buffers: Default::default(),
            draws_recorded: Cell::new(0),
            passes_begun: Cell::new(0),
            submissions: Cell::new(0),
            open: RefCell::new(None),
            open_draws: Cell::new(0),
            open_uses: RefCell::new(backend_data::ResourceUses::default()),
            draw_buffers: Vec::new(), instance_streams: Vec::new(),
            shader_identities: Default::default(),
            memo: Default::default(),
            last_constants: RefCell::new([(Default::default(), Vec::new(), 0, u64::MAX, 0), (Default::default(), Vec::new(), 0, u64::MAX, 0)]),
            uniforms: RefCell::new(uniforms),
            inline: RefCell::new(inline),
            layout0,
            group0,
            vs_inputs: Default::default(),
            ps_inputs: Default::default(),
            tex_layouts: Default::default(),
            pipeline_layouts: Default::default(),
            pipelines: Default::default(),
            tex_groups: Default::default(),
            last_group: Default::default(),
            group_key: Default::default(),
            target_views: Default::default(),
            pipeline_cache: None,
            warmup: None,
            warm_hits: 0,
            created_modules: 0,
            created_pipelines: 0,
        };
        if backend.encode_threads > 1 { backend.set_async_submit(true); }
        backend
    }

    /// Create the draws' pipelines through a pipeline cache kept in `dir`
    /// (loaded now, saved while pipelines are created and by
    /// [`save_pipeline_caches`]): later runs skip most of the driver's
    /// shader compilation. Whether the device has one (Vulkan, with
    /// `Features::PIPELINE_CACHE`). The pipelines are the same either way.
    pub fn enable_pipeline_cache(&mut self, dir: &std::path::Path) -> bool {
        self.pipeline_cache = pipelines::Persistent::open(&self.device, &self.adapter, dir);
        self.pipeline_cache.is_some()
    }

    /// Record observed recipes and precompile a previous run's pipelines on a
    /// background thread. The executable is read only, hashed off-thread.
    /// DDGPU_PIPELINE_WARMUP=0 records recipes without speculative compilation.
    pub fn enable_pipeline_warmup(&mut self, dir: &std::path::Path, exe: &std::path::Path) -> bool {
        let Some(cache) = self.pipeline_cache.clone() else { return false };
        self.warmup = warmup::Warmup::start(self.device.clone(), self.adapter.clone(), self.layout0.clone(), self.native_sim, cache, dir.to_owned(), exe.to_owned());
        self.warmup.is_some()
    }

    /// Draw shader modules and pipelines created on demand by this backend,
    /// excluding background warmup and fixed helper pipelines.
    pub fn pipeline_creation_counts(&self) -> (u64, u64) {
        (self.created_modules, self.created_pipelines)
    }

    /// Diagnostic snapshot: startup compilation finished, ready but unused
    /// pipelines, and unique warmed pipelines used by draws. Never waits.
    pub fn pipeline_warmup_status(&mut self) -> Option<(bool, usize, u64)> {
        self.poll_warmup();
        self.warmup.as_ref().map(|w| (w.finished(), w.ready_count(), self.warm_hits))
    }

    fn poll_warmup(&mut self) {
        if let Some(warmup) = self.warmup.as_mut() { warmup.drain(); }
    }

    /// Submit the recorded draws, after writing the ring bytes they use.
    fn flush(&self) {
        self.flush_async();
        self.wait_finisher();
    }

    /// Submit the open encoder; with async_submit its finish and submission
    /// run on the finisher thread (L124: wgpu-core encodes the recorded
    /// passes in finish, 61% of the backend worker in combo7) while this
    /// thread records the next batch. Only the draw-count flushes use it:
    /// every other flush waits (flush), and every direct queue write or
    /// submission waits first (wait_finisher), so the queue sees the same
    /// order as without it. The rings' staged bytes are written here, before
    /// the hand-off: they reach the GPU with this or an earlier submission,
    /// at offsets no earlier batch uses (a ring restarts only after flush).
    fn flush_async(&self) {
        self.end_pass();
        let mut encoders = std::mem::take(&mut *self.chunks.borrow_mut());
        if let Some(enc) = self.open.borrow_mut().take() { encoders.push(vk::Chunk::Wgpu(enc)); }
        self.uniforms.borrow_mut().upload(&self.queue);
        self.inline.borrow_mut().upload(&self.queue);
        if !encoders.is_empty() {
            match &self.finisher {
                Some(f) => {
                    f.submit_batch(encoders);
                    self.submissions.set(self.submissions.get() + 1);
                }
                None => {
                    self.wait_finisher();
                    let pool = encode::Pool::new(1, self.encode_profile.clone(), vk::finish);
                    let buffers = pool.finish(encoders);
                    let start = self.encode_profile.start();
                    vk::submit(&self.queue, buffers);
                    self.encode_profile.end(8, start);
                    self.submissions.set(self.submissions.get() + 1);
                }
            }
        }
        self.chunk_start.set(0);
        self.open_draws.set(0);
        self.open_uses.borrow_mut().clear();
    }

    /// Wait until the finisher has submitted everything handed to it.
    fn wait_finisher(&self) {
        if let Some(f) = &self.finisher {
            f.wait();
        }
    }

    /// L124: finish and submit draw batches on a second thread (ddplay turns
    /// it on). Multiple encoding workers require this coordinator even when
    /// `on` is false; select one worker to restore synchronous submissions.
    pub fn set_async_submit(&mut self, on: bool) {
        self.flush();
        self.finisher = (on || self.encode_threads > 1).then(||
            Finisher::new(self.queue.clone(), self.encode_threads, self.encode_profile.clone()));
    }

    /// Opt-in encoder workers (1..=4). Changes drain pending work first.
    /// Default 2 (encode::DEFAULT_THREADS); DDGPU_ENCODE_THREADS=1..4 overrides it at construction.
    /// Consecutive pass chunks are encoded independently and submitted in order.
    pub fn set_encode_threads(&mut self, threads: usize) {
        assert!((1..=4).contains(&threads));
        self.flush();
        self.encode_threads = threads;
        let async_submit = self.finisher.is_some();
        self.set_async_submit(async_submit);
    }

    /// Backend queue submissions, including explicit readbacks and queries.
    /// Present sinks submit independently and are not included.
    pub fn submission_count(&self) -> u64 {
        self.submissions.get()
    }

    fn submit_encoder(&self, enc: wgpu::CommandEncoder) {
        // L124: after everything the finisher holds
        self.wait_finisher();
        let start = self.encode_profile.start();
        let buffer = enc.finish();
        self.encode_profile.end(7, start);
        let start = self.encode_profile.start();
        self.queue.submit([buffer]);
        self.encode_profile.end(8, start);
        self.submissions.set(self.submissions.get() + 1);
    }

    /// End the open render pass: the encoder can record other commands
    /// (a copy) or be finished.
    fn end_pass(&self) {
        if let Some(pass) = self.vk_pass.borrow_mut().take() {
            // Any previous normal commands precede this raw pass. Later
            // copies/clears get a fresh core encoder, never the raw encoder.
            let mut chunks = self.chunks.borrow_mut();
            if let Some(enc) = self.open.borrow_mut().take() { chunks.push(vk::Chunk::Wgpu(enc)); }
            vk::Chunk::push_pass(&mut chunks, &self.device, pass,
                self.draws_per_submit.max(1).div_ceil(self.encode_threads as u32) as usize);
        }
        if let Some(pass) = self.pass.borrow_mut().take() {
            let start = self.encode_profile.start();
            drop(pass);
            self.encode_profile.end(1, start);
        }
        self.cpass.borrow_mut().take();
        // A boundary never adds/splits a render pass or changes its load/store
        // semantics. Track all chunks' resources until the whole batch submits.
        let chunk_draws = self.draws_per_submit.max(1).div_ceil(self.encode_threads as u32);
        if self.encode_threads > 1 && self.open_draws.get() - self.chunk_start.get() >= chunk_draws {
            if let Some(enc) = self.open.borrow_mut().take() {
                self.chunks.borrow_mut().push(vk::Chunk::Wgpu(enc));
                self.chunk_start.set(self.open_draws.get());
            }
        }
    }

    /// Submit what is recorded and wait until the GPU has run it (a replay
    /// without Presents frees finished submissions this way).
    pub fn wait_idle(&self) {
        self.flush();
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Draws run as compute dispatches so far (DDGPU_NATIVE_SIM, L113).
    pub fn quad_dispatches(&self) -> u64 {
        self.quads.dispatches
    }

    /// Take the shared encoder after ending any live draw pass. Copy helpers
    /// put it back rather than finishing/submitting it; wgpu retains all
    /// referenced textures, views, bind groups and temporary buffers.
    fn copy_encoder(&self) -> wgpu::CommandEncoder {
        self.end_pass();
        self.open.borrow_mut().take().unwrap_or_else(|| self.device.create_command_encoder(&Default::default()))
    }

    fn track_copy_targets(&self, targets: &[&Target]) {
        self.open_uses.borrow_mut().extend(targets.iter().filter_map(|t| t.texture.map(|x| x.resource)));
    }

    fn copy_recorded(&self) {
        // Keep the existing bounded batch policy, including copy-only streams.
        self.open_draws.set(self.open_draws.get() + 1);
        if self.open_draws.get() >= self.draws_per_submit {
            self.flush_async();
        }
    }

    /// Draws recorded and render passes begun for them so far.
    pub fn pass_counts(&self) -> (u64, u64) {
        (self.draws_recorded.get(), self.passes_begun.get())
    }

    /// Room in a ring for `bytes` (`len` reserved), flushing and starting
    /// the ring again when it is full: the offset.
    fn ring_alloc(&self, ring: &RefCell<Ring>, bytes: &[u8], len: u64, align: u64) -> u64 {
        self.ring_fill(ring, len, align, |dst| dst[..bytes.len()].copy_from_slice(bytes))
    }

    /// As [`ring_alloc`](Self::ring_alloc), the room filled by `fill`.
    fn ring_fill(&self, ring: &RefCell<Ring>, len: u64, align: u64, fill: impl FnOnce(&mut [u8])) -> u64 {
        if !ring.borrow().fits(len, align) {
            self.flush();
            ring.borrow_mut().restart();
        }
        let mut r = ring.borrow_mut();
        let (off, dst) = r.alloc(len, align).expect("ring room after a restart");
        fill(dst);
        off
    }

    /// Per-draw vertex or index data: in the inline ring, or a buffer of its
    /// own when it is too large for it.
    fn inline_data(&self, bytes: &[u8], usage: wgpu::BufferUsages) -> (wgpu::Buffer, u64, u64) {
        let len = (bytes.len() as u64).next_multiple_of(4).max(4);
        if len > INLINE_RING / 4 {
            return (self.inline_buffer(bytes, usage), 0, len);
        }
        let off = self.ring_alloc(&self.inline, bytes, len, 16);
        (self.inline.borrow().buffer.clone(), off, len)
    }

    /// A constant block in the uniform ring (same bytes as a draw's own
    /// uniform buffer had): its dynamic offset.
    /// L78: packed straight into the ring (it was built 4 bytes at a time
    /// into a new Vec, then copied), and a stage's block whose constants
    /// equal the previous draw's, still in the ring, is not written again:
    /// the draw uses its offset.
    fn constants(&self, c: &std::sync::Arc<d3d9cmd::Constants>, floats: usize, extra: &[[u32; 4]], block: u64, stage: usize) -> u32 {
        let len = 16 * (floats + 16 + 16 + extra.len());
        assert!(len as u64 <= block, "constant block of {len} bytes");
        let generation = self.uniforms.borrow().generation;
        let mut last = self.last_constants.borrow_mut();
        let last = &mut last[stage];
        let prefix = |v: &d3d9cmd::Constants| v.f.len().min(floats);
        if last.3 == generation && last.4 == floats &&
            (std::sync::Arc::ptr_eq(&last.0, c) ||
             (last.0.i == c.i && last.0.b == c.b && last.0.f[..prefix(&last.0)] == c.f[..prefix(c)])) && last.1[..] == *extra {
            return last.2;
        }
        // Bindings still expose the fixed maximum block size. Only the
        // compact shader layout's bytes are staged/uploaded; leave enough
        // buffer space after this offset for the binding descriptor.
        if !self.uniforms.borrow().fits(block, UNIFORM_ALIGN) {
            self.flush();
            self.uniforms.borrow_mut().restart();
        }
        let off = self.ring_fill(&self.uniforms, len as u64, UNIFORM_ALIGN, |dst| {
            backend_data::pack_constants(dst, c, floats, extra);
        }) as u32;
        last.0.clone_from(c);
        last.1.clear();
        last.1.extend_from_slice(extra);
        last.2 = off;
        last.3 = self.uniforms.borrow().generation;
        last.4 = floats;
        off
    }

    /// target_view, cached (release() drops the cache).
    fn cached_target_view(&mut self, t: &Target, srgb: bool) -> Option<(wgpu::TextureView, wgpu::TextureFormat)> {
        let key = (t.surface, t.texture.map(|x| x.resource), t.level, t.format, srgb);
        if let Some(v) = self.target_views.get(&key) {
            return v.clone();
        }
        let v = self.target_view(t, srgb);
        // Only persistent draw attachment views belong in the Vulkan cache.
        // Clear/copy helper views are transient and never use the raw recorder.
        if let (Some(vk),Some((view,format))) = (&mut self.vk,&v) {
            let (face,level) = t.texture.and_then(|tt| self.textures.get(&tt.resource))
                .map(|tx| if tx.kind == TexKind::Cube { (t.level/tx.levels,t.level%tx.levels) } else { (0,t.level) })
                .unwrap_or((0,0));
            let desc=wgpu::TextureViewDescriptor {
                format:Some(*format), dimension:Some(wgpu::TextureViewDimension::D2),
                base_mip_level:level,mip_level_count:Some(1),base_array_layer:face,array_layer_count:Some(1),
                ..Default::default()
            };
            let usage=if format.is_depth_stencil_format() {wgpu::TextureUses::DEPTH_STENCIL_WRITE} else {wgpu::TextureUses::COLOR_TARGET};
            if let Err(e)=vk.view(view,&desc,usage) {eprintln!("DDGPU_VK target fallback: {e}");}
        }
        self.target_views.insert(key, v.clone());
        v
    }

    /// A free occlusion slot (a new query set when none is left).
    fn occlusion_slot(&mut self) -> u32 {
        if self.free_slots.is_empty() {
            let set = self.occlusion_sets.len() as u32;
            self.occlusion_sets.push(self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("occlusion"), ty: wgpu::QueryType::Occlusion, count: OCCLUSION_SET,
            }));
            self.free_slots.extend((0..OCCLUSION_SET).rev().map(|k| set * OCCLUSION_SET + k));
        }
        self.free_slots.pop().unwrap()
    }

    /// Forget a guest query's slots and count.
    fn drop_query(&mut self, query: u32) {
        for s in self.query_slots.remove(&query).unwrap_or_default() {
            let users = self.slot_users.entry(s).or_insert(1);
            *users -= 1;
            if *users == 0 {
                self.slot_users.remove(&s);
                self.free_slots.push(s);
            }
        }
        self.query_counts.remove(&query);
        self.pending_queries.remove(&query);
    }

    /// The samples the draws in `slots` passed, from the GPU.
    /// Start resolving `slots` into a mappable buffer without waiting
    /// (L120): the state turns 1 when the counts can be read, 2 on failure.
    fn start_resolve(&self, slots: &[u32]) -> (wgpu::Buffer, std::sync::Arc<std::sync::atomic::AtomicU8>) {
        self.flush();
        let size = 256 * slots.len() as u64;
        let resolved = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        for (k, &s) in slots.iter().enumerate() {
            let i = s % OCCLUSION_SET;
            enc.resolve_query_set(&self.occlusion_sets[(s / OCCLUSION_SET) as usize], i..i + 1, &resolved, 256 * k as u64);
        }
        enc.copy_buffer_to_buffer(&resolved, 0, &read, 0, size);
        self.submit_encoder(enc);
        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let done = state.clone();
        read.slice(..).map_async(wgpu::MapMode::Read, move |r| done.store(if r.is_ok() { 1 } else { 2 }, std::sync::atomic::Ordering::Release));
        (read, state)
    }

    /// Keep a query's count; its slots are free for later draws.
    fn store_count(&mut self, query: u32, n: u64) -> u64 {
        self.drop_query(query);
        self.query_slots.insert(query, Vec::new());
        self.query_counts.insert(query, n);
        n
    }

    fn resolve(&self, slots: &[u32]) -> Option<u64> {
        self.flush();
        if slots.is_empty() {
            return Some(0);
        }
        // resolve destinations are 256-byte aligned: one slot per 256 bytes
        let size = 256 * slots.len() as u64;
        let resolved = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        for (k, &s) in slots.iter().enumerate() {
            let i = s % OCCLUSION_SET;
            enc.resolve_query_set(&self.occlusion_sets[(s / OCCLUSION_SET) as usize], i..i + 1, &resolved, 256 * k as u64);
        }
        enc.copy_buffer_to_buffer(&resolved, 0, &read, 0, size);
        self.submit_encoder(enc);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = read.slice(..).get_mapped_range();
        Some((0..slots.len()).map(|k| u64::from_le_bytes(data[256 * k..256 * k + 8].try_into().unwrap())).sum())
    }

    fn texture_for(&mut self, b: &TextureBinding) -> Option<&Tex> {
        let desc = TargetTexture { resource: b.resource, kind: b.kind, width: b.width, height: b.height, depth: b.depth, levels: b.levels, usage: b.usage };
        self.create_texture(&desc, b.format)?;
        self.textures.get(&b.resource)
    }

    /// Create a texture (once): a sampled texture in its sampled format, or
    /// a render-target or depth-stencil texture (D3DUSAGE_RENDERTARGET,
    /// D3DUSAGE_DEPTHSTENCIL) in its target format, a render attachment
    /// from creation whether it is first drawn into or first bound.
    fn create_texture(&mut self, t: &TargetTexture, d3d_format: u32) -> Option<()> {
        if self.textures.contains_key(&t.resource) {
            return Some(());
        }
        let target = t.usage & (USAGE_RENDERTARGET | USAGE_DEPTHSTENCIL) != 0;
        let (format, block, usage) = if target {
            let format = target_format(d3d_format, false)?;
            // L113: float targets the quad dispatches write (compute_quads)
            let storage = if self.native_sim && format == wgpu::TextureFormat::Rgba32Float { wgpu::TextureUsages::STORAGE_BINDING } else { wgpu::TextureUsages::empty() };
            (format, 1,
             wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | storage)
        } else {
            let (format, _, _, block) = sampled_format(d3d_format)?;
            (format, block, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC)
        };
        let (layers, dimension) = match t.kind {
            TexKind::Flat => (1, wgpu::TextureDimension::D2),
            TexKind::Cube => (6, wgpu::TextureDimension::D2),
            TexKind::Volume => (t.depth, wgpu::TextureDimension::D3),
        };
        let size = wgpu::Extent3d {
            width: t.width.next_multiple_of(block),
            height: t.height.next_multiple_of(block),
            depth_or_array_layers: layers,
        };
        // D3DSAMP_SRGBTEXTURE reads, and SRGBWRITEENABLE writes, through the
        // sRGB view
        let srgb = format.add_srgb_suffix();
        let views: &[wgpu::TextureFormat] = if srgb != format { &[srgb] } else { &[] };
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: t.levels,
            sample_count: 1,
            dimension,
            format,
            usage,
            view_formats: views,
        });
        if block_on(scope.pop()).is_some() {
            return None;
        }
        self.textures.insert(t.resource, Tex {
            texture, kind: t.kind, format: d3d_format, gpu_format: format, levels: t.levels,
            width: t.width, height: t.height, depth: t.depth, target,
        });
        for u in self.pending.remove(&t.resource).unwrap_or_default() {
            self.write_image(&u);
        }
        Some(())
    }

    /// Write an image upload into its (existing) texture.
    fn write_image(&mut self, u: &Upload) {
        let Upload::Image { resource, index, format, width, height, depth, pitch, slice: _, bytes } = u else { return };
        let Some(t) = self.textures.get(resource) else { return };
        let Some((sampled, conv, bpp, block)) = sampled_format(*format) else { return };
        if t.target && (conv != Conversion::Direct || sampled != t.gpu_format) {
            // a render-target texture is stored in its target format
            self.errors.push(format!("an image upload into render-target texture {resource:#x} of format {format} is not implemented"));
            return;
        }
        let (face, level) = match t.kind {
            TexKind::Cube => (index / t.levels, index % t.levels),
            _ => (0, *index),
        };
        let rows = height.div_ceil(block);
        let blocks_w = width.div_ceil(block);
        let (gpu_row, src_bpp) = match conv {
            Conversion::Direct => (blocks_w * bpp, bpp),
            _ => {
                let out_bpp = t.gpu_format.block_copy_size(None).unwrap_or(4);
                (width * out_bpp, bpp)
            }
        };
        let mut data = Vec::new();
        for z in 0..*depth {
            let slice = &bytes[(z * pitch * rows) as usize..];
            if conv == Conversion::Direct {
                for y in 0..rows {
                    let s = (y * pitch) as usize;
                    data.extend_from_slice(&slice[s..s + (blocks_w * bpp) as usize]);
                }
            } else {
                data.extend(convert(conv, slice, *width, rows, *pitch, src_bpp));
            }
        }
        let physical = wgpu::Extent3d { width: width.next_multiple_of(block), height: height.next_multiple_of(block), depth_or_array_layers: *depth };
        self.wait_finisher();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &t.texture,
                mip_level: level,
                origin: wgpu::Origin3d { x: 0, y: 0, z: if t.kind == TexKind::Cube { face } else { 0 } },
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(gpu_row), rows_per_image: Some(rows) },
            physical,
        );
    }

    /// The texture, mip level, array layer, format and size behind a target.
    fn target_texture(&mut self, t: &Target) -> Option<(wgpu::Texture, u32, u32, wgpu::TextureFormat, u32, u32)> {
        if let Some(tt) = t.texture {
            self.create_texture(&tt, t.format)?;
            let tx = self.textures.get(&tt.resource)?;
            let (face, level) = match tx.kind {
                TexKind::Cube => (t.level / tx.levels, t.level % tx.levels),
                _ => (0, t.level),
            };
            return Some((tx.texture.clone(), level, face, tx.gpu_format, t.width, t.height));
        }
        self.target_view(t, false)?;
        let rt = self.surfaces.get(&t.surface)?;
        Some((rt.texture.clone(), 0, 0, rt.format, rt.width, rt.height))
    }

    /// The pipeline that clears part of a target: a triangle over the
    /// scissored region writing each colour slot's value, the depth and the
    /// stencil reference.
    fn clear_pipeline(&mut self, key: &ClearKey) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        if let Some(p) = self.clears.get(key) {
            return p.clone();
        }
        let (colours, depth, write_depth, write_stencil, samples) = key;
        let mut wgsl = String::from("struct P { c: array<vec4<f32>, 4>, z: vec4<f32> }\n@group(0) @binding(0) var<uniform> p: P;\n");
        wgsl.push_str("@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {\n  let c = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));\n  return vec4<f32>(c * vec2<f32>(4.0, -4.0) + vec2<f32>(-1.0, 1.0), p.z.x, 1.0);\n}\n");
        let slots: Vec<usize> = colours.iter().enumerate().filter(|(_, f)| f.is_some()).map(|(k, _)| k).collect();
        if !slots.is_empty() {
            wgsl.push_str("struct O {");
            for k in &slots {
                wgsl.push_str(&format!(" @location({k}) o{k}: vec4<f32>,"));
            }
            wgsl.push_str(" }\n@fragment fn fs() -> O { return O(");
            wgsl.push_str(&slots.iter().map(|k| format!("p.c[{k}]")).collect::<Vec<_>>().join(", "));
            wgsl.push_str("); }\n");
        }
        let module = crate::pipeline_log::timed("module", "object-miss", || self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("clear"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) }));
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let pl = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let targets: Vec<Option<wgpu::ColorTargetState>> = colours.iter().map(|f| f.map(|format| wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })).collect();
        let replace = wgpu::StencilFaceState { compare: wgpu::CompareFunction::Always, fail_op: wgpu::StencilOperation::Replace, depth_fail_op: wgpu::StencilOperation::Replace, pass_op: wgpu::StencilOperation::Replace };
        let pipeline = crate::pipeline_log::timed("pipeline", "object-miss", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("clear"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(*write_depth),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: if *write_stencil {
                    wgpu::StencilState { front: replace, back: replace, read_mask: 0xFF, write_mask: 0xFF }
                } else {
                    wgpu::StencilState::default()
                },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: *samples, ..Default::default() },
            fragment: (!slots.is_empty()).then(|| wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &targets }),
            multiview_mask: None,
            cache: None,
        }));
        self.clears.insert(key.clone(), (pipeline.clone(), layout.clone()));
        (pipeline, layout)
    }

    /// A clear of `regions` (not the whole target): the clear pipeline,
    /// scissored to each region.
    fn clear_regions_with_pipeline(&mut self, c: &ClearCall, regions: &[[i32; 4]]) {
        let colour_views: Vec<Option<(wgpu::TextureView, wgpu::TextureFormat)>> = if c.flags & 1 != 0 {
            c.targets.iter().map(|t| t.as_ref().and_then(|t| self.target_view(t, c.srgb))).collect()
        } else {
            vec![None; 4]
        };
        let depth = if c.flags & 6 != 0 { c.depth.as_ref().and_then(|t| self.target_view(t, false)) } else { None };
        let write_stencil = c.flags & 4 != 0 && depth.as_ref().is_some_and(|(_, f)| f.has_stencil_aspect());
        let max_out = colour_views.iter().rposition(|v| v.is_some()).map(|p| p + 1).unwrap_or(0);
        let samples = c.targets.iter().flatten().chain(c.depth.iter()).next().map(sample_count).transpose().unwrap_or(None).unwrap_or(1);
        let key: ClearKey = (colour_views[..max_out].iter().map(|v| v.as_ref().map(|(_, f)| *f)).collect(), depth.as_ref().map(|(_, f)| *f), c.flags & 2 != 0, write_stencil, samples);
        let (pipeline, layout) = self.clear_pipeline(&key);
        let mut p: Vec<u8> = Vec::new();
        for k in 0..4 {
            let col = colour_views.get(k).and_then(|v| v.as_ref()).map(|(_, f)| clear_colour(c.colour, *f)).unwrap_or(wgpu::Color::TRANSPARENT);
            for v in [col.r, col.g, col.b, col.a] {
                p.extend_from_slice(&(v as f32).to_le_bytes());
            }
        }
        for v in [c.z, 0.0, 0.0, 0.0] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        let ubuf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: p.len() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        self.wait_finisher();
        self.queue.write_buffer(&ubuf, 0, &p);
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }] });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = colour_views[..max_out].iter().map(|v| v.as_ref().map(|(view, _)| wgpu::RenderPassColorAttachment {
                view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })).collect();
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &attachments,
                depth_stencil_attachment: depth.as_ref().map(|(view, f)| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: f.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_stencil_reference(c.stencil);
            for r in regions {
                pass.set_scissor_rect(r[0] as u32, r[1] as u32, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32);
                pass.draw(0..3, 0..1);
            }
        }
        self.submit_encoder(enc);
    }

    /// A multisampled colour texture resolved into a new single-sampled one
    /// (a render pass that loads, stores and resolves; the multisampled
    /// contents stay).
    fn resolve_texture(&mut self, src: &wgpu::Texture, format: wgpu::TextureFormat, w: u32, h: u32) -> Result<wgpu::Texture, String> {
        if format.is_depth_stencil_format() {
            return Err("resolving a multisampled depth-stencil surface is not implemented".into());
        }
        let dst = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("resolve"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[format.add_srgb_suffix()],
        });
        // the D3D9 driver resolves 8-bit targets gamma-correctly: the
        // samples averaged as linear light and encoded back to sRGB
        // (measured: a quarter-covered white pixel resolves to 137, the sRGB
        // encoding of 0.25; L31 tests/msaa.rs). Resolving through sRGB views
        // does that; float targets resolve linearly, as the driver does
        let gamma = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm);
        let view_format = Some(if gamma { format.add_srgb_suffix() } else { format });
        let sview = src.create_view(&wgpu::TextureViewDescriptor { format: view_format, ..Default::default() });
        let dview = dst.create_view(&wgpu::TextureViewDescriptor { format: view_format, ..Default::default() });
        let mut enc = self.copy_encoder();
        {
            let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sview, depth_slice: None, resolve_target: Some(&dview),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
        }
        *self.open.borrow_mut() = Some(enc);
        Ok(dst)
    }

    /// StretchRect between depth-stencil surfaces: single-sample copies
    /// transfer all aspects directly. Multisample copies write frag_depth,
    /// then draw once per stencil bit (wgpu has no stencil export). Equal
    /// sample counts copy every sample; to one sample, use the sample
    /// `depth_copy_sample` names. All commands stay in the open encoder.
    fn copy_depth(&mut self, src: &Target, dst: &Target) -> Result<(), String> {
        if !(is_depth_format(src.format) && is_depth_format(dst.format)) {
            return Err("between a depth-stencil and a colour surface".into());
        }
        if (src.width, src.height) != (dst.width, dst.height) || src.format != dst.format {
            return Err("surfaces of different sizes or formats".into());
        }
        let (stex, slevel, slayer, sformat, sw, sh) = self.target_texture(src).ok_or("unknown source")?;
        let (dtex, dlevel, dlayer, dformat, _, _) = self.target_texture(dst).ok_or("unknown destination")?;
        let (ssamples, dsamples) = (stex.sample_count(), dtex.sample_count());
        // wgpu texture-to-texture copies allow equal depth/stencil formats
        // when the whole subresource and all aspects are copied. The backend
        // gives single-sample targets COPY_SRC/DST usage; multisample targets
        // retain the sample-selection shader below. This preserves both
        // depth bits and stencil without constructing nine draw pipelines.
        if ssamples == 1 && dsamples == 1 && sformat == dformat {
            let mut enc = self.copy_encoder();
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &stex, mip_level: slevel, origin: wgpu::Origin3d { x: 0, y: 0, z: slayer }, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &dtex, mip_level: dlevel, origin: wgpu::Origin3d { x: 0, y: 0, z: dlayer }, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 },
            );
            *self.open.borrow_mut() = Some(enc);
            return Ok(());
        }
        let stencil = sformat.has_stencil_aspect() && dformat.has_stencil_aspect();
        let ms = ssamples > 1;
        let depth_ty = if ms { "texture_depth_multisampled_2d" } else { "texture_depth_2d" };
        let stencil_ty = if ms { "texture_multisampled_2d<u32>" } else { "texture_2d<u32>" };
        // equal sample counts copy sample by sample (per-sample shading);
        // to one sample, the driver's sample for the count
        let level = if !ms {
            "0".to_string()
        } else if dsamples == ssamples {
            "i32(si)".to_string()
        } else if dsamples == 1 {
            format!("{}", depth_copy_sample(ssamples).ok_or_else(|| format!("from {ssamples} samples"))?)
        } else {
            return Err(format!("from {ssamples} to {dsamples} samples"));
        };
        let si = if ms && dsamples == ssamples { ", @builtin(sample_index) si: u32" } else { "" };
        let wgsl = format!(
            "@group(0) @binding(0) var d: {depth_ty};\n@group(0) @binding(1) var st: {stencil_ty};\n\
             struct P {{ bit: vec4<u32> }}\n@group(0) @binding(2) var<uniform> p: P;\n\
             @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{\n\
               let c = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));\n\
               return vec4<f32>(c * vec2<f32>(4.0, -4.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);\n}}\n\
             @fragment fn depth_fs(@builtin(position) pos: vec4<f32>{si}) -> @builtin(frag_depth) f32 {{\n\
               return textureLoad(d, vec2<i32>(pos.xy), {level});\n}}\n\
             @fragment fn stencil_fs(@builtin(position) pos: vec4<f32>{si}) {{\n\
               let v = textureLoad(st, vec2<i32>(pos.xy), {level}).r;\n\
               if ((v & p.bit.x) == 0u) {{ discard; }}\n}}\n");
        let module = crate::pipeline_log::timed("module", "object-miss", || self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("depth copy"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) }));
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: ms }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: ms }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let pl = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let face = |op| wgpu::StencilFaceState { compare: wgpu::CompareFunction::Always, fail_op: op, depth_fail_op: op, pass_op: op };
        let pipeline = |entry: &str, write_depth: bool, write_mask: u32| crate::pipeline_log::timed("pipeline", "object-miss", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth copy"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: dformat,
                depth_write_enabled: Some(write_depth),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: if stencil {
                    wgpu::StencilState { front: face(wgpu::StencilOperation::Replace), back: face(wgpu::StencilOperation::Replace), read_mask: 0xFF, write_mask }
                } else {
                    wgpu::StencilState::default()
                },
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: dsamples, ..Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), targets: &[] }),
            multiview_mask: None,
            cache: None,
        }));
        let depth_pipe = pipeline("depth_fs", true, 0xFF);
        let bit_pipes: Vec<wgpu::RenderPipeline> = if stencil { (0..8).map(|b| pipeline("stencil_fs", false, 1 << b)).collect() } else { vec![] };
        let dview = stex.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let sview = if sformat.has_stencil_aspect() {
            stex.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::StencilOnly, ..Default::default() })
        } else {
            return Err("a depth format without stencil is not implemented".into());
        };
        let groups: Vec<wgpu::BindGroup> = (0..9u32).map(|k| {
            let bit: u32 = if k == 0 { 0 } else { 1 << (k - 1) };
            let mut bytes = Vec::new();
            for v in [bit, 0, 0, 0] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.wait_finisher();
            self.queue.write_buffer(&buf, 0, &bytes);
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None, layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&dview) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&sview) },
                    wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() },
                ],
            })
        }).collect();
        let target = dtex.create_view(&Default::default());
        let mut enc = self.copy_encoder();
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("depth copy"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: dformat.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            pass.set_pipeline(&depth_pipe);
            pass.set_bind_group(0, &groups[0], &[]);
            pass.set_stencil_reference(0);
            pass.draw(0..3, 0..1);
            pass.set_stencil_reference(0xFF);
            for (b, p) in bit_pipes.iter().enumerate() {
                pass.set_pipeline(p);
                pass.set_bind_group(0, &groups[b + 1], &[]);
                pass.draw(0..3, 0..1);
            }
        }
        *self.open.borrow_mut() = Some(enc);
        Ok(())
    }

    fn blit_pipeline(&mut self, format: wgpu::TextureFormat, samples: u32) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        if let Some(p) = self.blits.get(&(format, samples)) {
            return p.clone();
        }
        let module = crate::pipeline_log::timed("module", "object-miss", || self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT.into()) }));
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = crate::pipeline_log::timed("pipeline", "object-miss", || self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: None,
        }));
        self.blits.insert((format, samples), (pipeline.clone(), layout.clone()));
        (pipeline, layout)
    }

    /// Draw the blit quad into `rect` of a target (sampling `src` or filling
    /// with `colour`).
    #[allow(clippy::too_many_arguments)]
    fn blit(&mut self, dst: &Target, rect: [i32; 4], src: Option<(&wgpu::TextureView, [f32; 4], bool)>, colour: [f32; 4]) -> Result<(), String> {
        let (dtex, level, layer, format, w, h) = self.target_texture(dst).ok_or("unknown destination")?;
        self.track_copy_targets(&[dst]);
        let (pipeline, layout) = self.blit_pipeline(format, sample_count(dst)?);
        let view = dtex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2), base_mip_level: level, mip_level_count: Some(1),
            base_array_layer: layer, array_layer_count: Some(1), ..Default::default()
        });
        let (src_rect, linear) = src.map(|(_, r, l)| (r, l)).unwrap_or(([0.0, 0.0, 1.0, 1.0], false));
        let mut p: Vec<u8> = Vec::new();
        for f in src_rect.iter().chain(colour.iter()) {
            p.extend_from_slice(&f.to_le_bytes());
        }
        for u in [if src.is_some() { 0u32 } else { 1 }, 0, 0, 0] {
            p.extend_from_slice(&u.to_le_bytes());
        }
        let ubuf = self.inline_buffer(&p, wgpu::BufferUsages::UNIFORM);
        let dummy;
        let sview = match src {
            Some((v, _, _)) => v,
            None => {
                dummy = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: None, size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
                    dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
                }).create_view(&Default::default());
                &dummy
            }
        };
        let filter = if linear { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: filter, min_filter: filter, ..Default::default() });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None, layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(sview) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let (l, t) = (rect[0].clamp(0, w as i32) as u32, rect[1].clamp(0, h as i32) as u32);
        let (r, b) = (rect[2].clamp(0, w as i32) as u32, rect[3].clamp(0, h as i32) as u32);
        if r <= l || b <= t {
            return Ok(());
        }
        let mut enc = self.copy_encoder();
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_viewport(l as f32, t as f32, (r - l) as f32, (b - t) as f32, 0.0, 1.0);
            pass.draw(0..3, 0..1);
        }
        *self.open.borrow_mut() = Some(enc);
        Ok(())
    }

    /// The texture a target renders into: a standalone surface's own, or a
    /// render-target texture's level.
    fn target_view(&mut self, t: &Target, srgb: bool) -> Option<(wgpu::TextureView, wgpu::TextureFormat)> {
        if let Some(tt) = t.texture {
            // a level of a render-target texture, created here when it is
            // drawn into before it is bound
            self.create_texture(&tt, t.format)?;
            let tx = self.textures.get(&tt.resource)?;
            if !tx.target {
                return None;
            }
            let (face, level) = match tx.kind {
                TexKind::Cube => (t.level / tx.levels, t.level % tx.levels),
                _ => (0, t.level),
            };
            let format = if srgb { tx.gpu_format.add_srgb_suffix() } else { tx.gpu_format };
            let desc = wgpu::TextureViewDescriptor {
                format: Some(format),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_mip_level: level,
                mip_level_count: Some(1),
                base_array_layer: face,
                array_layer_count: Some(1),
                ..Default::default()
            };
            let view = tx.texture.create_view(&desc);
            return Some((view, format));
        }
        let format = target_format(t.format, false)?;
        if !self.surfaces.contains_key(&t.surface) {
            let srgb_view = match format {
                wgpu::TextureFormat::Bgra8Unorm => vec![wgpu::TextureFormat::Bgra8UnormSrgb],
                wgpu::TextureFormat::Rgba8Unorm => vec![wgpu::TextureFormat::Rgba8UnormSrgb],
                _ => vec![],
            };
            // a format the adapter cannot render to (or not at this sample
            // count) is refused (None); multisampled textures cannot be
            // copied in WebGPU, they are resolved (StretchRect)
            let samples = sample_count(t).ok()?;
            let usage = if samples > 1 {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING
            } else {
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING
            };
            let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &srgb_view,
            });
            if block_on(scope.pop()).is_some() {
                return None;
            }
            self.surfaces.insert(t.surface, Rt { texture, format, width: t.width, height: t.height });
        }
        let rt = self.surfaces.get(&t.surface)?;
        let view_format = if srgb { target_format(t.format, true).unwrap_or(rt.format) } else { rt.format };
        let desc = wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() };
        let view = rt.texture.create_view(&desc);
        Some((view, view_format))
    }

    /// A wgpu sampler for D3DSAMPLERSTATETYPE values `states` (1..13).
    /// D3D9 behaviour wgpu samplers cannot express is an error:
    /// D3DTADDRESS_MIRRORONCE, border colours other than transparent
    /// black, opaque black and opaque white. MIPMAPLODBIAS is applied in
    /// the pixel shader (d3d9wgsl's lod_bias).
    fn sampler(&mut self, states: &[u32; 14]) -> Result<wgpu::Sampler, String> {
        if let Some(s) = self.samplers.get(states) {
            return Ok(s.clone());
        }
        let border = states[1..4].contains(&4);
        if border && !self.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER) {
            return Err("D3DTADDRESS_BORDER: the adapter has no clamp-to-border".into());
        }
        if states[1..4].contains(&5) {
            return Err("D3DTADDRESS_MIRRORONCE is not implemented (wgpu has no mirror-clamp-to-edge)".into());
        }
        let address = |v: u32| match v {
            2 => wgpu::AddressMode::MirrorRepeat,
            3 => wgpu::AddressMode::ClampToEdge,
            4 => wgpu::AddressMode::ClampToBorder,
            _ => wgpu::AddressMode::Repeat,
        };
        let border_color = match (border, states[4]) {
            (false, _) => None,
            (true, 0) => Some(wgpu::SamplerBorderColor::TransparentBlack),
            (true, 0xFF00_0000) => Some(wgpu::SamplerBorderColor::OpaqueBlack),
            (true, 0xFFFF_FFFF) => Some(wgpu::SamplerBorderColor::OpaqueWhite),
            (true, c) => return Err(format!("border colour {c:#010x} (wgpu has transparent black, opaque black and opaque white)")),
        };
        // D3DTEXF_ANISOTROPIC (3) minification filters anisotropically up to
        // MAXANISOTROPY; wgpu needs every filter linear for anisotropy
        // (not with MIPFILTER NONE: wgpu would blend in level 1, see below)
        let anisotropic = states[6] == 3 && states[10] > 1 && states[7] != 0;
        let filter = |v: u32| if v >= 2 || anisotropic { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        let mip = |v: u32| if v >= 2 || anisotropic { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest };
        let desc = wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: address(states[1]),
            address_mode_v: address(states[2]),
            address_mode_w: address(states[3]),
            // with MIPFILTER NONE D3D9 still clamps the level of detail to
            // MAXMIPLEVEL before choosing MAGFILTER or MINFILTER (measured:
            // explore_d3d9_semantics), so MAXMIPLEVEL > 0 always minifies
            mag_filter: filter(if states[7] == 0 && states[9] > 0 { states[6] } else { states[5] }),
            min_filter: filter(states[6]),
            mipmap_filter: mip(states[7]),
            // D3DSAMP_MIPFILTER NONE samples level 0, whatever MAXMIPLEVEL
            // says (measured: explore_d3d9_semantics), choosing MAGFILTER or
            // MINFILTER by the unclamped level of detail. Vulkan chooses by
            // the clamped one, so the clamp is [0, 1/4]: a minified sample
            // still picks MINFILTER and the nearest level is 0. Otherwise
            // MAXMIPLEVEL is the most detailed level used.
            lod_min_clamp: if states[7] == 0 { 0.0 } else { states[9] as f32 },
            lod_max_clamp: if states[7] == 0 { 0.25 } else { 32.0 },
            compare: None,
            anisotropy_clamp: if anisotropic { states[10].min(16) as u16 } else { 1 },
            border_color,
        };
        let s = self.device.create_sampler(&desc);
        if let Some(vk) = &mut self.vk {
            if let Err(e) = vk.sampler(&s, &desc) { eprintln!("DDGPU_VK sampler fallback: {e}"); }
        }
        self.samplers.insert(*states, s.clone());
        Ok(s)
    }

    /// A translated shader, from the cache or translated now; `options` is
    /// built only for a translation (L94: a cache hit allocates nothing and
    /// shares the interface).
    fn module(&mut self, tokens: &[u32], options: impl FnOnce(&Self) -> d3d9wgsl::Options, key: ModuleKey) -> Result<(wgpu::ShaderModule, std::sync::Arc<d3d9wgsl::Interface>), String> {
        if let Some((m, i)) = self.modules.get(&key) {
            return Ok((m.clone(), i.clone()));
        }
        let mut options = options(self);
        let started = std::time::Instant::now();
        options.compact_constants = true;
        let shader = d3d9shader::parse(tokens).map_err(|e| e.to_string())?;
        let t = pipeline_log::timed("translation", "miss", || d3d9wgsl::translate_with(&shader, options)).map_err(|e| e.to_string())?;
        pipelines::translated(started);
        // Resolve parse/cache errors before opening the device error scope.
        // wgpu still validates the exact cached IR against this device.
        let warmed = self.warmup.as_mut().and_then(|w| w.source(key, &t.wgsl));
        if let Some(vk) = &mut self.vk {
            if let Err(e) = vk.shader(key, &t.wgsl) { eprintln!("DDGPU_VK shader fallback: {e}"); }
        }
        let module = match warmed {
            Some(module) => pipeline_log::timed("module-reuse", "warm-hit", || module),
            None => {
                let source = shader_cache::source(t.wgsl)?;
                let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
                let module = pipeline_log::timed("module", "object-miss", || self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source }));
                if let Some(e) = block_on(scope.pop()) { return Err(format!("{e}")); }
                pipelines::module_created(started);
                self.created_modules += 1;
                module
            }
        };
        let interface = std::sync::Arc::new(t.interface);
        self.modules.insert(key, (module.clone(), interface.clone()));
        Ok((module, interface))
    }

    fn buffer(&mut self, resource: u32) -> Option<wgpu::Buffer> {
        self.buffers.get(&resource).cloned()
    }

    fn inline_buffer(&self, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let size = (bytes.len() as u64).next_multiple_of(4).max(4);
        let b = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut padded = bytes.to_vec();
        padded.resize(size as usize, 0);
        self.wait_finisher();
        self.queue.write_buffer(&b, 0, &padded);
        b
    }

    /// Read a standalone target back as RGBA8 (BGRA formats swizzled).
    pub fn read_surface(&self, surface: u32) -> Option<Frame> {
        self.flush();
        let rt = self.surfaces.get(&surface)?;
        let bpp = rt.format.block_copy_size(None)?;
        if bpp != 4 {
            return None;
        }
        let row = (rt.width * 4).next_multiple_of(256);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None, size: (row * rt.height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &rt.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rt.height) } },
            wgpu::Extent3d { width: rt.width, height: rt.height, depth_or_array_layers: 1 },
        );
        self.submit_encoder(enc);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = buffer.slice(..).get_mapped_range();
        let bgra = matches!(rt.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut rgba = Vec::with_capacity((rt.width * rt.height * 4) as usize);
        for y in 0..rt.height {
            let r = &data[(y * row) as usize..(y * row + rt.width * 4) as usize];
            for px in r.chunks_exact(4) {
                rgba.extend_from_slice(&if bgra { [px[2], px[1], px[0], px[3]] } else { [px[0], px[1], px[2], px[3]] });
            }
        }
        Some(Frame { width: rt.width, height: rt.height, rgba })
    }

    /// What a draw derives from its shaders, declaration, render states,
    /// targets and streams: the shader modules, target views, the pipeline
    /// and its layouts (L99: kept in the draw memo).
    fn derive(&mut self, d: &DrawCall, instanced: &[u32]) -> Result<Derived, String> {
        self.poll_warmup();
        let state = |s: u32| d.render_states.binary_search_by_key(&s, |&(k, _)| k).ok().map(|i| d.render_states[i].1).unwrap_or(0);
        let vs_tokens = d.vertex_shader.as_ref().ok_or("draws without a vertex shader (fixed function) are not implemented")?;
        let ps_tokens = d.pixel_shader.as_ref().ok_or("draws without a pixel shader (fixed function) are not implemented")?;
        let (vh, ph) = (self.shader_identities.key(vs_tokens), self.shader_identities.key(ps_tokens));
        // the shaders' input declarations (usage, index), parsed once
        let dcl_inputs = |tokens: &[u32]| -> Result<Vec<(u32, u32)>, String> {
            let shader = d3d9shader::parse(tokens).map_err(|e| e.to_string())?;
            let mut v = Vec::new();
            for i in shader.instructions.iter().filter(|i| i.opcode == d3d9shader::op::DCL) {
                if let (Some(dst), Some(t)) = (&i.dst, i.decl) {
                    if dst.kind == d3d9shader::reg::INPUT {
                        v.push((t & 0x1F, (t >> 16) & 0xF));
                    }
                }
            }
            Ok(v)
        };
        if !self.vs_inputs.contains_key(&vh) {
            let v = dcl_inputs(vs_tokens)?;
            self.vs_inputs.insert(vh, v);
        }
        if !self.ps_inputs.contains_key(&ph) {
            let v = dcl_inputs(ps_tokens)?;
            self.ps_inputs.insert(ph, v);
        }
        // vertex inputs: each declared input's element
        let mut types = Vec::new();
        let mut elements = Vec::new();
        for (k, &(usage, index)) in self.vs_inputs[&vh].iter().enumerate() {
            let e = d.declaration.iter().find(|e| e[6] as u32 == usage && e[7] as u32 == index)
                .ok_or_else(|| format!("vertex input usage {usage} index {index} has no declaration element"))?;
            let (format, ty) = vertex_format(e[4]).ok_or_else(|| format!("vertex element type {}", e[4]))?;
            types.push(ty);
            elements.push((u16::from_le_bytes([e[0], e[1]]) as u32, u16::from_le_bytes([e[2], e[3]]) as u64, format, k as u32));
        }
        let (vs_module, vs_iface) = self.module(vs_tokens, |_| d3d9wgsl::Options { vs_input_types: Some(types), half_pixel: true, ..Default::default() }, (vh, 0, false, 0))?;
        // link the pixel inputs to the vertex outputs (built only for a
        // translation)
        let locations = |b: &Self| -> Vec<Option<u32>> {
            b.ps_inputs[&ph].iter().map(|&(u, n)| vs_iface.outputs_vs.iter().position(|o| o.0 == u && o.1 == n).map(|p| p as u32)).collect()
        };
        // D3D9's depth bias in the pixel shader (see d3d9wgsl Options::depth_bias)
        let depth_bias = d.depth.is_some() && state(7) != 0 && (state(195) != 0 || state(175) != 0);
        // render-target textures lack channels D3D9 samples as 1
        let ones = sampler_ones(d);
        let ps_options = |b: &Self| d3d9wgsl::Options { ps_input_locations: Some(locations(b)), alpha_test: true, lod_bias: true, depth_bias, sampler_ones: ones, ..Default::default() };
        let (ps_module, ps_iface) = self.module(ps_tokens, ps_options, (ph, vh, depth_bias, ones))?;

        // targets
        let srgb = state(194) != 0;
        let mut colour: Vec<Option<(wgpu::TextureView, wgpu::TextureFormat)>> = Vec::new();
        for (k, t) in d.targets.iter().enumerate() {
            colour.push(match t {
                Some(t) if ps_iface.outputs.contains(&(k as u32)) || k == 0 => {
                    Some(self.cached_target_view(t, srgb && k == 0).ok_or_else(|| format!("render target {k}: format {} is not supported", t.format))?)
                }
                _ => None,
            });
        }
        // the depth-stencil surface takes part when depth or stencil testing
        // is on; with ZENABLE off the depth buffer is neither tested nor written
        let z_on = state(7) != 0;
        let depth = match &d.depth {
            Some(t) if z_on || state(52) != 0 => self.cached_target_view(t, false),
            _ => None,
        };
        // every attachment has the same sample count (D3D9 requires the
        // render targets and the depth-stencil to share a multisample type)
        let used: Vec<&Target> = d.targets.iter().zip(&colour).filter_map(|(t, c)| c.as_ref().and(t.as_ref()))
            .chain(d.depth.as_ref().filter(|_| depth.is_some())).collect();
        let samples = match used.first() {
            Some(t) => sample_count(t)?,
            None => 1,
        };
        if used.iter().any(|t| sample_count(t).ok() != Some(samples)) {
            return Err("the render targets and the depth-stencil surface have different multisample types".into());
        }
        // pipeline state
        // D3D9 ignores the factors of MIN/MAX blending; wgpu wants them One
        let component = |src: u32, dst: u32, op: u32| {
            let operation = blend_op(op);
            if matches!(operation, wgpu::BlendOperation::Min | wgpu::BlendOperation::Max) {
                wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation }
            } else {
                wgpu::BlendComponent { src_factor: blend_factor(src), dst_factor: blend_factor(dst), operation }
            }
        };
        let blend = (state(27) != 0).then(|| {
            let colour = component(state(19), state(20), state(171));
            let alpha = if state(206) != 0 { component(state(207), state(208), state(209)) } else { colour };
            wgpu::BlendState { color: colour, alpha }
        });
        let writes = [168u32, 190, 191, 192];
        let max_out = colour.iter().rposition(|c| c.is_some()).map(|p| p + 1).unwrap_or(0);
        // the D3D9 driver blends into every render-target format it offers
        // (the recorded profile answers POSTPIXELSHADER_BLENDING for all of
        // them, 32-bit float ones included)
        let float32 = |f: &wgpu::TextureFormat| matches!(f, wgpu::TextureFormat::R32Float | wgpu::TextureFormat::Rg32Float | wgpu::TextureFormat::Rgba32Float);
        if blend.is_some() && colour.iter().flatten().any(|(_, f)| float32(f)) && !self.device.features().contains(wgpu::Features::FLOAT32_BLENDABLE) {
            return Err("blending into a 32-bit float target: the adapter has no FLOAT32_BLENDABLE".into());
        }
        let targets: Vec<Option<wgpu::ColorTargetState>> = (0..max_out).map(|k| colour[k].as_ref().map(|(_, f)| wgpu::ColorTargetState {
            format: *f,
            blend,
            write_mask: wgpu::ColorWrites::from_bits_truncate(state(writes[k])),
        })).collect();
        let depth_state = depth.as_ref().map(|(_, f)| wgpu::DepthStencilState {
            format: *f,
            depth_write_enabled: Some(z_on && state(14) != 0),
            depth_compare: Some(if z_on { compare(state(23)) } else { wgpu::CompareFunction::Always }),
            stencil: if state(52) != 0 && f.has_stencil_aspect() {
                let face = |func, fail, zfail, pass| wgpu::StencilFaceState {
                    compare: compare(state(func)),
                    fail_op: stencil_op(state(fail)),
                    depth_fail_op: stencil_op(state(zfail)),
                    pass_op: stencil_op(state(pass)),
                };
                // front faces are D3D9's clockwise ones (FrontFace::Cw);
                // TWOSIDEDSTENCILMODE gives counter-clockwise faces the CCW_ states
                let front = face(56, 53, 54, 55);
                let back = if state(185) != 0 { face(189, 186, 187, 188) } else { front };
                wgpu::StencilState { front, back, read_mask: state(58), write_mask: state(59) }
            } else {
                wgpu::StencilState::default()
            },
            // DEPTHBIAS and SLOPESCALEDEPTHBIAS are added by the pixel shader:
            // hardware bias scales the constant by an implementation-defined
            // step (measured 2^-23 for D24 on the NVIDIA Vulkan driver, where
            // D3D9 adds the value itself)
            bias: wgpu::DepthBiasState::default(),
        });
        let topology = match d.primitive {
            1 => wgpu::PrimitiveTopology::PointList,
            2 => wgpu::PrimitiveTopology::LineList,
            3 => wgpu::PrimitiveTopology::LineStrip,
            5 => wgpu::PrimitiveTopology::TriangleStrip,
            _ => wgpu::PrimitiveTopology::TriangleList,
        };
        let cull = match state(22) {
            2 => Some(wgpu::Face::Front),
            3 => Some(wgpu::Face::Back),
            _ => None,
        };
        let polygon_mode = match state(8) {
            1 => wgpu::PolygonMode::Point,
            2 => wgpu::PolygonMode::Line,
            _ => wgpu::PolygonMode::Fill,
        };
        let needs = match polygon_mode {
            wgpu::PolygonMode::Point => wgpu::Features::POLYGON_MODE_POINT,
            wgpu::PolygonMode::Line => wgpu::Features::POLYGON_MODE_LINE,
            _ => wgpu::Features::empty(),
        };
        if !self.device.features().contains(needs) {
            return Err(format!("D3DFILLMODE {}: the adapter cannot rasterise it", state(8)));
        }
        // vertex buffers by stream
        let mut streams: Vec<u32> = elements.iter().map(|e| e.0).collect();
        streams.sort();
        streams.dedup();
        let stride_of = |s: u32| -> Result<u32, String> {
            match &d.vertices {
                Vertices::Streams(v) => v.iter().find(|x| x.0 == s).map(|x| x.3).ok_or_else(|| format!("stream {s} is not set")),
                Vertices::Inline { stride, .. } => Ok(*stride),
            }
        };
        let mut buffers = Vec::new();
        for &s in &streams {
            let attrs = elements.iter().filter(|e| e.0 == s).map(|e| (e.2, e.1, e.3)).collect();
            let step = if instanced.contains(&s) { wgpu::VertexStepMode::Instance } else { wgpu::VertexStepMode::Vertex };
            buffers.push((stride_of(s)? as u64, step, attrs));
        }
        // bind group layouts: 0 = pixel constants (binding 0) and vertex
        // constants (binding 2) in the uniform ring; 1 = pixel textures;
        // 2 = vertex textures
        let kinds = |iface: &d3d9wgsl::Interface| -> Vec<(u32, u8)> {
            iface.samplers.iter().map(|&(n, kind)| (n, match kind {
                d3d9wgsl::TextureKind::Flat => 0,
                d3d9wgsl::TextureKind::Cube => 1,
                d3d9wgsl::TextureKind::Volume => 2,
            })).collect()
        };
        let (ps_kinds, vs_kinds) = (kinds(&ps_iface), kinds(&vs_iface));
        let layout1 = self.tex_layout(false, &ps_kinds);
        let layout2 = self.tex_layout(true, &vs_kinds);
        // An indexed strip needs its index format on the pipeline. wgpu then
        // restarts the strip at index 0xFFFF (0xFFFFFFFF for 32-bit indices),
        // which D3D9 does not do: such a strip would have to reference vertex
        // 65535 through 16-bit indices to differ.
        let strip_index = match (&d.indices, topology) {
            (Indices::Buffer(_, f) | Indices::Inline { format: f, .. },
             wgpu::PrimitiveTopology::LineStrip | wgpu::PrimitiveTopology::TriangleStrip) =>
                Some(if *f == 102 { wgpu::IndexFormat::Uint32 } else { wgpu::IndexFormat::Uint16 }),
            _ => None,
        };
        let key = PipelineKey {
            vs: (vh, 0, false, 0),
            ps: (ph, vh, depth_bias, ones),
            buffers,
            topology,
            strip_index,
            cull,
            polygon_mode,
            targets,
            depth: depth_state,
            samples,
            // D3DRS_MULTISAMPLEMASK: the samples a draw may write
            sample_mask: if samples > 1 { state(162) } else { !0 },
        };
        let key = backend_data::Hashed::new(key, cache::FxHasher::default());
        if let Some(pipeline) = self.warmup.as_mut().and_then(|w| w.pipeline(&key.key)) {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.pipelines.entry(key.clone()) {
                entry.insert(pipeline);
                self.warm_hits += 1;
                pipeline_log::timed("pipeline-reuse", "warm-hit", || ());
            }
        }
        let pipeline = match self.pipelines.get(&key) {
            Some(p) => {
                pipelines::pipeline_reused();
                p.clone()
            }
            None => {
                let started = std::time::Instant::now();
                let layout_key = (ps_kinds.clone(), vs_kinds.clone());
                if !self.pipeline_layouts.contains_key(&layout_key) {
                    let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[Some(&self.layout0), Some(&layout1), Some(&layout2)],
                        immediate_size: 0,
                    });
                    self.pipeline_layouts.insert(layout_key.clone(), layout);
                }
                let pipeline = warmup::create_pipeline(&self.device, &self.pipeline_layouts[&layout_key], &vs_module, &ps_module,
                    &key.key, self.pipeline_cache.as_ref().map(|p| &p.cache), "object-miss")?;
                if let Some(warmup) = &self.warmup { warmup.record(&key.key, &ps_kinds, &vs_kinds); }
                if let Some(p) = &self.pipeline_cache {
                    p.created();
                }
                pipelines::pipeline_created(started);
                self.created_pipelines += 1;
                self.pipelines.insert(key.clone(), pipeline.clone());
                pipeline
            }
        };
        let vk_pipeline = self.vk.as_mut().and_then(|vk| vk.pipeline(&key.key, &ps_kinds, &vs_kinds)
            .map_err(|e| eprintln!("DDGPU_VK pipeline fallback: {e}")).ok());
        Ok(Derived { vk_pipeline, ps_kinds, vs_kinds, layout1, layout2, depth_bias, streams, colour, depth, max_out, pipeline,
                     vs_floats: vs_iface.float_constants, ps_floats: ps_iface.float_constants })
    }

    /// The draw's derived state from the memo, or derived now (L99). The
    /// key holds the shader, declaration and render-state lists themselves
    /// (shared Arcs, compared by pointer; held, so an address is not reused
    /// while its entry lives) and the rest compared by value. release()
    /// clears the memo with the target views its entries hold.
    fn derived(&mut self, d: &DrawCall, vs: &Arc<[u32]>, ps: &Arc<[u32]>, instanced: &[u32]) -> Result<Arc<Derived>, String> {
        let ones = sampler_ones(d);
        let index_format = match &d.indices {
            Indices::Buffer(_, f) | Indices::Inline { format: f, .. } => *f,
            Indices::None => 0,
        };
        let slot = {
            let p = |x: usize| (x >> 4) as u64;
            let h = (p(Arc::as_ptr(vs) as *const u8 as usize) ^ p(Arc::as_ptr(ps) as *const u8 as usize).rotate_left(16)
                ^ p(Arc::as_ptr(&d.declaration) as *const u8 as usize).rotate_left(32) ^ d.render_states.address().rotate_left(48))
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h
        };
        if let Some(m) = self.memo.get(slot, |m| Arc::ptr_eq(&m.vs, vs) && Arc::ptr_eq(&m.ps, ps) && Arc::ptr_eq(&m.declaration, &d.declaration)
                && m.states.ptr_eq(&d.render_states) && m.primitive == d.primitive && m.index_format == index_format
                && m.ones == ones && m.instanced[..] == *instanced && m.targets == d.targets && m.depth == d.depth
                && m.strides.iter().copied().eq(strides(d))) {
            return Ok(m.derived.clone());
        }
        let derived = Arc::new(self.derive(d, instanced)?);
        self.memo.insert(slot, DrawMemo {
            vs: vs.clone(), ps: ps.clone(), declaration: d.declaration.clone(), states: d.render_states.clone(),
            primitive: d.primitive, index_format, ones, instanced: instanced.to_vec(),
            targets: d.targets.clone(), depth: d.depth.clone(), strides: strides(d).collect(),
            derived: derived.clone(),
        });
        Ok(derived)
    }

    fn try_draw(&mut self, d: &DrawCall) -> Result<(), String> {
        let mut buffers = std::mem::take(&mut self.draw_buffers);
        let mut instanced = std::mem::take(&mut self.instance_streams);
        let result = self.try_draw_buffered(d, &mut buffers, &mut instanced);
        // Also recycle after an unsupported draw; never retain resource handles
        // past the command (release/upload may follow immediately).
        buffers.clear();
        instanced.clear();
        self.draw_buffers = buffers;
        self.instance_streams = instanced;
        result
    }

    fn try_draw_buffered(&mut self, d: &DrawCall, vbufs: &mut Vec<(wgpu::Buffer, u64, u64, Option<u32>)>, instanced: &mut Vec<u32>) -> Result<(), String> {
        let state = |s: u32| d.render_states.binary_search_by_key(&s, |&(k, _)| k).ok().map(|i| d.render_states[i].1).unwrap_or(0);
        let vs_tokens = d.vertex_shader.as_ref().ok_or("draws without a vertex shader (fixed function) are not implemented")?;
        let ps_tokens = d.pixel_shader.as_ref().ok_or("draws without a pixel shader (fixed function) are not implemented")?;
        if d.declaration.is_empty() {
            return Err("FVF draws are not implemented".into());
        }
        let instances = instancing(d, instanced)?;
        let x = self.derived(d, vs_tokens, ps_tokens, instanced)?;
        let (ps_kinds, vs_kinds, layout1, layout2, streams) = (&x.ps_kinds, &x.vs_kinds, &x.layout1, &x.layout2, &x.streams);
        let (colour, depth, max_out, depth_bias, pipeline) = (&x.colour, &x.depth, x.max_out, x.depth_bias, &x.pipeline);
        // A texture sampled by a draw that also renders into it: D3D9 leaves
        // this to the driver (the samples read what was there before the
        // draw); wgpu forbids one texture as both binding and attachment in
        // a pass. The draw samples a copy made just before it, recorded on
        // the same encoder (L59).
        let attached: [Option<u32>; 5] = std::array::from_fn(|k| match k {
            0..4 => d.targets[k].as_ref().and_then(|t| t.texture.map(|x| x.resource)),
            _ => d.depth.as_ref().and_then(|t| t.texture.map(|x| x.resource)),
        });
        let attached = |r: u32| attached.contains(&Some(r));
        let mut snapshots: HashMap<u32, wgpu::Texture> = HashMap::new();
        for b in d.textures.iter() {
            if attached(b.resource) && !snapshots.contains_key(&b.resource) {
                let Some(tx) = self.texture_for(b) else { continue };
                let src = tx.texture.clone();
                let copy = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("feedback snapshot"),
                    size: src.size(),
                    mip_level_count: src.mip_level_count(),
                    sample_count: 1,
                    dimension: src.dimension(),
                    format: src.format(),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[src.format().add_srgb_suffix(), src.format().remove_srgb_suffix()],
                });
                self.end_pass();
                let mut enc = self.open.borrow_mut().take().unwrap_or_else(|| self.device.create_command_encoder(&Default::default()));
                for level in 0..src.mip_level_count() {
                    let size = src.size().mip_level_size(level, src.dimension());
                    enc.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo { texture: &src, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        wgpu::TexelCopyTextureInfo { texture: &copy, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        size,
                    );
                }
                *self.open.borrow_mut() = Some(enc);
                snapshots.insert(b.resource, copy);
            }
        }
        // texture bind groups, by what they bind. L94b: the key is built in
        // a reused buffer and looked up without allocating; a stage whose key
        // equals the previous draw's reuses its group without a lookup
        let mut groups: [Option<wgpu::BindGroup>; 2] = [None, None];
        let mut transient_vk_groups = Vec::new();
        for (stage, (kinds, layout, base, vs)) in [(ps_kinds, layout1, 0u32, false), (vs_kinds, layout2, 257u32, true)].into_iter().enumerate() {
            let mut entries = std::mem::take(&mut self.group_key[stage]);
            entries.clear();
            for &(n, kind) in kinds.iter() {
                match d.textures.iter().find(|t| t.sampler == base + n) {
                    Some(b) => entries.push((n, kind, b.resource, b.states[11] != 0, b.states)),
                    None => {
                        self.group_key[stage] = entries;
                        return Err(format!("sampler s{n} has no texture bound"));
                    }
                }
            }
            let feedback = !snapshots.is_empty() && entries.iter().any(|e| snapshots.contains_key(&e.2));
            if !feedback {
                let last = &self.last_group[stage];
                if let (Some(g), true) = (&last.1, last.0 == entries) {
                    groups[stage] = Some(g.clone());
                    self.group_key[stage] = entries;
                    continue;
                }
                let key = (vs, entries);
                let found = self.tex_groups.get(&key).cloned();
                let entries = key.1;
                if let Some(g) = found {
                    let last = &mut self.last_group[stage];
                    last.0.clear();
                    last.0.extend_from_slice(&entries);
                    last.1 = Some(g.clone());
                    groups[stage] = Some(g);
                    self.group_key[stage] = entries;
                    continue;
                }
                self.group_key[stage] = entries;
            } else {
                self.group_key[stage] = entries;
            }
            let gkey = (vs, self.group_key[stage].clone());
            let mut views = Vec::new();
            for &(n, kind, _, srgb_read, states) in &gkey.1 {
                let b = d.textures.iter().find(|t| t.sampler == base + n).expect("checked above");
                let view_dimension = match kind {
                    0 => wgpu::TextureViewDimension::D2,
                    1 => wgpu::TextureViewDimension::Cube,
                    _ => wgpu::TextureViewDimension::D3,
                };
                let tx = self.texture_for(b).ok_or_else(|| format!("texture format {:#x} is not supported", b.format))?;
                let format = if srgb_read { tx.gpu_format.add_srgb_suffix() } else { tx.gpu_format };
                let texture = snapshots.get(&b.resource).unwrap_or(&tx.texture);
                let desc = wgpu::TextureViewDescriptor { dimension: Some(view_dimension), format: Some(format), ..Default::default() };
                let view = texture.create_view(&desc);
                if let Some(vk) = &mut self.vk {
                    if let Err(e) = vk.view(&view, &desc, wgpu::TextureUses::RESOURCE) { eprintln!("DDGPU_VK texture fallback: {e}"); }
                }
                views.push((n, view, self.sampler(&states)?));
            }
            let mut entries = Vec::new();
            for (n, v, s) in &views {
                entries.push(wgpu::BindGroupEntry { binding: 2 * n, resource: wgpu::BindingResource::TextureView(v) });
                entries.push(wgpu::BindGroupEntry { binding: 2 * n + 1, resource: wgpu::BindingResource::Sampler(s) });
            }
            let g = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries });
            if let (Some(vk), Some(pipeline)) = (&mut self.vk, &x.vk_pipeline) {
                if let Err(e) = vk.group(&g, pipeline, stage, &views) { eprintln!("DDGPU_VK group fallback: {e}"); }
            }
            if feedback && self.vk.is_some() { transient_vk_groups.push(g.clone()); }
            if !feedback {
                let last = &mut self.last_group[stage];
                last.0.clear();
                last.0.extend_from_slice(&gkey.1);
                last.1 = Some(g.clone());
                self.tex_groups.insert(gkey, g.clone()); // a snapshot is for this draw only
            }
            groups[stage] = Some(g);
        }
        let groups = groups.map(|g| g.expect("both stages bound"));
        let vk_binding = x.vk_pipeline.as_ref().and_then(|p| {
            let cache = &self.vk.as_ref()?.groups;
            Some((p.clone(), [cache.get(&groups[0])?.clone(), cache.get(&groups[1])?.clone()]))
        });
        if let Some(vk) = &mut self.vk {
            for group in transient_vk_groups { vk.groups.remove(&group); }
        }
        // uniforms: half-pixel offset from the viewport; alpha test from the states
        let (vw, vh_) = (d.viewport[2].max(1) as f32, d.viewport[3].max(1) as f32);
        // D3D9 samples pixels at their integer corner, wgpu at the centre:
        // the geometry moves half a pixel right and down
        let hp = [(1.0 / vw).to_bits(), (-1.0 / vh_).to_bits(), 0, 0];
        // the alpha test, then each pixel sampler's MIPMAPLODBIAS (float bits)
        let mut ps_extra = [[state(15), state(25), state(24) & 0xFF, 0], [0; 4], [0; 4], [0; 4], [0; 4], [0; 4]];
        for t in d.textures.iter().filter(|t| t.sampler < 16) {
            ps_extra[1 + t.sampler as usize / 4][t.sampler as usize % 4] = t.states[8];
        }
        if depth_bias {
            ps_extra[5] = [state(195), state(175), 0, 0];
        }
        let ps_extra = &ps_extra[..if depth_bias { 6 } else { 5 }];
        // vertex and index data
        for &s in streams {
            match &d.vertices {
                Vertices::Streams(v) => {
                    let (_, res, off, _, _) = *v.iter().find(|x| x.0 == s).unwrap();
                    let b = self.buffer(res).ok_or_else(|| format!("vertex buffer {res:#x} has no data"))?;
                    let size = b.size();
                    vbufs.push((b, off as u64, size, Some(res)));
                }
                Vertices::Inline { bytes, .. } => {
                    let (b, off, len) = self.inline_data(bytes, wgpu::BufferUsages::VERTEX);
                    vbufs.push((b, off, off + len, None));
                }
            }
        }
        let fan = d.primitive == 6;
        // (buffer, start, end, resource), format, first index
        let (index, format, first) = match &d.indices {
            Indices::None if fan => {
                let idx: Vec<u32> = (1..=d.primitives).flat_map(|k| [d.start, d.start + k, d.start + k + 1]).collect();
                let (b, off, len) = self.inline_data(&idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>(), wgpu::BufferUsages::INDEX);
                (Some((b, off, off + len, None)), wgpu::IndexFormat::Uint32, 0)
            }
            Indices::None => (None, wgpu::IndexFormat::Uint16, d.start),
            Indices::Buffer(res, f) => {
                if fan {
                    return Err("indexed triangle fans are not implemented".into());
                }
                let b = self.buffer(*res).ok_or("index buffer has no data")?;
                let size = b.size();
                (Some((b, 0, size, Some(*res))), if *f == 102 { wgpu::IndexFormat::Uint32 } else { wgpu::IndexFormat::Uint16 }, d.start)
            }
            Indices::Inline { bytes, format } => {
                if fan {
                    return Err("indexed triangle fans are not implemented".into());
                }
                let (b, off, len) = self.inline_data(bytes, wgpu::BufferUsages::INDEX);
                (Some((b, off, off + len, None)), if *format == 102 { wgpu::IndexFormat::Uint32 } else { wgpu::IndexFormat::Uint16 }, 0)
            }
        };
        let count = if fan { 3 * d.primitives } else { match d.primitive {
            1 => d.primitives,
            2 => 2 * d.primitives,
            3 => d.primitives + 1,
            4 => 3 * d.primitives,
            _ => d.primitives + 2,
        } };
        // the constant blocks last: a ring restart above (a flush) must not
        // separate them from this draw
        // A restart between the two stage allocations would invalidate the
        // first offset. Reserve both maximum binding ranges before either.
        if !self.uniforms.borrow().fits(PS_BLOCK + VS_BLOCK + UNIFORM_ALIGN, UNIFORM_ALIGN) {
            self.flush();
            self.uniforms.borrow_mut().restart();
        }
        let ps_off = self.constants(&d.ps_constants, x.ps_floats, ps_extra, PS_BLOCK, 0);
        let vs_off = self.constants(&d.vs_constants, x.vs_floats, &[hp], VS_BLOCK, 1);
        // a draw made while guest occlusion queries count gets its own slot
        let slot = (!self.active_queries.is_empty()).then(|| self.occlusion_slot());
        if let Some(s) = slot {
            self.slot_users.insert(s, self.active_queries.len() as u32);
            for q in &self.active_queries {
                self.query_slots.entry(*q).or_default().push(s);
            }
        }
        let query_set = slot.map(|s| self.occlusion_sets[(s / OCCLUSION_SET) as usize].clone());
        {
            let mut uses = self.open_uses.borrow_mut();
            uses.extend(vbufs.iter().filter_map(|v| v.3));
            uses.extend(index.iter().filter_map(|v| v.3));
            uses.extend(d.textures.iter().map(|t| t.resource));
            uses.extend(d.targets.iter().chain(std::iter::once(&d.depth)).flatten().filter_map(|t| t.texture.map(|x| x.resource)));
        }
        let scissor = state(174) != 0;
        if self.vk.is_some() && slot.is_none() {
            let same = self.vk_pass.borrow().as_ref().is_some_and(|p|
                p.colors.len() == max_out
                && p.colors.iter().zip(&colour[..max_out]).all(|(a,c)| a.as_ref() == c.as_ref().map(|(v,_)| v))
                && p.depth.as_ref().map(|(v,_)| v) == depth.as_ref().map(|(v,_)| v)
                && (scissor || p.draws.last().is_none_or(|d| d.scissor.is_none())));
            if !same {
                self.end_pass();
                let extent = d.targets[..max_out].iter().zip(&colour[..max_out]).filter_map(|(t,v)| v.as_ref().and(t.as_ref())).chain(d.depth.iter().filter(|_| depth.is_some()))
                    .fold(wgpu::Extent3d { width: u32::MAX, height: u32::MAX, depth_or_array_layers: 1 }, |mut e,t| {
                        e.width=e.width.min(t.width); e.height=e.height.min(t.height); e
                    });
                *self.vk_pass.borrow_mut() = Some(vk::Pass {
                    native_colors: colour[..max_out].iter().map(|c| c.as_ref().and_then(|(v,_)| self.vk.as_ref().unwrap().views.get(v).cloned())).collect(),
                    native_depth: depth.as_ref().and_then(|(v,_)| self.vk.as_ref().unwrap().views.get(v).cloned()),
                    colors: colour[..max_out].iter().map(|c| c.as_ref().map(|(v,_)| v.clone())).collect(),
                    depth: depth.clone(), extent, draws: vec![], initialize: false, initialized: self.vk.as_ref().unwrap().initialized.clone(),
                });
                self.passes_begun.set(self.passes_begun.get()+1);
            }
            let v=d.viewport;
            self.vk_pass.borrow_mut().as_mut().unwrap().draws.push(vk::Draw {
                pipeline: pipeline.clone(), groups: [self.group0.clone(),groups[0].clone(),groups[1].clone()], native: vk_binding,
                offsets: [ps_off,vs_off], vertex: vbufs.iter().map(|(b,s,e,_)| (b.clone(),*s,*e)).collect(),
                index: index.as_ref().map(|(b,s,e,_)| (b.clone(),*s,*e,format)),
                viewport: [v[0] as f32,v[1] as f32,v[2] as f32,v[3] as f32,f32::from_bits(v[4]),f32::from_bits(v[5])],
                scissor: scissor.then(|| { let s=d.scissor; [s[0].max(0) as u32,s[1].max(0) as u32,(s[2]-s[0]).max(0) as u32,(s[3]-s[1]).max(0) as u32] }),
                stencil: state(57), blend: clear_colour(state(193),wgpu::TextureFormat::Rgba32Float), first,count,
                base: if fan {0} else {d.base_vertex}, instances,
            });
            self.draws_recorded.set(self.draws_recorded.get()+1);
            self.open_draws.set(self.open_draws.get()+1);
            if self.open_draws.get() >= self.draws_per_submit { self.flush_async(); }
            return Ok(());
        }
        // (L94b: compared in place; the key is built for a new pass only)
        let same_pass = matches!(&*self.pass.borrow(), Some(p) if (scissor || !p.scissored)
            && p.key.0.len() == max_out
            && p.key.0.iter().zip(&colour[..max_out]).all(|(a, c)| a.as_ref() == c.as_ref().map(|(v, _)| v))
            && p.key.1.as_ref() == depth.as_ref().map(|(v, _)| v)
            && p.key.2 == query_set);
        if !same_pass {
            if self.vk.is_some() { vk::unsupported_pass(); }
            let key: PassKey = (colour[..max_out].iter().map(|c| c.as_ref().map(|(v, _)| v.clone())).collect(),
                                depth.as_ref().map(|(v, _)| v.clone()), query_set.clone());
            self.end_pass();
            let mut enc = self.open.borrow_mut().take().unwrap_or_else(|| self.device.create_command_encoder(&Default::default()));
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = colour[..max_out].iter().map(|c| c.as_ref().map(|(view, _)| wgpu::RenderPassColorAttachment {
                view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })).collect();
            let timing = self.encode_profile.start();
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &attachments,
                depth_stencil_attachment: depth.as_ref().map(|(view, f)| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: f.has_stencil_aspect().then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: query_set.as_ref(),
                multiview_mask: None,
            }).forget_lifetime();
            self.encode_profile.end(0, timing);
            *self.open.borrow_mut() = Some(enc);
            *self.pass.borrow_mut() = Some(OpenPass { pass, key, scissored: false, last: PassState::default() });
            self.passes_begun.set(self.passes_begun.get() + 1);
        }
        {
            let mut open = self.pass.borrow_mut();
            let open = open.as_mut().expect("begun above");
            open.scissored |= scissor;
            let (pass, last) = (&mut open.pass, &mut open.last);
            if let Some(s) = slot {
                pass.begin_occlusion_query(s % OCCLUSION_SET);
            }
            let timing = self.encode_profile.start();
            if last.pipeline.as_ref() != Some(pipeline) {
                pass.set_pipeline(&pipeline);
                last.pipeline = Some(pipeline.clone());
            }
            self.encode_profile.end(2, timing);
            let timing = self.encode_profile.start();
            if last.group0 != Some([ps_off, vs_off]) {
                pass.set_bind_group(0, &self.group0, &[ps_off, vs_off]);
                last.group0 = Some([ps_off, vs_off]);
            }
            if last.group1.as_ref() != Some(&groups[0]) {
                pass.set_bind_group(1, &groups[0], &[]);
                last.group1 = Some(groups[0].clone());
            }
            if last.group2.as_ref() != Some(&groups[1]) {
                pass.set_bind_group(2, &groups[1], &[]);
                last.group2 = Some(groups[1].clone());
            }
            self.encode_profile.end(3, timing);
            let timing = self.encode_profile.start();
            let v = d.viewport;
            if last.viewport != Some(v) {
                pass.set_viewport(v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32, f32::from_bits(v[4]), f32::from_bits(v[5]));
                last.viewport = Some(v);
            }
            if scissor && last.scissor != Some(d.scissor) {
                let s = d.scissor;
                pass.set_scissor_rect(s[0].max(0) as u32, s[1].max(0) as u32, (s[2] - s[0]).max(0) as u32, (s[3] - s[1]).max(0) as u32);
                last.scissor = Some(s);
            }
            if last.stencil_ref != Some(state(57)) {
                pass.set_stencil_reference(state(57));
                last.stencil_ref = Some(state(57));
            }
            // BLENDFACTOR in single precision as the driver converts it
            // (byte * (1/255 as f32); measured on 32-bit float targets)
            if last.blend != Some(state(193)) {
                pass.set_blend_constant(clear_colour(state(193), wgpu::TextureFormat::Rgba32Float));
                last.blend = Some(state(193));
            }
            self.encode_profile.end(6, timing);
            let timing = self.encode_profile.start();
            for (k, (b, off, end, _)) in vbufs.iter().enumerate() {
                if last.vertex.len() <= k {
                    last.vertex.resize(k + 1, None);
                }
                if last.vertex[k].as_ref().is_none_or(|(lb, lo, le)| lb != b || lo != off || le != end) {
                    pass.set_vertex_buffer(k as u32, b.slice(*off..*end));
                    last.vertex[k] = Some((b.clone(), *off, *end));
                }
            }
            self.encode_profile.end(4, timing);
            let timing = self.encode_profile.start();
            match &index {
                Some((ib, off, end, _)) => {
                    if last.index.as_ref().is_none_or(|(lb, lo, le, lf)| lb != ib || lo != off || le != end || *lf != format) {
                        pass.set_index_buffer(ib.slice(*off..*end), format);
                        last.index = Some((ib.clone(), *off, *end, format));
                    }
                    pass.draw_indexed(first..first + count, if fan { 0 } else { d.base_vertex }, 0..instances);
                }
                None => pass.draw(first..first + count, 0..instances),
            }
            self.encode_profile.end(5, timing);
            if slot.is_some() {
                pass.end_occlusion_query();
            }
        }
        self.draws_recorded.set(self.draws_recorded.get() + 1);
        self.open_draws.set(self.open_draws.get() + 1);
        if self.open_draws.get() >= self.draws_per_submit {
            self.flush_async();
        }
        Ok(())
    }

    /// The bind group layout of a stage's textures and samplers.
    fn tex_layout(&mut self, vs: bool, kinds: &[(u32, u8)]) -> wgpu::BindGroupLayout {
        let key = (vs, kinds.to_vec());
        if let Some(l) = self.tex_layouts.get(&key) {
            return l.clone();
        }
        // L113: with DDGPU_NATIVE_SIM the pixel textures are the quad
        // dispatches' too (compute_quads)
        let l = warmup::texture_layout(&self.device, vs, kinds, self.native_sim);
        self.tex_layouts.insert(key, l.clone());
        l
    }
}

/// D3D9 hardware instancing (SetStreamSourceFreq): stream 0 set to
/// D3DSTREAMSOURCE_INDEXEDDATA | n draws indexed geometry n times; streams
/// set to D3DSTREAMSOURCE_INSTANCEDATA | 1 advance once per instance.
/// Measured on the driver (ddgpu tests, instanced_draws_match_the_d3d9_driver):
/// a non-indexed draw ignores the count and draws once, its instance-data
/// streams still giving every vertex instance 0's element. Returns the
/// instance count and the instanced streams.
/// Render-target textures lack channels D3D9 samples as 1: the pixel
/// samplers' masks (4 bits each).
fn sampler_ones(d: &DrawCall) -> u64 {
    d.textures.iter().filter(|t| t.sampler < 16 && t.usage & (USAGE_RENDERTARGET | USAGE_DEPTHSTENCIL) != 0)
        .fold(0u64, |m, t| m | (missing_channels(t.format) as u64) << (4 * t.sampler))
}

/// (stream, stride) of each vertex stream; inline vertices are stream MAX.
fn strides(d: &DrawCall) -> impl Iterator<Item = (u32, u32)> + '_ {
    let (streams, inline) = match &d.vertices {
        Vertices::Streams(v) => (&v[..], None),
        Vertices::Inline { stride, .. } => (&[][..], Some((u32::MAX, *stride))),
    };
    streams.iter().map(|x| (x.0, x.3)).chain(inline)
}

fn instancing(d: &DrawCall, instanced: &mut Vec<u32>) -> Result<u32, String> {
    instanced.clear();
    const INDEXEDDATA: u32 = 0x4000_0000;
    const INSTANCEDATA: u32 = 0x8000_0000;
    let Vertices::Streams(streams) = &d.vertices else { return Ok(1) };
    let geometry = streams.iter().find(|s| s.0 == 0).map(|s| s.4).unwrap_or(1);
    if geometry & INDEXEDDATA == 0 {
        return Ok(1);
    }
    for s in streams.iter() {
        if s.4 & INSTANCEDATA != 0 {
            if s.4 & 0x3FFF_FFFF != 1 {
                return Err(format!("instance data divisor {} on stream {} (wgpu steps every instance)", s.4 & 0x3FFF_FFFF, s.0));
            }
            instanced.push(s.0);
        }
    }
    let count = if matches!(d.indices, Indices::None) { 1 } else { (geometry & 0x3FFF_FFFF).max(1) };
    Ok(count)
}

fn vertex_format(ty: u8) -> Option<(wgpu::VertexFormat, d3d9wgsl::InputType)> {
    use d3d9wgsl::InputType::*;
    use wgpu::VertexFormat as V;
    Some(match ty {
        0 => (V::Float32, Float),
        1 => (V::Float32x2, Float),
        2 => (V::Float32x3, Float),
        3 => (V::Float32x4, Float),
        4 => (V::Unorm8x4Bgra, Float),
        5 => (V::Uint8x4, Uint),
        6 => (V::Sint16x2, Sint),
        7 => (V::Sint16x4, Sint),
        8 => (V::Unorm8x4, Float),
        9 => (V::Snorm16x2, Float),
        10 => (V::Snorm16x4, Float),
        11 => (V::Unorm16x2, Float),
        12 => (V::Unorm16x4, Float),
        15 => (V::Float16x2, Float),
        16 => (V::Float16x4, Float),
        _ => return None,
    })
}

fn blend_factor(v: u32) -> wgpu::BlendFactor {
    use wgpu::BlendFactor as B;
    match v {
        1 => B::Zero,
        3 => B::Src,
        4 => B::OneMinusSrc,
        5 => B::SrcAlpha,
        6 => B::OneMinusSrcAlpha,
        7 => B::DstAlpha,
        8 => B::OneMinusDstAlpha,
        9 => B::Dst,
        10 => B::OneMinusDst,
        11 => B::SrcAlphaSaturated,
        14 => B::Constant,
        15 => B::OneMinusConstant,
        _ => B::One,
    }
}

fn blend_op(v: u32) -> wgpu::BlendOperation {
    match v {
        2 => wgpu::BlendOperation::Subtract,
        3 => wgpu::BlendOperation::ReverseSubtract,
        4 => wgpu::BlendOperation::Min,
        5 => wgpu::BlendOperation::Max,
        _ => wgpu::BlendOperation::Add,
    }
}

fn compare(v: u32) -> wgpu::CompareFunction {
    use wgpu::CompareFunction as C;
    match v {
        1 => C::Never,
        2 => C::Less,
        3 => C::Equal,
        4 => C::LessEqual,
        5 => C::Greater,
        6 => C::NotEqual,
        7 => C::GreaterEqual,
        _ => C::Always,
    }
}

fn stencil_op(v: u32) -> wgpu::StencilOperation {
    use wgpu::StencilOperation as S;
    match v {
        2 => S::Zero,
        3 => S::Replace,
        4 => S::IncrementClamp,
        5 => S::DecrementClamp,
        6 => S::Invert,
        7 => S::IncrementWrap,
        8 => S::DecrementWrap,
        _ => S::Keep,
    }
}

/// The value Clear stores for a D3DCOLOR in a target of `format`, as the
/// D3D9 driver computes it (measured, tests/targets.rs): float targets get
/// byte * (1/255 as f32) in single precision (one ulp above k/255 for some
/// bytes), and 16-bit float targets that value truncated to half; unorm
/// targets k/255 (rounded to their precision by the GPU, which matches).
fn clear_colour(c: u32, format: wgpu::TextureFormat) -> wgpu::Color {
    use wgpu::TextureFormat as F;
    let byte = |s: u32| ((c >> s) & 0xFF) as f32 * (1.0f32 / 255.0);
    let channel = |s: u32| -> f64 {
        match format {
            F::R16Float | F::Rg16Float | F::Rgba16Float => half_toward_zero(byte(s)),
            F::R32Float | F::Rg32Float | F::Rgba32Float => byte(s) as f64,
            _ => ((c >> s) & 0xFF) as f64 / 255.0,
        }
    };
    wgpu::Color { r: channel(16), g: channel(8), b: channel(0), a: channel(24) }
}

/// A non-negative f32 below 65504 rounded toward zero to an IEEE half,
/// returned exactly.
fn half_toward_zero(v: f32) -> f64 {
    let b = v.to_bits();
    let e = ((b >> 23) & 0xFF) as i32 - 127;
    if v == 0.0 || e < -24 {
        return 0.0;
    }
    // keep 10 fraction bits for normal halves, fewer below 2^-14
    let keep = if e >= -14 { 10 } else { 10 - (-14 - e) };
    let drop = 23 - keep;
    f32::from_bits(b & !((1u32 << drop) - 1)) as f64
}

/// The sample a depth-stencil StretchRect from a multisampled source to a
/// single-sampled destination copies, by sample count: measured on the
/// D3D9 driver (L31 tests/msaa.rs depth_stencil_copies_match_the_d3d9_driver;
/// in the standard sample positions, which the driver's multisampled draws
/// match, these are the bottom-most samples).
fn depth_copy_sample(samples: u32) -> Option<u32> {
    match samples {
        2 => Some(0),
        4 => Some(3),
        8 => Some(6),
        _ => None,
    }
}

/// D3DFMT depth-stencil formats (D16_LOCKABLE..D24FS8) and the vendor
/// depth FOURCCs the backend maps.
fn is_depth_format(f: u32) -> bool {
    matches!(f, 70..=83) || f == fourcc(b"INTZ") || f == fourcc(b"DF24") || f == fourcc(b"DF16")
}

/// A partial clear's pipeline key: colour formats per slot, depth format,
/// depth written, stencil written, sample count.
type ClearKey = (Vec<Option<wgpu::TextureFormat>>, Option<wgpu::TextureFormat>, bool, bool, u32);

/// The rectangles (left, top, right, bottom) a Clear covers in a target of
/// `size`: the viewport, each of `rects` within it (none: the viewport),
/// within the scissor rectangle when the test is on (measured on the
/// driver, tests/targets.rs explore_clear_states).
fn clear_regions(c: &ClearCall, size: (i32, i32)) -> Vec<[i32; 4]> {
    let v = c.viewport;
    let vp = [v[0] as i32, v[1] as i32, (v[0] + v[2]) as i32, (v[1] + v[3]) as i32];
    let meet = |a: [i32; 4], b: [i32; 4]| [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    let base = if c.rects.is_empty() { vec![vp] } else { c.rects.iter().map(|&r| meet(r, vp)).collect() };
    base.into_iter()
        .map(|r| c.scissor.map_or(r, |s| meet(r, s)))
        .map(|r| meet(r, [0, 0, size.0, size.1]))
        .filter(|r| r[2] > r[0] && r[3] > r[1])
        .collect()
}

/// A rectangle, or the whole `w` x `h` target when D3D9 passed NULL (all zero).
fn rect_or_full(r: [i32; 4], w: u32, h: u32) -> [i32; 4] {
    if r == [0; 4] { [0, 0, w as i32, h as i32] } else { r }
}

impl GraphicsBackend for WgpuBackend {
    fn native_supported(&self) -> bool { true }
    fn native_compare_begin(&mut self, begin: d3d9cmd::NativeCompareBegin) -> bool { self.begin_native_comparison(begin) }
    fn native_compare_end(&mut self, id: u64, job: d3d9cmd::NativeJob) -> Result<(), d3d9cmd::NativeJob> { self.end_native_comparison(id, job) }
    fn native(&mut self, job: d3d9cmd::NativeJob) -> Result<(), d3d9cmd::NativeJob> {
        self.run_native_job(job)
    }
    fn copy(&mut self, c: &CopyCall) {
        // All paths stay ordered with preceding/following draws in the
        // open encoder. Uploads to either target must submit these uses first.
        self.end_pass();
        self.track_copy_targets(&[&c.src, &c.dst]);
        // Depth-stencil copies cover whole surfaces outside a D3D9 scene.
        if is_depth_format(c.src.format) || is_depth_format(c.dst.format) {
            if let Err(e) = self.copy_depth(&c.src, &c.dst) {
                self.errors.push(format!("StretchRect (depth-stencil): {e}"));
            }
            self.copy_recorded();
            return;
        }
        let Some((mut stex, slevel, slayer, sformat, sw, sh)) = self.target_texture(&c.src) else {
            self.errors.push("StretchRect from an unknown surface".into());
            return;
        };
        // a multisampled source is resolved (the samples of each pixel
        // averaged by the GPU's resolve) into a single-sampled texture of
        // its size, which is then copied like any other source
        if stex.sample_count() > 1 {
            match self.resolve_texture(&stex, sformat, sw, sh) {
                Ok(t) => stex = t,
                Err(e) => {
                    self.errors.push(format!("StretchRect: {e}"));
                    return;
                }
            }
        }
        let Some((dtex, dlevel, dlayer, dformat, dw, dh)) = self.target_texture(&c.dst) else {
            self.errors.push("StretchRect to an unknown surface".into());
            return;
        };
        let (sr, dr) = (rect_or_full(c.src_rect, sw, sh), rect_or_full(c.dst_rect, dw, dh));
        let same_size = sr[2] - sr[0] == dr[2] - dr[0] && sr[3] - sr[1] == dr[3] - dr[1];
        // (a multisampled destination is drawn into: WebGPU copies need one sample)
        if same_size && sformat == dformat && dtex.sample_count() == 1 && sr[0] >= 0 && sr[1] >= 0 && dr[0] >= 0 && dr[1] >= 0 {
            // L95: recorded into the open encoder after the draws before it
            // (the GPU runs them in that order), not a flush plus a
            // submission of its own; it counts toward the submit limit
            let mut enc = self.copy_encoder();
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &stex, mip_level: slevel, origin: wgpu::Origin3d { x: sr[0] as u32, y: sr[1] as u32, z: slayer }, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &dtex, mip_level: dlevel, origin: wgpu::Origin3d { x: dr[0] as u32, y: dr[1] as u32, z: dlayer }, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: (sr[2] - sr[0]) as u32, height: (sr[3] - sr[1]) as u32, depth_or_array_layers: 1 },
            );
            *self.open.borrow_mut() = Some(enc);
            self.copy_recorded();
            return;
        }
        let view = stex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2), base_mip_level: slevel, mip_level_count: Some(1),
            base_array_layer: slayer, array_layer_count: Some(1), ..Default::default()
        });
        let uv = [sr[0] as f32 / sw as f32, sr[1] as f32 / sh as f32, (sr[2] - sr[0]) as f32 / sw as f32, (sr[3] - sr[1]) as f32 / sh as f32];
        // D3DTEXF_LINEAR (2) and above filter; NONE/POINT take the nearest texel
        if let Err(e) = self.blit(&c.dst, dr, Some((&view, uv, c.filter >= 2)), [0.0; 4]) {
            self.errors.push(format!("StretchRect: {e}"));
        }
        self.copy_recorded();
    }

    fn fill(&mut self, f: &FillCall) {
        self.flush();
        let Some((_, _, _, _, w, h)) = self.target_texture(&f.dst) else { return };
        let ch = |s: u32| ((f.colour >> s) & 0xFF) as f32 / 255.0;
        if let Err(e) = self.blit(&f.dst, rect_or_full(f.rect, w, h), None, [ch(16), ch(8), ch(0), ch(24)]) {
            self.errors.push(format!("ColorFill: {e}"));
        }
    }

    fn read_back(&mut self, t: &Target) -> Option<Vec<u8>> {
        // D3D9 cannot read a multisampled target back (GetRenderTargetData
        // fails; it is resolved with StretchRect first)
        if t.samples != 0 {
            return None;
        }
        self.flush();
        let (tex, level, layer, format, w, h) = self.target_texture(t)?;
        let bpp = format.block_copy_size(None)?;
        let row = (w * bpp).next_multiple_of(256);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level, origin: wgpu::Origin3d { x: 0, y: 0, z: layer }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.submit_encoder(enc);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = buffer.slice(..).get_mapped_range();
        // BGRA8 is D3D9's A8R8G8B8 byte order already; float formats match
        let mut out = Vec::with_capacity((w * h * bpp) as usize);
        for y in 0..h {
            out.extend_from_slice(&data[(y * row) as usize..(y * row + w * bpp) as usize]);
        }
        Some(out)
    }

    fn upload(&mut self, upload: Upload) {
        // recorded draws read the old contents: submit them first
        let resource = match &upload {
            Upload::Buffer { resource, .. } | Upload::Image { resource, .. } => *resource,
        };
        if self.open_uses.borrow().contains(&resource) {
            self.flush();
        }
        match &upload {
            Upload::Buffer { resource, bytes } => {
                if self.native_sim {
                    if bytes.len() <= 512 {
                        self.small_buffers.insert(*resource, bytes.clone());
                    } else {
                        self.small_buffers.remove(resource);
                    }
                }
                // a buffer of the same size is written in place
                let size = (bytes.len() as u64).next_multiple_of(4).max(4);
                match self.buffers.get(resource) {
                    Some(b) if b.size() == size => {
                        let mut padded = bytes.clone();
                        padded.resize(size as usize, 0);
                        self.wait_finisher();
                        self.queue.write_buffer(b, 0, &padded);
                    }
                    _ => {
                        let b = self.inline_buffer(bytes, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX);
                        self.buffers.insert(*resource, b);
                    }
                }
            }
            Upload::Image { resource, .. } => {
                if self.textures.contains_key(resource) {
                    self.write_image(&upload);
                } else if self.surfaces.contains_key(resource) {
                    // a lockable render target written by the CPU: not yet
                    self.errors.push(format!("upload into render target {resource:#x} ignored"));
                } else {
                    self.pending.entry(*resource).or_default().push(upload);
                }
            }
        }
    }

    fn draw(&mut self, draw: &DrawCall) {
        if self.native_sim && self.try_quad(draw) {
            return;
        }
        if let Err(e) = self.try_draw(draw) {
            if self.errors.len() < 1000 {
                self.errors.push(e);
            }
        }
    }

    fn clear(&mut self, c: &ClearCall) {
        // L80: a whole-target clear is a pass of its own recorded into the
        // open encoder after the draws before it (the GPU runs them in that
        // order), not two submissions per clear
        self.end_pass();
        let views: Vec<Option<(wgpu::TextureView, wgpu::TextureFormat)>> = if c.flags & 1 != 0 {
            c.targets.iter().map(|t| t.as_ref().and_then(|t| self.target_view(t, c.srgb))).collect()
        } else {
            vec![]
        };
        let depth = if c.flags & 6 != 0 { c.depth.as_ref().and_then(|t| self.target_view(t, false)) } else { None };
        // D3D9 clears the viewport, each rectangle within it, within the
        // scissor rectangle when the scissor test is on (ClearCall)
        let size = c.targets.iter().flatten().chain(c.depth.iter()).next().map(|t| (t.width as i32, t.height as i32)).unwrap_or((0, 0));
        let regions = clear_regions(c, size);
        let whole = regions.len() == 1 && regions[0] == [0, 0, size.0, size.1];
        if !whole {
            if !regions.is_empty() {
                self.flush();
                self.clear_regions_with_pipeline(c, &regions);
            }
            return;
        }
        {
            let mut uses = self.open_uses.borrow_mut();
            uses.extend(c.targets.iter().chain(std::iter::once(&c.depth)).flatten().filter_map(|t| t.texture.map(|x| x.resource)));
        }
        let mut enc = self.open.borrow_mut().take().unwrap_or_else(|| self.device.create_command_encoder(&Default::default()));
        {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = views.iter().map(|v| v.as_ref().map(|(view, f)| wgpu::RenderPassColorAttachment {
                view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear_colour(c.colour, *f)), store: wgpu::StoreOp::Store },
            })).collect();
            let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &attachments,
                depth_stencil_attachment: depth.as_ref().map(|(view, f)| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: (c.flags & 2 != 0).then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(c.z), store: wgpu::StoreOp::Store }),
                    stencil_ops: (c.flags & 4 != 0 && f.has_stencil_aspect()).then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(c.stencil), store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        *self.open.borrow_mut() = Some(enc);
        // a clear counts like a draw toward the submit limit, so the open
        // encoder stays bounded when frames clear without a Present
        self.open_draws.set(self.open_draws.get() + 1);
        if self.open_draws.get() >= self.draws_per_submit {
            self.flush_async();
        }
    }

    fn query_begin(&mut self, query: u32) {
        self.drop_query(query);
        self.query_slots.insert(query, Vec::new());
        if !self.active_queries.contains(&query) {
            self.active_queries.push(query);
        }
    }

    fn query_end(&mut self, query: u32) {
        self.active_queries.retain(|&q| q != query);
        self.query_slots.entry(query).or_default();
    }

    fn query_result(&mut self, query: u32) -> Option<u64> {
        if let Some(&n) = self.query_counts.get(&query) {
            return Some(n);
        }
        if !self.async_queries {
            let slots = self.query_slots.get(&query)?.clone();
            let n = self.resolve(&slots)?;
            return Some(self.store_count(query, n));
        }
        // L120: the GPU is never waited for here (it stalled the backend
        // worker for each of a gameplay frame's six occlusion queries:
        // query_result was 26% of the worker, combo5). None until the
        // read-back is mapped; the caller asks again (GetData's S_FALSE).
        if !self.pending_queries.contains_key(&query) {
            let slots = self.query_slots.get(&query)?.clone();
            if slots.is_empty() {
                return Some(self.store_count(query, 0));
            }
            let (read, state) = self.start_resolve(&slots);
            self.pending_queries.insert(query, (read, state, slots.len()));
        }
        let _ = self.device.poll(wgpu::PollType::Poll);
        let (read, state, n) = {
            let e = self.pending_queries.get(&query)?;
            (e.0.clone(), e.1.clone(), e.2)
        };
        match state.load(std::sync::atomic::Ordering::Acquire) {
            0 => None,
            1 => {
                let count = {
                    let data = read.slice(..).get_mapped_range();
                    (0..n).map(|k| u64::from_le_bytes(data[256 * k..256 * k + 8].try_into().unwrap())).sum()
                };
                read.unmap();
                Some(self.store_count(query, count))
            }
            _ => {
                // the map failed: resolve again on the next ask
                self.pending_queries.remove(&query);
                None
            }
        }
    }

    fn gamma(&mut self, ramp: Option<&GammaRamp>) {
        self.gamma = ramp.map(|r| Box::new(*r));
    }

    fn release(&mut self, resource: u32) {
        self.flush();
        self.small_buffers.remove(&resource);
        self.quads.forget(resource);
        // Guest/resource identities may now be reused: the native worker
        // forgets its resources if it holds this one (a target, an imported
        // buffer or a sampled texture), keeping its pipelines (L141).
        if let Some(worker) = self.native_worker.as_mut() {
            let tex = self.textures.get(&resource);
            let held = self.surfaces.contains_key(&resource)
                || tex.is_some_and(|t| t.target)
                || worker.holds(self.buffers.get(&resource), tex.map(|t| &t.texture));
            if held {
                worker.reset_resources();
                self.native_stats.resource_resets += 1;
            }
        }
        // cached views and bind groups may hold the resource
        self.tex_groups.clear();
        if let Some(vk) = &mut self.vk { vk.release_resources(); }
        self.last_group = Default::default();
        self.target_views.clear();
        // memo entries hold target views (L99)
        self.memo.clear();
        self.textures.remove(&resource);
        self.pending.remove(&resource);
        self.buffers.remove(&resource);
        self.surfaces.remove(&resource);
        self.active_queries.retain(|&q| q != resource);
        self.drop_query(resource);
    }

    fn present(&mut self, back_buffer: &Target) {
        if self.vk.is_some() { vk::report(); }
        self.poll_native_comparisons_internal();
        pipeline_log::present();
        self.flush();
        if let Some(p) = &self.pipeline_cache {
            p.save_if_due();
        }
        if self.read_frames || self.on_present.is_some() {
            self.last_frame = self.read_surface(back_buffer.surface);
            // a full-screen gamma ramp maps each 8-bit channel through its
            // table (the high byte of the 16-bit entry)
            if let (Some(r), Some(f)) = (self.gamma.as_ref(), self.last_frame.as_mut()) {
                for px in f.rgba.chunks_exact_mut(4) {
                    for c in 0..3 {
                        px[c] = (r[c][px[c] as usize] >> 8) as u8;
                    }
                }
            }
        }
        if let (Some(f), Some(frame)) = (self.on_present.as_mut(), self.last_frame.as_ref()) {
            f(frame);
        }
        if let (Some(sink), Some(rt)) = (self.present_sink.as_mut(), self.surfaces.get(&back_buffer.surface)) {
            sink.present(&Presented {
                device: &self.device, queue: &self.queue, texture: &rt.texture, format: rt.format,
                width: rt.width, height: rt.height, gamma: self.gamma.as_deref(),
            });
        }
        self.encode_profile.present();
    }
}
