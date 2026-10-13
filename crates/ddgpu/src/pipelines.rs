//! What building shaders and pipelines costs, and a pipeline cache kept on
//! disk between runs (L71).
//!
//! A draw with a new combination of shaders and render states needs a new
//! render pipeline, and the driver compiles the shaders into it: while the
//! game loads, that compilation was most of the guest main thread's time
//! (profile_loadgame: `create_render_pipeline` under 74% of the samples).
//! The backend already builds each pipeline once per run (src/cache.rs);
//! the Vulkan pipeline cache keeps the driver's work across runs. It only
//! changes how long creating a pipeline takes: the pipeline is the same.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

static MODULES: AtomicU64 = AtomicU64::new(0);
static TRANSLATE_NS: AtomicU64 = AtomicU64::new(0);
static MODULE_NS: AtomicU64 = AtomicU64::new(0);
static PIPELINES: AtomicU64 = AtomicU64::new(0);
static PIPELINE_NS: AtomicU64 = AtomicU64::new(0);
static REUSED: AtomicU64 = AtomicU64::new(0);

/// Shader modules and render pipelines the backends of this process
/// created for draws, the time that took on the calling thread, and the
/// draws that found their pipeline built already.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// draw shader modules created, including background precreation
    pub modules: u64,
    pub module_time: Duration,
    /// Parsing/translating current bytecode to WGSL, including reflection
    /// for precreated modules; not necessarily a subset of `module_time`.
    pub translate_time: Duration,
    /// render pipelines created (the driver compiles the shaders)
    pub pipelines: u64,
    pub pipeline_time: Duration,
    pub reused: u64,
}

impl std::fmt::Display for PipelineStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mean = |t: Duration, n: u64| t.as_secs_f64() * 1e3 / n.max(1) as f64;
        write!(f, "shader modules {} ({:.0} ms, {:.2} ms each, {:.2} ms translating per module), pipelines {} ({:.0} ms, {:.2} ms each), draws reusing a pipeline {}",
               self.modules, self.module_time.as_secs_f64() * 1e3, mean(self.module_time, self.modules), mean(self.translate_time, self.modules),
               self.pipelines, self.pipeline_time.as_secs_f64() * 1e3, mean(self.pipeline_time, self.pipelines), self.reused)
    }
}

/// The counts so far.
pub fn pipeline_stats() -> PipelineStats {
    let ns = |a: &AtomicU64| Duration::from_nanos(a.load(Ordering::Relaxed));
    PipelineStats {
        modules: MODULES.load(Ordering::Relaxed),
        module_time: ns(&MODULE_NS),
        translate_time: ns(&TRANSLATE_NS),
        pipelines: PIPELINES.load(Ordering::Relaxed),
        pipeline_time: ns(&PIPELINE_NS),
        reused: REUSED.load(Ordering::Relaxed),
    }
}

