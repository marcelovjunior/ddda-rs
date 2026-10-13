//! Opt-in synchronization census. Times include dispatcher lock/scheduling costs;
//! contention is an observed primitive miss, not inferred from elapsed time.
use ddnative_base::{mem::Mem, x86::Cpu};
use std::{cell::{Cell, RefCell}, collections::HashMap, sync::{Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering::Relaxed}}, time::{Duration, Instant}};

static FRAMES: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicU64 = AtomicU64::new(0);
static WORKERS: AtomicU64 = AtomicU64::new(0);
static MAX_JOBS: AtomicU64 = AtomicU64::new(0);
const LIMIT: usize = 16384;
type Key = (u32, u32, u32, u32); // tid, service, return address, first argument
#[derive(Default)]
struct Counts { calls: AtomicU64, ns: AtomicU64, max_ns: AtomicU64,
    contended: AtomicU64, blocks: AtomicU64, timeouts: AtomicU64,
    service_contended: AtomicU64, histogram: [AtomicU64; 16] }
struct Row { key: Key, name: String, counts: Counts, reported_calls: AtomicU64 }
static ROWS: Mutex<Vec<Arc<Row>>> = Mutex::new(Vec::new());
thread_local! {
    static CACHE: RefCell<HashMap<Key, Arc<Row>>> = RefCell::new(HashMap::new());
    static MISSES: Cell<u64> = const { Cell::new(0) };
    static BLOCKS: Cell<u64> = const { Cell::new(0) };
    static LOCK_MISSES: Cell<u64> = const { Cell::new(0) };
    static ACTIVE_JOB: Cell<(u32, u32)> = const { Cell::new((0, 0)) }; // esp, return site
}
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("DDRUNTIME_SYNC_PROFILE").is_ok_and(|v| v == "1");
        if on {
            std::thread::Builder::new().name("sync-census".into()).spawn(|| loop {
                std::thread::sleep(Duration::from_secs(2));
                report();
            }).expect("sync census reporter");
        }
        on
    })
}
pub(crate) fn contention() { if enabled() { MISSES.with(|n| n.set(n.get().wrapping_add(1))); } }
pub(crate) fn blocking() { if enabled() { BLOCKS.with(|n| n.set(n.get().wrapping_add(1))); } }
pub(crate) fn service_contention() { if enabled() { LOCK_MISSES.with(|n| n.set(n.get().wrapping_add(1))); } }

fn selected(name: &str) -> bool {
    let name = name.split('!').next_back().unwrap_or(name);
    name.starts_with("Interlocked") || name.starts_with("Tls") ||
    name.contains("CriticalSection") || name.contains("Semaphore") || name.contains("Mutex") ||
    matches!(name, "WaitForSingleObject" | "WaitForMultipleObjects" | "WaitForSingleObjectEx" |
        "WaitForMultipleObjectsEx" | "SetEvent" | "ResetEvent" | "CreateEventA" |
        "CreateEventW" | "Sleep" | "SleepEx" | "SwitchToThread")
}

fn argument_key(name: &str, argument: u32) -> u32 {
    // Interlocked operands are often short-lived object fields. Keeping each
    // address produced ~15,000 rows and multi-second stderr snapshots in c26.
    // They have no measurable primitive-miss outcome here. Preserve site/tid
    // totals; retain actual handles/CS pointers/TLS indices for other services.
    if name.split('!').next_back().unwrap_or(name).starts_with("Interlocked") { 0 } else { argument }
}

