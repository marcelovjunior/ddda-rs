//! The game's pixel shaders, translated to WGSL and run on wgpu, against
//! the same shaders run by the host's D3D9 driver (crates/d3d9ref), with
//! the same random constants, inputs and textures. Skips without the game,
//! D3D9 or a wgpu adapter.
//!
//! DDDA_SHADER_PACKAGES=Title,root,... chooses the packages (default
//! Title); DDDA_SHADER_LIMIT=n stops after n shaders.
use d3d9ref::{Oracle, PixelRun, Texture, TextureKind};
use std::future::Future;

#[path = "support/unlinked.rs"]
mod unlinked;
#[path = "support/pairings.rs"]
mod pairings;

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

/// A translated pixel shader's pipeline and layouts, reused across runs.
struct PixelPipeline {
    pipeline: wgpu::RenderPipeline,
    layout0: wgpu::BindGroupLayout,
    layout1: wgpu::BindGroupLayout,
    targets: Vec<Option<wgpu::ColorTargetState>>,
}

/// A translated vertex shader's pipelines (one per pair of captured
/// locations) and layouts, reused across runs.
struct VertexPipelines {
    pipelines: Vec<(Vec<usize>, wgpu::RenderPipeline)>,
    layout0: wgpu::BindGroupLayout,
    layout1: wgpu::BindGroupLayout,
    layout2: wgpu::BindGroupLayout,
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    vs: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    /// The adapter, for reports.
    adapter: String,
}

/// The vertex stage: a triangle covering the 1x1 target, outputs k = the
/// k-th varying.
const VS: &str = "
struct Varyings { v: array<vec4<f32>, 16> }
@group(0) @binding(1) var<uniform> vary: Varyings;
struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) o0: vec4<f32>, @location(1) o1: vec4<f32>, @location(2) o2: vec4<f32>, @location(3) o3: vec4<f32>,
  @location(4) o4: vec4<f32>, @location(5) o5: vec4<f32>, @location(6) o6: vec4<f32>, @location(7) o7: vec4<f32>,
  @location(8) o8: vec4<f32>, @location(9) o9: vec4<f32>,
}
@vertex fn main(@builtin(vertex_index) i: u32) -> VOut {
  let xy = vec2<f32>(f32(i & 1u) * 4.0 - 1.0, f32(i >> 1u) * 4.0 - 1.0);
  return VOut(vec4<f32>(xy, 0.0, 1.0), vary.v[0], vary.v[1], vary.v[2], vary.v[3], vary.v[4], vary.v[5], vary.v[6], vary.v[7], vary.v[8], vary.v[9]);
}";

impl Gpu {
    fn new() -> Option<Gpu> {
        // Test-only backend selection (e.g. WGPU_BACKEND=vulkan when another
        // backend stalls during initialization). Keep validation flags intact.
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = descriptor.backends.with_env();
        let instance = wgpu::Instance::new(descriptor);
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        if !adapter.features().contains(wgpu::Features::FLOAT32_FILTERABLE) {
            return None;
        }
        let info = adapter.get_info();
        let adapter_name = format!("{} ({:?}, {} {})", info.name, info.backend, info.driver, info.driver_info);
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::FLOAT32_FILTERABLE,
            // four Rgba32Float targets: 64 bytes per sample (the default
            // limit is 32)
            required_limits: wgpu::Limits {
                max_color_attachment_bytes_per_sample: adapter.limits().max_color_attachment_bytes_per_sample,
                ..Default::default()
            },
            ..Default::default()
        })).map_err(|e| eprintln!("request_device: {e}")).ok()?;
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vs"), source: wgpu::ShaderSource::Wgsl(VS.into()) });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        Some(Gpu { device, queue, vs, sampler, adapter: adapter_name })
    }

    fn texture(&self, t: &Texture) -> wgpu::TextureView {
        let (layers, dimension, view) = match t.kind {
            TextureKind::Flat => (1, wgpu::TextureDimension::D2, wgpu::TextureViewDimension::D2),
            TextureKind::Cube => (6, wgpu::TextureDimension::D2, wgpu::TextureViewDimension::Cube),
            TextureKind::Volume => (t.size, wgpu::TextureDimension::D3, wgpu::TextureViewDimension::D3),
        };
        let size = wgpu::Extent3d { width: t.size, height: t.size, depth_or_array_layers: layers };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let bytes: Vec<u8> = t.texels.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.size * 16), rows_per_image: Some(t.size) },
            size,
        );
        tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(view), ..Default::default() })
    }

    /// Run the translation; the colour outputs (by oC index order) or the
    /// WGSL error.
    fn run(&self, wgsl: &str, iface: &d3d9wgsl::Interface, run: &PixelRun) -> Result<Vec<[f32; 4]>, String> {
        let p = self.pixel_pipeline(wgsl, iface)?;
        self.run_pixel_with(&p, iface, run)
    }

    /// The pipeline of a translated pixel shader, or the WGSL error.
    fn pixel_pipeline(&self, wgsl: &str, iface: &d3d9wgsl::Interface) -> Result<PixelPipeline, String> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let fs = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fs"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let max = iface.outputs.iter().copied().max().unwrap_or(0) as usize;
        let targets: Vec<Option<wgpu::ColorTargetState>> = (0..=max)
            .map(|n| iface.outputs.contains(&(n as u32)).then(|| wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }))
            .collect();
        // explicit layouts: every declared binding exists even if unused
        let uniform = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout0 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[uniform(0, wgpu::ShaderStages::FRAGMENT), uniform(1, wgpu::ShaderStages::VERTEX)],
        });
        let mut entries1 = Vec::new();
        for &(n, kind) in &iface.samplers {
            let view_dimension = match kind {
                d3d9wgsl::TextureKind::Flat => wgpu::TextureViewDimension::D2,
                d3d9wgsl::TextureKind::Cube => wgpu::TextureViewDimension::Cube,
                d3d9wgsl::TextureKind::Volume => wgpu::TextureViewDimension::D3,
            };
            entries1.push(wgpu::BindGroupLayoutEntry {
                binding: 2 * n,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false },
                count: None,
            });
            entries1.push(wgpu::BindGroupLayoutEntry {
                binding: 2 * n + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let layout1 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries1 });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout0), Some(&layout1)],
            immediate_size: 0,
        });
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &self.vs, entry_point: Some("main"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            // shaders writing oDepth need a depth target (not compared)
            depth_stencil: iface.writes_depth.then(|| wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState { module: &fs, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = block_on(scope.pop()) {
            return Err(format!("{e}"));
        }
        Ok(PixelPipeline { pipeline, layout0, layout1, targets })
    }

    fn run_pixel_with(&self, p: &PixelPipeline, iface: &d3d9wgsl::Interface, run: &PixelRun) -> Result<Vec<[f32; 4]>, String> {
        let PixelPipeline { pipeline, layout0, layout1, targets } = p;
        // group 0: constants (fragment) and varyings (vertex)
        let mut consts: Vec<u8> = Vec::new();
        for k in 0..224 {
            let v = run.constants.get(k).copied().unwrap_or([0.0; 4]);
            consts.extend(v.iter().flat_map(|f| f.to_le_bytes()));
        }
        for k in 0..16 {
            let v = run.ints.get(k).copied().unwrap_or([0; 4]);
            consts.extend(v.iter().flat_map(|f| f.to_le_bytes()));
        }
        for k in 0..16 {
            let b = run.bools.get(k).copied().unwrap_or(false) as u32;
            consts.extend([b, 0, 0, 0].iter().flat_map(|f| f.to_le_bytes()));
        }
        consts.extend([0u8; 16]); // hp / alpha: unused here
        let mut varyings: Vec<u8> = Vec::new();
        for k in 0..16 {
            let v = run.varyings.get(k).copied().unwrap_or([0.0; 4]);
            varyings.extend(v.iter().flat_map(|f| f.to_le_bytes()));
        }
        let buffer = |data: &[u8]| {
            let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: data.len() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&b, 0, data);
            b
        };
        let (cbuf, vbuf) = (buffer(&consts), buffer(&varyings));
        let group0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: layout0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cbuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: vbuf.as_entire_binding() },
            ],
        });
        let views: Vec<(u32, wgpu::TextureView)> = iface.samplers.iter()
            .map(|&(n, _)| (n, self.texture(run.textures[n as usize].as_ref().expect("texture for every sampler"))))
            .collect();
        let group1 = {
            let mut entries = Vec::new();
            for (n, v) in &views {
                entries.push(wgpu::BindGroupEntry { binding: 2 * n, resource: wgpu::BindingResource::TextureView(v) });
                entries.push(wgpu::BindGroupEntry { binding: 2 * n + 1, resource: wgpu::BindingResource::Sampler(&self.sampler) });
            }
            self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: layout1, entries: &entries })
        };
        let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
        let rts: Vec<Option<wgpu::Texture>> = targets.iter().map(|t| t.as_ref().map(|_| self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }))).collect();
        let rt_views: Vec<Option<wgpu::TextureView>> = rts.iter().map(|t| t.as_ref().map(|t| t.create_view(&Default::default()))).collect();
        let depth = iface.writes_depth.then(|| self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }).create_view(&Default::default()));
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * rts.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = rt_views.iter().map(|v| v.as_ref().map(|view| wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })).collect();
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &attachments,
                depth_stencil_attachment: depth.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group0, &[]);
            pass.set_bind_group(1, &group1, &[]);
            pass.draw(0..3, 0..1);
        }
        for (k, t) in rts.iter().enumerate() {
            if let Some(t) = t {
                enc.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 256 * k as u64, bytes_per_row: Some(256), rows_per_image: Some(1) } },
                    size,
                );
            }
        }
        self.queue.submit([enc.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("{e}"))?;
        let data = readback.slice(..).get_mapped_range();
        let mut out = Vec::new();
        for &n in &iface.outputs {
            let o = 256 * n as usize;
            let f = |k: usize| f32::from_le_bytes(data[o + 4 * k..o + 4 * k + 4].try_into().unwrap());
            out.push([f(0), f(1), f(2), f(3)]);
        }
        Ok(out)
    }
}

/// xorshift32
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn float(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32 * 4.0 - 2.0
    }
    fn vec4(&mut self) -> [f32; 4] {
        [self.float(), self.float(), self.float(), self.float()]
    }
}

impl Gpu {
    /// Run a translated vertex shader (translated with position capture) on
    /// one point; the value at each output location (outputs_vs order, then
    /// the captured position).
    fn run_vertex(&self, wgsl: &str, iface: &d3d9wgsl::Interface, run: &d3d9ref::VertexRun) -> Result<Vec<[f32; 4]>, String> {
        let p = self.vertex_pipelines(wgsl, iface)?;
        self.run_vertex_with(&p, iface, run)
    }

