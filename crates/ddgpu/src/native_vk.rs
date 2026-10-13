//! Raw encoding of the same validated native recorder state. All cached
//! identities belong to this recorder/device, never reusable guest addresses.
use super::*;
use crate::{cache::PipelineKey, vk};
use std::sync::Arc;

pub(super) struct Raw {
    context: vk::Context,
    pipelines: FxMap<Pipeline, Option<Arc<vk::Pipeline>>>,
    bindings: FxMap<(Pipeline, [TextureSet; 2]), Arc<vk::Bindings>>,
    pub pass: Option<vk::Pass>,
    pub chunks: Vec<vk::Chunk>,
}

impl Raw {
    pub fn new(device: &Device, buffer: &wgpu::Buffer) -> Result<Self, String> {
        Ok(Self {
            context: vk::Context::new(&device.gpu, buffer)?,
            pipelines: Default::default(),
            bindings: Default::default(),
            pass: None,
            chunks: vec![],
        })
    }

    fn view(&mut self, device: &Device, handle: View) -> Option<Arc<vk::View>> {
        let v = &device.views[handle.index as usize];
        if !self.context.views.contains_key(&v.gpu) {
            let mut usage = wgpu::TextureUses::empty();
            let allowed = v.gpu.texture().usage();
            if allowed.contains(wgpu::TextureUsages::TEXTURE_BINDING) {
                usage |= wgpu::TextureUses::RESOURCE;
            }
            if allowed.contains(wgpu::TextureUsages::RENDER_ATTACHMENT) {
                usage |= if v.format.is_depth_stencil_format() {
                    wgpu::TextureUses::DEPTH_STENCIL_WRITE
                } else {
                    wgpu::TextureUses::COLOR_TARGET
                };
            }
            self.context.view(&v.gpu, &v.desc, usage).ok()?;
        }
        self.context.views.get(&v.gpu).cloned()
    }

    fn pipeline(&mut self, device: &Device, handle: Pipeline) -> Result<Arc<vk::Pipeline>, String> {
        if let Some(template) = device.raw_pipelines.lock().unwrap().get(&handle) {
            return Ok(self.context.instantiate(template));
        }
        let p = &device.pipelines[handle.index as usize];
        let desc = p.desc;
        let vs = &device.modules[&(desc.vertex_shader, desc.input, None, 0, false)];
        let ps = &device.modules[&(
            desc.pixel_shader,
            desc.input,
            Some(desc.vertex_shader),
            desc.sampler_ones,
            desc.depth_bias,
        )];
        self.context.shader(vs.raw_key, &vs.source)?;
        self.context.shader(ps.raw_key, &ps.source)?;
        let input = &device.inputs[desc.input.index as usize];
        let blend = &device.blends[desc.blend.index as usize];
        let raster = &device.rasters[desc.rasterizer.index as usize];
        let depth = &device.depths[desc.depth_stencil.index as usize];
        let mut targets: Vec<_> = desc
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
        targets.truncate(targets.iter().rposition(Option::is_some).map_or(0, |n| n + 1));
        let key = PipelineKey {
            vs: vs.raw_key,
            ps: ps.raw_key,
            buffers: input
                .streams
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        s.stride,
                        s.step,
                        input
                            .elements
                            .iter()
                            .filter(|e| e.stream as usize == i)
                            .map(|e| (e.format, e.offset, e.location))
                            .collect(),
                    )
                })
                .collect(),
            topology: desc.topology,
            strip_index: desc.strip_index,
            cull: raster.cull,
            polygon_mode: raster.fill,
            targets,
            depth: desc.depth.map(|format| wgpu::DepthStencilState {
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
            samples: desc.samples,
            sample_mask: desc.sample_mask as u32,
        };
        let kinds = p.kinds.each_ref().map(|k| {
            k.iter()
                .map(|&(slot, kind)| {
                    (
                        slot,
                        match kind {
                            d3d9wgsl::TextureKind::Flat => 0,
                            d3d9wgsl::TextureKind::Cube => 1,
                            d3d9wgsl::TextureKind::Volume => 2,
                        },
                    )
                })
                .collect()
        });
        let pipeline = self.context.pipeline_state(
            &key,
            &kinds[0],
            &kinds[1],
            raster.front_face,
            blend.alpha_to_coverage,
            desc.sample_mask,
        )?;
        device
            .raw_pipelines
            .lock()
            .unwrap()
            .insert(handle, pipeline.template());
        Ok(pipeline)
    }

    pub fn bindings(
        &mut self,
        device: &Device,
        pipeline: Pipeline,
        sets: [TextureSet; 2],
        uniform: &wgpu::BindGroup,
    ) -> Arc<vk::Bindings> {
        let key = (pipeline, sets);
        if let Some(hit) = self.bindings.get(&key) {
            return hit.clone();
        }
        if !self.pipelines.contains_key(&pipeline) {
            let raw = self
                .pipeline(device, pipeline)
                .map_err(|e| eprintln!("native Vulkan pipeline fallback: {e}"))
                .ok();
            self.pipelines.insert(pipeline, raw);
        }
        let native = self.pipelines[&pipeline].clone().and_then(|p| {
            for (stage, handle) in sets.iter().enumerate() {
                let set = &device.sets[handle.index as usize];
                if self.context.groups.contains_key(&set.gpu) {
                    continue;
                }
                let mut views = vec![];
                for b in &set.bindings {
                    self.view(device, b.view)?;
                    let sampler = &device.samplers[b.sampler.index as usize];
                    if !self.context.samplers.contains_key(&sampler.gpu) {
                        if let Some(raw) = &sampler.raw {
                            self.context.samplers.insert(sampler.gpu.clone(), raw.clone());
                        } else {
                            self.context.sampler(&sampler.gpu, sampler.desc.as_ref()?).ok()?;
                        }
                    }
                    views.push((
                        b.slot,
                        device.views[b.view.index as usize].gpu.clone(),
                        sampler.gpu.clone(),
                    ));
                }
                self.context.group(&set.gpu, &p, stage, &views).ok()?;
            }
            Some((
                p,
                sets.map(|s| self.context.groups[&device.sets[s.index as usize].gpu].clone()),
            ))
        });
        let bindings = Arc::new(vk::Bindings {
            pipeline: device.pipelines[pipeline.index as usize].gpu.clone(),
            groups: [
                uniform.clone(),
                device.sets[sets[0].index as usize].gpu.clone(),
                device.sets[sets[1].index as usize].gpu.clone(),
            ],
            native,
        });
        self.bindings.insert(key, bindings.clone());
        bindings
    }

    pub fn begin(&mut self, device: &Device, desc: PassDesc, size: [u32; 2]) {
        let end = desc
            .colours
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |n| n + 1);
        let native_colors = desc.colours[..end]
            .iter()
            .map(|h| h.and_then(|h| self.view(device, h)))
            .collect();
        let native_depth = desc.depth.and_then(|h| self.view(device, h));
        self.pass = Some(vk::Pass {
            native_colors,
            native_depth,
            colors: desc.colours[..end]
                .iter()
                .map(|h| h.map(|h| device.views[h.index as usize].gpu.clone()))
                .collect(),
            depth: desc.depth.map(|h| {
                let v = &device.views[h.index as usize];
                (v.gpu.clone(), v.format)
            }),
            extent: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            draws: vec![],
            initialized: self.context.initialized.clone(),
            initialize: false,
            clear_colour: desc.clear_colour,
            clear_depth: desc.clear_depth,
            clear_stencil: desc.clear_stencil,
        });
    }

    pub fn end(&mut self, device: &Device) {
        if let Some(pass) = self.pass.take() {
            vk::Chunk::push_pass(&mut self.chunks, &device.gpu, pass, device.raw_chunk_draws);
        }
    }
}

