//! A bounded, ordered command stream. Only the consumer touches the backend.
use crate::*;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Work = Box<dyn FnOnce(&mut dyn GraphicsBackend) + Send>;

// The bounded channel allocates its slots once. Common commands live directly
// in those slots instead of allocating/freeing an erased closure per command.
// Only infrequent result/presentation hooks need the arbitrary callback arm.
enum Command {
    Draw(DrawCall),
    Draws(Vec<DrawCall>),
    Upload(Upload),
    Clear(ClearCall),
    Copy(CopyCall),
    Fill(FillCall),
    QueryBegin(u32),
    QueryEnd(u32),
    Release(u32),
    Native(NativeJob),
    NativeOrDraw(NativeJob, Vec<DrawCall>),
    Hook(Work),
}

impl Command {
    fn execute(self, backend: &mut dyn GraphicsBackend) {
        match self {
            Self::Draw(draw) => backend.draw(&draw),
            Self::Draws(draws) => backend.draws(draws),
            Self::Upload(upload) => backend.upload(upload),
            Self::Clear(clear) => backend.clear(&clear),
            Self::Copy(copy) => backend.copy(&copy),
            Self::Fill(fill) => backend.fill(&fill),
            Self::QueryBegin(query) => backend.query_begin(query),
            Self::QueryEnd(query) => backend.query_end(query),
            Self::Release(resource) => backend.release(resource),
            Self::Native(job) => {
                if let Err(job) = backend.native(job) {
                    eprintln!("ERROR: accepted native job rejected on backend worker: {} passes / {} draws; producer eligibility contract violated",
                        job.passes.len(), job.passes.iter().map(|p| p.draws.len()).sum::<usize>());
                }
            }
            Self::NativeOrDraw(job, fallback) => backend.native_or_draw(job, fallback),
            Self::Hook(work) => work(backend),
        }
    }
}

/// Moves a backend (including its presentation sink) onto one host thread.
/// Commands own their data; mutable resource IDs are interpreted in FIFO order.
/// The default queue holds at most 2048 commands, plus the executing command.
/// Two outstanding Presents bound frame latency independently of queue length.
/// Dropping the adapter drains the stream and joins the worker.
pub struct DeferredBackend {
    native_supported: bool,
    sender: Option<SyncSender<Command>>,
    worker: Option<JoinHandle<()>>,
    presents: VecDeque<Receiver<()>>,
    /// Commands recorded, and executed (the worker counts them): marks.
    recorded: u64,
    executed: Arc<AtomicU64>,
    /// Occlusion results (L102): each query_end starts a new generation;
    /// a result is asked of the worker once per generation and answered
    /// from here when it is back, so GetData never waits on the queue.
    generation: HashMap<u32, u64>,
    asked: HashMap<u32, u64>,
    results: Arc<Mutex<HashMap<u32, (u64, Option<u64>)>>>,
}

impl DeferredBackend {
    pub fn new(backend: Box<dyn GraphicsBackend>) -> Self {
        Self::with_capacity(backend, 2048)
    }

    /// A command-count bound, not a byte limit: a single upload may be large.
    /// A full queue blocks the producer; no command is dropped or reordered.
    pub fn with_capacity(mut backend: Box<dyn GraphicsBackend>, capacity: usize) -> Self {
        assert!(capacity > 0);
        let native_supported = backend.native_supported();
        let (sender, receiver) = mpsc::sync_channel::<Command>(capacity);
        let executed = Arc::new(AtomicU64::new(0));
        let counter = executed.clone();
        let worker = thread::Builder::new().name("d3d9-backend".into()).spawn(move || {
            while let Ok(work) = receiver.recv() {
                work.execute(backend.as_mut());
                counter.fetch_add(1, Ordering::Release);
            }
        }).expect("start D3D9 backend worker");
        Self {
            native_supported, sender: Some(sender), worker: Some(worker), presents: VecDeque::new(), recorded: 0, executed,
            generation: HashMap::new(), asked: HashMap::new(), results: Default::default(),
        }
    }

    fn enqueue(&mut self, work: impl FnOnce(&mut dyn GraphicsBackend) + Send + 'static) {
        self.command(Command::Hook(Box::new(work)));
    }

