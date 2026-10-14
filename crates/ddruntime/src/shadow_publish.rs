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
//! Guard: the first and every `DDRUNTIME_PARALLEL_SHADOW_CHECK`-th traversal (default 1000;
//! each proof runs seven private replays and stalls the game for seconds)
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

/// Guard proofs are opt-in (`DDRUNTIME_PARALLEL_SHADOW_CHECK=N`, head
/// #14617, user order): a proof stalls the frame for about a second. Without
/// it every exclusive traversal publishes, unproven, and nothing turns
/// publication off.
fn checking() -> bool {
    std::env::var_os("DDRUNTIME_PARALLEL_SHADOW_CHECK").is_some()
}

fn check_every() -> u64 {
    static N: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_CHECK").ok().and_then(|v| v.parse().ok()).filter(|&n| n > 0).unwrap_or(1000)
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

/// The recording contexts (the traversal's argument: one per shadow pass
/// kind) a guard proof passed for. Only these are published: a context the
/// proof cannot carry out (inconclusive) stays serial.
static PROVEN: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
/// The traversal number the next proof of an unproven context may run at.
static PROOF_DUE: AtomicU64 = AtomicU64::new(0);
static INCONCLUSIVE: AtomicU64 = AtomicU64::new(0);
/// Time spent waiting for the other participants to park, and windows
/// that became exclusive only after waiting.
static WAITED_US: AtomicU64 = AtomicU64::new(0);
static WAITED: AtomicU64 = AtomicU64::new(0);

/// How long a traversal waits for the other participants to park before it
/// runs serially (`DDRUNTIME_PARALLEL_SHADOW_WAIT_US`, default 0: s2d 33.7 fps vs 33.0 with 500 and 30.3 with 2000).
fn wait_limit() -> std::time::Duration {
    static US: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    std::time::Duration::from_micros(*US.get_or_init(|| {
        std::env::var("DDRUNTIME_PARALLEL_SHADOW_WAIT_US").ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    }))
}

fn proven(context: u32) -> bool {
    PROVEN.lock().unwrap_or_else(|e| e.into_inner()).contains(&context)
}

pub(super) fn decide(context: u32) -> Decision {
    let call = CALLS.fetch_add(1, Relaxed);
    if call % 512 == 511 {
        report();
    }
    if !checking() {
        Decision::Parallel
    } else if FALLBACK.load(Relaxed) {
        Decision::Serial
    } else if call % check_every() == 0 {
        Decision::Proof
    } else if proven(context) {
        Decision::Parallel
    } else if call >= PROOF_DUE.load(Relaxed) {
        // an unproven context: a proof, at most one per 16 traversals,
        // doubling with every inconclusive proof (up to the check interval)
        let gap = (16u64 << INCONCLUSIVE.load(Relaxed).min(16)).min(check_every().max(16));
        PROOF_DUE.store(call + gap, Relaxed);
        Decision::Proof
    } else {
        Decision::Serial
    }
}

/// A proof that could not be carried out (the capture or the serial replay
/// could not reproduce this traversal): no evidence either way.
fn inconclusive(reason: &str) -> bool {
    [
        "replay rejected",
        "incomplete/incoherent capture",
        "serial stream capture refused",
        "captured shader object missing",
        "original serial replay",
        "traversal appended no commands",
        "capture scheduler pointer",
        "replay needs more than",
    ]
    .iter()
    .any(|p| reason.starts_with(p))
}

/// The proof of a guard traversal on `context`. Passed: the context may be
/// published. Refuted (serial and parallel differ, or anything not listed in
/// [`inconclusive`]): publication OFF for the rest of the run. Inconclusive:
/// counted; the context stays serial (and is retried while unproven).
pub(super) fn proof_result(context: u32, result: &Result<usize, String>) {
    PROOFS.fetch_add(1, Relaxed);
    match result {
        Ok(_) => {
            let mut proven = PROVEN.lock().unwrap_or_else(|e| e.into_inner());
            if !proven.contains(&context) {
                proven.push(context);
                eprintln!("parallel-shadow publish: context {context:#x} proven ({} proven)", proven.len());
            }
        }
        Err(reason) if inconclusive(reason) => {
            if INCONCLUSIVE.fetch_add(1, Relaxed) < 8 {
                eprintln!("parallel-shadow publish: guard proof inconclusive on context {context:#x}: {reason}");
            }
        }
        Err(reason) => {
            if !FALLBACK.swap(true, Relaxed) {
                eprintln!("parallel-shadow publish: guard proof refuted, publication OFF for this run: {reason}");
            }
        }
    }
}

fn report() {
    eprintln!(
        "parallel-shadow publish: traversals {} windows {} (after waiting {}, {} us) not exclusive {} proofs {} (inconclusive {}, contexts proven {}) units {} lanes helped {} task errors {} window errors {} woke {} woke inside {} fallback {}",
        CALLS.load(Relaxed), WINDOWS.load(Relaxed), WAITED.load(Relaxed), WAITED_US.load(Relaxed),
        NOT_EXCLUSIVE.load(Relaxed), PROOFS.load(Relaxed), INCONCLUSIVE.load(Relaxed), PROVEN.lock().unwrap_or_else(|e| e.into_inner()).len(),
        UNITS.load(Relaxed), HELPED.load(Relaxed), TASK_ERRORS.load(Relaxed), WINDOW_ERRORS.load(Relaxed), WOKE.load(Relaxed),
        WOKE_INSIDE.load(Relaxed), FALLBACK.load(Relaxed)
    );
}

/// S2: lanes run as waits::help tasks on the parked participants
/// (`DDRUNTIME_PARALLEL_SHADOW_HELPERS=0`: S1, every lane on the window's
/// owner).
fn helpers_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("DDRUNTIME_PARALLEL_SHADOW_HELPERS").as_deref() != Ok("0"))
}

