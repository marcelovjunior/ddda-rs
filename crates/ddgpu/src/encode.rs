//! Ordered parallel finishing of wgpu 29's deferred command streams.
//!
//! wgpu-core 29 command/mod.rs::CommandEncoder::encode_commands invokes
//! render::encode_render_pass from finish(), not from the public draw APIs.
//! Each worker owns an encoder; only the coordinator ever submits.
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

pub(crate) const STAGES: [&str; 10] = [
    "record_begin", "record_end", "record_pipeline", "record_groups",
    "record_vertex", "record_draw_index", "record_dynamic", "finish_cpu",
    "submit", "finish_wall",
];

#[derive(Clone, Default)]
pub(crate) struct Profile(Option<Arc<Mutex<[Histogram; 10]>>>);

#[derive(Clone, Copy, Default)]
struct Histogram {
    calls: u64,
    ns: u64,
    // Upper bounds in powers of two nanoseconds (last bucket saturates).
    bins: [u64; 32],
}

impl Histogram {
    fn add(&mut self, ns: u64) {
        self.calls += 1;
        self.ns += ns;
        let bin = (64 - ns.saturating_sub(1).leading_zeros()).min(31) as usize;
        self.bins[bin] += 1;
    }

    fn percentile(&self, percent: u64) -> u64 {
        let wanted = (self.calls * percent).div_ceil(100);
        if wanted == 0 { return 0; }
        let mut count = 0;
        for (i, n) in self.bins.iter().enumerate() {
            count += n;
            if count >= wanted { return if i == 31 { u64::MAX } else { 1u64 << i }; }
        }
        u64::MAX
    }
}

impl Profile {
    pub(crate) fn from_env() -> Self {
        Self((std::env::var("DDGPU_ENCODE_PROFILE").as_deref() == Ok("1"))
            .then(|| Arc::new(Mutex::new([Histogram::default(); 10]))))
    }

    #[inline]
    pub(crate) fn start(&self) -> Option<Instant> {
        self.0.as_ref().map(|_| Instant::now())
    }

    #[inline]
    pub(crate) fn end(&self, stage: usize, start: Option<Instant>) {
        if let (Some(stats), Some(start)) = (&self.0, start) {
            let ns = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            stats.lock().unwrap()[stage].add(ns);
        }
    }

    // Called after the frame's finisher fence, so no sample straddles frames.
    pub(crate) fn present(&self) {
        let Some(stats) = &self.0 else { return };
        let snapshot = std::mem::take(&mut *stats.lock().unwrap());
        let fields: Vec<_> = STAGES.iter().zip(snapshot).map(|(name, h)| {
            format!("{name}={:.3}ms/{} p50<={}ns p95<={}ns",
                h.ns as f64 / 1e6, h.calls, h.percentile(50), h.percentile(95))
        }).collect();
        eprintln!("[ddgpu-encode frame] {}", fields.join("; "));
    }
}

pub(crate) fn threads_from_env() -> usize {
    parse_threads(std::env::var("DDGPU_ENCODE_THREADS").ok().as_deref())
}

// Default 2: combo22 in game (heavy scene) ran 21.0 fps with 2 encoder
// threads, 18.7 with 1 and 17.4 with 4 (contention on a shared machine).
pub(crate) const DEFAULT_THREADS: usize = 2;

fn parse_threads(value: Option<&str>) -> usize {
    value.and_then(|v| v.parse().ok()).filter(|n| (1..=4).contains(n)).unwrap_or(DEFAULT_THREADS)
}

type Finished<T> = Result<Vec<T>, Box<dyn std::any::Any + Send>>;

struct Work<C, R> {
    encoder: C,
    reply: mpsc::Sender<(usize, Finished<R>)>,
    index: usize,
}

pub(crate) struct Pool<C, R> {
    workers: Vec<(mpsc::Sender<Work<C, R>>, std::thread::JoinHandle<()>)>,
    profile: Profile,
    encode: fn(C) -> Vec<R>,
}

