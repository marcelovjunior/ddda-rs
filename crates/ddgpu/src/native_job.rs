//! N2 worker adapter. Guest state is already decoded; all resources resolve
//! through this backend's maps in FIFO order. Rejection never submits a job.
use crate::native_compare::{CompareAspect, CompareQueue, CompareResult, CompareTag};
use crate::{cache::FxMap, native as n, WgpuBackend};
use d3d9cmd::*;
use std::{ops::Range, sync::Arc};

#[derive(Clone)]
struct Shadow {
    texture: wgpu::Texture,
    linear: wgpu::TextureView,
    srgb: wgpu::TextureView,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}
struct ComparePair {
    target: Target,
    oracle: wgpu::Texture,
    level: u32,
    layer: u32,
    shadow: Shadow,
}
struct CompareSnapshot {
    begin: NativeCompareBegin,
    pairs: Vec<ComparePair>,
}
pub(crate) struct Comparisons {
    queue: CompareQueue,
    active: Option<CompareSnapshot>,
    shadows: [Option<Shadow>; 5],
    completed: std::collections::VecDeque<Result<CompareResult, String>>,
    matched: u64,
    mismatched: u64,
    unverified: u64,
}
impl Comparisons {
    fn new(device: &wgpu::Device) -> Self {
        // Host diagnostic budget, not a game-derived constant.
        Self {
            queue: CompareQueue::new(device, 256),
            active: None,
            shadows: Default::default(),
            completed: Default::default(),
            matched: 0,
            mismatched: 0,
            unverified: 0,
        }
    }
    fn report(&mut self, result: Result<CompareResult, String>) {
        match &result {
            Ok(r) if r.differing_pixels == 0 => {
                self.matched += 1;
                if self.matched == 1 || self.matched % 128 == 0 {
                    eprintln!(
                        "native-compare totals: matched={} mismatched={} unverified={} pending={}",
                        self.matched,
                        self.mismatched,
                        self.unverified,
                        self.queue.pending()
                    );
                }
            }
            Ok(r) => {
                self.mismatched += 1;
                eprintln!("{r}");
            }
            Err(e) => {
                self.unverified += 1;
                eprintln!("{e}");
            }
        }
        // Logs/counters retain every outcome. The optional structured drain
        // keeps only the latest 256 to bound unattended diagnostic memory.
        if self.completed.len() == 256 {
            self.completed.pop_front();
        }
        self.completed.push_back(result);
    }
}
impl Drop for Comparisons {
    fn drop(&mut self) {
        eprintln!(
            "native-compare shutdown: matched={} mismatched={} unverified={} pending={} active_begin={:?}",
            self.matched,
            self.mismatched,
            self.unverified,
            self.queue.pending(),
            self.active.as_ref().map(|x| x.begin.id)
        );
        if self.queue.pending() != 0 || self.active.is_some() {
            eprintln!("native-compare unverified: shutdown leaves pending/unfinished comparisons; no GPU wait performed");
        }
    }
}
impl WgpuBackend {
    /// Nonblocking structured diagnostic results (latest 256 retained). The
    /// backend also polls at native jobs and Present and logs every failure.
    pub fn poll_native_comparisons(&mut self) -> Vec<Result<CompareResult, String>> {
        self.poll_native_comparisons_internal();
        self.native_comparisons
            .as_mut()
            .map_or_else(Vec::new, |c| c.completed.drain(..).collect())
    }
    pub(crate) fn poll_native_comparisons_internal(&mut self) {
        if let Some(c) = &mut self.native_comparisons {
            match c.queue.poll(&self.device) {
                Ok(results) => {
                    for r in results {
                        c.report(r);
                    }
                }
                Err(e) => c.report(Err(e)),
            }
        }
    }
    pub(crate) fn begin_native_comparison(&mut self, begin: NativeCompareBegin) -> bool {
        self.poll_native_comparisons_internal();
        let mut comparisons = self
            .native_comparisons
            .take()
            .unwrap_or_else(|| Comparisons::new(&self.device));
        let result = self.seed_native_comparison(&begin, &mut comparisons);
        let ok = result.is_ok();
        match result {
            Ok(pairs) => comparisons.active = Some(CompareSnapshot { begin, pairs }),
            Err(e) => comparisons.report(Err(format!("native-compare unverified begin {}: {e}", begin.id))),
        }
        self.native_comparisons = Some(comparisons);
        ok
    }
    fn seed_native_comparison(
        &mut self,
        begin: &NativeCompareBegin,
        comparisons: &mut Comparisons,
    ) -> Result<Vec<ComparePair>, String> {
        if comparisons.active.is_some() {
            return Err("another begin is active".into());
        }
        if !self.active_queries.is_empty() {
            return Err("occlusion query active".into());
        }
        self.flush(); // Includes waiting for prior asynchronous finish/submit.
        let mut pairs = vec![];
        let mut needed = 0;
        for (slot, target) in begin.colours.iter().chain([&begin.depth]).enumerate() {
            let Some(target) = target else {
                continue;
            };
            if crate::sample_count(target)? != 1 {
                return Err("MSAA comparison unsupported".into());
            }
            let (oracle, level, layer, format, width, height) = self
                .target_texture(target)
                .ok_or("comparison target unavailable")?;
            if width == 0
                || height == 0
                || width.div_ceil(8) > self.device.limits().max_compute_workgroups_per_dimension
                || height.div_ceil(8) > self.device.limits().max_compute_workgroups_per_dimension
            {
                return Err("comparison target extent unsupported".into());
            }
            if oracle.dimension() != wgpu::TextureDimension::D2 {
                return Err("comparison requires D2 attachments".into());
            }
            if pairs
                .iter()
                .any(|p: &ComparePair| p.oracle == oracle && p.level == level && p.layer == layer)
            {
                return Err("duplicate comparison attachment".into());
            }
            needed += if format.is_depth_stencil_format() {
                usize::from(format.has_depth_aspect()) + usize::from(format.has_stencil_aspect())
            } else {
                1
            };
            if comparisons.queue.available() < needed {
                return Err("comparison summary capacity exhausted".into());
            }
            let replace = comparisons.shadows[slot]
                .as_ref()
                .is_none_or(|s| s.format != format || s.width != width || s.height != height);
            if replace {
                // Native handles retain their views; drop that cache before
                // replacing a pool slot, so window resizing cannot leak it.
                self.native_worker = None;
                let srgb_format = format.add_srgb_suffix();
                let view_formats = if srgb_format != format {
                    vec![srgb_format]
                } else {
                    vec![]
                };
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("native comparison shadow"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &view_formats,
                });
                let linear = texture.create_view(&Default::default());
                let srgb = if srgb_format != format {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(srgb_format),
                        ..Default::default()
                    })
                } else {
                    linear.clone()
                };
                comparisons.shadows[slot] = Some(Shadow {
                    texture,
                    linear,
                    srgb,
                    format,
                    width,
                    height,
                });
            }
            pairs.push(ComparePair {
                target: target.clone(),
                oracle,
                level,
                layer,
                shadow: comparisons.shadows[slot].as_ref().unwrap().clone(),
            });
        }
        if pairs.is_empty() {
            return Err("comparison has no attachments".into());
        }
        // Copy the actual pre-oracle load state, including depth AND stencil.
        // Pool reuse is safe because each prior comparison was submitted first.
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for p in &pairs {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &p.oracle,
                    mip_level: p.level,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: p.layer,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &p.shadow.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: p.shadow.width,
                    height: p.shadow.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.submit([encoder.finish()]);
        Ok(pairs)
    }
    pub(crate) fn end_native_comparison(&mut self, id: u64, job: NativeJob) -> Result<(), NativeJob> {
        let Some(mut comparisons) = self.native_comparisons.take() else {
            return Err(job);
        };
        if comparisons.active.as_ref().is_none_or(|s| s.begin.id != id) {
            comparisons.report(Err(format!(
                "native-compare unverified end {id}: no matching begin"
            )));
            self.native_comparisons = Some(comparisons);
            return Err(job);
        }
        let snapshot = comparisons.active.take().unwrap();
        let compatible = job.passes.len() == 1
            && job.passes[0].colours == snapshot.begin.colours
            && job.passes[0].depth == snapshot.begin.depth;
        if !compatible {
            comparisons.report(Err(format!(
                "native-compare unverified end {id}: expected one pass with begin attachments"
            )));
            self.native_comparisons = Some(comparisons);
            return Err(job);
        }
        self.flush(); // Oracle must be submitted before shadow replay/reduction.
        if let Err(job) = self.run_native_job_overrides(job, &snapshot.pairs) {
            comparisons.report(Err(format!(
                "native-compare unverified end {id}: native job rejected"
            )));
            self.native_comparisons = Some(comparisons);
            return Err(job);
        }
        // Replay can hand its render encoder to the async finisher. Reduction
        // submits directly, so it must follow that submission (no GPU wait).
        self.wait_finisher();
        for p in snapshot.pairs {
            let aspects = if p.shadow.format.is_depth_stencil_format() {
                [
                    (CompareAspect::Depth, p.shadow.format.has_depth_aspect()),
                    (CompareAspect::Stencil, p.shadow.format.has_stencil_aspect()),
                ]
                .into_iter()
                .filter_map(|(a, on)| on.then_some(a))
                .collect::<Vec<_>>()
            } else {
                vec![CompareAspect::Color]
            };
            for aspect in aspects {
                let texture_aspect = match aspect {
                    CompareAspect::Color => wgpu::TextureAspect::All,
                    CompareAspect::Depth => wgpu::TextureAspect::DepthOnly,
                    CompareAspect::Stencil => wgpu::TextureAspect::StencilOnly,
                };
                let desc = wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: p.level,
                    mip_level_count: Some(1),
                    base_array_layer: p.layer,
                    array_layer_count: Some(1),
                    aspect: texture_aspect,
                    ..Default::default()
                };
                let oracle = p.oracle.create_view(&desc);
                let native = p.shadow.texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: 0,
                    base_array_layer: 0,
                    ..desc
                });
                let tag = CompareTag {
                    pass: id,
                    target: p.target.surface,
                    aspect,
                };
                if let Err(e) = comparisons.queue.enqueue(
                    &self.device,
                    &self.queue,
                    tag,
                    &oracle,
                    &native,
                    p.shadow.width,
                    p.shadow.height,
                ) {
                    comparisons.report(Err(e));
                }
            }
        }
        self.native_comparisons = Some(comparisons);
        self.poll_native_comparisons_internal();
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct NativeJobStats {
    pub accepted: u64,
    pub rejected: u64,
    pub rejected_draws: u64,
    pub rejected_passes: u64,
    pub draws: u64,
    pub passes: u64,
    pub upload_bytes: u64,
    /// Ordered queue batches handed to the finisher (not GPU completion).
    pub submit_batches: u64,
    pub dirty_registers: u64,
    pub bundle_builds: u64,
    pub bundle_hits: u64,
    pub worker_instances: u64,
    /// releases of resources the worker held: its resource tables reset
    /// (pipelines kept)
    pub resource_resets: u64,
    /// Worker refusals, not producer enqueue failures. At most 64 exact
    /// reasons plus an overflow bucket, to bound diagnostic memory.
    pub rejection_reasons: std::collections::BTreeMap<String, u64>,
}
impl NativeJobStats {
    fn reject(&mut self, reason: &str, passes: u64, draws: u64) {
        self.rejected += 1;
        self.rejected_passes += passes;
        self.rejected_draws += draws;
        let key = if self.rejection_reasons.contains_key(reason) || self.rejection_reasons.len() < 64 {
            reason
        } else {
            "other (reason limit)"
        };
        let count = self.rejection_reasons.entry(key.to_owned()).or_default();
        *count += 1;
        if *count == 1 || self.rejected <= 16 || count.is_power_of_two() {
            eprintln!("native job rejected before submit: {reason} (reason_count={count})");
        }
    }
}
struct ShaderEntry {
    _tokens: Arc<[u32]>,
    handle: n::Shader,
    interface: Arc<d3d9wgsl::Interface>,
    reads: ConstantReads,
}
#[derive(Default)]
struct ConstantReads {
    floats: Vec<bool>,
    ints: [bool; 16],
    bools: [bool; 16],
}
type InputKey = (n::Shader, usize, [u32; 4]);
struct InputEntry {
    _declaration: Arc<[[u8; 8]]>,
    handle: n::InputLayout,
    streams: Arc<[u32]>,
}
struct FixedEntry {
    values: Arc<[(u32, u32)]>,
    blend: n::Blend,
    raster: n::Rasterizer,
    depth: n::DepthStencil,
}
pub(crate) struct Worker {
    device: n::Device,
    recording: Option<n::RecorderState>,
    constants: FxMap<n::Shader, n::ConstantBuffer>,
    /// the register arrays last applied to each constant buffer (held, so
    /// a pointer compare is exact): the same arrays are not applied again
    applied: FxMap<n::ConstantBuffer, NativeConstants>,
    shaders: FxMap<(usize, n::Stage), ShaderEntry>,
    inputs: FxMap<InputKey, InputEntry>,
    fixed: FxMap<NativeStateIdentity, Vec<FixedEntry>>,
    buffers: FxMap<wgpu::Buffer, n::Buffer>,
    views: FxMap<wgpu::TextureView, n::View>,
    /// render targets by surface and sRGB view (the backend's own target
    /// view cache is cleared at every release)
    targets: FxMap<(u32, bool), (n::View, wgpu::TextureFormat)>,
    sampled: FxMap<(wgpu::Texture, bool), n::View>,
    /// a copy of each texture drawn into and sampled by the same draw
    snapshots: FxMap<wgpu::Texture, wgpu::Texture>,
    samplers: FxMap<[u32; 14], n::Sampler>,
    texture_sets: FxMap<(n::Pipeline, usize), (Arc<[TextureBinding]>, [n::TextureSet; 2])>,
}
struct PreparedDraw {
    /// textures the draw samples while rendering into them: copied into
    /// their snapshots just before it (the D3D9 path's L59 snapshot)
    copies: Vec<(wgpu::Texture, wgpu::Texture)>,
    pipeline: n::Pipeline,
    shaders: [n::Shader; 2],
    sets: [n::TextureSet; 2],
    vertices: Vec<n::BufferSlice>,
    indices: Option<(n::BufferSlice, wgpu::IndexFormat)>,
    range: Range<u32>,
}
struct PreparedPass {
    desc: n::PassDesc,
    draws: Vec<PreparedDraw>,
}
impl WgpuBackend {
    pub fn native_job_stats(&self) -> NativeJobStats {
        self.native_stats.clone()
    }
    fn report_native_worker_stats(&self) {
        let s = &self.native_stats;
        let attempted = s.accepted + s.rejected;
        if attempted == 1 || attempted % 1024 == 0 {
            let (pipelines, texture_sets) = self
                .native_worker
                .as_ref()
                .map(|w| (w.device.pipeline_count(), w.device.texture_set_count()))
                .unwrap_or_default();
            // These are worker outcomes, unlike the producer's enqueue totals.
            // Submitted does not mean GPU-complete or image-verified.
            eprintln!("native-worker: attempted_jobs={attempted} native_jobs={} rejected_jobs={} rejected_draws={} rejected_passes={} submitted_draws={} submitted_passes={} upload_bytes={} dirty_registers={} bundle_builds={} bundle_hits={} worker_instances={} resource_resets={} pipelines={pipelines} texture_sets={texture_sets} submit_batches={}",
                s.accepted, s.rejected, s.rejected_draws, s.rejected_passes, s.draws, s.passes, s.upload_bytes,
                s.dirty_registers, s.bundle_builds, s.bundle_hits, s.worker_instances, s.resource_resets, s.submit_batches);
            if !s.rejection_reasons.is_empty() {
                eprintln!("native-worker refusal histogram: {:?}", s.rejection_reasons);
            }
        }
    }
    pub(crate) fn run_native_job(&mut self, job: NativeJob) -> Result<(), NativeJob> {
        self.poll_native_comparisons_internal();
        self.run_native_job_overrides(job, &[])
    }
    fn run_native_job_overrides(
        &mut self,
        job: NativeJob,
        overrides: &[ComparePair],
    ) -> Result<(), NativeJob> {
        let draws = job.passes.iter().map(|p| p.draws.len() as u64).sum::<u64>();
        let passes = job.passes.len() as u64;
        if !self.active_queries.is_empty() {
            self.native_stats.reject("occlusion query active", passes, draws);
            self.report_native_worker_stats();
            return Err(job);
        }
        // Hand off earlier D3D9 draws. Native preparation/recording can overlap
        // their finish; wait before the first native constant queue write.
        self.flush_async();
        let mut worker = self.native_worker.take().unwrap_or_else(|| {
            self.native_stats.worker_instances += 1;
            Worker::new(self)
        });
        let result = worker.run(self, &job, overrides);
        self.native_worker = Some(worker);
        let outcome = match result {
            Ok(()) => {
                self.native_stats.accepted += 1;
                self.native_stats.draws += draws;
                self.native_stats.passes += job.passes.len() as u64;
                if overrides.is_empty() {
                    self.draws_recorded.set(self.draws_recorded.get() + draws);
                    self.passes_begun
                        .set(self.passes_begun.get() + job.passes.len() as u64);
                }
                Ok(())
            }
            Err(reason) => {
                self.native_stats.reject(&reason, passes, draws);
                Err(job)
            }
        };
        self.report_native_worker_stats();
        outcome
    }
}
impl Worker {
    fn new(backend: &WgpuBackend) -> Self {
        Self {
            device: n::Device::new_cached(backend),
            recording: None,
            constants: Default::default(),
            applied: Default::default(),
            shaders: Default::default(),
            inputs: Default::default(),
            fixed: Default::default(),
            buffers: Default::default(),
            views: Default::default(),
            targets: Default::default(),
            sampled: Default::default(),
            snapshots: Default::default(),
            samplers: Default::default(),
            texture_sets: Default::default(),
        }
    }
    fn shader(
        &mut self,
        tokens: &Arc<[u32]>,
        stage: n::Stage,
    ) -> Result<(n::Shader, Arc<d3d9wgsl::Interface>), String> {
        let key = (tokens.as_ptr() as usize, stage);
        if !self.shaders.contains_key(&key) {
            let parsed = d3d9shader::parse(tokens).map_err(|e| e.to_string())?;
            let reads = ConstantReads::from_shader(&parsed)?;
            let interface = Arc::new(d3d9wgsl::translate(&parsed).map_err(|e| e.to_string())?.interface);
            let handle = self.device.create_compact_shader(tokens, stage)?;
            self.shaders.insert(
                key,
                ShaderEntry {
                    _tokens: tokens.clone(),
                    handle,
                    interface,
                    reads,
                },
            );
        }
        let s = &self.shaders[&key];
        Ok((s.handle, s.interface.clone()))
    }
    fn target(
        &mut self,
        backend: &mut WgpuBackend,
        target: &Target,
        srgb: bool,
        overrides: &[ComparePair],
    ) -> Result<(n::View, wgpu::TextureFormat), String> {
        if crate::sample_count(target)? != 1 {
            return Err("native job MSAA unsupported".into());
        }
        let shadow = overrides.iter().find(|p| p.target == *target);
        if shadow.is_none() {
            if let Some(&hit) = self.targets.get(&(target.surface, srgb)) {
                return Ok(hit);
            }
        }
        let (view, format) = if let Some(pair) = shadow {
            (
                if srgb {
                    pair.shadow.srgb.clone()
                } else {
                    pair.shadow.linear.clone()
                },
                if srgb {
                    pair.shadow.format.add_srgb_suffix()
                } else {
                    pair.shadow.format
                },
            )
        } else {
            backend
                .cached_target_view(target, srgb)
                .ok_or("native target unavailable")?
        };
        let handle = *self.views.entry(view.clone()).or_insert_with(|| {
            let texture = view.texture();
            let level = if shadow.is_none() && target.texture.is_some() { target.level % texture.mip_level_count() } else { 0 };
            let layer = if shadow.is_none() && target.texture.is_some() && texture.depth_or_array_layers() > 1 { target.level / texture.mip_level_count() } else { 0 };
            let desc = wgpu::TextureViewDescriptor { format:Some(format), dimension:Some(wgpu::TextureViewDimension::D2), base_mip_level:level, mip_level_count:Some(1), base_array_layer:layer, array_layer_count:Some(1), ..Default::default() };
            self.device.retain_view(view, format, [target.width, target.height], 1, desc)
        });
        if shadow.is_none() {
            self.targets.insert((target.surface, srgb), (handle, format));
        }
        Ok((handle, format))
    }
    /// The snapshot texture of `src` (made once, reused: each feedback draw
    /// copies into it in order, after earlier draws sampled it).
    fn snapshot(&mut self, device: &wgpu::Device, src: &wgpu::Texture) -> wgpu::Texture {
        self.snapshots
            .entry(src.clone())
            .or_insert_with(|| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("native feedback snapshot"),
                    size: src.size(),
                    mip_level_count: src.mip_level_count(),
                    sample_count: 1,
                    dimension: src.dimension(),
                    format: src.format(),
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[src.format().add_srgb_suffix(), src.format().remove_srgb_suffix()],
                })
            })
            .clone()
    }
    /// Whether the worker holds `buffer` or a view of `texture`.
    pub(crate) fn holds(&self, buffer: Option<&wgpu::Buffer>, texture: Option<&wgpu::Texture>) -> bool {
        buffer.is_some_and(|b| self.buffers.contains_key(b))
            || texture.is_some_and(|t| self.sampled.contains_key(&(t.clone(), false)) || self.sampled.contains_key(&(t.clone(), true)))
    }
    /// Forget every resource the worker imported, with the recorder state
    /// that may still name them; keep shaders, states and pipelines.
    pub(crate) fn reset_resources(&mut self) {
        self.device.reset_resources();
        self.snapshots.clear();
        self.recording = None;
        self.constants.clear();
        self.applied.clear();
        self.buffers.clear();
        self.views.clear();
        self.targets.clear();
        self.sampled.clear();
        self.texture_sets.clear();
    }
    fn buffer(
        &mut self,
        backend: &WgpuBackend,
        resource: u32,
        offset: u32,
    ) -> Result<n::BufferSlice, String> {
        let gpu = backend
            .buffers
            .get(&resource)
            .ok_or("native buffer not uploaded")?;
        if offset as u64 >= gpu.size() {
            return Err("native buffer offset outside resource".into());
        }
        let handle = *self
            .buffers
            .entry(gpu.clone())
            .or_insert_with(|| self.device.retain_buffer(gpu.clone()));
        Ok(n::BufferSlice {
            buffer: handle,
            offset: offset as u64,
            size: gpu.size() - offset as u64,
        })
    }
    fn fixed(
        &mut self,
        state: &NativeFixedState,
    ) -> Result<(n::Blend, n::Rasterizer, n::DepthStencil), String> {
        validate_fixed(state)?;
        // Hash identities only; compare actual words to defend reuse/hash collisions.
        let entries = self.fixed.entry(state.identity).or_default();
        if let Some(e) = entries.iter().find(|e| {
            e.values
                .iter()
                .filter(|v| !matches!(v.0, 57 | 193))
                .eq(state.render_states.iter().filter(|v| !matches!(v.0, 57 | 193)))
        }) {
            return Ok((e.blend, e.raster, e.depth));
        }
        let s = |key| state_value(state, key);
        let component = |src, dst, op| wgpu::BlendComponent {
            src_factor: crate::blend_factor(s(src)),
            dst_factor: crate::blend_factor(s(dst)),
            operation: crate::blend_op(s(op)),
        };
        let colour = component(19, 20, 171);
        let alpha = if s(206) != 0 {
            component(207, 208, 209)
        } else {
            colour
        };
        let blend = self.device.create_blend(n::BlendDesc {
            targets: [168, 190, 191, 192].map(|key| n::BlendTargetDesc {
                enable: s(27) != 0,
                colour,
                alpha,
                write_mask: wgpu::ColorWrites::from_bits_truncate(s(key)),
            }),
            alpha_to_coverage: false,
        });
        let raster = self.device.create_rasterizer(n::RasterizerDesc {
            cull: match s(22) {
                2 => Some(wgpu::Face::Front),
                3 => Some(wgpu::Face::Back),
                _ => None,
            },
            fill: match s(8) {
                1 => wgpu::PolygonMode::Point,
                2 => wgpu::PolygonMode::Line,
                _ => wgpu::PolygonMode::Fill,
            },
            scissor_enable: s(174) != 0,
            ..Default::default()
        });
        let face = |func, fail, zfail, pass| wgpu::StencilFaceState {
            compare: crate::compare(s(func)),
            fail_op: crate::stencil_op(s(fail)),
            depth_fail_op: crate::stencil_op(s(zfail)),
            pass_op: crate::stencil_op(s(pass)),
        };
        let front = face(56, 53, 54, 55);
        let depth = self.device.create_depth_stencil(n::DepthStencilDesc {
            depth_enable: s(7) != 0,
            depth_write: s(14) != 0,
            depth_func: crate::compare(s(23)),
            stencil_enable: s(52) != 0,
            front,
            back: if s(185) != 0 {
                face(189, 186, 187, 188)
            } else {
                front
            },
            read_mask: s(58),
            write_mask: s(59),
        });
        entries.push(FixedEntry {
            values: state.render_states.clone(),
            blend,
            raster,
            depth,
        });
        Ok((blend, raster, depth))
    }
    fn input(
        &mut self,
        d: &NativeDraw,
        vs: n::Shader,
        iface: &d3d9wgsl::Interface,
    ) -> Result<(n::InputLayout, Arc<[u32]>), String> {
        let mut strides = [0; 4];
        for stream in d.streams.iter() {
            strides[stream.stream as usize] = stream.stride;
        }
        let key = (vs, d.declaration.as_ptr() as usize, strides);
        if let Some(e) = self.inputs.get(&key) {
            return Ok((e.handle, e.streams.clone()));
        }
        let mut used: Vec<u32> = vec![];
        let mut elements = vec![];
        for (location, &(usage, index, _)) in iface.inputs.iter().enumerate() {
            let e = d
                .declaration
                .iter()
                .find(|e| e[6] as u32 == usage && e[7] as u32 == index)
                .ok_or("native input semantic missing")?;
            if e[5] != 0 {
                return Err("native nondefault declaration method".into());
            }
            let stream = u16::from_le_bytes([e[0], e[1]]) as u32;
            let (format, _) = crate::vertex_format(e[4]).ok_or("native vertex format unsupported")?;
            let offset = u16::from_le_bytes([e[2], e[3]]) as u64;
            used.push(stream);
            elements.push(n::InputElement {
                stream,
                offset,
                format,
                semantic: (usage, index),
                location: location as u32,
            });
        }
        used.sort_unstable();
        used.dedup();
        if used.is_empty() || used.len() > 4 {
            return Err("native requires one to four input streams".into());
        }
        let mut streams = vec![];
        for &stream in &used {
            let binding = d
                .streams
                .iter()
                .find(|s| s.stream == stream)
                .ok_or("native stream missing")?;
            if binding.stride == 0 || binding.stride % 4 != 0 {
                return Err("native stream stride unsupported".into());
            }
            streams.push(n::InputStream {
                stride: binding.stride as u64,
                step: wgpu::VertexStepMode::Vertex,
            });
        }
        for e in &mut elements {
            let dense = used.iter().position(|&s| s == e.stream).unwrap();
            if e.offset + e.format.size() > streams[dense].stride {
                return Err("native vertex element exceeds stride".into());
            }
            e.stream = dense as u32;
        }
        let handle = self
            .device
            .create_input_layout(n::InputLayoutDesc { streams, elements })?;
        self.inputs.insert(
            key,
            InputEntry {
                _declaration: d.declaration.clone(),
                handle,
                streams: used.clone().into(),
            },
        );
        Ok((handle, used.into()))
    }
    fn textures(
        &mut self,
        backend: &mut WgpuBackend,
        pass: &NativePass,
        d: &NativeDraw,
        pipeline: n::Pipeline,
        stage: n::Stage,
        iface: &d3d9wgsl::Interface,
    ) -> Result<n::TextureSet, String> {
        let base = if stage == n::Stage::Pixel { 0 } else { 257 };
        let mut bindings = vec![];
        for &(slot, kind) in &iface.samplers {
            let t = d
                .textures
                .iter()
                .find(|t| t.sampler == base + slot)
                .ok_or("native sampler not covered")?;
            let attached = attached(pass, t.resource);
            if !f32::from_bits(t.states[8]).is_finite()
                || (stage == n::Stage::Vertex && f32::from_bits(t.states[8]) != 0.0)
            {
                return Err("native sampler LOD bias unsupported".into());
            }
            let expected = match kind {
                d3d9wgsl::TextureKind::Flat => TexKind::Flat,
                d3d9wgsl::TextureKind::Cube => TexKind::Cube,
                d3d9wgsl::TextureKind::Volume => TexKind::Volume,
            };
            if t.kind != expected {
                return Err("native texture dimensionality mismatch".into());
            }
            let tx = backend
                .texture_for(t)
                .ok_or("native sampled texture unavailable")?;
            let srgb = t.states[11] != 0;
            let gpu_format = tx.gpu_format;
            // a texture the draw renders into is sampled from its snapshot
            let source = if attached {
                let src = tx.texture.clone();
                self.snapshot(&backend.device, &src)
            } else {
                tx.texture.clone()
            };
            let key = (source.clone(), srgb);
            let view = if let Some(&view) = self.sampled.get(&key) {
                view
            } else {
                let format = if srgb {
                    gpu_format.add_srgb_suffix()
                } else {
                    gpu_format
                };
                let desc = wgpu::TextureViewDescriptor {
                    format: Some(format),
                    dimension: Some(match expected {
                        TexKind::Flat => wgpu::TextureViewDimension::D2,
                        TexKind::Cube => wgpu::TextureViewDimension::Cube,
                        TexKind::Volume => wgpu::TextureViewDimension::D3,
                    }),
                    ..Default::default()
                };
                let view = source.create_view(&desc);
                let view = self.device.retain_view(view, format, [t.width, t.height], 1, desc);
                self.sampled.insert(key, view);
                view
            };
            let sampler = if let Some(&s) = self.samplers.get(&t.states) {
                s
            } else {
                let gpu = backend.sampler(&t.states)?;
                let raw = backend.vk.as_ref().and_then(|vk| vk.samplers.get(&gpu)).cloned();
                let s = self.device.retain_sampler(gpu, f32::from_bits(t.states[8]), raw);
                self.samplers.insert(t.states, s);
                s
            };
            bindings.push(n::TextureBinding { slot, view, sampler });
        }
        self.device.create_texture_set(pipeline, stage, &bindings)
    }
    fn prepare(
        &mut self,
        backend: &mut WgpuBackend,
        job: &NativeJob,
        overrides: &[ComparePair],
    ) -> Result<Vec<PreparedPass>, String> {
        let mut out = vec![];
        for pass in &job.passes {
            let target = pass
                .colours
                .iter()
                .flatten()
                .next()
                .or(pass.depth.as_ref())
                .ok_or("native pass has no attachment")?;
            if target.width == 0 || target.height == 0 {
                return Err("native empty target".into());
            }
            if pass
                .colours
                .iter()
                .chain([&pass.depth])
                .flatten()
                .any(|t| t.width != target.width || t.height != target.height)
            {
                return Err("native attachment extent mismatch".into());
            }
            let mut colours = [None; 4];
            let mut formats = [None; 4];
            let srgb = pass
                .draws
                .first()
                .is_some_and(|d| state_value(&d.state, 194) != 0);
            for (slot, t) in pass.colours.iter().enumerate() {
                if let Some(t) = t {
                    let (v, f) = self.target(backend, t, srgb && slot == 0, overrides)?;
                    colours[slot] = Some(v);
                    formats[slot] = Some(f);
                }
            }
            let depth = pass
                .depth
                .as_ref()
                .map(|t| self.target(backend, t, false, overrides))
                .transpose()?;
            if pass.clear_colour.is_some() && formats.iter().flatten().any(|f| Some(*f) != formats[0]) {
                return Err("native heterogeneous colour clear requires separate passes".into());
            }
            let mut prepared = PreparedPass {
                desc: n::PassDesc {
                    colours,
                    depth: depth.map(|x| x.0),
                    clear_colour: pass.clear_colour.map(|c| {
                        crate::clear_colour(c, formats[0].unwrap_or(wgpu::TextureFormat::Rgba8Unorm))
                    }),
                    clear_depth: pass.clear_depth.map(f32::from_bits),
                    clear_stencil: pass.clear_stencil,
                },
                draws: vec![],
            };
            if pass
                .clear_depth
                .is_some_and(|d| !f32::from_bits(d).is_finite() || !(0.0..=1.0).contains(&f32::from_bits(d)))
            {
                return Err("native invalid depth clear".into());
            }
            for d in &pass.draws {
                validate_draw(d, target)?;
                if (state_value(&d.state, 194) != 0) != srgb {
                    return Err("native sRGB write changes within pass".into());
                }
                let (vs, vi) = self.shader(&d.vertex_shader, n::Stage::Vertex)?;
                let (ps, pi) = self.shader(&d.pixel_shader, n::Stage::Pixel)?;
                self.shaders[&(d.vertex_shader.as_ptr() as usize, n::Stage::Vertex)]
                    .reads
                    .validate(&d.vs_constants)?;
                self.shaders[&(d.pixel_shader.as_ptr() as usize, n::Stage::Pixel)]
                    .reads
                    .validate(&d.ps_constants)?;
                if pass
                    .colours
                    .iter()
                    .enumerate()
                    .any(|(i, t)| i != 0 && t.is_some() && !pi.outputs.contains(&(i as u32)))
                {
                    return Err("native pass includes unwritten colour attachment".into());
                }
                let (blend, rasterizer, depth_stencil) = self.fixed(&d.state)?;
                let (input, streams) = self.input(d, vs, &vi)?;
                let topology = match d.primitive {
                    1 => wgpu::PrimitiveTopology::PointList,
                    2 => wgpu::PrimitiveTopology::LineList,
                    3 => wgpu::PrimitiveTopology::LineStrip,
                    4 => wgpu::PrimitiveTopology::TriangleList,
                    5 => wgpu::PrimitiveTopology::TriangleStrip,
                    _ => return Err("native triangle fan unsupported".into()),
                };
                let strip_index = native_strip_index(
                    d.primitive,
                    d.indices.map(|i| i.format),
                    d.min_index,
                    d.vertex_count,
                )?;
                let pipeline = self.device.create_pipeline(n::PipelineDesc {
                    vertex_shader: vs,
                    pixel_shader: ps,
                    input,
                    blend,
                    rasterizer,
                    depth_stencil,
                    colours: formats,
                    depth: depth.map(|x| x.1),
                    samples: 1,
                    sample_mask: !0,
                    topology,
                    strip_index,
                    sampler_ones: sampler_ones(d),
                    depth_bias: depth_bias(d, pass),
                })?;
                // Texture allocations only change after release, which drops
                // this worker. Retained Arc identities cannot be recycled.
                // textures sampled while rendered into: copied before the draw
                let mut copies = vec![];
                for t in d.textures.iter() {
                    let sampled = pi.samplers.iter().any(|&(s, _)| s == t.sampler)
                        || vi.samplers.iter().any(|&(s, _)| 257 + s == t.sampler);
                    if sampled && attached(pass, t.resource) {
                        let src = backend
                            .texture_for(t)
                            .ok_or("native sampled texture unavailable")?
                            .texture
                            .clone();
                        // depth-stencil textures cannot be copied whole
                        if src.format().is_depth_stencil_format() {
                            return Err("native sampled depth attachment feedback unsupported".into());
                        }
                        let snapshot = self.snapshot(&backend.device, &src);
                        copies.push((src, snapshot));
                    }
                }
                let set_key = (pipeline, d.textures.as_ptr() as usize);
                let sets = if let Some((_, sets)) = self.texture_sets.get(&set_key) {
                    *sets
                } else {
                    let sets = [
                        self.textures(backend, pass, d, pipeline, n::Stage::Pixel, &pi)?,
                        self.textures(backend, pass, d, pipeline, n::Stage::Vertex, &vi)?,
                    ];
                    self.texture_sets.insert(set_key, (d.textures.clone(), sets));
                    sets
                };
                let count = primitive_vertices(d.primitive, d.primitives)?;
                let end = d.start.checked_add(count).ok_or("native draw range overflow")?;
                let mut vertices = vec![];
                for &stream in streams.iter() {
                    let s = d.streams.iter().find(|s| s.stream == stream).unwrap();
                    let buffer = self.buffer(backend, s.resource, s.offset)?;
                    if d.indices.is_none() && (end as u64 * s.stride as u64) > buffer.size {
                        return Err("native vertex draw exceeds buffer".into());
                    }
                    vertices.push(buffer);
                }
                let indices = d
                    .indices
                    .map(|i| -> Result<_, String> {
                        let format = match i.format {
                            101 => wgpu::IndexFormat::Uint16,
                            102 => wgpu::IndexFormat::Uint32,
                            _ => return Err("native index format unsupported".into()),
                        };
                        let slice = self.buffer(backend, i.resource, 0)?;
                        if end as u64 * format.byte_size() as u64 > slice.size {
                            return Err("native index draw exceeds buffer".into());
                        }
                        Ok((slice, format))
                    })
                    .transpose()?;
                prepared.draws.push(PreparedDraw {
                    copies,
                    pipeline,
                    shaders: [ps, vs],
                    sets,
                    vertices,
                    indices,
                    range: d.start..end,
                });
            }
            out.push(prepared);
        }
        Ok(out)
    }
    fn run(
        &mut self,
        backend: &mut WgpuBackend,
        job: &NativeJob,
        overrides: &[ComparePair],
    ) -> Result<(), String> {
        // No render commands or clears until every resource/state is prepared.
        let prepared = self.prepare(backend, job, overrides)?;
        let draws = job.passes.iter().map(|p| p.draws.len() as u64).sum::<u64>();
        let bytes = draws
            .checked_mul(2 * 4864)
            .and_then(|n| n.checked_add(16384))
            .ok_or("native job too large")?;
        if bytes > backend.device.limits().max_buffer_size || bytes > u32::MAX as u64 {
            return Err("native job exceeds constant arena limit".into());
        }
        let mut recorder = match self.recording.take() {
            Some(state) if state.uniform_capacity() >= bytes => n::Recorder::from_state(&self.device, state),
            _ => {
                self.constants.clear();
                self.applied.clear();
                let resident_limit =
                    ((backend.device.limits().max_storage_buffer_binding_size - 16) * 4 / 5) / 256 * 256;
                let resident = bytes.next_multiple_of(256) <= resident_limit;
                let capacity = if resident {
                    bytes.next_power_of_two().min(resident_limit)
                } else {
                    bytes
                };
                let mut recorder = n::Recorder::with_resident_constants(&self.device, capacity, resident);
                // Host opt-in: exact repeated job sequences benefit; alternating
                // sequences can rebuild bundles, so do not assume a game win.
                recorder.enable_bundles(std::env::var_os("DDGPU_NATIVE_BUNDLES").is_some_and(|s| s == "1"));
                recorder
            }
        };
        let before = recorder.stats;
        let applied = &mut self.applied;
        let constants = &mut self.constants;
        for p in &prepared {
            for d in &p.draws {
                for shader in d.shaders {
                    constants
                        .entry(shader)
                        .or_insert_with(|| recorder.create_constant_buffer_for_shader(shader));
                }
            }
        }
        for (pass, prepared) in job.passes.iter().zip(&prepared) {
            recorder.begin_pass(prepared.desc)?;
            for (d, p) in pass.draws.iter().zip(&prepared.draws) {
                if !p.copies.is_empty() {
                    // as the D3D9 path: what the pass drew so far, copied,
                    // then the pass goes on loading its attachments
                    for (src, dst) in &p.copies {
                        recorder.copy_texture(src, dst);
                    }
                    recorder.begin_pass(n::PassDesc {
                        clear_colour: None,
                        clear_depth: None,
                        clear_stencil: None,
                        ..prepared.desc
                    })?;
                }
                recorder.set_pipeline(p.pipeline);
                for set in p.sets {
                    recorder.set_texture_set(set);
                }
                for (slot, &v) in p.vertices.iter().enumerate() {
                    recorder.set_vertex_buffer(slot as u32, v);
                }
                if let Some((buffer, format)) = p.indices {
                    recorder.set_index_buffer(buffer, format);
                }
                for (stage, updates) in [&d.ps_constants, &d.vs_constants].into_iter().enumerate() {
                    let shader = p.shaders[stage];
                    let c = constants[&shader];
                    if !applied.get(&c).is_some_and(|last| same_arrays(last, updates)) {
                        apply_constants(
                            &mut recorder,
                            c,
                            self.device.shader_constant_layout(shader),
                            updates,
                        )?;
                        applied.insert(c, updates.clone());
                    }
                    recorder.set_constants(c);
                }
                if depth_bias(d, pass) {
                    // PS extra slot 5, as the D3D9 path's ps_extra[5]
                    let shader = p.shaders[0];
                    recorder.update_constant_words(
                        constants[&shader],
                        self.device.shader_constant_layout(shader).extra_offset + 5 * 16,
                        &[state_value(&d.state, 195), state_value(&d.state, 175), 0, 0],
                    )?;
                }
                let v = d.viewport;
                let extra = [1.0 / v[2] as f32, -1.0 / v[3] as f32, 0.0, 0.0].map(f32::to_bits);
                let shader = p.shaders[1];
                recorder.update_constant_words(
                    constants[&shader],
                    self.device.shader_constant_layout(shader).extra_offset,
                    &extra,
                )?;
                recorder.set_viewport([
                    v[0] as f32,
                    v[1] as f32,
                    v[2] as f32,
                    v[3] as f32,
                    f32::from_bits(v[4]),
                    f32::from_bits(v[5]),
                ]);
                if state_value(&d.state, 174) != 0 {
                    if let Some([l, t, r, b]) = d.scissor {
                        recorder.set_scissor([l as u32, t as u32, (r - l) as u32, (b - t) as u32]);
                    }
                }
                recorder.set_stencil_reference(state_value(&d.state, 57));
                let colour =
                    crate::clear_colour(state_value(&d.state, 193), wgpu::TextureFormat::Rgba32Float);
                recorder.set_blend_constant([colour.r, colour.g, colour.b, colour.a]);
                if p.indices.is_some() {
                    recorder.draw_indexed(p.range.clone(), d.base_vertex, 0..1)?;
                } else {
                    recorder.draw(p.range.clone(), 0..1)?;
                }
            }
            recorder.end_pass();
        }
        // L124 finisher owns the expensive encoder finish. Queue writes may
        // only follow submission of the batches that read what they write:
        // with resident constants that is only the batch that last used the
        // next scatter source (L141); otherwise every earlier batch.
        match recorder.upload_gate() {
            Some(ticket) => backend.wait_finisher_until(ticket),
            None => backend.wait_finisher(),
        }
        let (scatter, mut chunks) = recorder.finish_chunks();
        let after = recorder.stats;
        self.recording = Some(recorder.into_state());
        backend.native_stats.upload_bytes += after.upload_bytes - before.upload_bytes;
        backend.native_stats.dirty_registers += after.dirty_registers - before.dirty_registers;
        backend.native_stats.bundle_builds += after.bundle_builds - before.bundle_builds;
        backend.native_stats.bundle_hits += after.bundle_hits - before.bundle_hits;
        if let Some(scatter) = scatter {
            // The ordered finisher submits the scatter and its consumers in
            // one batch. This removes a queue submit per dirty job without
            // changing the wait-before-write or scatter-before-draw boundary.
            chunks.insert(0, crate::vk::Chunk::Buffer(scatter));
        }
        if !chunks.is_empty() {
            backend.native_stats.submit_batches += 1;
            if let Some(finisher) = &backend.finisher {
                finisher.submit_batch(chunks);
            } else {
                crate::vk::submit(&backend.queue, chunks.into_iter().flat_map(crate::vk::finish).collect());
            }
            backend.submissions.set(backend.submissions.get() + 1);
        }
        // the scatter source this job wrote is free once its batch is in
        let ticket = backend.finisher_ticket();
        if let Some(state) = self.recording.as_mut() {
            state.tag_upload(ticket);
        }
        Ok(())
    }
}
/// The channels render-target textures sampled by pixel samplers lack (4
/// bits a sampler), as the D3D9 path's sampler_ones.
fn sampler_ones(d: &NativeDraw) -> u64 {
    d.textures
        .iter()
        .filter(|t| t.sampler < 16 && t.usage & (USAGE_RENDERTARGET | USAGE_DEPTHSTENCIL) != 0)
        .fold(0u64, |m, t| m | (crate::missing_channels(t.format) as u64) << (4 * t.sampler))
}
/// D3D9's depth bias applies (the D3D9 path's condition): a depth target,
/// ZENABLE, and DEPTHBIAS or SLOPESCALEDEPTHBIAS set.
fn depth_bias(d: &NativeDraw, pass: &NativePass) -> bool {
    pass.depth.is_some()
        && state_value(&d.state, 7) != 0
        && (state_value(&d.state, 195) != 0 || state_value(&d.state, 175) != 0)
}
/// Whether the pass renders into texture `resource`.
fn attached(pass: &NativePass, resource: u32) -> bool {
    pass.colours
        .iter()
        .chain([&pass.depth])
        .flatten()
        .any(|t| t.texture.is_some_and(|x| x.resource == resource))
}
fn state_value(state: &NativeFixedState, key: u32) -> u32 {
    state
        .render_states
        .binary_search_by_key(&key, |x| x.0)
        .ok()
        .map_or(0, |i| state.render_states[i].1)
}
/// D3D9 requires every used index to be inside the declared MinVertexIndex /
/// NumVertices interval, before adding BaseVertexIndex. When that interval
/// excludes the fixed restart sentinel, wgpu's indexed strip is equivalent.
/// Keep refusing zero/unproven/sentinel-containing intervals; do not assume
/// that real index contents avoid the sentinel when the bound allows it.
/// Source: Microsoft IDirect3DDevice9::DrawIndexedPrimitive, Remarks:
/// https://learn.microsoft.com/en-us/windows/win32/api/d3d9/nf-d3d9-idirect3ddevice9-drawindexedprimitive
fn native_strip_index(
    primitive: u32,
    format: Option<u32>,
    min_index: u32,
    vertex_count: u32,
) -> Result<Option<wgpu::IndexFormat>, String> {
    let Some(format) = format.filter(|_| matches!(primitive, 3 | 5)) else {
        return Ok(None);
    };
    // This change covers the triangle strips observed in cap1. Indexed lines
    // remain outside the validated native subset.
    if primitive == 3 {
        return Err("native indexed line strip unsupported".into());
    }
    // As the D3D9 path (crates/ddgpu derive): the strip's index format on
    // the pipeline whatever DrawIndexedPrimitive's MinIndex/NumVertices hint
    // says (L141: refusing on the hint, 750 jobs in c24nat, drew the same
    // strips through the D3D9 path anyway). wgpu restarts a strip at index
    // 0xFFFF (0xFFFFFFFF) where D3D9 does not: a limit both paths share.
    let _ = (min_index, vertex_count);
    match format {
        101 => Ok(Some(wgpu::IndexFormat::Uint16)),
        102 => Ok(Some(wgpu::IndexFormat::Uint32)),
        _ => Err("native index format unsupported".into()),
    }
}
fn validate_fixed(state: &NativeFixedState) -> Result<(), String> {
    if state.render_states.windows(2).any(|p| p[0].0 >= p[1].0) {
        return Err("native states must be sorted and unique".into());
    }
    // DEPTHBIAS (195) and SLOPESCALEDEPTHBIAS (175): in the pixel shader
    // as the D3D9 path does (PipelineDesc::depth_bias)
    for key in [15, 176, 152] {
        if state_value(state, key) != 0 {
            return Err(format!("native state {key} unsupported"));
        }
    }
    Ok(())
}
fn validate_draw(d: &NativeDraw, target: &Target) -> Result<(), String> {
    if d.instances != 1 {
        return Err("native instancing deferred".into());
    }
    if d.streams.len() > 4
        || d.streams.iter().enumerate().any(|(i, s)| {
            s.stream >= 4 || s.offset % 4 != 0 || d.streams[..i].iter().any(|p| p.stream == s.stream)
        })
    {
        return Err("native invalid/duplicate stream".into());
    }
    if d.textures
        .iter()
        .enumerate()
        .any(|(i, t)| d.textures[..i].iter().any(|p| p.sampler == t.sampler))
    {
        return Err("native duplicate sampler".into());
    }
    let v = d.viewport;
    if v[2] == 0
        || v[3] == 0
        || v[0].checked_add(v[2]).is_none_or(|x| x > target.width)
        || v[1].checked_add(v[3]).is_none_or(|y| y > target.height)
        || ![v[4], v[5]]
            .iter()
            .all(|&x| f32::from_bits(x).is_finite() && (0.0..=1.0).contains(&f32::from_bits(x)))
        || f32::from_bits(v[4]) > f32::from_bits(v[5])
    {
        return Err("native invalid viewport".into());
    }
    if state_value(&d.state, 174) != 0 {
        let [l, t, r, b] = d.scissor.ok_or("native enabled scissor missing")?;
        if l < 0 || t < 0 || r < l || b < t || r as u32 > target.width || b as u32 > target.height {
            return Err("native invalid scissor".into());
        }
    }
    Ok(())
}
fn primitive_vertices(kind: u32, primitives: u32) -> Result<u32, String> {
    if primitives == 0 {
        return Ok(0);
    }
    match kind {
        1 => Some(primitives),
        2 => primitives.checked_mul(2),
        3 => primitives.checked_add(1),
        4 => primitives.checked_mul(3),
        5 => primitives.checked_add(2),
        _ => None,
    }
    .ok_or("native primitive range unsupported/overflow".into())
}
/// The same register arrays (by pointer: they are immutable and held) at
/// the same registers: applying `b` after `a` changes nothing.
fn same_arrays(a: &NativeConstants, b: &NativeConstants) -> bool {
    a.floats.len() == b.floats.len()
        && a.ints.len() == b.ints.len()
        && a.bools.len() == b.bools.len()
        && a.floats.iter().zip(&b.floats).all(|(x, y)| x.first == y.first && Arc::ptr_eq(&x.registers, &y.registers))
        && a.ints.iter().zip(&b.ints).all(|(x, y)| x.first == y.first && Arc::ptr_eq(&x.registers, &y.registers))
        && a.bools.iter().zip(&b.bools).all(|(x, y)| x.first == y.first && Arc::ptr_eq(&x.registers, &y.registers))
}
fn apply_constants(
    recorder: &mut n::Recorder<'_>,
    buffer: n::ConstantBuffer,
    layout: n::ConstantLayout,
    updates: &NativeConstants,
) -> Result<(), String> {
    for write in &updates.floats {
        let first = (write.first as usize).min(layout.floats);
        let count = write.registers.len().min(layout.floats - first);
        if count != 0 {
            recorder.update_constant_words(buffer, first * 16, write.registers[..count].as_flattened())?;
        }
    }
    for write in &updates.ints {
        for (k, r) in write.registers.iter().enumerate() {
            recorder.update_constant_words(
                buffer,
                layout.int_offset + (write.first as usize + k) * 16,
                &r.map(|x| x as u32),
            )?;
        }
    }
    for write in &updates.bools {
        for (index, &value) in write.registers.iter().enumerate() {
            recorder.update_constant_words(
                buffer,
                layout.bool_offset + (write.first as usize + index) * 16,
                &[value],
            )?;
        }
    }
    Ok(())
}
impl ConstantReads {
    fn from_shader(shader: &d3d9shader::Shader) -> Result<Self, String> {
        use d3d9shader::{op, reg, Stage};
        let mut r = Self {
            floats: vec![false; if shader.stage == Stage::Vertex { 256 } else { 224 }],
            ..Default::default()
        };
        for ins in &shader.instructions {
            for (index, src) in ins.src.iter().enumerate() {
                let bank: &mut [bool] = match src.kind {
                    reg::CONST => &mut r.floats,
                    reg::CONSTINT => &mut r.ints,
                    reg::CONSTBOOL => &mut r.bools,
                    reg::CONST2 | reg::CONST3 | reg::CONST4 => {
                        return Err("native extended constant banks unsupported".into())
                    }
                    _ => continue,
                };
                if src.relative.is_some() {
                    bank.fill(true);
                    continue;
                }
                let rows = if index == 1 {
                    match ins.opcode {
                        op::M4X4 | op::M3X4 => 4,
                        op::M4X3 | op::M3X3 => 3,
                        op::M3X2 => 2,
                        _ => 1,
                    }
                } else {
                    1
                };
                let first = src.index as usize;
                bank.get_mut(first..first + rows)
                    .ok_or("native shader constant outside bank")?
                    .fill(true);
            }
        }
        for ins in &shader.instructions {
            if let Some(dst) = &ins.dst {
                let bank: &mut [bool] = match ins.opcode {
                    op::DEF => &mut r.floats,
                    op::DEFI => &mut r.ints,
                    op::DEFB => &mut r.bools,
                    _ => continue,
                };
                if let Some(needed) = bank.get_mut(dst.index as usize) {
                    *needed = false;
                }
            }
        }
        Ok(r)
    }
    fn validate(&self, updates: &NativeConstants) -> Result<(), String> {
        fn covered(required: &[bool], ranges: impl Iterator<Item = (u32, usize)>) -> Result<(), String> {
            let mut storage = [false; 256];
            let written = &mut storage[..required.len()];
            for (first, len) in ranges {
                let first = first as usize;
                let end = first.checked_add(len).ok_or("native constant range overflow")?;
                written
                    .get_mut(first..end)
                    .ok_or("native constant range outside bank")?
                    .fill(true);
            }
            if required
                .iter()
                .zip(written.iter())
                .any(|(&needed, &have)| needed && !have)
            {
                return Err("native constants do not cover shader reads".into());
            }
            Ok(())
        }
        covered(
            &self.floats,
            updates.floats.iter().map(|w| (w.first, w.registers.len())),
        )?;
        covered(
            &self.ints,
            updates.ints.iter().map(|w| (w.first, w.registers.len())),
        )?;
        covered(
            &self.bools,
            updates.bools.iter().map(|w| (w.first, w.registers.len())),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strip_restart_exclusion_uses_declared_index_range_without_wrapping() {
        for primitive in [5] {
            assert_eq!(
                native_strip_index(primitive, Some(101), 0, 65535).unwrap(),
                Some(wgpu::IndexFormat::Uint16)
            );
            assert!(native_strip_index(primitive, Some(101), 0, 65536).is_err());
            assert!(native_strip_index(primitive, Some(101), 65534, 1).is_ok());
            assert!(native_strip_index(primitive, Some(101), 65535, 1).is_err());
            assert!(native_strip_index(primitive, Some(101), 0, 0).is_err());
            assert_eq!(
                native_strip_index(primitive, Some(102), 0, u32::MAX).unwrap(),
                Some(wgpu::IndexFormat::Uint32)
            );
            assert!(native_strip_index(primitive, Some(102), u32::MAX - 1, 1).is_ok());
            assert!(native_strip_index(primitive, Some(102), u32::MAX - 1, 2).is_err());
            assert!(native_strip_index(primitive, Some(102), u32::MAX, u32::MAX).is_err());
            assert!(native_strip_index(primitive, Some(100), 0, 3).is_err());
            assert_eq!(native_strip_index(primitive, None, 0, 0).unwrap(), None);
        }
        assert!(native_strip_index(3, Some(101), 0, 3).is_err());
        assert_eq!(native_strip_index(4, Some(101), 0, 65536).unwrap(), None);
    }
    #[test]
    fn refusal_histogram_counts_exact_reasons_and_bounds_distinct_storage() {
        let mut stats = NativeJobStats::default();
        stats.reject("indexed strip", 2, 3);
        stats.reject("indexed strip", 1, 4);
        stats.reject("occlusion query active", 1, 5);
        assert_eq!(stats.rejected, 3);
        assert_eq!(stats.rejected_passes, 4);
        assert_eq!(stats.rejected_draws, 12);
        assert_eq!(stats.rejection_reasons["indexed strip"], 2);
        for i in 0..100 {
            stats.reject(&format!("reason {i}"), 0, 0);
        }
        assert_eq!(stats.rejection_reasons.len(), 65);
        assert_eq!(stats.rejection_reasons.values().sum::<u64>(), stats.rejected);
    }
    #[test]
    fn coverage_rejects_missing_out_of_range_and_accepts_def_constants() {
        let mut shader =
            d3d9shader::parse(&[0xffff0300, 0x02000001, 0x800f0800, 0xa0e40003, 0xffff]).unwrap();
        let required = ConstantReads::from_shader(&shader).unwrap();
        assert!(required.validate(&NativeConstants::default()).is_err());
        let mut constants = NativeConstants {
            floats: vec![NativeFloatWrite {
                first: 3,
                registers: vec![[0; 4]].into(),
            }],
            ..Default::default()
        };
        assert!(required.validate(&constants).is_ok());
        constants.floats[0].first = 224;
        assert!(required.validate(&constants).is_err());
        let mut def = shader.instructions[0].clone();
        def.opcode = d3d9shader::op::DEF;
        def.dst = Some(def.src[0].clone());
        def.src.clear();
        def.data = vec![0; 4];
        shader.instructions.insert(0, def);
        assert!(ConstantReads::from_shader(&shader)
            .unwrap()
            .validate(&NativeConstants::default())
            .is_ok());
    }
    #[test]
    fn coverage_includes_implicit_matrix_rows_and_relative_bank() {
        let mut shader =
            d3d9shader::parse(&[0xfffe0300, 0x02000001, 0xe00f0000, 0xa0e40000, 0xffff]).unwrap();
        let ins = &mut shader.instructions[0];
        ins.opcode = d3d9shader::op::M4X4;
        let mut row = ins.src[0].clone();
        row.index = 4;
        ins.src.push(row);
        let required = ConstantReads::from_shader(&shader).unwrap();
        assert!(required.floats[0]);
        assert!(required.floats[4..8].iter().all(|&r| r));
        assert!(!required.floats[8]);
        let mut address = shader.instructions[0].src[0].clone();
        address.kind = d3d9shader::reg::ADDR;
        shader.instructions[0].src[0].relative = Some(Box::new(address));
        assert!(ConstantReads::from_shader(&shader)
            .unwrap()
            .floats
            .iter()
            .all(|&r| r));
    }
}