pub(crate) fn translated(since: Instant) {
    TRANSLATE_NS.fetch_add(since.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

pub(crate) fn module_created(since: Instant) {
    MODULES.fetch_add(1, Ordering::Relaxed);
    MODULE_NS.fetch_add(since.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

pub(crate) fn pipeline_created(since: Instant) {
    PIPELINES.fetch_add(1, Ordering::Relaxed);
    PIPELINE_NS.fetch_add(since.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

pub(crate) fn pipeline_reused() {
    REUSED.fetch_add(1, Ordering::Relaxed);
}

/// Print [`pipeline_stats`] (cumulative) to stderr every `every`, on a
/// thread of its own.
pub fn report_pipeline_stats_every(every: Duration) {
    let start = Instant::now();
    let _ = std::thread::Builder::new().name("pipeline-stats".into()).spawn(move || loop {
        std::thread::sleep(every);
        eprintln!("pipelines at {:.0} s: {}", start.elapsed().as_secs_f64(), pipeline_stats());
    });
}

/// Saved at most this often while pipelines are being created.
const SAVE_INTERVAL: Duration = Duration::from_secs(20);

/// A pipeline cache backed by a file.
pub(crate) struct Persistent {
    pub cache: wgpu::PipelineCache,
    path: PathBuf,
    /// pipelines created through the cache since it was last saved
    unsaved: AtomicU64,
    last_save: Mutex<Instant>,
    saving: AtomicBool,
}

/// The caches open in this process ([`save_pipeline_caches`]).
static OPEN: Mutex<Vec<Arc<Persistent>>> = Mutex::new(Vec::new());

impl Persistent {
    /// The cache for this device in `dir` (one file per adapter and
    /// driver: wgpu::util::pipeline_cache_key), with what an earlier run
    /// saved there. None without the device feature (Vulkan only).
    pub fn open(device: &wgpu::Device, adapter: &wgpu::AdapterInfo, dir: &Path) -> Option<Arc<Persistent>> {
        if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return None;
        }
        let path = dir.join(wgpu::util::pipeline_cache_key(adapter)?);
        let data = std::fs::read(&path).ok();
        // SAFETY: the file holds what PipelineCache::get_data returned
        // (written whole, then renamed into place); wgpu checks its header
        // (adapter, driver, size, hash) and starts empty (fallback) if the
        // data is not for this device
        let cache = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor { label: Some("ddgpu pipelines"), data: data.as_deref(), fallback: true })
        };
        let p = Arc::new(Persistent { cache, path, unsaved: AtomicU64::new(0), last_save: Mutex::new(Instant::now()), saving: AtomicBool::new(false) });
        OPEN.lock().unwrap_or_else(|e| e.into_inner()).push(p.clone());
        Some(p)
    }

    pub fn created(&self) {
        self.unsaved.fetch_add(1, Ordering::Relaxed);
    }

    /// Save on another thread if pipelines were created and the last save
    /// is [`SAVE_INTERVAL`] old.
    pub fn save_if_due(self: &Arc<Self>) {
        if self.unsaved.load(Ordering::Relaxed) == 0 {
            return;
        }
        {
            let mut last = self.last_save.lock().unwrap_or_else(|e| e.into_inner());
            if last.elapsed() < SAVE_INTERVAL {
                return;
            }
            *last = Instant::now();
        }
        let me = self.clone();
        let _ = std::thread::Builder::new().name("pipeline-cache".into()).spawn(move || {
            if let Err(e) = me.save() {
                eprintln!("pipeline cache {}: {e}", me.path.display());
            }
        });
    }

    /// Write the cache's data (to a temporary file renamed over the old
    /// one, so a reader never sees half a file). One save at a time.
    pub fn save(&self) -> std::io::Result<()> {
        if self.saving.swap(true, Ordering::Acquire) {
            return Ok(());
        }
        let unsaved = self.unsaved.swap(0, Ordering::Relaxed);
        let r = (|| {
            let Some(data) = self.cache.get_data() else { return Ok(()) };
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = self.path.with_extension("tmp");
            std::fs::write(&tmp, &data)?;
            std::fs::rename(&tmp, &self.path)
        })();
        if r.is_err() {
            self.unsaved.fetch_add(unsaved, Ordering::Relaxed);
        }
        self.saving.store(false, Ordering::Release);
        r
    }
}

/// Save every open pipeline cache that has new pipelines now (a host
/// closing its window: the backend may never be dropped).
pub fn save_pipeline_caches() {
    crate::warmup::flush();
    let open: Vec<Arc<Persistent>> = OPEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for p in open {
        if p.unsaved.load(Ordering::Relaxed) > 0 {
            if let Err(e) = p.save() {
                eprintln!("pipeline cache {}: {e}", p.path.display());
            }
        }
    }
}

/// Same sandbox root as ddruntime::services::sandbox_home. The GPU crate
/// does not depend on the runtime. Explicit host cache overrides still work.
pub fn default_cache_dir() -> Option<PathBuf> {
    Some(std::env::var_os("DDRUNTIME_HOME").map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ddruntime")).join("pipelines"))
}
