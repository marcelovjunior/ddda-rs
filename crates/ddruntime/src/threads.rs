//! Guest threads, the guest lock and kernel objects (docs/RUNTIME.md,
//! "Platform services and guest threads").
//!
//! Every guest thread is a host thread with its own `Cpu` and Rust call
//! stack (a thread blocked in a service keeps its translated callers'
//! frames). One guest thread runs at a time: the holder of the guest lock.
//! A thread gives the lock up only inside services: when it waits (wait
//! functions, a contended critical section, Sleep), and at API calls once
//! its time slice (2 ms) is used up and another thread is ready. Translated
//! code is never interrupted between instructions, which is exact for this
//! game: its atomic operations are all imported Interlocked* calls (the
//! lifter emits no `lock`-prefixed instructions), so every interleaving
//! point the guest can observe is a service call. A loop that spins
//! without calling any service would keep the lock; none is known yet.
//!
//! Last-error and TLS values live in the caller's cached host-thread record,
//! keyed by runtime identity and guest thread id. They require no service lock
//! or save/restore at a hand-off (L153).
//!
//! Parallel mode (brief L42, `DDRUNTIME_PARALLEL=1`, off by default):
//! translated code runs without the guest lock, so guest threads execute
//! it at the same time; every service call takes the lock for its
//! duration (re-entrantly: a service that calls back into guest code may
//! reach another service). All runtime state therefore stays exactly as
//! serialised as in the serial mode; what changes is that translated code
//! of different threads interleaves at instruction granularity, as on a
//! multi-core machine. That is sound for this game because its translated
//! code contains no lock-prefixed instruction and no cmpxchg/xadd, and its
//! three memory xchg instructions address the thread's own stack (L42
//! Results); guest memory accesses are atomic with acquire/release order
//! (crate::memory), which is x86's TSO order for single accesses, and the
//! Interlocked services are host atomics on the guest word (crate::win32).
//!
//! Kernel objects follow the documented Win32 contracts: events (manual
//! and auto reset), semaphores, mutexes (recursive, owned by a thread) and
//! threads (signalled when they end), behind handles; waits for one or
//! several objects with timeouts.
use crate::dispatch::Registry;
use crate::memory::GuestMemory;
use crate::runtime::{self, Runtime};
use crate::services::{arg, ret, MAIN_THREAD_ID};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[path = "sync_profile.rs"]
pub(crate) mod sync_profile;
#[path = "thread_scheduling.rs"]
mod scheduling;

pub const WAIT_OBJECT_0: u32 = 0;
pub const WAIT_ABANDONED_0: u32 = 0x80;
pub const WAIT_TIMEOUT: u32 = 0x102;
pub const WAIT_FAILED: u32 = 0xFFFF_FFFF;
pub const INFINITE: u32 = 0xFFFF_FFFF;
pub const STILL_ACTIVE: u32 = 259;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_ALREADY_EXISTS: u32 = 183;
const ERROR_NOT_OWNER: u32 = 288;
const ERROR_TOO_MANY_POSTS: u32 = 298;
const CREATE_SUSPENDED: u32 = 0x4;
const STACK_SIZE_PARAM_IS_A_RESERVATION: u32 = 0x1_0000;
/// The pseudo handle GetCurrentThread returns.
pub const CURRENT_THREAD: u32 = 0xFFFF_FFFE;

/// Kernel object handles (distinct from file handles and window handles).
pub const HANDLE_BASE: u32 = 0x00F0_0004;
/// Where thread stacks are reserved (lazily backed): between the
/// VirtualAlloc region (crate::win32) and the main stack.
pub const STACKS_BASE: u32 = 0x7800_0000;
pub const STACKS_SIZE: u32 = 0x0600_0000;
/// The default stack reservation (the exe's SizeOfStackReserve is 1 MB).
const DEFAULT_STACK: u32 = 0x0010_0000;
/// Where a thread's start routine returns to (never executed).
pub const THREAD_RETURN: u32 = 0xFFFF_FFD0;
/// A guest thread that faults ends with this exit code
/// (STATUS_ACCESS_VIOLATION), and the fault is kept for the session.
pub const FAULT_EXIT: u32 = 0xC000_0005;
const SLICE: Duration = Duration::from_millis(2);

// --- the guest lock -----------------------------------------------------------

#[derive(Default)]
struct LockState {
    held: bool,
    holder: u32,
    holder_host: Option<std::thread::ThreadId>,
    /// Nested service entries of the holder (parallel mode; 1 otherwise).
    depth: u32,
    /// Threads ready to take the lock.
    waiting: u32,
    /// Incremented on every acquisition (a yielding thread waits for another).
    acquisitions: u64,
    /// Threads blocked in a kernel wait ([`block_on`]), by wait id.
    sleepers: HashMap<u64, Sleeper>,
    next_sleeper: u64,
    /// Suspend counts by thread id (a suspended thread does not take the lock).
    suspended: HashMap<u32, u32>,
}

/// A thread blocked in a kernel wait: the keys whose signal wakes it and a
/// condition variable of its own. A signal wakes only the
/// threads it concerns, and handing the guest lock over wakes none of them
/// (L69: with one shared condition variable, every LeaveCriticalSection,
/// SetEvent and lock release woke all of the game's ~20 idle worker
/// threads, and each took the guest lock to recheck its wait).
struct Sleeper {
    keys: Box<[u64]>,
    woken: bool,
    cv: Arc<Condvar>,
}

/// How long a thread waiting for the guest lock spins before it sleeps.
const LOCK_SPIN: Duration = Duration::from_micros(30);

pub struct GuestLock {
    state: Mutex<LockState>,
    cv: Condvar,
    /// A copy of `LockState::held` that a thread waiting for the lock spins
    /// on before it sleeps (L87): most services hold the lock ~1 us, so a
    /// short spin takes it without an OS sleep and wake-up (the gameplay
    /// lock statistics showed ~100,000 hand-offs per second).
    held_hint: std::sync::atomic::AtomicBool,
    /// Translated code runs without the lock (see the module comment).
    parallel: std::sync::atomic::AtomicBool,
    /// Event handles -> (object, state), read without the guest lock by
    /// SetEvent, ResetEvent and WaitForSingleObject (L86); kept by the
    /// handle's creation and CloseHandle (under the guest lock).
    events: std::sync::RwLock<crate::fasthash::FxMap<u32, (u32, Arc<EventCell>)>>,
    event_cache_id: u64,
    event_epoch: std::sync::atomic::AtomicU64,
    semaphores: std::sync::RwLock<crate::fasthash::FxMap<u32, (u32, Arc<SemaphoreCell>)>>,
    semaphore_gate: Arc<Mutex<()>>,
}

impl Default for GuestLock {
    /// The thread that creates the runtime holds the lock. The mode comes
    /// from `DDRUNTIME_PARALLEL` (1: parallel); [`set_parallel`] changes it.
    fn default() -> Self {
        SLICE_START.with(|t| t.set(Some(Instant::now())));
        let parallel = std::env::var("DDRUNTIME_PARALLEL").is_ok_and(|v| v == "1");
        if parallel {
            ANY_PARALLEL.store(true, std::sync::atomic::Ordering::Relaxed);
            ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        GuestLock {
            state: Mutex::new(LockState {
                held: true,
                holder: current_tid(),
                holder_host: None,
                depth: 1,
                ..LockState::default()
            }),
            cv: Condvar::new(),
            held_hint: std::sync::atomic::AtomicBool::new(true),
            parallel: std::sync::atomic::AtomicBool::new(parallel),
            events: Default::default(),
            event_cache_id: NEXT_EVENT_CACHE_ID.fetch_add(1, SeqCst),
            event_epoch: 0.into(),
            semaphores: Default::default(), semaphore_gate: Default::default(),
        }
    }
}

/// Set once any runtime of the process runs in parallel mode: until then a
/// service call skips the runtime lookup (the serial mode's cost stays as
/// it was).
static ANY_PARALLEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static NEXT_EVENT_CACHE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Whether this runtime runs guest threads in parallel.
pub fn parallel() -> bool {
    runtime::with(|rt| rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed))
}

/// Choose the mode for this runtime (before it starts guest threads).
pub fn set_parallel(on: bool) {
    if on {
        ANY_PARALLEL.store(true, std::sync::atomic::Ordering::Relaxed);
        ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    runtime::pin().lock.parallel.store(on, std::sync::atomic::Ordering::Relaxed);
}

thread_local! {
    static SLICE_START: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Make this host thread guest thread `tid` (tests of other modules).
#[cfg(test)]
pub(crate) fn set_test_tid(tid: u32) {
    runtime::local(|l| l.guest_tid.set(tid));
}

/// The running guest thread's id.
pub fn current_tid() -> u32 {
    runtime::local(|l| l.guest_tid.get())
}

impl GuestLock {
    pub(crate) fn identity(&self) -> u64 { self.event_cache_id }

    fn lock(&self) -> std::sync::MutexGuard<'_, LockState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Give the lock up (the caller holds it).
    fn release_locked(&self, s: &mut LockState) {
        debug_assert!(s.held && s.holder == current_tid());
        s.held = false;
        self.held_hint.store(false, std::sync::atomic::Ordering::Release);
        stats::released();
        self.cv.notify_all();
    }

    /// Take the lock when it is free and this thread is not suspended,
    /// with `depth` nested entries.
    fn acquire_locked<'a>(&'a self, mut s: std::sync::MutexGuard<'a, LockState>, depth: u32) {
        let me = current_tid();
        let asked = stats::on().then(Instant::now);
        s.waiting += 1;
        signal_preempt(&s);
        let mut spun = false;
        if s.held { sync_profile::service_contention(); }
        while s.held || s.suspended.get(&me).copied().unwrap_or(0) > 0 {
            // L87: spin briefly (only for a held lock, not a suspension)
            // before sleeping on the condvar; the state is rechecked under
            // the mutex either way, so this changes timing only
            if !spun && s.held && s.suspended.get(&me).copied().unwrap_or(0) == 0 {
                drop(s);
                let t0 = Instant::now();
                while self.held_hint.load(std::sync::atomic::Ordering::Acquire) && t0.elapsed() < LOCK_SPIN {
                    std::hint::spin_loop();
                }
                spun = true;
                s = self.lock();
                continue;
            }
            if asked.is_some() && s.held {
                stats::blocked(s.holder, s.holder_host);
            }
            s = self.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            spun = false;
        }
        if let Some(t) = asked {
            stats::acquired(t);
        }
        s.waiting -= 1;
        signal_preempt(&s);
        s.held = true;
        self.held_hint.store(true, std::sync::atomic::Ordering::Release);
        s.holder = me;
        s.holder_host = asked.map(|_| std::thread::current().id());
        s.depth = depth;
        s.acquisitions += 1;
        SLICE_START.with(|t| t.set(Some(Instant::now())));
    }
}

/// Run host-only work without the guest lock (parallel mode): work that
/// touches neither the runtime's state nor anything the guest can see
/// (GPU submission and presentation, reads from host files into host
/// buffers). Only the outermost service gives the lock up (a nested one may
/// run inside an outer service that holds runtime state borrowed), and `f`
/// must not borrow runtime state itself. In the serial mode, or nested,
/// `f` just runs.
pub(crate) fn unlocked<R>(f: impl FnOnce() -> R) -> R {
    // L93: a D3D service that also holds the guest lock keeps it: taking it
    // back while holding the D3D lock would invert the lock order (guest,
    // then D3D)
    if crate::d3d9dev::holds_device_lock() {
        return f();
    }
    if SERVICE_TAIL_ALLOWED.with(Cell::get) && release_service_tail() {
        return f();
    }
    let rt = runtime::pin();
    if !rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed) {
        return f();
    }
    {
        let mut s = rt.lock.lock();
        if !(s.held && s.holder == current_tid() && s.depth == 1) {
            drop(s);
            return f();
        }
        s.depth = 0;
        rt.lock.release_locked(&mut s);
    }
    struct Again(runtime::Pinned);
    impl Drop for Again {
        fn drop(&mut self) {
            let s = self.0.lock.lock();
            self.0.lock.acquire_locked(s, 1);
        }
    }
    let _again = Again(rt);
    f()
}

thread_local! {
    static SERVICE_TAIL_RELEASED: Cell<bool> = const { Cell::new(false) };
    static SERVICE_TAIL_ALLOWED: Cell<bool> = const { Cell::new(false) };
}

/// Finish an outermost service without reacquiring the guest lock. After
/// this call only host-owned backend work and CPU/stack return are allowed:
/// no GuestCell, callbacks, or further services. Nested/direct calls retain
/// their caller's lock. The backend mutex must already be held to preserve
/// submission order, exactly as for `unlocked`.
fn release_service_tail() -> bool {
    if runtime::local(|l| l.service_depth.get()) != 1 {
        return false;
    }
    let rt = runtime::pin();
    if !rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let mut s = rt.lock.lock();
    if s.held && s.holder == current_tid() && s.depth == 1 {
        s.depth = 0;
        rt.lock.release_locked(&mut s);
        SERVICE_TAIL_RELEASED.with(|v| v.set(true));
        return true;
    }
    false
}

/// Run `f` holding the guest lock: from inside [`unlocked`] (parallel
/// mode) it takes the lock for `f` only; where the thread holds it already
/// (or in the serial mode) `f` just runs. No time slice ends here.
pub(crate) fn with_lock<R>(f: impl FnOnce() -> R) -> R {
    let rt = runtime::pin();
    if !rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed) {
        return f();
    }
    enter(&rt);
    struct Leave(runtime::Pinned);
    impl Drop for Leave {
        fn drop(&mut self) {
            leave(&self.0);
        }
    }
    let _leave = Leave(rt);
    f()
}

/// Per-service guest-lock statistics (L64), on with `DDRUNTIME_LOCK_STATS=1`:
/// for each service, its calls, how long it held the guest lock and how long
/// it waited to take it (lock contention; not the time a wait function
/// spends waiting for its object). Nested services count in their callers
/// too. [`lock_stats`] formats them; ddplay and the drivers print them at
/// exit.
pub(crate) mod stats {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    #[derive(Default, Clone, Copy)]
    pub(crate) struct Stat {
        pub calls: u64,
        pub hold: Duration,
        pub max_hold: Duration,
        pub wait: Duration,
        pub max_wait: Duration,
        pub blocks: u64,
        pub contended_calls: u64,
        pub elapsed: Duration,
        pub acquisitions: u64,
    }

    /// 0: not read yet, 1: off, 2: on.
    static ON: AtomicU8 = AtomicU8::new(0);
    /// Blocked kernel waits woken to recheck (signals reaching a waiter).
    static WOKEN: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn on() -> bool {
        match ON.load(Ordering::Relaxed) {
            0 => {
                let on = std::env::var("DDRUNTIME_LOCK_STATS").is_ok_and(|v| v == "1");
                if ON.compare_exchange(0, if on { 2 } else { 1 }, Ordering::Relaxed, Ordering::Relaxed).is_ok() && on {
                    report_periodically();
                }
                on
            }
            v => v == 2,
        }
    }