    fn command(&mut self, command: Command) {
        // Before recording any part of a third frame, retire the oldest
        // Present. Waiting only at the third Present would allow its entire
        // frame's draws to get three frames ahead of the display.
        if self.presents.len() == 2 {
            self.presents.pop_front().unwrap().recv().expect("D3D9 backend worker stopped before Present");
        }
        if self.sender.as_ref().unwrap().send(command).is_err() {
            panic!("D3D9 backend worker stopped");
        }
        self.recorded += 1;
    }

    fn result<T: Send + 'static>(&mut self, work: impl FnOnce(&mut dyn GraphicsBackend) -> T + Send + 'static) -> T {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.enqueue(move |backend| { let _ = sender.send(work(backend)); });
        receiver.recv().expect("D3D9 backend worker stopped before result")
    }

    /// Wait for preceding host commands. GPU-result operations separately use
    /// the backend's existing GPU synchronization in read_back/query_result.
    pub fn drain(&mut self) {
        self.result(|_| ());
        self.presents.clear();
    }

    /// Carry host presentation context (for example thread-local display mode)
    /// with this frame. The hook must own its data and never access guest state.
    pub fn present_with(&mut self, target: &Target, before: impl FnOnce() + Send + 'static) {
        let target = target.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        self.enqueue(move |backend| {
            before();
            backend.present(&target);
            let _ = sender.send(());
        });
        self.presents.push_back(receiver);
    }
}

impl Drop for DeferredBackend {
    fn drop(&mut self) {
        // Closing the sole producer lets the receiver finish all accepted work.
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            if let Err(panic) = worker.join() {
                if !thread::panicking() { std::panic::resume_unwind(panic); }
            }
        }
    }
}

