//! L162e observational oracle for 00FEE320, DDDA.exe 2017-12-13 Steam.
//! The body is always the original lift. No native replacement is installed.
//! Capture records ordered Mem operations and full CPU at opaque external
//! call boundaries. Callees still execute once; their out-of-band service
//! effects are NOT claimed as fully captured. Replay retains that boundary.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use std::{
    cell::RefCell,
    io::Write,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        OnceLock,
    },
    time::Instant,
};

const LEAF: u32 = 0xfee320;
const SHADOW: u32 = 0xee2c90;
const FRAME: u32 = 0x41dd60;
const SKY_VTABLE: u32 = 0x160df98;
const EVENT_LIMIT: usize = 1_000_000; // Diagnostic bound, not game data.
const BYTE_LIMIT: usize = 16 * 1024 * 1024;
static BODIES: OnceLock<[LiftFn; 3]> = OnceLock::new();
static CONFIG: OnceLock<Option<Config>> = OnceLock::new();
static FRAME_ID: AtomicU64 = AtomicU64::new(0);
static SAMPLE_ID: AtomicU64 = AtomicU64::new(0);
struct Config {
    directory: std::path::PathBuf,
    capture: bool,
    first: u64,
    count: u64,
}
struct Host(u32);
impl Host {
    fn enter() -> Self {
        let mxcsr = x86::stmxcsr();
        ddnative_base::x87::park();
        x86::ldmxcsr(x86::MXCSR_DEFAULT);
        Self(mxcsr)
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        x86::ldmxcsr(self.0);
    }
}

// Stable, explicit CPU wire format, including lazy flags (not evaluated),
// both the saved and live MXCSR, and all 108 bytes of the x87 image.
pub(crate) fn snapshot(c: &Cpu) -> Vec<u32> {
    let live = x86::stmxcsr();
    let _host = Host::enter();
    let mut v = vec![c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi];
    v.extend(c.xmm.iter().flatten());
    v.extend([
        c.fl.kind as u32,
        c.fl.a,
        c.fl.b,
        c.fl.r,
        c.fl.bits,
        c.fl.cf_in as u32,
        c.df as u32,
        c.fs_base,
        c.sys_flags,
        c.mxcsr.0,
        live,
    ]);
    v.extend([c.seg.es, c.seg.cs, c.seg.ss, c.seg.ds, c.seg.fs, c.seg.gs].map(u32::from));
    v.extend(
        c.st.clone()
            .state
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap())),
    );
    v
}

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Read(u32, Vec<u8>),
    Write(u32, Vec<u8>),
    Probe(u32, u32, bool, Result<(), u32>),
    Alloc(u32, u32),
    Enter(u32, Vec<u32>),
    Leave(u32, Vec<u32>),
}
#[derive(Default)]
struct Log {
    events: Vec<Event>,
    overflow: bool,
    bytes: usize,
}
impl Log {
    fn add(&mut self, e: Event) {
        let bytes = match &e {
            Event::Read(_, b) | Event::Write(_, b) => b.len(),
            Event::Enter(_, c) | Event::Leave(_, c) => c.len() * 4,
            _ => 16,
        };
        if self.events.len() < EVENT_LIMIT && self.bytes.saturating_add(bytes) <= BYTE_LIMIT {
            self.bytes += bytes;
            self.events.push(e);
        } else {
            self.overflow = true;
        }
    }
}
type SharedLog = Rc<RefCell<Log>>;
thread_local! {
    static RECORD: RefCell<Option<SharedLog>> = const { RefCell::new(None) };
    static SAMPLE: RefCell<Option<Sample>> = const { RefCell::new(None) };
    static EXT_INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static IN_LEAF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static EXT_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LEAF_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
pub(crate) struct Call {
    input: Vec<u32>,
    output: Vec<u32>,
    log: Log,
    panicked: bool,
}
pub(crate) type GuestPanic = Box<dyn std::any::Any + Send>;
#[derive(Default)]
struct Sample {
    leaf_ns: u64,
    calls: u64,
    records: Vec<Call>,
    boundary_ns: u64,
    events: usize,
    bytes: usize,
    overflow: bool,
}

struct LeafScope(bool);
impl Drop for LeafScope {
    fn drop(&mut self) {
        IN_LEAF.with(|v| v.set(self.0));
    }
}
struct ExtScope(usize);
impl Drop for ExtScope {
    fn drop(&mut self) {
        EXT_DEPTH.with(|v| v.set(self.0));
    }
}

fn observe_external() {
    // Install once per guest thread, after runtime dispatch is installed.
    // Restoring via set_ext_calls(Box<closure(old Rc)>) on EVERY call would
    // grow an unbounded chain; this permanent dormant forwarder does not.
    EXT_INSTALLED.with(|installed| {
        if installed.replace(true) {
            return;
        }
        let old = x86::set_ext_calls(None).expect("runtime external dispatcher");
        x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            let active = IN_LEAF.with(|v| v.get());
            let depth = EXT_DEPTH.with(|v| {
                let old = v.get();
                v.set(old + 1);
                old
            });
            let _scope = ExtScope(depth);
            let log = RECORD.with(|v| v.borrow().clone());
            if let Some(log) = &log {
                let state = snapshot(c);
                let _host = Host::enter();
                log.borrow_mut().add(Event::Enter(target, state));
            }
            let start = (active && depth == LEAF_DEPTH.with(|v| v.get())).then(Instant::now);
            old(m, c, target);
            if let Some(start) = start {
                let ns = start.elapsed().as_nanos() as u64;
                SAMPLE.with(|v| {
                    if let Some(s) = v.borrow_mut().as_mut() {
                        s.boundary_ns += ns;
                    }
                });
            }
            if let Some(log) = log {
                let state = snapshot(c);
                let _host = Host::enter();
                log.borrow_mut().add(Event::Leave(target, state));
            }
        })));
    });
}