    /// With `DDRUNTIME_LOCK_STATS_EVERY=<seconds>`, print the statistics so
    /// far (cumulative) to stderr at that interval, not only at the end.
    fn report_periodically() {
        let every = std::env::var("DDRUNTIME_LOCK_STATS_EVERY").ok().and_then(|v| v.parse::<u64>().ok());
        let Some(every) = every.filter(|&s| s > 0) else {
            return;
        };
        let start = Instant::now();
        let _ = std::thread::Builder::new().name("lock-stats".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_secs(every));
            if let Some(s) = super::lock_stats() {
                eprintln!("lock stats at {:.0} s:\n{s}", start.elapsed().as_secs_f64());
            }
        });
    }

    /// Turn the statistics on, from zero (tests).
    #[cfg(test)]
    pub(crate) fn force_on() {
        ON.store(2, Ordering::Relaxed);
        WOKEN.store(0, Ordering::Relaxed);
        table().lock().unwrap_or_else(|e| e.into_inner()).clear();
        blockers().lock().unwrap_or_else(|e| e.into_inner()).clear();
        FRAME.with(|f| f.borrow_mut().clear());
        FRAME_NUMBER.with(|n| n.set(0));
    }

    /// A signal woke a blocked kernel wait.
    pub(super) fn woken() {
        if on() {
            WOKEN.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn woken_total() -> u64 {
        WOKEN.load(Ordering::Relaxed)
    }

    pub(crate) fn table() -> &'static Mutex<HashMap<String, Stat>> {
        static T: OnceLock<Mutex<HashMap<String, Stat>>> = OnceLock::new();
        T.get_or_init(Default::default)
    }

    fn active() -> &'static Mutex<HashMap<std::thread::ThreadId, (u32, String)>> {
        static T: OnceLock<Mutex<HashMap<std::thread::ThreadId, (u32, String)>>> = OnceLock::new();
        T.get_or_init(Default::default)
    }

    fn blockers() -> &'static Mutex<HashMap<String, u64>> {
        static T: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
        T.get_or_init(Default::default)
    }

    pub(crate) struct Call {
        name: String,
        previous: Option<(u32, String)>,
        before: (Duration, Duration),
        blocks: u64,
        acquisitions: u64,
        start: Instant,
    }

    pub(crate) fn start(name: String) -> Call {
        let previous = active().lock().unwrap_or_else(|e| e.into_inner())
            .insert(std::thread::current().id(), (super::current_tid(), name.clone()));
        Call { name, previous, before: totals(), blocks: BLOCKS.with(Cell::get),
            acquisitions: ACQUISITIONS.with(Cell::get), start: Instant::now() }
    }

    impl Drop for Call {
        fn drop(&mut self) {
            let blocks = BLOCKS.with(Cell::get) - self.blocks;
            let elapsed = self.start.elapsed();
            let key = format!("tid={:#x} {}", super::current_tid(), self.name);
            record(key, self.before, blocks, elapsed, ACQUISITIONS.with(Cell::get) - self.acquisitions);
            FRAME.with(|frame| {
                let mut frame = frame.borrow_mut();
                let s = frame.entry(self.name.clone()).or_default();
                s.calls += 1;
                s.blocks += blocks;
                s.contended_calls += u64::from(blocks != 0);
                s.wait += totals().1 - self.before.1;
                s.elapsed += elapsed;
                s.acquisitions += ACQUISITIONS.with(Cell::get) - self.acquisitions;
                // A Present call delimits this thread's frame (including
                // failed Presents); no cross-thread frame ownership assumed.
                if self.name.starts_with("IDirect3DDevice9::Present (")
                    || self.name.starts_with("IDirect3DSwapChain9::Present (")
                {
                    FRAME_NUMBER.with(|n| {
                        n.set(n.get() + 1);
                        eprintln!("lock frame tid={:#x} frame={} calls/contended_calls/blocks/wait_us/service_us: {:?}",
                            super::current_tid(), n.get(), frame.iter().map(|(name, s)|
                                (name, s.calls, s.contended_calls, s.blocks, s.wait.as_micros(), s.elapsed.as_micros())).collect::<Vec<_>>());
                    });
                    frame.clear();
                }
            });
            let mut a = active().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(previous) = self.previous.take() {
                a.insert(std::thread::current().id(), previous);
            } else {
                a.remove(&std::thread::current().id());
            }
        }
    }

    /// A condition-variable block, attributed to the holder observed while
    /// the guest-lock mutex is held. Reblocks after wakeup count separately.
    pub(super) fn blocked(holder: u32, holder_host: Option<std::thread::ThreadId>) {
        BLOCKS.with(|n| n.set(n.get() + 1));
        let a = active().lock().unwrap_or_else(|e| e.into_inner());
        let waiter = a.get(&std::thread::current().id()).map(|(_, s)| s.as_str()).unwrap_or("outside service");
        let owner = holder_host.and_then(|id| a.get(&id)).map(|(_, s)| s.as_str()).unwrap_or("outside service");
        let key = format!("tid={:#x} {waiter} <- tid={holder:#x} {owner}", super::current_tid());
        *blockers().lock().unwrap_or_else(|e| e.into_inner()).entry(key).or_default() += 1;
    }

    pub(super) fn blocker_report() -> String {
        let t = blockers().lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<_> = t.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        rows.into_iter().map(|(name, count)| format!("{count:>9} blocks {name}\n")).collect()
    }

    #[cfg(test)]
    pub(super) fn frame_service(name: &str) -> Stat {
        FRAME.with(|f| f.borrow()[name])
    }

    thread_local! {
        static HELD_SINCE: Cell<Option<Instant>> = const { Cell::new(None) };
        static HELD: Cell<Duration> = const { Cell::new(Duration::ZERO) };
        static WAITED: Cell<Duration> = const { Cell::new(Duration::ZERO) };
        static BLOCKS: Cell<u64> = const { Cell::new(0) };
        static ACQUISITIONS: Cell<u64> = const { Cell::new(0) };
        static FRAME: RefCell<std::collections::BTreeMap<String, Stat>> = RefCell::new(Default::default());
        static FRAME_NUMBER: Cell<u64> = const { Cell::new(0) };
    }

    /// This thread took the lock after asking at `asked`.
    pub(super) fn acquired(asked: Instant) {
        ACQUISITIONS.with(|n| n.set(n.get() + 1));
        let now = Instant::now();
        WAITED.with(|w| w.set(w.get() + (now - asked)));
        HELD_SINCE.with(|h| h.set(Some(now)));
    }

    /// This thread gave the lock up.
    pub(super) fn released() {
        if let Some(t) = HELD_SINCE.with(|h| h.take()) {
            HELD.with(|d| d.set(d.get() + t.elapsed()));
        }
    }

    /// (held, waited) by this thread so far.
    pub(crate) fn totals() -> (Duration, Duration) {
        let held = HELD.with(Cell::get) + HELD_SINCE.with(Cell::get).map_or(Duration::ZERO, |t| t.elapsed());
        (held, WAITED.with(Cell::get))
    }

    /// Add one service call that started at `before` (totals()).
    fn record(name: String, before: (Duration, Duration), blocks: u64, elapsed: Duration, acquisitions: u64) {
        let (held, waited) = totals();
        let (hold, wait) = (held - before.0, waited - before.1);
        let mut t = table().lock().unwrap_or_else(|e| e.into_inner());
        let s = t.entry(name).or_default();
        s.calls += 1;
        s.hold += hold;
        s.max_hold = s.max_hold.max(hold);
        s.wait += wait;
        s.max_wait = s.max_wait.max(wait);
        s.blocks += blocks;
        s.contended_calls += u64::from(blocks != 0);
        s.elapsed += elapsed;
        s.acquisitions += acquisitions;
    }
}

/// The lock statistics so far (`DDRUNTIME_LOCK_STATS=1`), the services
/// holding the lock longest first; None when they are off.
pub fn lock_stats() -> Option<String> {
    if !stats::on() {
        return None;
    }
    let t = stats::table().lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<_> = t.iter().collect();
    rows.sort_by(|a, b| b.1.hold.cmp(&a.1.hold));
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let mut out = format!("blocked kernel waits woken: {}
guest lock by service: calls, held ms (total, mean, max), waited ms (total, mean, max)
", stats::woken_total());
    for (name, s) in rows {
        let n = s.calls.max(1) as f64;
        out += &format!("{:>9} {:>10.1} {:>8.3} {:>8.2}  {:>10.1} {:>8.3} {:>8.2}  {name}
",
                        s.calls, ms(s.hold), ms(s.hold) / n, ms(s.max_hold), ms(s.wait), ms(s.wait) / n, ms(s.max_wait));
        out += &format!("          acquisitions={} contended_calls={} blocks={} service_ms={:.3}\n",
            s.acquisitions, s.contended_calls, s.blocks, ms(s.elapsed));
    }
    drop(t);
    out += &stats::blocker_report();
    Some(out)
}

/// Whether this host thread holds the guest lock (tests, diagnostics).
pub fn holds_guest_lock() -> bool {
    holds_lock()
}

/// Whether this host thread holds the guest lock.
pub(crate) fn holds_lock() -> bool {
    let rt = runtime::pin();
    let s = rt.lock.lock();
    s.held && s.holder == current_tid()
}

/// Whether this thread is running a service (crate::dispatch).
#[cfg(test)]
pub(crate) fn in_service() -> bool {
    runtime::local(|l| l.service_depth.get()) > 0
}

/// Holds the guest lock for one service call (parallel mode); in the
/// serial mode the caller already holds it, and only the time slice is
/// checked.
pub(crate) struct ServiceLock(Option<runtime::Pinned>);

pub(crate) fn service() -> ServiceLock {
    if !ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed) {
        maybe_yield();
        return ServiceLock(None);
    }
    let rt = runtime::pin();
    if !rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed) {
        // an API call: where a guest thread's time slice may end
        maybe_yield();
        return ServiceLock(None);
    }
    enter(&rt);
    ServiceLock(Some(rt))
}

/// A lock-free service's entry (crate::dispatch::Binding::LockFree): no
/// lock in parallel mode; in the serial mode the API call where a time
/// slice may end, as for any service.
pub(crate) fn service_lock_free() -> ServiceLock {
    if !ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed)
        || !runtime::with(|rt| rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed))
    {
        maybe_yield();
    }
    ServiceLock(None)
}

impl Drop for ServiceLock {
    fn drop(&mut self) {
        if let Some(rt) = self.0.take() {
            if !SERVICE_TAIL_RELEASED.with(|v| v.replace(false)) {
                leave(&rt);
            }
        }
    }
}

/// Take the lock (or nest one level deeper if this thread holds it).
fn enter(rt: &Runtime) {
    let s = rt.lock.lock();
    if s.held && s.holder == current_tid() {
        let mut s = s;
        s.depth += 1;
        return;
    }
    rt.lock.acquire_locked(s, 1);
}

/// Undo one [`enter`]; the outermost gives the lock up.
fn leave(rt: &Runtime) {
    let mut s = rt.lock.lock();
    debug_assert!(s.held && s.holder == current_tid() && s.depth > 0);
    if s.depth > 1 {
        s.depth -= 1;
        return;
    }
    s.depth = 0;
    rt.lock.release_locked(&mut s);
}

/// Run guest code from a thread that holds the lock outside any service
/// (a session's top-level call): in parallel mode without the lock, so
/// the other threads' services can run meanwhile.
pub fn without_lock<R>(f: impl FnOnce() -> R) -> R {
    let rt = runtime::pin();
    let release = rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed) && {
        let s = rt.lock.lock();
        s.held && s.holder == current_tid()
    };
    if !release {
        return f();
    }
    let depth = {
        let mut s = rt.lock.lock();
        let d = s.depth;
        rt.lock.release_locked(&mut s);
        d
    };
    struct Reacquire(runtime::Pinned, u32);
    impl Drop for Reacquire {
        fn drop(&mut self) {
            let s = self.0.lock.lock();
            self.0.lock.acquire_locked(s, self.1);
        }
    }
    let _again = Reacquire(rt, depth);
    f()
}

/// Wait keys: what a blocked thread waits for ([`block_on`]), so that a
/// state change ([`signal_key`]) wakes only the threads waiting for it.
pub(crate) mod key {
    /// A kernel object (event, semaphore, mutex, thread) by object id.
    pub(crate) fn object(o: u32) -> u64 {
        1 << 32 | o as u64
    }
    /// A critical section by guest address.
    pub(crate) fn critical_section(cs: u32) -> u64 {
        2 << 32 | cs as u64
    }
    /// An APC queued to a guest thread.
    pub(crate) fn apc(tid: u32) -> u64 {
        3 << 32 | tid as u64
    }
    /// An XAudio2 engine (its release ends its worker).
    pub(crate) fn audio_engine(engine: u32) -> u64 {
        4 << 32 | engine as u64
    }
    /// The end of an audio pass (crate::audio).
    pub(crate) const AUDIO_PASS: u64 = 5 << 32;
    /// The audio test clock advanced (crate::audio).
    pub(crate) const AUDIO_CLOCK: u64 = 6 << 32;
}

/// Note that the state behind `key` changed: the threads waiting for it
/// recheck.
pub(crate) fn signal_key(key: u64) {
    let rt = runtime::pin();
    wake(&mut rt.lock.lock(), Some(key));
}

/// Wake the threads waiting for `key` (None: every blocked thread).
fn wake(s: &mut LockState, key: Option<u64>) {
    for w in s.sleepers.values_mut() {
        if !w.woken && key.is_none_or(|k| w.keys.contains(&k)) {
            w.woken = true;
            w.cv.notify_one();
            stats::woken();
        }
    }
}

/// Block until one of `keys` is signalled, a thread exits (which may
/// concern any waiter) or `deadline`, giving the lock to the other guest
/// threads meanwhile. The caller rechecks its condition. Whoever changes
/// the state the caller checked holds the guest lock, so signals only after
/// this thread is registered: no wakeup is lost.
pub(crate) fn block_on(keys: &[u64], deadline: Option<Instant>) {
    wait_until(keys, deadline, || false)
}

/// Block until one of `keys` is signalled, a thread exits or `deadline`,
/// unless `cond` holds; the caller rechecks its condition. Unlike
/// [`block_on`] the caller need not hold the guest lock (a lock-free
/// service in parallel mode, L86); if it does, the lock is given to the
/// other guest threads meanwhile. `cond` is checked after this thread is
/// registered, under the lock state's mutex, which [`signal_key`] takes: a
/// state change made without the guest lock and signalled afterwards
/// cannot be lost, as long as the changer signals whenever a waiter may
/// exist (the waiter counts it before checking; see the critical sections
/// and EventCell).
pub(crate) fn wait_until(keys: &[u64], deadline: Option<Instant>, cond: impl Fn() -> bool) {
    sync_profile::blocking();
    let keys: Box<[u64]> = keys.into();
    let rt = runtime::pin();
    let mut s = rt.lock.lock();
    let held = s.held && s.holder == current_tid();
    let depth = s.depth;
    let id = s.next_sleeper;
    s.next_sleeper += 1;
    let cv = Arc::new(Condvar::new());
    s.sleepers.insert(id, Sleeper { keys, woken: false, cv: cv.clone() });
    if cond() {
        s.sleepers.remove(&id);
        return;
    }
    if held {
        rt.lock.release_locked(&mut s);
    }
    while !s.sleepers[&id].woken {
        match deadline {
            None => s = cv.wait(s).unwrap_or_else(|e| e.into_inner()),
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    break;
                }
                s = cv.wait_timeout(s, d - now).unwrap_or_else(|e| e.into_inner()).0;
            }
        }
    }
    s.sleepers.remove(&id);
    if held {
        rt.lock.acquire_locked(s, depth);
    }
}

/// Let another ready thread run first (Sleep(0), SwitchToThread, the end
/// of a time slice).
pub(crate) fn yield_now() {
    let rt = runtime::pin();
    let mut s = rt.lock.lock();
    if s.waiting == 0 {
        SLICE_START.with(|t| t.set(Some(Instant::now())));
        return;
    }
    let seen = s.acquisitions;
    let depth = s.depth;
    rt.lock.release_locked(&mut s);
    while s.acquisitions == seen && s.waiting > 0 {
        s = rt.lock.cv.wait(s).unwrap_or_else(|e| e.into_inner());
    }
    rt.lock.acquire_locked(s, depth);
}

