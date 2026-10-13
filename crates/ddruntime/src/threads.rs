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
//! Per-thread runtime state (the last error, TLS values) is swapped when a
//! thread takes the lock after another thread had it (the state stays with
//! its owner while nobody else runs), so the services that use it are
//! unchanged.
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
use crate::services::{arg, ret, with_state, MAIN_THREAD_ID};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const WAIT_OBJECT_0: u32 = 0;
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
    /// Nested service entries of the holder (parallel mode; 1 otherwise).
    depth: u32,
    /// The thread whose per-thread state (last error, TLS) is loaded.
    state_owner: u32,
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
                depth: 1,
                state_owner: current_tid(),
                ..LockState::default()
            }),
            cv: Condvar::new(),
            held_hint: std::sync::atomic::AtomicBool::new(true),
            parallel: std::sync::atomic::AtomicBool::new(parallel),
            events: Default::default(),
        }
    }
}

/// Set once any runtime of the process runs in parallel mode: until then a
/// service call skips the runtime lookup (the serial mode's cost stays as
/// it was).
static ANY_PARALLEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this runtime runs guest threads in parallel.
pub fn parallel() -> bool {
    runtime::current().lock.parallel.load(std::sync::atomic::Ordering::Relaxed)
}

/// Choose the mode for this runtime (before it starts guest threads).
pub fn set_parallel(on: bool) {
    if on {
        ANY_PARALLEL.store(true, std::sync::atomic::Ordering::Relaxed);
        ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    runtime::current().lock.parallel.store(on, std::sync::atomic::Ordering::Relaxed);
}

thread_local! {
    static CURRENT_TID: Cell<u32> = const { Cell::new(MAIN_THREAD_ID) };
    static SLICE_START: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// The running guest thread's id.
pub fn current_tid() -> u32 {
    CURRENT_TID.with(Cell::get)
}

impl GuestLock {
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
    let rt = runtime::current();
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
    struct Again(Arc<Runtime>);
    impl Drop for Again {
        fn drop(&mut self) {
            let s = self.0.lock.lock();
            self.0.lock.acquire_locked(s, 1);
            adopt_context(&self.0);
        }
    }
    let _again = Again(rt);
    f()
}

/// Run `f` holding the guest lock: from inside [`unlocked`] (parallel
/// mode) it takes the lock for `f` only; where the thread holds it already
/// (or in the serial mode) `f` just runs. No time slice ends here.
pub(crate) fn with_lock<R>(f: impl FnOnce() -> R) -> R {
    let rt = runtime::current();
    if !rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed) {
        return f();
    }
    enter(&rt);
    struct Leave(Arc<Runtime>);
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
    use std::cell::Cell;
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

    thread_local! {
        static HELD_SINCE: Cell<Option<Instant>> = const { Cell::new(None) };
        static HELD: Cell<Duration> = const { Cell::new(Duration::ZERO) };
        static WAITED: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    }

    /// This thread took the lock after asking at `asked`.
    pub(super) fn acquired(asked: Instant) {
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
    pub(crate) fn record(name: String, before: (Duration, Duration)) {
        let (held, waited) = totals();
        let (hold, wait) = (held - before.0, waited - before.1);
        let mut t = table().lock().unwrap_or_else(|e| e.into_inner());
        let s = t.entry(name).or_default();
        s.calls += 1;
        s.hold += hold;
        s.max_hold = s.max_hold.max(hold);
        s.wait += wait;
        s.max_wait = s.max_wait.max(wait);
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
    }
    Some(out)
}

/// Whether this host thread holds the guest lock (tests, diagnostics).
pub fn holds_guest_lock() -> bool {
    holds_lock()
}

/// Whether this host thread holds the guest lock.
pub(crate) fn holds_lock() -> bool {
    let rt = runtime::current();
    let s = rt.lock.lock();
    s.held && s.holder == current_tid()
}

/// Whether this thread is running a service (crate::dispatch).
#[cfg(test)]
pub(crate) fn in_service() -> bool {
    IN_SERVICE.with(Cell::get) > 0
}

/// Holds the guest lock for one service call (parallel mode); in the
/// serial mode the caller already holds it, and only the time slice is
/// checked.
pub(crate) struct ServiceLock(Option<Arc<Runtime>>);

pub(crate) fn service() -> ServiceLock {
    if !ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed) {
        maybe_yield();
        return ServiceLock(None);
    }
    let rt = runtime::current();
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
        || !runtime::current().lock.parallel.load(std::sync::atomic::Ordering::Relaxed)
    {
        maybe_yield();
    }
    ServiceLock(None)
}

impl Drop for ServiceLock {
    fn drop(&mut self) {
        if let Some(rt) = self.0.take() {
            leave(&rt);
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
    adopt_context(rt);
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
    let rt = runtime::current();
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
    struct Reacquire(Arc<Runtime>, u32);
    impl Drop for Reacquire {
        fn drop(&mut self) {
            let s = self.0.lock.lock();
            self.0.lock.acquire_locked(s, self.1);
            adopt_context(&self.0);
        }
    }
    let _again = Reacquire(rt.clone(), depth);
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
    let rt = runtime::current();
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
    let keys: Box<[u64]> = keys.into();
    let rt = runtime::current();
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
        adopt_context(&rt);
    }
}

/// Let another ready thread run first (Sleep(0), SwitchToThread, the end
/// of a time slice).
pub(crate) fn yield_now() {
    let rt = runtime::current();
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
    adopt_context(&rt);
}

/// Lifted calls become preemption points while a runnable guest thread
/// waits for the lock (suspended threads wait too, but cannot take it).
fn signal_preempt(s: &LockState) {
    let runnable = s.waiting as usize > s.suspended.values().filter(|n| **n > 0).count();
    ddnative_host::PREEMPT.store(runnable, std::sync::atomic::Ordering::Relaxed);
}

thread_local! {
    /// Services running on this thread (dispatch.rs): lifted code a service
    /// calls back into (an IStream read inside the movie service, a WndProc)
    /// runs while the service may hold runtime state borrowed, so it is no
    /// preemption point (L57c: a yield there borrowed that state again and
    /// panicked, and the dying thread left the guest lock held).
    static IN_SERVICE: Cell<u32> = const { Cell::new(0) };
}

/// Marks this thread as running a service until dropped.
pub(crate) struct ServiceScope;

impl ServiceScope {
    pub(crate) fn enter() -> Self {
        IN_SERVICE.with(|n| n.set(n.get() + 1));
        ServiceScope
    }
}

impl Drop for ServiceScope {
    fn drop(&mut self) {
        IN_SERVICE.with(|n| n.set(n.get() - 1));
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
    if !due || ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed) || IN_SERVICE.with(Cell::get) > 0 {
        return;
    }
    let rt = runtime::current();
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
    let rt = runtime::current();
    let depth = {
        let mut s = rt.lock.lock();
        let d = s.depth;
        rt.lock.release_locked(&mut s);
        d
    };
    std::thread::sleep(duration);
    let s = rt.lock.lock();
    rt.lock.acquire_locked(s, depth);
    adopt_context(&rt);
}

// --- per-thread context -------------------------------------------------------

#[derive(Default, Clone)]
struct Context {
    last_error: u32,
    /// (allocation epoch, value) per TLS index
    tls: Vec<(u32, Option<u32>)>,
}

/// Load this thread's per-thread state if another thread's is loaded
/// (the caller has just taken the lock).
fn adopt_context(rt: &Runtime) {
    let me = current_tid();
    let previous = std::mem::replace(&mut rt.lock.lock().state_owner, me);
    if previous != me {
        save_context(rt, previous);
        load_context(rt);
    }
}

fn save_context(rt: &Runtime, tid: u32) {
    let ctx = rt.state.with(|s| Context {
        last_error: s.last_error,
        tls: s.tls.iter().enumerate().map(|(i, v)| (s.tls_epoch.get(i).copied().unwrap_or(0), *v)).collect(),
    });
    rt.kernel.with(|k| k.contexts.insert(tid, ctx));
}

fn load_context(rt: &Runtime) {
    let tid = current_tid();
    let ctx = rt.kernel.with(|k| k.contexts.get(&tid).cloned()).unwrap_or_default();
    rt.state.with(|s| {
        s.last_error = ctx.last_error;
        for i in 0..s.tls.len() {
            if s.tls[i].is_some() {
                // an allocated index: this thread's value if it set one since
                // the index was (re)allocated, else 0
                let epoch = s.tls_epoch.get(i).copied().unwrap_or(0);
                s.tls[i] = Some(match ctx.tls.get(i) {
                    Some(&(e, Some(v))) if e == epoch => v,
                    _ => 0,
                });
            }
        }
    });
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
}

impl EventCell {
    pub fn new(manual: bool, set: bool) -> Arc<EventCell> {
        Arc::new(EventCell { manual, set: set.into(), waiters: 0.into() })
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

    /// Set or reset; the threads waiting for it recheck (`object`: its id).
    fn store(&self, object: u32, value: bool) {
        self.set.store(value, SeqCst);
        if value && self.waiters.load(SeqCst) > 0 {
            signal_key(key::object(object));
        }
    }
}

#[derive(Debug)]
pub enum Object {
    Event(Arc<EventCell>),
    Semaphore { count: u32, max: u32 },
    Mutex { owner: u32, recursion: u32 },
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
    contexts: HashMap<u32, Context>,
    /// thread id -> its object
    threads: HashMap<u32, u32>,
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
    let closed = kernel(|k| k.handles.remove(&h).is_some());
    if closed {
        runtime::current().lock.events.write().unwrap_or_else(|e| e.into_inner()).remove(&h);
    }
    closed
}

/// The event behind handle `h`, read without the guest lock.
fn event_of(h: u32) -> Option<(u32, Arc<EventCell>)> {
    runtime::current().lock.events.read().unwrap_or_else(|e| e.into_inner()).get(&h).cloned()
}

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
}

/// A named object: the existing one (ERROR_ALREADY_EXISTS) or a new one.
fn create_named(m: &dyn Mem, name: u32, make: impl FnOnce() -> Object) -> u32 {
    let name = (name != 0).then(|| crate::user32::guest_str_a(m, name));
    let (h, o) = kernel(|k| {
        if let Some(&o) = name.as_ref().and_then(|n| k.names.get(n)) {
            with_state(|s| s.last_error = ERROR_ALREADY_EXISTS);
            return (k.handle(o), o);
        }
        with_state(|s| s.last_error = 0);
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
        runtime::current().lock.events.write().unwrap_or_else(|e| e.into_inner()).insert(h, (o, e));
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
        create_named(m, name, || Object::Semaphore { count, max })
    };
    ret(m, cpu, 16);
}

/// ReleaseSemaphore(handle, count, previous*).
pub fn release_semaphore(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, n, prev) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let r = kernel(|k| {
        let o = k.object_of(h).ok_or(ERROR_INVALID_HANDLE)?;
        match k.objects.get_mut(&o) {
            Some(Object::Semaphore { count, max }) => match count.checked_add(n).filter(|c| c <= max && n > 0) {
                Some(c) => Ok((o, std::mem::replace(count, c))),
                None => Err(ERROR_TOO_MANY_POSTS),
            },
            _ => Err(ERROR_INVALID_HANDLE),
        }
    });
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
    cpu.eax = create_named(m, name, || if own { Object::Mutex { owner: me, recursion: 1 } } else { Object::Mutex { owner: 0, recursion: 0 } });
    ret(m, cpu, 12);
}

/// ReleaseMutex(handle).
pub fn release_mutex(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    let me = current_tid();
    let r = kernel(|k| {
        let o = k.object_of(h).ok_or(ERROR_INVALID_HANDLE)?;
        match k.objects.get_mut(&o) {
            Some(Object::Mutex { owner, recursion }) if *owner == me => {
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
        Object::Semaphore { count, .. } => *count > 0,
        Object::Mutex { owner, .. } => *owner == 0 || *owner == me,
        Object::Thread { exit, .. } => exit.is_some(),
    }
}

/// The effect of a satisfied wait; false if an event was taken meanwhile
/// by a wait that holds no guest lock (the others change under it only).
fn consume(o: &mut Object, me: u32) -> bool {
    match o {
        Object::Event(e) => return e.take(),
        Object::Semaphore { count, .. } => *count -= 1,
        Object::Mutex { owner, recursion } => {
            *owner = me;
            *recursion += 1;
        }
        _ => {}
    }
    true
}

/// Wait for `handles` (all of them, or any), up to `ms` milliseconds.
/// The caller holds the guest lock (semaphores, mutexes and threads change
/// only under it); events change without it (L86), so this thread counts
/// itself a waiter of each event before it checks them, and blocks unless
/// one is set (`wait_until`).
pub fn wait(handles: &[u32], all: bool, ms: u32) -> u32 {
    let me = current_tid();
    let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    let events: Vec<(u32, Arc<EventCell>)> = kernel(|k| {
        handles.iter().filter_map(|&h| k.object_of(h)).filter_map(|o| match k.objects.get(&o) {
            Some(Object::Event(e)) => Some((o, e.clone())),
            _ => None,
        }).collect()
    });
    for (_, e) in &events {
        e.waiters.fetch_add(1, SeqCst);
    }
    let r = loop {
        let r = kernel(|k| {
            let objs: Option<Vec<u32>> = handles.iter().map(|&h| k.object_of(h)).collect();
            let Some(objs) = objs else { return Ok(WAIT_FAILED) };
            if all {
                if objs.iter().all(|o| signalled(&k.objects[o], me)) {
                    // the events first: one may be taken meanwhile by a
                    // wait without the guest lock; then nothing is taken
                    let mut taken = Vec::new();
                    for &o in &objs {
                        if let Object::Event(e) = &k.objects[&o] {
                            if !e.take() {
                                return Err(taken);
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
                    return Ok(WAIT_OBJECT_0);
                }
            } else {
                for (i, o) in objs.iter().enumerate() {
                    if signalled(&k.objects[o], me) && consume(k.objects.get_mut(o).unwrap(), me) {
                        return Ok(WAIT_OBJECT_0 + i as u32);
                    }
                }
            }
            Err(Vec::new())
        });
        match r {
            Ok(WAIT_FAILED) => {
                set_error(ERROR_INVALID_HANDLE);
                break WAIT_FAILED;
            }
            Ok(r) => break r,
            Err(taken) => {
                // a wait for all that lost an event: the ones it took are set
                // again, and their waiters recheck
                for (o, e) in &taken {
                    e.store(*o, true);
                }
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    break WAIT_TIMEOUT;
                }
                let keys: Vec<u64> = kernel(|k| handles.iter().filter_map(|&h| k.object_of(h)).map(key::object).collect());
                wait_until(&keys, deadline, || events.iter().any(|(_, e)| e.is_set()));
            }
        }
    };
    for (_, e) in &events {
        e.waiters.fetch_sub(1, SeqCst);
    }
    r
}

/// A wait for one event without the guest lock (L86).
fn wait_event(object: u32, e: &EventCell, ms: u32) -> u32 {
    if e.take() {
        return WAIT_OBJECT_0;
    }
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
            std::hint::spin_loop();
            n += 1;
            if n % 64 == 0 && (start.elapsed() >= CS_SPIN || deadline.is_some_and(|d| Instant::now() >= d)) {
                break;
            }
        }
    }
    e.waiters.fetch_add(1, SeqCst);
    let r = loop {
        if e.take() {
            break WAIT_OBJECT_0;
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break WAIT_TIMEOUT;
        }
        wait_until(&[key::object(object)], deadline, || e.is_set());
    };
    e.waiters.fetch_sub(1, SeqCst);
    r
}

/// WaitForSingleObject(handle, ms).
/// A lock-free service (L86): an event is waited for without the guest
/// lock; any other object as before, under it.
pub fn wait_for_single_object(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, ms) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match event_of(h) {
        Some((o, e)) => wait_event(o, &e, ms),
        None => {
            let _lock = service();
            wait(&[h], false, ms)
        }
    };
    ret(m, cpu, 8);
}

/// WaitForMultipleObjects(count, handles*, wait all, ms).
pub fn wait_for_multiple_objects(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (n, p, all, ms) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2) != 0, arg(m, cpu, 3));
    let handles: Vec<u32> = (0..n.min(64)).map(|k| m.u32(p + 4 * k)).collect();
    cpu.eax = wait(&handles, all, ms);
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
    std::thread::Builder::new()
        .name(format!("guest thread {tid:#x}"))
        .stack_size(64 << 20)
        .spawn(move || run_thread(rt2, memory, registry, tid, start, param, stack_top, reserve))
        .expect("host thread for a guest thread");
    if tid_out != 0 {
        m.set_u32(tid_out, tid);
    }
    cpu.eax = h;
    ret(m, cpu, 24);
}

fn run_thread(rt: Arc<Runtime>, mut memory: GuestMemory, registry: Arc<Registry>, tid: u32, start: u32, param: u32, stack_top: u32, stack_size: u32) {
    runtime::enter(rt.clone());
    CURRENT_TID.with(|c| c.set(tid));
    registry.install();
    ddnative_base::platform::redirect(&crate::session::REDIRECTED_MODELS);
    let parallel = rt.lock.parallel.load(std::sync::atomic::Ordering::Relaxed);
    if !parallel {
        let s = rt.lock.lock();
        rt.lock.acquire_locked(s, 1);
        adopt_context(&rt);
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
            let rt = runtime::current();
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
            let rt = runtime::current();
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

/// SetThreadPriority(handle, priority): accepted (the host schedules the
/// one running guest thread; priorities do not change which runs).
pub fn set_thread_priority(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 8);
}

/// SetThreadIdealProcessor(handle, processor) -> the previous ideal
/// processor (0: the runtime reports one processor, GetSystemInfo).
pub fn set_thread_ideal_processor(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 8);
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
            CURRENT_TID.with(|c| c.set(tid));
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
                r.run_service(f, &mut m, &mut c, target, locked);
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
        let (f, locked) = match r.get(target) {
            Some(Binding::Service(f)) => (f, true),
            Some(Binding::LockFree(f)) => (f, false),
            _ => unreachable!(),
        };
        r.run_service(f, m, c, target, locked);
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
                        CURRENT_TID.with(|c| c.set(tid));
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
                CURRENT_TID.with(|c| c.set(0x3000));
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
                    CURRENT_TID.with(|c| c.set(0x5000 + 4 * k as u32));
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
}
