//! Optional, local-only pipeline recipes. All disk I/O and speculative GPU
//! compilation run on background creator threads. The draw consumer never waits for
//! a recipe: missing/unfinished/invalid entries use normal validated creation.
use crate::cache::{FxMap, ModuleKey, PipelineKey, SamplerKinds};
use bincode::Options;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc};

const VERSION: u32 = 1;
const LIMIT: u64 = 16 << 20;
const MAX_RECIPES: usize = 16384;
type Digest = [u8; 32];

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Recipe {
    pub key: PipelineKey,
    vs: Digest,
    ps: Digest,
    ps_kinds: SamplerKinds,
    vs_kinds: SamplerKinds,
}

impl Recipe {
    fn matches_sources(&self, sources: &FxMap<ModuleKey, Digest>) -> bool {
        sources.get(&self.key.vs) == Some(&self.vs) && sources.get(&self.key.ps) == Some(&self.ps)
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    identity: String,
    recipes: Vec<Recipe>,
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new().with_fixint_encoding().with_limit(LIMIT).reject_trailing_bytes()
}

fn encode(identity: &str, recipes: &[Recipe]) -> Option<Vec<u8>> {
    let payload = codec().serialize(&Manifest { version: VERSION, identity: identity.into(), recipes: recipes.to_vec() }).ok()?;
    let mut bytes = blake3::hash(&payload).as_bytes().to_vec();
    bytes.extend(payload);
    Some(bytes)
}

fn decode(identity: &str, bytes: &[u8]) -> Option<Vec<Recipe>> {
    if bytes.len() as u64 > LIMIT { return None; }
    let (digest, payload) = bytes.split_at_checked(32)?;
    if digest != blake3::hash(payload).as_bytes() { return None; }
    let m: Manifest = codec().deserialize(payload).ok()?;
    (m.version == VERSION && m.identity == identity && m.recipes.len() <= MAX_RECIPES).then_some(m.recipes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension(format!("{}.{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&tmp); }
    result
}

enum Record {
    Shader(u64),
    Source(Digest, String),
    Pipeline(Recipe),
    Flush(mpsc::Sender<()>),
}

enum Ready {
    Module(Digest, wgpu::ShaderModule),
    Pipeline(Recipe, wgpu::RenderPipeline),
}

static OPEN: Mutex<Vec<mpsc::Sender<Record>>> = Mutex::new(Vec::new());

/// Called by the host's existing close-time cache save, not by a draw.
pub(crate) fn flush() {
    let mut open = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    open.retain(|sender| {
        let (tx, rx) = mpsc::channel();
        if sender.send(Record::Flush(tx)).is_err() { return false; }
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        true
    });
}

pub(crate) struct Warmup {
    sender: mpsc::Sender<Record>,
    ready: mpsc::Receiver<Ready>,
    sources: FxMap<ModuleKey, Digest>,
    modules: FxMap<Digest, wgpu::ShaderModule>,
    pipelines: FxMap<PipelineKey, (Recipe, wgpu::RenderPipeline)>,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl Drop for Warmup {
    fn drop(&mut self) { self.stop.store(true, Ordering::Relaxed); }
}

impl Warmup {
    pub fn start(device: wgpu::Device, adapter: wgpu::AdapterInfo, layout0: wgpu::BindGroupLayout, ps_compute: bool,
                 cache: Arc<crate::pipelines::Persistent>, dir: PathBuf, exe: PathBuf) -> Option<Self> {
        let driver = wgpu::util::pipeline_cache_key(&adapter)?;
        let (sender, records) = mpsc::channel();
        let (completed, ready) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        std::thread::Builder::new().name("pipeline-warmup".into()).spawn(move || {
            if let Err(e) = worker(device, layout0, ps_compute, cache, &dir, &exe, &driver, records, completed, cancelled, done.clone()) {
                eprintln!("pipeline warmup disabled: {e}");
            }
            done.store(true, Ordering::Release);
        }).ok()?;
        OPEN.lock().unwrap_or_else(|e| e.into_inner()).push(sender.clone());
        Some(Self { sender, ready, sources: Default::default(), modules: Default::default(), pipelines: Default::default(), stop, finished })
    }

    pub fn shader_created(&self, tokens: &[u32]) {
        let _ = self.sender.send(Record::Shader(crate::cache::token_hash(tokens)));
    }

    pub fn finished(&self) -> bool { self.finished.load(Ordering::Acquire) }

    pub fn drain(&mut self) {
        while let Ok(ready) = self.ready.try_recv() {
            match ready {
                Ready::Module(hash, module) => { self.modules.insert(hash, module); }
                Ready::Pipeline(recipe, pipeline) => { self.pipelines.insert(recipe.key.clone(), (recipe, pipeline)); }
            }
        }
    }

    pub fn ready_count(&self) -> usize { self.pipelines.len() }

    pub fn pipeline(&mut self, key: &PipelineKey) -> Option<wgpu::RenderPipeline> {
        let (recipe, pipeline) = self.pipelines.remove(key)?;
        recipe.matches_sources(&self.sources).then_some(pipeline)
    }

    pub fn source(&mut self, key: ModuleKey, source: &str) -> Option<wgpu::ShaderModule> {
        let hash = *blake3::hash(source.as_bytes()).as_bytes();
        if self.sources.insert(key, hash) != Some(hash) {
            let _ = self.sender.send(Record::Source(hash, source.into()));
        }
        self.modules.get(&hash).cloned()
    }

    pub fn record(&self, key: &PipelineKey, ps_kinds: &SamplerKinds, vs_kinds: &SamplerKinds) {
        let (Some(vs), Some(ps)) = (self.sources.get(&key.vs), self.sources.get(&key.ps)) else { return };
        let _ = self.sender.send(Record::Pipeline(Recipe {
            key: key.clone(), vs: *vs, ps: *ps, ps_kinds: ps_kinds.clone(), vs_kinds: vs_kinds.clone(),
        }));
    }
}

fn source_path(dir: &Path, digest: &Digest) -> PathBuf {
    dir.join("sources").join(format!("{}.wgsl", blake3::Hash::from_bytes(*digest).to_hex()))
}

fn identity(exe: &Path, driver: &str) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(exe)?;
    let mut hash = blake3::Hasher::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 { break; }
        hash.update(&bytes[..n]);
    }
    // Invalidate translated sources and descriptors on implementation changes,
    // as well as exe/driver changes. No game bytes are embedded in the binary.
    let mut schema = blake3::Hasher::new();
    schema.update(include_bytes!("../../d3d9wgsl/src/lib.rs"));
    schema.update(include_bytes!("cache.rs"));
    schema.update(include_bytes!("lib.rs"));
    schema.update(include_bytes!("warmup.rs"));
    Ok(format!("v{VERSION}-wgpu29.0.4-{}-{driver}-{}", hash.finalize().to_hex(), schema.finalize().to_hex()))
}

struct Store {
    path: PathBuf,
    dir: PathBuf,
    identity: String,
    recipes: Vec<Recipe>,
    dirty: bool,
    saved: std::time::Instant,
}

impl Store {
    fn record(&mut self, record: Record) {
        match record {
            Record::Shader(_) => {},
            Record::Source(hash, source) => { let _ = atomic_write(&source_path(&self.dir, &hash), source.as_bytes()); }
            Record::Pipeline(recipe) => {
                if let Some(old) = self.recipes.iter_mut().find(|r| r.key == recipe.key) {
                    // The same bytecode identity can be translated with a new
                    // input/link variant. Keep the most recently used recipe.
                    if old.vs != recipe.vs || old.ps != recipe.ps {
                        *old = recipe;
                        self.dirty = true;
                    }
                } else if self.recipes.len() < MAX_RECIPES {
                    self.recipes.push(recipe);
                    self.dirty = true;
                }
            }
            Record::Flush(done) => { self.save(); let _ = done.send(()); }
        }
    }
    fn save(&mut self) {
        if !self.dirty { return; }
        if let Some(bytes) = encode(&self.identity, &self.recipes) {
            match atomic_write(&self.path, &bytes) {
                Ok(()) => { self.dirty = false; self.saved = std::time::Instant::now(); }
                Err(e) => eprintln!("pipeline recipes: {e}"),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn worker(device: wgpu::Device, layout0: wgpu::BindGroupLayout, ps_compute: bool, cache: Arc<crate::pipelines::Persistent>,
          dir: &Path, exe: &Path, driver: &str, records: mpsc::Receiver<Record>, ready: mpsc::Sender<Ready>, stop: Arc<AtomicBool>, done: Arc<AtomicBool>) -> Result<(), String> {
    let driver = format!("{driver}-{:?}-{:?}", device.features(), device.limits());
    let identity = identity(exe, &driver).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.keys", blake3::hash(identity.as_bytes()).to_hex()));
    let recipes = std::fs::metadata(&path).ok().filter(|m| m.len() <= LIMIT)
        .and_then(|_| std::fs::read(&path).ok()).and_then(|b| decode(&identity, &b)).unwrap_or_default();
    let pending = if std::env::var("DDGPU_PIPELINE_WARMUP").as_deref() == Ok("0") { Vec::new() } else { recipes.clone() };
    let mut store = Store { path, dir: dir.to_owned(), identity, recipes, dirty: false, saved: std::time::Instant::now() };
    let queue = Arc::new(Mutex::new(RecipeQueue(pending.into())));
    let creators = creator_count(std::env::var("DDGPU_PIPELINE_THREADS").ok().as_deref());
    // Only creator threads wait on these cells, never the draw consumer.
    let modules: ModuleCells = Arc::new(Mutex::new(FxMap::default()));
    let started = std::time::Instant::now();
    let compiled = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let recipe_count = queue.lock().unwrap().0.len();
    let mut shader_events = 0usize;
    let mut workers = Vec::new();
    for id in 0..creators {
        let (device, layout0, cache, ready, stop, queue, modules, dir, compiled) =
            (device.clone(), layout0.clone(), cache.clone(), ready.clone(), stop.clone(), queue.clone(), modules.clone(), dir.to_owned(), compiled.clone());
        match std::thread::Builder::new().name(format!("pipeline-creator-{id}")).spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Some(recipe) = queue.lock().unwrap_or_else(|e| e.into_inner()).0.pop_front() else { break };
                let built = (|| {
                    let vs_module = cached_module(&modules, recipe.vs, || load_module(&device, &dir, recipe.vs, &ready))?;
                    let ps_module = cached_module(&modules, recipe.ps, || load_module(&device, &dir, recipe.ps, &ready))?;
                    let ps = texture_layout(&device, false, &recipe.ps_kinds, ps_compute);
                    let vs = texture_layout(&device, true, &recipe.vs_kinds, ps_compute);
                    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("warmup"), bind_group_layouts: &[Some(&layout0), Some(&ps), Some(&vs)], immediate_size: 0,
                    });
                    let started = std::time::Instant::now();
                    let pipeline = create_pipeline(&device, &layout, &vs_module, &ps_module, &recipe.key, Some(&cache.cache), "warmup")?;
                    cache.created();
                    crate::pipelines::pipeline_created(started);
                    let _ = ready.send(Ready::Pipeline(recipe, pipeline));
                    compiled.fetch_add(1, Ordering::Relaxed);
                    Ok::<_, String>(())
                })();
                if let Err(e) = built { eprintln!("pipeline warmup entry ignored: {e}"); }
            }
        }) {
            Ok(worker) => workers.push(worker),
            Err(e) => eprintln!("pipeline creator unavailable: {e}"),
        }
    }
    // Persistence and advisory shader events stay responsive during compilation.
    // Queue locks cover only pop/reordering, never GPU calls or module waits.
    while workers.iter().any(|w| !w.is_finished()) {
        if store.saved.elapsed().as_secs() >= 1 { store.save(); }
        match records.recv_timeout(std::time::Duration::from_millis(10)) {
            Ok(Record::Shader(hash)) => {
                shader_events += 1;
                queue.lock().unwrap_or_else(|e| e.into_inner()).prioritize(hash);
            },
            Ok(record) => store.record(record),
            Err(mpsc::RecvTimeoutError::Disconnected) => { stop.store(true, Ordering::Relaxed); },
            Err(mpsc::RecvTimeoutError::Timeout) => {},
        }
    }
    for worker in workers { if worker.join().is_err() { eprintln!("pipeline creator panicked; draw fallback retained"); } }
    eprintln!("DDGPU_WARMUP creators={creators} recipes={recipe_count} compiled={} shader_events={shader_events} elapsed_ms={:.3} pending={} cancelled={}",
        compiled.load(Ordering::Relaxed), started.elapsed().as_secs_f64() * 1000.0, queue.lock().unwrap_or_else(|e| e.into_inner()).0.len(), stop.load(Ordering::Relaxed));
    done.store(true, Ordering::Release);
    while !stop.load(Ordering::Relaxed) {
        if store.saved.elapsed().as_secs() >= 1 { store.save(); }
        match records.recv_timeout(std::time::Duration::from_millis(250)) {
            Ok(record) => store.record(record),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {},
        }
    }
    for record in records.try_iter() { store.record(record); }
    store.save();
    Ok(())
}

// Invalid values retain the previous one-creator policy; explicit N is capped
// to four to bound speculative CPU contention on shared hosts.
fn creator_count(value: Option<&str>) -> usize {
    value.and_then(|v| v.parse::<usize>().ok()).filter(|&n| n > 0).unwrap_or(1).min(4)
}

struct RecipeQueue(std::collections::VecDeque<Recipe>);
impl RecipeQueue {
    fn prioritize(&mut self, hash: u64) {
        // Stable partition: retain observed first-use order within each class.
        let mut matching = std::collections::VecDeque::new();
        let mut rest = std::collections::VecDeque::new();
        for recipe in self.0.drain(..) {
            if recipe.key.vs.0 == hash || recipe.key.ps.0 == hash { matching.push_back(recipe); }
            else { rest.push_back(recipe); }
        }
        matching.append(&mut rest);
        self.0 = matching;
    }
}

type ModuleCells = Arc<Mutex<FxMap<Digest, Arc<std::sync::OnceLock<Result<wgpu::ShaderModule, String>>>>>>;
fn cached_module(cells: &ModuleCells, hash: Digest, load: impl FnOnce() -> Result<wgpu::ShaderModule, String>) -> Result<wgpu::ShaderModule, String> {
    let cell = cells.lock().unwrap_or_else(|e| e.into_inner()).entry(hash).or_default().clone();
    cell.get_or_init(load).clone()
}

fn load_module(device: &wgpu::Device, dir: &Path, hash: Digest, ready: &mpsc::Sender<Ready>) -> Result<wgpu::ShaderModule, String> {
    let started = std::time::Instant::now();
    let path = source_path(dir, &hash);
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 4 << 20 { return Err("source too large".into()); }
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if blake3::hash(source.as_bytes()).as_bytes() != &hash { return Err("source checksum".into()); }
    let source = crate::shader_cache::source(source)?;
    // wgpu 29 std error scopes are thread-local (api/device.rs).
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = crate::pipeline_log::timed("module", "warmup", || device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("warmup"), source }));
    if let Some(e) = crate::block_on(scope.pop()) { return Err(e.to_string()); }
    crate::pipelines::module_created(started);
    let _ = ready.send(Ready::Module(hash, module.clone()));
    Ok(module)
}