/// Lifted calls become preemption points while a runnable guest thread
/// waits for the lock (suspended threads wait too, but cannot take it).
fn signal_preempt(s: &LockState) {
    // `preempt` already refuses to yield once ANY_PARALLEL is set. Avoid
    // scanning suspended threads twice per acquisition and enabling the
    // lifted-call slow path for a hook which cannot do anything (L82).
    let runnable = !ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed)
        && s.waiting as usize > s.suspended.values().filter(|n| **n > 0).count();
    ddnative_host::PREEMPT.store(runnable, std::sync::atomic::Ordering::Relaxed);
}

/// Marks this thread as running a service until dropped.
pub(crate) struct ServiceScope {
    previous_tail: bool,
}

impl ServiceScope {
    #[cfg(test)]
    pub(crate) fn enter() -> Self {
        Self::enter_with_tail(false)
    }

    pub(crate) fn enter_with_tail(tail: bool) -> Self {
        runtime::local(|l| l.service_depth.set(l.service_depth.get() + 1));
        Self { previous_tail: SERVICE_TAIL_ALLOWED.with(|v| v.replace(tail)) }
    }
}

impl Drop for ServiceScope {
    fn drop(&mut self) {
        runtime::local(|l| l.service_depth.set(l.service_depth.get() - 1));
        SERVICE_TAIL_ALLOWED.with(|v| v.set(self.previous_tail));
    }
}

/// The preemption hook of lifted calls (ddnative_host::set_preempt_hook):
/// in serial mode, the lock holder yields when its time slice is used up.
/// The guest MXCSR is live in the host register inside lifted code, so it
/// is kept across the switch.
pub(crate) fn preempt() {
    thread_local! {
        static CALLS: Cell<u32> = const { Cell::new(0) };
    }
    // the check below reads the clock: once per 1,024 lifted calls is
    // plenty (the rate lives here, not in the shards' ddnative-base, L60)
    let due = CALLS.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n % 1024 == 0
    });
    if !due || ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed) || runtime::local(|l| l.service_depth.get()) > 0 {
        return;
    }
    let rt = runtime::pin();
    {
        let s = rt.lock.lock();
        if !(s.held && s.holder == current_tid()) {
            return;
        }
    }
    let mxcsr = ddnative_base::x86::stmxcsr();
    maybe_yield();
    ddnative_base::x86::ldmxcsr(mxcsr);
}

/// A lifted function entry while a runnable thread waits (memory.rs): the
/// clock is read once per 256 entries.
pub(crate) fn preempt_entry() {
    thread_local! {
        static ENTRIES: Cell<u32> = const { Cell::new(0) };
    }
    let due = ENTRIES.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n % 256 == 0
    });
    if due {
        preempt();
    }
}

/// At an API call: yield if the time slice is used up.
pub(crate) fn maybe_yield() {
    let due = SLICE_START.with(|t| t.get().is_some_and(|t0| t0.elapsed() >= SLICE));
    if due {
        yield_now();
    }
}

/// Sleep for `duration` without the lock.
fn sleep_unlocked(duration: Duration) {
    let rt = runtime::pin();
    let depth = {
        let mut s = rt.lock.lock();
        let d = s.depth;
        rt.lock.release_locked(&mut s);
        d
    };
    std::thread::sleep(duration);
    let s = rt.lock.lock();
    rt.lock.acquire_locked(s, depth);
}

// --- kernel objects -----------------------------------------------------------

/// An event's state in host atomics, so that SetEvent, ResetEvent and a
/// wait for one event take no guest lock (L86: the game calls them ~25,000
/// times a second and they queued for the lock).
#[derive(Debug)]
pub struct EventCell {
    pub manual: bool,
    set: std::sync::atomic::AtomicBool,
    /// threads in a wait that includes this event (counted before they
    /// check it): SetEvent signals the event's key only then
    waiters: std::sync::atomic::AtomicU32,
    address_waiters: std::sync::atomic::AtomicU32,
}

impl EventCell {
    pub fn new(manual: bool, set: bool) -> Arc<EventCell> {
        Arc::new(EventCell { manual, set: set.into(), waiters: 0.into(), address_waiters: 0.into() })
    }

    pub fn is_set(&self) -> bool {
        self.set.load(SeqCst)
    }

    /// A satisfied wait: an auto-reset event is reset, so that one waiter
    /// gets it; false if it was not set.
    fn take(&self) -> bool {
        if self.manual {
            self.is_set()
        } else {
            self.set.compare_exchange(true, false, SeqCst, SeqCst).is_ok()
        }
    }

    /// Wake the threads waiting for it without changing it, so that they run
    /// their rechecks (a published task, L172).
    fn poke(&self, object: u32) {
        #[cfg(windows)]
        if self.address_waiters.load(SeqCst) > 0 {
            address_wait::wake(&self.set);
        }
        signal_key(key::object(object));
    }

    /// Set or reset; the threads waiting for it recheck (`object`: its id).
    fn store(&self, object: u32, value: bool) {
        self.set.store(value, SeqCst);
        #[cfg(windows)]
        if value && self.address_waiters.load(SeqCst) > 0 {
            address_wait::wake(&self.set);
        }
        if value && self.waiters.load(SeqCst) > 0 {
            signal_key(key::object(object));
        }
    }
}

/// Semaphore counts can change outside the service lock. One narrow gate
/// covers count mutations, including a multiwait's readiness/commit phase;
/// therefore a competing single wait cannot partially consume a wait-all.
#[derive(Debug)]
pub struct SemaphoreCell {
    count: std::sync::atomic::AtomicU32,
    max: u32,
    gate: Arc<Mutex<()>>,
}
impl SemaphoreCell {
    fn new(count: u32, max: u32) -> Arc<Self> {
        Arc::new(Self { count: count.into(), max, gate: runtime::with(|rt| rt.lock.semaphore_gate.clone()) })
    }
    fn ready(&self) -> bool { self.count.load(SeqCst) != 0 }
    fn take_locked(&self) -> bool {
        let count = self.count.load(SeqCst);
        if count == 0 { return false; }
        self.count.store(count - 1, SeqCst);
        true
    }
    fn take(&self) -> bool {
        let _gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        self.take_locked()
    }
    fn release(&self, n: u32) -> Result<u32, u32> {
        if (n as i32) <= 0 { return Err(87); } // ERROR_INVALID_PARAMETER
        let _gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        let count = self.count.load(SeqCst);
        let next = count.checked_add(n).filter(|c| *c <= self.max)
            .ok_or(ERROR_TOO_MANY_POSTS)?;
        self.count.store(next, SeqCst);
        Ok(count)
    }
}
fn semaphore_of(h: u32) -> Option<(u32, Arc<SemaphoreCell>)> {
    runtime::with(|rt| rt.lock.semaphores.read().unwrap_or_else(|e| e.into_inner()).get(&h).cloned())
}

#[derive(Debug)]
pub enum Object {
    Event(Arc<EventCell>),
    Semaphore(Arc<SemaphoreCell>),
    Mutex { owner: u32, recursion: u32, abandoned: bool },
    Thread { tid: u32, exit: Option<u32> },
}

#[derive(Default)]
pub(crate) struct Kernel {
    handles: HashMap<u32, u32>,
    objects: HashMap<u32, Object>,
    names: HashMap<String, u32>,
    next_handle: u32,
    next_object: u32,
    next_tid: u32,
    next_stack: u32,
    /// thread id -> its object
    threads: HashMap<u32, u32>,
    // Keep the OS handle alive for scheduling calls, including before the
    // guest's first ResumeThread. Dropping a JoinHandle detaches; never join
    // while holding the runtime lock. Released with the final guest handle.
    host_threads: HashMap<u32, std::thread::JoinHandle<()>>,
    /// Queued asynchronous procedure calls per thread: (routine, three
    /// stdcall arguments), run at the thread's next alertable wait.
    apcs: HashMap<u32, std::collections::VecDeque<(u32, [u32; 3])>>,
    /// Faults that ended guest threads: (thread id, message).
    pub(crate) faults: Vec<(u32, String)>,
}

impl Kernel {
    fn add(&mut self, o: Object) -> u32 {
        self.next_object += 1;
        self.objects.insert(self.next_object, o);
        self.next_object
    }
    fn handle(&mut self, object: u32) -> u32 {
        if self.next_handle == 0 {
            self.next_handle = HANDLE_BASE;
        }
        let h = self.next_handle;
        self.next_handle += 4;
        self.handles.insert(h, object);
        h
    }
    fn object_of(&self, h: u32) -> Option<u32> {
        if h == CURRENT_THREAD {
            return self.threads.get(&current_tid()).copied();
        }
        self.handles.get(&h).copied()
    }
}

fn kernel<R>(f: impl FnOnce(&mut Kernel) -> R) -> R {
    runtime::with(|rt| rt.kernel.with(f))
}

/// Faults that ended guest threads so far (thread id, message).
pub fn faults() -> Vec<(u32, String)> {
    kernel(|k| k.faults.clone())
}

/// Close a kernel object handle; false if `h` is not one (CloseHandle
/// then tries the file handles).
pub(crate) fn close(h: u32) -> bool {
    let closed = kernel(|k| {
        let Some(object) = k.handles.remove(&h) else { return false; };
        if let Some(Object::Thread { tid, .. }) = k.objects.get(&object) {
            if !k.handles.values().any(|&other| other == object) {
                k.host_threads.remove(tid); // detach, never wait for the worker
            }
        }
        true
    });
    if closed {
        runtime::pin().lock.events.write().unwrap_or_else(|e| e.into_inner()).remove(&h);
        runtime::pin().lock.event_epoch.fetch_add(1, SeqCst);
        runtime::with(|rt| rt.lock.semaphores.write().unwrap_or_else(|e| e.into_inner()).remove(&h));
    }
    closed
}

/// The event behind handle `h`, read without the guest lock.
fn event_of(h: u32) -> Option<(u32, Arc<EventCell>)> {
    static CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let cached = *CACHE.get_or_init(|| std::env::var("DDRUNTIME_SYNC_CACHE").is_ok_and(|v| v == "1"));
    event_lookup(&runtime::pin().lock, h, cached)
}

type CachedEvent = (u32, u32, Arc<EventCell>);
struct EventCache {
    runtime: u64,
    epoch: u64,
    slots: [Option<CachedEvent>; 64],
}
thread_local! {
    static EVENT_CACHE: std::cell::RefCell<EventCache> = const { std::cell::RefCell::new(EventCache {
        runtime: 0, epoch: 0, slots: [const { None }; 64],
    }) };
}
/// Handles are monotonic and objects live for the runtime's lifetime. A close
/// invalidates every thread's bounded cache; a distinct runtime id prevents
/// allocator-address reuse from resurrecting handles in a later session.
/// Concurrent CloseHandle versus wait remains Win32's undefined operation.
fn event_lookup(lock: &GuestLock, h: u32, cached: bool) -> Option<(u32, Arc<EventCell>)> {
    if !cached {
        return lock.events.read().unwrap_or_else(|e| e.into_inner()).get(&h).cloned();
    }
    EVENT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let epoch = lock.event_epoch.load(SeqCst);
        if cache.runtime != lock.event_cache_id || cache.epoch != epoch {
            cache.slots.fill(None);
            cache.runtime = lock.event_cache_id;
            cache.epoch = epoch;
        }
        let slot = &mut cache.slots[((h >> 2) & 63) as usize];
        if let Some((handle, object, cell)) = slot {
            if *handle == h { return Some((*object, cell.clone())); }
        }
        let result = lock.events.read().unwrap_or_else(|e| e.into_inner()).get(&h).cloned();
        if let Some((object, cell)) = &result { *slot = Some((h, *object, cell.clone())); }
        result
    })
}

fn set_error(e: u32) {
    crate::runtime::set_last_error(e);
}

/// A named object: the existing one (ERROR_ALREADY_EXISTS) or a new one.
fn create_named(m: &dyn Mem, name: u32, make: impl FnOnce() -> Object) -> u32 {
    let name = (name != 0).then(|| crate::user32::guest_str_a(m, name));
    let (h, o) = kernel(|k| {
        if let Some(&o) = name.as_ref().and_then(|n| k.names.get(n)) {
            crate::runtime::set_last_error(ERROR_ALREADY_EXISTS);
            return (k.handle(o), o);
        }
        crate::runtime::set_last_error(0);
        let o = k.add(make());
        if let Some(n) = name {
            k.names.insert(n, o);
        }
        (k.handle(o), o)
    });
    if let Some(Object::Event(e)) = kernel(|k| k.objects.get(&o).map(|x| match x {
        Object::Event(e) => Object::Event(e.clone()),
        _ => Object::Thread { tid: 0, exit: None },
    })) {
        runtime::pin().lock.events.write().unwrap_or_else(|e| e.into_inner()).insert(h, (o, e));
    }
    if let Some(e) = kernel(|k| match &k.objects[&o] { Object::Semaphore(e) => Some(e.clone()), _ => None }) {
        runtime::with(|rt| rt.lock.semaphores.write().unwrap_or_else(|e| e.into_inner()).insert(h, (o, e)));
    }
    h
}

/// CreateEventA(security, manual reset, initial state, name).
pub fn create_event(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (manual, set, name) = (arg(m, cpu, 1) != 0, arg(m, cpu, 2) != 0, arg(m, cpu, 3));
    cpu.eax = create_named(m, name, || Object::Event(EventCell::new(manual, set)));
    ret(m, cpu, 16);
}

/// SetEvent / ResetEvent without the guest lock (L86: a lock-free
/// service); a handle that is not an event fails as before.
fn set_event_state(m: &mut dyn Mem, cpu: &mut Cpu, value: bool) {
    let h = arg(m, cpu, 0);
    cpu.eax = match event_of(h) {
        Some((o, e)) => {
            e.store(o, value);
            1
        }
        None => {
            let _lock = service();
            set_error(ERROR_INVALID_HANDLE);
            0
        }
    };
    ret(m, cpu, 4);
}
/// SetEvent(handle).
pub fn set_event(m: &mut dyn Mem, cpu: &mut Cpu) { set_event_state(m, cpu, true) }
/// ResetEvent(handle).
pub fn reset_event(m: &mut dyn Mem, cpu: &mut Cpu) { set_event_state(m, cpu, false) }

/// CreateSemaphoreA(security, initial, maximum, name).
pub fn create_semaphore(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (count, max, name) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = if (max as i32) <= 0 || count > max {
        set_error(87); // ERROR_INVALID_PARAMETER
        0
    } else {
        create_named(m, name, || Object::Semaphore(SemaphoreCell::new(count, max)))
    };
    ret(m, cpu, 16);
}

/// ReleaseSemaphore(handle, count, previous*).
pub fn release_semaphore(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, n, prev) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    // Host Win32 validates signed ReleaseCount before looking up the handle.
    let r = if (n as i32) <= 0 { Err(87) } else {
        semaphore_of(h).ok_or(ERROR_INVALID_HANDLE)
            .and_then(|(o, e)| e.release(n).map(|old| (o, old)))
    };
    cpu.eax = match r {
        Ok((o, old)) => {
            if prev != 0 {
                m.set_u32(prev, old);
            }
            signal_key(key::object(o));
            1
        }
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 12);
}

/// CreateMutexA(security, initial owner, name).
pub fn create_mutex(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (own, name) = (arg(m, cpu, 1) != 0, arg(m, cpu, 2));
    let me = current_tid();
    cpu.eax = create_named(m, name, || Object::Mutex { owner: if own { me } else { 0 }, recursion: u32::from(own), abandoned: false });
    ret(m, cpu, 12);
}

/// ReleaseMutex(handle).
pub fn release_mutex(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    let me = current_tid();
    let r = kernel(|k| {
        let o = k.object_of(h).ok_or(ERROR_INVALID_HANDLE)?;
        match k.objects.get_mut(&o) {
            Some(Object::Mutex { owner, recursion, .. }) if *owner == me => {
                *recursion -= 1;
                if *recursion == 0 {
                    *owner = 0;
                }
                Ok(o)
            }
            Some(Object::Mutex { .. }) => Err(ERROR_NOT_OWNER),
            _ => Err(ERROR_INVALID_HANDLE),
        }
    });
    cpu.eax = match r {
        Ok(o) => {
            signal_key(key::object(o));
            1
        }
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 4);
}