struct Observed<'a> {
    inner: &'a mut dyn Mem,
    log: SharedLog,
}
impl Observed<'_> {
    fn add(&self, event: Event) {
        let _host = Host::enter();
        self.log.borrow_mut().add(event);
    }
}
impl Mem for Observed<'_> {
    // No page table: every lifted read/write must reach this observer.
    fn probe(&self, a: u32, n: u32, w: bool) -> Result<(), u32> {
        let result = self.inner.probe(a, n, w);
        self.add(Event::Probe(a, n, w, result));
        result
    }
    fn u8(&self, a: u32) -> u8 {
        let v = self.inner.u8(a);
        self.add(Event::Read(a, vec![v]));
        v
    }
    fn u16(&self, a: u32) -> u16 {
        let v = self.inner.u16(a);
        self.add(Event::Read(a, v.to_le_bytes().to_vec()));
        v
    }
    fn u32(&self, a: u32) -> u32 {
        let v = self.inner.u32(a);
        self.add(Event::Read(a, v.to_le_bytes().to_vec()));
        v
    }
    fn read_bytes(&self, a: u32, out: &mut [u8]) {
        self.inner.read_bytes(a, out);
        if out.len() <= BYTE_LIMIT {
            self.add(Event::Read(a, out.to_vec()));
        } else {
            self.log.borrow_mut().overflow = true;
        }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.inner.set_u8(a, v);
        self.add(Event::Write(a, vec![v]));
    }
    fn set_u16(&mut self, a: u32, v: u16) {
        self.inner.set_u16(a, v);
        self.add(Event::Write(a, v.to_le_bytes().to_vec()));
    }
    fn set_u32(&mut self, a: u32, v: u32) {
        self.inner.set_u32(a, v);
        self.add(Event::Write(a, v.to_le_bytes().to_vec()));
    }
    fn alloc(&mut self, n: u32) -> u32 {
        let a = self.inner.alloc(n);
        self.add(Event::Alloc(n, a));
        a
    }
}

