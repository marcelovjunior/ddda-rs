//! Immutable native rendering objects. This is a host API, not a decoding of
//! nDraw's guest descriptors (see docs/NDRAW.md). Creation is the cold path;
//! recorders hold small handles and never reconstruct D3D9 DrawCalls.
//!
//! Resource imports retain wgpu objects. Call `WgpuBackend::flush` through
//! `Device::from_backend` / the import methods before switching producers,
//! and submit native work before returning to the D3D9 backend. Imports are
//! snapshots: replacing/releasing a backend resource does not retarget a handle.
use crate::cache::{FxMap, PS_BLOCK, VS_BLOCK};
#[path = "native_uniforms.rs"]
mod native_uniforms;
use native_uniforms::Uniforms;
use std::{
    num::NonZeroU64,
    ops::Range,
    sync::atomic::{AtomicU32, Ordering},
};

macro_rules! handles {
    ($($name:ident),*) => {$ (
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name { owner: u32, index: u32 }
    )*};
}
handles!(
    Blend,
    Rasterizer,
    DepthStencil,
    Sampler,
    InputLayout,
    Shader,
    Texture,
    View,
    Buffer,
    Pipeline,
    TextureSet,
    ConstantBuffer
);

#[derive(Clone, Copy, Debug)]
pub struct BlendTargetDesc {
    pub enable: bool,
    pub colour: wgpu::BlendComponent,
    pub alpha: wgpu::BlendComponent,
    pub write_mask: wgpu::ColorWrites,
}
impl Default for BlendTargetDesc {
    fn default() -> Self {
        Self {
            enable: false,
            colour: wgpu::BlendComponent::REPLACE,
            alpha: wgpu::BlendComponent::REPLACE,
            write_mask: wgpu::ColorWrites::ALL,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct BlendDesc {
    pub targets: [BlendTargetDesc; 4],
    pub alpha_to_coverage: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RasterizerDesc {
    pub cull: Option<wgpu::Face>,
    pub fill: wgpu::PolygonMode,
    pub front_face: wgpu::FrontFace,
    /// D3D11/wgpu hardware bias units, not D3D9's floating depth offset.
    pub depth_bias: i32,
    pub slope_bias: f32,
    pub bias_clamp: f32,
    pub scissor_enable: bool,
}
impl Default for RasterizerDesc {
    fn default() -> Self {
        Self {
            cull: None,
            fill: wgpu::PolygonMode::Fill,
            front_face: wgpu::FrontFace::Cw,
            depth_bias: 0,
            slope_bias: 0.0,
            bias_clamp: 0.0,
            scissor_enable: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct DepthStencilDesc {
    pub depth_enable: bool,
    pub depth_write: bool,
    pub depth_func: wgpu::CompareFunction,
    pub stencil_enable: bool,
    pub front: wgpu::StencilFaceState,
    pub back: wgpu::StencilFaceState,
    pub read_mask: u32,
    pub write_mask: u32,
}
impl Default for DepthStencilDesc {
    fn default() -> Self {
        Self {
            depth_enable: false,
            depth_write: false,
            depth_func: wgpu::CompareFunction::LessEqual,
            stencil_enable: false,
            front: Default::default(),
            back: Default::default(),
            read_mask: !0,
            write_mask: !0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct SamplerDesc {
    pub min: wgpu::FilterMode,
    pub mag: wgpu::FilterMode,
    pub mip: wgpu::MipmapFilterMode,
    pub address: [wgpu::AddressMode; 3],
    /// Pixel-stage bias is supplied through translated shader constants;
    /// wgpu has no sampler LOD bias. Vertex-stage nonzero bias is rejected.
    pub lod_bias: f32,
    pub lod_min: f32,
    pub lod_max: f32,
    pub max_anisotropy: u16,
    pub border: Option<wgpu::SamplerBorderColor>,
}
impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            min: wgpu::FilterMode::Nearest,
            mag: wgpu::FilterMode::Nearest,
            mip: wgpu::MipmapFilterMode::Nearest,
            address: [wgpu::AddressMode::ClampToEdge; 3],
            lod_bias: 0.0,
            lod_min: 0.0,
            lod_max: 32.0,
            max_anisotropy: 1,
            border: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct InputElement {
    pub stream: u32,
    pub offset: u64,
    pub format: wgpu::VertexFormat,
    /// Semantic usage/index as declared in the shader bytecode.
    pub semantic: (u32, u32),
    pub location: u32,
}
#[derive(Clone, Debug)]
pub struct InputStream {
    pub stride: u64,
    pub step: wgpu::VertexStepMode,
}
#[derive(Clone, Debug)]
pub struct InputLayoutDesc {
    pub streams: Vec<InputStream>,
    pub elements: Vec<InputElement>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    Pixel,
    Vertex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PipelineDesc {
    pub vertex_shader: Shader,
    pub pixel_shader: Shader,
    pub input: InputLayout,
    pub blend: Blend,
    pub rasterizer: Rasterizer,
    pub depth_stencil: DepthStencil,
    pub colours: [Option<wgpu::TextureFormat>; 4],
    pub depth: Option<wgpu::TextureFormat>,
    pub samples: u32,
    pub sample_mask: u64,
    pub topology: wgpu::PrimitiveTopology,
    pub strip_index: Option<wgpu::IndexFormat>,
    /// The pixel shader's translation, as the D3D9 path's (crates/ddgpu
    /// derive): the channels (4 bits per pixel sampler) render-target
    /// textures lack, which D3D9 samples as 1; and D3D9's depth bias in the
    /// pixel shader (DEPTHBIAS / SLOPESCALEDEPTHBIAS in PS extra slot 5).
    pub sampler_ones: u64,
    pub depth_bias: bool,
}

struct ShaderObject {
    tokens: Vec<u32>,
    stage: Stage,
    compact: bool,
    constants: ConstantLayout,
}
struct SamplerObject {
    gpu: wgpu::Sampler,
    lod_bias: f32,
}
struct Module {
    gpu: wgpu::ShaderModule,
    interface: d3d9wgsl::Interface,
}
struct PipelineObject {
    gpu: wgpu::RenderPipeline,
    layouts: [wgpu::BindGroupLayout; 2],
    kinds: [Vec<(u32, d3d9wgsl::TextureKind)>; 2],
    desc: PipelineDesc,
    constants: [ConstantLayout; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureBinding {
    pub slot: u32,
    pub view: View,
    pub sampler: Sampler,
}
struct TextureSetObject {
    gpu: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    stage: Stage,
    bias: [u32; 16],
}
struct ViewObject {
    gpu: wgpu::TextureView,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    samples: u32,
}

/// Immutable device objects can be read by several independently owned
/// recorders. No implicit global player, encoder, pass, or constant state.
pub struct Device {
    id: u32,
    gpu: wgpu::Device,
    queue: wgpu::Queue,
    uniform_layout: wgpu::BindGroupLayout,
    blends: Vec<BlendDesc>,
    rasters: Vec<RasterizerDesc>,
    depths: Vec<DepthStencilDesc>,
    samplers: Vec<SamplerObject>,
    inputs: Vec<InputLayoutDesc>,
    shaders: Vec<ShaderObject>,
    modules: FxMap<(Shader, InputLayout, Option<Shader>, u64, bool), Module>,
    textures: Vec<wgpu::Texture>,
    views: Vec<ViewObject>,
    buffers: Vec<wgpu::Buffer>,
    pipelines: Vec<PipelineObject>,
    pipeline_cache: FxMap<PipelineDesc, Pipeline>,
    disk_cache: Option<std::sync::Arc<crate::pipelines::Persistent>>,
    sets: Vec<TextureSetObject>,
    texture_layouts: FxMap<(Stage, Vec<(u32, u8)>), wgpu::BindGroupLayout>,
    set_cache: FxMap<(wgpu::BindGroupLayout, Vec<TextureBinding>), TextureSet>,
}
fn owner() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .expect("native owner ids exhausted")
}
macro_rules! insert {
    ($self:ident, $field:ident, $handle:ident, $value:expr) => {{
        let index = u32::try_from($self.$field.len()).expect("native handle space exhausted");
        $self.$field.push($value);
        $handle {
            owner: $self.id,
            index,
        }
    }};
}
impl Device {
    pub fn from_backend(backend: &mut crate::WgpuBackend) -> Self {
        backend.flush();
        Self::new_cached(backend)
    }
    pub(crate) fn new_cached(backend: &crate::WgpuBackend) -> Self {
        let mut device = Self::new(backend.device.clone(), backend.queue.clone());
        if std::env::var("DDGPU_NATIVE_PIPELINE_CACHE").as_deref() == Ok("1") {
            device.disk_cache = backend.pipeline_cache.clone();
        }
        device
    }
    pub fn new(gpu: wgpu::Device, queue: wgpu::Queue) -> Self {
        let entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            count: None,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: None,
            },
        };
        let uniform_layout = gpu.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native constants"),
            entries: &[
                entry(0, wgpu::ShaderStages::FRAGMENT),
                entry(2, wgpu::ShaderStages::VERTEX),
            ],
        });
        Self {
            id: owner(),
            gpu,
            queue,
            uniform_layout,
            blends: vec![],
            rasters: vec![],
            depths: vec![],
            samplers: vec![],
            inputs: vec![],
            shaders: vec![],
            modules: Default::default(),
            textures: vec![],
            views: vec![],
            buffers: vec![],
            pipelines: vec![],
            pipeline_cache: Default::default(),
            disk_cache: None,
            sets: vec![],
            texture_layouts: Default::default(),
            set_cache: Default::default(),
        }
    }
    fn check(&self, id: u32) {
        assert_eq!(id, self.id, "handle belongs to another native device");
    }
    pub fn create_blend(&mut self, mut desc: BlendDesc) -> Blend {
        // MIN/MAX ignore factors in the descriptor vocabulary. wgpu requires
        // canonical One factors for those operations.
        for target in &mut desc.targets {
            for c in [&mut target.colour, &mut target.alpha] {
                if matches!(c.operation, wgpu::BlendOperation::Min | wgpu::BlendOperation::Max) {
                    c.src_factor = wgpu::BlendFactor::One;
                    c.dst_factor = wgpu::BlendFactor::One;
                }
            }
        }
        insert!(self, blends, Blend, desc)
    }
    pub fn create_rasterizer(&mut self, desc: RasterizerDesc) -> Rasterizer {
        insert!(self, rasters, Rasterizer, desc)
    }
    pub fn create_depth_stencil(&mut self, desc: DepthStencilDesc) -> DepthStencil {
        insert!(self, depths, DepthStencil, desc)
    }
    pub fn create_sampler(&mut self, desc: &SamplerDesc) -> Result<Sampler, String> {
        if !desc.lod_bias.is_finite() {
            return Err("non-finite sampler LOD bias".into());
        }
        let scope = self.gpu.push_error_scope(wgpu::ErrorFilter::Validation);
        let sampler = self.gpu.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("native sampler"),
            address_mode_u: desc.address[0],
            address_mode_v: desc.address[1],
            address_mode_w: desc.address[2],
            mag_filter: desc.mag,
            min_filter: desc.min,
            mipmap_filter: desc.mip,
            lod_min_clamp: desc.lod_min,
            lod_max_clamp: desc.lod_max,
            anisotropy_clamp: desc.max_anisotropy,
            border_color: desc.border,
            ..Default::default()
        });
        if let Some(e) = crate::block_on(scope.pop()) {
            return Err(e.to_string());
        }
        Ok(insert!(
            self,
            samplers,
            Sampler,
            SamplerObject {
                gpu: sampler,
                lod_bias: desc.lod_bias
            }
        ))
    }
    pub fn create_input_layout(&mut self, desc: InputLayoutDesc) -> Result<InputLayout, String> {
        if desc.streams.len() > 16
            || desc
                .elements
                .iter()
                .any(|e| e.stream as usize >= desc.streams.len())
        {
            return Err("native input stream out of range (maximum 16)".into());
        }
        for (n, e) in desc.elements.iter().enumerate() {
            if desc.elements[..n]
                .iter()
                .any(|p| p.location == e.location || p.semantic == e.semantic)
            {
                return Err("duplicate native input location/semantic".into());
            }
        }
        Ok(insert!(self, inputs, InputLayout, desc))
    }
    pub fn create_shader(&mut self, tokens: &[u32], stage: Stage) -> Result<Shader, String> {
        self.create_shader_layout(tokens, stage, false)
    }
    /// L90's bytecode-proven extent; relative addressing keeps the full bank.
    /// Pair with `Recorder::create_constant_buffer_for_shader`.
    pub fn create_compact_shader(&mut self, tokens: &[u32], stage: Stage) -> Result<Shader, String> {
        self.create_shader_layout(tokens, stage, true)
    }
    fn create_shader_layout(
        &mut self,
        tokens: &[u32],
        stage: Stage,
        compact: bool,
    ) -> Result<Shader, String> {
        let parsed = d3d9shader::parse(tokens).map_err(|e| e.to_string())?;
        let tag = tokens[0] >> 16;
        if tag != if stage == Stage::Vertex { 0xfffe } else { 0xffff } {
            return Err("shader stage mismatch".into());
        }
        Ok(insert!(
            self,
            shaders,
            Shader,
            ShaderObject {
                tokens: tokens.to_vec(),
                stage,
                compact,
                constants: ConstantLayout::new(
                    stage,
                    if compact {
                        parsed.float_constant_count().max(1)
                    } else if stage == Stage::Pixel {
                        224
                    } else {
                        256
                    }
                ),
            }
        ))
    }
    pub fn shader_constant_layout(&self, shader: Shader) -> ConstantLayout {
        self.check(shader.owner);
        self.shaders[shader.index as usize].constants
    }
    pub fn import_buffer(
        &mut self,
        backend: &mut crate::WgpuBackend,
        resource: u32,
    ) -> Result<Buffer, String> {
        if backend.device != self.gpu {
            return Err("resource is on another wgpu device".into());
        }
        backend.flush();
        let b = backend.buffer(resource).ok_or("buffer not uploaded")?;
        Ok(insert!(self, buffers, Buffer, b))
    }
    // The backend adapter has already resolved and validated these objects on
    // the same device. Retain their actual identities, not reusable guest IDs.
    pub(crate) fn retain_buffer(&mut self, gpu: wgpu::Buffer) -> Buffer {
        insert!(self, buffers, Buffer, gpu)
    }
    pub(crate) fn retain_view(
        &mut self,
        gpu: wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        samples: u32,
    ) -> View {
        insert!(
            self,
            views,
            View,
            ViewObject {
                gpu,
                format,
                size,
                samples
            }
        )
    }
    pub(crate) fn retain_sampler(&mut self, gpu: wgpu::Sampler, lod_bias: f32) -> Sampler {
        insert!(self, samplers, Sampler, SamplerObject { gpu, lod_bias })
    }
    pub fn create_buffer(&mut self, bytes: &[u8], usage: wgpu::BufferUsages) -> Buffer {
        use wgpu::util::DeviceExt;
        let b = self.gpu.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("native buffer"),
            contents: bytes,
            usage,
        });
        insert!(self, buffers, Buffer, b)
    }
    pub fn create_texture(&mut self, desc: &wgpu::TextureDescriptor<'_>) -> Texture {
        let t = self.gpu.create_texture(desc);
        insert!(self, textures, Texture, t)
    }
    pub fn import_texture(
        &mut self,
        backend: &mut crate::WgpuBackend,
        binding: &d3d9cmd::TextureBinding,
    ) -> Result<Texture, String> {
        if backend.device != self.gpu {
            return Err("resource is on another wgpu device".into());
        }
        backend.flush();
        let t = backend
            .texture_for(binding)
            .ok_or("texture unavailable")?
            .texture
            .clone();
        Ok(insert!(self, textures, Texture, t))
    }
    pub fn create_view(&mut self, texture: Texture, desc: &wgpu::TextureViewDescriptor<'_>) -> View {
        self.check(texture.owner);
        let t = &self.textures[texture.index as usize];
        let v = ViewObject {
            gpu: t.create_view(desc),
            format: desc.format.unwrap_or(t.format()),
            size: [
                (t.width() >> desc.base_mip_level).max(1),
                (t.height() >> desc.base_mip_level).max(1),
            ],
            samples: t.sample_count(),
        };
        insert!(self, views, View, v)
    }
    /// Both render-target and depth-stencil views use the same retained view
    /// type; the texture format determines which attachment slot accepts it.
    pub fn import_target(
        &mut self,
        backend: &mut crate::WgpuBackend,
        target: &d3d9cmd::Target,
        srgb: bool,
    ) -> Result<View, String> {
        if backend.device != self.gpu {
            return Err("resource is on another wgpu device".into());
        }
        backend.flush();
        let (gpu, format) = backend.target_view(target, srgb).ok_or("target unavailable")?;
        let v = ViewObject {
            gpu,
            format,
            size: [target.width, target.height],
            samples: crate::sample_count(target)?,
        };
        Ok(insert!(self, views, View, v))
    }
    fn module(
        &mut self,
        shader: Shader,
        input: InputLayout,
        vertex: Option<Shader>,
        ones: u64,
        depth_bias: bool,
    ) -> Result<(), String> {
        let key = (shader, input, vertex, ones, depth_bias);
        if self.modules.contains_key(&key) {
            return Ok(());
        }
        let s = &self.shaders[shader.index as usize];
        let parsed = d3d9shader::parse(&s.tokens).map_err(|e| e.to_string())?;
        let first = d3d9wgsl::translate(&parsed).map_err(|e| e.to_string())?;
        let mut options = d3d9wgsl::Options::default();
        options.compact_constants = s.compact;
        if s.stage == Stage::Vertex {
            options.half_pixel = true;
            let layout = &self.inputs[input.index as usize];
            let mut types = vec![];
            for (location, &(u, i, _)) in first.interface.inputs.iter().enumerate() {
                let e = layout
                    .elements
                    .iter()
                    .find(|e| e.semantic == (u, i))
                    .ok_or("missing vertex semantic")?;
                if e.location != location as u32 {
                    return Err("input location must match translated semantic order".into());
                }
                let name = format!("{:?}", e.format);
                types.push(if name.starts_with("Uint") {
                    d3d9wgsl::InputType::Uint
                } else if name.starts_with("Sint") {
                    d3d9wgsl::InputType::Sint
                } else {
                    d3d9wgsl::InputType::Float
                });
            }
            options.vs_input_types = Some(types);
        } else {
            options.lod_bias = true;
            options.sampler_ones = ones;
            options.depth_bias = depth_bias;
            let vs = &self.modules[&(
                vertex.ok_or("pixel shader requires a linked vertex shader")?,
                input,
                None,
                0,
                false,
            )]
                .interface;
            options.ps_input_locations = Some(
                first
                    .interface
                    .inputs
                    .iter()
                    .map(|&(u, i, _)| {
                        vs.outputs_vs
                            .iter()
                            .position(|o| (o.0, o.1) == (u, i))
                            .map(|n| n as u32)
                    })
                    .collect(),
            );
        }
        let translation = d3d9wgsl::translate_with(&parsed, options).map_err(|e| e.to_string())?;
        let source = crate::shader_cache::source(translation.wgsl)?;
        let scope = self.gpu.push_error_scope(wgpu::ErrorFilter::Validation);
        let gpu = self.gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native translated shader"),
            source,
        });
        if let Some(e) = crate::block_on(scope.pop()) {
            return Err(e.to_string());
        }
        self.modules.insert(
            key,
            Module {
                gpu,
                interface: translation.interface,
            },
        );
        Ok(())
    }
    pub fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<Pipeline, String> {
        for id in [
            desc.vertex_shader.owner,
            desc.pixel_shader.owner,
            desc.input.owner,
            desc.blend.owner,
            desc.rasterizer.owner,
            desc.depth_stencil.owner,
        ] {
            self.check(id);
        }
        if let Some(&h) = self.pipeline_cache.get(&desc) {
            return Ok(h);
        }
        if self.shaders[desc.vertex_shader.index as usize].stage != Stage::Vertex
            || self.shaders[desc.pixel_shader.index as usize].stage != Stage::Pixel
        {
            return Err("pipeline shader stage mismatch".into());
        }
        self.module(desc.vertex_shader, desc.input, None, 0, false)?;
        self.module(desc.pixel_shader, desc.input, Some(desc.vertex_shader), desc.sampler_ones, desc.depth_bias)?;
        let vs = &self.modules[&(desc.vertex_shader, desc.input, None, 0, false)];
        let ps = &self.modules[&(desc.pixel_shader, desc.input, Some(desc.vertex_shader), desc.sampler_ones, desc.depth_bias)];
        let kinds = [ps.interface.samplers.clone(), vs.interface.samplers.clone()];
        let mut make_layout = |stage: usize| {
            let key = (
                if stage == 0 { Stage::Pixel } else { Stage::Vertex },
                kinds[stage]
                    .iter()
                    .map(|&(n, k)| {
                        (
                            n,
                            match k {
                                d3d9wgsl::TextureKind::Flat => 0,
                                d3d9wgsl::TextureKind::Cube => 1,
                                d3d9wgsl::TextureKind::Volume => 2,
                            },
                        )
                    })
                    .collect(),
            );
            if let Some(layout) = self.texture_layouts.get(&key) {
                return layout.clone();
            }
            let mut entries = vec![];
            for &(slot, kind) in &kinds[stage] {
                let visibility = if stage == 0 {
                    wgpu::ShaderStages::FRAGMENT
                } else {
                    wgpu::ShaderStages::VERTEX
                };
                let view_dimension = match kind {
                    d3d9wgsl::TextureKind::Flat => wgpu::TextureViewDimension::D2,
                    d3d9wgsl::TextureKind::Cube => wgpu::TextureViewDimension::Cube,
                    d3d9wgsl::TextureKind::Volume => wgpu::TextureViewDimension::D3,
                };
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 2 * slot,
                    visibility,
                    count: None,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension,
                        multisampled: false,
                    },
                });
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 2 * slot + 1,
                    visibility,
                    count: None,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                });
            }
            let layout = self
                .gpu
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("native textures"),
                    entries: &entries,
                });
            self.texture_layouts.insert(key, layout.clone());
            layout
        };
        let layouts = [make_layout(0), make_layout(1)];
        let layout = self.gpu.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native pipeline"),
            bind_group_layouts: &[Some(&self.uniform_layout), Some(&layouts[0]), Some(&layouts[1])],
            immediate_size: 0,
        });
        let input = &self.inputs[desc.input.index as usize];
        let attrs: Vec<Vec<_>> = (0..input.streams.len())
            .map(|stream| {
                input
                    .elements
                    .iter()
                    .filter(|e| e.stream as usize == stream)
                    .map(|e| wgpu::VertexAttribute {
                        format: e.format,
                        offset: e.offset,
                        shader_location: e.location,
                    })
                    .collect()
            })
            .collect();
        let buffers: Vec<_> = input
            .streams
            .iter()
            .zip(&attrs)
            .map(|(s, a)| wgpu::VertexBufferLayout {
                array_stride: s.stride,
                step_mode: s.step,
                attributes: a,
            })
            .collect();
        let blend = &self.blends[desc.blend.index as usize];
        let raster = &self.rasters[desc.rasterizer.index as usize];
        let depth = &self.depths[desc.depth_stencil.index as usize];
        let targets: Vec<_> = desc
            .colours
            .iter()
            .zip(&blend.targets)
            .map(|(f, b)| {
                f.map(|format| wgpu::ColorTargetState {
                    format,
                    blend: b.enable.then_some(wgpu::BlendState {
                        color: b.colour,
                        alpha: b.alpha,
                    }),
                    write_mask: b.write_mask,
                })
            })
            .collect();
        let end = targets.iter().rposition(Option::is_some).map_or(0, |n| n + 1);
        let scope = self.gpu.push_error_scope(wgpu::ErrorFilter::Validation);
        let gpu = self.gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &vs.gpu,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &ps.gpu,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &targets[..end],
            }),
            primitive: wgpu::PrimitiveState {
                topology: desc.topology,
                strip_index_format: desc.strip_index,
                front_face: raster.front_face,
                cull_mode: raster.cull,
                polygon_mode: raster.fill,
                ..Default::default()
            },
            depth_stencil: desc.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(depth.depth_enable && depth.depth_write),
                depth_compare: Some(if depth.depth_enable {
                    depth.depth_func
                } else {
                    wgpu::CompareFunction::Always
                }),
                stencil: if depth.stencil_enable {
                    wgpu::StencilState {
                        front: depth.front,
                        back: depth.back,
                        read_mask: depth.read_mask,
                        write_mask: depth.write_mask,
                    }
                } else {
                    Default::default()
                },
                bias: wgpu::DepthBiasState {
                    constant: raster.depth_bias,
                    slope_scale: raster.slope_bias,
                    clamp: raster.bias_clamp,
                },
            }),
            multisample: wgpu::MultisampleState {
                count: desc.samples,
                mask: desc.sample_mask,
                alpha_to_coverage_enabled: blend.alpha_to_coverage,
            },
            multiview_mask: None,
            cache: self.disk_cache.as_ref().map(|cache| &cache.cache),
        });
        if let Some(e) = crate::block_on(scope.pop()) {
            return Err(e.to_string());
        }
        if let Some(cache) = &self.disk_cache {
            cache.created();
            cache.save_if_due();
        }
        let constants = [
            self.shader_constant_layout(desc.pixel_shader),
            self.shader_constant_layout(desc.vertex_shader),
        ];
        let h = insert!(
            self,
            pipelines,
            Pipeline,
            PipelineObject {
                gpu,
                layouts,
                kinds,
                desc,
                constants,
            }
        );
        self.pipeline_cache.insert(desc, h);
        Ok(h)
    }
    /// Cold-path interning. A draw binds the resulting integer handle, never
    /// hashes this tuple or allocates its entries again.
    pub fn create_texture_set(
        &mut self,
        pipeline: Pipeline,
        stage: Stage,
        bindings: &[TextureBinding],
    ) -> Result<TextureSet, String> {
        self.check(pipeline.owner);
        let mut bindings = bindings.to_vec();
        bindings.sort_by_key(|b| b.slot);
        let p = &self.pipelines[pipeline.index as usize];
        let n = if stage == Stage::Pixel { 0 } else { 1 };
        let key = (p.layouts[n].clone(), bindings);
        if let Some(&h) = self.set_cache.get(&key) {
            return Ok(h);
        }
        if key.1.len() != p.kinds[n].len()
            || key.1.windows(2).any(|b| b[0].slot == b[1].slot)
            || key.1.iter().any(|b| !p.kinds[n].iter().any(|k| b.slot == k.0))
        {
            return Err("texture set must exactly cover shader sampler slots".into());
        }
        let mut entries = vec![];
        let mut bias = [0; 16];
        for b in &key.1 {
            self.check(b.view.owner);
            self.check(b.sampler.owner);
            let sampler = &self.samplers[b.sampler.index as usize];
            if stage == Stage::Vertex && sampler.lod_bias != 0.0 {
                return Err("vertex sampler LOD bias is not supported by the translator".into());
            }
            if stage == Stage::Pixel {
                if b.slot >= 16 {
                    return Err("pixel sampler slot exceeds 15".into());
                }
                bias[b.slot as usize] = sampler.lod_bias.to_bits();
            }
            entries.push(wgpu::BindGroupEntry {
                binding: 2 * b.slot,
                resource: wgpu::BindingResource::TextureView(&self.views[b.view.index as usize].gpu),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 2 * b.slot + 1,
                resource: wgpu::BindingResource::Sampler(&sampler.gpu),
            });
        }
        let scope = self.gpu.push_error_scope(wgpu::ErrorFilter::Validation);
        let gpu = self.gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native texture set"),
            layout: &p.layouts[n],
            entries: &entries,
        });
        if let Some(e) = crate::block_on(scope.pop()) {
            return Err(e.to_string());
        }
        let h = insert!(
            self,
            sets,
            TextureSet,
            TextureSetObject {
                gpu,
                layout: p.layouts[n].clone(),
                stage,
                bias
            }
        );
        self.set_cache.insert(key, h);
        Ok(h)
    }
    pub fn pipeline_count(&self) -> usize {
        self.pipelines.len()
    }
    /// Forget every imported or created texture, view, buffer and texture
    /// set (their handles become invalid; the caller drops its own). The
    /// states, shaders, modules, pipelines, samplers and layouts stay: they
    /// do not refer to resources (L141: a guest release used to drop the
    /// whole device, its pipelines included).
    pub fn reset_resources(&mut self) {
        self.textures.clear();
        self.views.clear();
        self.buffers.clear();
        self.sets.clear();
        self.set_cache.clear();
    }
    pub fn texture_set_count(&self) -> usize {
        self.sets.len()
    }
}