impl GraphicsBackend for DeferredBackend {
    fn native_supported(&self) -> bool { self.native_supported }
    fn native(&mut self, job: NativeJob) -> Result<(), NativeJob> {
        if !self.native_supported { return Err(job); }
        self.command(Command::Native(job));
        Ok(())
    }
    fn native_or_draw(&mut self, job: NativeJob, fallback: Vec<DrawCall>) {
        // Decide on the worker, where resource/query state is authoritative.
        // In particular, never return enqueue success and discard a later
        // rejection (L133: indexed strips rejected whole multi-pass jobs).
        self.command(Command::NativeOrDraw(job, fallback));
    }
    fn native_compare_begin(&mut self, begin: NativeCompareBegin) -> bool {
        self.result(move |b| b.native_compare_begin(begin))
    }
    fn native_compare_end(&mut self, id: u64, job: NativeJob) -> Result<(), NativeJob> {
        self.result(move |b| b.native_compare_end(id, job))
    }
    fn upload(&mut self, upload: Upload) { self.command(Command::Upload(upload)); }
    fn draw(&mut self, draw: &DrawCall) {
        self.command(Command::Draw(draw.clone()));
    }
    fn draw_owned(&mut self, draw: DrawCall) {
        self.enqueue(move |b| b.draw(&draw));
    }
    fn draws(&mut self, draws: Vec<DrawCall>) {
        self.command(Command::Draws(draws));
    }
    fn clear(&mut self, clear: &ClearCall) {
        self.command(Command::Clear(clear.clone()));
    }
    fn present(&mut self, target: &Target) { self.present_with(target, || {}); }
    fn copy(&mut self, copy: &CopyCall) {
        self.command(Command::Copy(copy.clone()));
    }
    fn fill(&mut self, fill: &FillCall) {
        self.command(Command::Fill(fill.clone()));
    }
    fn read_back(&mut self, target: &Target) -> Option<Vec<u8>> {
        let target = target.clone();
        self.result(move |b| b.read_back(&target))
    }
    fn query_begin(&mut self, query: u32) { self.command(Command::QueryBegin(query)); }
    fn query_end(&mut self, query: u32) {
        *self.generation.entry(query).or_default() += 1;
        self.command(Command::QueryEnd(query));
    }
    /// Never waits (L102): the first call after a query_end asks the worker
    /// for the count and answers None (GetData's S_FALSE, as a driver does
    /// while the GPU has not finished); later calls answer the count once
    /// the worker has put it here. A result is for one generation, so a
    /// count arriving after the query was issued again is not answered.
    fn query_result(&mut self, query: u32) -> Option<u64> {
        let generation = self.generation.get(&query).copied().unwrap_or(0);
        {
            let mut results = self.results.lock().unwrap();
            if let Some(&(g, v)) = results.get(&query) {
                if g == generation && v.is_some() {
                    return v;
                }
                if g == generation {
                    // the backend had no count yet (L120: it does not wait
                    // for the GPU): ask again
                    results.remove(&query);
                    self.asked.remove(&query);
                }
            }
        }
        if self.asked.get(&query) != Some(&generation) {
            self.asked.insert(query, generation);
            let results = self.results.clone();
            self.enqueue(move |b| {
                let v = b.query_result(query);
                results.lock().unwrap().insert(query, (generation, v));
            });
        }
        None
    }
    fn gamma(&mut self, ramp: Option<&GammaRamp>) {
        let ramp = ramp.copied();
        self.enqueue(move |b| b.gamma(ramp.as_ref()));
    }
    fn release(&mut self, resource: u32) { self.command(Command::Release(resource)); }
    fn fence(&mut self) { self.result(|b| b.fence()); }
    fn mark(&mut self) -> u64 { self.recorded }
    fn passed(&self, mark: u64) -> bool { self.executed.load(Ordering::Acquire) >= mark }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct Log(Arc<Mutex<Vec<String>>>);
    impl Log {
        fn add(&self, text: String) { self.0.lock().unwrap().push(text); }
    }
    impl GraphicsBackend for Log {
        fn upload(&mut self, u: Upload) { self.add(format!("upload:{u:?}")); }
        fn draw(&mut self, d: &DrawCall) { self.add(format!("draw:{d:?}")); }
        fn clear(&mut self, c: &ClearCall) { self.add(format!("clear:{c:?}")); }
        fn present(&mut self, t: &Target) { self.add(format!("present:{}", t.surface)); }
        fn copy(&mut self, c: &CopyCall) { self.add(format!("copy:{c:?}")); }
        fn fill(&mut self, f: &FillCall) { self.add(format!("fill:{f:?}")); }
        fn read_back(&mut self, _: &Target) -> Option<Vec<u8>> { self.add("read".into()); Some(vec![42]) }
        fn query_begin(&mut self, q: u32) { self.add(format!("begin:{q}")); }
        fn query_end(&mut self, q: u32) { self.add(format!("end:{q}")); }
        fn query_result(&mut self, q: u32) -> Option<u64> { self.add(format!("result:{q}")); Some(7) }
        fn gamma(&mut self, ramp: Option<&GammaRamp>) { self.add(format!("gamma:{}", ramp.unwrap()[0][0])); }
        fn release(&mut self, r: u32) { self.add(format!("release:{r}")); }
    }
    fn target() -> Target {
        Target { surface: 1, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
    }
    fn queue(capacity: usize) -> (DeferredBackend, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (DeferredBackend::with_capacity(Box::new(Log(log.clone())), capacity), log)
    }

    #[test]
    fn owned_commands_results_and_release_match_synchronous_order() {
        fn frame(b: &mut dyn GraphicsBackend, unblock: Option<mpsc::Sender<()>>) {
            let t = target();
            b.upload(Upload::Buffer { resource: 2, bytes: vec![1, 2, 3] });
            let mut c = ClearCall { rects: vec![[0, 0, 2, 2]], flags: 1, colour: 42, z: 1.0, stencil: 0,
                targets: [Some(t.clone()), None, None, None], depth: None, viewport: [0; 6], scissor: None, srgb: false };
            b.clear(&c);
            c.colour = 99;
            b.query_begin(3);
            let mut d = DrawCall { primitive: 4, primitives: 1, start: 0, base_vertex: 0, min_index: 0, vertex_count: 3,
                vertices: Vertices::Inline { bytes: vec![12; 36], stride: 12 }, indices: Indices::Inline { bytes: vec![0, 1, 2], format: 101 },
                vertex_shader: Some(vec![1, 2].into()), pixel_shader: Some(vec![3, 4].into()), declaration: vec![].into(), fvf: 0,
                vs_constants: Constants { f: vec![[9; 4]], ..Default::default() }.into(), ps_constants: Constants::default().into(),
                textures: vec![].into(), render_states: vec![(7, 1)].into(), viewport: [0; 6], scissor: [0; 4], targets: c.targets.clone(), depth: None };
            b.draw(&d);
            d.vs_constants = Constants::default().into();
            if let Vertices::Inline { bytes, .. } = &mut d.vertices { bytes.fill(99); }
            if let Indices::Inline { bytes, .. } = &mut d.indices { bytes.fill(99); }
            d.vertex_shader = Some(vec![99].into());
            d.pixel_shader = Some(vec![99].into());
            b.query_end(3);
            b.copy(&CopyCall { src: t.clone(), dst: t.clone(), src_rect: [0; 4], dst_rect: [0; 4], filter: 0 });
            b.fill(&FillCall { dst: t.clone(), rect: [0; 4], colour: 17 });
            let mut ramp = [[12; 256]; 3];
            b.gamma(Some(&ramp));
            ramp[0][0] = 99;
            assert_eq!(ramp[0][0], 99);
            b.present(&t);
            if let Some(unblock) = unblock { unblock.send(()).unwrap(); }
            assert_eq!(b.read_back(&t), Some(vec![42]));
            // a deferred backend answers None until the worker has the count
            let r = loop {
                if let Some(r) = b.query_result(3) { break r; }
                b.fence();
            };
            assert_eq!(r, 7);
            b.release(2);
            b.upload(Upload::Buffer { resource: 2, bytes: vec![9] });
        }
        let expected = Arc::new(Mutex::new(Vec::new()));
        frame(&mut Log(expected.clone()), None);
        let (mut deferred, actual) = queue(32);
        let (unblock, blocked) = mpsc::channel();
        deferred.enqueue(move |_| { blocked.recv().unwrap(); });
        frame(&mut deferred, Some(unblock));
        drop(deferred); // must drain the final release and reused resource
        assert_eq!(*actual.lock().unwrap(), *expected.lock().unwrap());
    }

    #[test]
    fn a_run_of_draws_is_one_command_in_its_place() {
        let (mut deferred, log) = queue(8);
        let t = target();
        let draw = |start: u32| DrawCall {
            primitive: 4, primitives: 1, start, base_vertex: 0, min_index: 0, vertex_count: 3,
            vertices: Vertices::Inline { bytes: vec![0; 36], stride: 12 }, indices: Indices::None,
            vertex_shader: None, pixel_shader: None, declaration: vec![].into(), fvf: 0,
            vs_constants: Constants::default().into(), ps_constants: Constants::default().into(),
            textures: vec![].into(), render_states: vec![].into(), viewport: [0; 6], scissor: [0; 4],
            targets: [Some(t.clone()), None, None, None], depth: None,
        };
        deferred.release(1);
        let before = deferred.mark();
        deferred.draws(vec![draw(0), draw(3), draw(6)]);
        assert_eq!(deferred.mark(), before + 1, "one command");
        deferred.release(2);
        deferred.fence();
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 5);
        assert_eq!(log[0], "release:1");
        for (k, start) in [0, 3, 6].into_iter().enumerate() {
            assert_eq!(log[1 + k], format!("draw:{:?}", draw(start)));
        }
        assert_eq!(log[4], "release:2");
    }

