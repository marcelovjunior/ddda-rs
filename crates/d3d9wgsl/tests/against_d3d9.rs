//! The game's pixel shaders, translated to WGSL and run on wgpu, against
//! the same shaders run by the host's D3D9 driver (crates/d3d9ref), with
//! the same random constants, inputs and textures. Skips without the game,
//! D3D9 or a wgpu adapter.
//!
//! DDDA_SHADER_PACKAGES=Title,root,... chooses the packages (default
//! Title); DDDA_SHADER_LIMIT=n stops after n shaders.
use d3d9ref::{Oracle, PixelRun, Texture, TextureKind};
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

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    vs: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        if !adapter.features().contains(wgpu::Features::FLOAT32_FILTERABLE) {
            return None;
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::FLOAT32_FILTERABLE,
            ..Default::default()
        })).ok()?;
        let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vs"), source: wgpu::ShaderSource::Wgsl(VS.into()) });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        Some(Gpu { device, queue, vs, sampler })
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
            layout: &layout0,
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
            self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout1, entries: &entries })
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
            pass.set_pipeline(&pipeline);
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
            layout: &layout0,
            entries: &[wgpu::BindGroupEntry { binding: 2, resource: cbuf.as_entire_binding() }],
        });
        let group1 = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout1, entries: &[] });
        let views: Vec<(u32, wgpu::TextureView)> = iface.samplers.iter()
            .map(|&(n, _)| (n, self.texture(run.textures[n as usize].as_ref().expect("texture for every sampler"))))
            .collect();
        let mut e2 = Vec::new();
        for (n, v) in &views {
            e2.push(wgpu::BindGroupEntry { binding: 2 * n, resource: wgpu::BindingResource::TextureView(v) });
            e2.push(wgpu::BindGroupEntry { binding: 2 * n + 1, resource: wgpu::BindingResource::Sampler(&self.sampler) });
        }
        let group2 = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout2, entries: &e2 });
        let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 };
        let mut values = Vec::new();
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
                pass.set_pipeline(&pipeline);
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
        if let Some(e) = block_on(scope.pop()) {
            return Err(format!("{e}"));
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
            let t = match d3d9wgsl::translate_with(&shader, d3d9wgsl::Options { capture_position: Some([0.0, 0.0, 0.0, 1.0]) }) {
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
                        let Ok(ct) = d3d9wgsl::translate_with(&cs, d3d9wgsl::Options { capture_position: Some([0.0, 0.0, 0.0, 1.0]) }) else { continue };
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
