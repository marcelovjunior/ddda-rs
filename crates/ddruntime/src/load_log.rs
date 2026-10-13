//! Opt-in loading telemetry. Service times are inclusive elapsed time summed
//! across threads, not exclusive CPU time or additive parts of wall time.
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    OnceLock,
};
use std::time::{Duration, Instant};

#[path = "load_timeline.rs"]
pub mod timeline;

#[cfg(any(feature = "game", test))]
mod boot;

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
static ZLIB_ENTRY_CALLS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
static ZLIB_ENTRY_NS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
struct ZlibCaller {
    address: AtomicU64,
    calls: AtomicU64,
    ns: AtomicU64,
    input: AtomicU64,
    output: AtomicU64,
}
impl ZlibCaller {
    const fn new() -> Self { Self { address: AtomicU64::new(0), calls: AtomicU64::new(0), ns: AtomicU64::new(0), input: AtomicU64::new(0), output: AtomicU64::new(0) } }
}
static ZLIB_CALLERS: [ZlibCaller; 32] = [const { ZlibCaller::new() }; 32];
static ZLIB_CALLER_OVERFLOW: AtomicU64 = AtomicU64::new(0);
static ZLIB_HELPER_CALLS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
static ZLIB_HELPER_NS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
const ZLIB_HELPERS: [u32; 5] = [0x00cf2cb0, 0x00cf34d0, 0x00cf38d0, 0x00cf01e0, 0x00cf30c0];

#[cfg(any(feature = "game", test))]
fn zlib_caller(address: u32, elapsed: u64, input: u32, output: u32) {
    // Store address+1 so even a synthetic null return address has a bucket.
    let key = u64::from(address) + 1;
    for slot in &ZLIB_CALLERS {
        let found = slot.address.compare_exchange(0, key, Relaxed, Relaxed).unwrap_or_else(|v| v);
        if found == 0 || found == key {
            slot.calls.fetch_add(1, Relaxed);
            slot.ns.fetch_add(elapsed, Relaxed);
            slot.input.fetch_add(u64::from(input), Relaxed);
            slot.output.fetch_add(u64::from(output), Relaxed);
            return;
        }
    }
    ZLIB_CALLER_OVERFLOW.fetch_add(1, Relaxed);
}
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static CACHE_HOST_BYTES: AtomicU64 = AtomicU64::new(0);
static GEOMETRY_CALLS: AtomicU64 = AtomicU64::new(0);
static GEOMETRY_NS: AtomicU64 = AtomicU64::new(0);
static VORBIS_CALLS: AtomicU64 = AtomicU64::new(0);
static VORBIS_NS: AtomicU64 = AtomicU64::new(0);

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
    timeline::register_thread("host_main");
    eprintln!("load-log t=0.000000 event=process_start");
    event("main_entry", format_args!(""));
}

