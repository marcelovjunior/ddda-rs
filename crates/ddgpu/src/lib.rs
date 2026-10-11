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
//! First version: one render pass per draw and per clear, per-draw
//! uniform buffers. Correctness first; batching comes after the draws are
//! checked against the D3D9 driver.
use d3d9cmd::{ClearCall, CopyCall, DrawCall, FillCall, GammaRamp, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};
use std::collections::HashMap;

pub mod present;
use std::future::Future;

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
}

struct Rt {
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
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
    device: wgpu::Device,
    queue: wgpu::Queue,
    textures: HashMap<u32, Tex>,
    /// image uploads for textures not created yet: resource -> (index, upload)
    pending: HashMap<u32, Vec<Upload>>,
    buffers: HashMap<u32, wgpu::Buffer>,
    /// standalone surfaces used as targets (back buffers, render targets,
    /// depth-stencil)
    surfaces: HashMap<u32, Rt>,
    modules: HashMap<(u64, u64), (wgpu::ShaderModule, d3d9wgsl::Interface)>,
    samplers: HashMap<[u32; 14], wgpu::Sampler>,
    blits: HashMap<wgpu::TextureFormat, (wgpu::RenderPipeline, wgpu::BindGroupLayout)>,
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
    /// guest queries between D3DISSUE_BEGIN and D3DISSUE_END
    active_queries: Vec<u32>,
    gamma: Option<Box<GammaRamp>>,
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

impl WgpuBackend {
    /// The device features the backend needs: hosts that create the
    /// device themselves (a window sharing it with its surface) request
    /// these.
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::TEXTURE_COMPRESSION_BC
    }