/// Lanes run by a helper thread, and lane tasks that failed.
static HELPED: AtomicU64 = AtomicU64::new(0);
static TASK_ERRORS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The private scheduler of this thread's open window (0: none).
    static WINDOW_SCHEDULER: Cell<u32> = const { Cell::new(0) };
    /// This thread owns a live window whose parked participants take tasks.
    static LIVE_WINDOW: Cell<bool> = const { Cell::new(false) };
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
    // the heap is guest state: under the service lock (translated code runs
    // without it in parallel mode)
    let _lock = crate::threads::service();
    crate::services::with_state(|st| st.heap.alloc(size)).expect("guest heap for the parallel shadow copies")
}

fn copy(m: &mut dyn Mem, from: u32, to: u32, size: u32) {
    let mut bytes = vec![0u8; size as usize];
    m.read_bytes(from, &mut bytes);
    crate::fastmem::write_bulk(m, to, &bytes);
}

/// The private recording manager: the manager's header and its eight inline
/// recording contexts, of which only the header and the first two (the
/// private scheduler has two participants: FED200 initialises lanes 0..2,
/// the units record into them, FED3B0 merges them) are copied. The rest is
/// fresh private storage: the proof's parallel runs use the same copy and
/// their definedness runs fill the uncopied bytes with 0x00 and 0xFF, so a
/// dependence on them refutes the proof.
pub(super) const MANAGER_BYTES: u32 = 0xd40 + 8 * LANE_BYTES;
pub(super) const MANAGER_COPIED: [std::ops::Range<u32>; 1] = [0..0xd40 + 2 * LANE_BYTES];
/// The private scheduler, copied whole.
pub(super) const SCHEDULER_BYTES: u32 = 0x1c150;
pub(super) const SCHEDULER_COPIED: [std::ops::Range<u32>; 1] = [0..0x1c150];

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
    // the others are usually still running units of the outer batch: wait
    // (bounded) until they are all parked, without a window (a participant
    // that parks while one is open would count as running inside it)
    let start = std::time::Instant::now();
    let mut waited = false;
    while crate::waits::verdict(&crate::threads::waits::others(), &starts, &dones).is_none() {
        if start.elapsed() >= wait_limit() {
            WAITED_US.fetch_add(start.elapsed().as_micros() as u64, Relaxed);
            NOT_EXCLUSIVE.fetch_add(1, Relaxed);
            return false;
        }
        waited = true;
        std::thread::yield_now();
    }
    if waited {
        WAITED.fetch_add(1, Relaxed);
        WAITED_US.fetch_add(start.elapsed().as_micros() as u64, Relaxed);
    }
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
    LIVE_WINDOW.with(|w| w.set(helpers_enabled()));
    let result = run_window(m, cpu, original, &storage);
    LIVE_WINDOW.with(|w| w.set(false));
    if window.violated() {
        WOKE_INSIDE.fetch_add(1, Relaxed);
    }
    drop(window);
    if let Err(reason) = result {
        // the published state is the original parallel branch's, but not
        // the proven shape: with guard proofs on, stop publishing; without
        // them (the default) count it and go on
        if WINDOW_ERRORS.fetch_add(1, Relaxed) < 8 {
            eprintln!("parallel-shadow publish: window error: {reason}");
        }
        if checking() && !FALLBACK.swap(true, Relaxed) {
            eprintln!("parallel-shadow publish: publication OFF");
        }
    }
    true
}