    #[test]
    fn full_queue_blocks_without_losing_commands() {
        let (mut deferred, log) = queue(1);
        let (unblock, blocked) = mpsc::channel();
        let (started, ready) = mpsc::channel();
        deferred.enqueue(move |_| { started.send(()).unwrap(); blocked.recv().unwrap(); });
        ready.recv().unwrap();
        deferred.release(1); // fills the sole pending slot
        let (sent, received) = mpsc::channel();
        let producer = thread::spawn(move || { deferred.release(2); sent.send(()).unwrap(); deferred });
        let waiting = received.recv_timeout(Duration::from_millis(50)).is_err();
        unblock.send(()).unwrap();
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(producer.join().unwrap());
        assert!(waiting);
        assert_eq!(*log.lock().unwrap(), ["release:1", "release:2"]);
    }

    #[test]
    fn snapshot_remains_owned_while_worker_is_blocked() {
        let (mut deferred, log) = queue(8);
        let (unblock, blocked) = mpsc::channel();
        deferred.enqueue(move |_| { blocked.recv().unwrap(); });
        let mut clear = ClearCall { rects: vec![[0, 0, 2, 2]], flags: 1, colour: 42, z: 1.0, stencil: 0,
            targets: [Some(target()), None, None, None], depth: None, viewport: [0; 6], scissor: None, srgb: false };
        let expected = format!("clear:{clear:?}");
        deferred.clear(&clear);
        clear.rects[0] = [99; 4];
        clear.targets[0].as_mut().unwrap().surface = 99;
        clear.colour = 99;
        drop(clear);
        let mut ramp = [[12; 256]; 3];
        deferred.gamma(Some(&ramp));
        ramp[0][0] = 99;
        assert_eq!(ramp[0][0], 99);
        unblock.send(()).unwrap();
        deferred.fence();
        assert_eq!(*log.lock().unwrap(), [expected, "gamma:12".into()]);
    }