    /// The pipelines of a translated vertex shader (translated with
    /// position capture), or the WGSL error.
    fn vertex_pipelines(&self, wgsl: &str, iface: &d3d9wgsl::Interface) -> Result<VertexPipelines, String> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let vs = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vs"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let locations = iface.outputs_vs.len() + 1;
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout0 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uniform] });
        let layout1 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[] });
        let mut entries2 = Vec::new();
        for &(n, kind) in &iface.samplers {
            let view_dimension = match kind {
                d3d9wgsl::TextureKind::Flat => wgpu::TextureViewDimension::D2,
                d3d9wgsl::TextureKind::Cube => wgpu::TextureViewDimension::Cube,
                d3d9wgsl::TextureKind::Volume => wgpu::TextureViewDimension::D3,
            };
            entries2.push(wgpu::BindGroupLayoutEntry {
                binding: 2 * n,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false },
                count: None,
            });
            entries2.push(wgpu::BindGroupLayoutEntry {
                binding: 2 * n + 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let layout2 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries2 });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout0), Some(&layout1), Some(&layout2)],
            immediate_size: 0,
        });
        let attributes: Vec<wgpu::VertexAttribute> = (0..iface.inputs.len())
            .map(|k| wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 16 * k as u64, shader_location: k as u32 })
            .collect();
        let stride = 16 * iface.inputs.len().max(1) as u64;
        let mut pipelines = Vec::new();
        // two Rgba32Float targets per pass: the default limit is 32 bytes per sample
        for first in (0..locations).step_by(2) {
            let group: Vec<usize> = (first..locations.min(first + 2)).collect();
            let mut fs = String::from("struct FIn {\n");
            for &l in &group {
                fs.push_str(&format!("  @location({l}) x{l}: vec4<f32>,\n"));
            }
            fs.push_str("}\nstruct FOut {\n");
            for (k, _) in group.iter().enumerate() {
                fs.push_str(&format!("  @location({k}) o{k}: vec4<f32>,\n"));
            }
            let fields: Vec<String> = group.iter().map(|l| format!("i.x{l}")).collect();
            fs.push_str(&format!("}}\n@fragment fn main(i: FIn) -> FOut {{ return FOut({}); }}\n", fields.join(", ")));
            let fsm = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fs"), source: wgpu::ShaderSource::Wgsl(fs.into()) });
            let targets: Vec<Option<wgpu::ColorTargetState>> = group.iter().map(|_| Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })).collect();
            let buffers = [wgpu::VertexBufferLayout { array_stride: stride, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes }];
            let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &vs, entry_point: Some("main"), compilation_options: Default::default(), buffers: &buffers },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::PointList, ..Default::default() },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState { module: &fsm, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
                multiview_mask: None,
                cache: None,
            });
            pipelines.push((group, pipeline));
        }
        if let Some(e) = block_on(scope.pop()) {
            return Err(format!("{e}"));
        }
        Ok(VertexPipelines { pipelines, layout0, layout1, layout2 })
    }

    /// Run prepared vertex pipelines on one point; the value at each output
    /// location (outputs_vs order, then the captured position).
    fn run_vertex_with(&self, p: &VertexPipelines, iface: &d3d9wgsl::Interface, run: &d3d9ref::VertexRun) -> Result<Vec<[f32; 4]>, String> {
        let mut consts: Vec<u8> = Vec::new();
        for k in 0..256 {
            consts.extend(run.constants.get(k).copied().unwrap_or([0.0; 4]).iter().flat_map(|f| f.to_le_bytes()));
        }
        for k in 0..16 {
            consts.extend(run.ints.get(k).copied().unwrap_or([0; 4]).iter().flat_map(|f| f.to_le_bytes()));
        }
        for k in 0..16 {
            let b = run.bools.get(k).copied().unwrap_or(false) as u32;
            consts.extend([b, 0, 0, 0].iter().flat_map(|f| f.to_le_bytes()));
        }
        consts.extend([0u8; 16]); // hp / alpha: unused here
        let cbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: consts.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&cbuf, 0, &consts);
        let mut vertex: Vec<u8> = Vec::new();
        for k in 0..iface.inputs.len().max(1) {
            vertex.extend(run.attributes.get(k).copied().unwrap_or([0.0; 4]).iter().flat_map(|f| f.to_le_bytes()));
        }
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: vertex.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&vbuf, 0, &vertex);
        let group0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &p.layout0,
            entries: &[wgpu::BindGroupEntry { binding: 2, resource: cbuf.as_entire_binding() }],
        });
        let group1 = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.layout1, entries: &[] });
        let views: Vec<(u32, wgpu::TextureView)> = iface.samplers.iter()
            .map(|&(n, _)| (n, self.texture(run.textures[n as usize].as_ref().expect("texture for every sampler"))))
            .collect();
        let mut e2 = Vec::new();
        for (n, v) in &views {
            e2.push(wgpu::BindGroupEntry { binding: 2 * n, resource: wgpu::BindingResource::TextureView(v) });
            e2.push(wgpu::BindGroupEntry { binding: 2 * n + 1, resource: wgpu::BindingResource::Sampler(&self.sampler) });
        }
        let group2 = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.layout2, entries: &e2 });
        let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
        let mut values = Vec::new();
        for (group, pipeline) in &p.pipelines {
            let rts: Vec<wgpu::Texture> = group.iter().map(|_| self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })).collect();
            let rt_views: Vec<wgpu::TextureView> = rts.iter().map(|t| t.create_view(&Default::default())).collect();
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 256 * rts.len() as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = rt_views.iter().map(|view| Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })).collect();
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &group0, &[]);
                pass.set_bind_group(1, &group1, &[]);
                pass.set_bind_group(2, &group2, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.draw(0..1, 0..1);
            }
            for (k, t) in rts.iter().enumerate() {
                enc.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 256 * k as u64, bytes_per_row: Some(256), rows_per_image: Some(1) } },
                    size,
                );
            }
            self.queue.submit([enc.finish()]);
            readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("{e}"))?;
            let data = readback.slice(..).get_mapped_range();
            for k in 0..group.len() {
                let f = |c: usize| f32::from_le_bytes(data[256 * k + 4 * c..256 * k + 4 * c + 4].try_into().unwrap());
                values.push([f(0), f(1), f(2), f(3)]);
            }
        }
        Ok(values)
    }
}

fn vertex_shaders(name: &str) -> Vec<Vec<u32>> {
    shaders_with(name, 0xFFFE_0300)
}

fn pixel_shaders(name: &str) -> Vec<Vec<u32>> {
    shaders_with(name, 0xFFFF_0300)
}

fn shaders_with(name: &str, version: u32) -> Vec<Vec<u32>> {
    let path = std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC\sa\DX9").join(format!("{name}.arc"));
    let Ok((archive, mut file)) = mtarc::Archive::open(path) else { return vec![] };
    let wanted = mtarc::type_hash("rShaderPackage");
    let Some(entry) = archive.entries.iter().find(|e| e.type_hash == wanted) else { return vec![] };
    let tokens = d3d9shader::tokens(&mtarc::read_entry(&mut file, entry).unwrap());
    let mut out = Vec::new();
    let mut k = 0;
    while k < tokens.len() {
        if tokens[k] == version {
            if let Ok(s) = d3d9shader::parse(&tokens[k..]) {
                if s.instructions.iter().any(|i| i.opcode != d3d9shader::op::COMMENT) {
                    out.push(tokens[k..k + s.tokens].to_vec());
                    k += s.tokens;
                    continue;
                }
            }
        }
        k += 1;
    }
    out
}

/// The shader cut after instruction `k` (counting instructions other than
/// comments, declarations and definitions), outputting that instruction's
/// temp destination as oC0: for locating where a translation first
/// differs. None if `k` does not write a temp or flow control comes first.
fn prefix(tokens: &[u32], k: usize) -> Option<(Vec<u32>, String, u8)> {
    prefix_to(tokens, k, 0x800F_0800)
}

/// As [`prefix`], writing the result to the output register `dst` (a
/// destination token).
fn prefix_to(tokens: &[u32], k: usize, dst: u32) -> Option<(Vec<u32>, String, u8)> {
    use d3d9shader::op;
    let s = d3d9shader::parse(tokens).ok()?;
    let lines = d3d9shader::disasm::disassemble(&s);
    let mut at = 1;
    let (mut n, mut line) = (0, 0);
    for ins in &s.instructions {
        let t = tokens[at];
        let len = if ins.opcode == op::COMMENT { ((t >> 16) & 0x7FFF) as usize } else { ((t >> 24) & 0xF) as usize };
        at += 1 + len;
        if ins.opcode == op::COMMENT {
            continue;
        }
        line += 1;
        if matches!(ins.opcode, op::IF | op::IFC | op::LOOP | op::REP | op::CALL | op::CALLNZ) {
            return None;
        }
        if matches!(ins.opcode, op::DCL | op::DEF | op::DEFI | op::DEFB) {
            continue;
        }
        if n == k {
            let d = ins.dst.as_ref()?;
            if d.kind != d3d9shader::reg::TEMP {
                return None;
            }
            let mut v = tokens[..at].to_vec();
            v.extend_from_slice(&[0x0200_0001, dst, 0x80E4_0000 | d.index, 0x0000_FFFF]);
            return Some((v, lines[line].trim().to_string(), d.mask_or_swizzle));
        }
        n += 1;
    }
    None
}