    /// A backend on the default adapter without a window.
    pub fn headless() -> Option<WgpuBackend> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let needed = Self::required_features();
        if !adapter.features().contains(needed) {
            return None;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: needed,
            ..Default::default()
        })).ok()?;
        Some(Self::new(device, queue))
    }

    /// A backend on a device the host created (with
    /// [`required_features`](Self::required_features)), e.g. the one a
    /// window's surface is configured with, so presenting needs no copy
    /// between devices. `read_frames` starts on, as in headless mode; a
    /// host that shows frames through a [`PresentSink`] turns it off.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> WgpuBackend {
        WgpuBackend {
            device,
            queue,
            textures: HashMap::new(),
            pending: HashMap::new(),
            buffers: HashMap::new(),
            surfaces: HashMap::new(),
            modules: HashMap::new(),
            samplers: HashMap::new(),
            blits: HashMap::new(),
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
            active_queries: Vec::new(),
            gamma: None,
        }
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
    }

    /// The samples the draws in `slots` passed, from the GPU.
    fn resolve(&self, slots: &[u32]) -> Option<u64> {
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
        self.queue.submit([enc.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = read.slice(..).get_mapped_range();
        Some((0..slots.len()).map(|k| u64::from_le_bytes(data[256 * k..256 * k + 8].try_into().unwrap())).sum())
    }

    fn texture_for(&mut self, b: &TextureBinding) -> Option<&Tex> {
        if !self.textures.contains_key(&b.resource) {
            let (format, _, _, block) = sampled_format(b.format)?;
            let (layers, dimension) = match b.kind {
                TexKind::Flat => (1, wgpu::TextureDimension::D2),
                TexKind::Cube => (6, wgpu::TextureDimension::D2),
                TexKind::Volume => (b.depth, wgpu::TextureDimension::D3),
            };
            let size = wgpu::Extent3d {
                width: b.width.next_multiple_of(block),
                height: b.height.next_multiple_of(block),
                depth_or_array_layers: layers,
            };
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: b.levels,
                sample_count: 1,
                dimension,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            self.textures.insert(b.resource, Tex {
                texture, kind: b.kind, format: b.format, gpu_format: format, levels: b.levels,
                width: b.width, height: b.height, depth: b.depth,
            });
            for u in self.pending.remove(&b.resource).unwrap_or_default() {
                self.write_image(&u);
            }
        }
        self.textures.get(&b.resource)
    }

    /// Write an image upload into its (existing) texture.
    fn write_image(&mut self, u: &Upload) {
        let Upload::Image { resource, index, format, width, height, depth, pitch, slice: _, bytes } = u else { return };
        let Some(t) = self.textures.get(resource) else { return };
        let Some((_, conv, bpp, block)) = sampled_format(*format) else { return };
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
        if let Some(tex) = t.texture {
            let tx = self.textures.get(&tex)?;
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

    fn blit_pipeline(&mut self, format: wgpu::TextureFormat) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        if let Some(p) = self.blits.get(&format) {
            return p.clone();
        }
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT.into()) });
        let layout = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: None,
        });
        self.blits.insert(format, (pipeline.clone(), layout.clone()));
        (pipeline, layout)
    }

    /// Draw the blit quad into `rect` of a target (sampling `src` or filling
    /// with `colour`).
    #[allow(clippy::too_many_arguments)]
    fn blit(&mut self, dst: &Target, rect: [i32; 4], src: Option<(&wgpu::TextureView, [f32; 4], bool)>, colour: [f32; 4]) -> Result<(), String> {
        let (dtex, level, layer, format, w, h) = self.target_texture(dst).ok_or("unknown destination")?;
        let (pipeline, layout) = self.blit_pipeline(format);
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
        let mut enc = self.device.create_command_encoder(&Default::default());
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
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// The texture a target renders into: a standalone surface's own, or a
    /// render-target texture's level.
    fn target_view(&mut self, t: &Target, srgb: bool) -> Option<(wgpu::TextureView, wgpu::TextureFormat)> {
        if let Some(tex) = t.texture {
            let tx = self.textures.get(&tex)?;
            let (face, level) = match tx.kind {
                TexKind::Cube => (t.level / tx.levels, t.level % tx.levels),
                _ => (0, t.level),
            };
            let view = tx.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_mip_level: level,
                mip_level_count: Some(1),
                base_array_layer: face,
                array_layer_count: Some(1),
                ..Default::default()
            });
            return Some((view, tx.gpu_format));
        }
        let format = target_format(t.format, false)?;
        let rt = self.surfaces.entry(t.surface).or_insert_with(|| {
            let srgb_view = match format {
                wgpu::TextureFormat::Bgra8Unorm => vec![wgpu::TextureFormat::Bgra8UnormSrgb],
                wgpu::TextureFormat::Rgba8Unorm => vec![wgpu::TextureFormat::Rgba8UnormSrgb],
                _ => vec![],
            };
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &srgb_view,
            });
            Rt { texture, format, width: t.width, height: t.height }
        });
        let view_format = if srgb { target_format(t.format, true).unwrap_or(rt.format) } else { rt.format };
        let view = rt.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() });
        Some((view, view_format))
    }

    fn sampler(&mut self, states: &[u32; 14]) -> wgpu::Sampler {
        if let Some(s) = self.samplers.get(states) {
            return s.clone();
        }
        let address = |v: u32| match v {
            2 => wgpu::AddressMode::MirrorRepeat,
            3 | 5 => wgpu::AddressMode::ClampToEdge,
            4 => wgpu::AddressMode::ClampToBorder,
            _ => wgpu::AddressMode::Repeat,
        };
        let filter = |v: u32| if v >= 2 { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        let mip = |v: u32| if v >= 2 { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest };
        let s = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: None,
            address_mode_u: address(states[1]),
            address_mode_v: address(states[2]),
            address_mode_w: address(states[3]),
            mag_filter: filter(states[5]),
            min_filter: filter(states[6]),
            mipmap_filter: mip(states[7]),
            // D3DSAMP_MIPFILTER NONE samples level 0 only; MAXMIPLEVEL is the
            // most detailed level used
            lod_min_clamp: states[9] as f32,
            lod_max_clamp: if states[7] == 0 { states[9] as f32 } else { 32.0 },
            compare: None,
            anisotropy_clamp: 1,
            border_color: if address(states[1]) == wgpu::AddressMode::ClampToBorder { Some(wgpu::SamplerBorderColor::TransparentBlack) } else { None },
        });
        self.samplers.insert(*states, s.clone());
        s
    }

    fn module(&mut self, tokens: &[u32], options: d3d9wgsl::Options, key: (u64, u64)) -> Result<(wgpu::ShaderModule, d3d9wgsl::Interface), String> {
        if let Some((m, i)) = self.modules.get(&key) {
            return Ok((m.clone(), i.clone()));
        }
        let shader = d3d9shader::parse(tokens).map_err(|e| e.to_string())?;
        let t = d3d9wgsl::translate_with(&shader, options).map_err(|e| e.to_string())?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(t.wgsl.into()) });
        if let Some(e) = block_on(scope.pop()) {
            return Err(format!("{e}"));
        }
        self.modules.insert(key, (module.clone(), t.interface.clone()));
        Ok((module, t.interface))
    }

    fn buffer(&mut self, resource: u32) -> Option<wgpu::Buffer> {
        self.buffers.get(&resource).cloned()
    }

    fn inline_buffer(&self, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let size = (bytes.len() as u64).next_multiple_of(4).max(4);
        let b = self.device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut padded = bytes.to_vec();
        padded.resize(size as usize, 0);
        self.queue.write_buffer(&b, 0, &padded);
        b
    }

    fn uniform(&self, c: &d3d9cmd::Constants, floats: usize, last: [u32; 4]) -> wgpu::Buffer {
        let mut bytes = Vec::with_capacity(floats * 16 + 528);
        for k in 0..floats {
            for w in c.f.get(k).copied().unwrap_or([0; 4]) {
                bytes.extend_from_slice(&w.to_le_bytes());
            }
        }
        for v in c.i {
            for w in v {
                bytes.extend_from_slice(&w.to_le_bytes());
            }
        }
        for b in c.b {
            for w in [b, 0, 0, 0] {
                bytes.extend_from_slice(&w.to_le_bytes());
            }
        }
        for w in last {
            bytes.extend_from_slice(&w.to_le_bytes());
        }
        self.inline_buffer(&bytes, wgpu::BufferUsages::UNIFORM)
    }

    /// Read a standalone target back as RGBA8 (BGRA formats swizzled).
    pub fn read_surface(&self, surface: u32) -> Option<Frame> {
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
        self.queue.submit([enc.finish()]);
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

    fn try_draw(&mut self, d: &DrawCall) -> Result<(), String> {
        let state = |s: u32| d.render_states.binary_search_by_key(&s, |&(k, _)| k).ok().map(|i| d.render_states[i].1).unwrap_or(0);
        let vs_tokens = d.vertex_shader.as_ref().ok_or("draws without a vertex shader (fixed function) are not implemented")?;
        let ps_tokens = d.pixel_shader.as_ref().ok_or("draws without a pixel shader (fixed function) are not implemented")?;
        if d.declaration.is_empty() {
            return Err("FVF draws are not implemented".into());
        }
        let hash = |t: &[u32]| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.hash(&mut h);
            h.finish()
        };
        let (vh, ph) = (hash(vs_tokens), hash(ps_tokens));
        // vertex inputs: each declared input's element
        let vs_shader = d3d9shader::parse(vs_tokens).map_err(|e| e.to_string())?;
        let mut vs_inputs = Vec::new();
        for i in vs_shader.instructions.iter().filter(|i| i.opcode == d3d9shader::op::DCL) {
            if let (Some(dst), Some(t)) = (&i.dst, i.decl) {
                if dst.kind == d3d9shader::reg::INPUT {
                    vs_inputs.push((t & 0x1F, (t >> 16) & 0xF));
                }
            }
        }
        let mut types = Vec::new();
        let mut elements = Vec::new();
        for (k, &(usage, index)) in vs_inputs.iter().enumerate() {
            let e = d.declaration.iter().find(|e| e[6] as u32 == usage && e[7] as u32 == index)
                .ok_or_else(|| format!("vertex input usage {usage} index {index} has no declaration element"))?;
            let (format, ty) = vertex_format(e[4]).ok_or_else(|| format!("vertex element type {}", e[4]))?;
            types.push(ty);
            elements.push((u16::from_le_bytes([e[0], e[1]]) as u32, u16::from_le_bytes([e[2], e[3]]) as u64, format, k as u32));
        }
        let (vs_module, vs_iface) = self.module(vs_tokens, d3d9wgsl::Options { vs_input_types: Some(types), half_pixel: true, ..Default::default() }, (vh, 0))?;
        // link the pixel inputs to the vertex outputs
        let ps_shader = d3d9shader::parse(ps_tokens).map_err(|e| e.to_string())?;
        let mut locations = Vec::new();
        for i in ps_shader.instructions.iter().filter(|i| i.opcode == d3d9shader::op::DCL) {
            if let (Some(dst), Some(t)) = (&i.dst, i.decl) {
                if dst.kind == d3d9shader::reg::INPUT {
                    let (u, n) = (t & 0x1F, (t >> 16) & 0xF);
                    locations.push(vs_iface.outputs_vs.iter().position(|o| o.0 == u && o.1 == n).map(|p| p as u32));
                }
            }
        }
        let (ps_module, ps_iface) = self.module(ps_tokens, d3d9wgsl::Options { ps_input_locations: Some(locations), alpha_test: true, ..Default::default() }, (ph, vh))?;

        // targets
        let srgb = state(194) != 0;
        let mut colour: Vec<Option<(wgpu::TextureView, wgpu::TextureFormat)>> = Vec::new();
        for (k, t) in d.targets.iter().enumerate() {
            colour.push(match t {
                Some(t) if ps_iface.outputs.contains(&(k as u32)) || k == 0 => self.target_view(t, srgb && k == 0),
                _ => None,
            });
        }
        let depth = match &d.depth {
            Some(t) if state(7) != 0 => self.target_view(t, false),
            _ => None,
        };
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
        let targets: Vec<Option<wgpu::ColorTargetState>> = (0..max_out).map(|k| colour[k].as_ref().map(|(_, f)| wgpu::ColorTargetState {
            format: *f,
            blend: if f.is_srgb() || matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba16Float) { blend } else { None },
            write_mask: wgpu::ColorWrites::from_bits_truncate(state(writes[k])),
        })).collect();
        let depth_state = depth.as_ref().map(|(_, f)| wgpu::DepthStencilState {
            format: *f,
            depth_write_enabled: Some(state(14) != 0),
            depth_compare: Some(compare(state(23))),
            stencil: if state(52) != 0 && f.has_stencil_aspect() {
                let face = wgpu::StencilFaceState {
                    compare: compare(state(56)),
                    fail_op: stencil_op(state(53)),
                    depth_fail_op: stencil_op(state(54)),
                    pass_op: stencil_op(state(55)),
                };
                wgpu::StencilState { front: face, back: face, read_mask: state(58), write_mask: state(59) }
            } else {
                wgpu::StencilState::default()
            },
            bias: wgpu::DepthBiasState { constant: 0, slope_scale: f32::from_bits(state(175)), clamp: 0.0 },
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
        let mut attrs: Vec<Vec<wgpu::VertexAttribute>> = Vec::new();
        let mut strides = Vec::new();
        let mut steps = Vec::new();
        let (instances, instanced) = instancing(d)?;
        for &s in &streams {
            attrs.push(elements.iter().filter(|e| e.0 == s).map(|e| wgpu::VertexAttribute { format: e.2, offset: e.1, shader_location: e.3 }).collect());
            strides.push(stride_of(s)? as u64);
            steps.push(if instanced.contains(&s) { wgpu::VertexStepMode::Instance } else { wgpu::VertexStepMode::Vertex });
        }
        let layouts: Vec<wgpu::VertexBufferLayout> = attrs.iter().zip(&strides).zip(&steps)
            .map(|((a, &st), &step_mode)| wgpu::VertexBufferLayout { array_stride: st, step_mode, attributes: a })
            .collect();
        // bind group layouts: 0 = vertex constants (binding 2) and pixel
        // constants (binding 0); 1 = pixel textures; 2 = vertex textures
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding, visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout0 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[uniform(0, wgpu::ShaderStages::FRAGMENT), uniform(2, wgpu::ShaderStages::VERTEX)],
        });
        let tex_entries = |iface: &d3d9wgsl::Interface, visibility: wgpu::ShaderStages| -> Vec<wgpu::BindGroupLayoutEntry> {
            let mut v = Vec::new();
            for &(n, kind) in &iface.samplers {
                let view_dimension = match kind {
                    d3d9wgsl::TextureKind::Flat => wgpu::TextureViewDimension::D2,
                    d3d9wgsl::TextureKind::Cube => wgpu::TextureViewDimension::Cube,
                    d3d9wgsl::TextureKind::Volume => wgpu::TextureViewDimension::D3,
                };
                v.push(wgpu::BindGroupLayoutEntry { binding: 2 * n, visibility, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false }, count: None });
                v.push(wgpu::BindGroupLayoutEntry { binding: 2 * n + 1, visibility, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None });
            }
            v
        };
        let layout1 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &tex_entries(&ps_iface, wgpu::ShaderStages::FRAGMENT) });
        let layout2 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &tex_entries(&vs_iface, wgpu::ShaderStages::VERTEX) });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout0), Some(&layout1), Some(&layout2)],
            immediate_size: 0,
        });
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &vs_module, entry_point: Some("main"), compilation_options: Default::default(), buffers: &layouts },
            primitive: wgpu::PrimitiveState { topology, front_face: wgpu::FrontFace::Cw, cull_mode: cull, ..Default::default() },
            depth_stencil: depth_state,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState { module: &ps_module, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = block_on(scope.pop()) {
            return Err(format!("pipeline: {e}"));
        }
        // uniforms: half-pixel offset from the viewport; alpha test from the states
        let (vw, vh_) = (d.viewport[2].max(1) as f32, d.viewport[3].max(1) as f32);
        // D3D9 samples pixels at their integer corner, wgpu at the centre:
        // the geometry moves half a pixel right and down
        let hp = [(1.0 / vw).to_bits(), (-1.0 / vh_).to_bits(), 0, 0];
        let vsu = self.uniform(&d.vs_constants, 256, hp);
        let psu = self.uniform(&d.ps_constants, 224, [state(15), state(25), state(24) & 0xFF, 0]);
        let group0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout0,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: psu.as_entire_binding() }, wgpu::BindGroupEntry { binding: 2, resource: vsu.as_entire_binding() }],
        });
        let mut groups = Vec::new();
        for (iface, layout, base) in [(&ps_iface, &layout1, 0u32), (&vs_iface, &layout2, 257u32)] {
            let mut views = Vec::new();
            for &(n, kind) in &iface.samplers {
                let binding = d.textures.iter().find(|t| t.sampler == base + n);
                let view_dimension = match kind {
                    d3d9wgsl::TextureKind::Flat => wgpu::TextureViewDimension::D2,
                    d3d9wgsl::TextureKind::Cube => wgpu::TextureViewDimension::Cube,
                    d3d9wgsl::TextureKind::Volume => wgpu::TextureViewDimension::D3,
                };
                let (view, states) = match binding {
                    Some(b) => {
                        let srgb_read = b.states[11] != 0;
                        let tx = self.texture_for(b).ok_or_else(|| format!("texture format {:#x} is not supported", b.format))?;
                        let format = if srgb_read { tx.gpu_format.add_srgb_suffix() } else { tx.gpu_format };
                        (tx.texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(view_dimension), format: Some(format), ..Default::default() }), b.states)
                    }
                    None => return Err(format!("sampler s{n} has no texture bound")),
                };
                views.push((n, view, self.sampler(&states)));
            }
            let mut entries = Vec::new();
            for (n, v, s) in &views {
                entries.push(wgpu::BindGroupEntry { binding: 2 * n, resource: wgpu::BindingResource::TextureView(v) });
                entries.push(wgpu::BindGroupEntry { binding: 2 * n + 1, resource: wgpu::BindingResource::Sampler(s) });
            }
            groups.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries }));
        }
        // vertex and index data
        let mut vbufs = Vec::new();
        for &s in &streams {
            match &d.vertices {
                Vertices::Streams(v) => {
                    let (_, res, off, _, _) = *v.iter().find(|x| x.0 == s).unwrap();
                    let b = self.buffer(res).ok_or_else(|| format!("vertex buffer {res:#x} has no data"))?;
                    vbufs.push((b, off as u64));
                }
                Vertices::Inline { bytes, .. } => vbufs.push((self.inline_buffer(bytes, wgpu::BufferUsages::VERTEX), 0)),
            }
        }
        let fan = d.primitive == 6;
        let (index, format, first) = match &d.indices {
            Indices::None if fan => {
                let idx: Vec<u32> = (1..=d.primitives).flat_map(|k| [d.start, d.start + k, d.start + k + 1]).collect();
                (Some(self.inline_buffer(&idx.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>(), wgpu::BufferUsages::INDEX)), wgpu::IndexFormat::Uint32, 0)
            }
            Indices::None => (None, wgpu::IndexFormat::Uint16, d.start),
            Indices::Buffer(res, f) => {
                if fan {
                    return Err("indexed triangle fans are not implemented".into());
                }
                (Some(self.buffer(*res).ok_or("index buffer has no data")?), if *f == 102 { wgpu::IndexFormat::Uint32 } else { wgpu::IndexFormat::Uint16 }, d.start)
            }
            Indices::Inline { bytes, format } => {
                if fan {
                    return Err("indexed triangle fans are not implemented".into());
                }
                (Some(self.inline_buffer(bytes, wgpu::BufferUsages::INDEX)), if *format == 102 { wgpu::IndexFormat::Uint32 } else { wgpu::IndexFormat::Uint16 }, 0)
            }
        };
        let count = if fan { 3 * d.primitives } else { match d.primitive {
            1 => d.primitives,
            2 => 2 * d.primitives,
            3 => d.primitives + 1,
            4 => 3 * d.primitives,
            _ => d.primitives + 2,
        } };
        // a draw made while guest occlusion queries count gets its own slot
        let slot = (!self.active_queries.is_empty()).then(|| self.occlusion_slot());
        if let Some(s) = slot {
            self.slot_users.insert(s, self.active_queries.len() as u32);
            for q in &self.active_queries {
                self.query_slots.entry(*q).or_default().push(s);
            }
        }
        let query_set = slot.map(|s| self.occlusion_sets[(s / OCCLUSION_SET) as usize].clone());
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = colour[..max_out].iter().map(|c| c.as_ref().map(|(view, _)| wgpu::RenderPassColorAttachment {
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
                occlusion_query_set: query_set.as_ref(),
                multiview_mask: None,
            });
            if let Some(s) = slot {
                pass.begin_occlusion_query(s % OCCLUSION_SET);
            }
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group0, &[]);
            pass.set_bind_group(1, &groups[0], &[]);
            pass.set_bind_group(2, &groups[1], &[]);
            let v = d.viewport;
            pass.set_viewport(v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32, f32::from_bits(v[4]), f32::from_bits(v[5]));
            if state(174) != 0 {
                let s = d.scissor;
                pass.set_scissor_rect(s[0].max(0) as u32, s[1].max(0) as u32, (s[2] - s[0]).max(0) as u32, (s[3] - s[1]).max(0) as u32);
            }
            pass.set_stencil_reference(state(57));
            pass.set_blend_constant(colour_of(state(193)));
            for (k, (b, off)) in vbufs.iter().enumerate() {
                pass.set_vertex_buffer(k as u32, b.slice(off..));
            }
            match index {
                Some(ib) => {
                    pass.set_index_buffer(ib.slice(..), format);
                    pass.draw_indexed(first..first + count, if fan { 0 } else { d.base_vertex }, 0..instances);
                }
                None => pass.draw(first..first + count, 0..instances),
            }
            if slot.is_some() {
                pass.end_occlusion_query();
            }
        }
        self.queue.submit([enc.finish()]);
        Ok(())
    }
}

