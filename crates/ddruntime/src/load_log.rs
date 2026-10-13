//! Opt-in loading telemetry. Service times are inclusive elapsed time summed
//! across threads, not exclusive CPU time or additive parts of wall time.
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    OnceLock,
};
use std::time::{Duration, Instant};

static ENABLED: OnceLock<bool> = OnceLock::new();
static CLOCK: OnceLock<(Instant, Duration)> = OnceLock::new();
static READS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static READ_NS: AtomicU64 = AtomicU64::new(0);
static OPENS: AtomicU64 = AtomicU64::new(0);
static OPEN_NS: AtomicU64 = AtomicU64::new(0);
static ARCHIVES: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_NS: AtomicU64 = AtomicU64::new(0);
static ARCHIVE_READS: AtomicU64 = AtomicU64::new(0);
static INFLATE_NS: AtomicU64 = AtomicU64::new(0);
static INFLATES: AtomicU64 = AtomicU64::new(0);
static ZLIB_NS: AtomicU64 = AtomicU64::new(0);
static ZLIB_CALLS: AtomicU64 = AtomicU64::new(0);
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static CACHE_HOST_BYTES: AtomicU64 = AtomicU64::new(0);
static GEOMETRY_CALLS: AtomicU64 = AtomicU64::new(0);
static GEOMETRY_NS: AtomicU64 = AtomicU64::new(0);
static VORBIS_CALLS: AtomicU64 = AtomicU64::new(0);
static VORBIS_NS: AtomicU64 = AtomicU64::new(0);
static UNMAP_BACKED_PAGES: AtomicU64 = AtomicU64::new(0);
static MEMORY_TABLES: std::sync::Mutex<Vec<std::sync::Weak<ddnative_base::mem::PageTable>>> =
    std::sync::Mutex::new(Vec::new());

/// Process commitment and resident memory are different measurements.
#[derive(Debug, Clone, Copy)]
pub struct ProcessMemory {
    pub private_bytes: u64,
    pub working_set_bytes: u64,
    pub peak_working_set_bytes: u64,
    pub peak_commit_bytes: u64,
}

#[cfg(windows)]
pub fn process_memory() -> Option<ProcessMemory> {
    // PROCESS_MEMORY_COUNTERS_EX, checked against the installed Windows
    // bindings (windows-sys 0.61.2, Win32/System/ProcessStatus).
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        size: u32,
        faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged_pool: usize,
        paged_pool: usize,
        peak_nonpaged_pool: usize,
        nonpaged_pool: usize,
        commit: usize,
        peak_commit: usize,
        private_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetCurrentProcess() -> *mut std::ffi::c_void; }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, size: u32) -> i32;
    }
    let mut counters = Counters { size: std::mem::size_of::<Counters>() as u32, ..Counters::default() };
    let size = counters.size;
    // SAFETY: current-process pseudo handle and correctly sized writable
    // PROCESS_MEMORY_COUNTERS_EX storage. Failure is reported as unavailable.
    if unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) } == 0 {
        return None;
    }
    Some(ProcessMemory {
        private_bytes: counters.private_usage as u64,
        working_set_bytes: counters.working_set as u64,
        peak_working_set_bytes: counters.peak_working_set as u64,
        peak_commit_bytes: counters.peak_commit as u64,
    })
}

#[cfg(not(windows))]
pub fn process_memory() -> Option<ProcessMemory> { None }

fn track_table(table: std::sync::Weak<ddnative_base::mem::PageTable>) {
    let mut tables = MEMORY_TABLES.lock().unwrap_or_else(|e| e.into_inner());
    tables.retain(|table| table.strong_count() != 0);
    if !tables.iter().any(|existing| existing.ptr_eq(&table)) { tables.push(table); }
}

/// Register a weak diagnostic handle, never retaining a session's pages.
pub fn track_memory(memory: &crate::memory::GuestMemory) {
    if enabled() { track_table(memory.diagnostic_table()); }
}

pub(crate) fn unmap_backed(pages: u64) {
    UNMAP_BACKED_PAGES.fetch_add(pages, Relaxed);
}