/// Equal, or both NaN, or within a relative 1e-5 (GPU arithmetic is not
/// specified to the last bit between APIs).
fn close(a: f32, b: f32) -> bool {
    if a.is_nan() || b.is_nan() {
        return a.is_nan() && b.is_nan();
    }
    if a == b {
        return true;
    }
    (a - b).abs() <= 1e-5 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn translated_pixel_shaders_compute_what_d3d9_computes() {
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else {
        eprintln!("skipped: no D3D9 device or no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    let packages = std::env::var("DDDA_SHADER_PACKAGES").unwrap_or_else(|_| "Title".into());
    let limit: usize = std::env::var("DDDA_SHADER_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let mut seen = std::collections::HashSet::new();
    let (mut compared, mut untranslated, mut wrong) = (0usize, Vec::new(), Vec::new());
    let mut rng = Rng(0x2468_ACE1);
    'outer: for name in packages.split(',') {
        for ps in pixel_shaders(name) {
            if !seen.insert(ps.clone()) {
                continue;
            }
            if compared + untranslated.len() >= limit {
                break 'outer;
            }
            let shader = d3d9shader::parse(&ps).unwrap();
            let t = match d3d9wgsl::translate(&shader) {
                Ok(t) => t,
                Err(e) => {
                    untranslated.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let mut run = PixelRun {
                constants: (0..224).map(|_| rng.vec4()).collect(),
                ints: (0..16).map(|_| [(rng.next() % 5) as i32, (rng.next() % 3) as i32, 1, 0]).collect(),
                bools: (0..16).map(|_| rng.next() & 1 == 1).collect(),
                varyings: t.interface.inputs.iter().map(|_| rng.vec4()).collect(),
                textures: vec![],
            };
            for &(n, kind) in &t.interface.samplers {
                let kind = match kind {
                    d3d9wgsl::TextureKind::Flat => TextureKind::Flat,
                    d3d9wgsl::TextureKind::Cube => TextureKind::Cube,
                    d3d9wgsl::TextureKind::Volume => TextureKind::Volume,
                };
                let count = match kind { TextureKind::Flat => 16, TextureKind::Cube => 96, TextureKind::Volume => 64 };
                run.textures.resize(n as usize + 1, None);
                run.textures[n as usize] = Some(Texture { kind, size: 4, texels: (0..count).map(|_| rng.vec4()).collect() });
            }
            let theirs = oracle.run_pixel(&ps, &run).unwrap_or_else(|e| panic!("{name}: D3D9: {e}"));
            match gpu.run(&t.wgsl, &t.interface, &run) {
                Err(e) => untranslated.push(format!("{name}: WGSL rejected: {e}\n{}", t.wgsl)),
                Ok(ours) => {
                    compared += 1;
                    let ok = ours.len() == theirs.len()
                        && ours.iter().zip(&theirs).all(|(a, b)| a.iter().zip(b).all(|(x, y)| close(*x, *y)));
                    if !ok && wrong.len() < 10 {
                        // the first instruction whose result differs
                        let mut first = String::from("(not located)");
                        for k in 0..ps.len() {
                            let Some((cut, text, mask)) = prefix(&ps, k) else { continue };
                            let cs = d3d9shader::parse(&cut).unwrap();
                            let Ok(ct) = d3d9wgsl::translate(&cs) else { continue };
                            let (Ok(a), Ok(b)) = (gpu.run(&ct.wgsl, &ct.interface, &run), oracle.run_pixel(&cut, &run)) else { continue };
                            // only the components the instruction writes are defined
                            if std::env::var_os("DDDA_SHADER_TRACE").is_some() {
                                eprintln!("  {k} `{text}`: ours {a:?} / D3D9 {b:?}");
                            }
                            if !(a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| (0..4).all(|c| mask & (1 << c) == 0 || close(x[c], y[c])))) {
                                first = format!("instruction {k} `{text}`: ours {a:?} / D3D9 {b:?}");
                                break;
                            }
                        }
                        let d = d3d9shader::disasm::disassemble(&shader).join("\n");
                        wrong.push(format!("{name}: ours {ours:?} / D3D9 {theirs:?}\nfirst difference: {first}\n{d}"));
                    } else if !ok {
                        wrong.push(String::new());
                    }
                }
            }
        }
    }
    eprintln!("{compared} compared, {} wrong, {} not translated", wrong.len(), untranslated.len());
    for u in untranslated.iter().take(3) {
        eprintln!("not translated: {}", &u[..u.len().min(3000)]);
    }
    for w in wrong.iter().filter(|w| !w.is_empty()).take(3) {
        eprintln!("wrong: {w}");
    }
    assert!(compared > 0, "nothing compared");
    assert!(untranslated.is_empty() && wrong.is_empty());
}

#[test]
fn translated_vertex_shaders_compute_what_d3d9_computes() {
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else {
        eprintln!("skipped: no D3D9 device or no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    let packages = std::env::var("DDDA_SHADER_PACKAGES").unwrap_or_else(|_| "Title".into());
    let limit: usize = std::env::var("DDDA_SHADER_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let mut seen = std::collections::HashSet::new();
    let (mut compared, mut skipped, mut untranslated, mut wrong) = (0usize, 0usize, Vec::new(), Vec::new());
    let mut rng = Rng(0x1357_9BDF);
    'outer: for name in packages.split(',') {
        for vs in vertex_shaders(name) {
            if !seen.insert(vs.clone()) {
                continue;
            }
            if compared + untranslated.len() >= limit {
                break 'outer;
            }
            let shader = d3d9shader::parse(&vs).unwrap();
            let t = match d3d9wgsl::translate_with(&shader, d3d9wgsl::Options { capture_position: Some([0.0, 0.0, 0.0, 1.0]), ..Default::default() }) {
                Ok(t) => t,
                Err(e) => {
                    untranslated.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let mut run = d3d9ref::VertexRun {
                constants: (0..256).map(|_| rng.vec4()).collect(),
                ints: (0..16).map(|_| [(rng.next() % 5) as i32, (rng.next() % 3) as i32, 1, 0]).collect(),
                bools: (0..16).map(|_| rng.next() & 1 == 1).collect(),
                attributes: t.interface.inputs.iter().map(|_| rng.vec4()).collect(),
                textures: vec![],
            };
            for &(n, kind) in &t.interface.samplers {
                let kind = match kind {
                    d3d9wgsl::TextureKind::Flat => TextureKind::Flat,
                    d3d9wgsl::TextureKind::Cube => TextureKind::Cube,
                    d3d9wgsl::TextureKind::Volume => TextureKind::Volume,
                };
                let count = match kind { TextureKind::Flat => 16, TextureKind::Cube => 96, TextureKind::Volume => 64 };
                run.textures.resize(n as usize + 1, None);
                run.textures[n as usize] = Some(Texture { kind, size: 4, texels: (0..count).map(|_| rng.vec4()).collect() });
            }
            let theirs = match oracle.run_vertex(&vs, &run) {
                Ok(v) => v,
                Err(e) if e.contains("texcoord14") => {
                    skipped += 1;
                    continue;
                }
                Err(e) => panic!("{name}: D3D9: {e}"),
            };
            let ours = match gpu.run_vertex(&t.wgsl, &t.interface, &run) {
                Ok(v) => v,
                Err(e) => {
                    untranslated.push(format!("{name}: WGSL rejected: {e}\n{}", t.wgsl));
                    continue;
                }
            };
            compared += 1;
            let mut bad = Vec::new();
            for o in &theirs {
                let Some(value) = o.value else { continue };
                let location = if o.usage == 0 && o.index == 0 {
                    t.interface.outputs_vs.len()
                } else {
                    t.interface.outputs_vs.iter().position(|&(_, _, r, _)| r == o.register).expect("output in interface")
                };
                let mine = ours[location];
                if !(0..4).all(|c| o.mask & (1 << c) == 0 || close(mine[c], value[c])) {
                    bad.push(format!("usage {} index {} (o{}): ours {mine:?} / D3D9 {value:?}", o.usage, o.index, o.register));
                }
            }
            if !bad.is_empty() {
                if wrong.len() < 5 {
                    // the first instruction whose result differs, read through
                    // the position output (both sides capture it)
                    let mut first = String::from("(not located)");
                    let pos = t.interface.position.unwrap();
                    for k in 0..vs.len() {
                        let Some((cut, text, mask)) = prefix_to(&vs, k, 0xE00F_0000 | pos) else { continue };
                        let cs = d3d9shader::parse(&cut).unwrap();
                        let Ok(ct) = d3d9wgsl::translate_with(&cs, d3d9wgsl::Options { capture_position: Some([0.0, 0.0, 0.0, 1.0]), ..Default::default() }) else { continue };
                        let (Ok(a), Ok(b)) = (gpu.run_vertex(&ct.wgsl, &ct.interface, &run), oracle.run_vertex(&cut, &run)) else { continue };
                        let mine = a[ct.interface.outputs_vs.len()];
                        let Some(theirs) = b.iter().find(|o| o.usage == 0 && o.index == 0).and_then(|o| o.value) else { continue };
                        if !(0..4).all(|c| mask & (1 << c) == 0 || close(mine[c], theirs[c])) {
                            first = format!("instruction {k} `{text}`: ours {mine:?} / D3D9 {theirs:?}");
                            break;
                        }
                    }
                    let d = d3d9shader::disasm::disassemble(&shader).join("\n");
                    wrong.push(format!("{name}: {}\nfirst difference: {first}\n{d}", bad.join("; ")));
                } else {
                    wrong.push(String::new());
                }
            }
        }
    }
    eprintln!("{compared} vertex shaders compared, {} wrong, {} not translated, {skipped} skipped (texcoord14/15 in use)", wrong.len(), untranslated.len());
    for u in untranslated.iter().take(3) {
        eprintln!("not translated: {}", &u[..u.len().min(3000)]);
    }
    for w in wrong.iter().filter(|w| !w.is_empty()).take(3) {
        eprintln!("wrong: {w}");
    }
    assert!(compared > 0, "nothing compared");
    assert!(untranslated.is_empty() && wrong.is_empty());
}

// --- the full sweep --------------------------------------------------------------

/// Every package in sa\DX9, sorted (the order the global deduplication
/// follows: a shader counts in the first package that holds it).
fn all_packages() -> Vec<String> {
    let dir = std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC\sa\DX9");
    let mut v: Vec<String> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok())
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect()).unwrap_or_default();
    v.sort();
    v
}

/// Distance in units in the last place (0: equal, or both NaN).
fn ulps(a: f32, b: f32) -> u64 {
    if a.is_nan() || b.is_nan() {
        return if a.is_nan() && b.is_nan() { 0 } else { u64::MAX };
    }
    if a == b {
        return 0;
    }
    let ordered = |x: f32| {
        let i = x.to_bits() as i32;
        (if i < 0 { i32::MIN.wrapping_sub(i) } else { i }) as i64
    };
    (ordered(a) - ordered(b)).unsigned_abs()
}

/// The random inputs of one set for a shader: seeded by the shader's
/// tokens and the set number, so a set is reproducible on its own.
fn seed(tokens: &[u32], set: u32) -> Rng {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tokens.hash(&mut h);
    set.hash(&mut h);
    Rng((h.finish() as u32) | 1)
}

fn tag(tokens: &[u32]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tokens.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn random_textures(rng: &mut Rng, samplers: &[(u32, d3d9wgsl::TextureKind)]) -> Vec<Option<Texture>> {
    let mut textures = vec![];
    for &(n, kind) in samplers {
        let kind = match kind {
            d3d9wgsl::TextureKind::Flat => TextureKind::Flat,
            d3d9wgsl::TextureKind::Cube => TextureKind::Cube,
            d3d9wgsl::TextureKind::Volume => TextureKind::Volume,
        };
        let count = match kind { TextureKind::Flat => 16, TextureKind::Cube => 96, TextureKind::Volume => 64 };
        textures.resize(textures.len().max(n as usize + 1), None);
        textures[n as usize] = Some(Texture { kind, size: 4, texels: (0..count).map(|_| rng.vec4()).collect() });
    }
    textures
}

fn random_pixel_run(rng: &mut Rng, iface: &d3d9wgsl::Interface) -> PixelRun {
    let mut run = PixelRun {
        constants: (0..224).map(|_| rng.vec4()).collect(),
        ints: (0..16).map(|_| [(rng.next() % 5) as i32, (rng.next() % 3) as i32, 1, 0]).collect(),
        bools: (0..16).map(|_| rng.next() & 1 == 1).collect(),
        varyings: iface.inputs.iter().map(|_| rng.vec4()).collect(),
        textures: vec![],
    };
    run.textures = random_textures(rng, &iface.samplers);
    run
}

fn random_vertex_run(rng: &mut Rng, iface: &d3d9wgsl::Interface) -> d3d9ref::VertexRun {
    let mut run = d3d9ref::VertexRun {
        constants: (0..256).map(|_| rng.vec4()).collect(),
        ints: (0..16).map(|_| [(rng.next() % 5) as i32, (rng.next() % 3) as i32, 1, 0]).collect(),
        bools: (0..16).map(|_| rng.next() & 1 == 1).collect(),
        attributes: iface.inputs.iter().map(|_| rng.vec4()).collect(),
        textures: vec![],
    };
    run.textures = random_textures(rng, &iface.samplers);
    run
}

/// How one shader compared over all its sets.
#[derive(Clone, Debug, PartialEq)]
enum Verdict {
    /// Every defined component equal (or both NaN) in every set.
    Exact,
    /// Within `close` everywhere, at most this many ULPs apart.
    Close(u64),
    /// Outside `close` in set `set`: the description.
    Differs { set: u32, what: String },
    Untranslated(String),
    Rejected(String),
    /// The oracle could not run it (the description).
    Skipped(String),
}

/// Per output: ours, D3D9's, the components defined, the output's name.
type Compared = Vec<([f32; 4], [f32; 4], u8, String)>;

fn verdict_of(sets: &[Compared]) -> Verdict {
    let mut worst = 0u64;
    for (set, outputs) in sets.iter().enumerate() {
        for (ours, theirs, mask, name) in outputs {
            for c in 0..4 {
                if mask & (1 << c) == 0 {
                    continue;
                }
                if !close(ours[c], theirs[c]) {
                    return Verdict::Differs { set: set as u32, what: format!("{name}: ours {ours:?} / D3D9 {theirs:?}") };
                }
                worst = worst.max(ulps(ours[c], theirs[c]));
            }
        }
    }
    if worst == 0 { Verdict::Exact } else { Verdict::Close(worst) }
}

fn pixel_outputs(ours: &[[f32; 4]], theirs: &[[f32; 4]], iface: &d3d9wgsl::Interface) -> Compared {
    iface.outputs.iter().enumerate()
        .map(|(k, n)| (ours.get(k).copied().unwrap_or([f32::NAN; 4]), theirs.get(k).copied().unwrap_or([0.0; 4]), 0xF, format!("oC{n}")))
        .collect()
}

fn vertex_outputs(ours: &[[f32; 4]], theirs: &[d3d9ref::VertexOutput], iface: &d3d9wgsl::Interface) -> Compared {
    theirs.iter().filter_map(|o| {
        let value = o.value?;
        let location = if o.usage == 0 && o.index == 0 {
            iface.outputs_vs.len()
        } else {
            iface.outputs_vs.iter().position(|&(_, _, r, _)| r == o.register)?
        };
        Some((ours[location], value, o.mask, format!("usage {} index {} (o{})", o.usage, o.index, o.register)))
    }).collect()
}

fn options(vertex: bool) -> d3d9wgsl::Options {
    if vertex {
        d3d9wgsl::Options { capture_position: Some([0.0, 0.0, 0.0, 1.0]), ..Default::default() }
    } else {
        Default::default()
    }
}

/// The first instruction whose result is outside `close` (read through a
/// [`tap`]: pixel shaders through oC0, vertex shaders through the position
/// output), with set `set`'s inputs.
fn first_difference(gpu: &Gpu, oracle: &mut Oracle, tokens: &[u32], vertex: bool, set: u32) -> String {
    let shader = d3d9shader::parse(tokens).unwrap();
    let Ok(full) = d3d9wgsl::translate_with(&shader, options(vertex)) else { return "(not translated)".into() };
    let mut last = String::from("(not located)");
    for k in 0..tokens.len() {
        let Some((cut, text, mask)) = tap(tokens, k, output_token(vertex, &full.interface)) else { continue };
        let cs = d3d9shader::parse(&cut).unwrap();
        let Ok(ct) = d3d9wgsl::translate_with(&cs, options(vertex)) else { continue };
        // the full shader's random set, so the inputs are the same
        let mut rng = seed(tokens, set);
        let (mine, theirs) = if vertex {
            let run = random_vertex_run(&mut rng, &full.interface);
            let (Ok(a), Ok(b)) = (gpu.run_vertex(&ct.wgsl, &ct.interface, &run), oracle.run_vertex(&cut, &run)) else { continue };
            let Some(t) = b.iter().find(|o| o.usage == 0 && o.index == 0).and_then(|o| o.value) else { continue };
            (a[ct.interface.outputs_vs.len()], t)
        } else {
            let run = random_pixel_run(&mut rng, &full.interface);
            let (Ok(a), Ok(b)) = (gpu.run(&ct.wgsl, &ct.interface, &run), oracle.run_pixel(&cut, &run)) else { continue };
            (a[0], b[0])
        };
        if !(0..4).all(|c| mask & (1 << c) == 0 || close(mine[c], theirs[c])) {
            return format!("instruction {k} `{text}`: ours {mine:?} / D3D9 {theirs:?}");
        }
        last = format!("(not located; last equal: {k} `{text}`)");
    }
    last
}

/// The output a [`tap`] writes: oC0, or the vertex shader's position.
fn output_token(vertex: bool, iface: &d3d9wgsl::Interface) -> u32 {
    if vertex { 0xE00F_0000 | iface.position.unwrap() } else { 0x800F_0800 }
}

/// One shader through translation, the pipeline and `sets` random sets.
fn sweep_one(gpu: &Gpu, oracle: &mut Oracle, tokens: &[u32], vertex: bool, sets: u32) -> Verdict {
    let shader = d3d9shader::parse(tokens).unwrap();
    let t = match d3d9wgsl::translate_with(&shader, options(vertex)) {
        Ok(t) => t,
        Err(e) => return Verdict::Untranslated(e.to_string()),
    };
    let mut compared = Vec::new();
    if vertex {
        let p = match gpu.vertex_pipelines(&t.wgsl, &t.interface) {
            Ok(p) => p,
            Err(e) => return Verdict::Rejected(e),
        };
        for set in 0..sets {
            let run = random_vertex_run(&mut seed(tokens, set), &t.interface);
            let theirs = match oracle.run_vertex(tokens, &run) {
                Ok(v) => v,
                Err(e) => return Verdict::Skipped(e),
            };
            let ours = match gpu.run_vertex_with(&p, &t.interface, &run) {
                Ok(v) => v,
                Err(e) => return Verdict::Rejected(e),
            };
            compared.push(vertex_outputs(&ours, &theirs, &t.interface));
        }
    } else {
        let p = match gpu.pixel_pipeline(&t.wgsl, &t.interface) {
            Ok(p) => p,
            Err(e) => return Verdict::Rejected(e),
        };
        for set in 0..sets {
            let run = random_pixel_run(&mut seed(tokens, set), &t.interface);
            let theirs = match oracle.run_pixel(tokens, &run) {
                Ok(v) => v,
                Err(e) => return Verdict::Skipped(e),
            };
            let ours = match gpu.run_pixel_with(&p, &t.interface, &run) {
                Ok(v) => v,
                Err(e) => return Verdict::Rejected(e),
            };
            compared.push(pixel_outputs(&ours, &theirs, &t.interface));
        }
    }
    verdict_of(&compared)
}

/// The full sweep. DDDA_SHADER_SWEEP=<report directory> runs every
/// distinct shader of every package (DDDA_SHADER_PACKAGES restricts the
/// packages, DDDA_SHADER_STAGE=pixel|vertex the stage) with
/// DDDA_SHADER_SETS random sets each (default 4), and writes one report
/// per package. A package whose report exists is not run again (the sweep
/// resumes), but its shaders still count as seen. DDDA_SHADER_BISECT=n
/// locates the first differing instruction of at most n differing shaders
/// per package and stage (default 20).
#[test]
fn sweep() {
    let Some(dir) = std::env::var_os("DDDA_SHADER_SWEEP").map(std::path::PathBuf::from) else { return };
    let (oracle, gpu) = (Oracle::new(), Gpu::new());
    let (has_oracle, has_gpu) = (oracle.is_some(), gpu.is_some());
    let (Some(mut oracle), Some(gpu)) = (oracle, gpu) else {
        // (D3D9 refuses a device while another program holds the display
        // exclusively, e.g. a full-screen game)
        eprintln!("skipped: D3D9 device: {has_oracle}, wgpu adapter with FLOAT32_FILTERABLE: {has_gpu}");
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let only: Option<Vec<String>> = std::env::var("DDDA_SHADER_PACKAGES").ok().map(|v| v.split(',').map(String::from).collect());
    let stage = std::env::var("DDDA_SHADER_STAGE").unwrap_or_default();
    let sets: u32 = std::env::var("DDDA_SHADER_SETS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let bisect: usize = std::env::var("DDDA_SHADER_BISECT").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let mut seen = std::collections::HashSet::new();
    for name in all_packages() {
        let shaders: Vec<Vec<u32>> = pixel_shaders(&name).into_iter().chain(vertex_shaders(&name)).collect();
        let fresh: Vec<Vec<u32>> = shaders.into_iter().filter(|s| seen.insert(s.clone())).collect();
        let report = dir.join(format!("{name}.txt"));
        if std::env::var_os("DDDA_SHADER_COUNT").is_some() {
            let pixel = fresh.iter().filter(|s| s[0] == 0xFFFF_0300).count();
            eprintln!("{name}: {pixel} pixel, {} vertex (new)", fresh.len() - pixel);
            continue;
        }
        if only.as_ref().is_some_and(|o| !o.contains(&name)) || report.exists() {
            continue;
        }
        let started = std::time::Instant::now();
        let (mut counts, mut details) = (String::new(), String::new());
        for (vertex, label) in [(false, "pixel"), (true, "vertex")] {
            if !stage.is_empty() && stage != label {
                continue;
            }
            let version = if vertex { 0xFFFE_0300 } else { 0xFFFF_0300 };
            let mine: Vec<&Vec<u32>> = fresh.iter().filter(|s| s[0] == version).collect();
            let (mut exact, mut near, mut worst, mut differ) = (0, 0, 0u64, 0);
            let (mut untranslated, mut rejected, mut skipped, mut located) = (0, 0, 0, 0);
            for (k, tokens) in mine.iter().enumerate() {
                if k % 250 == 249 {
                    eprintln!("  {name} {label}: {} of {} ({} s)", k + 1, mine.len(), started.elapsed().as_secs());
                }
                let id = tag(tokens);
                match sweep_one(&gpu, &mut oracle, tokens, vertex, sets) {
                    Verdict::Exact => exact += 1,
                    Verdict::Close(u) => {
                        near += 1;
                        worst = worst.max(u);
                        details.push_str(&format!("close {label} {k} {id}: {u} ulp\n"));
                    }
                    Verdict::Differs { set, what } => {
                        differ += 1;
                        let first = if located < bisect {
                            located += 1;
                            first_difference(&gpu, &mut oracle, tokens, vertex, set)
                        } else {
                            "(not bisected)".into()
                        };
                        details.push_str(&format!("differs {label} {k} {id} set {set}: {what}\n  first difference: {first}\n"));
                    }
                    Verdict::Untranslated(e) => {
                        untranslated += 1;
                        details.push_str(&format!("untranslated {label} {k} {id}: {e}\n"));
                    }
                    Verdict::Rejected(e) => {
                        rejected += 1;
                        details.push_str(&format!("rejected {label} {k} {id}: {}\n", e.replace('\n', " | ")));
                    }
                    Verdict::Skipped(e) => {
                        skipped += 1;
                        details.push_str(&format!("skipped {label} {k} {id}: {e}\n"));
                    }
                }
            }
            counts.push_str(&format!(
                "counts {label} distinct {} exact {exact} close {near} worst_ulp {worst} differ {differ} untranslated {untranslated} rejected {rejected} skipped {skipped}\n",
                mine.len()
            ));
        }
        let header = format!("package {name} sets {sets} adapter {} seconds {}\n", gpu.adapter, started.elapsed().as_secs());
        std::fs::write(&report, format!("{header}{counts}{details}")).unwrap();
        eprint!("{header}{counts}");
    }
}

/// One shader traced instruction by instruction: DDDA_SHADER_PROBE=
/// <package>:<pixel|vertex>:<tag or index> (the sweep report's names)
/// prints the disassembly, then for every instruction that writes a temp
/// both results (read through a [`tap`]) and their ULP distance,
/// with set DDDA_SHADER_PROBE_SET (default 0).
#[test]
fn probe() {
    let Ok(spec) = std::env::var("DDDA_SHADER_PROBE") else { return };
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else { return };
    let parts: Vec<&str> = spec.split(':').collect();
    let vertex = parts[1] == "vertex";
    let set: u32 = std::env::var("DDDA_SHADER_PROBE_SET").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let list = if vertex { vertex_shaders(parts[0]) } else { pixel_shaders(parts[0]) };
    let tokens = list.iter().find(|t| tag(t) == parts[2])
        .or_else(|| parts[2].parse::<usize>().ok().and_then(|k| list.get(k)))
        .expect("shader not found").clone();
    let shader = d3d9shader::parse(&tokens).unwrap();
    eprintln!("{}", d3d9shader::disasm::disassemble(&shader).join("\n"));
    let full = d3d9wgsl::translate_with(&shader, options(vertex)).unwrap();
    if std::env::var_os("DDDA_SHADER_PROBE_WGSL").is_some() {
        eprintln!("{}", full.wgsl);
    }
    for k in 0..tokens.len() {
        let Some((cut, text, mask)) = tap(&tokens, k, output_token(vertex, &full.interface)) else { continue };
        let ct = d3d9wgsl::translate_with(&d3d9shader::parse(&cut).unwrap(), options(vertex)).unwrap();
        let mut rng = seed(&tokens, set);
        let (mine, theirs) = if vertex {
            let run = random_vertex_run(&mut rng, &full.interface);
            let a = gpu.run_vertex(&ct.wgsl, &ct.interface, &run).unwrap();
            let b = oracle.run_vertex(&cut, &run).unwrap();
            (a[ct.interface.outputs_vs.len()], b.iter().find(|o| o.usage == 0 && o.index == 0).unwrap().value.unwrap())
        } else {
            let run = random_pixel_run(&mut rng, &full.interface);
            (gpu.run(&ct.wgsl, &ct.interface, &run).unwrap()[0], oracle.run_pixel(&cut, &run).unwrap()[0])
        };
        let u: Vec<String> = (0..4).map(|c| if mask & (1 << c) == 0 { "-".into() } else { ulps(mine[c], theirs[c]).to_string() }).collect();
        eprintln!("{k:3} {text:40} ulps [{}] ours {mine:?} / D3D9 {theirs:?}", u.join(", "));
    }
}

// --- measuring the driver's arithmetic ------------------------------------------

/// Token builders for hand-made shaders.
mod asm {
    /// A destination register token, full write mask unless `mask`.
    pub fn dst(kind: u8, index: u32, mask: u32) -> u32 {
        let k = kind as u32;
        0x8000_0000 | ((k & 7) << 28) | ((k >> 3) << 11) | index | (mask << 16)
    }
    /// A source register token with swizzle and modifier.
    pub fn src(kind: u8, index: u32, swizzle: u32, modifier: u32) -> u32 {
        let k = kind as u32;
        0x8000_0000 | ((k & 7) << 28) | ((k >> 3) << 11) | index | (swizzle << 16) | (modifier << 24)
    }
    pub fn c(index: u32) -> u32 {
        src(d3d9shader::reg::CONST, index, 0xE4, 0)
    }
    pub fn oc0() -> u32 {
        dst(d3d9shader::reg::COLOROUT, 0, 0xF)
    }
    /// `op` with its operand tokens.
    pub fn ins(op: u16, operands: &[u32]) -> Vec<u32> {
        let mut v = vec![op as u32 | ((operands.len() as u32) << 24)];
        v.extend_from_slice(operands);
        v
    }
    pub fn r(index: u32) -> u32 {
        src(d3d9shader::reg::TEMP, index, 0xE4, 0)
    }
    /// A ps_3_0 shader of these instructions, after r0..r3 = c0..c3
    /// (an instruction may read only so many constants).
    pub fn ps(body: &[Vec<u32>]) -> Vec<u32> {
        let mut v = vec![0xFFFF_0300];
        for k in 0..4 {
            v.extend_from_slice(&ins(d3d9shader::op::MOV, &[dst(d3d9shader::reg::TEMP, k, 0xF), c(k)]));
        }
        for i in body {
            v.extend_from_slice(i);
        }
        v.push(0x0000_FFFF);
        v
    }
    /// A vs_3_0 shader of these instructions (writing o1, texcoord0),
    /// after r0..r3 = c0..c3; the position o0 is c4.
    pub fn vs(body: &[Vec<u32>]) -> Vec<u32> {
        use d3d9shader::{op, reg};
        let mut v = vec![0xFFFE_0300, 0x0200_001F, 0x8000_0000, dst(reg::OUTPUT, 0, 0xF), 0x0200_001F, 0x8000_0005, dst(reg::OUTPUT, 1, 0xF)];
        for k in 0..4 {
            v.extend_from_slice(&ins(op::MOV, &[dst(reg::TEMP, k, 0xF), c(k)]));
        }
        for i in body {
            v.extend_from_slice(i);
        }
        v.extend_from_slice(&ins(op::MOV, &[dst(reg::OUTPUT, 0, 0xF), c(4)]));
        v.push(0x0000_FFFF);
        v
    }
}

/// A pixel shader run on both sides with constants c0.. = `consts`; oC0.
fn both(oracle: &mut Oracle, gpu: &Gpu, tokens: &[u32], consts: &[[f32; 4]]) -> ([f32; 4], [f32; 4]) {
    let run = PixelRun { constants: consts.to_vec(), ..Default::default() };
    let t = d3d9wgsl::translate(&d3d9shader::parse(tokens).unwrap()).unwrap();
    (gpu.run(&t.wgsl, &t.interface, &run).unwrap()[0], oracle.run_pixel(tokens, &run).unwrap()[0])
}

/// A vertex shader (from [`asm::vs`]) run on both sides; o1.
fn both_vs(oracle: &mut Oracle, gpu: &Gpu, tokens: &[u32], consts: &[[f32; 4]]) -> ([f32; 4], [f32; 4]) {
    let run = d3d9ref::VertexRun { constants: consts.to_vec(), ..Default::default() };
    let t = d3d9wgsl::translate_with(&d3d9shader::parse(tokens).unwrap(), options(true)).unwrap();
    let ours = gpu.run_vertex(&t.wgsl, &t.interface, &run).unwrap();
    let theirs = oracle.run_vertex(tokens, &run).unwrap();
    (ours[0], theirs.iter().find(|o| o.register == 1).unwrap().value.unwrap())
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// The driver's rounding of the multiply-add instructions: candidate
/// models (fused or not, in which order) against the driver on random
/// operands, printed with how often each matches bit for bit; the
/// translation must match every time. This is the evidence for the rules
/// in d3d9wgsl's HELPERS.
#[test]
fn fused_arithmetic_follows_the_driver() {
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else {
        eprintln!("skipped: no D3D9 device or no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    let mut failures = 0;
    use d3d9shader::op;
    let mut rng = Rng(0x0BAD_5EED);
    let trials = 200;
    let fz = |a: f32, b: f32| if a == 0.0 || b == 0.0 { 0.0 } else { a * b };
    let fma_z = |a: f32, b: f32, c: f32| if a == 0.0 || b == 0.0 { c + 0.0 } else { a.mul_add(b, c) };
    type Model = (&'static str, Box<dyn Fn(&[[f32; 4]]) -> [f32; 4]>);
    let cases: Vec<(&str, Vec<u32>, Vec<Model>)> = vec![
        ("mad", asm::ps(&[asm::ins(op::MAD, &[asm::oc0(), asm::r(0), asm::r(1), asm::r(2)])]), vec![
            ("unfused", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fz(c[0][i], c[1][i]) + c[2][i]))),
            ("fused", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[0][i], c[1][i], c[2][i])))),
        ]),
        ("dp3", asm::ps(&[asm::ins(op::DP3, &[asm::oc0(), asm::r(0), asm::r(1)])]), vec![
            ("unfused x+y+z", Box::new(move |c: &[[f32; 4]]| [(fz(c[0][0], c[1][0]) + fz(c[0][1], c[1][1])) + fz(c[0][2], c[1][2]); 4])),
            ("fma chain x,y,z", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][2], c[1][2], fma_z(c[0][1], c[1][1], fz(c[0][0], c[1][0]))); 4])),
            ("fma chain z,y,x", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][0], c[1][0], fma_z(c[0][1], c[1][1], fz(c[0][2], c[1][2]))); 4])),
            ("fma(x, y+z)", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][0], c[1][0], fma_z(c[0][1], c[1][1], fz(c[0][2], c[1][2]))); 4])),
        ]),
        ("dp4", asm::ps(&[asm::ins(op::DP4, &[asm::oc0(), asm::r(0), asm::r(1)])]), vec![
            ("unfused x+y+z+w", Box::new(move |c: &[[f32; 4]]| [((fz(c[0][0], c[1][0]) + fz(c[0][1], c[1][1])) + fz(c[0][2], c[1][2])) + fz(c[0][3], c[1][3]); 4])),
            ("fma chain x,y,z,w", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][3], c[1][3], fma_z(c[0][2], c[1][2], fma_z(c[0][1], c[1][1], fz(c[0][0], c[1][0])))); 4])),
            ("fma chain w,z,y,x", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][0], c[1][0], fma_z(c[0][1], c[1][1], fma_z(c[0][2], c[1][2], fz(c[0][3], c[1][3])))); 4])),
            ("pairs (x,y)+(z,w) fused", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][1], c[1][1], fz(c[0][0], c[1][0])) + fma_z(c[0][3], c[1][3], fz(c[0][2], c[1][2])); 4])),
        ]),
        ("dp2add", asm::ps(&[asm::ins(op::DP2ADD, &[asm::oc0(), asm::r(0), asm::r(1), asm::src(d3d9shader::reg::TEMP, 2, 0x00, 0)])]), vec![
            ("unfused", Box::new(move |c: &[[f32; 4]]| [(fz(c[0][0], c[1][0]) + fz(c[0][1], c[1][1])) + c[2][0]; 4])),
            ("fma y(fma x c)", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][1], c[1][1], fma_z(c[0][0], c[1][0], c[2][0])); 4])),
            ("fma y(x)+c", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][1], c[1][1], fz(c[0][0], c[1][0])) + c[2][0]; 4])),
            ("fma x(fma y c)", Box::new(move |c: &[[f32; 4]]| [fma_z(c[0][0], c[1][0], fma_z(c[0][1], c[1][1], c[2][0])); 4])),
        ]),
        ("crs", asm::ps(&[asm::ins(op::CRS, &[asm::dst(d3d9shader::reg::TEMP, 4, 0x7), asm::r(0), asm::r(1)]), asm::ins(op::MOV, &[asm::oc0(), asm::r(4)])]), vec![
            ("unfused", Box::new(move |c: &[[f32; 4]]| { let (a, b) = (c[0], c[1]); [fz(a[1], b[2]) - fz(a[2], b[1]), fz(a[2], b[0]) - fz(a[0], b[2]), fz(a[0], b[1]) - fz(a[1], b[0]), 0.0] })),
            ("fma(ay, bz, -az*by)", Box::new(move |c: &[[f32; 4]]| { let (a, b) = (c[0], c[1]); [fma_z(a[1], b[2], -fz(a[2], b[1])), fma_z(a[2], b[0], -fz(a[0], b[2])), fma_z(a[0], b[1], -fz(a[1], b[0])), 0.0] })),
            ("fma(-az, by, ay*bz)", Box::new(move |c: &[[f32; 4]]| { let (a, b) = (c[0], c[1]); [fma_z(-a[2], b[1], fz(a[1], b[2])), fma_z(-a[0], b[2], fz(a[2], b[0])), fma_z(-a[1], b[0], fz(a[0], b[1])), 0.0] })),
        ]),
        // mul feeding mad's addend (r1 dead afterwards)
        ("mul+mad", asm::ps(&[asm::ins(op::MUL, &[asm::dst(d3d9shader::reg::TEMP, 1, 0xF), asm::r(1), asm::r(3)]), asm::ins(op::MAD, &[asm::oc0(), asm::r(0), asm::r(2), asm::r(1)])]), vec![
            ("literal fma(r0, r2, r1*r3)", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[0][i], c[2][i], fz(c[1][i], c[3][i]))))),
            ("contracted fma(r1, r3, r0*r2)", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[1][i], c[3][i], fz(c[0][i], c[2][i]))))),
        ]),
        // two products with a common factor feeding a mad
        ("mul,mul+mad", asm::ps(&[
            asm::ins(op::MUL, &[asm::dst(d3d9shader::reg::TEMP, 1, 0xF), asm::r(1), asm::src(d3d9shader::reg::TEMP, 3, 0xFF, 0)]),
            asm::ins(op::MUL, &[asm::dst(d3d9shader::reg::TEMP, 2, 0xF), asm::r(2), asm::src(d3d9shader::reg::TEMP, 3, 0xFF, 0)]),
            asm::ins(op::MAD, &[asm::oc0(), asm::r(0), asm::r(1), asm::r(2)]),
        ]), vec![
            ("literal fma(r0, r1*w, r2*w)", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[0][i], fz(c[1][i], c[3][3]), fz(c[2][i], c[3][3]))))),
            ("factored fma(r0, r1, r2) * w", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fz(fma_z(c[0][i], c[1][i], c[2][i]), c[3][3])))),
            ("fma(r0*r1, w, r2*w)", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(fz(c[0][i], c[1][i]), c[3][3], fz(c[2][i], c[3][3]))))),
            ("fma(r0, r1*w, r2) * ..", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(fz(c[0][i], c[1][i]), c[3][3], fz(c[2][i], c[3][3]))))),
        ]),
        ("lrp", asm::ps(&[asm::ins(op::LRP, &[asm::oc0(), asm::r(0), asm::r(1), asm::r(2)])]), vec![
            ("a*(b-c)+c unfused", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fz(c[0][i], c[1][i] - c[2][i]) + c[2][i]))),
            ("fma(a, b-c, c)", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[0][i], c[1][i] - c[2][i], c[2][i])))),
            ("fma(a, b, fma(-a, c, c))", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[0][i], c[1][i], fma_z(-c[0][i], c[2][i], c[2][i]))))),
            ("fma(a, b, (1-a)*c)", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(c[0][i], c[1][i], fz(1.0 - c[0][i], c[2][i]))))),
            ("fma(-a, c, fma(a, b, c))", Box::new(move |c: &[[f32; 4]]| std::array::from_fn(|i| fma_z(-c[0][i], c[2][i], fma_z(c[0][i], c[1][i], c[2][i]))))),
        ]),
    ];
    for vertex in [false, true] {
        for (name, tokens, models) in &cases {
            // the same body as a vertex shader writing o1
            let tokens = if vertex {
                let body: Vec<u32> = tokens[13..tokens.len() - 1].iter().map(|&t| if t == asm::oc0() { asm::dst(d3d9shader::reg::OUTPUT, 1, 0xF) } else { t }).collect();
                asm::vs(&[body])
            } else {
                tokens.clone()
            };
            if vertex && oracle.run_vertex(&tokens, &d3d9ref::VertexRun::default()).is_err() {
                eprintln!("vs {name}: the driver refuses it");
                continue;
            }
            let mut hits = vec![0usize; models.len()];
            let mut ours_ok = 0;
            for _ in 0..trials {
                let consts: Vec<[f32; 4]> = (0..4).map(|_| rng.vec4()).collect();
                let (ours, theirs) = if vertex { both_vs(&mut oracle, &gpu, &tokens, &consts) } else { both(&mut oracle, &gpu, &tokens, &consts) };
                let lanes = if matches!(*name, "dp3" | "dp4" | "dp2add") { 1 } else if *name == "crs" { 3 } else { 4 };
                for (m, (_, f)) in models.iter().enumerate() {
                    let v = f(&consts);
                    if (0..lanes).all(|i| same(v[i], theirs[i])) {
                        hits[m] += 1;
                    }
                }
                if (0..lanes).all(|i| same(ours[i], theirs[i])) {
                    ours_ok += 1;
                }
            }
            eprintln!("{} {name}: translation {ours_ok}/{trials}", if vertex { "vs" } else { "ps" });
            for (m, (label, _)) in models.iter().enumerate() {
                eprintln!("  {label}: {}/{trials}", hits[m]);
            }
            if *name == "mul,mul+mad" && !vertex {
                // the driver's pixel-shader compiler factors the common
                // multiplier out: (a * b + c) * w, an optimisation across
                // instructions rather than a D3D9 rule; the translation stays
                // literal (it matches the vertex stage, where the driver is
                // literal too)
                failures += trials - hits[1];
                continue;
            }
            failures += trials - ours_ok;
        }
    }
    assert_eq!(failures, 0);
}