fn config() -> Option<&'static Config> {
    CONFIG
        .get_or_init(|| {
            let directory = std::env::var_os("DDRUNTIME_FEE320_TRACE")?;
            if directory.is_empty() {
                return None;
            }
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).expect("FEE320 private trace directory");
            let number = |name: &str, default| {
                std::env::var(name)
                    .map(|s| s.parse::<u64>().expect(name))
                    .unwrap_or(default)
            };
            Some(Config {
                directory,
                capture: std::env::var("DDRUNTIME_FEE320_CAPTURE").as_deref() == Ok("1"),
                first: number("DDRUNTIME_FEE320_FIRST", 0),
                count: number("DDRUNTIME_FEE320_FRAMES", 30),
            })
        })
        .as_ref()
}
pub(crate) fn enabled() -> bool {
    config().is_some()
}
pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    if !enabled() {
        return;
    }
    // FEE320 MUST use its unmodified lift, not a previously installed port.
    assert!(
        !ports.iter().any(|(a, _)| *a == LEAF),
        "FEE320 oracle conflicts with native override"
    );
    let originals = [LEAF, SHADOW, FRAME].map(|address| {
        ports
            .iter()
            .chain(bodies)
            .find(|(a, _)| *a == address)
            .map(|(_, f)| *f)
            .expect("oracle original body")
    });
    BODIES.set(originals).expect("FEE320 oracle installed once");
    for (address, body) in [
        (LEAF, leaf as LiftFn),
        (SHADOW, shadow as LiftFn),
        (FRAME, frame as LiftFn),
    ] {
        ports.retain(|(a, _)| *a != address);
        ports.push((address, body));
    }
}
fn frame(m: &mut dyn Mem, c: &mut Cpu) {
    FRAME_ID.fetch_add(1, Relaxed);
    BODIES.get().unwrap()[2](m, c);
}
struct RestoreSample(Option<Sample>);
impl Drop for RestoreSample {
    fn drop(&mut self) {
        SAMPLE.with(|v| *v.borrow_mut() = self.0.take());
    }
}
struct RestoreRecord(Option<SharedLog>);
impl Drop for RestoreRecord {
    fn drop(&mut self) {
        RECORD.with(|v| *v.borrow_mut() = self.0.take());
    }
}

/// Shared with the ranked codegen corpus: execute a whole original body
/// once and return its bounded CPU/Mem/callee transcript. Requires the
/// runtime external dispatcher already installed on this thread. The
/// caller MUST resume any returned panic after saving/rejecting the record.
/// Nested captures are rejected: wrappers must retain the original body
/// without starting another capture while `capturing()` is true.
pub(crate) fn capturing() -> bool {
    RECORD.with(|v| v.borrow().is_some())
}