/// Whether an object is signalled for thread `me`.
fn signalled(o: &Object, me: u32) -> bool {
    match o {
        Object::Event(e) => e.is_set(),
        Object::Semaphore(e) => e.ready(),
        Object::Mutex { owner, .. } => *owner == 0 || *owner == me,
        Object::Thread { exit, .. } => exit.is_some(),
    }
}

/// The effect of a satisfied wait; false if an event was taken meanwhile.
/// Callers that consume semaphores hold their narrow count gate.
fn consume(o: &mut Object, me: u32) -> bool {
    match o {
        Object::Event(e) => return e.take(),
        Object::Semaphore(e) => return e.take_locked(),
        Object::Mutex { owner, recursion, abandoned } => {
            *owner = me;
            *recursion += 1;
            *abandoned = false;
        }
        _ => {}
    }
    true
}

fn abandoned(o: &Object) -> bool {
    matches!(o, Object::Mutex { abandoned: true, .. })
}

/// A terminating owner releases all its recursive mutex entries. The next
/// successful waiter receives ownership plus WAIT_ABANDONED exactly once.
/// Microsoft Learn: WaitForSingleObject / WaitForMultipleObjects.
fn abandon_mutexes(k: &mut Kernel, tid: u32) {
    for o in k.objects.values_mut() {
        if let Object::Mutex { owner, recursion, abandoned } = o {
            if *owner == tid {
                *owner = 0;
                *recursion = 0;
                *abandoned = true;
            }
        }
    }
}

/// Wait for `handles` (all of them, or any), up to `ms` milliseconds.
/// The caller holds the guest lock for mutexes and thread objects. Events
/// and semaphore counts may change without it: readiness is rechecked after
/// sleeper registration, and semaphore wait-all commits hold the count gate.
pub fn wait(handles: &[u32], all: bool, ms: u32) -> u32 {
    let me = current_tid();
    // The common single-object success/poll requires no temporary vectors or
    // host clock. Semaphore count mutations use their own narrow gate.
    if handles.len() == 1 {
        let ready = kernel(|k| {
            let object = k.object_of(handles[0])?;
            let value = k.objects.get_mut(&object)?;
            let result = if abandoned(value) { WAIT_ABANDONED_0 } else { WAIT_OBJECT_0 };
            let taken = match value {
                Object::Semaphore(e) => e.take(),
                _ => signalled(value, me) && consume(value, me),
            };
            Some((taken, result))
        });
        match ready {
            None => { set_error(ERROR_INVALID_HANDLE); return WAIT_FAILED; }
            Some((true, result)) => return result,
            Some((false, _)) if ms == 0 => { sync_profile::contention(); return WAIT_TIMEOUT; }
            _ => {}
        }
    }
    let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    let events: Vec<(u32, Arc<EventCell>)> = kernel(|k| {
        handles.iter().filter_map(|&h| k.object_of(h)).filter_map(|o| match k.objects.get(&o) {
            Some(Object::Event(e)) => Some((o, e.clone())),
            _ => None,
        }).collect()
    });
    let semaphores: Vec<(u32, Arc<SemaphoreCell>)> = kernel(|k| handles.iter()
        .filter_map(|&h| k.object_of(h)).filter_map(|o| match &k.objects[&o] {
            Object::Semaphore(e) => Some((o, e.clone())), _ => None,
        }).collect());
    let semaphore_gate = semaphores.first().map(|(_, e)| e.gate.clone());
    for (_, e) in &events {
        e.waiters.fetch_add(1, SeqCst);
    }
    let r = loop {
        // Err: the events taken by a wait for all that lost another (to
        // put back), and the objects whose events the check found set and
        // so does not wait for (the recheck condition is an event that was
        // not set becoming set; "any event set" stayed true for a wait for
        // all of a set event and an unsignalled semaphore, so the thread
        // rechecked forever holding the guest lock: the deadlock at the
        // game's logos)
        let r = kernel(|k| {
            let _gate = semaphore_gate.as_ref().map(|g| g.lock().unwrap_or_else(|e| e.into_inner()));
            let objs: Option<Vec<u32>> = handles.iter().map(|&h| k.object_of(h)).collect();
            let Some(objs) = objs else { return Ok(WAIT_FAILED) };
            let states: Vec<bool> = objs.iter().map(|o| signalled(&k.objects[o], me)).collect();
            let mut seen_set: Vec<u32> = Vec::new();
            if all {
                if states.iter().all(|&s| s) {
                    let result = if objs.iter().any(|o| abandoned(&k.objects[o])) { WAIT_ABANDONED_0 } else { WAIT_OBJECT_0 };
                    // the events first: one may be taken meanwhile by a
                    // wait without the guest lock; then nothing is taken
                    let mut taken = Vec::new();
                    for &o in &objs {
                        if let Object::Event(e) = &k.objects[&o] {
                            if !e.take() {
                                return Err((taken, seen_set));
                            }
                            if !e.manual {
                                taken.push((o, e.clone()));
                            }
                        }
                    }
                    for o in &objs {
                        if !matches!(k.objects[o], Object::Event(_)) {
                            consume(k.objects.get_mut(o).unwrap(), me);
                        }
                    }
                    return Ok(result);
                }
                // Already-ready events/semaphores do not force a busy recheck;
                // the missing independent objects, and locked objects, do.
                seen_set = objs.iter().zip(&states).filter(|&(o, &s)| s && matches!(k.objects[o], Object::Event(_) | Object::Semaphore(_))).map(|(&o, _)| o).collect();
            } else {
                for (i, o) in objs.iter().enumerate() {
                    let result = if abandoned(&k.objects[o]) { WAIT_ABANDONED_0 } else { WAIT_OBJECT_0 };
                    // an event taken meanwhile counts as not set
                    if states[i] && consume(k.objects.get_mut(o).unwrap(), me) {
                        return Ok(result + i as u32);
                    }
                }
            }
            Err((Vec::new(), seen_set))
        });
        match r {
            Ok(WAIT_FAILED) => {
                set_error(ERROR_INVALID_HANDLE);
                break WAIT_FAILED;
            }
            Ok(r) => break r,
            Err((taken, seen_set)) => {
                sync_profile::contention();
                // a wait for all that lost an event: the ones it took are set
                // again, and their waiters recheck
                for (o, e) in &taken {
                    e.store(*o, true);
                }
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    break WAIT_TIMEOUT;
                }
                if waits::help_due(handles) {
                    break waits::WAIT_HELP;
                }
                let keys: Vec<u64> = kernel(|k| handles.iter().filter_map(|&h| k.object_of(h)).map(key::object).collect());
                // Recheck independent events/semaphores after registration:
                // releases between the snapshot and parking cannot be lost.
                wait_until(&keys, deadline, || events.iter().any(|(o, e)| !seen_set.contains(o) && e.is_set())
                    || semaphores.iter().any(|(o, e)| !seen_set.contains(o) && e.ready())
                    || waits::help_due(handles));
            }
        }
    };
    for (_, e) in &events {
        e.waiters.fetch_sub(1, SeqCst);
    }
    r
}

/// A wait for one event without the guest lock (L86).
fn wait_event(handle: u32, object: u32, e: &EventCell, ms: u32) -> u32 {
    if e.take() {
        return WAIT_OBJECT_0;
    }
    sync_profile::contention();
    if ms == 0 {
        return WAIT_TIMEOUT;
    }
    let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    // a short spin first while other guest threads run, as for a critical
    // section: the game's threads hand events back and forth quickly, and
    // a sleep costs an OS wake-up
    if parallel() && !holds_lock() {
        let start = Instant::now();
        let mut n = 0u32;
        loop {
            if e.is_set() && e.take() {
                return WAIT_OBJECT_0;
            }
            if waits::help_due(&[handle]) {
                return waits::WAIT_HELP;
            }
            std::hint::spin_loop();
            n += 1;
            if n % 64 == 0 && (start.elapsed() >= CS_SPIN || deadline.is_some_and(|d| Instant::now() >= d)) {
                break;
            }
        }
    }
    #[cfg(windows)]
    if address_wait::enabled() && parallel() && !holds_lock() {
        return address_wait::event(handle, e, deadline);
    }
    e.waiters.fetch_add(1, SeqCst);
    let r = loop {
        if e.take() {
            break WAIT_OBJECT_0;
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break WAIT_TIMEOUT;
        }
        if waits::help_due(&[handle]) {
            break waits::WAIT_HELP;
        }
        wait_until(&[key::object(object)], deadline, || e.is_set() || waits::help_due(&[handle]));
    };
    e.waiters.fetch_sub(1, SeqCst);
    r
}

/// The sleeper protocol rechecks after registration; count releases notify
/// outside the narrow gate, so neither a lost wake nor a lock inversion occurs.
fn wait_semaphore(handle: u32, object: u32, e: &SemaphoreCell, ms: u32) -> u32 {
    if e.take() { return WAIT_OBJECT_0; }
    sync_profile::contention();
    if ms == 0 { return WAIT_TIMEOUT; }
    let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    loop {
        if e.take() { return WAIT_OBJECT_0; }
        if deadline.is_some_and(|d| Instant::now() >= d) { return WAIT_TIMEOUT; }
        if waits::help_due(&[handle]) { return waits::WAIT_HELP; }
        wait_until(&[key::object(object)], deadline, || e.ready() || waits::help_due(&[handle]));
    }
}

/// Windows address waits compare the value before parking and may wake
/// spuriously. Always recheck/take, using the original absolute deadline.
/// https://learn.microsoft.com/windows/win32/api/synchapi/nf-synchapi-waitonaddress
#[cfg(windows)]
mod address_wait {
    use super::*;
    #[link(name = "synchronization")]
    unsafe extern "system" {
        fn WaitOnAddress(address: *const std::ffi::c_void, compare: *const std::ffi::c_void, size: usize, ms: u32) -> i32;
        fn WakeByAddressAll(address: *const std::ffi::c_void);
    }
    pub(super) fn enabled() -> bool {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var("DDRUNTIME_SYNC_FUTEX").is_ok_and(|v| v == "1"))
    }
    pub(super) fn wake(value: &std::sync::atomic::AtomicBool) {
        // SAFETY: EventCell lives in an Arc for the entire wait/signal.
        unsafe { WakeByAddressAll(value.as_ptr().cast()) }
    }
    pub(super) fn event(handle: u32, e: &EventCell, deadline: Option<Instant>) -> u32 {
        e.address_waiters.fetch_add(1, SeqCst);
        let result = loop {
            if e.take() { break WAIT_OBJECT_0; }
            if waits::help_due(&[handle]) { break waits::WAIT_HELP; }
            let ms = match deadline {
                None => INFINITE,
                Some(d) => {
                    let now = Instant::now();
                    if now >= d { break WAIT_TIMEOUT; }
                    // Round up so sub-ms remainder does not turn into polling.
                    (d - now).as_nanos().div_ceil(1_000_000).min((INFINITE - 1) as u128) as u32
                }
            };
            sync_profile::blocking();
            let unset = false;
            // SAFETY: both bool values are one byte; e is alive and stable.
            // SeqCst waiter registration + state check pairs with store/wake.
            unsafe { WaitOnAddress(e.set.as_ptr().cast(), (&unset as *const bool).cast(), 1, ms); }
        };
        e.address_waiters.fetch_sub(1, SeqCst);
        result
    }
}

pub(crate) use crate::waits;

/// Milliseconds left until `deadline` (INFINITE without one; 0 once passed),
/// for a wait that resumes after running tasks.
fn remaining_ms(deadline: Option<Instant>) -> u32 {
    match deadline {
        None => INFINITE,
        Some(d) => d.saturating_duration_since(Instant::now()).as_nanos().div_ceil(1_000_000).min((INFINITE - 1) as u128) as u32,
    }
}

/// Wake the waiters of `handle` without changing it (a published task,
/// L172): events and semaphores recheck their conditions.
pub(crate) fn poke_handle(handle: u32) {
    if let Some((o, e)) = event_of(handle) {
        e.poke(o);
    } else if let Some((o, _)) = semaphore_of(handle) {
        signal_key(key::object(o));
    }
}

/// WaitForSingleObject(handle, ms).
/// Events and semaphores wait without the service lock; mutexes and thread
/// objects retain the service-locked path.
pub fn wait_for_single_object(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, ms) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let blocked = waits::blocked(&[h]);
    let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    let mut left = ms;
    cpu.eax = loop {
        let r = match event_of(h) {
            Some((o, e)) => wait_event(h, o, &e, left),
            None => match semaphore_of(h) {
                Some((o, e)) => wait_semaphore(h, o, &e, left),
                None => { let _lock = service(); wait(&[h], false, left) }
            }
        };
        if r != waits::WAIT_HELP {
            break r;
        }
        // an open window published tasks: run them here, then wait on
        without_lock(|| waits::run_pending(m, cpu));
        left = remaining_ms(deadline);
    };
    blocked.pass(m, cpu);
    ret(m, cpu, 8);
}

/// WaitForMultipleObjects(count, handles*, wait all, ms).
pub fn wait_for_multiple_objects(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (n, p, all, ms) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2) != 0, arg(m, cpu, 3));
    if n == 0 || n > 64 {
        set_error(87); // ERROR_INVALID_PARAMETER (MAXIMUM_WAIT_OBJECTS = 64)
        cpu.eax = WAIT_FAILED;
    } else {
        let mut handles = [0; 64];
        for k in 0..n { handles[k as usize] = m.u32(p + 4 * k); }
        let blocked = waits::blocked(&handles[..n as usize]);
        let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
        let mut left = ms;
        cpu.eax = loop {
            let r = wait(&handles[..n as usize], all, left);
            if r != waits::WAIT_HELP {
                break r;
            }
            // (this service holds the guest lock: tasks run without it)
            without_lock(|| waits::run_pending(m, cpu));
            left = remaining_ms(deadline);
        };
        blocked.pass(m, cpu);
    }
    ret(m, cpu, 16);
}

// --- critical sections -------------------------------------------------------------
//
// CRITICAL_SECTION (winnt.h RTL_CRITICAL_SECTION, 24 bytes on x86): +0
// DebugInfo, +4 LockCount, +8 RecursionCount, +12 OwningThread, +16
// LockSemaphore, +20 SpinCount. L86: Enter and Leave work on the guest's
// own words with host atomics, as Windows does, and take no guest lock;
// only contention blocks (a short spin, then a wait for the section's key).
// The fields hold what they held before (crate::services' convention since
// L21): OwningThread the owner's id, RecursionCount its entries, LockCount
// -1 plus its entries (waiters are not counted).

/// Threads blocked in EnterCriticalSection (any section): LeaveCriticalSection
/// signals only while there are some.
static CS_WAITERS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// How long a contended Enter spins before it waits (a section is held
/// ~1 us; measured gameplay mean 0.64 us). Unmeasured guess, not tuned.
const CS_SPIN: Duration = Duration::from_micros(20);

/// The section's LockCount, RecursionCount and OwningThread as atomics;
/// None if they are not aligned writable guest words.
fn cs_words(m: &dyn Mem, cs: u32) -> Option<(&std::sync::atomic::AtomicU32, &std::sync::atomic::AtomicU32, &std::sync::atomic::AtomicU32)> {
    Some((m.atomic_u32(cs.wrapping_add(4))?, m.atomic_u32(cs.wrapping_add(8))?, m.atomic_u32(cs.wrapping_add(12))?))
}