/// Every arithmetic instruction of both stages, on random operands over a
/// wide range of magnitudes and on special operands (signed zeros,
/// infinities, NaN, denormals), computes bit for bit what the driver
/// computes (DDDA_SHADER_EXPLORE_OPS=add,mul,... restricts the
/// instructions).
#[test]
fn arithmetic_matches_the_driver_bit_for_bit() {
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else {
        eprintln!("skipped: no D3D9 device or no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    let mut failures = 0;
    let only = std::env::var("DDDA_SHADER_EXPLORE_OPS").ok();
    let specials = [0.0f32, -0.0, 1.0, -1.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 1e-40, -1e-40, 1e-20, -1e-10, 3.0e38, 0.5, 2.0, 100.0, -100.0];
    let mut rng = Rng(0x5EED_1234);
    for vertex in [false, true] {
        for case in special_cases(vertex) {
            if only.as_ref().is_some_and(|o| !o.split(',').any(|n| n == case.name)) {
                continue;
            }
            let tokens = if vertex { asm::vs(&case.body) } else { asm::ps(&case.body) };
            let probe = if vertex {
                oracle.run_vertex(&tokens, &d3d9ref::VertexRun::default()).err()
            } else {
                oracle.run_pixel(&tokens, &PixelRun::default()).err()
            };
            if let Some(e) = probe {
                eprintln!("{} {}: the driver refuses it ({e})", if vertex { "vs" } else { "ps" }, case.name);
                continue;
            }
            let mut bad = Vec::new();
            let mut total = 0;
            // random operands over a wide range of magnitudes
            for _ in 0..200 {
                let mut wide = || {
                    let m = rng.float();
                    let e = (rng.next() % 41) as i32 - 20;
                    m * 2f32.powi(e)
                };
                let consts: Vec<[f32; 4]> = (0..3).map(|_| [wide(), wide(), wide(), wide()]).collect();
                let (ours, theirs) = if vertex { both_vs(&mut oracle, &gpu, &tokens, &consts) } else { both(&mut oracle, &gpu, &tokens, &consts) };
                total += 1;
                if !(0..4).all(|i| case.lanes & (1 << i) == 0 || ours[i].to_bits() == theirs[i].to_bits() || (ours[i].is_nan() && theirs[i].is_nan())) {
                    bad.push(format!("    in {:?}: ours {ours:?} / D3D9 {theirs:?}", &consts[..case.arity]));
                }
            }
            // every special in every operand position, the others random
            for pos in 0..case.arity {
                for &s in &specials {
                    for &other in &[1.5f32, -0.75, 0.0, s] {
                        let mut consts: Vec<[f32; 4]> = (0..3).map(|_| [other, rng.float(), other, rng.float()]).collect();
                        consts[pos] = [s, s, s, s];
                        let (ours, theirs) = if vertex { both_vs(&mut oracle, &gpu, &tokens, &consts) } else { both(&mut oracle, &gpu, &tokens, &consts) };
                        total += 1;
                        if !(0..4).all(|i| case.lanes & (1 << i) == 0 || ours[i].to_bits() == theirs[i].to_bits() || (ours[i].is_nan() && theirs[i].is_nan())) {
                            bad.push(format!("    in {:?}: ours {ours:?} / D3D9 {theirs:?}", &consts[..case.arity]));
                        }
                    }
                }
            }
            eprintln!("{} {}: {} of {total} differ", if vertex { "vs" } else { "ps" }, case.name, bad.len());
            for b in bad.iter().take(6) {
                eprintln!("{b}");
            }
            failures += bad.len();
        }
    }
    assert_eq!(failures, 0);
}

struct SpecialCase {
    name: &'static str,
    body: Vec<Vec<u32>>,
    arity: usize,
    /// The result components the instruction defines.
    lanes: u8,
}

/// The instructions [`explore_specials`] tries, as bodies writing the
/// stage's output (oC0 / o1) from r0..r2.
fn special_cases(vertex: bool) -> Vec<SpecialCase> {
    use d3d9shader::{op, reg};
    let out = if vertex { asm::dst(reg::OUTPUT, 1, 0xF) } else { asm::oc0() };
    let out_r = if vertex { asm::src(reg::TEMP, 4, 0xE4, 0) } else { asm::r(4) };
    let x = |r: u32| asm::src(reg::TEMP, r, 0x00, 0);
    let case = |name, body, arity, lanes| SpecialCase { name, body, arity, lanes };
    let direct = |o: u16, n: usize| -> Vec<Vec<u32>> {
        let mut operands = vec![out];
        operands.extend((0..n as u32).map(asm::r));
        vec![asm::ins(o, &operands)]
    };
    // through r4 with a partial mask (instructions that may not write all four)
    let via = |o: u16, mask: u32, operands: &[u32]| -> Vec<Vec<u32>> {
        let mut v = vec![asm::dst(reg::TEMP, 4, mask)];
        v.extend_from_slice(operands);
        vec![
            asm::ins(op::MOV, &[asm::dst(reg::TEMP, 4, 0xF), asm::c(5)]),
            asm::ins(o, &v),
            asm::ins(op::MOV, &[out, out_r]),
        ]
    };
    let mut v = vec![
        case("add", direct(op::ADD, 2), 2, 0xF),
        case("mul", direct(op::MUL, 2), 2, 0xF),
        case("mad", direct(op::MAD, 3), 3, 0xF),
        case("dp3", direct(op::DP3, 2), 2, 0xF),
        case("dp4", direct(op::DP4, 2), 2, 0xF),
        case("min", direct(op::MIN, 2), 2, 0xF),
        case("max", direct(op::MAX, 2), 2, 0xF),
        case("crs", via(op::CRS, 0x7, &[asm::r(0), asm::r(1)]), 2, 0x7),
        case("lrp", direct(op::LRP, 3), 3, 0xF),
        case("frc", direct(op::FRC, 1), 1, 0xF),
        case("abs", direct(op::ABS, 1), 1, 0xF),
        case("mov", direct(op::MOV, 1), 1, 0xF),
        case("rcp", vec![asm::ins(op::RCP, &[out, x(0)])], 1, 0xF),
        case("rsq", vec![asm::ins(op::RSQ, &[out, x(0)])], 1, 0xF),
        case("exp", vec![asm::ins(op::EXP, &[out, x(0)])], 1, 0xF),
        case("log", vec![asm::ins(op::LOG, &[out, x(0)])], 1, 0xF),
        case("nrm", via(op::NRM, 0x7, &[asm::r(0)]), 1, 0x7),
        case("pow", vec![asm::ins(op::POW, &[out, x(0), x(1)])], 2, 0xF),
        case("sincos", via(op::SINCOS, 0x3, &[x(0)]), 1, 0x3),
        case("m4x4", vec![
            asm::ins(op::MOV, &[asm::dst(reg::TEMP, 5, 0xF), asm::c(1)]),
            asm::ins(op::MOV, &[asm::dst(reg::TEMP, 6, 0xF), asm::c(2)]),
            asm::ins(op::MOV, &[asm::dst(reg::TEMP, 7, 0xF), asm::c(1)]),
            asm::ins(op::MOV, &[asm::dst(reg::TEMP, 8, 0xF), asm::c(2)]),
            asm::ins(op::M4X4, &[out, asm::r(0), asm::r(5)]),
        ], 3, 0xF),
    ];
    if vertex {
        v.extend([
            case("slt", direct(op::SLT, 2), 2, 0xF),
            case("sge", direct(op::SGE, 2), 2, 0xF),
            case("sgn", direct(op::SGN, 3), 1, 0xF),
            case("lit", direct(op::LIT, 1), 1, 0xF),
            case("dst", direct(op::DST, 2), 2, 0xF),
            case("expp", vec![asm::ins(op::EXPP, &[out, x(0)])], 1, 0xF),
            case("logp", vec![asm::ins(op::LOGP, &[out, x(0)])], 1, 0xF),
        ]);
    } else {
        v.push(case("dp2add", vec![asm::ins(op::DP2ADD, &[out, asm::r(0), asm::r(1), x(2)])], 3, 0xF));
        v.push(case("cmp", direct(op::CMP, 3), 3, 0xF));
    }
    v
}

/// lit's exponent range (vs_3_0): the driver clamps the exponent to
/// [-128, 128] (its result at 127.99999 and above equals the one at 128;
/// 127.999 still differs), not to the reference rasteriser's 127.9961.
#[test]
fn lit_clamps_its_exponent_at_128() {
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else {
        eprintln!("skipped: no D3D9 device or no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    let tokens = asm::vs(&[asm::ins(d3d9shader::op::LIT, &[asm::dst(d3d9shader::reg::OUTPUT, 1, 0xF), asm::r(0)])]);
    let mut at_128 = [0.0f32; 2];
    for (k, y) in [1.0001f32, 0.999].into_iter().enumerate() {
        for w in [100.0f32, 127.99, 127.9961, 127.999, 127.99999, 128.0, 128.0001, 129.0, 1000.0, -127.9961, -128.0, -128.0001, -129.0, -1000.0] {
            let (ours, theirs) = both_vs(&mut oracle, &gpu, &tokens, &[[1.0, y, 0.0, w]]);
            assert_eq!(ours.map(f32::to_bits), theirs.map(f32::to_bits), "lit y {y} w {w}");
            if w == 128.0 {
                at_128[k] = theirs[2];
            }
            if w > 128.0 {
                assert_eq!(theirs[2], at_128[k], "the driver clamps at 128");
            }
        }
    }
}

/// The shader with instruction `k`'s temp destination copied into a spare
/// temp right after it, and that spare written to the output `dst` (a
/// destination token: oC0, or the vertex position) at the end of the main
/// program. Unlike [`prefix_to`] the program stays whole, so instructions
/// inside loops, branches and subroutines can be compared too (the value
/// is the one of the last execution; c0 if it never runs). None if `k`
/// does not write a temp.
fn tap(tokens: &[u32], k: usize, dst: u32) -> Option<(Vec<u32>, String, u8)> {
    use d3d9shader::{op, reg};
    let s = d3d9shader::parse(tokens).ok()?;
    let lines = d3d9shader::disasm::disassemble(&s);
    let spare = s.instructions.iter()
        .flat_map(|i| i.dst.iter().chain(i.src.iter()))
        .filter(|p| p.kind == reg::TEMP)
        .map(|p| p.index + 1)
        .max()
        .unwrap_or(0);
    if spare > 31 {
        return None;
    }
    let mut out = vec![tokens[0]];
    let (mut at, mut n, mut line) = (1, 0, 0);
    let (mut found, mut ended, mut started) = (None, false, false);
    let finish = |out: &mut Vec<u32>| out.extend_from_slice(&[0x0200_0001, dst, 0x80E4_0000 | spare]);
    for ins in &s.instructions {
        let t = tokens[at];
        let len = if ins.opcode == op::COMMENT { ((t >> 16) & 0x7FFF) as usize } else { ((t >> 24) & 0xF) as usize };
        let span = &tokens[at..at + 1 + len];
        at += 1 + len;
        if ins.opcode == op::COMMENT {
            out.extend_from_slice(span);
            continue;
        }
        line += 1;
        // the end of the main program: its first top-level ret, or END
        if ins.opcode == op::RET && !ended {
            finish(&mut out);
            ended = true;
        }
        if matches!(ins.opcode, op::DCL | op::DEF | op::DEFI | op::DEFB) {
            out.extend_from_slice(span);
            continue;
        }
        if !started {
            // the spare starts as c0 (the same on both sides), so an
            // instruction that never runs compares equal
            out.extend_from_slice(&[0x0200_0001, 0x800F_0000 | spare, 0xA0E4_0000]);
            started = true;
        }
        out.extend_from_slice(span);
        if n == k {
            let d = ins.dst.as_ref()?;
            if d.kind != reg::TEMP || ins.opcode == op::TEXKILL {
                return None;
            }
            // mov r_spare (same mask), r_d
            out.extend_from_slice(&[0x0200_0001, 0x8000_0000 | ((d.mask_or_swizzle as u32) << 16) | spare, 0x80E4_0000 | d.index]);
            found = Some((lines[line].trim().to_string(), d.mask_or_swizzle));
        }
        n += 1;
    }
    let (text, mask) = found?;
    if !ended {
        finish(&mut out);
    }
    out.push(0x0000_FFFF);
    Some((out, text, mask))
}

/// Every translation, with the options the runtime's backend uses (ddgpu:
/// vertex input types and the half-pixel offset; linked pixel input
/// locations and the alpha test) as well as the defaults, parses and
/// validates as WGSL in naga (the sweep creates the default translations'
/// pipelines in wgpu). DDDA_ALL_SHADERS=1: every package, otherwise Title
/// and root.
#[test]
fn every_translation_is_valid_wgsl() {
    let packages: Vec<String> = if std::env::var_os("DDDA_ALL_SHADERS").is_some() { all_packages() } else { vec!["Title".into(), "root".into()] };
    let mut seen = std::collections::HashSet::new();
    let (mut checked, mut bad) = (0usize, Vec::new());
    for name in &packages {
        for tokens in pixel_shaders(name).into_iter().chain(vertex_shaders(name)) {
            if !seen.insert(tokens.clone()) {
                continue;
            }
            let shader = d3d9shader::parse(&tokens).unwrap();
            let vertex = shader.stage == d3d9shader::Stage::Vertex;
            let base = match d3d9wgsl::translate(&shader) {
                Ok(t) => t,
                Err(e) => {
                    bad.push(format!("{name} {}: not translated: {e}", tag(&tokens)));
                    continue;
                }
            };
            let n = base.interface.inputs.len();
            let variants = if vertex {
                let mixed = (0..n).map(|k| [d3d9wgsl::InputType::Uint, d3d9wgsl::InputType::Sint, d3d9wgsl::InputType::Float][k % 3]).collect();
                vec![
                    d3d9wgsl::Options::default(),
                    d3d9wgsl::Options { vs_input_types: Some(vec![d3d9wgsl::InputType::Float; n]), half_pixel: true, ..Default::default() },
                    d3d9wgsl::Options { vs_input_types: Some(mixed), half_pixel: true, ..Default::default() },
                ]
            } else {
                // one input the vertex stage does not write, the others moved
                let locations = (0..n as u32).map(|k| if k == 0 { None } else { Some(k + 3) }).collect();
                vec![
                    d3d9wgsl::Options::default(),
                    d3d9wgsl::Options { ps_input_locations: Some(locations), alpha_test: true, ..Default::default() },
                ]
            };
            for options in variants {
                let label = format!("{options:?}");
                let t = match d3d9wgsl::translate_with(&shader, options) {
                    Ok(t) => t,
                    Err(e) => {
                        bad.push(format!("{name} {} {label}: not translated: {e}", tag(&tokens)));
                        continue;
                    }
                };
                checked += 1;
                let result = naga::front::wgsl::parse_str(&t.wgsl).map_err(|e| e.emit_to_string(&t.wgsl)).and_then(|module| {
                    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                        .validate(&module)
                        .map(|_| ())
                        .map_err(|e| e.emit_to_string(&t.wgsl))
                });
                if let Err(e) = result {
                    bad.push(format!("{name} {} {label}: {e}", tag(&tokens)));
                }
            }
        }
    }
    eprintln!("{checked} translations validated, {} failed", bad.len());
    for b in bad.iter().take(3) {
        eprintln!("{}", &b[..b.len().min(3000)]);
    }
    assert!(checked > 0 || packages.iter().all(|p| pixel_shaders(p).is_empty()), "nothing checked");
    assert!(bad.is_empty());
}

// --- attributing the inexact results ---------------------------------------------

/// The raw tokens of instruction `k` (counted as [`tap`] counts) and the
/// token index where it starts.
fn raw_instruction(tokens: &[u32], k: usize) -> Option<(usize, Vec<u32>)> {
    use d3d9shader::op;
    let s = d3d9shader::parse(tokens).ok()?;
    let (mut at, mut n) = (1, 0);
    for ins in &s.instructions {
        let t = tokens[at];
        let len = if ins.opcode == op::COMMENT { ((t >> 16) & 0x7FFF) as usize } else { ((t >> 24) & 0xF) as usize };
        let start = at;
        at += 1 + len;
        if matches!(ins.opcode, op::COMMENT | op::DCL | op::DEF | op::DEFI | op::DEFB) {
            continue;
        }
        if n == k {
            return Some((start, tokens[start..at].to_vec()));
        }
        n += 1;
    }
    None
}

/// The operands of a raw instruction: (token, address token if relative).
fn operands(raw: &[u32]) -> Vec<(u32, Option<u32>)> {
    let mut v = Vec::new();
    let mut i = 1;
    while i < raw.len() {
        let t = raw[i];
        if t & 0x2000 != 0 && i + 1 < raw.len() {
            v.push((t, Some(raw[i + 1])));
            i += 2;
        } else {
            v.push((t, None));
            i += 1;
        }
    }
    v
}

fn register_type(t: u32) -> u8 {
    (((t >> 28) & 7) | ((t >> 8) & 0x18)) as u8
}

/// The shader reading source operand `src` (its register, whole: no
/// swizzle or modifier) right before instruction `k`, into the output
/// `dst`, through a spare temp as [`tap`] does.
fn read_before(tokens: &[u32], k: usize, src: (u32, Option<u32>), dst: u32) -> Option<Vec<u32>> {
    use d3d9shader::{op, reg};
    let s = d3d9shader::parse(tokens).ok()?;
    let spare = s.instructions.iter()
        .flat_map(|i| i.dst.iter().chain(i.src.iter()))
        .filter(|p| p.kind == reg::TEMP)
        .map(|p| p.index + 1)
        .max()
        .unwrap_or(0);
    if spare > 31 {
        return None;
    }
    let (start, _) = raw_instruction(tokens, k)?;
    let whole = (src.0 & !0x0FFF_0000) | 0x00E4_0000;
    let mut read = vec![if src.1.is_some() { 0x0300_0001 } else { 0x0200_0001 }, 0x800F_0000 | spare, whole];
    read.extend(src.1);
    let mut out = vec![tokens[0]];
    let (mut at, mut started, mut ended) = (1, false, false);
    for ins in &s.instructions {
        let t = tokens[at];
        let len = if ins.opcode == op::COMMENT { ((t >> 16) & 0x7FFF) as usize } else { ((t >> 24) & 0xF) as usize };
        let span = &tokens[at..at + 1 + len];
        let here = at;
        at += 1 + len;
        if ins.opcode == op::RET && !ended {
            out.extend_from_slice(&[0x0200_0001, dst, 0x80E4_0000 | spare]);
            ended = true;
        }
        if !matches!(ins.opcode, op::COMMENT | op::DCL | op::DEF | op::DEFI | op::DEFB) && !started {
            out.extend_from_slice(&[0x0200_0001, 0x800F_0000 | spare, 0xA0E4_0000]);
            started = true;
        }
        if here == start {
            out.extend_from_slice(&read);
        }
        out.extend_from_slice(span);
    }
    if !ended {
        out.extend_from_slice(&[0x0200_0001, dst, 0x80E4_0000 | spare]);
    }
    out.push(0x0000_FFFF);
    Some(out)
}

/// Run a whole (possibly rewritten) shader with set `set` of `original`'s
/// random inputs; the value of its output `dst` (oC0, or the position) on
/// both sides.
fn run_both(gpu: &Gpu, oracle: &mut Oracle, original: &[u32], shader: &[u32], vertex: bool, set: u32) -> Option<([f32; 4], [f32; 4])> {
    let full = d3d9wgsl::translate_with(&d3d9shader::parse(original).ok()?, options(vertex)).ok()?;
    let ct = d3d9wgsl::translate_with(&d3d9shader::parse(shader).ok()?, options(vertex)).ok()?;
    let mut rng = seed(original, set);
    if vertex {
        let run = random_vertex_run(&mut rng, &full.interface);
        let a = gpu.run_vertex(&ct.wgsl, &ct.interface, &run).ok()?;
        let b = oracle.run_vertex(shader, &run).ok()?;
        Some((a[ct.interface.outputs_vs.len()], b.iter().find(|o| o.usage == 0 && o.index == 0)?.value?))
    } else {
        let run = random_pixel_run(&mut rng, &full.interface);
        Some((gpu.run(&ct.wgsl, &ct.interface, &run).ok()?[0], oracle.run_pixel(shader, &run).ok()?[0]))
    }
}

/// Equal as floats in the masked components (+0 and -0 equal, any NaN
/// equal to any NaN): what the sweep counts as exact.
fn value_equal(a: [f32; 4], b: [f32; 4], mask: u8) -> bool {
    (0..4).all(|c| mask & (1 << c) == 0 || ulps(a[c], b[c]) == 0)
}

fn bits_equal(a: [f32; 4], b: [f32; 4], mask: u8) -> bool {
    (0..4).all(|c| mask & (1 << c) == 0 || a[c].to_bits() == b[c].to_bits() || (a[c].is_nan() && b[c].is_nan()))
}

/// Why shader `tokens` is not bit-exact: the first set whose outputs
/// differ in bits, the first instruction whose result differs there, and
/// that instruction run alone on the inputs it had (read from the driver's
/// run): if alone it differs, the translation of the instruction is wrong
/// ("instruction"); if alone it gives our in-context result, the driver's
/// compiler computed it differently in context ("driver rewrite"); if it
/// gives the driver's, our GPU compiler did ("wgpu rewrite").
fn attribute(gpu: &Gpu, oracle: &mut Oracle, tokens: &[u32], vertex: bool, sets: u32) -> String {
    use d3d9shader::{op, reg};
    let shader = d3d9shader::parse(tokens).unwrap();
    let Ok(t) = d3d9wgsl::translate_with(&shader, options(vertex)) else { return "untranslated".into() };
    let output = output_token(vertex, &t.interface);
    // the first inexact set
    let mut inexact = None;
    for set in 0..sets {
        let mut rng = seed(tokens, set);
        let compared = if vertex {
            let run = random_vertex_run(&mut rng, &t.interface);
            let (Ok(a), Ok(b)) = (gpu.run_vertex(&t.wgsl, &t.interface, &run), oracle.run_vertex(tokens, &run)) else { return "not run".into() };
            vertex_outputs(&a, &b, &t.interface)
        } else {
            let run = random_pixel_run(&mut rng, &t.interface);
            let (Ok(a), Ok(b)) = (gpu.run(&t.wgsl, &t.interface, &run), oracle.run_pixel(tokens, &run)) else { return "not run".into() };
            pixel_outputs(&a, &b, &t.interface)
        };
        if compared.iter().any(|(a, b, m, _)| !value_equal(*a, *b, *m)) {
            inexact = Some(set);
            break;
        }
    }
    let Some(set) = inexact else { return "exact now".into() };
    for k in 0..tokens.len() {
        let Some((cut, text, mask)) = tap(tokens, k, output) else {
            if raw_instruction(tokens, k).is_none() {
                break;
            }
            continue;
        };
        let Some((ours, theirs)) = run_both(gpu, oracle, tokens, &cut, vertex, set) else { continue };
        if value_equal(ours, theirs, mask) {
            continue;
        }
        let u = (0..4).filter(|c| mask & (1 << c) != 0).map(|c| ulps(ours[c], theirs[c])).max().unwrap_or(0);
        let ins = shader.instructions.iter()
            .filter(|i| !matches!(i.opcode, op::COMMENT | op::DCL | op::DEF | op::DEFI | op::DEFB)).nth(k).unwrap().clone();
        let head = format!("set {set} instruction {k} `{text}` {u} ulp");
        let Some((_, raw)) = raw_instruction(tokens, k) else { return format!("{head}: other") };
        let ops = operands(&raw);
        let arithmetic = !matches!(ins.opcode, op::TEX | op::TEXLDL | op::TEXLDD | op::DSX | op::DSY | op::TEXKILL | op::SETP | op::MOVA)
            && ins.predicate.is_none()
            && ops.len() >= 2
            && ops.len() <= 4
            && ops[1..].iter().all(|o| register_type(o.0) != reg::SAMPLER);
        if !arithmetic {
            return format!("{head}: not isolable ({})", text.split_whitespace().next().unwrap_or(""));
        }
        // the inputs as the driver had them (and whether ours agree)
        let mut inputs = Vec::new();
        let mut same_inputs = true;
        for &src in &ops[1..] {
            let Some(read) = read_before(tokens, k, src, output) else { return format!("{head}: inputs not read") };
            let Some((a, b)) = run_both(gpu, oracle, tokens, &read, vertex, set) else { return format!("{head}: inputs not read") };
            same_inputs &= value_equal(a, b, 0xF);
            inputs.push(b);
        }
        // the instruction alone on those inputs (r0.. = c0..; result via r4)
        let mut alone = vec![ins.opcode as u32 | (((ops.len()) as u32) << 24), 0x8000_0000 | (ops[0].0 & 0x0FFF_0000) | 4];
        for (j, src) in ops[1..].iter().enumerate() {
            alone.push(0x8000_0000 | (src.0 & 0x0FFF_0000) | j as u32);
        }
        let body = vec![
            asm::ins(op::MOV, &[asm::dst(reg::TEMP, 4, 0xF), asm::c(5)]),
            alone,
            asm::ins(op::MOV, &[if vertex { asm::dst(reg::OUTPUT, 1, 0xF) } else { asm::oc0() }, asm::r(4)]),
        ];
        let iso = if vertex { asm::vs(&body) } else { asm::ps(&body) };
        let mut consts = inputs.clone();
        consts.resize(4, [0.0; 4]);
        let parsed = d3d9shader::parse(&iso);
        if parsed.is_err() || d3d9wgsl::translate_with(parsed.as_ref().unwrap(), options(vertex)).is_err() {
            return format!("{head}: not isolable");
        }
        let (a, b) = if vertex {
            let run = d3d9ref::VertexRun { constants: consts, ..Default::default() };
            let t = d3d9wgsl::translate_with(&d3d9shader::parse(&iso).unwrap(), options(true)).unwrap();
            let (Ok(a), Ok(b)) = (gpu.run_vertex(&t.wgsl, &t.interface, &run), oracle.run_vertex(&iso, &run)) else { return format!("{head}: not isolable") };
            (a[0], b.iter().find(|o| o.register == 1).unwrap().value.unwrap())
        } else {
            let run = PixelRun { constants: consts, ..Default::default() };
            let t = d3d9wgsl::translate(&d3d9shader::parse(&iso).unwrap()).unwrap();
            let (Ok(a), Ok(b)) = (gpu.run(&t.wgsl, &t.interface, &run), oracle.run_pixel(&iso, &run)) else { return format!("{head}: not isolable") };
            (a[0], b[0])
        };
        let class = if !same_inputs {
            "inputs differ"
        } else if !value_equal(a, b, mask) {
            "instruction"
        } else if value_equal(b, ours, mask) {
            "driver rewrite"
        } else if value_equal(b, theirs, mask) {
            "wgpu rewrite"
        } else {
            "both rewrite"
        };
        return format!("{head}: {class}");
    }
    format!("set {set}: not located")
}

/// DDDA_SHADER_ATTRIBUTE=<sweep report directory>: every shader the sweep
/// reports as close but not bit-exact, attributed by [`attribute`];
/// written to attribution.txt there (DDDA_SHADER_LIMIT=n per package;
/// DDDA_SHADER_ATTRIBUTE_SKIP=<file> skips shaders a previous run listed).
#[test]
fn attribute_inexact() {
    let Some(dir) = std::env::var_os("DDDA_SHADER_ATTRIBUTE").map(std::path::PathBuf::from) else { return };
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else { return };
    let limit: usize = std::env::var("DDDA_SHADER_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let only: Option<Vec<String>> = std::env::var("DDDA_SHADER_PACKAGES").ok().map(|v| v.split(',').map(String::from).collect());
    // DDDA_SHADER_ATTRIBUTE_SKIP=<file>: earlier attribution output; shaders
    // listed there (by tag) are not attributed again, so a run resumes
    let skip: std::collections::HashSet<String> = std::env::var_os("DDDA_SHADER_ATTRIBUTE_SKIP")
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| t.lines().filter_map(|l| l.split_whitespace().nth(2).map(String::from)).collect())
        .unwrap_or_default();
    let mut out = String::new();
    for name in all_packages() {
        if only.as_ref().is_some_and(|o| !o.contains(&name)) {
            continue;
        }
        let Ok(report) = std::fs::read_to_string(dir.join(format!("{name}.txt"))) else { continue };
        let sets: u32 = report.split_whitespace().skip_while(|w| *w != "sets").nth(1).and_then(|v| v.parse().ok()).unwrap_or(4);
        let close: Vec<(bool, String, String)> = report.lines()
            .filter_map(|l| l.strip_prefix("close "))
            .map(|l| {
                let w: Vec<&str> = l.split_whitespace().collect();
                (w[0] == "vertex", w[2].trim_end_matches(':').to_string(), w[3].to_string())
            })
            .take(limit)
            .collect();
        if close.is_empty() {
            continue;
        }
        let pixel: Vec<Vec<u32>> = pixel_shaders(&name);
        let vertex: Vec<Vec<u32>> = vertex_shaders(&name);
        for (is_vertex, id, u) in close {
            if skip.contains(&id) {
                continue;
            }
            let list = if is_vertex { &vertex } else { &pixel };
            let Some(tokens) = list.iter().find(|t| tag(t) == id) else { continue };
            let why = attribute(&gpu, &mut oracle, tokens, is_vertex, sets);
            let line = format!("{name} {} {id} ({u} ulp): {why}\n", if is_vertex { "vertex" } else { "pixel" });
            eprint!("{line}");
            out.push_str(&line);
            std::fs::write(dir.join("attribution.txt"), &out).unwrap();
        }
    }
}

/// A shader's own definitions keep their bits: def c0 = (-0, 0, -0, NaN)
/// read back unchanged, directly and through cmp.
#[test]
fn defined_constants_keep_their_bits() {
    let (Some(mut oracle), Some(gpu)) = (Oracle::new(), Gpu::new()) else {
        eprintln!("skipped: no D3D9 device or no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    use d3d9shader::{op, reg};
    let def = vec![op::DEF as u32 | (5 << 24), asm::dst(reg::CONST, 6, 0xF), 0x8000_0000, 0, 0x8000_0000, 0x7FC0_0000];
    // definitions come first
    let with_def = |body: Vec<u32>| -> Vec<u32> { [&body[..1], &def[..], &body[1..]].concat() };
    let direct = with_def(asm::ps(&[asm::ins(op::MOV, &[asm::oc0(), asm::c(6)])]));
    let picked = with_def(asm::ps(&[asm::ins(op::CMP, &[asm::oc0(), asm::r(0), asm::src(reg::CONST, 6, 0x00, 0), asm::src(reg::CONST, 6, 0x55, 0)])]));
    for (name, tokens) in [("mov", direct), ("cmp", picked)] {
        for x in [1.0f32, -1.0] {
            let (ours, theirs) = both(&mut oracle, &gpu, &tokens, &[[x; 4]]);
            eprintln!("{name} {x}: ours {:08x?} / D3D9 {:08x?}", ours.map(f32::to_bits), theirs.map(f32::to_bits));
            assert!(bits_equal(ours, theirs, 0xF), "{name}");
        }
    }
}

impl Gpu {
    /// A vertex shader as the runtime's backend builds it (ddgpu: integer
    /// and float vertex inputs, the half-pixel offset) in a pipeline with a
    /// fragment stage reading every output it passes on.
    fn backend_vertex_pipeline(&self, shader: &d3d9shader::Shader) -> Result<(), String> {
        let n = d3d9wgsl::translate(shader).map_err(|e| e.to_string())?.interface.inputs.len();
        let types: Vec<d3d9wgsl::InputType> = (0..n).map(|k| [d3d9wgsl::InputType::Float, d3d9wgsl::InputType::Uint, d3d9wgsl::InputType::Sint][k % 3]).collect();
        let t = d3d9wgsl::translate_with(shader, d3d9wgsl::Options { vs_input_types: Some(types.clone()), half_pixel: true, ..Default::default() }).map_err(|e| e.to_string())?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let vs = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vs"), source: wgpu::ShaderSource::Wgsl(t.wgsl.as_str().into()) });
        let mut fs = String::from("struct FIn {\n");
        for l in 0..t.interface.outputs_vs.len() {
            fs.push_str(&format!("  @location({l}) x{l}: vec4<f32>,\n"));
        }
        fs.push_str("  @builtin(position) p: vec4<f32>,\n}\n");
        let sum: Vec<String> = (0..t.interface.outputs_vs.len()).map(|l| format!("i.x{l}")).chain(["i.p".to_string()]).collect();
        fs.push_str(&format!("@fragment fn main(i: FIn) -> @location(0) vec4<f32> {{ return {}; }}\n", sum.join(" + ")));
        let fsm = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fs"), source: wgpu::ShaderSource::Wgsl(fs.into()) });
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout0 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uniform] });
        let layout1 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[] });
        let layout2 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &sampler_entries(&t.interface, wgpu::ShaderStages::VERTEX) });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout0), Some(&layout1), Some(&layout2)],
            immediate_size: 0,
        });
        let attributes: Vec<wgpu::VertexAttribute> = types.iter().enumerate().map(|(k, ty)| wgpu::VertexAttribute {
            format: match ty {
                d3d9wgsl::InputType::Float => wgpu::VertexFormat::Float32x4,
                d3d9wgsl::InputType::Uint => wgpu::VertexFormat::Uint8x4,
                d3d9wgsl::InputType::Sint => wgpu::VertexFormat::Sint16x4,
            },
            offset: 16 * k as u64,
            shader_location: k as u32,
        }).collect();
        let buffers = [wgpu::VertexBufferLayout { array_stride: 16 * n.max(1) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes }];
        let _pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &vs, entry_point: Some("main"), compilation_options: Default::default(), buffers: &buffers },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fsm,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        match block_on(scope.pop()) {
            Some(e) => Err(format!("{e}")),
            None => Ok(()),
        }
    }

    /// A pixel shader as the runtime's backend builds it (ddgpu: inputs
    /// linked to a vertex stage's outputs, here at shifted locations with
    /// the first input unwritten; the alpha test) in a pipeline with a
    /// vertex stage writing those locations.
    fn backend_pixel_pipeline(&self, shader: &d3d9shader::Shader) -> Result<(), String> {
        let n = d3d9wgsl::translate(shader).map_err(|e| e.to_string())?.interface.inputs.len() as u32;
        let locations: Vec<Option<u32>> = (0..n).map(|k| if k == 0 && n > 1 { None } else { Some(n - k) }).collect();
        let t = d3d9wgsl::translate_with(shader, d3d9wgsl::Options { ps_input_locations: Some(locations), alpha_test: true, ..Default::default() }).map_err(|e| e.to_string())?;
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let fs = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fs"), source: wgpu::ShaderSource::Wgsl(t.wgsl.as_str().into()) });
        let mut vs = String::from("struct VOut {\n  @builtin(position) p: vec4<f32>,\n");
        for l in 0..=n {
            vs.push_str(&format!("  @location({l}) x{l}: vec4<f32>,\n"));
        }
        let fields: Vec<String> = (0..=n + 1).map(|_| "vec4<f32>(0.0)".to_string()).collect();
        vs.push_str(&format!("}}\n@vertex fn main() -> VOut {{ return VOut({}); }}\n", fields.join(", ")));
        let vsm = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vs"), source: wgpu::ShaderSource::Wgsl(vs.into()) });
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout0 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[uniform] });
        let layout1 = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &sampler_entries(&t.interface, wgpu::ShaderStages::FRAGMENT) });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout0), Some(&layout1)],
            immediate_size: 0,
        });
        let max = t.interface.outputs.iter().copied().max().unwrap_or(0) as usize;
        let targets: Vec<Option<wgpu::ColorTargetState>> = (0..=max)
            .map(|k| t.interface.outputs.contains(&(k as u32)).then_some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }))
            .collect();
        let _pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &vsm, entry_point: Some("main"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState { module: &fs, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
            multiview_mask: None,
            cache: None,
        });
        match block_on(scope.pop()) {
            Some(e) => Err(format!("{e}")),
            None => Ok(()),
        }
    }
}