pub(crate) fn capture_call(body: LiftFn, m: &mut dyn Mem, c: &mut Cpu) -> (Call, Option<GuestPanic>) {
    assert!(
        !capturing(),
        "nested corpus capture would lose outer callee seams"
    );
    observe_external();
    let input = snapshot(c);
    let log = Rc::new(RefCell::new(Log::default()));
    let scope = RestoreRecord(RECORD.with(|v| v.replace(Some(log.clone()))));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        body(
            &mut Observed {
                inner: m,
                log: log.clone(),
            },
            c,
        );
    }));
    let output = snapshot(c);
    drop(scope);
    let log = Rc::try_unwrap(log)
        .ok()
        .expect("released oracle observer")
        .into_inner();
    (
        Call {
            input,
            output,
            log,
            panicked: result.is_err(),
        },
        result.err(),
    )
}
fn shadow(m: &mut dyn Mem, c: &mut Cpu) {
    let cfg = config().unwrap();
    let frame = FRAME_ID.load(Relaxed);
    if frame < cfg.first || frame - cfg.first >= cfg.count || m.u32(c.ecx) != SKY_VTABLE {
        BODIES.get().unwrap()[1](m, c);
        return;
    }
    let object = c.ecx;
    let scope = RestoreSample(SAMPLE.with(|v| v.replace(Some(Sample::default()))));
    let start = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| BODIES.get().unwrap()[1](m, c)));
    let elapsed = start.elapsed().as_nanos() as u64;
    let sample = SAMPLE.with(|v| v.borrow_mut().take().unwrap());
    drop(scope);
    {
        let _host = Host::enter();
        let id = SAMPLE_ID.fetch_add(1, Relaxed);
        let path = cfg.directory.join(format!(
            "fee320-{frame}-{}-{id}.json",
            crate::threads::current_tid()
        ));
        // create_new rejects accidentally mixing/overwriting repeat runs.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("new FEE320 trace");
        write_sample(
            &mut file,
            frame,
            object,
            elapsed,
            &sample,
            cfg.capture,
            result.is_err(),
        )
        .expect("write FEE320 trace");
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
fn leaf(m: &mut dyn Mem, c: &mut Cpu) {
    if !SAMPLE.with(|v| v.borrow().is_some()) || IN_LEAF.with(|v| v.get()) {
        BODIES.get().unwrap()[0](m, c);
        return;
    }
    observe_external();
    LEAF_DEPTH.with(|v| v.set(EXT_DEPTH.with(|v| v.get())));
    let _leaf = LeafScope(IN_LEAF.with(|v| v.replace(true)));
    if !config().unwrap().capture {
        let start = Instant::now();
        BODIES.get().unwrap()[0](m, c);
        let ns = start.elapsed().as_nanos() as u64;
        SAMPLE.with(|v| {
            let mut s = v.borrow_mut();
            let s = s.as_mut().unwrap();
            s.calls += 1;
            s.leaf_ns += ns;
        });
        return;
    }
    let exhausted = SAMPLE.with(|v| {
        let mut s = v.borrow_mut();
        let s = s.as_mut().unwrap();
        let exhausted = s.events >= EVENT_LIMIT || s.bytes >= BYTE_LIMIT || s.records.len() >= 4096;
        if exhausted {
            s.overflow = true;
            s.calls += 1;
        }
        exhausted
    });
    if exhausted {
        BODIES.get().unwrap()[0](m, c);
        return;
    }
    let start = Instant::now();
    let (call, panic) = capture_call(BODIES.get().unwrap()[0], m, c);
    let ns = start.elapsed().as_nanos() as u64;
    let _host = Host::enter();
    SAMPLE.with(|v| {
        let mut s = v.borrow_mut();
        let s = s.as_mut().unwrap();
        s.calls += 1;
        s.leaf_ns += ns;
        s.events += call.log.events.len();
        s.bytes += call.log.bytes;
        s.records.push(call);
    });
    drop(_host);
    if let Some(panic) = panic {
        std::panic::resume_unwind(panic);
    }
}

fn write_sample(
    out: &mut dyn Write,
    frame: u64,
    object: u32,
    ns: u64,
    sample: &Sample,
    capture: bool,
    panicked: bool,
) -> std::io::Result<()> {
    write!(out,"{{\"version\":1,\"origin\":\"runtime-lift\",\"entry\":{LEAF},\"frame\":{frame},\"object\":{object},\"capture\":{capture},\"panicked\":{panicked},\"overflow\":{},\"shadow_ns\":{ns},\"leaf_ns\":{},\"boundary_ns\":{},\"calls\":{},\"records\":[",sample.overflow,sample.leaf_ns,sample.boundary_ns,sample.calls)?;
    for (i, c) in sample.records.iter().enumerate() {
        if i != 0 {
            write!(out, ",")?;
        }
        write_call(out, c)?;
    }
    writeln!(out, "]}}")
}

/// Serialize the shared 84-word CPU/ordered-event contract as one JSON object.
pub(crate) fn write_call(out: &mut dyn Write, c: &Call) -> std::io::Result<()> {
    let _host = Host::enter();
    write!(
        out,
        "{{\"input\":{:?},\"output\":{:?},\"overflow\":{},\"panicked\":{},\"events\":[",
        c.input, c.output, c.log.overflow, c.panicked
    )?;
    for (i, event) in c.log.events.iter().enumerate() {
        if i != 0 {
            write!(out, ",")?;
        }
        match event {
            Event::Read(a, b) => write!(out, "[\"r\",{a},{b:?}]")?,
            Event::Write(a, b) => write!(out, "[\"w\",{a},{b:?}]")?,
            Event::Enter(a, c) => write!(out, "[\"enter\",{a},{c:?}]")?,
            Event::Leave(a, c) => write!(out, "[\"leave\",{a},{c:?}]")?,
            Event::Alloc(n, a) => write!(out, "[\"alloc\",{n},{a}]")?,
            Event::Probe(a, n, w, r) => write!(
                out,
                "[\"probe\",{a},{n},{w},{}]",
                r.err().map(|a| a.to_string()).unwrap_or("null".into())
            )?,
        }
    }
    write!(out, "]}}")
}

#[cfg(test)]
pub(crate) fn test_capture(body: LiftFn, m: &mut dyn Mem, c: &mut Cpu) -> String {
    // Each isolated test installs its own external handler first.
    EXT_INSTALLED.with(|v| v.set(false));
    let (call, panic) = capture_call(body, m, c);
    assert!(panic.is_none(), "synthetic original body panicked");
    let sample = Sample {
        calls: 1,
        records: vec![call],
        ..Default::default()
    };
    let mut text = vec![];
    write_sample(&mut text, 0, 0, 0, &sample, true, false).unwrap();
    String::from_utf8(text)
        .unwrap()
        .replace("runtime-lift", "synthetic-lift")
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Memory {
        bytes: [u8; 32],
        widths: RefCell<Vec<usize>>,
    }
    impl Mem for Memory {
        fn probe(&self, a: u32, n: u32, _: bool) -> Result<(), u32> {
            if a as u64 + n as u64 <= 32 {
                Ok(())
            } else {
                Err(32)
            }
        }
        fn u8(&self, a: u32) -> u8 {
            self.widths.borrow_mut().push(1);
            self.bytes[a as usize]
        }
        fn u32(&self, a: u32) -> u32 {
            self.widths.borrow_mut().push(4);
            u32::from_le_bytes(self.bytes[a as usize..a as usize + 4].try_into().unwrap())
        }
        fn set_u8(&mut self, a: u32, v: u8) {
            self.bytes[a as usize] = v;
        }
        fn set_u32(&mut self, a: u32, v: u32) {
            self.widths.borrow_mut().push(4);
            self.bytes[a as usize..a as usize + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    #[test]
    fn shared_capture_keeps_nested_external_seams_and_returns_panics() {
        EXT_INSTALLED.with(|v| v.set(false));
        x86::set_ext_calls(Some(Box::new(|m, c, target| {
            if target == 9 {
                x86::call_ext(m, c, 10);
            }
            m.set_u32(4, target);
            c.eax = target;
        })));
        fn body(m: &mut dyn Mem, c: &mut Cpu) {
            x86::call_ext(m, c, 9);
        }
        let mut m = Memory {
            bytes: [0; 32],
            widths: RefCell::new(vec![]),
        };
        let mut c = Cpu::default();
        let (call, panic) = capture_call(body, &mut m, &mut c);
        assert!(panic.is_none());
        assert!(matches!(
            call.log.events.as_slice(),
            [
                Event::Enter(9, _),
                Event::Enter(10, _),
                Event::Write(4, _),
                Event::Leave(10, _),
                Event::Write(4, _),
                Event::Leave(9, _)
            ]
        ));
        assert_eq!(c.eax, 9);
        assert!(!capturing());
        fn fault(_: &mut dyn Mem, _: &mut Cpu) {
            panic!("synthetic original failure");
        }
        let (call, panic) = capture_call(fault, &mut m, &mut c);
        assert!(call.panicked);
        assert!(panic.is_some());
        assert!(!capturing());
        x86::set_ext_calls(None);
        EXT_INSTALLED.with(|v| v.set(false));
    }
    #[test]
    fn observer_preserves_width_probe_and_effect_order() {
        let mut m = Memory {
            bytes: [7; 32],
            widths: RefCell::new(vec![]),
        };
        let log = Rc::new(RefCell::new(Log::default()));
        let mut observed = Observed {
            inner: &mut m,
            log: log.clone(),
        };
        assert!(observed.page_table().is_none());
        observed.set_u32(3, 0xabcdef12);
        assert_eq!(observed.u32(3), 0xabcdef12);
        assert_eq!(observed.probe(31, 4, false), Err(32));
        drop(observed);
        assert_eq!(*m.widths.borrow(), [4, 4]);
        assert_eq!(
            log.borrow().events,
            vec![
                Event::Write(3, vec![18, 239, 205, 171]),
                Event::Read(3, vec![18, 239, 205, 171]),
                Event::Probe(31, 4, false, Err(32))
            ]
        );
        assert_eq!(m.bytes[2], 7);
        assert_eq!(m.bytes[7], 7);
    }
    #[test]
    fn snapshots_preserve_live_mxcsr_and_full_machine_state() {
        let saved = x86::stmxcsr();
        let chosen = 0x5fa0;
        x86::ldmxcsr(chosen);
        let mut c = Cpu::default();
        c.eax = 0xf00d;
        c.xmm[7][3] = 0x12345678;
        c.df = true;
        let state = snapshot(&c);
        assert_eq!(state.len(), 84);
        assert_eq!(state[0], 0xf00d);
        assert_eq!(state[39], 0x12345678);
        assert_eq!(state[50], chosen);
        assert_eq!(x86::stmxcsr(), chosen);
        x86::ldmxcsr(saved);
    }
    #[test]
    fn bounded_log_is_explicitly_rejected_on_overflow() {
        let mut log = Log::default();
        for _ in 0..=EVENT_LIMIT {
            log.add(Event::Alloc(0, 0));
        }
        assert_eq!(log.events.len(), EVENT_LIMIT);
        assert!(log.overflow);
    }
    #[test]
    fn record_scope_restores_on_unwind() {
        let log = Rc::new(RefCell::new(Log::default()));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _scope = RestoreRecord(RECORD.with(|v| v.replace(Some(log.clone()))));
            panic!("synthetic");
        }));
        assert!(RECORD.with(|v| v.borrow().is_none()));
    }
}
