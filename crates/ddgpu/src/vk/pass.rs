use super::{
    objects::{Group, Pipeline, View},
    DRAWS, ERRORS, FALLBACK, INITIALIZE, PASSES,
};
use crate::cache::{FxMap, FxSet};
use std::{
    num::NonZeroU64,
    sync::{atomic::Ordering, Arc, Mutex},
};
use wgpu::hal::{self, api::Vulkan, CommandEncoder};

#[derive(Default, Clone)]
pub(crate) struct Initialized {
    buffers: FxSet<wgpu::Buffer>,
    views: FxSet<wgpu::TextureView>,
}

pub(crate) struct Bindings {
    pub pipeline: wgpu::RenderPipeline,
    pub groups: [wgpu::BindGroup; 3],
    pub native: Option<(Arc<Pipeline>, [Arc<Group>; 2])>,
}

pub(crate) struct Draw {
    pub bindings: Arc<Bindings>,
    pub offsets: [u32; 2],
    pub vertex: smallvec::SmallVec<[(wgpu::Buffer, u64, u64); 1]>,
    pub index: Option<(wgpu::Buffer, u64, u64, wgpu::IndexFormat)>,
    pub viewport: [f32; 6],
    pub scissor: Option<[u32; 4]>,
    pub stencil: u32,
    pub blend: wgpu::Color,
    pub first: u32,
    pub count: u32,
    pub base: i32,
    pub instances: u32,
}

pub(crate) struct Pass {
    pub native_colors: Vec<Option<Arc<View>>>,
    pub native_depth: Option<Arc<View>>,
    pub colors: Vec<Option<wgpu::TextureView>>,
    pub depth: Option<(wgpu::TextureView, wgpu::TextureFormat)>,
    pub extent: wgpu::Extent3d,
    pub draws: Vec<Draw>,
    pub initialized: Arc<Mutex<Initialized>>,
    pub initialize: bool,
}

/// A submission payload. The caller must retain `keepalive` through the queue's
/// completion callback, not merely through `Queue::submit`.
pub(crate) struct Ready {
    pub commands: Vec<wgpu::CommandBuffer>,
    pub keepalive: Vec<Pass>,
}
impl Ready {
    pub fn submit(self, queue: &wgpu::Queue) {
        queue.submit(self.commands);
        // Publish initialization only after ordered submission, never from an
        // encoding worker: a later chunk may finish before an earlier chunk.
        for pass in self.keepalive.iter().filter(|p| p.initialize) {
            if let Some((buffers, _)) = pass.resources() {
                let mut old = pass.initialized.lock().unwrap();
                old.buffers.extend(buffers.into_keys());
                old.views.extend(pass.views().cloned());
            }
        }
        if !self.keepalive.is_empty() {
            queue.on_submitted_work_done(move || drop(self.keepalive));
        }
    }
}

impl Pass {
    pub fn finish_batch(device: &wgpu::Device, passes: Vec<Self>) -> Vec<Ready> {
        let Some(first) = passes.first() else {
            return vec![];
        };
        let mut initialized = first.initialized.lock().unwrap().clone();
        let mut ready = vec![];
        let mut run = vec![];
        for mut pass in passes {
            let resources = pass.resources();
            let usable = resources.as_ref().is_some_and(|(buffers, _)| {
                buffers.keys().all(|b| initialized.buffers.contains(b))
                    && pass.views().all(|v| initialized.views.contains(v))
            });
            if usable {
                run.push((pass, resources.unwrap()));
                continue;
            }
            Self::flush_run(device, &mut run, &mut ready);
            FALLBACK.fetch_add(1, Ordering::Relaxed);
            let command = pass.wgpu(device);
            if let Some((buffers, _)) = resources {
                INITIALIZE.fetch_add(1, Ordering::Relaxed);
                initialized.buffers.extend(buffers.into_keys());
                initialized.views.extend(pass.views().cloned());
                pass.initialize = true;
            }
            ready.push(Ready {
                commands: vec![command],
                keepalive: vec![pass],
            });
        }
        Self::flush_run(device, &mut run, &mut ready);
        ready
    }