static WINDOW_ERRORS: AtomicU64 = AtomicU64::new(0);

/// One lane of a private batch: its recording context and its jobs, run in
/// order with DBD8D0's callback registers and return addresses
/// (0x00DBD9CB / 0x00DBDA4C / 0x00DBDABC for modes 1 to 3).
struct Lane {
    scheduler: u32,
    mode: u32,
    count: u32,
    context: u32,
    jobs: std::ops::Range<usize>,
}

impl Lane {
    /// `base`: the registers every callback starts from (its ESP is restored
    /// after each).
    fn run(&self, m: &mut dyn Mem, base: &Cpu) {
        let (scheduler, mode, context) = (self.scheduler, self.mode, self.context);
        // one x87 state for the lane, carried from unit to unit as on a
        // DBD8D0 worker (a fresh one per unit would be acquired by every
        // unit and left in the host's spill table), released at the end
        let mut x87 = base.st.clone();
        for job in self.jobs.clone() {
            let entry = scheduler + 0xc0 + job as u32 * if mode < 2 { 8 } else { 12 };
            let mut c = base.clone();
            std::mem::swap(&mut c.st, &mut x87);
            c.ecx = m.u32(entry);
            let target = m.u32(entry + 4);
            c.esi = entry;
            c.edi = self.count;
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
            std::mem::swap(&mut c.st, &mut x87);
            assert_eq!(c.esp, base.esp, "parallel-shadow publish: job callback stack ABI");
            UNITS.fetch_add(1, Relaxed);
        }
        if x87.home != 0 {
            (ddnative_base::hooks::get().x87_release)(&mut x87);
        }
    }
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
        for part in SCHEDULER_COPIED {
            copy(m, scheduler + part.start, private_scheduler + part.start, part.end - part.start);
        }
        for part in MANAGER_COPIED {
            copy(m, manager + part.start, private_manager + part.start, part.end - part.start);
        }
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
    let lanes: Vec<Lane> = ranges
        .into_iter()
        .enumerate()
        .map(|(lane, jobs)| Lane {
            scheduler,
            mode,
            count: count as u32,
            context: m.u32(scheduler + 0x1bdd0 + lane as u32 * 0x20),
            jobs,
        })
        .collect();
    if multiple && LIVE_WINDOW.with(Cell::get) {
        // S2: one task per lane, taken by the parked participants and this
        // thread. Each starts from this thread's registers and x87 image
        // (control word) on its own thread's TEB and stack, as DBD8D0's
        // workers run their claimed units on theirs.
        let me = crate::threads::current_tid();
        let tasks: Vec<crate::threads::waits::Task> = lanes
            .into_iter()
            .map(|lane| {
                let registers = incoming.clone();
                Box::new(move |m: &mut dyn Mem, task: &mut Cpu| {
                    if crate::threads::current_tid() != me {
                        HELPED.fetch_add(1, Relaxed);
                    }
                    let mut base = registers;
                    base.esp = task.esp;
                    base.fs_base = task.fs_base;
                    lane.run(m, &base);
                    *task = base;
                    Ok(())
                }) as crate::threads::waits::Task
            })
            .collect();
        let results = crate::threads::waits::help(m, &incoming, tasks);
        if let Some(error) = results.into_iter().find_map(Result::err) {
            TASK_ERRORS.fetch_add(1, Relaxed);
            panic!("parallel-shadow publish: lane task failed: {error}");
        }
    } else {
        for lane in lanes {
            lane.run(m, &incoming);
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