pub(crate) struct Sample { row: Arc<Row>, start: Instant, misses: u64, blocks: u64, lock_misses: u64 }
pub(crate) fn begin(name: Option<&str>, target: u32, m: &dyn Mem, cpu: &Cpu) -> Option<Sample> {
    if !enabled() { return None; }
    let name = name?;
    if !selected(name) { return None; }
    // Diagnostic reads must not introduce a fault before the service does.
    let word = |a| m.probe(a, 4, false).ok().map_or(0, |_| m.u32(a));
    let key = (super::current_tid(), target, word(cpu.esp), argument_key(name, word(cpu.esp.wrapping_add(4))));
    // Observational fields at the fan-out join, from 0x00DBE560's disassembly:
    // ESI is the scheduler; +0x98 is participation count, +0x8c queued jobs.
    if key.2 == 0x00dbe66d && name.ends_with("!WaitForMultipleObjects") {
        WORKERS.store(word(cpu.esi.wrapping_add(0x98)) as u64, Relaxed);
        MAX_JOBS.fetch_max(word(cpu.esi.wrapping_add(0x8c)) as u64, Relaxed);
    }
    start(name, key)
}
fn start(name: &str, key: Key) -> Option<Sample> {
    let row = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(row) = cache.get(&key) { return Some(row.clone()); }
        let mut rows = ROWS.lock().unwrap_or_else(|e| e.into_inner());
        if rows.len() == LIMIT { OVERFLOW.fetch_add(1, Relaxed); return None; }
        let row = Arc::new(Row { key, name: name.into(), counts: Counts::default(), reported_calls: 0.into() });
        rows.push(row.clone());
        cache.insert(key, row.clone());
        Some(row)
    })?;
    Some(Sample { row, start: Instant::now(), misses: MISSES.with(Cell::get), blocks: BLOCKS.with(Cell::get), lock_misses: LOCK_MISSES.with(Cell::get) })
}

/// Observational call-site filter: main 0x00DBD8D0 and worker 0x00DBDC90
/// (DDDA.exe 2017-12-13 Steam; disassembly / verified lifted control flow).
/// Both have four indirect job returns, excluding the preceding observer
/// callbacks. c26 instrumented only the main dispatcher; its absence of worker
/// rows was a coverage gap, not evidence that workers execute no jobs.
fn job_site(site: u32) -> bool {
    matches!(site, 0x00dbd958 | 0x00dbd9cb | 0x00dbda4c | 0x00dbdabc |
        0x00dbdd4c | 0x00dbddbf | 0x00dbde40 | 0x00dbdead)
}
pub(crate) struct JobSample {
    sample: Option<Sample>,
    previous: (u32, u32),
}
impl Drop for JobSample {
    fn drop(&mut self) { ACTIVE_JOB.with(|a| a.set(self.previous)); }
}