    fn flush_run(device: &wgpu::Device, run: &mut Vec<(Pass, Resources)>, ready: &mut Vec<Ready>) {
        if run.is_empty() {
            return;
        }
        match Self::raw_batch(device, run) {
            Ok(commands) => {
                PASSES.fetch_add(run.len() as u64, Ordering::Relaxed);
                DRAWS.fetch_add(
                    run.iter().map(|(p, _)| p.draws.len() as u64).sum::<u64>(),
                    Ordering::Relaxed,
                );
                ready.push(Ready {
                    commands,
                    keepalive: std::mem::take(run).into_iter().map(|(p, _)| p).collect(),
                });
            }
            Err(e) => {
                if ERRORS.fetch_add(1, Ordering::Relaxed) == 0 {
                    eprintln!("DDGPU_VK encode fallback: {e}");
                }
                for (pass, _) in run.drain(..) {
                    FALLBACK.fetch_add(1, Ordering::Relaxed);
                    ready.push(Ready {
                        commands: vec![pass.wgpu(device)],
                        keepalive: vec![pass],
                    });
                }
            }
        }
    }

    fn views(&self) -> impl Iterator<Item = &wgpu::TextureView> {
        self.colors
            .iter()
            .flatten()
            .chain(self.depth.iter().map(|(v, _)| v))
            .chain(
                self.draws
                    .iter()
                    .filter_map(|d| d.bindings.native.as_ref())
                    .flat_map(|(_, groups)| groups.iter().flat_map(|g| g.textures.iter())),
            )
    }

    fn resources(&self) -> Option<Resources> {
        if self
            .colors
            .iter()
            .zip(&self.native_colors)
            .any(|(w, n)| w.is_some() != n.is_some())
            || self.depth.is_some() != self.native_depth.is_some()
        {
            return None;
        }
        let mut buffers = FxMap::default();
        let mut groups_seen = FxSet::default();
        let mut textures = FxMap::default();
        let mut texture = |view: &wgpu::TextureView, usage| {
            let tx = view.texture();
            if tx.sample_count() != 1 {
                return None;
            }
            // Track each owner once per pass; repeated draws only compare
            // borrowed handles and never enter wgpu-core resource tracking.
            if let Some(old) = textures.get(tx) {
                if *old != usage { return None; }
            } else {
                textures.insert(tx.clone(), usage);
            }
            Some(())
        };
        let mut attachments = FxSet::default();
        for view in self.colors.iter().flatten() {
            if !attachments.insert(view.texture().clone()) {
                return None;
            }
            texture(view, wgpu::TextureUses::COLOR_TARGET)?;
        }
        if let Some((view, _)) = &self.depth {
            if !attachments.insert(view.texture().clone()) {
                return None;
            }
            texture(view, wgpu::TextureUses::DEPTH_STENCIL_WRITE)?;
        }
        for draw in &self.draws {
            let (pipeline, groups) = draw.bindings.native.as_ref()?;
            if pipeline.samples != 1 {
                return None;
            }
            if draw.vertex.len() < pipeline.vertex_layouts.len() {
                return None;
            }
            for (i, (stride, step, last_attribute)) in pipeline.vertex_layouts.iter().enumerate() {
                let (_, start, end) = &draw.vertex[i];
                let count = if *step == wgpu::VertexStepMode::Instance {
                    draw.instances as u64
                } else if draw.index.is_some() {
                    1
                } else {
                    draw.first as u64 + draw.count as u64
                };
                if start > end
                    || (count > 0
                        && (count - 1).checked_mul(*stride)?.checked_add(*last_attribute)? > end - start)
                {
                    return None;
                }
            }
            for (offset, size) in draw
                .offsets
                .iter()
                .zip([crate::cache::PS_BLOCK, crate::cache::VS_BLOCK])
            {
                if offset % crate::cache::UNIFORM_ALIGN as u32 != 0
                    || *offset as u64 + size > pipeline.uniforms.buffers[0].size()
                {
                    return None;
                }
            }
            for b in &pipeline.uniforms.buffers {
                include_buffer(&mut buffers, b, wgpu::BufferUses::UNIFORM);
            }
            for group in groups {
                if !groups_seen.insert(Arc::as_ptr(group) as usize) { continue; }
                for view in &group.textures {
                    texture(view, wgpu::TextureUses::RESOURCE)?;
                }
            }
            for (b, start, end) in &draw.vertex {
                if start >= end || *end > b.size() || start % 4 != 0 {
                    return None;
                }
                include_buffer(&mut buffers, b, wgpu::BufferUses::VERTEX);
            }
            if let Some((b, start, end, format)) = &draw.index {
                let stride = if *format == wgpu::IndexFormat::Uint16 {
                    2
                } else {
                    4
                };
                if start >= end
                    || *end > b.size()
                    || start % stride != 0
                    || (draw.first as u64 + draw.count as u64) * stride > end - start
                {
                    return None;
                }
                include_buffer(&mut buffers, b, wgpu::BufferUses::INDEX);
            }
            let [x, y, w, h, min, max] = draw.viewport;
            if ![x, y, w, h, min, max].into_iter().all(f32::is_finite)
                || x < 0.0
                || y < 0.0
                || w <= 0.0
                || h <= 0.0
                || x + w > self.extent.width as f32
                || y + h > self.extent.height as f32
                || !(0.0..=1.0).contains(&min)
                || !(0.0..=1.0).contains(&max)
            {
                return None;
            }
            if let Some([x, y, w, h]) = draw.scissor {
                if x.checked_add(w)? > self.extent.width || y.checked_add(h)? > self.extent.height {
                    return None;
                }
            }
        }
        Some((buffers, textures))
    }