/// Translated shader constant ABI: float registers, 16 int4 registers,
/// 16 bool registers padded to uint4, then stage extras. VS extra[0].xy
/// is the half-pixel correction (1/viewport width, -1/viewport height).
/// PS extra[1..5] holds sampler LOD biases, supplied by texture-set binding.
pub fn constant_block_size(stage: Stage) -> usize {
    if stage == Stage::Pixel {
        PS_BLOCK as usize
    } else {
        VS_BLOCK as usize
    }
}
struct ConstantObject {
    stage: Stage,
    layout: ConstantLayout,
    bytes: Vec<u8>,
    offset: Option<(u64, u32)>,
    uploaded_bias: [u32; 16],
}
/// Offsets are bytes; floats/int/bool slots are 16 bytes each. In compact
/// layouts the following banks move with the proven float prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstantLayout {
    pub stage: Stage,
    pub floats: usize,
    pub int_offset: usize,
    pub bool_offset: usize,
    pub extra_offset: usize,
    pub byte_len: usize,
}
impl ConstantLayout {
    fn new(stage: Stage, floats: usize) -> Self {
        let int_offset = floats * 16;
        let bool_offset = int_offset + 256;
        let extra_offset = bool_offset + 256;
        Self {
            stage,
            floats,
            int_offset,
            bool_offset,
            extra_offset,
            byte_len: extra_offset + if stage == Stage::Pixel { 96 } else { 16 },
        }
    }
}
fn write_bias(dst: &mut [u8], bias: [u32; 16], extra_offset: usize) {
    for (word, value) in dst[extra_offset + 16..extra_offset + 80]
        .chunks_exact_mut(4)
        .zip(bias)
    {
        word.copy_from_slice(&value.to_le_bytes());
    }
}
fn allocate_uniform(
    ring: &mut Uniforms,
    layout: ConstantLayout,
    align: u64,
) -> Result<(u64, &mut [u8]), &'static str> {
    // Bind group ranges remain the ABI maxima. A compact allocation can
    // overlap the following binding's unused tail, but must fit the buffer.
    if !ring.fits(constant_block_size(layout.stage) as u64, align) {
        return Err("native uniform ring full: submit before retrying");
    }
    ring.alloc(layout.byte_len as u64, align)
        .ok_or("native uniform ring full: submit before retrying")
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferSlice {
    pub buffer: Buffer,
    pub offset: u64,
    pub size: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct PassDesc {
    pub colours: [Option<View>; 4],
    pub depth: Option<View>,
    /// None loads the current attachment. Values clear at pass start only.
    pub clear_colour: Option<wgpu::Color>,
    pub clear_depth: Option<f32>,
    pub clear_stencil: Option<u32>,
}
#[derive(Default, Clone, Copy, Debug)]
pub struct RecorderStats {
    pub draws: u64,
    pub uniform_writes: u64,
    pub pipeline_sets: u64,
    pub texture_sets: u64,
    pub upload_bytes: u64,
    pub dirty_registers: u64,
    pub bundle_builds: u64,
    pub bundle_hits: u64,
}
#[derive(Default)]
struct LastState {
    pipeline: Option<Pipeline>,
    offsets: Option<[u32; 2]>,
    sets: [Option<TextureSet>; 2],
    vertex: [Option<BufferSlice>; 16],
    index: Option<(BufferSlice, wgpu::IndexFormat)>,
    viewport: Option<[f32; 6]>,
    scissor: Option<[u32; 4]>,
    stencil: Option<u32>,
    blend: Option<[f64; 4]>,
}
struct Pass {
    gpu: wgpu::RenderPass<'static>,
    desc: PassDesc,
    size: [u32; 2],
    last: LastState,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct BundleDraw {
    pipeline: Pipeline,
    offsets: [u32; 2],
    sets: [Option<TextureSet>; 2],
    vertex: [Option<BufferSlice>; 16],
    index: Option<(BufferSlice, wgpu::IndexFormat)>,
    range: Range<u32>,
    base_vertex: Option<i32>,
    instances: Range<u32>,
}
struct CachedBundle {
    draws: Vec<BundleDraw>,
    colours: [Option<wgpu::TextureFormat>; 4],
    depth: Option<wgpu::TextureFormat>,
    samples: u32,
    gpu: wgpu::RenderBundle,
}
/// One independently owned encoder/uniform ring. Several recorders can read
/// one immutable Device concurrently; submission ordering is the caller's job.
/// Instancing is explicit in both draw methods and input stream step modes.
pub struct Recorder<'a> {
    device: &'a Device,
    // Drop order is significant after forget_lifetime: pass before encoder.
    pass: Option<Pass>,
    encoder: Option<wgpu::CommandEncoder>,
    id: u32,
    ring: Uniforms,
    group: wgpu::BindGroup,
    constants: Vec<ConstantObject>,
    bound_constants: [Option<ConstantBuffer>; 2],
    pipeline: Option<Pipeline>,
    sets: [Option<TextureSet>; 2],
    vertex: [Option<BufferSlice>; 16],
    index: Option<(BufferSlice, wgpu::IndexFormat)>,
    viewport: Option<[f32; 6]>,
    scissor: Option<[u32; 4]>,
    stencil: u32,
    blend: [f64; 4],
    bundles: bool,
    pending_bundle: Vec<BundleDraw>,
    bundle_cache: Vec<CachedBundle>,
    bundle_cursor: usize,
    pub stats: RecorderStats,
}
/// Owned, closed recorder storage. Detach after submission/encoder handoff,
/// mutate/prewarm Device objects, then resume without reallocating uniform
/// storage, constant handles or cached bundles. It belongs to one Device.
pub struct RecorderState {
    device_id: u32,
    id: u32,
    ring: Uniforms,
    group: wgpu::BindGroup,
    constants: Vec<ConstantObject>,
    bound_constants: [Option<ConstantBuffer>; 2],
    pipeline: Option<Pipeline>,
    sets: [Option<TextureSet>; 2],
    vertex: [Option<BufferSlice>; 16],
    index: Option<(BufferSlice, wgpu::IndexFormat)>,
    viewport: Option<[f32; 6]>,
    scissor: Option<[u32; 4]>,
    stencil: u32,
    blend: [f64; 4],
    bundles: bool,
    pending_bundle: Vec<BundleDraw>,
    bundle_cache: Vec<CachedBundle>,
    bundle_cursor: usize,
    pub stats: RecorderStats,
}
impl RecorderState {
    pub fn uniform_capacity(&self) -> u64 {
        self.ring.capacity()
    }
}
impl<'a> Recorder<'a> {
    pub fn into_state(self) -> RecorderState {
        assert!(
            self.pass.is_none() && self.encoder.is_none() && self.pending_bundle.is_empty(),
            "submit or finish the recorder before detaching"
        );
        RecorderState {
            device_id: self.device.id,
            id: self.id,
            ring: self.ring,
            group: self.group,
            constants: self.constants,
            bound_constants: self.bound_constants,
            pipeline: self.pipeline,
            sets: self.sets,
            vertex: self.vertex,
            index: self.index,
            viewport: self.viewport,
            scissor: self.scissor,
            stencil: self.stencil,
            blend: self.blend,
            bundles: self.bundles,
            pending_bundle: self.pending_bundle,
            bundle_cache: self.bundle_cache,
            bundle_cursor: self.bundle_cursor,
            stats: self.stats,
        }
    }
    pub fn from_state(device: &'a Device, state: RecorderState) -> Self {
        device.check(state.device_id);
        Self {
            device,
            pass: None,
            encoder: None,
            id: state.id,
            ring: state.ring,
            group: state.group,
            constants: state.constants,
            bound_constants: state.bound_constants,
            pipeline: state.pipeline,
            sets: state.sets,
            vertex: state.vertex,
            index: state.index,
            viewport: state.viewport,
            scissor: state.scissor,
            stencil: state.stencil,
            blend: state.blend,
            bundles: state.bundles,
            pending_bundle: state.pending_bundle,
            bundle_cache: state.bundle_cache,
            bundle_cursor: state.bundle_cursor,
            stats: state.stats,
        }
    }
    pub fn new(device: &'a Device, uniform_bytes: u64) -> Self {
        Self::with_resident_constants(device, uniform_bytes, false)
    }
    /// Opt-in stable snapshot arena. Uploads only registers changed since the
    /// preceding submission, with a compute scatter before all recorded draws.
    /// Does not equate sequential draw deltas with cross-frame slot deltas.
    pub fn with_resident_constants(device: &'a Device, uniform_bytes: u64, resident: bool) -> Self {
        assert!(uniform_bytes >= VS_BLOCK.max(PS_BLOCK) && uniform_bytes <= u32::MAX as u64);
        let ring = Uniforms::new(&device.gpu, uniform_bytes.next_multiple_of(256), resident);
        let binding = |binding, size| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &ring.buffer,
                offset: 0,
                size: NonZeroU64::new(size),
            }),
        };
        let group = device.gpu.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native constants"),
            layout: &device.uniform_layout,
            entries: &[binding(0, PS_BLOCK), binding(2, VS_BLOCK)],
        });
        Self {
            device,
            id: owner(),
            ring,
            group,
            constants: vec![],
            bound_constants: [None; 2],
            encoder: None,
            pass: None,
            pipeline: None,
            sets: [None; 2],
            vertex: [None; 16],
            index: None,
            viewport: None,
            scissor: None,
            stencil: 0,
            blend: [0.0; 4],
            bundles: false,
            pending_bundle: Vec::new(),
            bundle_cache: Vec::new(),
            bundle_cursor: 0,
            stats: Default::default(),
        }
    }
    /// Exact sequence comparison, no hash-only acceptance. Dynamic pass state
    /// splits sequences; constants may change without rebuilding a bundle as
    /// long as the stable offsets and all resource identities are unchanged.
    /// Cache retains only the preceding submission's sequence slots.
    pub fn enable_bundles(&mut self, enabled: bool) {
        self.flush_bundle();
        self.bundles = enabled;
        if !enabled {
            self.bundle_cache.clear();
            self.bundle_cursor = 0;
        }
    }
    fn flush_bundle(&mut self) {
        if self.pending_bundle.is_empty() {
            return;
        }
        let pass = self.pass.as_mut().expect("pending bundle needs a pass");
        let colours = pass
            .desc
            .colours
            .map(|h| h.map(|h| self.device.views[h.index as usize].format));
        let depth = pass
            .desc
            .depth
            .map(|h| self.device.views[h.index as usize].format);
        let first = pass
            .desc
            .colours
            .iter()
            .flatten()
            .copied()
            .next()
            .or(pass.desc.depth)
            .unwrap();
        let samples = self.device.views[first.index as usize].samples;
        let hit = self.bundle_cache.get(self.bundle_cursor).is_some_and(|b| {
            b.colours == colours && b.depth == depth && b.samples == samples && b.draws == self.pending_bundle
        });
        if hit {
            self.stats.bundle_hits += 1;
        } else {
            let end = colours.iter().rposition(Option::is_some).map_or(0, |n| n + 1);
            let mut encoder =
                self.device
                    .gpu
                    .create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                        label: Some("native repeated sequence"),
                        color_formats: &colours[..end],
                        depth_stencil: depth.map(|format| wgpu::RenderBundleDepthStencil {
                            format,
                            depth_read_only: false,
                            stencil_read_only: false,
                        }),
                        sample_count: samples,
                        multiview: None,
                    });
            let mut last = LastState::default();
            for draw in &self.pending_bundle {
                let pipeline = &self.device.pipelines[draw.pipeline.index as usize];
                if last.pipeline != Some(draw.pipeline) {
                    encoder.set_pipeline(&pipeline.gpu);
                    last.pipeline = Some(draw.pipeline);
                }
                if last.offsets != Some(draw.offsets) {
                    encoder.set_bind_group(0, &self.group, &draw.offsets);
                    last.offsets = Some(draw.offsets);
                }
                for n in 0..2 {
                    if last.sets[n] != draw.sets[n] {
                        encoder.set_bind_group(
                            n as u32 + 1,
                            &self.device.sets[draw.sets[n].unwrap().index as usize].gpu,
                            &[],
                        );
                        last.sets[n] = draw.sets[n];
                    }
                }
                let streams = self.device.inputs[pipeline.desc.input.index as usize]
                    .streams
                    .len();
                for n in 0..streams {
                    if last.vertex[n] != draw.vertex[n] {
                        let v = draw.vertex[n].unwrap();
                        encoder.set_vertex_buffer(
                            n as u32,
                            self.device.buffers[v.buffer.index as usize].slice(v.offset..v.offset + v.size),
                        );
                        last.vertex[n] = draw.vertex[n];
                    }
                }
                if let Some(base) = draw.base_vertex {
                    if last.index != draw.index {
                        let (i, format) = draw.index.unwrap();
                        encoder.set_index_buffer(
                            self.device.buffers[i.buffer.index as usize].slice(i.offset..i.offset + i.size),
                            format,
                        );
                        last.index = draw.index;
                    }
                    encoder.draw_indexed(draw.range.clone(), base, draw.instances.clone());
                } else {
                    encoder.draw(draw.range.clone(), draw.instances.clone());
                }
            }
            let gpu = encoder.finish(&wgpu::RenderBundleDescriptor {
                label: Some("native repeated sequence"),
            });
            let cached = CachedBundle {
                draws: self.pending_bundle.clone(),
                colours,
                depth,
                samples,
                gpu,
            };
            if self.bundle_cursor == self.bundle_cache.len() {
                self.bundle_cache.push(cached);
            } else {
                self.bundle_cache[self.bundle_cursor] = cached;
            }
            self.stats.bundle_builds += 1;
        }
        pass.gpu
            .execute_bundles([&self.bundle_cache[self.bundle_cursor].gpu]);
        // execute_bundles clears the pass's pipeline/buffer/bind-group state.
        pass.last.pipeline = None;
        pass.last.offsets = None;
        pass.last.sets = [None; 2];
        pass.last.vertex = [None; 16];
        pass.last.index = None;
        self.bundle_cursor += 1;
        self.pending_bundle.clear();
    }
    fn bundle_draw(&mut self, range: Range<u32>, base_vertex: Option<i32>, instances: Range<u32>) {
        self.pending_bundle.push(BundleDraw {
            pipeline: self.pipeline.unwrap(),
            offsets: self.pass.as_ref().unwrap().last.offsets.unwrap(),
            sets: self.sets,
            vertex: self.vertex,
            index: if base_vertex.is_some() { self.index } else { None },
            range,
            base_vertex,
            instances,
        });
    }
    pub fn create_constant_buffer(&mut self, stage: Stage) -> ConstantBuffer {
        self.create_constant_layout(ConstantLayout::new(
            stage,
            if stage == Stage::Pixel { 224 } else { 256 },
        ))
    }
    pub fn create_constant_buffer_for_shader(&mut self, shader: Shader) -> ConstantBuffer {
        self.create_constant_layout(self.device.shader_constant_layout(shader))
    }
    fn create_constant_layout(&mut self, layout: ConstantLayout) -> ConstantBuffer {
        insert!(
            self,
            constants,
            ConstantBuffer,
            ConstantObject {
                stage: layout.stage,
                layout,
                bytes: vec![0; layout.byte_len],
                offset: None,
                uploaded_bias: [0; 16]
            }
        )
    }
    /// Writes one new immutable ring snapshot per changed block, never per
    /// draw. If the ring is full returns an error without changing this block;
    /// submit, reopen the pass with load operations, and retry the update.
    pub fn update_constants(&mut self, handle: ConstantBuffer, bytes: &[u8]) -> Result<(), &'static str> {
        assert_eq!(
            handle.owner, self.id,
            "constant buffer belongs to another recorder"
        );
        let c = &mut self.constants[handle.index as usize];
        let bias = self.sets[0].map_or([0; 16], |h| self.device.sets[h.index as usize].bias);
        if bytes.len() != c.bytes.len() {
            return Err("wrong constant block size");
        }
        if c.offset.is_some_and(|(g, _)| g == self.ring.generation)
            && c.bytes == bytes
            && (c.stage != Stage::Pixel || c.uploaded_bias == bias)
        {
            return Ok(());
        }
        let align = self.device.gpu.limits().min_uniform_buffer_offset_alignment as u64;
        let (off, dst) = allocate_uniform(&mut self.ring, c.layout, align)?;
        dst.copy_from_slice(bytes);
        c.bytes.copy_from_slice(bytes);
        if c.stage == Stage::Pixel {
            write_bias(dst, bias, c.layout.extra_offset);
            c.uploaded_bias = bias;
        }
        c.offset = Some((self.ring.generation, off as u32));
        self.stats.uniform_writes += 1;
        Ok(())
    }
    pub fn set_constants(&mut self, handle: ConstantBuffer) {
        assert_eq!(
            handle.owner, self.id,
            "constant buffer belongs to another recorder"
        );
        let n = if self.constants[handle.index as usize].stage == Stage::Pixel {
            0
        } else {
            1
        };
        self.bound_constants[n] = Some(handle);
    }
    /// Apply a byte-range delta to the retained CPU block. Several deltas
    /// coalesce into one immutable ring snapshot at the next draw; previous
    /// draws keep their offsets. Register writes need not rebuild a full Vec.
    /// Full-block GPU storage is still required by this translator ABI.
    /// `update_constant_range` for little-endian words, without building a
    /// byte buffer (L141: the worker converted every draw's banks).
    pub fn update_constant_words(
        &mut self,
        handle: ConstantBuffer,
        offset: usize,
        words: &[u32],
    ) -> Result<(), &'static str> {
        assert_eq!(
            handle.owner, self.id,
            "constant buffer belongs to another recorder"
        );
        let c = &mut self.constants[handle.index as usize];
        let end = words.len().checked_mul(4).and_then(|n| n.checked_add(offset)).ok_or("constant range overflow")?;
        let dst = c.bytes.get_mut(offset..end).ok_or("constant range outside block")?;
        let mut changed = false;
        for (d, w) in dst.chunks_exact_mut(4).zip(words) {
            let w = w.to_le_bytes();
            if *d != w {
                d.copy_from_slice(&w);
                changed = true;
            }
        }
        if changed {
            c.offset = None;
        }
        Ok(())
    }
    pub fn update_constant_range(
        &mut self,
        handle: ConstantBuffer,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), &'static str> {
        assert_eq!(
            handle.owner, self.id,
            "constant buffer belongs to another recorder"
        );
        let c = &mut self.constants[handle.index as usize];
        let end = offset.checked_add(bytes.len()).ok_or("constant range overflow")?;
        let dst = c
            .bytes
            .get_mut(offset..end)
            .ok_or("constant range outside block")?;
        if dst != bytes {
            dst.copy_from_slice(bytes);
            c.offset = None;
        }
        Ok(())
    }
    pub fn set_pipeline(&mut self, pipeline: Pipeline) {
        self.device.check(pipeline.owner);
        if self.pipeline.is_some_and(|old| {
            self.device.rasters[self.device.pipelines[old.index as usize].desc.rasterizer.index as usize]
                .scissor_enable
                != self.device.rasters[self.device.pipelines[pipeline.index as usize]
                    .desc
                    .rasterizer
                    .index as usize]
                    .scissor_enable
        }) {
            self.flush_bundle();
        }
        self.pipeline = Some(pipeline);
    }
    pub fn set_texture_set(&mut self, set: TextureSet) {
        self.device.check(set.owner);
        let n = if self.device.sets[set.index as usize].stage == Stage::Pixel {
            0
        } else {
            1
        };
        self.sets[n] = Some(set);
    }
    pub fn set_vertex_buffer(&mut self, stream: u32, slice: BufferSlice) {
        self.device.check(slice.buffer.owner);
        self.vertex[stream as usize] = Some(slice);
    }
    pub fn set_index_buffer(&mut self, slice: BufferSlice, format: wgpu::IndexFormat) {
        self.device.check(slice.buffer.owner);
        self.index = Some((slice, format));
    }
    pub fn set_viewport(&mut self, v: [f32; 6]) {
        if self.viewport != Some(v) {
            self.flush_bundle();
        }
        self.viewport = Some(v);
    }
    pub fn set_scissor(&mut self, rect: [u32; 4]) {
        if self.scissor != Some(rect) {
            self.flush_bundle();
        }
        self.scissor = Some(rect);
    }
    pub fn set_stencil_reference(&mut self, reference: u32) {
        if self.stencil != reference {
            self.flush_bundle();
        }
        self.stencil = reference;
    }
    pub fn set_blend_constant(&mut self, colour: [f64; 4]) {
        if self.blend != colour {
            self.flush_bundle();
        }
        self.blend = colour;
    }
    pub fn begin_pass(&mut self, desc: PassDesc) -> Result<(), &'static str> {
        self.end_pass();
        let device = self.device;
        let first = desc
            .colours
            .iter()
            .flatten()
            .next()
            .copied()
            .or(desc.depth)
            .ok_or("native pass has no attachments")?;
        device.check(first.owner);
        let v = &device.views[first.index as usize];
        let size = v.size;
        for h in desc.colours.iter().flatten().copied().chain(desc.depth) {
            device.check(h.owner);
            let other = &device.views[h.index as usize];
            if other.size != size || other.samples != v.samples {
                return Err("native attachments differ in size/sample count");
            }
        }
        let colours = desc.colours.map(|h| {
            h.map(|h| wgpu::RenderPassColorAttachment {
                view: &device.views[h.index as usize].gpu,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: desc.clear_colour.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                },
            })
        });
        let end = colours.iter().rposition(Option::is_some).map_or(0, |n| n + 1);
        let depth = desc.depth.map(|h| {
            let v = &device.views[h.index as usize];
            wgpu::RenderPassDepthStencilAttachment {
                view: &v.gpu,
                depth_ops: v.format.has_depth_aspect().then_some(wgpu::Operations {
                    load: desc.clear_depth.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: v.format.has_stencil_aspect().then_some(wgpu::Operations {
                    load: desc.clear_stencil.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                }),
            }
        });
        let encoder = self
            .encoder
            .get_or_insert_with(|| device.gpu.create_command_encoder(&Default::default()));
        let gpu = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native pass"),
                color_attachments: &colours[..end],
                depth_stencil_attachment: depth,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        self.pass = Some(Pass {
            gpu,
            desc,
            size,
            last: Default::default(),
        });
        Ok(())
    }
    fn prepare(&mut self, indexed: bool) -> Result<(), &'static str> {
        let pipeline = self.pipeline.ok_or("native pipeline not set")?;
        let p = &self.device.pipelines[pipeline.index as usize];
        let pass = self.pass.as_mut().ok_or("native pass not open")?;
        let formats = pass
            .desc
            .colours
            .map(|h| h.map(|h| self.device.views[h.index as usize].format));
        let first = pass
            .desc
            .colours
            .iter()
            .flatten()
            .next()
            .copied()
            .or(pass.desc.depth)
            .unwrap();
        if formats != p.desc.colours
            || pass
                .desc
                .depth
                .map(|h| self.device.views[h.index as usize].format)
                != p.desc.depth
            || self.device.views[first.index as usize].samples != p.desc.samples
        {
            return Err("pipeline attachment formats/sample count mismatch");
        }
        let mut offsets = [0; 2];
        for (n, off) in offsets.iter_mut().enumerate() {
            let handle = self.bound_constants[n].ok_or("native constant buffer not bound")?;
            let c = &mut self.constants[handle.index as usize];
            if c.layout != p.constants[n] {
                return Err("constant layout does not match bound shader");
            }
            let bias = self.sets[0].map_or([0; 16], |h| self.device.sets[h.index as usize].bias);
            if !c.offset.is_some_and(|(g, _)| g == self.ring.generation)
                || (n == 0 && c.uploaded_bias != bias)
            {
                let (offset, bytes) = allocate_uniform(
                    &mut self.ring,
                    c.layout,
                    self.device.gpu.limits().min_uniform_buffer_offset_alignment as u64,
                )?;
                bytes.copy_from_slice(&c.bytes);
                if n == 0 {
                    write_bias(bytes, bias, c.layout.extra_offset);
                    c.uploaded_bias = bias;
                }
                c.offset = Some((self.ring.generation, offset as u32));
                self.stats.uniform_writes += 1;
            }
            *off = c.offset.unwrap().1;
            let set =
                self.sets[n].ok_or("native texture set not bound (bind an empty set for unused stages)")?;
            if self.device.sets[set.index as usize].layout != p.layouts[n] {
                return Err("texture set layout incompatible with pipeline");
            }
        }
        let streams = self.device.inputs[p.desc.input.index as usize].streams.len();
        if self.vertex[..streams].iter().any(Option::is_none) || (indexed && self.index.is_none()) {
            return Err("native vertex/index buffer not bound");
        }
        if !self.bundles {
            if pass.last.pipeline != Some(pipeline) {
                pass.gpu.set_pipeline(&p.gpu);
                pass.last.pipeline = Some(pipeline);
                self.stats.pipeline_sets += 1;
            }
            if pass.last.offsets != Some(offsets) {
                pass.gpu.set_bind_group(0, &self.group, &offsets);
                pass.last.offsets = Some(offsets);
            }
            for n in 0..2 {
                if pass.last.sets[n] != self.sets[n] {
                    pass.gpu.set_bind_group(
                        n as u32 + 1,
                        &self.device.sets[self.sets[n].unwrap().index as usize].gpu,
                        &[],
                    );
                    pass.last.sets[n] = self.sets[n];
                    self.stats.texture_sets += 1;
                }
            }
            for n in 0..streams {
                if pass.last.vertex[n] != self.vertex[n] {
                    let s = self.vertex[n].unwrap();
                    pass.gpu.set_vertex_buffer(
                        n as u32,
                        self.device.buffers[s.buffer.index as usize].slice(s.offset..s.offset + s.size),
                    );
                    pass.last.vertex[n] = Some(s);
                }
            }
            if indexed && pass.last.index != self.index {
                let (s, f) = self.index.unwrap();
                pass.gpu.set_index_buffer(
                    self.device.buffers[s.buffer.index as usize].slice(s.offset..s.offset + s.size),
                    f,
                );
                pass.last.index = self.index;
            }
        } else {
            pass.last.offsets = Some(offsets);
        }
        let viewport =
            self.viewport
                .unwrap_or([0.0, 0.0, pass.size[0] as f32, pass.size[1] as f32, 0.0, 1.0]);
        if pass.last.viewport != Some(viewport) {
            pass.gpu.set_viewport(
                viewport[0],
                viewport[1],
                viewport[2],
                viewport[3],
                viewport[4],
                viewport[5],
            );
            pass.last.viewport = Some(viewport);
        }
        let scissor = if self.device.rasters[p.desc.rasterizer.index as usize].scissor_enable {
            self.scissor.ok_or("scissor-enabled pipeline needs a rectangle")?
        } else {
            [0, 0, pass.size[0], pass.size[1]]
        };
        if pass.last.scissor != Some(scissor) {
            pass.gpu
                .set_scissor_rect(scissor[0], scissor[1], scissor[2], scissor[3]);
            pass.last.scissor = Some(scissor);
        }
        if pass.last.stencil != Some(self.stencil) {
            pass.gpu.set_stencil_reference(self.stencil);
            pass.last.stencil = Some(self.stencil);
        }
        if pass.last.blend != Some(self.blend) {
            let [r, g, b, a] = self.blend;
            pass.gpu.set_blend_constant(wgpu::Color { r, g, b, a });
            pass.last.blend = Some(self.blend);
        }
        Ok(())
    }
    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) -> Result<(), &'static str> {
        self.prepare(false)?;
        if self.bundles {
            self.bundle_draw(vertices, None, instances);
        } else {
            self.pass.as_mut().unwrap().gpu.draw(vertices, instances);
        }
        self.stats.draws += 1;
        Ok(())
    }
    pub fn draw_indexed(
        &mut self,
        indices: Range<u32>,
        base_vertex: i32,
        instances: Range<u32>,
    ) -> Result<(), &'static str> {
        self.prepare(true)?;
        if self.bundles {
            self.bundle_draw(indices, Some(base_vertex), instances);
        } else {
            self.pass
                .as_mut()
                .unwrap()
                .gpu
                .draw_indexed(indices, base_vertex, instances);
        }
        self.stats.draws += 1;
        Ok(())
    }
    pub fn end_pass(&mut self) {
        self.flush_bundle();
        self.pass.take();
    }
    /// Flush CPU uploads and detach the still-unfinished render encoder. The
    /// caller must order prior submissions before invoking this (queue writes
    /// happen here), submit the optional scatter before the render encoder,
    /// and submit both before reusing this recorder's GPU storage.
    pub(crate) fn finish_parts(&mut self) -> (Option<wgpu::CommandBuffer>, Option<wgpu::CommandEncoder>) {
        self.end_pass();
        self.bundle_cache.truncate(self.bundle_cursor);
        self.bundle_cursor = 0;
        let Some(encoder) = self.encoder.take() else {
            self.ring.restart();
            return (None, None);
        };
        let upload = self
            .ring
            .upload(&self.device.gpu, &self.device.queue, &mut self.stats);
        self.ring.restart();
        (upload, Some(encoder))
    }
    pub fn submit(&mut self) -> Option<wgpu::SubmissionIndex> {
        let (upload, encoder) = self.finish_parts();
        encoder.map(|encoder| {
            self.device
                .queue
                .submit(upload.into_iter().chain([encoder.finish()]))
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn immutable_device_is_shared_and_recorders_can_move_to_workers() {
        fn shared<T: Send + Sync>() {}
        fn movable<T: Send>() {}
        shared::<super::Device>();
        movable::<super::Recorder<'static>>();
        movable::<super::RecorderState>();
    }
}
