//! Parallel guest threads (brief L42): several guest threads run
//! translated code at the same time while services stay serialised.
//! Everything is called through the registry, as translated code calls it
//! (call_ext), so services take the guest lock exactly as in a session.
use ddnative_base::{mem::Mem, x86, x86::Cpu};
use ddruntime::{
    dispatch::{Binding, Registry},
    memory::{GuestMemory, READ, WRITE},
    services, threads, win32,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const DATA: u32 = 0x0030_0000;
const COUNTER: u32 = DATA + 0x100;
const PLAIN: u32 = DATA + 0x104;
const CS: u32 = DATA + 0x200;
const HANDLES: u32 = DATA + 0x300;
const PRIVATE: u32 = 0x0040_0000; // 64 KiB per thread for busy work

const WORKER_INTERLOCKED: u32 = 0x0060_0000;
const WORKER_CS: u32 = 0x0060_0010;
const S_INC: u32 = 0x0070_0000;
const S_ENTER: u32 = 0x0070_0010;
const S_LEAVE: u32 = 0x0070_0020;
const S_CREATE_THREAD: u32 = 0x0070_0030;
const S_WAIT_ALL: u32 = 0x0070_0040;
const S_INIT_CS: u32 = 0x0070_0050;
const S_CAS: u32 = 0x0070_0060;

/// Call a registered target as translated code does (stdcall).
fn guest_call(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, args: &[u32]) -> u32 {
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0xFFFF_FFF0);
    x86::call_ext(m, cpu, target);
    cpu.eax
}

/// Translated-code work between service calls: reads and writes in the
/// thread's own area (no service call, so no lock in parallel mode).
fn busy(m: &mut dyn Mem, base: u32, rounds: u32) {
    for k in 0..rounds {
        let a = base + 4 * (k % 1024);
        let v = m.u32(a);
        m.set_u32(a, v.wrapping_mul(31).wrapping_add(k));
    }
}

/// Overlap detectors: how many workers are inside translated code now,
/// and the most seen at once (one pair per test: tests run concurrently).
struct Overlap {
    now: AtomicU32,
    max: AtomicU32,
}
impl Overlap {
    const fn new() -> Self {
        Overlap {
            now: AtomicU32::new(0),
            max: AtomicU32::new(0),
        }
    }
    fn enter(&self) {
        let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
        self.max.fetch_max(n, Ordering::SeqCst);
    }
    fn leave(&self) {
        self.now.fetch_sub(1, Ordering::SeqCst);
    }
}
static OVERLAP_INTERLOCKED: [Overlap; 2] = [Overlap::new(), Overlap::new()];
static OVERLAP_CS: [Overlap; 2] = [Overlap::new(), Overlap::new()];

/// Start routine (stdcall, one parameter: iterations | mode << 31): adds 1
/// to COUNTER with InterlockedIncrement and to PLAIN without any lock.
fn worker_interlocked(m: &mut dyn Mem, cpu: &mut Cpu) {
    let param = m.u32(cpu.esp + 4);
    let (n, overlap) = (param & 0x7FFF_FFFF, &OVERLAP_INTERLOCKED[(param >> 31) as usize]);
    let base = PRIVATE + 0x1_0000 * (threads::current_tid() % 16);
    for _ in 0..n {
        overlap.enter();
        busy(m, base, 64);
        overlap.leave();
        guest_call(m, cpu, S_INC, &[COUNTER]);
    }
    cpu.eax = 0;
    let _ = m.u32(cpu.esp);
    cpu.esp += 8;
}

/// Start routine: COUNTER += 1 under the critical section, with the
/// read-modify-write spread over translated code between Enter and Leave.
fn worker_cs(m: &mut dyn Mem, cpu: &mut Cpu) {
    let param = m.u32(cpu.esp + 4);
    let (n, overlap) = (param & 0x7FFF_FFFF, &OVERLAP_CS[(param >> 31) as usize]);
    let base = PRIVATE + 0x1_0000 * (threads::current_tid() % 16);
    for _ in 0..n {
        overlap.enter();
        busy(m, base, 64);
        overlap.leave();
        guest_call(m, cpu, S_ENTER, &[CS]);
        let v = m.u32(COUNTER);
        busy(m, base, 16);
        m.set_u32(COUNTER, v + 1);
        guest_call(m, cpu, S_LEAVE, &[CS]);
    }
    cpu.eax = 0;
    let _ = m.u32(cpu.esp);
    cpu.esp += 8;
}

/// A machine with the workers and services bound, in the given mode.
fn machine(parallel: bool) -> (GuestMemory, Cpu) {
    // a fresh runtime for this test thread (it holds the guest lock)
    ddruntime::runtime::enter(Arc::new(ddruntime::runtime::Runtime::default()));
    threads::set_parallel(parallel);
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    m.map(DATA, 0x1000, READ | WRITE).unwrap();
    m.map_lazy(PRIVATE, 0x10_0000, READ | WRITE).unwrap();
    m.map_lazy(threads::STACKS_BASE, threads::STACKS_SIZE, READ | WRITE)
        .unwrap();
    let mut r = Registry::new();
    for (a, b) in [
        (WORKER_INTERLOCKED, Binding::Lifted(worker_interlocked)),
        (WORKER_CS, Binding::Lifted(worker_cs)),
        (S_INC, Binding::Service(win32::interlocked_increment)),
        (S_ENTER, Binding::Service(services::enter_critical_section)),
        (S_LEAVE, Binding::Service(services::leave_critical_section)),
        (S_CREATE_THREAD, Binding::Service(threads::create_thread)),
        (S_WAIT_ALL, Binding::Service(threads::wait_for_multiple_objects)),
        (S_INIT_CS, Binding::Service(services::initialize_critical_section)),
        (S_CAS, Binding::Service(win32::interlocked_compare_exchange)),
    ] {
        r.insert(a, b).unwrap();
    }
    let r = Arc::new(r);
    r.install();
    threads::set_machine(m.share(), r);
    (
        m,
        Cpu {
            esp: 0x0011_0000,
            ..Cpu::default()
        },
    )
}