/// KERNEL32 `EnterCriticalSection(cs)` (stdcall): takes ownership or
/// recurses; while another guest thread owns it, spins briefly, then waits
/// (without the guest lock) until it is released.
pub fn enter_critical_section(m: &mut dyn Mem, cpu: &mut Cpu) {
    let cs = arg(m, cpu, 0);
    let me = current_tid();
    match cs_words(m, cs) {
        Some((lock_count, recursion, owner)) => {
            match owner.compare_exchange(0, me, SeqCst, SeqCst) {
                Ok(_) => {}
                Err(o) if o == me => {}
                Err(_) => contend(cs, owner, me),
            }
            lock_count.fetch_add(1, SeqCst);
            recursion.fetch_add(1, SeqCst);
        }
        None => {
            // not atomic words: as before L86, under the guest lock
            let _lock = service();
            loop {
                let owner = m.u32(cs.wrapping_add(12));
                if owner == 0 || owner == me {
                    break;
                }
                CS_WAITERS.fetch_add(1, SeqCst);
                block_on(&[key::critical_section(cs)], None);
                CS_WAITERS.fetch_sub(1, SeqCst);
            }
            m.set_u32(cs.wrapping_add(4), m.u32(cs.wrapping_add(4)).wrapping_add(1));
            m.set_u32(cs.wrapping_add(8), m.u32(cs.wrapping_add(8)).wrapping_add(1));
            m.set_u32(cs.wrapping_add(12), me);
        }
    }
    ret(m, cpu, 4);
}

/// Take a section another thread owns.
fn contend(cs: u32, owner: &std::sync::atomic::AtomicU32, me: u32) {
    sync_profile::contention();
    // spin while other guest threads run (parallel mode, no guest lock held)
    if parallel() && !holds_lock() {
        let start = Instant::now();
        let mut n = 0u32;
        loop {
            if owner.load(SeqCst) == 0 && owner.compare_exchange(0, me, SeqCst, SeqCst).is_ok() {
                return;
            }
            std::hint::spin_loop();
            n += 1;
            if n % 64 == 0 && start.elapsed() >= CS_SPIN {
                break;
            }
        }
    }
    // counted before the owner is checked again: a Leave that frees it
    // afterwards sees the count and signals (wait_until)
    CS_WAITERS.fetch_add(1, SeqCst);
    while owner.compare_exchange(0, me, SeqCst, SeqCst).is_err() {
        wait_until(&[key::critical_section(cs)], None, || owner.load(SeqCst) == 0);
    }
    CS_WAITERS.fetch_sub(1, SeqCst);
}

/// KERNEL32 `LeaveCriticalSection(cs)` (stdcall): undoes one Enter.
pub fn leave_critical_section(m: &mut dyn Mem, cpu: &mut Cpu) {
    let cs = arg(m, cpu, 0);
    let me = current_tid();
    match cs_words(m, cs) {
        Some((lock_count, recursion, owner)) => {
            let rec = recursion.load(SeqCst);
            assert!(rec > 0 && owner.load(SeqCst) == me, "LeaveCriticalSection({cs:#x}) without a matching Enter");
            recursion.store(rec - 1, SeqCst);
            lock_count.fetch_sub(1, SeqCst);
            if rec == 1 {
                owner.store(0, SeqCst);
                if CS_WAITERS.load(SeqCst) > 0 {
                    signal_key(key::critical_section(cs));
                }
            }
        }
        None => {
            let _lock = service();
            let rec = m.u32(cs.wrapping_add(8));
            assert!(rec > 0 && m.u32(cs.wrapping_add(12)) == me, "LeaveCriticalSection({cs:#x}) without a matching Enter");
            m.set_u32(cs.wrapping_add(8), rec - 1);
            m.set_u32(cs.wrapping_add(4), m.u32(cs.wrapping_add(4)).wrapping_sub(1));
            if rec == 1 {
                m.set_u32(cs.wrapping_add(12), 0);
                signal_key(key::critical_section(cs));
            }
        }
    }
    ret(m, cpu, 4);
}

/// Sleep(ms): Sleep(0) lets a ready thread run; INFINITE never returns.
pub fn sleep(m: &mut dyn Mem, cpu: &mut Cpu) {
    let ms = arg(m, cpu, 0);
    sleep_for(ms);
    ret(m, cpu, 4);
}
fn sleep_for(ms: u32) {
    match ms {
        0 => yield_now(),
        INFINITE => loop {
            block_on(&[], None);
        },
        _ => sleep_unlocked(Duration::from_millis(ms as u64)),
    }
}
/// Queue an asynchronous procedure call (an I/O completion routine) to
/// guest thread `tid`; it runs at that thread's next alertable wait.
pub(crate) fn queue_apc(tid: u32, routine: u32, args: [u32; 3]) {
    kernel(|k| k.apcs.entry(tid).or_default().push_back((routine, args)));
    signal_key(key::apc(tid));
}

/// Run this thread's queued APCs, oldest first; whether any ran.
fn run_apcs(m: &mut dyn Mem, cpu: &mut Cpu) -> bool {
    let me = current_tid();
    let mut ran = false;
    while let Some((routine, args)) = kernel(|k| k.apcs.get_mut(&me).and_then(|q| q.pop_front())) {
        crate::services::call_guest(m, cpu, routine, &args);
        ran = true;
    }
    ran
}

pub const WAIT_IO_COMPLETION: u32 = 0xC0;

/// SleepEx(ms, alertable): an alertable sleep first runs the thread's
/// queued completion routines and returns WAIT_IO_COMPLETION if there were
/// any; otherwise it sleeps like Sleep, returning early (also
/// WAIT_IO_COMPLETION) if a completion is queued to this thread meanwhile.
pub fn sleep_ex(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (ms, alertable) = (arg(m, cpu, 0), arg(m, cpu, 1) != 0);
    cpu.eax = if !alertable {
        sleep_for(ms);
        0
    } else {
        let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
        loop {
            if run_apcs(m, cpu) {
                break WAIT_IO_COMPLETION;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                break 0;
            }
            if ms == 0 {
                yield_now();
                break 0;
            }
            // nothing queued yet: wait for a state change (a completion
            // queued to this thread signals) or the deadline
            block_on(&[key::apc(current_tid())], deadline);
        }
    };
    ret(m, cpu, 8);
}

/// GetCurrentThreadId().
pub fn get_current_thread_id(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = current_tid();
    ret(m, cpu, 0);
}

// --- threads ----------------------------------------------------------------------

/// What a new host thread needs to run guest code.
pub(crate) struct Machine {
    pub(crate) memory: GuestMemory,
    pub(crate) registry: Arc<Registry>,
}

/// Give the current runtime the address space and dispatcher its new
/// threads run on (Session::new does this).
pub fn set_machine(memory: GuestMemory, registry: Arc<Registry>) {
    runtime::with(|rt| rt.machine.with(|mm| *mm = Some(Machine { memory, registry })));
}

fn thread_object(h: u32) -> Option<(u32, u32)> {
    kernel(|k| {
        let o = k.object_of(h)?;
        match k.objects.get(&o) {
            Some(Object::Thread { tid, .. }) => Some((o, *tid)),
            _ => None,
        }
    })
}

/// CreateThread(security, stack size, start, parameter, flags, thread id*).
pub fn create_thread(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (stack, start, param, flags, tid_out) = (arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4), arg(m, cpu, 5));
    let rt = runtime::current();
    let machine = rt.machine.with(|mm| mm.as_ref().map(|x| (x.memory.share(), x.registry.clone())));
    let Some((memory, registry)) = machine else { panic!("CreateThread outside a session (no guest machine)") };
    // the stack: the reservation (at least the exe's default), 64 KiB granular
    let reserve = if flags & STACK_SIZE_PARAM_IS_A_RESERVATION != 0 { stack } else { stack.max(DEFAULT_STACK) };
    let reserve = reserve.max(DEFAULT_STACK).next_multiple_of(0x1_0000);
    let (tid, h, stack_top) = kernel(|k| {
        if k.next_tid == 0 {
            k.next_tid = MAIN_THREAD_ID;
        }
        k.next_tid += 4;
        let tid = k.next_tid;
        if k.next_stack == 0 {
            k.next_stack = STACKS_BASE;
        }
        let base = k.next_stack;
        assert!(base + reserve <= STACKS_BASE + STACKS_SIZE, "guest thread stacks exhausted");
        k.next_stack += reserve;
        let o = k.add(Object::Thread { tid, exit: None });
        k.threads.insert(tid, o);
        (tid, k.handle(o), base + reserve)
    });
    if flags & CREATE_SUSPENDED != 0 {
        rt.lock.lock().suspended.insert(tid, 1);
    }
    let rt2 = rt.clone();
    let host = std::thread::Builder::new()
        .name(format!("guest thread {tid:#x}"))
        .stack_size(64 << 20)
        .spawn(move || run_thread(rt2, memory, registry, tid, start, param, stack_top, reserve))
        .expect("host thread for a guest thread");
    kernel(|k| { k.host_threads.insert(tid, host); });
    if tid_out != 0 {
        m.set_u32(tid_out, tid);
    }
    cpu.eax = h;
    ret(m, cpu, 24);
}

fn run_thread(rt: Arc<Runtime>, mut memory: GuestMemory, registry: Arc<Registry>, tid: u32, start: u32, param: u32, stack_top: u32, stack_size: u32) {
    runtime::enter(rt.clone());
    runtime::local(|l| l.guest_tid.set(tid));
    crate::load_log::timeline::guest_thread(start);
    registry.install();
    ddnative_base::platform::redirect(&crate::session::REDIRECTED_MODELS);
    let parallel = rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed);
    if !parallel {
        let s = rt.lock.lock();
        rt.lock.acquire_locked(s, 1);
    } else {
        // wait while created suspended (the serial mode waits in acquire)
        enter(&rt);
    }
    // the TEB is written under the lock in both modes
    let fs_base = crate::seh::initialize_teb(&mut memory, tid, stack_top - stack_size, stack_top);
    if parallel {
        leave(&rt);
    }
    let mut cpu = Cpu { esp: stack_top - 16, fs_base, ..Cpu::default() };
    cpu.esp -= 4;
    memory.set_u32(cpu.esp, param);
    cpu.esp -= 4;
    memory.set_u32(cpu.esp, THREAD_RETURN);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| registry.call(&mut memory, &mut cpu, start)));
    if parallel {
        // the exit (and a fault) are recorded under the lock
        enter(&rt);
    }
    let code = match r {
        Ok(Ok(())) => cpu.eax,
        Ok(Err(u)) => {
            // L21: a worker thread's stop is the run's next stop, but the
            // main thread runs on; say so at once (the run may never end)
            eprintln!("guest thread {tid} stopped: {u}");
            rt.kernel.with(|k| k.faults.push((tid, u.to_string())));
            FAULT_EXIT
        }
        Err(e) => {
            let msg = e.downcast_ref::<String>().cloned()
                .or_else(|| e.downcast_ref::<crate::seh::Stop>().map(ToString::to_string))
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .or_else(|| e.downcast_ref::<crate::dispatch::Unbound>().map(|u| u.to_string()))
                .unwrap_or_else(|| "panic".into());
            eprintln!("guest thread {tid} stopped: {msg}");
            rt.kernel.with(|k| k.faults.push((tid, msg)));
            FAULT_EXIT
        }
    };
    rt.kernel.with(|k| {
        abandon_mutexes(k, tid);
        if let Some(&o) = k.threads.get(&tid) {
            if let Some(Object::Thread { exit, .. }) = k.objects.get_mut(&o) {
                *exit = Some(code);
            }
        }
    });
    // its thread object, and whatever it owned (a critical section left
    // entered), may concern any waiter
    let mut s = rt.lock.lock();
    wake(&mut s, None);
    rt.lock.release_locked(&mut s);
}

/// ResumeThread(handle) -> the previous suspend count.
pub fn resume_thread(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    cpu.eax = match thread_object(h) {
        Some((_, tid)) => {
            let rt = runtime::pin();
            let mut s = rt.lock.lock();
            let n = s.suspended.get(&tid).copied().unwrap_or(0);
            if n > 0 {
                s.suspended.insert(tid, n - 1);
            }
            rt.lock.cv.notify_all();
            n
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            u32::MAX
        }
    };
    ret(m, cpu, 4);
}

/// SuspendThread(handle) -> the previous suspend count. The thread stops
/// at its next service boundary (it cannot be between two guest
/// instructions: only the lock holder runs). In parallel mode a thread
/// running translated code keeps running until its next service call,
/// where Windows would stop it at once (a modelling difference, L42).
pub fn suspend_thread(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    cpu.eax = match thread_object(h) {
        Some((_, tid)) => {
            assert_ne!(tid, current_tid(), "a guest thread suspending itself is not modelled");
            let rt = runtime::pin();
            let mut s = rt.lock.lock();
            let n = s.suspended.get(&tid).copied().unwrap_or(0);
            s.suspended.insert(tid, n + 1);
            n
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            u32::MAX
        }
    };
    ret(m, cpu, 4);
}

/// GetExitCodeThread(handle, code*): STILL_ACTIVE while it runs.
pub fn get_exit_code_thread(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let code = thread_object(h).and_then(|(o, _)| kernel(|k| match k.objects.get(&o) {
        Some(Object::Thread { exit, .. }) => Some(exit.unwrap_or(STILL_ACTIVE)),
        _ => None,
    }));
    cpu.eax = match code {
        Some(c) => {
            m.set_u32(out, c);
            1
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            0
        }
    };
    ret(m, cpu, 8);
}

/// Forward original priority requests when DDRUNTIME_THREAD_PRIORITY=1.
pub fn set_thread_priority(m: &mut dyn Mem, cpu: &mut Cpu) {
    scheduling::service(m, cpu, false, scheduling::priority_enabled());
}