/// A stage's texture and sampler layout; `ps_compute`: the pixel stage's
/// is also visible to compute (DDGPU_NATIVE_SIM's quad dispatches, L113).
pub(crate) fn texture_layout(device: &wgpu::Device, vs: bool, kinds: &[(u32, u8)], ps_compute: bool) -> wgpu::BindGroupLayout {
    let visibility = if vs {
        wgpu::ShaderStages::VERTEX
    } else if ps_compute {
        wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE
    } else {
        wgpu::ShaderStages::FRAGMENT
    };
    let mut entries = Vec::new();
    for &(n, kind) in kinds {
        let view_dimension = match kind { 0 => wgpu::TextureViewDimension::D2, 1 => wgpu::TextureViewDimension::Cube, _ => wgpu::TextureViewDimension::D3 };
        entries.push(wgpu::BindGroupLayoutEntry { binding: 2 * n, visibility, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false }, count: None });
        entries.push(wgpu::BindGroupLayoutEntry { binding: 2 * n + 1, visibility, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout, vs: &wgpu::ShaderModule, ps: &wgpu::ShaderModule,
                             key: &PipelineKey, cache: Option<&wgpu::PipelineCache>, disposition: &str) -> Result<wgpu::RenderPipeline, String> {
    let attrs: Vec<Vec<wgpu::VertexAttribute>> = key.buffers.iter()
        .map(|b| b.2.iter().map(|&(format, offset, shader_location)| wgpu::VertexAttribute { format, offset, shader_location }).collect()).collect();
    let layouts: Vec<wgpu::VertexBufferLayout> = key.buffers.iter().zip(&attrs)
        .map(|(b, a)| wgpu::VertexBufferLayout { array_stride: b.0, step_mode: b.1, attributes: a }).collect();
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = crate::pipeline_log::timed("pipeline", disposition, || device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None, layout: Some(layout),
        vertex: wgpu::VertexState { module: vs, entry_point: Some("main"), compilation_options: Default::default(), buffers: &layouts },
        primitive: wgpu::PrimitiveState { topology: key.topology, strip_index_format: key.strip_index, front_face: wgpu::FrontFace::Cw, cull_mode: key.cull, polygon_mode: key.polygon_mode, ..Default::default() },
        depth_stencil: key.depth.clone(),
        multisample: wgpu::MultisampleState { count: key.samples, mask: key.sample_mask as u64, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState { module: ps, entry_point: Some("main"), compilation_options: Default::default(), targets: &key.targets }),
        multiview_mask: None, cache,
    }));
    if let Some(e) = crate::block_on(scope.pop()) { return Err(format!("pipeline: {e}")); }
    Ok(pipeline)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creator_policy_is_opt_in_and_bounded() {
        for value in [None, Some("0"), Some("bad"), Some("-1")] { assert_eq!(creator_count(value), 1); }
        assert_eq!(creator_count(Some("2")), 2);
        assert_eq!(creator_count(Some("4")), 4);
        assert_eq!(creator_count(Some("99")), 4);
    }

    #[test]
    fn shader_events_prioritize_exact_recipes_without_duplicates() {
        let mut entries = Vec::new();
        for i in 0..6 { let mut r = recipe(); r.key.vs.0 = i; r.key.ps.0 = 100 + i % 2; entries.push(r); }
        let mut queue = RecipeQueue(entries.into());
        queue.prioritize(101);
        assert_eq!(queue.0.iter().map(|r| r.key.vs.0).collect::<Vec<_>>(), [1, 3, 5, 0, 2, 4]);
        queue.prioritize(2);
        assert_eq!(queue.0.iter().map(|r| r.key.vs.0).collect::<Vec<_>>(), [2, 1, 3, 5, 0, 4]);
        queue.prioritize(999);
        assert_eq!(queue.0.len(), 6);
        // Concurrent consumers take each recipe exactly once.
        let queue = Arc::new(Mutex::new(queue));
        let seen = Arc::new(Mutex::new(Vec::new()));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let (queue, seen) = (queue.clone(), seen.clone());
                scope.spawn(move || loop {
                    let Some(r) = queue.lock().unwrap().0.pop_front() else { break };
                    seen.lock().unwrap().push(r.key.vs.0);
                });
            }
        });
        let mut seen = seen.lock().unwrap().clone(); seen.sort();
        assert_eq!(seen, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn failed_module_is_attempted_once_across_creators() {
        let cells = ModuleCells::default();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let (cells, calls) = (&cells, &calls);
                scope.spawn(move || {
                    assert!(cached_module(cells, [7; 32], || {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Err("invalid source".into())
                    }).is_err());
                });
            }
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    fn recipe() -> Recipe {
        Recipe {
            key: PipelineKey { vs: (1, 2, true, 3), ps: (4, 5, false, 6),
                buffers: vec![(16, wgpu::VertexStepMode::Instance, vec![(wgpu::VertexFormat::Float32x4, 0, 1)])],
                topology: wgpu::PrimitiveTopology::TriangleStrip, strip_index: Some(wgpu::IndexFormat::Uint32),
                cull: Some(wgpu::Face::Back), polygon_mode: wgpu::PolygonMode::Line,
                targets: vec![None, Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::RED })],
                depth: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth24PlusStencil8, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
                samples: 4, sample_mask: 0x76543210,
            }, vs: [1; 32], ps: [2; 32], ps_kinds: vec![(2, 1)], vs_kinds: vec![(3, 2)],
        }
    }
    #[test]
    fn recipe_roundtrip() {
        let r = recipe();
        let bytes = encode("exe-driver", &[r.clone()]).unwrap();
        let restored = decode("exe-driver", &bytes).unwrap();
        assert!(restored[0].key == r.key);
        assert_eq!(restored[0].vs, r.vs);
        assert_eq!(restored[0].ps, r.ps);
        assert_eq!(restored[0].ps_kinds, r.ps_kinds);
        assert_eq!(restored[0].vs_kinds, r.vs_kinds);
        assert_eq!(encode("exe-driver", &restored).unwrap(), bytes);
    }
    #[test]
    fn shader_variants_must_match_before_adopting_a_pipeline() {
        let recipe = recipe();
        let mut sources = FxMap::default();
        assert!(!recipe.matches_sources(&sources));
        sources.insert(recipe.key.vs, recipe.vs);
        sources.insert(recipe.key.ps, recipe.ps);
        assert!(recipe.matches_sources(&sources));
        sources.insert(recipe.key.vs, [9; 32]);
        assert!(!recipe.matches_sources(&sources), "same token key, different generated input variant");
        sources.insert(recipe.key.vs, recipe.vs);
        sources.insert(recipe.key.ps, [9; 32]);
        assert!(!recipe.matches_sources(&sources), "different linked pixel variant");
    }
    #[test]
    fn version_identity_and_corruption_are_misses() {
        let bytes = encode("exe-driver", &[recipe()]).unwrap();
        assert!(decode("other-exe-driver", &bytes).is_none());
        for len in [0, 16, 32, bytes.len() - 1] { assert!(decode("exe-driver", &bytes[..len]).is_none()); }
        let mut corrupt = bytes.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(decode("exe-driver", &corrupt).is_none());
        let mut m: Manifest = codec().deserialize(&bytes[32..]).unwrap();
        m.version += 1;
        let payload = codec().serialize(&m).unwrap();
        let mut changed = blake3::hash(&payload).as_bytes().to_vec();
        changed.extend(payload);
        assert!(decode("exe-driver", &changed).is_none());
    }

    #[test]
    fn atomic_file_roundtrip_and_executable_driver_namespaces() {
        let dir = std::env::temp_dir().join(format!("ddgpu-recipe-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("synthetic-exe");
        std::fs::write(&exe, b"synthetic executable one").unwrap();
        let first = identity(&exe, "driver-one").unwrap();
        assert_ne!(first, identity(&exe, "driver-two").unwrap());
        std::fs::write(&exe, b"synthetic executable two").unwrap();
        assert_ne!(first, identity(&exe, "driver-one").unwrap());
        let file = dir.join("test.keys");
        let bytes = encode(&first, &[recipe()]).unwrap();
        atomic_write(&file, &bytes).unwrap();
        assert_eq!(decode(&first, &std::fs::read(&file).unwrap()).unwrap().len(), 1);
        atomic_write(&file, b"corrupt/truncated").unwrap();
        assert!(decode(&first, &std::fs::read(&file).unwrap()).is_none());
        atomic_write(&file, &bytes).unwrap();
        assert!(decode(&first, &std::fs::read(&file).unwrap()).is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