    fn wgpu(&self, device: &wgpu::Device) -> wgpu::CommandBuffer {
        let mut encoder = device.create_command_encoder(&Default::default());
        let colors = self
            .colors
            .iter()
            .map(|view| {
                view.as_ref().map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })
            })
            .collect::<Vec<_>>();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("L138 fallback"),
                color_attachments: &colors,
                depth_stencil_attachment: self.depth.as_ref().map(|(view, format)| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: format.has_stencil_aspect().then_some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                    }
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for d in &self.draws {
                pass.set_pipeline(&d.bindings.pipeline);
                for (i, g) in d.bindings.groups.iter().enumerate() {
                    pass.set_bind_group(i as u32, g, if i == 0 { &d.offsets } else { &[] });
                }
                let [x, y, w, h, min, max] = d.viewport;
                pass.set_viewport(x, y, w, h, min, max);
                let [x, y, w, h] = d.scissor.unwrap_or([0, 0, self.extent.width, self.extent.height]);
                pass.set_scissor_rect(x, y, w, h);
                pass.set_stencil_reference(d.stencil);
                pass.set_blend_constant(d.blend);
                for (i, (b, start, end)) in d.vertex.iter().enumerate() {
                    pass.set_vertex_buffer(i as u32, b.slice(*start..*end));
                }
                if let Some((b, start, end, format)) = &d.index {
                    pass.set_index_buffer(b.slice(*start..*end), *format);
                    pass.draw_indexed(d.first..d.first + d.count, d.base, 0..d.instances);
                } else {
                    pass.draw(d.first..d.first + d.count, 0..d.instances);
                }
            }
        }
        encoder.finish()
    }

    fn raw_batch(
        device: &wgpu::Device,
        passes: &[(Pass, Resources)],
    ) -> Result<Vec<wgpu::CommandBuffer>, String> {
        let mut buffers: FxMap<wgpu::Buffer, wgpu::BufferUses> = FxMap::default();
        let mut first = FxMap::default();
        for (_, (bs, ts)) in passes {
            for (b, s) in bs {
                buffers.entry(b.clone()).and_modify(|v| *v |= *s).or_insert(*s);
            }
            for (t, s) in ts {
                first.entry(t.clone()).or_insert(*s);
            }
        }
        // Each resource starts in the layout declared to core. Between passes
        // HAL barriers preserve write/read and write/write dependencies. Restore
        // those starting layouts before returning control to wgpu's tracker.
        unsafe {
            for b in buffers.keys() {
                b.as_hal::<Vulkan>().ok_or("buffer destroyed")?;
            }
            for t in first.keys() {
                t.as_hal::<Vulkan>().ok_or("texture destroyed")?;
            }
            let mut current = first.clone();
            let mut used = FxSet::default();
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.as_hal_mut::<Vulkan, _, _>(|h| {
                let h = h.ok_or("not Vulkan".to_string())?;
                for (pass, (_, textures)) in passes {
                    for (t, to) in textures {
                        let from = current[t];
                        if used.contains(t) && (from != *to || from != wgpu::TextureUses::RESOURCE) {
                            transition(h, t, from, *to)?;
                        }
                        current.insert(t.clone(), *to);
                        used.insert(t.clone());
                    }
                    pass.record(h)?;
                }
                for (t, from) in current {
                    let to = first[&t];
                    if from != to {
                        transition(h, &t, from, to)?;
                    }
                }
                Ok::<_, String>(())
            })?;
            let raw = encoder.finish();
            let mut before = device.create_command_encoder(&Default::default());
            before.transition_resources(
                buffers.iter().map(|(buffer, state)| wgpu::BufferTransition {
                    buffer,
                    state: *state,
                }),
                first.iter().map(|(texture, state)| wgpu::TextureTransition {
                    texture,
                    selector: None,
                    state: *state,
                }),
            );
            Ok(vec![before.finish(), raw])
        }
    }

    // SAFETY: checked resources and recipes; all Vulkan objects retained by
    // this pass until GPU completion. Only one snatch guard exists at a time.
    unsafe fn record(&self, h: &mut hal::vulkan::CommandEncoder) -> Result<(), String> {
        unsafe {
            let attachments = self
                .native_colors
                .iter()
                .map(|v| {
                    v.as_ref().map(|v| hal::ColorAttachment {
                        target: hal::Attachment {
                            view: &*v.raw,
                            usage: wgpu::TextureUses::COLOR_TARGET,
                        },
                        depth_slice: None,
                        resolve_target: None,
                        ops: hal::AttachmentOps::LOAD | hal::AttachmentOps::STORE,
                        clear_value: wgpu::Color::TRANSPARENT,
                    })
                })
                .collect::<Vec<_>>();
            h.begin_render_pass(&hal::RenderPassDescriptor {
                label: None,
                extent: self.extent,
                sample_count: 1,
                color_attachments: &attachments,
                depth_stencil_attachment: self.native_depth.as_ref().map(|v| hal::DepthStencilAttachment {
                    target: hal::Attachment {
                        view: &*v.raw,
                        usage: wgpu::TextureUses::DEPTH_STENCIL_WRITE,
                    },
                    depth_ops: hal::AttachmentOps::LOAD | hal::AttachmentOps::STORE,
                    stencil_ops: if self.depth.as_ref().unwrap().1.has_stencil_aspect() {
                        hal::AttachmentOps::LOAD | hal::AttachmentOps::STORE
                    } else {
                        hal::AttachmentOps::empty()
                    },
                    clear_value: (0.0, 0),
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .map_err(|e| e.to_string())?;
            let mut last: Option<&Draw> = None;
            for d in &self.draws {
                let (p, groups) = d.bindings.native.as_ref().unwrap();
                let previous = last.and_then(|d| d.bindings.native.as_ref());
                let layout_changed = previous.is_none_or(|(old, _)| !Arc::ptr_eq(&old.layout, &p.layout));
                if previous.is_none_or(|(old, _)| !Arc::ptr_eq(old, p)) {
                    h.set_render_pipeline(&p.raw);
                }
                if layout_changed || last.is_none_or(|old| old.offsets != d.offsets) {
                    h.set_bind_group(&p.layout.raw, 0, &p.uniforms.raw, &d.offsets);
                }
                for (i, g) in groups.iter().enumerate() {
                    if layout_changed || previous.is_none_or(|(_, old)| !Arc::ptr_eq(&old[i], g)) {
                        h.set_bind_group(&p.layout.raw, i as u32 + 1, &g.raw, &[]);
                    }
                }
                let [x, y, w, height, min, max] = d.viewport;
                if last.is_none_or(|old| old.viewport != d.viewport) {
                    h.set_viewport(&hal::Rect { x, y, w, h: height }, min..max);
                }
                let [x, y, w, height] = d.scissor.unwrap_or([0, 0, self.extent.width, self.extent.height]);
                if last.is_none_or(|old| old.scissor != d.scissor) {
                    h.set_scissor_rect(&hal::Rect { x, y, w, h: height });
                }
                if last.is_none_or(|old| old.stencil != d.stencil) {
                    h.set_stencil_reference(d.stencil);
                }
                if last.is_none_or(|old| old.blend != d.blend) {
                    h.set_blend_constants(&[
                        d.blend.r as f32,
                        d.blend.g as f32,
                        d.blend.b as f32,
                        d.blend.a as f32,
                    ]);
                }
                for (i, (b, start, end)) in d.vertex.iter().enumerate() {
                    if last.is_some_and(|old| old.vertex.get(i) == d.vertex.get(i)) {
                        continue;
                    }
                    let guard = b
                        .as_hal::<Vulkan>()
                        .expect("owned buffer checked before encoding");
                    h.set_vertex_buffer(
                        i as u32,
                        hal::BufferBinding::new_unchecked(&*guard, *start, NonZeroU64::new(end - start)),
                    );
                }
                if let Some((b, start, end, format)) = &d.index {
                    if last.is_none_or(|old| old.index != d.index) {
                        let guard = b
                            .as_hal::<Vulkan>()
                            .expect("owned buffer checked before encoding");
                        h.set_index_buffer(
                            hal::BufferBinding::new_unchecked(&*guard, *start, NonZeroU64::new(end - start)),
                            *format,
                        );
                    }
                    h.draw_indexed(d.first, d.count, d.base, 0, d.instances);
                } else {
                    h.draw(d.first, d.count, 0, d.instances);
                }
                last = Some(d);
            }
            h.end_render_pass();
            Ok(())
        }
    }
}

type Resources = (
    FxMap<wgpu::Buffer, wgpu::BufferUses>,
    FxMap<wgpu::Texture, wgpu::TextureUses>,
);

unsafe fn transition(
    h: &mut hal::vulkan::CommandEncoder,
    texture: &wgpu::Texture,
    from: wgpu::TextureUses,
    to: wgpu::TextureUses,
) -> Result<(), String> {
    unsafe {
        let guard = texture.as_hal::<Vulkan>().ok_or("texture destroyed")?;
        h.transition_textures(std::iter::once(hal::TextureBarrier {
            texture: &*guard,
            range: wgpu::ImageSubresourceRange {
                aspect: wgpu::TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: Some(texture.mip_level_count()),
                base_array_layer: 0,
                array_layer_count: Some(if texture.dimension() == wgpu::TextureDimension::D3 {
                    1
                } else {
                    texture.depth_or_array_layers()
                }),
            },
            usage: hal::StateTransition { from, to },
        }));
    }
    Ok(())
}

// All recorded draws retain their owners through queue completion. Building
// the pass preamble clones each distinct buffer once, never once per draw.
fn include_buffer(buffers: &mut FxMap<wgpu::Buffer, wgpu::BufferUses>, buffer: &wgpu::Buffer, usage: wgpu::BufferUses) {
    if let Some(old) = buffers.get_mut(buffer) { *old |= usage; }
    else { buffers.insert(buffer.clone(), usage); }
}