/// Start `count` guest threads at `start` with `param` and wait for all.
fn run_workers(m: &mut GuestMemory, cpu: &mut Cpu, start: u32, count: u32, param: u32) {
    for k in 0..count {
        let h = guest_call(m, cpu, S_CREATE_THREAD, &[0, 0, start, param, 0, 0]);
        assert_ne!(h, 0);
        m.set_u32(HANDLES + 4 * k, h);
    }
    let r = guest_call(m, cpu, S_WAIT_ALL, &[count, HANDLES, 1, threads::INFINITE]);
    assert_eq!(r, threads::WAIT_OBJECT_0);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
}

fn interlocked_counts(parallel: bool) -> u32 {
    let (mut m, mut cpu) = machine(parallel);
    let mode = parallel as u32;
    run_workers(&mut m, &mut cpu, WORKER_INTERLOCKED, 4, 20_000 | mode << 31);
    assert_eq!(m.u32(COUNTER), 80_000, "InterlockedIncrement lost no update");
    x86::set_ext_calls(None);
    OVERLAP_INTERLOCKED[mode as usize].max.load(Ordering::SeqCst)
}

#[test]
fn interlocked_counters_are_exact_in_both_modes() {
    // serial: translated code of two threads never overlaps
    assert_eq!(interlocked_counts(false), 1);
    // parallel: it does (on a host with more than one core), and still no
    // increment is lost
    let max = interlocked_counts(true);
    if std::thread::available_parallelism().map_or(1, |n| n.get()) > 1 {
        assert!(max >= 2, "translated code ran concurrently (max {max} at once)");
    }
}

fn critical_section_counts(parallel: bool) -> u32 {
    let (mut m, mut cpu) = machine(parallel);
    let mode = parallel as u32;
    guest_call(&mut m, &mut cpu, S_INIT_CS, &[CS]);
    run_workers(&mut m, &mut cpu, WORKER_CS, 4, 2_000 | mode << 31);
    assert_eq!(
        m.u32(COUNTER),
        8_000,
        "the critical section excluded the other threads"
    );
    assert_eq!(m.u32(CS + 12), 0, "unowned at the end");
    x86::set_ext_calls(None);
    OVERLAP_CS[mode as usize].max.load(Ordering::SeqCst)
}

#[test]
fn critical_sections_exclude_in_both_modes() {
    assert_eq!(critical_section_counts(false), 1);
    let max = critical_section_counts(true);
    if std::thread::available_parallelism().map_or(1, |n| n.get()) > 1 {
        assert!(max >= 2, "translated code ran concurrently (max {max} at once)");
    }
}

#[test]
fn interlocked_compare_exchange_is_atomic_and_unaligned_falls_back() {
    let (mut m, mut cpu) = machine(true);
    m.set_u32(COUNTER, 5);
    assert_eq!(guest_call(&mut m, &mut cpu, S_CAS, &[COUNTER, 9, 5]), 5);
    assert_eq!(m.u32(COUNTER), 9);
    assert_eq!(
        guest_call(&mut m, &mut cpu, S_CAS, &[COUNTER, 1, 5]),
        9,
        "comparand differs: no store"
    );
    assert_eq!(m.u32(COUNTER), 9);
    // an unaligned operand takes the ordinary accessors
    m.set_u32(PLAIN + 1, 7);
    assert_eq!(guest_call(&mut m, &mut cpu, S_CAS, &[PLAIN + 1, 8, 7]), 7);
    assert_eq!(m.u32(PLAIN + 1), 8);
    // a lazy page gets its bytes on the first atomic access
    assert_eq!(guest_call(&mut m, &mut cpu, S_INC, &[PRIVATE + 0xF_0000]), 1);
    assert_eq!(m.u32(PRIVATE + 0xF_0000), 1);
    x86::set_ext_calls(None);
}

#[test]
fn the_processor_count_follows_the_mode() {
    for parallel in [false, true] {
        let (mut m, mut cpu) = machine(parallel);
        let info = DATA + 0x400;
        cpu.esp -= 8;
        m.set_u32(cpu.esp + 4, info);
        m.set_u32(cpu.esp, 0xFFFF_FFF0);
        win32::get_system_info(&mut m, &mut cpu);
        let n = m.u32(info + 20);
        let mask = m.u32(info + 16);
        if parallel {
            let host = std::thread::available_parallelism()
                .map_or(1, |n| n.get())
                .min(32) as u32;
            assert_eq!(n, host);
        } else {
            assert_eq!(n, 1);
        }
        assert_eq!(mask, (1u64 << n).wrapping_sub(1) as u32);
        x86::set_ext_calls(None);
    }
}