fn guest_memory() -> (u64, u64) {
    let mut tables = MEMORY_TABLES.lock().unwrap_or_else(|e| e.into_inner());
    tables.retain(|table| table.strong_count() != 0);
    let mut spaces = 0;
    let mut pages = 0;
    for table in tables.iter().filter_map(std::sync::Weak::upgrade) {
        spaces += 1;
        pages += table.backed_count() as u64;
    }
    (spaces, pages * 4096)
}

fn memory_totals() {
    let (spaces, guest_bytes) = guest_memory();
    // unback retains old allocations until PageTable drops. This cumulative
    // observation helps identify churn, but is not a retained-byte gauge:
    // it spans dropped sessions and can race concurrent unmaps.
    let unmapped = UNMAP_BACKED_PAGES.load(Relaxed) * 4096;
    if let Some(p) = process_memory() {
        event("memory_totals", format_args!(
            "available=1 private_bytes={} working_set_bytes={} peak_working_set_bytes={} peak_commit_bytes={} guest_spaces={} guest_backed_bytes={} guest_unmap_backed_bytes={}",
            p.private_bytes, p.working_set_bytes, p.peak_working_set_bytes,
            p.peak_commit_bytes, spaces, guest_bytes, unmapped));
    } else {
        event("memory_totals", format_args!(
            "available=0 guest_spaces={} guest_backed_bytes={} guest_unmap_backed_bytes={}", spaces, guest_bytes, unmapped));
    }
}

#[cfg(test)]
mod memory_counter_tests {
    use super::*;
    use ddnative_base::mem::Mem;

    #[test]
    fn tracks_backed_pages_without_retaining_or_double_counting_a_space() {
        let mut memory = crate::memory::GuestMemory::new();
        memory.map_lazy(0x1000, 0x2000, 3).unwrap();
        let weak = memory.diagnostic_table();
        track_table(weak.clone());
        track_table(memory.share().diagnostic_table());
        assert_eq!(guest_memory(), (1, 0));
        memory.set_u8(0x1000, 42);
        assert_eq!(guest_memory(), (1, 4096));
        memory.unmap(0x1000, 4096);
        assert_eq!(guest_memory(), (1, 0));
        drop(memory);
        assert!(weak.upgrade().is_none());
        assert_eq!(guest_memory(), (0, 0));
    }
}

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var("DDRUNTIME_LOAD_LOG").as_deref() == Ok("1"))
}

// Use Windows' process creation time to include work before Rust main. After
// this one sample the clock is monotonic, independent of wall-clock changes.
#[cfg(windows)]
fn process_age() -> Duration {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    impl FileTime {
        fn ticks(&self) -> u64 {
            (u64::from(self.high) << 32) | u64::from(self.low)
        }
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            h: *mut std::ffi::c_void,
            c: *mut FileTime,
            e: *mut FileTime,
            k: *mut FileTime,
            u: *mut FileTime,
        ) -> i32;
        fn GetSystemTimeAsFileTime(t: *mut FileTime);
    }
    let (mut c, mut e, mut k, mut u, mut now) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    // SAFETY: pseudo handle and writable FILETIME structures have the Win32 ABI.
    unsafe {
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) == 0 {
            return Duration::ZERO;
        }
        GetSystemTimeAsFileTime(&mut now);
    }
    Duration::from_nanos(now.ticks().saturating_sub(c.ticks()).saturating_mul(100))
}
#[cfg(not(windows))]
fn process_age() -> Duration {
    Duration::ZERO
}

pub fn seconds() -> f64 {
    let (start, age) = CLOCK.get_or_init(|| (Instant::now(), process_age()));
    (*age + start.elapsed()).as_secs_f64()
}

/// Call at main entry. Non-Windows tools use main entry as their origin.
pub fn start() {
    if !enabled() {
        return;
    }
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Relaxed) {
        return;
    }
    let _ = seconds();
    eprintln!("load-log t=0.000000 event=process_start");
    event("main_entry", format_args!(""));
}

pub fn event(name: &str, details: std::fmt::Arguments<'_>) {
    if enabled() {
        eprintln!("load-log t={:.6} event={name} {details}", seconds());
    }
}

pub fn once(flag: &AtomicBool, name: &str) {
    if enabled() && !flag.swap(true, Relaxed) {
        event(name, format_args!(""));
    }
}

