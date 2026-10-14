//! L172 stage S1 (claude-9, head #14546/#14554): the shadow traversal's
//! parallel branch run live and published, `DDRUNTIME_PARALLEL_SHADOW=publish`.
//!
//! The original runs DB63E0's parallel branch only at top level: no job
//! batch in progress (flag 0x018D0BB0 clear, which also makes shared code
//! skip its critical sections) and the scheduler at [0x018D0B28] idle. Here
//! the traversal runs inside a job of the outer batch, so a window first
//! establishes that state: every other job participant must be blocked in
//! its wait (the workers on their start events, the main thread on the done
//! events of the outer join; threads::waits, which also holds any of them
//! that wakes until the window closes). Inside the window the globals take
//! their top-level values, the scheduler pointer a private scheduler (a copy
//! in guest memory, as the proof replay's), and the manager a private copy
//! of its eight inline recording contexts (FED200 must not overwrite a
//! caller that is one of them). The original DB63E0 then runs; its DBE560
//! calls on the private scheduler run the queued units here, lane by lane on
//! this thread (S1: zero helpers), with the callback registers of DBD8D0's
//! modes 1 to 3. Afterwards the recording counters are rebased into serial
//! order (as the proof does) and everything is restored before the window
//! closes. Not exclusive: the traversal runs serially, unchanged.
//!
//! Guard: every `DDRUNTIME_PARALLEL_SHADOW_CHECK`-th traversal (default 20)
//! runs serially with the private serial==parallel proof of the proof
//! runner instead; any refused proof turns publication off for the rest of
//! the run. Counts are logged every 512 traversals.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64};

pub(super) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var("DDRUNTIME_PARALLEL_SHADOW").as_deref() == Ok("publish");
        if on {
            crate::threads::waits::ENABLED.store(true, Relaxed);
        }
        on
    })
}

fn check_every() -> u64 {
    static N: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_CHECK").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(20)
    })
}

static FALLBACK: AtomicBool = AtomicBool::new(false);
static CALLS: AtomicU64 = AtomicU64::new(0);
static WINDOWS: AtomicU64 = AtomicU64::new(0);
static NOT_EXCLUSIVE: AtomicU64 = AtomicU64::new(0);
static PROOFS: AtomicU64 = AtomicU64::new(0);
static UNITS: AtomicU64 = AtomicU64::new(0);
/// windows aborted before the redirect because a participant woke
static WOKE: AtomicU64 = AtomicU64::new(0);
/// participants that woke (and were held) during a completed window
static WOKE_INSIDE: AtomicU64 = AtomicU64::new(0);

pub(super) enum Decision {
    Serial,
    Proof,
    Parallel,
}

pub(super) fn decide() -> Decision {
    let call = CALLS.fetch_add(1, Relaxed);
    if call % 512 == 511 {
        report();
    }
    if FALLBACK.load(Relaxed) {
        Decision::Serial
    } else if call % check_every() == 0 {
        Decision::Proof
    } else {
        Decision::Parallel
    }
}

/// The proof of a guard traversal: any refusal ends publication.
pub(super) fn proof_result(result: &Result<usize, String>) {
    PROOFS.fetch_add(1, Relaxed);
    if let Err(reason) = result {
        if !FALLBACK.swap(true, Relaxed) {
            eprintln!("parallel-shadow publish: guard proof refused, publication OFF for this run: {reason}");
        }
    }
}

fn report() {
    eprintln!(
        "parallel-shadow publish: traversals {} windows {} not exclusive {} proofs {} units {} woke {} woke inside {} fallback {}",
        CALLS.load(Relaxed), WINDOWS.load(Relaxed), NOT_EXCLUSIVE.load(Relaxed), PROOFS.load(Relaxed),
        UNITS.load(Relaxed), WOKE.load(Relaxed), WOKE_INSIDE.load(Relaxed), FALLBACK.load(Relaxed)
    );
}