pub fn event(name: &str, details: std::fmt::Arguments<'_>) {
    if enabled() {
        eprintln!("load-log t={:.6} event={name} {details}", seconds());
        if matches!(name, "main_entry" | "gpu_init_begin" | "gpu_init_end" |
            "runtime_init_begin" | "runtime_init_end" | "guest_start" | "guest_end" |
            "first_present" | "movie_start" | "movie_end") {
            timeline::transition(name, details);
        }
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

/// Counter order is shared with the interval formatter; all are completion
/// counters, sampled independently (not an atomic stop-the-world snapshot).
fn interval_counters() -> [u64; 15] {
    [&READS, &BYTES, &READ_NS, &ARCHIVE_READS, &ARCHIVE_NS,
     &INFLATES, &INFLATE_NS, &ZLIB_CALLS, &ZLIB_NS,
     &ZLIB_HELPER_NS[0], &ZLIB_HELPER_NS[1], &ZLIB_HELPER_NS[3],
     &GEOMETRY_NS, &OPEN_NS, &VORBIS_NS].map(|v| v.load(Relaxed))
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
    for slot in &ZLIB_CALLERS {
        let key = slot.address.load(Relaxed);
        if key != 0 {
            event("zlib_caller", format_args!("return_pc={:#010x} calls={} ns={} input_bytes={} output_bytes={}", key - 1,
                slot.calls.load(Relaxed), slot.ns.load(Relaxed), slot.input.load(Relaxed), slot.output.load(Relaxed)));
        }
    }
    if ZLIB_CALLER_OVERFLOW.load(Relaxed) != 0 {
        event("zlib_caller_overflow", format_args!("calls={}", ZLIB_CALLER_OVERFLOW.load(Relaxed)));
    }
    for (i, address) in ZLIB_HELPERS.iter().enumerate() {
        event("zlib_helper", format_args!("address={address:#010x} calls={} ns={}",
            ZLIB_HELPER_CALLS[i].load(Relaxed), ZLIB_HELPER_NS[i].load(Relaxed)));
    }
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
    event("zlib_entry_totals", format_args!(
        "large_calls={} large_ns={} short_input_calls={} short_input_ns={} short_output_calls={} short_output_ns={} both_short_calls={} both_short_ns={} null_calls={} null_ns={}",
        ZLIB_ENTRY_CALLS[0].load(Relaxed), ZLIB_ENTRY_NS[0].load(Relaxed),
        ZLIB_ENTRY_CALLS[1].load(Relaxed), ZLIB_ENTRY_NS[1].load(Relaxed),
        ZLIB_ENTRY_CALLS[2].load(Relaxed), ZLIB_ENTRY_NS[2].load(Relaxed),
        ZLIB_ENTRY_CALLS[3].load(Relaxed), ZLIB_ENTRY_NS[3].load(Relaxed),
        ZLIB_ENTRY_CALLS[4].load(Relaxed), ZLIB_ENTRY_NS[4].load(Relaxed)));
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
    static ORIGINAL_HELPERS: [OnceLock<LiftFn>; 5] = [const { OnceLock::new() }; 5];
    fn helper<const I: usize>(m: &mut dyn Mem, cpu: &mut Cpu) {
        let start = Instant::now();
        ORIGINAL_HELPERS[I].get().expect("zlib helper timing body")(m, cpu);
        ZLIB_HELPER_NS[I].fetch_add(start.elapsed().as_nanos() as u64, Relaxed);
        ZLIB_HELPER_CALLS[I].fetch_add(1, Relaxed);
    }
    // Read-only exe/Ghidra: procedural mesh construction and vorbisfile's
    // packet-processing entry. Exact dispatch boundaries disambiguate PDB
    // nearest-public-symbol attribution of private split helpers.
    const GEOMETRY: u32 = 0x00fe9580;
    const VORBIS: u32 = 0x012eb690;
    static ORIGINAL_GEOMETRY: OnceLock<LiftFn> = OnceLock::new();
    static ORIGINAL_VORBIS: OnceLock<LiftFn> = OnceLock::new();
    static ORIGINAL_TITLE: OnceLock<LiftFn> = OnceLock::new();
    static ORIGINAL_AREA: OnceLock<LiftFn> = OnceLock::new();
    static ORIGINAL_STAGE: OnceLock<LiftFn> = OnceLock::new();
    static TITLE_STATE: AtomicU64 = AtomicU64::new(u64::MAX);
    static STAGE_STATE: AtomicU64 = AtomicU64::new(u64::MAX);
    // Observational reads from 0x005317b0 (aTitle slot 7) and 0x00504330
    // (aStage slot 7), DDDA.exe 2017-12-13 Steam. See GAMEPLAY_PATH.md.
    // These are code-state boundaries, not proof that a GUI was displayed.
    fn title_state(m: &dyn Mem, object: u32) {
        let state = m.u32(object.wrapping_add(0xf0));
        let key = (u64::from(object) << 32) | u64::from(state);
        if TITLE_STATE.swap(key, Relaxed) != key {
            let label = match state {
                0x00531800 => "title_press_start",
                0x005325e0 | 0x00531a20 => "main_menu",
                0x00532110 => "save_load_start",
                0x005330e0 => "game_main_handover",
                _ => "title_state",
            };
            timeline::transition(label, format_args!("object={object:#x} state={state:#010x} boundary=title_dispatch"));
        }
    }
    fn title(m: &mut dyn Mem, cpu: &mut Cpu) {
        let object = cpu.ecx;
        title_state(m, object);
        boot::observe(m, object);
        ORIGINAL_TITLE.get().expect("title timing body")(m, cpu);
        // No post-call dereference: a callback can destroy its own object.
    }
    fn area(m: &mut dyn Mem, cpu: &mut Cpu) {
        let dti = m.u32(cpu.esp.wrapping_add(4));
        timeline::transition("area_request", format_args!("dti={dti:#010x} boundary=request_not_ready"));
        ORIGINAL_AREA.get().expect("area timing body")(m, cpu);
    }
    fn stage(m: &mut dyn Mem, cpu: &mut Cpu) {
        timeline::stage_started();
        let object = cpu.ecx;
        let state = m.u32(object.wrapping_add(4));
        // 0x0050adc0 case 0xe ends the load sequence by writing +4=2;
        // 0x00504330 dispatches that mode to 0x0050b870. This marks a
        // post-load stage frame, not a guarantee that input is enabled.
        if state & 0xff == 2 { timeline::gameplay_started(); }
        // +4/+5 are mode/substate; +6 is a countdown in 0x0050adc0.
        // Do not make every countdown tick an expensive transition snapshot.
        let key = (u64::from(object) << 32) | u64::from(state & 0xffff);
        if STAGE_STATE.swap(key, Relaxed) != key {
            timeline::transition("stage_state", format_args!("object={object:#x} state={state:#x} stage={} boundary=stage_update_not_player_control", m.u32(object.wrapping_add(0x724))));
        }
        ORIGINAL_STAGE.get().expect("stage timing body")(m, cpu);
    }
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
        let caller = m.u32(cpu.esp);
        let stream = m.u32(cpu.esp.wrapping_add(4));
        let (input, output) = if stream == 0 { (0, 0) } else { (m.u32(stream + 4), m.u32(stream + 16)) };
        let bucket = if stream == 0 { 4 } else {
            zlib_entry_bucket(m.u32(stream.wrapping_add(4)), m.u32(stream.wrapping_add(16)))
        };
        let start = Instant::now();
        ORIGINAL_INFLATE.get().expect("inflate timing body")(m, cpu);
        let elapsed = start.elapsed().as_nanos() as u64;
        let (used, produced) = if stream == 0 { (0, 0) } else {
            (input.wrapping_sub(m.u32(stream + 4)), output.wrapping_sub(m.u32(stream + 16)))
        };
        zlib_caller(caller, elapsed, used, produced);
        ZLIB_NS.fetch_add(elapsed, Relaxed);
        ZLIB_CALLS.fetch_add(1, Relaxed);
        ZLIB_ENTRY_CALLS[bucket].fetch_add(1, Relaxed);
        ZLIB_ENTRY_NS[bucket].fetch_add(elapsed, Relaxed);
        if archive {
            INFLATE_NS.fetch_add(elapsed, Relaxed);
            INFLATES.fetch_add(1, Relaxed);
        }
    }

    // Entry buffer sizes only, NOT proof that the fast leaf was executed.
    // 0x00cf0e6f (DDDA.exe 2017-12-13 Steam) requires input >= 6 and output
    // >= 258 before calling 0x00cf2cb0; the state machine may consume bytes
    // or return before reaching that branch. z_stream fields are +4/+16.
    fn zlib_entry_bucket(input: u32, output: u32) -> usize {
        usize::from(input < 6) | (usize::from(output < 258) << 1)
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
            (0x005317b0, title as LiftFn, &ORIGINAL_TITLE),
            (0x00db4c10, area as LiftFn, &ORIGINAL_AREA),
            (0x00504330, stage as LiftFn, &ORIGINAL_STAGE),
            (ZLIB_HELPERS[0], helper::<0> as LiftFn, &ORIGINAL_HELPERS[0]),
            (ZLIB_HELPERS[1], helper::<1> as LiftFn, &ORIGINAL_HELPERS[1]),
            (ZLIB_HELPERS[2], helper::<2> as LiftFn, &ORIGINAL_HELPERS[2]),
            (ZLIB_HELPERS[3], helper::<3> as LiftFn, &ORIGINAL_HELPERS[3]),
            (ZLIB_HELPERS[4], helper::<4> as LiftFn, &ORIGINAL_HELPERS[4]),
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
        fn zlib_entry_thresholds_match_fast_leaf_guard() {
            assert_eq!(zlib_entry_bucket(6, 258), 0);
            assert_eq!(zlib_entry_bucket(5, 258), 1);
            assert_eq!(zlib_entry_bucket(6, 257), 2);
            assert_eq!(zlib_entry_bucket(0, 0), 3);
            assert_eq!(zlib_entry_bucket(u32::MAX, u32::MAX), 0);
        }
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
            let mut ports = vec![(ARCHIVE_READ, body as LiftFn), (ZLIB_HELPERS[4], body as LiftFn)];
            wrap_archive(&mut ports, &[(ARCHIVE_READ, replaced), (INFLATE, body), (GEOMETRY, body), (VORBIS, body),
                (0x005317b0, body), (0x00db4c10, body), (0x00504330, body), (ZLIB_HELPERS[4], replaced)]);
            for (address, f) in ports {
                let mut m = ddnative_base::mem::Sparse::default();
                m.set_u32(0x018750f0, 1); // L163 optional logo-region observation
                m.set_u32(35 + 0xf0, 0x00531800); // observed aTitle dispatcher field
                m.set_u32(35 + 4, 2); // observed aStage mode/substate
                m.set_u32(35 + 0x724, 100); // observed stage number
                m.set_u32(100, 0x12345678); // guest return PC for caller census
                m.set_u32(104, 4); // geometry's subdivision argument
                if address == INFLATE {
                    m.set_u32(104, 0x2000);
                    m.set_u32(0x2004, 5);
                    m.set_u32(0x2010, 257);
                }
                let mut cpu = Cpu {
                    ecx: 35,
                    esp: 100,
                    ..Cpu::default()
                };
                f(&mut m, &mut cpu);
                assert_eq!((cpu.eax, cpu.esp, m.u32(0x1000)), (42, 112, 42));
                assert_eq!((m.u32(35 + 0xf0), m.u32(35 + 4), m.u32(35 + 0x724)), (0x00531800, 2, 100));
                if address == INFLATE {
                    assert_eq!((m.u32(0x2004), m.u32(0x2010)), (5, 257));
                    assert_eq!(ZLIB_ENTRY_CALLS[3].load(Relaxed), 1);
                }
            }
            assert_eq!(ZLIB_HELPER_CALLS[4].load(Relaxed), 1);
            assert_eq!(ARCHIVE_DEPTH.with(|d| d.get()), 0);
        }
    }
}