pub(crate) fn begin() -> Option<Instant> {
    enabled().then(Instant::now)
}
pub(crate) fn read_done(start: Option<Instant>, bytes: u64) {
    if let Some(start) = start {
        READ_NS.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
        READS.fetch_add(1, Relaxed);
        BYTES.fetch_add(bytes, Relaxed);
    }
}
pub(crate) fn cache_read(hit: bool, host_bytes: u64) {
    if enabled() {
        if hit { CACHE_HITS.fetch_add(1, Relaxed); }
        else { CACHE_MISSES.fetch_add(1, Relaxed); }
        CACHE_HOST_BYTES.fetch_add(host_bytes, Relaxed);
    }
}
pub(crate) fn open_done(start: Option<Instant>, name: &str, success: bool) {
    if let Some(start) = start {
        OPEN_NS.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
        OPENS.fetch_add(1, Relaxed);
        if success
            && std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("arc"))
        {
            ARCHIVES.fetch_add(1, Relaxed);
        }
    }
}

/// Cumulative counters; in-flight service calls are counted on completion.
pub fn totals() {
    if !enabled() {
        return;
    }
    memory_totals();
    event("boot_cpu_totals", format_args!(
        "geometry_calls={} geometry_ns={} vorbis_calls={} vorbis_ns={}",
        GEOMETRY_CALLS.load(Relaxed), GEOMETRY_NS.load(Relaxed),
        VORBIS_CALLS.load(Relaxed), VORBIS_NS.load(Relaxed)));
    event(
        "io_totals",
        format_args!(
            "reads={} bytes={} read_ns={} opens={} open_ns={} archive_opens={} cache_hits={} cache_misses={} cache_host_bytes={}",
            READS.load(Relaxed),
            BYTES.load(Relaxed),
            READ_NS.load(Relaxed),
            OPENS.load(Relaxed),
            OPEN_NS.load(Relaxed),
            ARCHIVES.load(Relaxed),
            CACHE_HITS.load(Relaxed),
            CACHE_MISSES.load(Relaxed),
            CACHE_HOST_BYTES.load(Relaxed)
        ),
    );
    event(
        "archive_totals",
        format_args!(
            "archive_reads={} archive_ns={} inflates={} inflate_ns={} zlib_calls={} zlib_ns={}",
            ARCHIVE_READS.load(Relaxed),
            ARCHIVE_NS.load(Relaxed),
            INFLATES.load(Relaxed),
            INFLATE_NS.load(Relaxed),
            ZLIB_CALLS.load(Relaxed),
            ZLIB_NS.load(Relaxed)
        ),
    );
}

#[cfg(any(feature = "game", test))]
pub(crate) use guest::wrap_archive;

#[cfg(any(feature = "game", test))]
mod guest {
    use super::*;
    use ddnative_base::{
        mem::Mem,
        x86::{Cpu, LiftFn},
    };
    // Boundaries identified in DDDA.exe 2017-12-13 Steam, Ghidra read-only:
    // rArchive::DecompressStream vf12 (vtable 0x0142e278 + 0x30) calls the
    // zlib inflate entry 0x00cf02e0. These wrappers run the unmodified bodies.
    const ARCHIVE_READ: u32 = 0x00e0d320;
    const INFLATE: u32 = 0x00cf02e0;
    static ORIGINAL_READ: OnceLock<LiftFn> = OnceLock::new();
    static ORIGINAL_INFLATE: OnceLock<LiftFn> = OnceLock::new();
    // Read-only exe/Ghidra: procedural mesh construction and vorbisfile's
    // packet-processing entry. Exact dispatch boundaries disambiguate PDB
    // nearest-public-symbol attribution of private split helpers.
    const GEOMETRY: u32 = 0x00fe9580;
    const VORBIS: u32 = 0x012eb690;
    static ORIGINAL_GEOMETRY: OnceLock<LiftFn> = OnceLock::new();
    static ORIGINAL_VORBIS: OnceLock<LiftFn> = OnceLock::new();
    thread_local! { static ARCHIVE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }

