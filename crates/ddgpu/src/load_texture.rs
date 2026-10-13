//! Optional backend texture CPU wall costs, separate from producer enqueue.
//! Counts complete operations. Snapshot reads are approximate across threads;
//! timings are inclusive host durations, never GPU execution timestamps.
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering::Relaxed}};
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static COUNTERS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];

pub(crate) fn begin() -> Option<Instant> {
    (*ENABLED.get_or_init(|| std::env::var("DDRUNTIME_LOAD_LOG").as_deref() == Ok("1")))
        .then(Instant::now)
}

/// Creation attempts, failures, creation ns, completed writes, prepared bytes,
/// preparation ns, finisher wait ns, queue.write_texture ns, total write ns.
pub fn snapshot() -> [u64; 9] {
    std::array::from_fn(|i| COUNTERS[i].load(Relaxed))
}

fn add(values: [u64; 9]) {
    for (counter, value) in COUNTERS.iter().zip(values) {
        if value != 0 { counter.fetch_add(value, Relaxed); }
    }
}

pub(crate) fn created(start: Option<Instant>, success: bool) {
    if let Some(start) = start {
        add([1, u64::from(!success), start.elapsed().as_nanos() as u64, 0, 0, 0, 0, 0, 0]);
    }
}

pub(crate) struct Write {
    start: Option<Instant>,
    prepared: Option<Instant>,
    waited: Option<Instant>,
}

impl Write {
    pub(crate) fn begin() -> Self {
        Self { start: begin(), prepared: None, waited: None }
    }
    pub(crate) fn prepared(&mut self) {
        self.prepared = self.start.map(|_| Instant::now());
    }
    pub(crate) fn waited(&mut self) {
        self.waited = self.start.map(|_| Instant::now());
    }
    pub(crate) fn finish(self, bytes: usize) {
        if let (Some(start), Some(prepared), Some(waited)) = (self.start, self.prepared, self.waited) {
            add(write_values(start, prepared, waited, Instant::now(), bytes));
        }
    }
}

fn write_values(start: Instant, prepared: Instant, waited: Instant, end: Instant, bytes: usize) -> [u64; 9] {
    [0, 0, 0, 1, bytes as u64, prepared.duration_since(start).as_nanos() as u64,
        waited.duration_since(prepared).as_nanos() as u64,
        end.duration_since(waited).as_nanos() as u64, end.duration_since(start).as_nanos() as u64]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_phases_partition_completed_operation_without_double_counting() {
        let start = Instant::now();
        let at = |n| start + std::time::Duration::from_nanos(n);
        let values = write_values(start, at(11), at(18), at(31), 4096);
        assert_eq!(values, [0, 0, 0, 1, 4096, 11, 7, 13, 31]);
        assert_eq!(values[5] + values[6] + values[7], values[8]);
    }
}