    #[test]
    fn two_presents_return_and_next_frame_waits_for_first() {
        let (mut deferred, log) = queue(8);
        let producer_id = thread::current().id();
        let (unblock, blocked) = mpsc::channel();
        deferred.present_with(&target(), move || {
            assert_ne!(thread::current().id(), producer_id);
            blocked.recv().unwrap();
        });
        deferred.present(&target());
        let (sent, received) = mpsc::channel();
        let producer = thread::spawn(move || { deferred.release(7); sent.send(()).unwrap(); deferred.present(&target()); deferred });
        let waiting = received.recv_timeout(Duration::from_millis(50)).is_err();
        unblock.send(()).unwrap();
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(producer.join().unwrap());
        assert!(waiting);
        assert_eq!(*log.lock().unwrap(), ["present:1", "present:1", "release:7", "present:1"]);
    }

    #[test]
    fn worker_failure_disconnects_results_and_drop_does_not_double_panic() {
        let (mut deferred, _) = queue(1);
        deferred.enqueue(|_| panic!("backend failed"));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || deferred.drain())).is_err());
    }

    #[test]
    fn unsupported_native_job_returns_owned_payload_after_prior_uploads() {
        let (mut backend, log) = queue(4);
        backend.upload(Upload::Buffer { resource: 42, bytes: vec![1, 2, 3] });
        let job = NativeJob { passes: vec![NativePass { colours: [Some(target()), None, None, None], clear_colour: Some(17), ..Default::default() }] };
        assert_eq!(backend.native(job.clone()), Err(job.clone()));
        assert!(!backend.native_compare_begin(NativeCompareBegin { id: 7, ..Default::default() }));
        assert_eq!(backend.native_compare_end(7, job.clone()), Err(job));
        backend.release(42);
        backend.drain();
        let entries = log.lock().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].starts_with("upload:"));
        assert_eq!(entries[1], "release:42");
    }
    struct NativeLog(Log, bool);
    impl GraphicsBackend for NativeLog {
        fn native_supported(&self) -> bool { true }
        fn native(&mut self, job: NativeJob) -> Result<(), NativeJob> {
            self.0.add(format!("native:{}", job.passes.len()));
            if self.1 { Err(job) } else { Ok(()) }
        }
        fn upload(&mut self, u: Upload) { self.0.upload(u); }
        fn draw(&mut self, d: &DrawCall) { self.0.draw(d); }
        fn clear(&mut self, c: &ClearCall) { self.0.clear(c); }
        fn present(&mut self, t: &Target) { self.0.present(t); }
        fn copy(&mut self, c: &CopyCall) { self.0.copy(c); }
        fn fill(&mut self, c: &FillCall) { self.0.fill(c); }
        fn read_back(&mut self, t: &Target) -> Option<Vec<u8>> { self.0.read_back(t) }
        fn query_begin(&mut self, q: u32) { self.0.query_begin(q); }
        fn query_end(&mut self, q: u32) { self.0.query_end(q); }
        fn query_result(&mut self, q: u32) -> Option<u64> { self.0.query_result(q) }
        fn gamma(&mut self, g: Option<&GammaRamp>) { self.0.gamma(g); }
        fn release(&mut self, r: u32) { self.0.release(r); }
    }
    #[test]
    fn accepted_native_job_shares_resource_and_query_fifo() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut backend = DeferredBackend::with_capacity(Box::new(NativeLog(Log(log.clone()), false)), 4);
        backend.upload(Upload::Buffer { resource: 42, bytes: vec![9] });
        assert_eq!(backend.native(NativeJob { passes: vec![NativePass::default()] }), Ok(()));
        backend.query_begin(7);
        backend.release(42);
        backend.drain();
        let entries = log.lock().unwrap();
        assert!(entries[0].starts_with("upload:"));
        assert_eq!(&entries[1..], ["native:1", "begin:7", "release:42"]);
    }

    #[test]
    fn supported_native_enqueue_does_not_wait_for_prior_work() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut backend = DeferredBackend::with_capacity(Box::new(NativeLog(Log(log.clone()), false)), 4);
        assert!(backend.native_supported());
        let (unblock, blocked) = mpsc::channel();
        backend.enqueue(move |_| { blocked.recv().unwrap(); });
        let (sent, received) = mpsc::channel();
        let producer = thread::spawn(move || {
            let result = backend.native(NativeJob::default());
            sent.send(result).unwrap(); backend
        });
        let result = received.recv_timeout(Duration::from_secs(2));
        unblock.send(()).unwrap();
        drop(producer.join().unwrap());
        assert_eq!(result.unwrap(), Ok(()), "native enqueue must not drain worker");
        assert_eq!(*log.lock().unwrap(), ["native:0"]);
    }
    fn fallback_draw() -> DrawCall {
        DrawCall {
            primitive: 5, primitives: 1, start: 0, base_vertex: 0,
            min_index: 0, vertex_count: 3,
            vertices: Vertices::Inline { bytes: vec![12; 36], stride: 12 },
            indices: Indices::Inline { bytes: vec![0, 0, 1, 0, 2, 0], format: 101 },
            vertex_shader: Some(vec![1, 2].into()), pixel_shader: Some(vec![3, 4].into()),
            declaration: vec![].into(), fvf: 0,
            vs_constants: Constants::default().into(), ps_constants: Constants::default().into(),
            textures: vec![].into(), render_states: vec![(7, 1)].into(),
            viewport: [0; 6], scissor: [0; 4],
            targets: [Some(target()), None, None, None], depth: None,
        }
    }
    #[test]
    fn rejected_native_worker_runs_original_draws_once_in_fifo() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut backend = DeferredBackend::with_capacity(Box::new(NativeLog(Log(log.clone()), true)), 4);
        backend.upload(Upload::Buffer { resource: 42, bytes: vec![7] });
        backend.query_begin(9);
        let first = fallback_draw();
        let mut second = first.clone();
        second.start = 3;
        let expected = [format!("draw:{first:?}"), format!("draw:{second:?}")];
        backend.native_or_draw(NativeJob { passes: vec![NativePass::default(), NativePass::default()] }, vec![first, second]);
        backend.query_end(9);
        backend.release(42);
        backend.drain();
        let entries = log.lock().unwrap();
        assert_eq!(entries.len(), 7);
        assert!(entries[0].starts_with("upload:"));
        assert_eq!(&entries[1..3], ["begin:9", "native:2"]);
        assert_eq!(&entries[3..5], &expected);
        assert_eq!(&entries[5..], ["end:9", "release:42"]);
    }
    #[test]
    fn native_with_fallback_is_async_and_acceptance_does_not_duplicate() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut backend = DeferredBackend::with_capacity(Box::new(NativeLog(Log(log.clone()), false)), 4);
        let (unblock, blocked) = mpsc::channel();
        backend.enqueue(move |_| { blocked.recv().unwrap(); });
        let (sent, received) = mpsc::channel();
        let producer = thread::spawn(move || {
            backend.native_or_draw(NativeJob::default(), vec![fallback_draw()]);
            sent.send(()).unwrap(); backend
        });
        let result = received.recv_timeout(Duration::from_secs(2));
        unblock.send(()).unwrap();
        drop(producer.join().unwrap());
        assert!(result.is_ok(), "fallback transport must not drain worker");
        assert_eq!(*log.lock().unwrap(), ["native:0"]);
    }
    #[test]
    fn unsupported_backend_executes_fallback_once() {
        let (mut backend, log) = queue(4);
        let draw = fallback_draw();
        let expected = format!("draw:{draw:?}");
        backend.native_or_draw(NativeJob::default(), vec![draw]);
        backend.drain();
        assert_eq!(*log.lock().unwrap(), [expected]);
    }
    #[test]
    #[should_panic(expected = "draw-only job")]
    fn draw_fallback_refuses_jobs_containing_clears() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut backend = NativeLog(Log(log), true);
        backend.native_or_draw(NativeJob { passes: vec![NativePass {
            clear_colour: Some(0), ..Default::default()
        }] }, vec![fallback_draw()]);
    }

}