    struct ArchiveScope(Instant);
    impl ArchiveScope {
        fn enter() -> Self {
            ARCHIVE_DEPTH.with(|d| d.set(d.get() + 1));
            Self(Instant::now())
        }
    }
    impl Drop for ArchiveScope {
        fn drop(&mut self) {
            ARCHIVE_NS.fetch_add(self.0.elapsed().as_nanos() as u64, Relaxed);
            ARCHIVE_READS.fetch_add(1, Relaxed);
            ARCHIVE_DEPTH.with(|d| d.set(d.get() - 1));
        }
    }
    fn archive_read(m: &mut dyn Mem, cpu: &mut Cpu) {
        let _scope = ArchiveScope::enter();
        ORIGINAL_READ.get().expect("archive timing body")(m, cpu);
    }
    fn inflate(m: &mut dyn Mem, cpu: &mut Cpu) {
        let archive = ARCHIVE_DEPTH.with(|d| d.get() != 0);
        let start = Instant::now();
        ORIGINAL_INFLATE.get().expect("inflate timing body")(m, cpu);
        let elapsed = start.elapsed().as_nanos() as u64;
        ZLIB_NS.fetch_add(elapsed, Relaxed);
        ZLIB_CALLS.fetch_add(1, Relaxed);
        if archive {
            INFLATE_NS.fetch_add(elapsed, Relaxed);
            INFLATES.fetch_add(1, Relaxed);
        }
    }

    fn geometry(m: &mut dyn Mem, cpu: &mut Cpu) {
        let start = Instant::now();
        event("boot_geometry_begin", format_args!("subdivisions={}", m.u32(cpu.esp + 4)));
        ORIGINAL_GEOMETRY.get().expect("geometry timing body")(m, cpu);
        let elapsed = start.elapsed().as_nanos() as u64;
        GEOMETRY_NS.fetch_add(elapsed, Relaxed);
        GEOMETRY_CALLS.fetch_add(1, Relaxed);
        event("boot_geometry_end", format_args!("elapsed_ns={elapsed}"));
    }

    fn vorbis(m: &mut dyn Mem, cpu: &mut Cpu) {
        let start = Instant::now();
        ORIGINAL_VORBIS.get().expect("vorbis timing body")(m, cpu);
        VORBIS_NS.fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
        VORBIS_CALLS.fetch_add(1, Relaxed);
    }

    /// Preserve installed mods/ports as the wrapped body. Never bypass an
    /// override, recurse through dispatch, or change guest registers/memory.
    pub(crate) fn wrap_archive(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
        for (address, wrapper, original) in [
            (ARCHIVE_READ, archive_read as LiftFn, &ORIGINAL_READ),
            (INFLATE, inflate as LiftFn, &ORIGINAL_INFLATE),
            (GEOMETRY, geometry as LiftFn, &ORIGINAL_GEOMETRY),
            (VORBIS, vorbis as LiftFn, &ORIGINAL_VORBIS),
        ] {
            let body = ports.iter().chain(bodies).find(|e| e.0 == address).map(|e| e.1);
            if let Some(body) = body {
                original.set(body).expect("load wrappers registered once");
                ports.retain(|e| e.0 != address);
                ports.push((address, wrapper));
            } else {
                event("timing_unavailable", format_args!("address={address:#x}"));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn wrappers_preserve_guest_effects_and_existing_override() {
            fn body(m: &mut dyn Mem, cpu: &mut Cpu) {
                cpu.eax = cpu.ecx.wrapping_add(7);
                m.set_u32(0x1000, cpu.eax);
                cpu.esp = cpu.esp.wrapping_add(12);
            }
            fn replaced(_: &mut dyn Mem, _: &mut Cpu) {
                panic!("bypassed override")
            }
            let mut ports = vec![(ARCHIVE_READ, body as LiftFn)];
            wrap_archive(&mut ports, &[(ARCHIVE_READ, replaced), (INFLATE, body), (GEOMETRY, body), (VORBIS, body)]);
            for (_, f) in ports {
                let mut m = ddnative_base::mem::Sparse::default();
                m.set_u32(104, 4); // geometry's subdivision argument
                let mut cpu = Cpu {
                    ecx: 35,
                    esp: 100,
                    ..Cpu::default()
                };
                f(&mut m, &mut cpu);
                assert_eq!((cpu.eax, cpu.esp, m.u32(0x1000)), (42, 112, 42));
            }
            assert_eq!(ARCHIVE_DEPTH.with(|d| d.get()), 0);
        }
    }
}
