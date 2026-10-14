//! L138: owned Vulkan objects and pass recording, using wgpu-hal's Vulkan
//! implementation (ash) and the same Naga compiler as the wgpu oracle.
//!
//! No raw queue submission. Each run of raw passes is preceded by a separate
//! wgpu transition command buffer. HAL barriers handle internal transitions
//! and restore the layouts known to wgpu. The submitted batch retains every
//! object until wgpu's completion callback runs.

mod objects;
mod pass;
mod mapped;
mod shadow;
pub use shadow::{shadow_stats, ShadowStats};
pub(crate) use mapped::MappedUniforms;
pub(crate) use objects::{Context, Pipeline, PipelineTemplate, View, Object};
pub(crate) use pass::{Bindings, Draw, Pass, Ready};

use std::sync::atomic::{AtomicU64, Ordering};

static PASSES: AtomicU64 = AtomicU64::new(0);
static DRAWS: AtomicU64 = AtomicU64::new(0);
static FALLBACK: AtomicU64 = AtomicU64::new(0);
static QUERY_FALLBACK: AtomicU64 = AtomicU64::new(0);
static RESOURCE_FALLBACK: AtomicU64 = AtomicU64::new(0);
static INITIALIZE: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);
static LIVE_VIEWS: AtomicU64 = AtomicU64::new(0);
static MAPPED_RINGS: AtomicU64 = AtomicU64::new(0);
static MAPPED_RESTARTS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub passes: u64,
    pub draws: u64,
    pub fallback_passes: u64,
    pub initialization_passes: u64,
    pub query_passes: u64,
    pub resource_fallback_passes: u64,
    pub encode_errors: u64,
    pub live_views: u64,
    pub mapped_rings: u64,
    pub mapped_restarts: u64,
}

pub fn stats() -> Stats {
    Stats {
        passes: PASSES.load(Ordering::Relaxed),
        draws: DRAWS.load(Ordering::Relaxed),
        fallback_passes: FALLBACK.load(Ordering::Relaxed),
        initialization_passes: INITIALIZE.load(Ordering::Relaxed),
        query_passes: QUERY_FALLBACK.load(Ordering::Relaxed),
        resource_fallback_passes: RESOURCE_FALLBACK.load(Ordering::Relaxed),
        encode_errors: ERRORS.load(Ordering::Relaxed),
        live_views: LIVE_VIEWS.load(Ordering::Relaxed),
        mapped_rings: MAPPED_RINGS.load(Ordering::Relaxed),
        mapped_restarts: MAPPED_RESTARTS.load(Ordering::Relaxed),
    }
}

pub(crate) fn unsupported_pass() {
    QUERY_FALLBACK.fetch_add(1, Ordering::Relaxed);
    FALLBACK.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn report() {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if last.is_none_or(|t| t.elapsed().as_secs() >= 2) {
        eprintln!("ddgpu-vk {:?}", stats());
        shadow::report();
        *last = Some(std::time::Instant::now());
    }
}

pub(crate) fn enabled() -> bool {
    std::env::var("DDGPU_VK").as_deref() == Ok("1")
}

/// Owned chunks accepted by L137's ordered encoder pool.
pub(crate) enum Chunk {
    /// Already finished upload/scatter, ordered before the rendering chunks.
    Buffer(wgpu::CommandBuffer),
    Wgpu(wgpu::CommandEncoder),
    Vulkan(wgpu::Device, Vec<Pass>, usize),
}

impl Chunk {
    pub fn push_pass(chunks: &mut Vec<Self>, device: &wgpu::Device, pass: Pass, max_draws: usize) {
        let count = pass.draws.len();
        if let Some(Self::Vulkan(_, passes, draws)) = chunks.last_mut().filter(|c| matches!(c, Self::Vulkan(_, passes,draws) if *draws < max_draws && passes.last().is_some_and(|p| std::sync::Arc::ptr_eq(&p.initialized,&pass.initialized)))) {
            *draws += count;
            passes.push(pass);
        } else {
            chunks.push(Self::Vulkan(device.clone(), vec![pass], count));
        }
    }
}

pub(crate) fn finish(chunk: Chunk) -> Vec<Ready> {
    match chunk {
        Chunk::Buffer(buffer) => vec![Ready { commands: vec![buffer], keepalive: vec![], checks: vec![] }],
        Chunk::Wgpu(encoder) => vec![Ready {
            commands: crate::encode::finish_wgpu(encoder),
            keepalive: vec![],
            checks: vec![],
        }],
        Chunk::Vulkan(device, passes, _) => Pass::finish_batch(&device, passes),
    }
}

pub(crate) fn submit(queue: &wgpu::Queue, ready: Vec<Ready>) -> wgpu::SubmissionIndex {
    let mut combined = Ready {
        commands: vec![],
        keepalive: vec![],
        checks: vec![],
    };
    if crate::gpu_ts::enabled() {
        let mut runs = vec![];
        for r in ready {
            let label = r.keepalive.iter().max_by_key(|p| p.extent.width as u64 * p.extent.height as u64)
                .map_or_else(|| "other".to_string(), |p| crate::gpu_ts::pass_label(p.extent, p.native_colors.iter().flatten().count(), p.native_depth.is_some()));
            combined.keepalive.extend(r.keepalive);
            combined.checks.extend(r.checks);
            runs.push((label, r.commands));
        }
        combined.commands = crate::gpu_ts::wrap(runs);
        return combined.submit(queue);
    }
    for r in ready {
        combined.commands.extend(r.commands);
        combined.keepalive.extend(r.keepalive);
        combined.checks.extend(r.checks);
    }
    combined.submit(queue)
}