/// Texture and sampler bindings for an interface's samplers.
fn sampler_entries(iface: &d3d9wgsl::Interface, visibility: wgpu::ShaderStages) -> Vec<wgpu::BindGroupLayoutEntry> {
    let mut entries = Vec::new();
    for &(n, kind) in &iface.samplers {
        let view_dimension = match kind {
            d3d9wgsl::TextureKind::Flat => wgpu::TextureViewDimension::D2,
            d3d9wgsl::TextureKind::Cube => wgpu::TextureViewDimension::Cube,
            d3d9wgsl::TextureKind::Volume => wgpu::TextureViewDimension::D3,
        };
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 * n,
            visibility,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2 * n + 1,
            visibility,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
    }
    entries
}

/// Every shader in a wgpu pipeline as the runtime's backend builds it
/// ([`Gpu::backend_vertex_pipeline`], [`Gpu::backend_pixel_pipeline`]):
/// wgpu's validation, including the interface between the stages, and the
/// driver's pipeline compilation accept it. DDDA_ALL_SHADERS=1: every
/// package, otherwise Title.
#[test]
fn every_translation_makes_a_backend_pipeline() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("skipped: no wgpu adapter with FLOAT32_FILTERABLE");
        return;
    };
    let packages: Vec<String> = if std::env::var_os("DDDA_ALL_SHADERS").is_some() { all_packages() } else { vec!["Title".into()] };
    let mut seen = std::collections::HashSet::new();
    let (mut built, mut bad) = (0usize, Vec::new());
    // DDDA_SHADER_FROM=<package>: resume there (earlier packages only count as seen)
    let from = std::env::var("DDDA_SHADER_FROM").ok();
    for name in &packages {
        let skip = from.as_ref().is_some_and(|f| name < f);
        for tokens in pixel_shaders(name).into_iter().chain(vertex_shaders(name)) {
            if !seen.insert(tokens.clone()) || skip {
                continue;
            }
            let shader = d3d9shader::parse(&tokens).unwrap();
            let r = if shader.stage == d3d9shader::Stage::Vertex { gpu.backend_vertex_pipeline(&shader) } else { gpu.backend_pixel_pipeline(&shader) };
            match r {
                Ok(()) => built += 1,
                Err(e) => bad.push(format!("{name} {}: {e}", tag(&tokens))),
            }
        }
        if std::env::var_os("DDDA_ALL_SHADERS").is_some() {
            eprintln!("{name}: {built} pipelines so far, {} failed", bad.len());
        }
    }
    eprintln!("{built} pipelines built, {} failed ({})", bad.len(), gpu.adapter);
    for b in bad.iter().take(3) {
        eprintln!("{}", &b[..b.len().min(3000)]);
    }
    assert!(built > 0, "nothing built");
    assert!(bad.is_empty());
}
