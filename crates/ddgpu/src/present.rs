//! Showing presented back buffers without reading them back (brief L25):
//! [`Blitter`] draws a back buffer into any colour target on the same
//! device, typically a window's surface texture.
//!
//! - The image keeps its aspect ratio, centred, with black bars.
//! - At the back buffer's own size every pixel is copied exactly (texel
//!   loads, no filtering). Scaled, it is filtered bilinearly. A window
//!   that differs from the game's back buffer is the host's choice, not
//!   something D3D9 defines.
//! - A gamma ramp maps each 8-bit channel through its table (the entry's
//!   high byte), exactly like the backend's read-back path; scaled images
//!   map each texel before filtering.
//! - The bytes are shown as they are: draw into a non-sRGB view
//!   ([`Blitter::view_format`]), as D3D9 shows the back buffer.
use crate::Presented;
use d3d9cmd::GammaRamp;
use std::collections::HashMap;

const SHADER: &str = "
struct Params { dst: vec4<f32>, src: vec2<f32>, gamma: u32, exact: u32 }
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var lut: texture_2d<f32>;
@group(0) @binding(2) var<uniform> p: Params;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
  let c = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  return vec4<f32>(c * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
}
fn texel(x: i32, y: i32) -> vec3<f32> {
  let s = vec2<i32>(p.src);
  let c = textureLoad(frame, clamp(vec2<i32>(x, y), vec2<i32>(0), s - 1), 0).rgb;
  if (p.gamma == 0u) { return c; }
  let k = vec3<i32>(round(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0));
  return vec3<f32>(textureLoad(lut, vec2<i32>(k.r, 0), 0).r,
                   textureLoad(lut, vec2<i32>(k.g, 0), 0).g,
                   textureLoad(lut, vec2<i32>(k.b, 0), 0).b);
}
@fragment fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
  let q = (pos.xy - p.dst.xy) / p.dst.zw;
  if (any(q < vec2<f32>(0.0)) || any(q >= vec2<f32>(1.0))) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
  let uv = q * p.src;
  if (p.exact == 1u) {
    let i = vec2<i32>(floor(uv));
    return vec4<f32>(texel(i.x, i.y), 1.0);
  }
  let t = uv - 0.5;
  let i = vec2<i32>(floor(t));
  let f = t - floor(t);
  let top = mix(texel(i.x, i.y), texel(i.x + 1, i.y), f.x);
  let bottom = mix(texel(i.x, i.y + 1), texel(i.x + 1, i.y + 1), f.x);
  return vec4<f32>(mix(top, bottom, f.y), 1.0);
}
";

pub struct Blitter {
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    params: wgpu::Buffer,
    lut: wgpu::Texture,
    /// the ramp the lookup texture holds
    lut_ramp: Option<Box<GammaRamp>>,
}

/// Where a `sw` x `sh` image goes in a `tw` x `th` target, aspect kept:
/// (x, y, width, height) in target pixels, whole pixels.
pub fn fit(sw: u32, sh: u32, tw: u32, th: u32) -> [u32; 4] {
    let (sw, sh) = (sw.max(1) as u64, sh.max(1) as u64);
    let (tw64, th64) = (tw as u64, th as u64);
    // the largest size with the source's aspect that fits
    let (w, h) = if tw64 * sh <= th64 * sw { (tw64, (tw64 * sh / sw).max(1)) } else { ((th64 * sw / sh).max(1), th64) };
    [((tw64 - w) / 2) as u32, ((th64 - h) / 2) as u32, w as u32, h as u32]
}

impl Blitter {
    pub fn new(device: &wgpu::Device) -> Blitter {
        let module = crate::pipeline_log::timed("module", "object-miss", || device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("present"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) }));
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present"),
            entries: &[
                texture(0),
                texture(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("present"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let params = device.create_buffer(&wgpu::BufferDescriptor { label: Some("present"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gamma ramp"),
            size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Blitter { module, layout, pipeline_layout, pipelines: HashMap::new(), params, lut, lut_ramp: None }
    }

    /// The format to draw into for a surface or texture of `format`: the
    /// same without sRGB encoding.
    pub fn view_format(format: wgpu::TextureFormat) -> wgpu::TextureFormat {
        format.remove_srgb_suffix()
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        self.pipelines.entry(format).or_insert_with(|| crate::pipeline_log::timed("pipeline", "object-miss", || device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState { module: &self.module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        }))).clone()
    }

    /// Draw the presented back buffer into `target` (a view of `format`,
    /// `width` x `height` pixels), covering all of it, and submit.
    pub fn draw(&mut self, frame: &Presented, target: &wgpu::TextureView, format: wgpu::TextureFormat, width: u32, height: u32) {
        let (device, queue) = (frame.device, frame.queue);
        if let Some(r) = frame.gamma {
            if self.lut_ramp.as_deref() != Some(r) {
                let texels: Vec<u8> = (0..256).flat_map(|k| [(r[0][k] >> 8) as u8, (r[1][k] >> 8) as u8, (r[2][k] >> 8) as u8, 255]).collect();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &self.lut, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    &texels,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(1) },
                    wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
                );
                self.lut_ramp = Some(Box::new(*r));
            }
        }
        let dst = fit(frame.width, frame.height, width, height);
        let exact = (dst[2], dst[3]) == (frame.width, frame.height);
        let mut params = Vec::with_capacity(32);
        for f in [dst[0] as f32, dst[1] as f32, dst[2] as f32, dst[3] as f32, frame.width as f32, frame.height as f32] {
            params.extend_from_slice(&f.to_le_bytes());
        }
        params.extend_from_slice(&(frame.gamma.is_some() as u32).to_le_bytes());
        params.extend_from_slice(&(exact as u32).to_le_bytes());
        queue.write_buffer(&self.params, 0, &params);
        let pipeline = self.pipeline(device, format);
        let source = frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(frame.format.remove_srgb_suffix()), ..Default::default() });
        let lut = self.lut.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut) },
                wgpu::BindGroupEntry { binding: 2, resource: self.params.as_entire_binding() },
            ],
        });
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        crate::gpu_busy::submitted(queue, true);
    }
}