/// D3D9 hardware instancing (SetStreamSourceFreq): stream 0 set to
/// D3DSTREAMSOURCE_INDEXEDDATA | n draws indexed geometry n times; streams
/// set to D3DSTREAMSOURCE_INSTANCEDATA | 1 advance once per instance.
/// Measured on the driver (ddgpu tests, instanced_draws_match_the_d3d9_driver):
/// a non-indexed draw ignores the count and draws once, its instance-data
/// streams still giving every vertex instance 0's element. Returns the
/// instance count and the instanced streams.
fn instancing(d: &DrawCall) -> Result<(u32, Vec<u32>), String> {
    const INDEXEDDATA: u32 = 0x4000_0000;
    const INSTANCEDATA: u32 = 0x8000_0000;
    let Vertices::Streams(streams) = &d.vertices else { return Ok((1, vec![])) };
    let geometry = streams.iter().find(|s| s.0 == 0).map(|s| s.4).unwrap_or(1);
    if geometry & INDEXEDDATA == 0 {
        return Ok((1, vec![]));
    }
    let mut instanced = Vec::new();
    for s in streams {
        if s.4 & INSTANCEDATA != 0 {
            if s.4 & 0x3FFF_FFFF != 1 {
                return Err(format!("instance data divisor {} on stream {} (wgpu steps every instance)", s.4 & 0x3FFF_FFFF, s.0));
            }
            instanced.push(s.0);
        }
    }
    let count = if matches!(d.indices, Indices::None) { 1 } else { (geometry & 0x3FFF_FFFF).max(1) };
    Ok((count, instanced))
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

/// A D3DCOLOR (ARGB) as a wgpu colour.
fn colour_of(c: u32) -> wgpu::Color {
    let ch = |s: u32| ((c >> s) & 0xFF) as f64 / 255.0;
    wgpu::Color { r: ch(16), g: ch(8), b: ch(0), a: ch(24) }
}

/// A rectangle, or the whole `w` x `h` target when D3D9 passed NULL (all zero).
fn rect_or_full(r: [i32; 4], w: u32, h: u32) -> [i32; 4] {
    if r == [0; 4] { [0, 0, w as i32, h as i32] } else { r }
}

impl GraphicsBackend for WgpuBackend {
    fn copy(&mut self, c: &CopyCall) {
        let Some((stex, slevel, slayer, sformat, sw, sh)) = self.target_texture(&c.src) else {
            self.errors.push("StretchRect from an unknown surface".into());
            return;
        };
        let Some((dtex, dlevel, dlayer, dformat, dw, dh)) = self.target_texture(&c.dst) else {
            self.errors.push("StretchRect to an unknown surface".into());
            return;
        };
        let (sr, dr) = (rect_or_full(c.src_rect, sw, sh), rect_or_full(c.dst_rect, dw, dh));
        let same_size = sr[2] - sr[0] == dr[2] - dr[0] && sr[3] - sr[1] == dr[3] - dr[1];
        if same_size && sformat == dformat && sr[0] >= 0 && sr[1] >= 0 && dr[0] >= 0 && dr[1] >= 0 {
            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &stex, mip_level: slevel, origin: wgpu::Origin3d { x: sr[0] as u32, y: sr[1] as u32, z: slayer }, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &dtex, mip_level: dlevel, origin: wgpu::Origin3d { x: dr[0] as u32, y: dr[1] as u32, z: dlayer }, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: (sr[2] - sr[0]) as u32, height: (sr[3] - sr[1]) as u32, depth_or_array_layers: 1 },
            );
            self.queue.submit([enc.finish()]);
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
    }

    fn fill(&mut self, f: &FillCall) {
        let Some((_, _, _, _, w, h)) = self.target_texture(&f.dst) else { return };
        let ch = |s: u32| ((f.colour >> s) & 0xFF) as f32 / 255.0;
        if let Err(e) = self.blit(&f.dst, rect_or_full(f.rect, w, h), None, [ch(16), ch(8), ch(0), ch(24)]) {
            self.errors.push(format!("ColorFill: {e}"));
        }
    }

    fn read_back(&mut self, t: &Target) -> Option<Vec<u8>> {
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
        self.queue.submit([enc.finish()]);
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
        match &upload {
            Upload::Buffer { resource, bytes } => {
                let b = self.inline_buffer(bytes, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX);
                self.buffers.insert(*resource, b);
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
        if let Err(e) = self.try_draw(draw) {
            if self.errors.len() < 1000 {
                self.errors.push(e);
            }
        }
    }

    fn clear(&mut self, c: &ClearCall) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        let colour = colour_of(c.colour);
        let views: Vec<Option<(wgpu::TextureView, wgpu::TextureFormat)>> = if c.flags & 1 != 0 {
            c.targets.iter().map(|t| t.as_ref().and_then(|t| self.target_view(t, false))).collect()
        } else {
            vec![]
        };
        let depth = if c.flags & 6 != 0 { c.depth.as_ref().and_then(|t| self.target_view(t, false)) } else { None };
        if !c.rects.is_empty() {
            self.errors.push("Clear with rectangles is done as a full clear".into());
        }
        {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = views.iter().map(|v| v.as_ref().map(|(view, _)| wgpu::RenderPassColorAttachment {
                view, depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(colour), store: wgpu::StoreOp::Store },
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
        self.queue.submit([enc.finish()]);
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
        let slots = self.query_slots.get(&query)?.clone();
        let n = self.resolve(&slots)?;
        // the count stays; the slots are free for later draws
        self.drop_query(query);
        self.query_slots.insert(query, Vec::new());
        self.query_counts.insert(query, n);
        Some(n)
    }

    fn gamma(&mut self, ramp: Option<&GammaRamp>) {
        self.gamma = ramp.map(|r| Box::new(*r));
    }

    fn release(&mut self, resource: u32) {
        self.textures.remove(&resource);
        self.pending.remove(&resource);
        self.buffers.remove(&resource);
        self.surfaces.remove(&resource);
        self.active_queries.retain(|&q| q != resource);
        self.drop_query(resource);
    }

    fn present(&mut self, back_buffer: &Target) {
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
    }
}