impl Recorder<'_> {
    pub(super) fn raw_draw(&mut self, range: Range<u32>, base: Option<i32>, instances: Range<u32>) {
        let raw = self.raw.as_mut().unwrap();
        let pipeline = self.pipeline.unwrap();
        let p = &self.device.pipelines[pipeline.index as usize];
        let streams = self.device.inputs[p.desc.input.index as usize].streams.len();
        let bindings = raw.bindings(self.device, pipeline, self.sets.map(Option::unwrap), &self.group);
        let state = &self.pass.as_ref().unwrap().last;
        let [r, g, b, a] = self.blend;
        raw.pass.as_mut().unwrap().draws.push(crate::vk::Draw {
            bindings,
            offsets: state.offsets.unwrap(),
            vertex: self.vertex[..streams]
                .iter()
                .map(|s| {
                    let s = s.unwrap();
                    (
                        self.device.buffers[s.buffer.index as usize].clone(),
                        s.offset,
                        s.offset + s.size,
                    )
                })
                .collect(),
            index: base.and_then(|_| self.index).map(|(s, f)| {
                (
                    self.device.buffers[s.buffer.index as usize].clone(),
                    s.offset,
                    s.offset + s.size,
                    f,
                )
            }),
            viewport: state.viewport.unwrap(),
            scissor: state.scissor,
            stencil: self.stencil,
            blend: wgpu::Color { r, g, b, a },
            first: range.start,
            count: range.end - range.start,
            base: base.unwrap_or(0),
            instances: instances.end - instances.start,
            first_instance: instances.start,
        });
    }
    /// Same upload ordering contract as finish_parts, with raw chunks retained
    /// for the existing ordered finisher and GPU-completion keepalives.
    pub(crate) fn finish_chunks(&mut self) -> (Option<wgpu::CommandEncoder>, Vec<crate::vk::Chunk>) {
        if self.raw.is_none() {
            let (upload, encoder) = self.finish_parts();
            return (upload, encoder.into_iter().map(crate::vk::Chunk::Wgpu).collect());
        }
        self.end_pass();
        let chunks = std::mem::take(&mut self.raw.as_mut().unwrap().chunks);
        let upload = if chunks.is_empty() {
            None
        } else {
            self.ring
                .upload(&self.device.gpu, &self.device.queue, &mut self.stats)
        };
        self.ring.restart();
        (upload, chunks)
    }
}