impl<C: Send + 'static, R: Send + 'static> Pool<C, R> {
    /// A chunk may expand into several ordered buffers (e.g. barriers + raw
    /// commands). The callback must only encode; queue writes/submits remain
    /// outside the pool. Results are flattened in input order, never arrival order.
    pub(crate) fn new(threads: usize, profile: Profile, encode: fn(C) -> Vec<R>) -> Self {
        let workers = if threads <= 1 { Vec::new() } else {
            (0..threads).map(|id| {
                let (send, recv) = mpsc::channel::<Work<C, R>>();
                let profile = profile.clone();
                let thread = std::thread::Builder::new().name(format!("wgpu-encode-{id}"))
                    .spawn(move || {
                        for work in recv {
                            // Propagate validation panics to the coordinator instead of
                            // leaving its fence permanently unsignalled.
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                measure(work.encoder, &profile, encode)
                            }));
                            if work.reply.send((work.index, result)).is_err() { break; }
                        }
                    }).expect("start encoder worker");
                (send, thread)
            }).collect()
        };
        Self { workers, profile, encode }
    }

    pub(crate) fn finish(&self, encoders: Vec<C>) -> Vec<R> {
        let start = self.profile.start();
        let result = if self.workers.is_empty() || encoders.len() <= 1 {
            encoders.into_iter().flat_map(|enc| measure(enc, &self.profile, self.encode)).collect()
        } else {
            let mut ordered: Vec<_> = (0..encoders.len()).map(|_| None).collect();
            let (send, recv) = mpsc::channel();
            for (index, encoder) in encoders.into_iter().enumerate() {
                self.workers[index % self.workers.len()].0.send(Work {
                    encoder, reply: send.clone(), index,
                }).expect("encoder worker stopped");
            }
            drop(send);
            for _ in 0..ordered.len() {
                let (index, result) = recv.recv().expect("encoder worker lost a result");
                ordered[index] = Some(match result {
                    Ok(buffer) => buffer,
                    Err(panic) => std::panic::resume_unwind(panic),
                });
            }
            ordered.into_iter().flat_map(Option::unwrap).collect()
        };
        self.profile.end(9, start);
        result
    }
}

pub(crate) fn finish_wgpu(encoder: wgpu::CommandEncoder) -> Vec<wgpu::CommandBuffer> {
    vec![encoder.finish()]
}

fn measure<C, R>(encoder: C, profile: &Profile, encode: fn(C) -> Vec<R>) -> Vec<R> {
    let start = profile.start();
    let buffer = encode(encoder);
    profile.end(7, start);
    buffer
}

impl<C, R> Drop for Pool<C, R> {
    fn drop(&mut self) {
        let threads: Vec<_> = self.workers.drain(..).map(|(send, thread)| {
            drop(send);
            thread
        }).collect();
        for thread in threads { let _ = thread.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_and_defaults_to_two() {
        for value in [None, Some(""), Some("0"), Some("5"), Some("-1"), Some("foo")] {
            assert_eq!(parse_threads(value), DEFAULT_THREADS);
        }
        for n in 1..=4 { assert_eq!(parse_threads(Some(&n.to_string())), n); }
    }

    #[test]
    fn chunks_finish_out_of_order_but_expand_in_input_order() {
        fn encode(n: u32) -> Vec<u32> {
            if n == 0 { std::thread::sleep(std::time::Duration::from_millis(10)); }
            vec![n * 2, n * 2 + 1]
        }
        for threads in [1, 2, 4] {
            let pool = Pool::new(threads, Profile::default(), encode);
            for _ in 0..3 {
                assert_eq!(pool.finish((0..9).collect()), (0..18).collect::<Vec<_>>());
            }
            assert!(pool.finish(vec![]).is_empty());
        }
    }

    #[test]
    fn worker_panic_reaches_coordinator_without_a_deadlock() {
        let pool = Pool::new(2, Profile::default(), |n: u32| -> Vec<u32> {
            assert_ne!(n, 1, "test encoder error");
            vec![n]
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.finish(vec![0, 1])));
        assert!(result.is_err());
    }

    #[test]
    fn histogram_bounds_and_counts() {
        let mut h = Histogram::default();
        for ns in [0, 1, 2, 3, 4, 5, 8, 9] { h.add(ns); }
        assert_eq!(h.calls, 8);
        assert_eq!(h.ns, 32);
        assert_eq!(h.percentile(50), 4);
        assert_eq!(h.percentile(95), 16);
    }
}