thread_local! {
    /// The private scheduler of this thread's open window (0: none).
    static WINDOW_SCHEDULER: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn window_scheduler() -> u32 {
    WINDOW_SCHEDULER.with(Cell::get)
}

/// Guest storage for the private copies, allocated once and grown.
pub(super) struct Storage {
    pub(super) scheduler: u32,
    pub(super) manager: u32,
    /// (address, capacity)
    pub(super) secondary: (u32, u32),
    /// the scheduler and manager copies already hold the current bytes
    /// (the replay test builds them in its snapshot)
    pub(super) prefilled: bool,
}
static STORAGE: Mutex<Option<Storage>> = Mutex::new(None);

fn allocate(size: u32) -> u32 {
    crate::services::with_state(|st| st.heap.alloc(size)).expect("guest heap for the parallel shadow copies")
}

fn copy(m: &mut dyn Mem, from: u32, to: u32, size: u32) {
    let mut bytes = vec![0u8; size as usize];
    m.read_bytes(from, &mut bytes);
    crate::fastmem::write_bulk(m, to, &bytes);
}

/// The job participants' start and done events from the scheduler's slots.
fn participant_events(m: &dyn Mem, scheduler: u32) -> (Vec<u32>, Vec<u32>) {
    let participants = m.u32(scheduler + 0x98).min(64);
    let (mut starts, mut dones) = (Vec::new(), Vec::new());
    for k in 1..participants {
        let slot = scheduler + 0x1bdec + 0x20 * (k - 1);
        dones.push(m.u32(slot));
        starts.push(m.u32(slot - 4));
    }
    (starts, dones)
}

/// Run the traversal's parallel branch in a window. False: not exclusive
/// (nothing was changed; the caller runs it serially).
pub(super) fn run(m: &mut dyn Mem, cpu: &mut Cpu, original: LiftFn) -> bool {
    let scheduler = m.u32(SCHEDULER_GLOBAL);
    let (starts, dones) = participant_events(m, scheduler);
    // open first (a waking participant is then held and flagged), then check
    let guarded: Vec<u32> = starts.iter().chain(&dones).copied().collect();
    let Some(window) = crate::threads::waits::open(&guarded) else {
        NOT_EXCLUSIVE.fetch_add(1, Relaxed);
        return false;
    };
    if window.check(&starts, &dones).is_none() {
        NOT_EXCLUSIVE.fetch_add(1, Relaxed);
        return false;
    }
    WINDOWS.fetch_add(1, Relaxed);
    let context = m.u32(cpu.esp + 4);
    let capacity = m.u32(context + 0x4c).saturating_mul(8);
    let storage = {
        let mut s = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
        let st = s.get_or_insert_with(|| Storage {
            scheduler: allocate(0x1c150),
            manager: allocate(0xd40 + 8 * LANE_BYTES),
            secondary: (0, 0),
            prefilled: false,
        });
        if st.secondary.1 < capacity {
            st.secondary = (allocate(capacity.max(1)), capacity);
        }
        Storage { scheduler: st.scheduler, manager: st.manager, secondary: st.secondary, prefilled: false }
    };
    // the last check before any redirect: a participant that woke while the
    // window was being set up (it is held, but the premise is gone) aborts
    if window.violated() || window.check(&starts, &dones).is_none() {
        NOT_EXCLUSIVE.fetch_add(1, Relaxed);
        WOKE.fetch_add(1, Relaxed);
        return false;
    }
    let result = run_window(m, cpu, original, &storage);
    if window.violated() {
        WOKE_INSIDE.fetch_add(1, Relaxed);
    }
    drop(window);
    if let Err(reason) = result {
        // the published state is the original parallel branch's, but not
        // proven equal: stop publishing
        if !FALLBACK.swap(true, Relaxed) {
            eprintln!("parallel-shadow publish: {reason}; publication OFF");
        }
    }
    true
}

/// The window's body (the caller holds the window): the top-level globals,
/// the private copies, the original traversal, the rebase, the restore.
/// Err: the run completed but is not the proven shape.
pub(super) fn run_window(m: &mut dyn Mem, cpu: &mut Cpu, original: LiftFn, storage: &Storage) -> Result<usize, String> {
    let scheduler = m.u32(SCHEDULER_GLOBAL);
    let context = m.u32(cpu.esp + 4);
    let (private_scheduler, private_manager, secondary) = (storage.scheduler, storage.manager, storage.secondary.0);
    // the private copies and the top-level globals (as the proof replay)
    let manager = cpu.ecx;
    if !storage.prefilled {
        copy(m, scheduler, private_scheduler, 0x1c150);
        copy(m, manager, private_manager, 0xd40 + 8 * LANE_BYTES);
    }
    let nesting = [m.u8(NESTING), m.u8(NESTING_OTHER)];
    let use_secondary = m.u32(context + 0x54) == 0;
    if use_secondary {
        m.set_u32(context + 0x54, secondary);
    }
    m.set_u8(NESTING, 0);
    m.set_u8(NESTING_OTHER, 0);
    m.set_u32(SCHEDULER_GLOBAL, private_scheduler);
    m.set_u32(private_scheduler + 0x98, 2);
    m.set_u32(private_scheduler + 0x8c, 0);
    m.set_u32(private_scheduler + 0x1c14c, 0);
    cpu.ecx = private_manager;
    let lane_base = private_manager + 0xd40;
    let initial_counter = m.u32(context + 0x74);
    let initial_roots = m.u32(context + 0x48);
    let initial_lanes: Vec<u32> = (0..2).map(|lane| m.u32(lane_base + lane * LANE_BYTES + 0x74)).collect();
    WINDOW_SCHEDULER.with(|w| w.set(private_scheduler));
    // a panic in the traversal must not leave the redirect behind (the
    // window itself is released by the caller's drop while unwinding)
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| original(m, cpu)));
    WINDOW_SCHEDULER.with(|w| w.set(0));
    if let Err(payload) = ran {
        m.set_u32(SCHEDULER_GLOBAL, scheduler);
        m.set_u8(NESTING, nesting[0]);
        m.set_u8(NESTING_OTHER, nesting[1]);
        if use_secondary {
            m.set_u32(context + 0x54, 0);
        }
        cpu.ecx = manager;
        std::panic::resume_unwind(payload);
    }
    let balanced = [m.u8(NESTING), m.u8(NESTING_OTHER)] == [0, 0] && m.u32(SCHEDULER_GLOBAL) == private_scheduler;
    m.set_u32(SCHEDULER_GLOBAL, scheduler);
    m.set_u8(NESTING, nesting[0]);
    m.set_u8(NESTING_OTHER, nesting[1]);
    if use_secondary {
        m.set_u32(context + 0x54, 0);
    }
    let rebased = sequences::rebase(m, context, lane_base, &initial_lanes, initial_counter, initial_roots);
    if !balanced {
        return Err("the private scheduler scope was not balanced".into());
    }
    rebased.map_err(|e| format!("rebase refused: {e}"))
}

