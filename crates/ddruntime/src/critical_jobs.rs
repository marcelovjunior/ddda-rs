//! L158 observational scheduler timeline. Original layout/call sites:
//! 0x00DBE560 (batch), 0x00DBD8D0 (main), 0x00DBDC90 (workers),
//! DDDA.exe 2017-12-13 Steam. This does not replace scheduler behavior.
//! Set DDRUNTIME_CRITICAL_JOBS to a private output file, alongside
//! DDRUNTIME_SYNC_PROFILE=1. Present counters label producer progress only.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{mem::Mem, x86::{self, Cpu, LiftFn}};
use std::{cell::Cell, io::{BufWriter, Write}, sync::{OnceLock,
    atomic::{AtomicU64, Ordering::{Acquire, Release, Relaxed}},
    mpsc::{sync_channel, SyncSender}}, time::{Duration, Instant}};

static TRACE: OnceLock<Option<Trace>> = OnceLock::new();
static BODY: OnceLock<LiftFn> = OnceLock::new();
static FRAME_BODY: OnceLock<LiftFn> = OnceLock::new();
static FRAME: AtomicU64 = AtomicU64::new(0);
static NEXT: AtomicU64 = AtomicU64::new(1);
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static PRESENT: AtomicU64 = AtomicU64::new(0);
static LOST: AtomicU64 = AtomicU64::new(0);
thread_local! { static JOB: Cell<Option<(u32,u32)>> = const { Cell::new(None) }; }
struct Trace { sender: SyncSender<Event>, epoch: Instant }

#[derive(Debug)]
enum Event {
    Frame { frame: u64, ns: u64 },
    Batch { id: u64, frame: u64, present: u64, scheduler: u32, queued: u32, mode: u32,
            participants: u32, configured: u32, ns: u64, nested: bool },
    End { id: u64, ns: u64, lost: u64, panicked: bool },
    Job { id: u64, tid: u32, site: u32, target: u32, object: u32,
          vtable: u32, resolved: u32, record: u32, start: u64, end: u64, panicked: bool },
    Join { id: u64, start: u64, end: u64, result: u32 },
    Wake { id: u64, end_id: u64, tid: u32, site: u32, handle: u32, start: u64, end: u64, result: u32 },
}

#[inline]
pub(crate) fn enabled() -> bool { trace().is_some() }
#[inline]
fn trace() -> Option<&'static Trace> {
    TRACE.get_or_init(|| {
        let path = std::env::var_os("DDRUNTIME_CRITICAL_JOBS")?;
        if path.is_empty() { return None; }
        let file = match std::fs::File::create(path) {
            Ok(f) => f,
            Err(e) => { eprintln!("critical-jobs disabled: {e}"); return None; }
        };
        // Diagnostic capacity, not a recovered game constant. A full queue
        // drops telemetry, never blocks a guest worker. The parser rejects
        // batches crossing a loss and requires every queued callback.
        let (sender, receiver) = sync_channel::<Event>(65536);
        std::thread::Builder::new().name("critical-jobs".into()).spawn(move || {
            let mut out = BufWriter::new(file);
            if writeln!(out, "critical version=1").is_err() { return; }
            let mut flushed = Instant::now();
            loop {
                match receiver.recv_timeout(Duration::from_millis(200)) {
                    Ok(event) => { if writeln!(out, "{event:?}").is_err() { return; } }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {},
                    Err(_) => break,
                }
                if flushed.elapsed() >= Duration::from_millis(200) {
                    if out.flush().is_err() { return; }
                    flushed = Instant::now();
                }
            }
            let _ = out.flush();
        }).expect("critical job writer");
        Some(Trace { sender, epoch: Instant::now() })
    }).as_ref()
}
fn now() -> u64 { trace().unwrap().epoch.elapsed().as_nanos().min(u64::MAX as u128) as u64 }
fn emit(event: Event) {
    if trace().unwrap().sender.try_send(event).is_err() { LOST.fetch_add(1, Relaxed); }
}

// Host diagnostics must not change the guest's MXCSR, including on unwind.
struct Host(u32);
impl Host {
    fn enter() -> Self {
        ddnative_base::x87::park();
        let saved = x86::stmxcsr();
        x86::ldmxcsr(x86::MXCSR_DEFAULT);
        Self(saved)
    }
}
impl Drop for Host { fn drop(&mut self) { x86::ldmxcsr(self.0); } }
fn word(m: &dyn Mem, base: u32, offset: u32) -> Option<u32> {
    let a = base.checked_add(offset)?;
    a.checked_add(3)?;
    m.probe(a, 4, false).ok()?;
    Some(m.u32(a))
}

pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    if !enabled() { return; }
    let body = ports.iter().chain(bodies).find(|(a, _)| *a == 0xdbe560)
        .map(|(_, f)| *f).expect("critical scheduler body");
    BODY.set(body).expect("critical scheduler installed once");
    ports.retain(|(a, _)| *a != 0xdbe560);
    ports.push((0xdbe560, batch));
    // The application's frame virtual method (004079E0 -> 00DF0D20 ->
    // 0041DD60). This is an iteration label, independent of Present count.
    let frame_body = ports.iter().chain(bodies).find(|(a, _)| *a == 0x41dd60)
        .map(|(_, f)| *f).expect("critical frame body");
    FRAME_BODY.set(frame_body).expect("critical frame installed once");
    ports.retain(|(a, _)| *a != 0x41dd60);
    ports.push((0x41dd60, frame));
}
fn frame(m: &mut dyn Mem, cpu: &mut Cpu) {
    {
        let _host = Host::enter();
        let frame = FRAME.fetch_add(1, Relaxed) + 1;
        emit(Event::Frame { frame, ns: now() });
    }
    FRAME_BODY.get().unwrap()(m, cpu);
}
struct Batch { id: u64, previous: u64 }
impl Drop for Batch {
    fn drop(&mut self) {
        let _host = Host::enter();
        let end = now();
        ACTIVE.store(self.previous, Release);
        emit(Event::End { id: self.id, ns: end, lost: LOST.load(Relaxed), panicked: std::thread::panicking() });
    }
}
fn batch(m: &mut dyn Mem, cpu: &mut Cpu) {
    let scope = {
        let _host = Host::enter();
        let id = NEXT.fetch_add(1, Relaxed);
        let previous = ACTIVE.swap(id, std::sync::atomic::Ordering::AcqRel);
        emit(Event::Batch { id, frame: FRAME.load(Relaxed), present: PRESENT.load(Relaxed), scheduler: cpu.ecx,
            queued: word(m, cpu.ecx, 0x8c).unwrap_or(u32::MAX),
            mode: word(m, cpu.esp, 4).unwrap_or(u32::MAX),
            participants: word(m, cpu.ecx, 0x98).unwrap_or(u32::MAX),
            configured: word(m, cpu.ecx, 0x9c).unwrap_or(u32::MAX),
            ns: now(), nested: previous != 0 });
        Batch { id, previous }
    };
    BODY.get().unwrap()(m, cpu);
    drop(scope);
}
fn job_site(site: u32) -> bool {
    matches!(site, 0xdbd958 | 0xdbd9cb | 0xdbda4c | 0xdbdabc |
                   0xdbdd4c | 0xdbddbf | 0xdbde40 | 0xdbdead)
}
pub(crate) struct Job { id: u64, tid: u32, site: u32, target: u32, object: u32,
    vtable: u32, resolved: u32, record: u32, start: u64, previous: Option<(u32,u32)> }
