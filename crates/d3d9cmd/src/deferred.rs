//! A bounded, ordered command stream. Only the consumer touches the backend.
use crate::*;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Work = Box<dyn FnOnce(&mut dyn GraphicsBackend) + Send>;

/// Moves a backend (including its presentation sink) onto one host thread.
/// Commands own their data; mutable resource IDs are interpreted in FIFO order.
/// The default queue holds at most 2048 commands, plus the executing command.
/// Two outstanding Presents bound frame latency independently of queue length.
/// Dropping the adapter drains the stream and joins the worker.
pub struct DeferredBackend {
    sender: Option<SyncSender<Work>>,
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
        let (sender, receiver) = mpsc::sync_channel::<Work>(capacity);
        let executed = Arc::new(AtomicU64::new(0));
        let counter = executed.clone();
        let worker = thread::Builder::new().name("d3d9-backend".into()).spawn(move || {
            while let Ok(work) = receiver.recv() {
                work(backend.as_mut());
                counter.fetch_add(1, Ordering::Release);
            }
        }).expect("start D3D9 backend worker");
        Self {
            sender: Some(sender), worker: Some(worker), presents: VecDeque::new(), recorded: 0, executed,
            generation: HashMap::new(), asked: HashMap::new(), results: Default::default(),
        }
    }

    fn enqueue(&mut self, work: impl FnOnce(&mut dyn GraphicsBackend) + Send + 'static) {
        // Before recording any part of a third frame, retire the oldest
        // Present. Waiting only at the third Present would allow its entire
        // frame's draws to get three frames ahead of the display.
        if self.presents.len() == 2 {
            self.presents.pop_front().unwrap().recv().expect("D3D9 backend worker stopped before Present");
        }
        if self.sender.as_ref().unwrap().send(Box::new(work)).is_err() {
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
    fn upload(&mut self, upload: Upload) { self.enqueue(move |b| b.upload(upload)); }
    fn draw(&mut self, draw: &DrawCall) {
        let draw = draw.clone();
        self.enqueue(move |b| b.draw(&draw));
    }
    fn clear(&mut self, clear: &ClearCall) {
        let clear = clear.clone();
        self.enqueue(move |b| b.clear(&clear));
    }
    fn present(&mut self, target: &Target) { self.present_with(target, || {}); }
    fn copy(&mut self, copy: &CopyCall) {
        let copy = copy.clone();
        self.enqueue(move |b| b.copy(&copy));
    }
    fn fill(&mut self, fill: &FillCall) {
        let fill = fill.clone();
        self.enqueue(move |b| b.fill(&fill));
    }
    fn read_back(&mut self, target: &Target) -> Option<Vec<u8>> {
        let target = target.clone();
        self.result(move |b| b.read_back(&target))
    }
    fn query_begin(&mut self, query: u32) { self.enqueue(move |b| b.query_begin(query)); }
    fn query_end(&mut self, query: u32) {
        *self.generation.entry(query).or_default() += 1;
        self.enqueue(move |b| b.query_end(query));
    }
    /// Never waits (L102): the first call after a query_end asks the worker
    /// for the count and answers None (GetData's S_FALSE, as a driver does
    /// while the GPU has not finished); later calls answer the count once
    /// the worker has put it here. A result is for one generation, so a
    /// count arriving after the query was issued again is not answered.
    fn query_result(&mut self, query: u32) -> Option<u64> {
        let generation = self.generation.get(&query).copied().unwrap_or(0);
        if let Some(&(g, v)) = self.results.lock().unwrap().get(&query) {
            if g == generation {
                return v;
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
    fn release(&mut self, resource: u32) { self.enqueue(move |b| b.release(resource)); }
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
}