/// Forward processor hints when DDRUNTIME_THREAD_IDEAL=1; this is not affinity.
pub fn set_thread_ideal_processor(m: &mut dyn Mem, cpu: &mut Cpu) {
    scheduling::service(m, cpu, true, scheduling::ideal_enabled());
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "CreateThread", create_thread),
    ("KERNEL32.dll", "ResumeThread", resume_thread),
    ("KERNEL32.dll", "SuspendThread", suspend_thread),
    ("KERNEL32.dll", "GetExitCodeThread", get_exit_code_thread),
    ("KERNEL32.dll", "SetThreadPriority", set_thread_priority),
    ("KERNEL32.dll", "SetThreadIdealProcessor", set_thread_ideal_processor),
    ("KERNEL32.dll", "GetCurrentThreadId", get_current_thread_id),
    ("KERNEL32.dll", "CreateEventA", create_event),
    ("KERNEL32.dll", "SetEvent", set_event),
    ("KERNEL32.dll", "ResetEvent", reset_event),
    ("KERNEL32.dll", "CreateSemaphoreA", create_semaphore),
    ("KERNEL32.dll", "ReleaseSemaphore", release_semaphore),
    ("KERNEL32.dll", "CreateMutexA", create_mutex),
    ("KERNEL32.dll", "ReleaseMutex", release_mutex),
    ("KERNEL32.dll", "WaitForSingleObject", wait_for_single_object),
    ("KERNEL32.dll", "WaitForMultipleObjects", wait_for_multiple_objects),
    ("KERNEL32.dll", "Sleep", sleep),
    ("KERNEL32.dll", "SleepEx", sleep_ex),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::{Binding, Registry};
    use ddnative_base::x86::LiftFn;
    use crate::memory::{GuestMemory, READ, WRITE};
    use d3d9cmd::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    include!("sync_followup.rs");
    include!("sync_bench.rs");
    include!("waits_help_tests.rs");
    include!("window_stress.rs");
    include!("window_stress_help.rs");

    #[test]
    fn event_cache_invalidates_closed_handles_and_runtime_identity() {
        let (rt, mut m) = sync_runtime();
        let h = create_named(&m, 0, || Object::Event(EventCell::new(false, true)));
        let (object, event) = event_lookup(&rt.lock, h, true).unwrap();
        assert!(event.take());
        assert!(!event_lookup(&rt.lock, h, true).unwrap().1.take());
        event.store(object, true);
        assert!(event_lookup(&rt.lock, h, false).unwrap().1.take());
        assert!(close(h));
        assert!(event_lookup(&rt.lock, h, true).is_none());
        let (next, _) = sync_runtime();
        let replacement = create_named(&m, 0, || Object::Event(EventCell::new(true, true)));
        assert_eq!(h, replacement);
        let fresh = event_lookup(&next.lock, h, true).unwrap().1;
        assert!(!Arc::ptr_eq(&event, &fresh));
        assert!(fresh.take() && fresh.take());
        m.set_u32(DATA, 0); // guest memory remains usable across cache changes
    }

    #[test]
    fn event_cache_aliases_and_collisions_preserve_object_identity() {
        let (rt, m) = sync_runtime();
        let h = create_named(&m, 0, || Object::Event(EventCell::new(false, true)));
        let (object, event) = event_lookup(&rt.lock, h, true).unwrap();
        let alias = kernel(|k| k.handle(object));
        rt.lock.events.write().unwrap().insert(alias, (object, event.clone()));
        assert!(event_lookup(&rt.lock, alias, true).unwrap().1.take());
        assert!(!event_lookup(&rt.lock, h, true).unwrap().1.take());
        for _ in 0..128 {
            let other = create_named(&m, 0, || Object::Event(EventCell::new(true, true)));
            assert!(event_lookup(&rt.lock, other, true).unwrap().1.manual);
        }
        assert!(Arc::ptr_eq(&event, &event_lookup(&rt.lock, h, true).unwrap().1));
        close(alias);
        assert!(event_lookup(&rt.lock, alias, true).is_none());
        assert!(event_lookup(&rt.lock, h, true).is_some());
    }

    #[test]
    fn single_wait_polls_and_abandoned_mutex_results() {
        let (_, m) = sync_runtime();
        let sem = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(1, 2)));
        assert_eq!(wait(&[sem], false, 0), WAIT_OBJECT_0);
        assert_eq!(wait(&[sem], false, 0), WAIT_TIMEOUT);
        assert_eq!(wait(&[0xdead], false, 0), WAIT_FAILED);
        let mutex = create_named(&m, 0, || Object::Mutex { owner: 0x4321, recursion: 3, abandoned: false });
        assert_eq!(wait(&[mutex], false, 0), WAIT_TIMEOUT);
        kernel(|k| abandon_mutexes(k, 0x4321));
        assert_eq!(wait(&[mutex], false, 0), WAIT_ABANDONED_0);
        assert_eq!(wait(&[mutex], false, 0), WAIT_OBJECT_0, "recursive entry is no longer abandoned");
        kernel(|k| abandon_mutexes(k, current_tid()));
        assert_eq!(wait(&[sem, mutex], false, 0), WAIT_ABANDONED_0 + 1);
        kernel(|k| abandon_mutexes(k, current_tid()));
        assert_eq!(wait(&[sem, mutex], true, 0), WAIT_TIMEOUT, "wait-all cannot consume the abandoned mutex early");
        let event = create_named(&m, 0, || Object::Event(EventCell::new(true, true)));
        assert_eq!(wait(&[event, mutex], true, 0), WAIT_ABANDONED_0);
        assert_eq!(wait(&[mutex], false, 0), WAIT_OBJECT_0);
    }

    #[test]
    fn multiple_wait_rejects_invalid_counts_without_reading_handles() {
        let (_, mut m) = sync_runtime();
        let mut cpu = Cpu::default();
        for n in [0, 65, u32::MAX] {
            cpu.esp = DATA + 0x800;
            for (i, v) in [0x1234, n, 0xdead0000, 1, 0].into_iter().enumerate() {
                m.set_u32(cpu.esp + 4 * i as u32, v);
            }
            wait_for_multiple_objects(&mut m, &mut cpu);
            assert_eq!(cpu.eax, WAIT_FAILED);
            assert_eq!(crate::runtime::last_error(), 87);
            assert_eq!(cpu.esp, DATA + 0x814);
        }
    }

    #[test]
    #[ignore = "synthetic before/after microbenchmark; run explicitly"]
    fn sync_event_lookup_benchmark() {
        let (rt, m) = sync_runtime();
        let h = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        for round in 0..6 {
            for cached in if round % 2 == 0 { [false, true] } else { [true, false] } {
                let start = Instant::now();
                for _ in 0..1_000_000 {
                    let (o, e) = event_lookup(&rt.lock, std::hint::black_box(h), cached).unwrap();
                    e.store(o, true);
                    assert!(e.take());
                    std::hint::black_box(e);
                }
                eprintln!("L153 lookup+set+take cached={cached} round={round} ns={:.3}", start.elapsed().as_nanos() as f64 / 1_000_000.0);
            }
        }
        for round in 0..4 {
            for cached in if round % 2 == 0 { [false, true] } else { [true, false] } {
                let handles: Vec<_> = (0..8).map(|_| create_named(&m, 0, || Object::Event(EventCell::new(false, false)))).collect();
                let barrier = Arc::new(std::sync::Barrier::new(9));
                let workers: Vec<_> = handles.into_iter().map(|h| {
                    let (rt, barrier) = (rt.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        // Warm the cache before the common start.
                        event_lookup(&rt.lock, h, cached).unwrap();
                        barrier.wait();
                        for _ in 0..100_000 {
                            let (o, e) = event_lookup(&rt.lock, h, cached).unwrap();
                            e.store(o, true);
                            assert!(e.take());
                            std::hint::black_box(e);
                        }
                    })
                }).collect();
                let start = Instant::now();
                barrier.wait();
                for w in workers { w.join().unwrap(); }
                eprintln!("L153 8-thread separate-events cached={cached} round={round} wall_ns_per_op={:.3}", start.elapsed().as_nanos() as f64 / 800_000.0);
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn address_event_timeout_manual_and_auto_reset() {
        let e = EventCell::new(false, false);
        assert_eq!(address_wait::event(0, &e, Some(Instant::now())), WAIT_TIMEOUT);
        e.store(1, true);
        assert_eq!(address_wait::event(0, &e, Some(Instant::now())), WAIT_OBJECT_0);
        assert!(!e.take());
        for manual in [false, true] {
            let e = EventCell::new(manual, false);
            let barrier = Arc::new(std::sync::Barrier::new(5));
            let workers: Vec<_> = (0..4).map(|_| {
                let (e, barrier) = (e.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    address_wait::event(0, &e, Some(Instant::now() + Duration::from_millis(250)))
                })
            }).collect();
            barrier.wait();
            let deadline = Instant::now() + Duration::from_secs(2);
            while e.address_waiters.load(SeqCst) < 4 && Instant::now() < deadline { std::thread::yield_now(); }
            e.store(1, true);
            let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
            assert_eq!(results.iter().filter(|&&r| r == WAIT_OBJECT_0).count(), if manual { 4 } else { 1 });
            assert_eq!(e.is_set(), manual);
            assert_eq!(e.address_waiters.load(SeqCst), 0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn address_event_ping_pong_loses_no_wakes() {
        let a = EventCell::new(false, false);
        let b = EventCell::new(false, false);
        let (aa, bb) = (a.clone(), b.clone());
        let worker = std::thread::spawn(move || {
            for _ in 0..5000 {
                assert_eq!(address_wait::event(0, &aa, Some(Instant::now() + Duration::from_secs(5))), WAIT_OBJECT_0);
                bb.store(2, true);
            }
        });
        for _ in 0..5000 {
            a.store(1, true);
            assert_eq!(address_wait::event(0, &b, Some(Instant::now() + Duration::from_secs(5))), WAIT_OBJECT_0);
        }
        worker.join().unwrap();
    }

    /// A backend whose Present takes `PRESENT` (a display waiting for its
    /// refresh); everything else does nothing.
    struct Slow;
    const PRESENT: Duration = Duration::from_millis(4);
    impl GraphicsBackend for Slow {
        fn upload(&mut self, _: Upload) {}
        fn draw(&mut self, _: &DrawCall) {}
        fn clear(&mut self, _: &ClearCall) {}
        fn present(&mut self, _: &Target) {
            std::thread::sleep(PRESENT);
        }
        fn copy(&mut self, _: &CopyCall) {}
        fn fill(&mut self, _: &FillCall) {}
        fn read_back(&mut self, _: &Target) -> Option<Vec<u8>> {
            None
        }
        fn query_begin(&mut self, _: u32) {}
        fn query_end(&mut self, _: u32) {}
        fn query_result(&mut self, _: u32) -> Option<u64> {
            None
        }
        fn gamma(&mut self, _: Option<&GammaRamp>) {}
        fn release(&mut self, _: u32) {}
    }

    fn target() -> Target {
        Target { surface: 1, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
    }
    /// Present as before L64: the backend's work under the guest lock.
    fn present_locked(m: &mut dyn Mem, c: &mut Cpu) {
        crate::d3d9draw::backend(|b| b.present(&target()));
        crate::services::ret(m, c, 0);
    }
    /// Present as d3d9draw::present_back_buffer does it now.
    fn present_unlocked(m: &mut dyn Mem, c: &mut Cpu) {
        crate::d3d9draw::backend_unlocked(|b| b.present(&target()));
        crate::services::ret(m, c, 0);
    }
    fn noop(m: &mut dyn Mem, c: &mut Cpu) {
        crate::services::ret(m, c, 0);
    }

    thread_local! {
        static TAIL_SYNC: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>> = const { std::cell::RefCell::new(None) };
    }

    fn tail_host_work() {
        assert!(!holds_lock());
        TAIL_SYNC.with(|sync| {
            let sync = sync.borrow();
            let (start, held) = sync.as_ref().unwrap();
            start.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(5)).unwrap();
        });
    }

    fn tail_before(m: &mut dyn Mem, c: &mut Cpu) {
        crate::d3d9draw::backend_unlocked(|_| tail_host_work());
        ret(m, c, 0);
    }

    fn tail_busy(m: &mut dyn Mem, c: &mut Cpu) {
        assert!(holds_lock());
        TAIL_SYNC.with(|sync| sync.borrow().as_ref().unwrap().0.send(()).unwrap());
        // Host-side contention fixture, not a game timing constant.
        std::thread::sleep(Duration::from_millis(4));
        ret(m, c, 0);
    }

    /// Each simulated render submission lets a second thread acquire the
    /// guest lock. Measure only the render service; drain the busy worker
    /// between frames so entry contention cannot hide the redundant tail.
    fn render_tail_scenario(tail: bool) -> (Duration, stats::Stat, String) {
        use std::sync::mpsc::channel;
        const FRAMES: usize = 32;
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt.clone());
        set_parallel(true);
        let mut r = Registry::new();
        r.insert(0x7000_0010, if tail { Binding::ServiceTail(tail_before) } else { Binding::Service(tail_before) }).unwrap();
        r.name(0x7000_0010, "L82::RenderTail");
        r.insert(0x7000_0020, Binding::Service(tail_busy)).unwrap();
        r.name(0x7000_0020, "L82::BusyWorker");
        let r = Arc::new(r);
        let (start_tx, start_rx) = channel();
        let (held_tx, held_rx) = channel();
        let (done_tx, done_rx) = channel();
        TAIL_SYNC.with(|s| *s.borrow_mut() = Some((start_tx, held_rx)));
        let total = without_lock(|| {
            let worker_rt = rt.clone();
            let worker_r = r.clone();
            let worker = std::thread::spawn(move || {
                runtime::enter(worker_rt);
                runtime::local(|l| l.guest_tid.set(0x3082));
                let (_, unused) = channel();
                TAIL_SYNC.with(|s| *s.borrow_mut() = Some((held_tx, unused)));
                let mut m = GuestMemory::new();
                m.map(0x10000, 0x10000, READ | WRITE).unwrap();
                let mut c = Cpu::default();
                for _ in 0..FRAMES {
                    start_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    call_service(&worker_r, &mut m, &mut c, 0x1F000, 0x7000_0020, &[]);
                    done_tx.send(()).unwrap();
                }
            });
            let mut m = GuestMemory::new();
            m.map(0x10000, 0x10000, READ | WRITE).unwrap();
            let mut c = Cpu::default();
            let mut total = Duration::ZERO;
            for _ in 0..FRAMES {
                let start = Instant::now();
                call_service(&r, &mut m, &mut c, 0x1F000, 0x7000_0010, &[]);
                total += start.elapsed();
                assert_eq!(c.esp, 0x1F000);
                assert!(!holds_lock());
                done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            worker.join().unwrap();
            total
        });
        TAIL_SYNC.with(|s| *s.borrow_mut() = None);
        let stat = stats::frame_service("L82::RenderTail");
        (total / FRAMES as u32, stat, lock_stats().unwrap())
    }

    #[test]
    fn render_tail_avoids_reacquiring_a_busy_guest_lock() {
        stats::force_on();
        let before = render_tail_scenario(false);
        stats::force_on();
        let after = render_tail_scenario(true);
        eprintln!("L82 32 render calls, busy worker 4 ms: before {:?}/call, {} acquisitions, {} blocks, {:?} wait; after {:?}/call, {} acquisitions, {} blocks, {:?} wait",
            before.0, before.1.acquisitions, before.1.blocks, before.1.wait,
            after.0, after.1.acquisitions, after.1.blocks, after.1.wait);
        eprintln!("before:\n{}after:\n{}", before.2, after.2);
        assert_eq!(before.1.calls, 32);
        assert_eq!(after.1.calls, 32);
        assert_eq!(before.1.acquisitions, 64);
        assert_eq!(after.1.acquisitions, 32);
        assert!(before.1.blocks >= 32);
        assert_eq!(after.1.blocks, 0);
        assert_eq!(before.1.contended_calls, 32);
        assert_eq!(after.1.contended_calls, 0);
    }

    fn tail_probe(m: &mut dyn Mem, c: &mut Cpu) {
        c.eax = unlocked(holds_lock) as u32;
        ret(m, c, 0);
    }

    #[test]
    fn render_tail_preserves_serial_direct_and_nested_lock_ownership() {
        for parallel in [false, true] {
            let rt = Arc::new(Runtime::default());
            runtime::enter(rt);
            set_parallel(parallel);
            release_service_tail(); // direct call outside a service
            assert!(holds_lock());
            let mut r = Registry::new();
            r.insert(0x7000_0010, Binding::ServiceTail(tail_probe)).unwrap();
            let mut m = GuestMemory::new();
            m.map(0x10000, 0x10000, READ | WRITE).unwrap();
            let mut c = Cpu::default();
            // Parallel entry nests inside the caller's already held lock.
            call_service(&r, &mut m, &mut c, 0x1F000, 0x7000_0010, &[]);
            assert_eq!(c.eax, 1);
            assert!(holds_lock());
        }
    }

    #[test]
    fn parallel_waiters_do_not_enable_serial_preemption_hooks() {
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt);
        set_parallel(true);
        let state = LockState { waiting: 3, ..Default::default() };
        signal_preempt(&state);
        assert!(!ddnative_host::PREEMPT.load(Ordering::Relaxed));
    }

    fn tail_panic(_: &mut dyn Mem, _: &mut Cpu) {
        unlocked(|| panic!("synthetic backend fault"));
    }

    fn tail_nested(m: &mut dyn Mem, c: &mut Cpu) {
        let _nested = ServiceScope::enter();
        release_service_tail();
        assert!(holds_lock(), "nested scopes cannot release their caller's lock");
        ret(m, c, 0);
    }

    #[test]
    fn render_tail_unwind_does_not_poison_the_next_service() {
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt);
        set_parallel(true);
        let mut r = Registry::new();
        r.insert(0x7000_0010, Binding::ServiceTail(tail_panic)).unwrap();
        r.insert(0x7000_0020, Binding::Service(tail_nested)).unwrap();
        let mut m = GuestMemory::new();
        m.map(0x10000, 0x10000, READ | WRITE).unwrap();
        let mut c = Cpu::default();
        without_lock(|| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call_service(&r, &mut m, &mut c, 0x1F000, 0x7000_0010, &[]);
            }));
            assert!(result.is_err());
            assert!(!holds_lock());
            call_service(&r, &mut m, &mut c, 0x1F000, 0x7000_0020, &[]);
            assert!(!holds_lock());
        });
        assert!(holds_lock());
    }

    /// One guest thread calls the service at `target` `calls` times, or
    /// (calls == 0) until `done`, pausing `pause` between calls; the
    /// duration of each call.
    fn guest_thread(rt: Arc<Runtime>, r: Arc<Registry>, tid: u32, target: u32, calls: u32,
                    done: Arc<AtomicBool>, pause: Duration) -> std::thread::JoinHandle<Vec<Duration>> {
        guest_thread_on(GuestMemory::new(), rt, r, tid, target, calls, done, pause)
    }

    /// As guest_thread, on (a handle to) `m`: each thread's stack at its
    /// own 64 KiB below 0x0100_0000 + tid * 0x10000, the argument at
    /// [esp+4] = 0x0090_0000 (an operand page all threads share).
    #[allow(clippy::too_many_arguments)]
    fn guest_thread_on(mut m: GuestMemory, rt: Arc<Runtime>, r: Arc<Registry>, tid: u32, target: u32, calls: u32,
                       done: Arc<AtomicBool>, pause: Duration) -> std::thread::JoinHandle<Vec<Duration>> {
        std::thread::spawn(move || {
            runtime::enter(rt);
            runtime::local(|l| l.guest_tid.set(tid));
            let stack = 0x0100_0000 + tid * 0x1_0000;
            let _ = m.map(stack, 0x1_0000, READ | WRITE);
            let top = stack + 0xF000;
            let mut c = Cpu { esp: top, ..Cpu::default() };
            let (f, locked) = match r.get(target) {
                Some(Binding::Service(f)) => (f, true),
                Some(Binding::LockFree(f)) => (f, false),
                _ => unreachable!(),
            };
            let mut took = Vec::new();
            while if calls == 0 { !done.load(Ordering::SeqCst) } else { took.len() < calls as usize } {
                c.esp = top;
                m.set_u32(c.esp, 0xFFFF_0000);
                m.set_u32(c.esp + 4, 0x0090_0000);
                let t = Instant::now();
                r.run_service(f, &mut m, &mut c, target, locked, false);
                took.push(t.elapsed());
                std::thread::sleep(pause);
            }
            done.store(true, Ordering::SeqCst);
            took
        })
    }

    /// Thread 1 presents `frames` times with `present`; thread 2 calls a
    /// cheap service meanwhile. Thread 2's mean and longest call (µs) and
    /// its number of calls.
    fn scenario(present: fn(&mut dyn Mem, &mut Cpu), frames: u32) -> (f64, f64, usize) {
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt.clone());
        set_parallel(true);
        crate::d3d9draw::set_backend(Box::new(Slow));
        let mut r = Registry::new();
        r.insert(0x7000_0010, Binding::Service(present)).unwrap();
        r.name(0x7000_0010, "TEST.dll!Present");
        r.insert(0x7000_0020, Binding::Service(noop)).unwrap();
        r.name(0x7000_0020, "TEST.dll!Noop");
        let r = Arc::new(r);
        let done = Arc::new(AtomicBool::new(false));
        // guest code runs without the lock in parallel mode
        without_lock(|| {
            let presenter = guest_thread(rt.clone(), r.clone(), 0x3000, 0x7000_0010, frames, done.clone(), Duration::ZERO);
            let other = guest_thread(rt.clone(), r.clone(), 0x3004, 0x7000_0020, 0, done.clone(), Duration::from_micros(200));
            presenter.join().unwrap();
            let took = other.join().unwrap();
            let us = |d: &Duration| d.as_secs_f64() * 1e6;
            let mean = took.iter().map(us).sum::<f64>() / took.len().max(1) as f64;
            let max = took.iter().map(us).fold(0.0, f64::max);
            (mean, max, took.len())
        })
    }

    /// L64 B: Present's backend work (the display wait) no longer holds the
    /// guest lock in parallel mode, so the other threads' services do not
    /// wait for it. The numbers are in the test output
    /// (`cargo test -p ddruntime --no-default-features --lib present_ -- --nocapture`).
    #[test]
    fn present_runs_without_the_guest_lock_in_parallel_mode() {
        stats::force_on();
        let before = scenario(present_locked, 50);
        let before_stats = lock_stats().unwrap();
        stats::force_on();
        let after = scenario(present_unlocked, 50);
        let after_stats = lock_stats().unwrap();
        eprintln!("another thread's service call while one presents (4 ms per Present, 50 Presents):");
        eprintln!("  before (Present under the lock): mean {:8.1} us, max {:8.1} us, {} calls", before.0, before.1, before.2);
        eprintln!("  after  (Present unlocked):       mean {:8.1} us, max {:8.1} us, {} calls", after.0, after.1, after.2);
        eprintln!("before, {before_stats}after, {after_stats}");
        assert!(after.0 * 4.0 < before.0, "the other thread no longer waits for Present: {before:?} -> {after:?}");
        // the call count depends on the scheduler (under a parallel test
        // load it varied by about 10% either way): only a large drop fails
        assert!(after.2 * 5 >= before.2 * 4, "and gets as many calls done: {before:?} -> {after:?}");
    }

    /// A service that holds the guest lock for 4 ms (a long locked call).
    fn hog(m: &mut dyn Mem, c: &mut Cpu) {
        std::thread::sleep(PRESENT);
        crate::services::ret(m, c, 0);
    }
    /// Whether the guest lock was held inside the service.
    static HELD_INSIDE: AtomicBool = AtomicBool::new(false);
    fn probe(m: &mut dyn Mem, c: &mut Cpu) {
        HELD_INSIDE.store(holds_lock(), Ordering::SeqCst);
        crate::services::ret(m, c, 0);
    }

    /// L65: a Binding::LockFree service runs without the guest lock in
    /// parallel mode; in the serial mode it runs under the caller's lock
    /// like any service (and its call stays a yield point).
    #[test]
    fn a_lock_free_service_takes_no_guest_lock_in_parallel_mode() {
        for parallel in [true, false] {
            let rt = Arc::new(Runtime::default());
            runtime::enter(rt.clone());
            set_parallel(parallel);
            let mut r = Registry::new();
            r.insert(0x7000_0030, Binding::LockFree(probe)).unwrap();
            r.insert(0x7000_0040, Binding::Service(probe)).unwrap();
            let mut m = GuestMemory::new();
            m.map(0x10000, 0x10000, READ | WRITE).unwrap();
            for (target, locked) in [(0x7000_0030, false), (0x7000_0040, true)] {
                let mut c = Cpu { esp: 0x1F000, ..Cpu::default() };
                m.set_u32(c.esp, 0xFFFF_0000);
                let call = |m: &mut GuestMemory, c: &mut Cpu| r.call(m, c, target).unwrap();
                if parallel {
                    without_lock(|| call(&mut m, &mut c));
                    assert_eq!(HELD_INSIDE.load(Ordering::SeqCst), locked, "parallel, locked binding {locked}");
                } else {
                    call(&mut m, &mut c);
                    assert!(HELD_INSIDE.load(Ordering::SeqCst), "serial: the caller holds the lock");
                }
            }
        }
    }

    /// One thread holds the guest lock 4 ms per call (50 calls) while
    /// another calls InterlockedIncrement bound as `binding`: the second
    /// thread's mean and longest call (µs), its calls, and whether the
    /// counter equals its calls.
    fn interlocked_scenario(binding: fn(LiftFn) -> Binding) -> (f64, f64, usize, bool) {
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt.clone());
        set_parallel(true);
        let mut r = Registry::new();
        r.insert(0x7000_0010, Binding::Service(hog)).unwrap();
        r.name(0x7000_0010, "TEST.dll!Hog");
        r.insert(0x7000_0020, binding(crate::win32::interlocked_increment)).unwrap();
        r.name(0x7000_0020, "KERNEL32.dll!InterlockedIncrement");
        let r = Arc::new(r);
        let mut m = GuestMemory::new();
        m.map(0x0090_0000, 0x1000, READ | WRITE).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        without_lock(|| {
            let hogs = guest_thread_on(m.share(), rt.clone(), r.clone(), 0x10, 0x7000_0010, 50, done.clone(), Duration::ZERO);
            let other = guest_thread_on(m.share(), rt.clone(), r.clone(), 0x14, 0x7000_0020, 0, done.clone(), Duration::from_micros(200));
            hogs.join().unwrap();
            let took = other.join().unwrap();
            let us = |d: &Duration| d.as_secs_f64() * 1e6;
            let mean = took.iter().map(us).sum::<f64>() / took.len().max(1) as f64;
            let max = took.iter().map(us).fold(0.0, f64::max);
            (mean, max, took.len(), m.u32(0x0090_0000) as usize == took.len())
        })
    }

    /// L65: InterlockedIncrement (and the other lock-free services) no
    /// longer wait for another thread's long locked service. Numbers with
    /// `-- --nocapture`.
    #[test]
    fn interlocked_does_not_wait_for_the_guest_lock() {
        stats::force_on();
        let before = interlocked_scenario(Binding::Service);
        let before_stats = lock_stats().unwrap();
        stats::force_on();
        let after = interlocked_scenario(Binding::LockFree);
        let after_stats = lock_stats().unwrap();
        eprintln!("InterlockedIncrement while another thread holds the lock 4 ms per call (50 calls):");
        eprintln!("  before (Service):  mean {:8.1} us, max {:8.1} us, {} calls", before.0, before.1, before.2);
        eprintln!("  after  (LockFree): mean {:8.1} us, max {:8.1} us, {} calls", after.0, after.1, after.2);
        eprintln!("before, {before_stats}after, {after_stats}");
        assert!(before.3 && after.3, "every increment counted: {before:?} {after:?}");
        assert!(after.0 * 4.0 < before.0, "{before:?} -> {after:?}");
        // the call count depends on the scheduler: only a large drop fails
        assert!(after.2 * 5 >= before.2 * 4, "{before:?} -> {after:?}");
    }

    /// WaitForSingleObject(handle, INFINITE) as an idle worker does.
    fn wait_service(m: &mut dyn Mem, c: &mut Cpu) {
        let h = m.u32(c.esp + 4);
        c.eax = wait(&[h], false, INFINITE);
        crate::services::ret(m, c, 4);
    }

    /// Call the service at `target` with `args` on this thread's stack.
    fn call_service(r: &Registry, m: &mut GuestMemory, c: &mut Cpu, top: u32, target: u32, args: &[u32]) {
        c.esp = top;
        for a in args.iter().rev() {
            c.esp -= 4;
            m.set_u32(c.esp, *a);
        }
        c.esp -= 4;
        m.set_u32(c.esp, 0xFFFF_0000);
        let (f, locked, tail) = match r.get(target) {
            Some(Binding::Service(f)) => (f, true, false),
            Some(Binding::ServiceTail(f)) => (f, true, true),
            Some(Binding::LockFree(f)) => (f, false, false),
            _ => unreachable!(),
        };
        r.run_service(f, m, c, target, locked, tail);
    }

    /// `idle` guest threads wait on their own events (as the game's worker
    /// pools do) while one thread enters and leaves a critical section and
    /// calls a cheap service `n` times: that thread's mean iteration (µs).
    fn idle_waiters_scenario(idle: u32, n: u32) -> f64 {
        const ENTER: u32 = 0x7000_0100;
        const LEAVE: u32 = 0x7000_0110;
        const NOOP: u32 = 0x7000_0120;
        const WAIT: u32 = 0x7000_0130;
        const SET: u32 = 0x7000_0140;
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt.clone());
        set_parallel(true);
        let mut r = Registry::new();
        for (a, f, name) in [
            (ENTER, enter_critical_section as LiftFn, "KERNEL32.dll!EnterCriticalSection"),
            (LEAVE, leave_critical_section, "KERNEL32.dll!LeaveCriticalSection"),
            (NOOP, noop, "TEST.dll!Noop"),
            (WAIT, wait_service, "KERNEL32.dll!WaitForSingleObject"),
            (SET, set_event, "KERNEL32.dll!SetEvent"),
        ] {
            r.insert(a, Binding::Service(f)).unwrap();
            r.name(a, name);
        }
        let r = Arc::new(r);
        let mut m = GuestMemory::new();
        m.map(0x0090_0000, 0x1000, READ | WRITE).unwrap(); // the critical section
        let events: Vec<u32> = (0..idle).map(|_| create_named(&m, 0, || Object::Event(EventCell::new(false, false)))).collect();
        without_lock(|| {
            let waiters: Vec<_> = events
                .iter()
                .enumerate()
                .map(|(k, &h)| {
                    let (rt, r, mut m) = (rt.clone(), r.clone(), m.share());
                    std::thread::spawn(move || {
                        runtime::enter(rt);
                        let tid = 0x4000 + 4 * k as u32;
                        runtime::local(|l| l.guest_tid.set(tid));
                        let stack = 0x0100_0000 + k as u32 * 0x1_0000;
                        m.map(stack, 0x1_0000, READ | WRITE).unwrap();
                        let mut c = Cpu::default();
                        call_service(&r, &mut m, &mut c, stack + 0xF000, WAIT, &[h]);
                    })
                })
                .collect();
            std::thread::sleep(Duration::from_millis(50)); // all waiting
            let (rt2, r2, mut m2) = (rt.clone(), r.clone(), m.share());
            let worker = std::thread::spawn(move || {
                runtime::enter(rt2);
                runtime::local(|l| l.guest_tid.set(0x3000));
                let stack = 0x0200_0000;
                m2.map(stack, 0x1_0000, READ | WRITE).unwrap();
                let mut c = Cpu::default();
                let t = Instant::now();
                for _ in 0..n {
                    call_service(&r2, &mut m2, &mut c, stack + 0xF000, ENTER, &[0x0090_0000]);
                    call_service(&r2, &mut m2, &mut c, stack + 0xF000, NOOP, &[]);
                    call_service(&r2, &mut m2, &mut c, stack + 0xF000, LEAVE, &[0x0090_0000]);
                }
                let mean = t.elapsed().as_secs_f64() * 1e6 / n as f64;
                for &h in &events {
                    call_service(&r2, &mut m2, &mut c, stack + 0xF000, SET, &[h]);
                }
                mean
            });
            let mean = worker.join().unwrap();
            for w in waiters {
                w.join().unwrap();
            }
            mean
        })
    }

    /// L69: idle waiters (WaitForSingleObject on their own objects) must not
    /// slow a busy thread's services: a LeaveCriticalSection or SetEvent
    /// wakes only the threads waiting for that object. Numbers with
    /// `-- --nocapture`.
    #[test]
    fn idle_waiters_do_not_slow_a_busy_thread() {
        stats::force_on();
        let alone = idle_waiters_scenario(0, 3000);
        let alone_stats = lock_stats().unwrap();
        stats::force_on();
        let with_idle = idle_waiters_scenario(16, 3000);
        let idle_stats = lock_stats().unwrap();
        eprintln!("EnterCriticalSection + a service + LeaveCriticalSection per iteration:");
        eprintln!("  no other threads:          {alone:8.2} us");
        eprintln!("  16 idle waiting threads:   {with_idle:8.2} us");
        eprintln!("alone, {alone_stats}with 16 idle, {idle_stats}");
        assert!(with_idle < alone * 3.0 + 5.0, "idle waiters slow the busy thread: {alone:.2} -> {with_idle:.2} us");
    }

    // --- L86: critical sections and events without the guest lock ------------------

    const S_ENTER: u32 = 0x7100_0000;
    const S_LEAVE: u32 = 0x7100_0010;
    const S_SET: u32 = 0x7100_0020;
    const S_RESET: u32 = 0x7100_0030;
    const S_WAIT1: u32 = 0x7100_0040;
    const S_WAITN: u32 = 0x7100_0050;
    const S_NOOP: u32 = 0x7100_0060;
    const S_RELEASE_SEM: u32 = 0x7100_0070;
    /// Guest data: critical sections at +0 and +0x40, counters at +0x100,
    /// handle arrays at +0x200.
    const DATA: u32 = 0x0090_0000;

    /// The synchronisation services, bound as the game's imports are
    /// (services::LOCK_FREE), or all under the guest lock (`lock_free`
    /// false: as before L86).
    fn sync_registry(lock_free: bool) -> Arc<Registry> {
        let mut r = Registry::new();
        for (a, f, name, free) in [
            (S_ENTER, enter_critical_section as LiftFn, "KERNEL32.dll!EnterCriticalSection", true),
            (S_LEAVE, leave_critical_section, "KERNEL32.dll!LeaveCriticalSection", true),
            (S_SET, set_event, "KERNEL32.dll!SetEvent", true),
            (S_RESET, reset_event, "KERNEL32.dll!ResetEvent", true),
            (S_WAIT1, wait_for_single_object, "KERNEL32.dll!WaitForSingleObject", true),
            (S_WAITN, wait_for_multiple_objects, "KERNEL32.dll!WaitForMultipleObjects", false),
            (S_NOOP, noop, "TEST.dll!Noop", false),
            (S_RELEASE_SEM, release_semaphore, "KERNEL32.dll!ReleaseSemaphore", true),
        ] {
            r.insert(a, if free && lock_free { Binding::LockFree(f) } else { Binding::Service(f) }).unwrap();
            r.name(a, name);
        }
        Arc::new(r)
    }

    /// A guest thread of a stress test: calls services on its own stack.
    struct GuestCaller {
        r: Arc<Registry>,
        m: GuestMemory,
        c: Cpu,
        top: u32,
    }

    impl GuestCaller {
        fn call(&mut self, target: u32, args: &[u32]) -> u32 {
            call_service(&self.r, &mut self.m, &mut self.c, self.top, target, args);
            self.c.eax
        }
    }

    /// A parallel-mode runtime with the guest data mapped.
    fn sync_runtime() -> (Arc<Runtime>, GuestMemory) {
        let rt = Arc::new(Runtime::default());
        runtime::enter(rt.clone());
        set_parallel(true);
        let mut m = GuestMemory::new();
        m.map(DATA, 0x1000, READ | WRITE).unwrap();
        (rt, m)
    }

    /// Run `n` guest threads (ids 0x5000 + 4k) each calling `body(k, caller)`;
    /// the wall time.
    fn run_guest_threads(rt: &Arc<Runtime>, r: &Arc<Registry>, m: &GuestMemory, n: usize,
                         body: Arc<dyn Fn(usize, &mut GuestCaller) + Send + Sync>) -> Duration {
        without_lock(|| {
            let t = Instant::now();
            let threads: Vec<_> = (0..n).map(|k| {
                let (rt, r, mut m, body) = (rt.clone(), r.clone(), m.share(), body.clone());
                std::thread::spawn(move || {
                    runtime::enter(rt);
                    runtime::local(|l| l.guest_tid.set(0x5000 + 4 * k as u32));
                    let stack = 0x0300_0000 + k as u32 * 0x1_0000;
                    // mapped by an earlier run in the same memory, or now
                    let _ = m.map(stack, 0x1_0000, READ | WRITE);
                    let mut caller = GuestCaller { r, m, c: Cpu::default(), top: stack + 0xF000 };
                    body(k, &mut caller);
                })
            }).collect();
            for th in threads {
                th.join().unwrap();
            }
            t.elapsed()
        })
    }

    fn critical_section_stress(lock_free: bool, n: usize, iters: u32) -> Duration {
        let (rt, mut m) = sync_runtime();
        for cs in [DATA, DATA + 0x40] {
            for (off, v) in [(0, u32::MAX), (4, u32::MAX), (8, 0), (12, 0), (16, 0), (20, 0)] {
                m.set_u32(cs + off, v);
            }
        }
        let r = sync_registry(lock_free);
        let t = run_guest_threads(&rt, &r, &m, n, Arc::new(move |k, g: &mut GuestCaller| {
            // half the threads also count under a second section inside the first
            for _ in 0..iters {
                g.call(S_ENTER, &[DATA]);
                g.call(S_ENTER, &[DATA]); // recursion
                let v = g.m.u32(DATA + 0x100);
                for _ in 0..8 {
                    std::hint::spin_loop();
                }
                g.m.set_u32(DATA + 0x100, v + 1);
                if k % 2 == 0 {
                    g.call(S_ENTER, &[DATA + 0x40]);
                    let w = g.m.u32(DATA + 0x104);
                    g.m.set_u32(DATA + 0x104, w + 1);
                    g.call(S_LEAVE, &[DATA + 0x40]);
                }
                g.call(S_LEAVE, &[DATA]);
                g.call(S_LEAVE, &[DATA]);
            }
        }));
        assert_eq!(m.u32(DATA + 0x100), n as u32 * iters, "mutual exclusion");
        assert_eq!(m.u32(DATA + 0x104), n.div_ceil(2) as u32 * iters, "mutual exclusion (nested section)");
        for cs in [DATA, DATA + 0x40] {
            assert_eq!([m.u32(cs + 4), m.u32(cs + 8), m.u32(cs + 12)], [u32::MAX, 0, 0], "fields of {cs:#x} after the last Leave");
        }
        t
    }

    /// L86: 8 threads enter (recursively) and leave two critical sections
    /// 4,000 times each around a non-atomic increment: no increment is
    /// lost, the fields end as InitializeCriticalSection left them. With
    /// the sections lock-free and, for comparison, under the guest lock.
    #[test]
    fn critical_sections_exclude_under_contention_without_the_guest_lock() {
        stats::force_on();
        let free = critical_section_stress(true, 8, 4000);
        let free_stats = lock_stats().unwrap();
        stats::force_on();
        let locked = critical_section_stress(false, 8, 4000);
        let locked_stats = lock_stats().unwrap();
        eprintln!("8 threads x 4,000 x (Enter, Enter, increment, [Enter, increment, Leave], Leave, Leave):");
        eprintln!("  lock-free sections:        {:8.1} ms", free.as_secs_f64() * 1e3);
        eprintln!("  under the guest lock:      {:8.1} ms", locked.as_secs_f64() * 1e3);
        eprintln!("lock-free, {free_stats}under the lock, {locked_stats}");
    }

    /// L86: pairs of threads hand two auto-reset events back and forth
    /// (SetEvent, WaitForSingleObject) 5,000 times each, 4 pairs at once,
    /// lock-free; every wait has a 10 s timeout, so a lost wakeup fails
    /// the test instead of hanging it.
    #[test]
    fn events_lose_no_wakeup_without_the_guest_lock() {
        let (rt, m) = sync_runtime();
        let pairs = 4;
        let rounds = 5000u32;
        let events: Vec<u32> = (0..2 * pairs).map(|_| create_named(&m, 0, || Object::Event(EventCell::new(false, false)))).collect();
        let r = sync_registry(true);
        let ev = Arc::new(events);
        let timeouts = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let (ev2, to2) = (ev.clone(), timeouts.clone());
        let t = run_guest_threads(&rt, &r, &m, 2 * pairs, Arc::new(move |k, g: &mut GuestCaller| {
            let (ping, pong) = (ev2[k / 2 * 2], ev2[k / 2 * 2 + 1]);
            for _ in 0..rounds {
                if k % 2 == 0 {
                    g.call(S_SET, &[ping]);
                    if g.call(S_WAIT1, &[pong, 10_000]) != WAIT_OBJECT_0 {
                        to2.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                } else {
                    if g.call(S_WAIT1, &[ping, 10_000]) != WAIT_OBJECT_0 {
                        to2.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    g.call(S_SET, &[pong]);
                }
            }
        }));
        eprintln!("{pairs} pairs x {rounds} event round trips: {:.1} ms ({:.2} us per round trip)",
                  t.as_secs_f64() * 1e3, t.as_secs_f64() * 1e6 / rounds as f64);
        assert_eq!(timeouts.load(Ordering::Relaxed), 0, "a wait timed out: a lost wakeup");
    }

    /// L86: one auto-reset event handed to 6 competing waiters 3,000 times:
    /// each SetEvent releases exactly one of them (the producer waits for
    /// an acknowledgement before setting it again).
    #[test]
    fn an_auto_reset_event_releases_one_waiter_per_set() {
        let (rt, m) = sync_runtime();
        let work = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let ack = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let r = sync_registry(true);
        let sets = 3000u32;
        let taken = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (taken2, done2, failed2) = (taken.clone(), done.clone(), failed.clone());
        run_guest_threads(&rt, &r, &m, 7, Arc::new(move |k, g: &mut GuestCaller| {
            if k == 0 {
                for _ in 0..sets {
                    g.call(S_SET, &[work]);
                    if g.call(S_WAIT1, &[ack, 10_000]) != WAIT_OBJECT_0 {
                        failed2.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                done2.store(true, Ordering::SeqCst);
            } else {
                while !done2.load(Ordering::SeqCst) {
                    if g.call(S_WAIT1, &[work, 20]) == WAIT_OBJECT_0 {
                        taken2.fetch_add(1, Ordering::SeqCst);
                        g.call(S_SET, &[ack]);
                    }
                }
            }
        }));
        assert!(!failed.load(Ordering::SeqCst), "the producer's acknowledgement timed out");
        assert_eq!(taken.load(Ordering::SeqCst), sets, "one waiter released per SetEvent");
    }

    /// L86: WaitForMultipleObjects (under the guest lock) is woken by
    /// SetEvent without it, and returns the event that was set, 3,000
    /// times; ResetEvent clears a manual-reset event for both kinds of
    /// wait.
    #[test]
    fn a_wait_for_several_objects_sees_lock_free_events() {
        let (rt, m) = sync_runtime();
        let a = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let b = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let ack = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let manual = create_named(&m, 0, || Object::Event(EventCell::new(true, true)));
        let mut mm = m.share();
        mm.set_u32(DATA + 0x200, a);
        mm.set_u32(DATA + 0x204, b);
        let r = sync_registry(true);
        let rounds = 3000u32;
        let wrong = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let wrong2 = wrong.clone();
        run_guest_threads(&rt, &r, &m, 2, Arc::new(move |k, g: &mut GuestCaller| {
            for i in 0..rounds {
                if k == 0 {
                    g.call(S_SET, &[if i % 2 == 0 { a } else { b }]);
                    if g.call(S_WAIT1, &[ack, 10_000]) != WAIT_OBJECT_0 {
                        wrong2.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                } else {
                    let got = g.call(S_WAITN, &[2, DATA + 0x200, 0, 10_000]);
                    if got != WAIT_OBJECT_0 + i % 2 {
                        wrong2.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    g.call(S_SET, &[ack]);
                }
            }
        }));
        assert_eq!(wrong.load(Ordering::Relaxed), 0, "a wait timed out or returned the wrong object");
        // the manual-reset event: set, then reset, for one and for several
        let r2 = r.clone();
        run_guest_threads(&rt, &r2, &m, 1, Arc::new(move |_, g: &mut GuestCaller| {
            assert_eq!(g.call(S_WAIT1, &[manual, 0]), WAIT_OBJECT_0);
            assert_eq!(g.call(S_WAIT1, &[manual, 0]), WAIT_OBJECT_0, "a manual-reset event stays set");
            g.call(S_RESET, &[manual]);
            assert_eq!(g.call(S_WAIT1, &[manual, 0]), WAIT_TIMEOUT);
            g.m.set_u32(DATA + 0x208, manual);
            assert_eq!(g.call(S_WAITN, &[1, DATA + 0x208, 0, 0]), WAIT_TIMEOUT);
            g.call(S_SET, &[manual]);
            assert_eq!(g.call(S_WAITN, &[1, DATA + 0x208, 1, 0]), WAIT_OBJECT_0);
        }));
    }

    /// L86 regression (the game deadlocked at the logos): a thread waits in
    /// WaitForMultipleObjects under the guest lock for an event or a
    /// semaphore, while signallers first need the guest lock (a locked
    /// service, ReleaseSemaphore) and others keep taking it; 2,000 rounds,
    /// every wait with a timeout, so a wait that keeps the lock fails the
    /// test instead of hanging it.
    #[test]
    fn a_locked_wait_lets_signallers_that_need_the_lock_run() {
        let (rt, m) = sync_runtime();
        let ev = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let sem = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(0, 1)));
        let ack = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let mut mm = m.share();
        mm.set_u32(DATA + 0x200, ev);
        mm.set_u32(DATA + 0x204, sem);
        let r = sync_registry(true);
        let rounds = 2000u32;
        let failures = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (f2, stop2) = (failures.clone(), stop.clone());
        run_guest_threads(&rt, &r, &m, 4, Arc::new(move |k, g: &mut GuestCaller| match k {
            // the waiter: under the guest lock
            0 => {
                for i in 0..rounds {
                    let got = g.call(S_WAITN, &[2, DATA + 0x200, 0, 10_000]);
                    if got != WAIT_OBJECT_0 + i % 2 {
                        f2.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                    g.call(S_SET, &[ack]);
                }
                stop2.store(true, Ordering::SeqCst);
            }
            // the signaller: takes the guest lock first
            1 => {
                for i in 0..rounds {
                    g.call(S_NOOP, &[]);
                    if i % 2 == 0 {
                        g.call(S_SET, &[ev]);
                    } else {
                        g.call(S_RELEASE_SEM, &[sem, 1, 0]);
                    }
                    if g.call(S_WAIT1, &[ack, 10_000]) != WAIT_OBJECT_0 {
                        f2.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                }
            }
            // lock traffic
            _ => {
                while !stop2.load(Ordering::SeqCst) {
                    g.call(S_NOOP, &[]);
                }
            }
        }));
        assert_eq!(failures.load(Ordering::SeqCst), 0, "a wait timed out or returned the wrong object");
    }

    /// L86 regression: a wait for ALL of an event that is set and a
    /// semaphore that is not, under the guest lock, while the thread that
    /// releases the semaphore needs the guest lock first (ReleaseSemaphore
    /// is a locked service). The wait must give the lock up until the
    /// semaphore is released: it spun holding it (its recheck condition,
    /// "an event is set", stayed true), and the game deadlocked at the
    /// logos. 500 rounds; the waiter's timeout fails the test instead of
    /// hanging it.
    #[test]
    fn a_wait_for_all_gives_the_lock_up_while_an_event_is_already_set() {
        let (rt, m) = sync_runtime();
        let ev = create_named(&m, 0, || Object::Event(EventCell::new(true, true)));
        let sem = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(0, 1)));
        let ack = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
        let mut mm = m.share();
        mm.set_u32(DATA + 0x200, ev);
        mm.set_u32(DATA + 0x204, sem);
        let r = sync_registry(true);
        let rounds = 500u32;
        let failures = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let f2 = failures.clone();
        run_guest_threads(&rt, &r, &m, 2, Arc::new(move |k, g: &mut GuestCaller| {
            for _ in 0..rounds {
                if k == 0 {
                    if g.call(S_WAITN, &[2, DATA + 0x200, 1, 5_000]) != WAIT_OBJECT_0 {
                        f2.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    g.call(S_SET, &[ack]);
                } else {
                    // let the waiter block first, sometimes
                    std::thread::yield_now();
                    g.call(S_RELEASE_SEM, &[sem, 1, 0]);
                    if g.call(S_WAIT1, &[ack, 5_000]) != WAIT_OBJECT_0 {
                        f2.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                }
            }
        }));
        assert_eq!(failures.load(Ordering::SeqCst), 0, "a wait timed out: the wait for all kept the guest lock");
    }
}