pub(crate) fn begin_job(target: u32, m: &dyn Mem, cpu: &Cpu) -> Option<Job> {
    if !enabled() { return None; }
    let site = word(m, cpu.esp, 0)?;
    if !job_site(site) { return None; }
    // A tail redispatch retains the same return address and ESP. Count its
    // outer scheduler callback only; true nested stack frames remain distinct.
    let marker = (cpu.esp, site);
    if JOB.with(Cell::get) == Some(marker) { return None; }
    let _host = Host::enter();
    let id = ACTIVE.load(Acquire);
    if id == 0 { return None; }
    let vtable = word(m, cpu.ecx, 0).unwrap_or(0);
    // These three wrappers dispatch their object through a virtual method.
    // DB7620's alternate branch instead calls BFC7D0; this field is a
    // candidate virtual target, NOT an assertion that branch executed.
    let slot = match target { 0xdb7620 => Some(0x2c), 0x4b9ff0 => Some(0x20),
                              0x1090f70 => Some(0x24), _ => None };
    let resolved = slot.and_then(|o| word(m, vtable, o)).unwrap_or(0);
    let previous = JOB.with(|v| v.replace(Some(marker)));
    Some(Job { id, tid: crate::threads::current_tid(), site, target, object: cpu.ecx,
        vtable, resolved, record: cpu.esi, start: now(), previous })
}
impl Drop for Job {
    fn drop(&mut self) {
        let _host = Host::enter();
        let end = now();
        JOB.with(|v| v.set(self.previous));
        emit(Event::Job { id: self.id, tid: self.tid, site: self.site, target: self.target,
            object: self.object, vtable: self.vtable, resolved: self.resolved,
            record: self.record, start: self.start, end, panicked: std::thread::panicking() });
    }
}
pub(crate) struct Join { id: u64, start: u64, wake: Option<(u32,u32,u32)> }
pub(crate) fn begin_join(name: Option<&str>, m: &dyn Mem, cpu: &Cpu) -> Option<Join> {
    if !enabled() { return None; }
    let name = name?;
    if !(name.ends_with("!WaitForMultipleObjects") || name.ends_with("!WaitForSingleObject") || name.ends_with("!SetEvent")) { return None; }
    let site = word(m, cpu.esp, 0)?;
    let id = ACTIVE.load(Acquire);
    let wake = match site {
        0xdbe603 if name.ends_with("!SetEvent") => Some((crate::threads::current_tid(), site, word(m, cpu.esp, 4)?)),
        0xdbdcbc if name.ends_with("!WaitForSingleObject") => Some((crate::threads::current_tid(), site, word(m, cpu.esp, 4)?)),
        0xdbe66d if id != 0 && name.ends_with("!WaitForMultipleObjects") => None,
        _ => return None,
    };
    Some(Join { id, start: now(), wake })
}
impl Join { pub(crate) fn finish(self, result: u32) {
    let end = now();
    if let Some((tid, site, handle)) = self.wake {
        emit(Event::Wake { id: self.id, end_id: ACTIVE.load(Acquire), tid, site, handle,
            start: self.start, end, result });
    } else {
        emit(Event::Join { id: self.id, start: self.start, end, result });
    }
} }
pub(crate) fn presented(name: Option<&str>, result: u32) {
    if enabled() && result == 0 && name.is_some_and(|n|
        n.starts_with("IDirect3DDevice9::Present (") || n.starts_with("IDirect3DSwapChain9::Present (")) {
        PRESENT.fetch_add(1, Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_sites_exclude_observers() {
        for s in [0xdbd958,0xdbd9cb,0xdbda4c,0xdbdabc,0xdbdd4c,0xdbddbf,0xdbde40,0xdbdead] { assert!(job_site(s)); }
        for s in [0xdbd951,0xdbd9c3,0xdbda41,0xdbdab0,0xdbdd45,0xdbddb7,0xdbde35,0xdbdea1] { assert!(!job_site(s)); }
    }
    #[test]
    fn host_boundary_preserves_rounding_and_sticky_flags() {
        let saved = x86::stmxcsr();
        let guest = x86::MXCSR_DEFAULT | 0x6000 | 0x21;
        x86::ldmxcsr(guest);
        { let _scope = Host::enter(); assert_eq!(x86::stmxcsr(), x86::MXCSR_DEFAULT); }
        assert_eq!(x86::stmxcsr(), guest);
        x86::ldmxcsr(saved);
    }
    #[test]
    fn guarded_probe_does_not_wrap_or_fault() {
        let mut m = crate::memory::GuestMemory::new();
        m.map(0x10000, 4096, crate::memory::READ | crate::memory::WRITE).unwrap();
        m.set_u32(0x10000, 42);
        assert_eq!(word(&m, 0x10000, 0), Some(42));
        assert_eq!(word(&m, 0x10fff, 0), None);
        assert_eq!(word(&m, u32::MAX, 1), None);
    }
    #[test]
    fn opt_in_batch_preserves_guest_and_suppresses_tail_redispatch() {
        if !enabled() { return; }
        fn fake(m: &mut dyn Mem, cpu: &mut Cpu) {
            assert!(ACTIVE.load(Acquire) != 0);
            let saved = cpu.esi;
            cpu.esi = cpu.ecx + 0xc0;
            let first = begin_job(0x4b9ff0, m, cpu).unwrap();
            assert_eq!(first.resolved, 0x12345678);
            assert!(begin_job(0x12345678, m, cpu).is_none());
            drop(first);
            assert!(JOB.with(Cell::get).is_none());
            cpu.esi += 8;
            drop(begin_job(0x4b9ff0, m, cpu).unwrap());
            cpu.esi = saved;
            m.set_u32(cpu.esp, 0xdbe66d);
            begin_join(Some("KERNEL32.dll!WaitForMultipleObjects"), m, cpu).unwrap().finish(0);
            m.set_u32(cpu.esp, 0xdbd958);
        }
        BODY.set(fake).unwrap();
        let mut m = crate::memory::GuestMemory::new();
        m.map(0x10000, 4096, crate::memory::READ | crate::memory::WRITE).unwrap();
        m.set_u32(0x10000, 0xdbd958);
        m.set_u32(0x10100, 0x10200);
        m.set_u32(0x10220, 0x12345678);
        m.set_u32(0x1018c, 2);
        let mut cpu = Cpu { esp: 0x10000, ecx: 0x10100, ..Cpu::default() };
        let saved = x86::stmxcsr();
        let guest = x86::MXCSR_DEFAULT | 0x6000 | 0x21;
        x86::ldmxcsr(guest);
        batch(&mut m, &mut cpu);
        assert_eq!(x86::stmxcsr(), guest);
        x86::ldmxcsr(saved);
        assert_eq!(cpu.esp, 0x10000);
        assert_eq!(cpu.ecx, 0x10100);
        assert_eq!(m.u32(cpu.esp), 0xdbd958);
        assert_eq!(ACTIVE.load(Acquire), 0);
        // Let the independent writer flush this diagnostic fixture, never a
        // production worker delay.
        std::thread::sleep(Duration::from_millis(250));
    }
}