pub(crate) fn begin_job(target: u32, m: &dyn Mem, cpu: &Cpu) -> Option<JobSample> {
    if !enabled() || m.probe(cpu.esp, 4, false).is_err() { return None; }
    let site = m.u32(cpu.esp);
    if !job_site(site) { return None; }
    let frame = (cpu.esp, site);
    // A lifted tail jump redispatches with the same guest return slot. Count
    // the outer job once, not each tail target (e.g. 0x00B54050 -> 0x00B66110,
    // DDDA.exe 2017-12-13 Steam). A nested scheduler call has a different ESP.
    if ACTIVE_JOB.with(Cell::get) == frame { return None; }
    // Diagnostics must respect the same host FPU boundary as services.
    ddnative_base::x87::park();
    let mxcsr = ddnative_base::x86::stmxcsr();
    ddnative_base::x86::ldmxcsr(ddnative_base::x86::MXCSR_DEFAULT);
    let sample = start("scheduler-job", (super::current_tid(), target, site, 0));
    ddnative_base::x86::ldmxcsr(mxcsr);
    sample.map(|sample| JobSample {
        sample: Some(sample), previous: ACTIVE_JOB.with(|a| a.replace(frame)),
    })
}
pub(crate) fn finish_job(mut sample: JobSample) {
    ddnative_base::x87::park();
    let mxcsr = ddnative_base::x86::stmxcsr();
    ddnative_base::x86::ldmxcsr(ddnative_base::x86::MXCSR_DEFAULT);
    sample.sample.take().unwrap().finish(0);
    ddnative_base::x86::ldmxcsr(mxcsr);
}
impl Sample {
    pub(crate) fn finish(self, result: u32) {
        let ns = self.start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let c = &self.row.counts;
        c.ns.fetch_add(ns, Relaxed);
        c.max_ns.fetch_max(ns, Relaxed);
        c.contended.fetch_add(u64::from(MISSES.with(Cell::get) != self.misses), Relaxed);
        c.service_contended.fetch_add(u64::from(LOCK_MISSES.with(Cell::get) != self.lock_misses), Relaxed);
        c.blocks.fetch_add(BLOCKS.with(Cell::get).wrapping_sub(self.blocks), Relaxed);
        c.histogram[bucket(ns)].fetch_add(1, Relaxed);
        if self.row.name.contains("WaitFor") && result == super::WAIT_TIMEOUT { c.timeouts.fetch_add(1, Relaxed); }
        c.calls.fetch_add(1, Relaxed);
    }
}
// Log2 microsecond buckets: <1us, <2us, ... <16384us, >=16384us.
fn bucket(ns: u64) -> usize { (64 - (ns / 1000).leading_zeros() as usize).min(15) }
pub(crate) fn presented(name: Option<&str>, result: u32) {
    if enabled() && result == 0 && name.is_some_and(|n|
        n.starts_with("IDirect3DDevice9::Present (") || n.starts_with("IDirect3DSwapChain9::Present (")) {
        FRAMES.fetch_add(1, Relaxed);
    }
}
fn report() {
    // Clone ownership under the registry lock; formatting/I/O never holds it.
    let rows = ROWS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let frames = FRAMES.load(Relaxed);
    static START: OnceLock<Instant> = OnceLock::new();
    static REPORTS: AtomicU64 = AtomicU64::new(0);
    let full = REPORTS.fetch_add(1, Relaxed) % 30 == 0;
    eprintln!("sync-census frames={frames} rows={} overflow={} cumulative=1 sparse={} report_ms={} participants={} max_jobs={}",
        rows.len(), OVERFLOW.load(Relaxed), u32::from(!full), START.get_or_init(Instant::now).elapsed().as_millis(), WORKERS.load(Relaxed), MAX_JOBS.load(Relaxed));
    for row in rows {
        let (tid, target, site, arg0) = row.key;
        let c = &row.counts;
        let calls = c.calls.load(Relaxed);
        // Rows contain cumulative counters; omitted rows retain their last
        // completed snapshot values (sparse=1). Do not reprint thousands of
        // startup-only CS/allocator rows throughout gameplay.
        if calls == 0 || (!full && calls == row.reported_calls.load(Relaxed)) { continue; }
        let hist: Vec<_> = c.histogram.iter().map(|n| n.load(Relaxed)).collect();
        eprintln!("sync-row tid={tid:#x} target={target:#x} site={site:#x} arg0={arg0:#x} calls={calls} ns={} max_ns={} contended={} service_contended={} blocks={} timeouts={} hist_us={hist:?} name={}",
            c.ns.load(Relaxed), c.max_ns.load(Relaxed), c.contended.load(Relaxed), c.service_contended.load(Relaxed), c.blocks.load(Relaxed), c.timeouts.load(Relaxed), row.name);
        row.reported_calls.store(calls, Relaxed);
    }
    eprintln!("sync-census-end frames={frames}");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn census_selects_only_sync_services() {
        for n in ["WaitForMultipleObjects", "InterlockedCompareExchange", "TlsGetValue", "Sleep", "SwitchToThread", "ReleaseSemaphore", "EnterCriticalSection"] {
            assert!(selected(&format!("KERNEL32.dll!{n}")));
        }
        assert!(!selected("KERNEL32.dll!ReadFile"));
        assert!(!selected("IDirect3DDevice9::Present (slot 17)"));
    }
    #[test]
    fn census_counts_observed_contention_and_timeout() {
        let row = Arc::new(Row { key: (1,2,3,4), name: "WaitForSingleObject".into(), counts: Counts::default(), reported_calls: 0.into() });
        let sample = Sample { row: row.clone(), start: Instant::now(), misses: MISSES.with(Cell::get), blocks: BLOCKS.with(Cell::get), lock_misses: LOCK_MISSES.with(Cell::get) };
        MISSES.with(|n| n.set(n.get() + 1));
        BLOCKS.with(|n| n.set(n.get() + 2));
        sample.finish(super::super::WAIT_TIMEOUT);
        assert_eq!(row.counts.calls.load(Relaxed), 1);
        assert_eq!(row.counts.contended.load(Relaxed), 1);
        assert_eq!(row.counts.blocks.load(Relaxed), 2);
        assert_eq!(row.counts.timeouts.load(Relaxed), 1);
    }
    #[test]
    fn job_filter_and_duration_buckets() {
        for site in [0xdbd958, 0xdbd9cb, 0xdbda4c, 0xdbdabc, 0xdbdd4c, 0xdbddbf, 0xdbde40, 0xdbdead] { assert!(job_site(site)); }
        for observer in [0xdbd951, 0xdbd9c3, 0xdbda41, 0xdbdab0, 0xdbdd45, 0xdbddb7, 0xdbde35, 0xdbdea1] { assert!(!job_site(observer)); }
        assert_eq!(argument_key("KERNEL32.dll!InterlockedIncrement", 0x1234), 0);
        for name in ["WaitForSingleObject", "SetEvent", "EnterCriticalSection", "TlsGetValue"] {
            assert_eq!(argument_key(name, 0x1234), 0x1234);
        }
        assert_eq!([0, 999, 1000, 1999, 2000, u64::MAX].map(bucket), [0, 0, 1, 1, 2, 15]);
    }
    #[test]
    fn job_sample_keeps_guest_rounding_and_counts_completion() {
        // Also run in the opt-in subprocess gate; default diagnostics stay off.
        if !enabled() { return; }
        use ddnative_base::x86;
        struct Restore(u32);
        impl Drop for Restore { fn drop(&mut self) { x86::ldmxcsr(self.0); } }
        let _restore = Restore(x86::stmxcsr());
        let mut m = crate::memory::GuestMemory::new();
        m.map(0x10000, 4096, crate::memory::READ | crate::memory::WRITE).unwrap();
        let cpu = Cpu { esp: 0x10000, ..Cpu::default() };
        let guest = x86::MXCSR_DEFAULT | 0x6000;
        for site in [0xdbd958, 0xdbd9cb, 0xdbda4c, 0xdbdabc, 0xdbdd4c, 0xdbddbf, 0xdbde40, 0xdbdead] {
            m.set_u32(0x10000, site);
            x86::ldmxcsr(guest);
            let sample = begin_job(0x12345678, &m, &cpu).unwrap();
            assert_eq!(x86::stmxcsr(), guest);
            let row = sample.sample.as_ref().unwrap().row.clone();
            let before = row.counts.calls.load(Relaxed);
            assert!(begin_job(0x87654321, &m, &cpu).is_none(), "tail target is the same job frame");
            // A genuine nested dispatch has its own stack slot; unwind it
            // without completing to verify restoration of the outer marker.
            m.set_u32(0x10004, site);
            let nested_cpu = Cpu { esp: 0x10004, ..Cpu::default() };
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _nested = begin_job(0x12345679, &m, &nested_cpu).unwrap();
                panic!("synthetic nested job fault");
            }));
            assert!(unwind.is_err());
            assert!(begin_job(0x87654321, &m, &cpu).is_none());
            // Host panic formatting may touch MXCSR; reestablish the guest
            // mode before testing finish_job's boundary.
            x86::ldmxcsr(guest);
            finish_job(sample);
            assert_eq!(x86::stmxcsr(), guest);
            assert_eq!(row.counts.calls.load(Relaxed), before + 1);
            assert_eq!(ACTIVE_JOB.with(Cell::get), (0, 0));
        }
    }
}
