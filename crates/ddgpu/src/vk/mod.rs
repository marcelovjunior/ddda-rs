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
pub(crate) use objects::{Context, Pipeline};
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
    enabled_value(std::env::var("DDGPU_VK").ok().as_deref())
}

// L159 staged adoption: merge this default change only after the head's
// same-run gameplay image gate. Explicit 0 keeps the wgpu oracle available.
fn enabled_value(value: Option<&str>) -> bool {
    matches!(value, None | Some("1"))
}

/// Owned chunks accepted by L137's ordered encoder pool.
pub(crate) enum Chunk {
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
        Chunk::Wgpu(encoder) => vec![Ready {
            commands: crate::encode::finish_wgpu(encoder),
            keepalive: vec![],
            checks: vec![],
        }],
        Chunk::Vulkan(device, passes, _) => Pass::finish_batch(&device, passes),
    }
}

pub(crate) fn submit(queue: &wgpu::Queue, ready: Vec<Ready>) {
    let mut combined = Ready {
        commands: vec![],
        keepalive: vec![],
        checks: vec![],
    };
    for r in ready {
        combined.commands.extend(r.commands);
        combined.keepalive.extend(r.keepalive);
        combined.checks.extend(r.checks);
    }
    combined.submit(queue);
}

#[cfg(test)]
mod selection_tests {
    #[test]
    fn defaults_on_with_explicit_wgpu_escape_hatch() {
        assert!(super::enabled_value(None));
        assert!(super::enabled_value(Some("1")));
        for value in ["0", "", "false", "invalid", "2"] {
            assert!(!super::enabled_value(Some(value)));
        }
    }
}