/// DBE560 on a window's private scheduler (S1: no helpers): the queued
/// units run here, lane by lane, as DBD8D0's modes 1 to 3 call them (the
/// proof replay's private_schedule on live memory).
pub(super) fn dispatch(m: &mut dyn Mem, cpu: &mut Cpu) {
    let scheduler = cpu.ecx;
    let count = m.u32(scheduler + 0x8c) as usize;
    let mode = m.u32(cpu.esp + 4);
    assert!(
        (1..=3).contains(&mode) && count <= 8192 && m.u32(scheduler + 0x1c14c) == 0,
        "parallel-shadow publish: unsupported private batch mode={mode} count={count}"
    );
    if count == 0 {
        m.set_u32(scheduler + 0x1c148, 0);
        m.set_u32(scheduler + 0x1c14c, 0);
        cpu.esp = cpu.esp.wrapping_add(8);
        return;
    }
    let participants = m.u32(scheduler + 0x98);
    let multiple = count > 1 && participants > 1;
    if !multiple {
        m.set_u32(scheduler + 0x98, 1);
    }
    let nesting = [m.u8(NESTING), m.u8(NESTING_OTHER)];
    if multiple {
        m.set_u8(NESTING, 1);
        m.set_u8(NESTING_OTHER, 1);
        m.set_u8(scheduler + 0xb8, 1);
    }
    m.set_u32(scheduler + 0x90, 0);
    m.set_u32(scheduler + 0x94, mode);
    let incoming = cpu.clone();
    let ranges = lanes::partitions(count, if multiple { 2 } else { 1 }).expect("lane partition");
    for (lane, jobs) in ranges.into_iter().enumerate() {
        let context = m.u32(scheduler + 0x1bdd0 + lane as u32 * 0x20);
        for job in jobs {
            let entry = scheduler + 0xc0 + job as u32 * if mode < 2 { 8 } else { 12 };
            let mut c = incoming.clone();
            c.ecx = m.u32(entry);
            let target = m.u32(entry + 4);
            c.esi = entry;
            c.edi = count as u32;
            if mode == 1 || mode == 3 {
                c.ebx = context;
                c.ebp = scheduler;
            } else {
                c.ebx = scheduler;
            }
            if mode >= 2 {
                c.edx = m.u32(entry + 8);
                c.eax = target;
                let argument = c.edx;
                push(m, &mut c, argument);
            }
            if mode == 1 || mode == 3 {
                push(m, &mut c, context);
            }
            if mode == 1 {
                c.edx = target;
            }
            push(m, &mut c, match mode {
                1 => 0xdbd9cb,
                2 => 0xdbda4c,
                _ => 0xdbdabc,
            });
            x86::call_ext(m, &mut c, target);
            assert_eq!(c.esp, incoming.esp, "parallel-shadow publish: job callback stack ABI");
            UNITS.fetch_add(1, Relaxed);
        }
    }
    if multiple {
        m.set_u8(NESTING, nesting[0]);
        m.set_u8(NESTING_OTHER, nesting[1]);
        m.set_u8(scheduler + 0xb8, 0);
    } else {
        m.set_u32(scheduler + 0x98, participants);
    }
    m.set_u32(scheduler + 0x90, count as u32);
    for offset in [0x8c, 0x1c148, 0x1c14c] {
        m.set_u32(scheduler + offset, 0);
    }
    cpu.esp = cpu.esp.wrapping_add(8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GuestMemory, READ, WRITE};

    /// L172 P3: a panic inside the traversal restores the globals, and the
    /// window the caller holds is released while unwinding (the gated threads
    /// of the outer batch must never stay held).
    #[test]
    fn a_panic_in_the_traversal_restores_the_redirect_and_releases_the_window() {
        let mut m = GuestMemory::new();
        for (base, size) in [(0x018d_0000u32, 0x1000u32), (0x0200_0000, 0x1c150), (0x0300_0000, 0xd40 + 8 * LANE_BYTES), (0x0600_0000, 0x1c150),
                             (0x0700_0000, 0xd40 + 8 * LANE_BYTES), (0x0400_0000, 0x1000), (0x0500_0000, 0x1000)] {
            m.map(base, (size + 0xfff) & !0xfff, READ | WRITE).unwrap();
        }
        let (scheduler, manager, context, stack) = (0x0200_0000u32, 0x0300_0000u32, 0x0400_0000u32, 0x0500_0800u32);
        m.set_u32(SCHEDULER_GLOBAL, scheduler);
        m.set_u8(NESTING, 3);
        m.set_u8(NESTING_OTHER, 5);
        m.set_u32(context + 0x54, 0);
        m.set_u32(stack + 4, context);
        let storage = Storage { scheduler: 0x0600_0000, manager: 0x0700_0000, secondary: (0x0500_0000, 0x100), prefilled: false };
        let mut cpu = Cpu { esp: stack, ecx: manager, ..Cpu::default() };
        fn failing(_: &mut dyn Mem, _: &mut Cpu) {
            panic!("traversal fails");
        }
        crate::threads::waits::ENABLED.store(true, Relaxed);
        let window = crate::threads::waits::open(&[0x7777]).expect("window");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = window;
            let _ = run_window(&mut m, &mut cpu, failing, &storage);
        }));
        assert!(outcome.is_err());
        assert_eq!(m.u32(SCHEDULER_GLOBAL), scheduler, "the scheduler global was left redirected");
        assert_eq!((m.u8(NESTING), m.u8(NESTING_OTHER)), (3, 5), "the nesting bytes were left reset");
        assert_eq!(m.u32(context + 0x54), 0);
        assert_eq!(cpu.ecx, manager);
        assert_eq!(window_scheduler(), 0);
        // released: a new window can be opened
        assert!(crate::threads::waits::open(&[0x7777]).is_some(), "the window was not released by the unwind");
        crate::threads::waits::ENABLED.store(false, Relaxed);
    }
}
