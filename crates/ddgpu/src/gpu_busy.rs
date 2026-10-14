//! L192 measurement (`DDGPU_BUSY_STATS=1`, off by default): GPU busy time per
//! presented frame, from submission-completion callbacks. A submit's busy span
//! runs from max(its submission, the previous completion) to its completion,
//! so queued work is not double counted and idle gaps are not counted.
//! One line per second: busy ms per second, per present, submits, presents.
use std::sync::{atomic::{AtomicU64, Ordering::Relaxed}, Mutex, OnceLock};
use std::time::Instant;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDGPU_BUSY_STATS").as_deref() == Ok("1"))
}

struct Window {
    last_done: Option<Instant>,
    busy_ns: u64,
    submits: u64,
    since: Instant,
}
static WINDOW: Mutex<Option<Window>> = Mutex::new(None);
static PRESENTS: AtomicU64 = AtomicU64::new(0);

/// Call right after a `queue.submit`. `present`: this submit ends a frame.
pub(crate) fn submitted(queue: &wgpu::Queue, present: bool) {
    if !enabled() {
        return;
    }
    if present {
        PRESENTS.fetch_add(1, Relaxed);
    }
    let at = Instant::now();
    queue.on_submitted_work_done(move || {
        let now = Instant::now();
        let mut guard = WINDOW.lock().unwrap_or_else(|e| e.into_inner());
        let w = guard.get_or_insert_with(|| Window { last_done: None, busy_ns: 0, submits: 0, since: now });
        let start = w.last_done.map_or(at, |d| d.max(at));
        if now > start {
            w.busy_ns += (now - start).as_nanos() as u64;
        }
        w.last_done = Some(now);
        w.submits += 1;
        if now.duration_since(w.since).as_secs_f64() >= 1.0 {
            let presents = PRESENTS.swap(0, Relaxed).max(1);
            let wall = now.duration_since(w.since).as_secs_f64();
            eprintln!(
                "gpu-busy: {:.1} ms busy per {:.2} s ({:.0}% of wall), {} presents -> {:.2} ms busy per present, {} submits",
                w.busy_ns as f64 / 1e6, wall, w.busy_ns as f64 / 1e9 / wall * 100.0, presents,
                w.busy_ns as f64 / 1e6 / presents as f64, w.submits
            );
            *guard = Some(Window { last_done: Some(now), busy_ns: 0, submits: 0, since: now });
        }
    });
}
