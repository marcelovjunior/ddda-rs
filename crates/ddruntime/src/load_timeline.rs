//! Transition deltas. CPU is Windows kernel+user time; *_ns costs are
//! inclusive wall durations and overlap CPU/each other. No guest mutation.
use super::*;
use std::sync::Mutex;

/// modules, module_ns, translate_ns, pipelines, pipeline_ns. The frontend
/// installs a read-only atomic snapshot; no renderer dependency in runtime.
pub type GpuSnapshot = fn() -> [u64; 5];
static GPU: OnceLock<GpuSnapshot> = OnceLock::new();
pub fn set_gpu_snapshot(snapshot: GpuSnapshot) { let _ = GPU.set(snapshot); }
pub fn register_main_thread() { register_thread("main"); }
static STAGE_STARTED: AtomicBool = AtomicBool::new(false);
static STAGE_PRESENTED: AtomicBool = AtomicBool::new(false);
static GAMEPLAY_STARTED: AtomicBool = AtomicBool::new(false);
static GAMEPLAY_PRESENTED: AtomicBool = AtomicBool::new(false);
#[cfg(any(feature = "game", test))]
pub(super) fn stage_started() { STAGE_STARTED.store(true, Relaxed); }
#[cfg(any(feature = "game", test))]
pub(super) fn gameplay_started() { GAMEPLAY_STARTED.store(true, Relaxed); }
pub(crate) fn present() {
    if enabled() && STAGE_STARTED.load(Relaxed) && !STAGE_PRESENTED.swap(true, Relaxed) {
        transition("first_stage_present", format_args!("boundary=first_present_after_stage_update player_control=unverified"));
    }
    if enabled() && GAMEPLAY_STARTED.load(Relaxed) && !GAMEPLAY_PRESENTED.swap(true, Relaxed) {
        transition("first_gameplay_present", format_args!("boundary=first_present_after_stage_mode_2 player_control=unverified"));
    }
}
static UPLOADS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
static EXTRA: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
pub(crate) fn archive_io(start: Option<Instant>, bytes: u64) {
    if let Some(start) = start {
        for (slot, value) in EXTRA[..3].iter().zip([1, bytes, start.elapsed().as_nanos() as u64]) { slot.fetch_add(value, Relaxed); }
    }
}
#[cfg(feature = "emu")]
pub(crate) fn emulated(start: Option<Instant>) {
    if let Some(start) = start {
        EXTRA[3].fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
    }
}
thread_local! { static SERVICE_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
pub(crate) struct ServiceSample(Option<Instant>);
impl ServiceSample {
    pub(crate) fn begin() -> Self {
        if !enabled() { return Self(None); }
        let sampled = SERVICE_COUNT.with(|count| {
            let n = count.get(); count.set(n.wrapping_add(1)); n & 255 == 0
        });
        Self(sampled.then(Instant::now))
    }
}
impl Drop for ServiceSample {
    fn drop(&mut self) {
        if let Some(start) = self.0 {
            EXTRA[4].fetch_add(1, Relaxed);
            EXTRA[5].fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
        }
    }
}
pub(crate) fn texture_upload(start: Option<Instant>, bytes: u64) {
    if let Some(start) = start {
        for (slot, value) in UPLOADS.iter().zip([1, bytes, start.elapsed().as_nanos() as u64]) {
            slot.fetch_add(value, Relaxed);
        }
    }
}

#[derive(Default)]
struct Interval { time: f64, name: String, counters: [u64; 30], cpu: [u64; 7] }
static INTERVAL: Mutex<Option<Interval>> = Mutex::new(None);
const NAMES: [&str; 30] = ["reads", "bytes", "read_ns", "archive_reads", "archive_ns",
    "inflates", "inflate_ns", "zlib_calls", "zlib_ns", "zlib_fast_ns", "adler_ns",
    "window_ns", "geometry_ns", "open_ns", "vorbis_ns", "modules", "module_ns",
    "translate_ns", "pipelines", "pipeline_ns", "texture_uploads", "texture_bytes", "texture_enqueue_ns",
    "archive_io_reads", "archive_io_bytes", "archive_io_ns", "emu_engine_ns",
    "service_samples", "service_sample_ns", "zlib_table_ns"];
const CLASSES: [&str; 7] = ["main", "render", "jobs", "loader_io", "host_main", "backend", "other"];
fn delta<const N: usize>(now: [u64; N], before: [u64; N]) -> [u64; N] {
    std::array::from_fn(|i| now[i].saturating_sub(before[i]))
}

/// One summary per transition. Only host telemetry locks are acquired. GPU
/// providers must never acquire guest/device locks (transitions run in both).
pub fn transition(name: &str, details: std::fmt::Arguments<'_>) {
    if !enabled() { return; }
    // Wrappers run with guest floating-point state live. Formatting a timestamp
    // must not set guest sticky bits or inherit its rounding/exception masks.
    struct Restore(u32);
    impl Drop for Restore { fn drop(&mut self) { ddnative_base::x86::ldmxcsr(self.0); } }
    let _restore = Restore(ddnative_base::x86::stmxcsr());
    ddnative_base::x87::park();
    ddnative_base::x86::ldmxcsr(ddnative_base::x86::MXCSR_DEFAULT);
    let mut previous = INTERVAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut counters = [0; 30];
    counters[..15].copy_from_slice(&interval_counters());
    if let Some(snapshot) = GPU.get() { counters[15..20].copy_from_slice(&snapshot()); }
    for i in 0..3 { counters[20+i] = UPLOADS[i].load(Relaxed); }
    for i in 0..6 { counters[23+i] = EXTRA[i].load(Relaxed); }
    counters[29] = ZLIB_HELPER_NS[4].load(Relaxed);
    let cpu = cpu_snapshot(name == "first_present");
    let now = seconds();
    let old = previous.take().unwrap_or_default();
    let mut fields = String::new();
    use std::fmt::Write;
    for (key, value) in NAMES.iter().zip(delta(counters, old.counters)) {
        let _ = write!(fields, " {key}={value}");
    }
    for (key, value) in CLASSES.iter().zip(delta(cpu, old.cpu)) {
        let _ = write!(fields, " cpu_{key}_ns={value}");
    }
    eprintln!("load-log t={now:.6} event=interval from={} to={name} wall_ns={} gpu_available={} cpu_available={} emu_available={} costs=inclusive service_sample_stride=256 lifted_ns=unavailable {details}{fields}",
        if old.name.is_empty() { "process_start" } else { &old.name },
        ((now-old.time).max(0.0)*1e9) as u64, GPU.get().is_some(), cfg!(windows), cfg!(feature = "emu"));
    *previous = Some(Interval { time: now, name: name.into(), counters, cpu });
}

// Sources: CreateThread sites recorded in docs/RUNTIME.md, DDDA.exe
// 2017-12-13 Steam. Unknown entry points remain explicitly unclassified.
pub(crate) fn guest_thread(entry: u32) {
    register_thread(match entry {
        0x00dac090 => "render",
        0x00dbdef0 | 0x00dbdf10 => "jobs",
        0x00dba9a0 | 0x00db9f40 => "loader_io",
        _ => "other",
    });
}

#[cfg(windows)]
pub(crate) use windows_cpu::register_thread;
#[cfg(windows)]
use windows_cpu::cpu_snapshot;
#[cfg(not(windows))]
pub(crate) fn register_thread(_: &str) {}
#[cfg(not(windows))]
fn cpu_snapshot(_: bool) -> [u64; 7] { [0; 7] }

#[cfg(windows)]
mod windows_cpu {
    use super::*;
    use std::collections::HashMap;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime { low: u32, high: u32 }
    impl FileTime { fn ns(&self) -> u64 { ((u64::from(self.high)<<32)|u64::from(self.low))*100 } }
    #[repr(C)]
    struct Entry { size: u32, usage: u32, tid: u32, pid: u32, base: i32, delta: i32, flags: u32 }
    #[link(name="kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(h: isize, c: *mut FileTime, e: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
        fn OpenThread(access: u32, inherit: i32, id: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn GetThreadTimes(h: isize, c: *mut FileTime, e: *mut FileTime, k: *mut FileTime, u: *mut FileTime) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Thread32First(h: isize, entry: *mut Entry) -> i32;
        fn Thread32Next(h: isize, entry: *mut Entry) -> i32;
        fn GetThreadDescription(h: isize, description: *mut *mut u16) -> i32;
        fn LocalFree(p: *mut u16) -> isize;
    }
    struct Thread { handle: isize, class: usize, last: u64 }
    impl Drop for Thread { fn drop(&mut self) { unsafe { CloseHandle(self.handle); } } }
    static DISCOVERED: AtomicBool = AtomicBool::new(false);
    static THREADS: OnceLock<Mutex<HashMap<(u32, u64), Thread>>> = OnceLock::new();
    fn threads() -> &'static Mutex<HashMap<(u32, u64), Thread>> { THREADS.get_or_init(Default::default) }
    pub(crate) fn register_thread(class: &str) {
        if !enabled() { return; }
        let mut threads = threads().lock().unwrap_or_else(|e| e.into_inner());
        // Owned real handle retains final CPU totals even after thread exit.
        unsafe {
            let id = GetCurrentThreadId();
            let class = CLASSES.iter().position(|s| *s == class).unwrap_or(6);
            let h = OpenThread(0x0800, 0, id); // THREAD_QUERY_LIMITED_INFORMATION
            if h != 0 {
                let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
                if GetThreadTimes(h, &mut c, &mut e, &mut k, &mut u) == 0 { CloseHandle(h); return; }
                // A retained handle belongs to a thread object, while Windows
                // may reuse its numeric ID after exit. Preserve both lifetimes.
                let key = (id, c.ns());
                if let Some(t) = threads.get_mut(&key) { t.class = class; CloseHandle(h); }
                else { threads.insert(key, Thread { handle: h, class, last: k.ns() + u.ns() }); }
            }
        }
    }
    pub(super) fn cpu_snapshot(discover: bool) -> [u64; 7] {
        let mut threads = threads().lock().unwrap_or_else(|e| e.into_inner());
        // Discover host backend/encoder/audio threads ONCE, at first Present,
        // when the renderer exists. Toolhelp enumerates the whole machine and
        // previously dominated frequent title/stage transitions (c30load).
        // Guest threads register at entry; all later samples query cached
        // handles. Unregistered host CPU remains in the process residual.
        unsafe {
            let snapshot = if discover && !DISCOVERED.swap(true, Relaxed) {
                CreateToolhelp32Snapshot(4, 0)
            } else { -1 };
            if snapshot != -1 {
                let mut e = Entry { size: std::mem::size_of::<Entry>() as u32, usage: 0, tid: 0, pid: 0, base: 0, delta: 0, flags: 0 };
                let mut ok = Thread32First(snapshot, &mut e);
                while ok != 0 {
                    if e.pid == GetCurrentProcessId() {
                        let h = OpenThread(0x0800, 0, e.tid);
                        if h != 0 {
                            let mut p = std::ptr::null_mut();
                            let mut class = 6;
                            if GetThreadDescription(h, &mut p) >= 0 && !p.is_null() {
                                let mut n = 0;
                                while *p.add(n) != 0 { n += 1; }
                                let name = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                                if name.starts_with("d3d9-backend") || name.starts_with("wgpu-") { class = 5; }
                                LocalFree(p);
                            }
                            let (mut c, mut exit, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
                            if GetThreadTimes(h, &mut c, &mut exit, &mut k, &mut u) != 0 {
                                threads.entry((e.tid, c.ns())).or_insert(Thread { handle: h, class, last: k.ns() + u.ns() });
                            } else { CloseHandle(h); }
                        }
                    }
                    ok = Thread32Next(snapshot, &mut e);
                }
                CloseHandle(snapshot);
            }
            let mut sums = [0; 7];
            for t in threads.values_mut() {
                let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
                if GetThreadTimes(t.handle, &mut c, &mut e, &mut k, &mut u) != 0 { t.last = k.ns()+u.ns(); }
                sums[t.class] += t.last;
            }
            let (mut c, mut e, mut k, mut u) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
            if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) != 0 {
                // Process totals include unregistered or already-exited host
                // workers. Samples are sequential, so clamp quantization skew.
                let classified: u64 = sums[..6].iter().sum();
                sums[6] = (k.ns() + u.ns()).saturating_sub(classified);
            }
            sums
        }
    }

    #[cfg(test)]
    #[test]
    fn cached_handle_retains_exited_worker_cpu_without_discovery() {
        if !enabled() { return; }
        let id = std::thread::spawn(|| {
            register_thread("loader_io");
            register_thread("loader_io"); // idempotent, even after discovery
            let start = Instant::now();
            let mut value = 1u64;
            while start.elapsed() < Duration::from_millis(40) {
                value = std::hint::black_box(value.wrapping_mul(1664525).wrapping_add(1013904223));
            }
            unsafe { GetCurrentThreadId() }
        }).join().unwrap();
        let a = cpu_snapshot(false);
        let threads = threads().lock().unwrap_or_else(|e| e.into_inner());
        let retained: Vec<_> = threads.iter().filter(|((tid, _), _)| *tid == id).collect();
        assert!(!retained.is_empty());
        assert!(retained.iter().any(|(_, t)| t.last > 0 && t.class == 3));
        drop(threads);
        let b = cpu_snapshot(false);
        assert!(b[3] >= a[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interval_delta_is_incremental_and_handles_counter_reset() {
        assert_eq!(delta([8, 100, 3], [3, 80, 4]), [5, 20, 0]);
        assert_eq!(delta([8, 100, 3], [8, 100, 3]), [0; 3]);
    }
    #[test]
    fn windows_cpu_snapshot_is_monotonic() {
        let a = cpu_snapshot(false).iter().sum::<u64>();
        let b = cpu_snapshot(false).iter().sum::<u64>();
        assert!(b >= a);
    }
    #[test]
    fn transition_preserves_guest_mxcsr() {
        let original = ddnative_base::x86::stmxcsr();
        ddnative_base::x86::ldmxcsr(0x3fa1); // non-default rounding and sticky bits
        transition("test_marker", format_args!(""));
        let after = ddnative_base::x86::stmxcsr();
        ddnative_base::x86::ldmxcsr(original);
        assert_eq!(after, 0x3fa1);
    }
}
