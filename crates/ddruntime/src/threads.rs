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
    /// Incremented whenever a kernel object or lock may have changed state.
    signals: u64,
    /// Suspend counts by thread id (a suspended thread does not take the lock).
    suspended: HashMap<u32, u32>,
}

pub struct GuestLock {
    state: Mutex<LockState>,
    cv: Condvar,
    /// Translated code runs without the lock (see the module comment).
    parallel: std::sync::atomic::AtomicBool,
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
            parallel: std::sync::atomic::AtomicBool::new(parallel),
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
        self.cv.notify_all();
    }

    /// Take the lock when it is free and this thread is not suspended,
    /// with `depth` nested entries.
    fn acquire_locked<'a>(&'a self, mut s: std::sync::MutexGuard<'a, LockState>, depth: u32) {
        let me = current_tid();
        s.waiting += 1;
        signal_preempt(&s);
        while s.held || s.suspended.get(&me).copied().unwrap_or(0) > 0 {
            s = self.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        s.waiting -= 1;
        signal_preempt(&s);
        s.held = true;
        s.holder = me;
        s.depth = depth;
        s.acquisitions += 1;
        SLICE_START.with(|t| t.set(Some(Instant::now())));
    }
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
pub(crate) fn without_lock<R>(f: impl FnOnce() -> R) -> R {
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

/// Note that kernel state changed: every waiting thread rechecks.
pub(crate) fn signal() {
    let rt = runtime::current();
    let mut s = rt.lock.lock();
    s.signals += 1;
    rt.lock.cv.notify_all();
}

/// Block until kernel state changes (or `deadline`), giving the lock to the
/// other guest threads meanwhile. The caller rechecks its condition.
pub(crate) fn block(deadline: Option<Instant>) {
    let rt = runtime::current();
    let mut s = rt.lock.lock();
    let seen = s.signals;
    let depth = s.depth;
    rt.lock.release_locked(&mut s);
    while s.signals == seen {
        match deadline {
            None => s = rt.lock.cv.wait(s).unwrap_or_else(|e| e.into_inner()),
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    break;
                }
                s = rt.lock.cv.wait_timeout(s, d - now).unwrap_or_else(|e| e.into_inner()).0;
            }
        }
    }
    rt.lock.acquire_locked(s, depth);
    adopt_context(&rt);
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
    ddnative_base::x86::PREEMPT.store(runnable, std::sync::atomic::Ordering::Relaxed);
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

/// The preemption hook of lifted calls (ddnative_base::x86::set_preempt_hook):
/// in serial mode, the lock holder yields when its time slice is used up.
/// The guest MXCSR is live in the host register inside lifted code, so it
/// is kept across the switch.
pub(crate) fn preempt() {
    if ANY_PARALLEL.load(std::sync::atomic::Ordering::Relaxed) || IN_SERVICE.with(Cell::get) > 0 {
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

#[derive(Debug)]
pub enum Object {
    Event { manual: bool, set: bool },
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
    kernel(|k| k.handles.remove(&h).is_some())
}

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
}

/// A named object: the existing one (ERROR_ALREADY_EXISTS) or a new one.
fn create_named(m: &dyn Mem, name: u32, make: impl FnOnce() -> Object) -> u32 {
    let name = (name != 0).then(|| crate::user32::guest_str_a(m, name));
    kernel(|k| {
        if let Some(&o) = name.as_ref().and_then(|n| k.names.get(n)) {
            with_state(|s| s.last_error = ERROR_ALREADY_EXISTS);
            return k.handle(o);
        }
        with_state(|s| s.last_error = 0);
        let o = k.add(make());
        if let Some(n) = name {
            k.names.insert(n, o);
        }
        k.handle(o)
    })
}

/// CreateEventA(security, manual reset, initial state, name).
pub fn create_event(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (manual, set, name) = (arg(m, cpu, 1) != 0, arg(m, cpu, 2) != 0, arg(m, cpu, 3));
    cpu.eax = create_named(m, name, || Object::Event { manual, set });
    ret(m, cpu, 16);
}

fn set_event_state(m: &mut dyn Mem, cpu: &mut Cpu, value: bool) {
    let h = arg(m, cpu, 0);
    let ok = kernel(|k| match k.object_of(h).and_then(|o| k.objects.get_mut(&o)) {
        Some(Object::Event { set, .. }) => {
            *set = value;
            true
        }
        _ => false,
    });
    if ok {
        if value {
            signal();
        }
    } else {
        set_error(ERROR_INVALID_HANDLE);
    }
    cpu.eax = ok as u32;
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
    let r = kernel(|k| match k.object_of(h).and_then(|o| k.objects.get_mut(&o)) {
        Some(Object::Semaphore { count, max }) => match count.checked_add(n).filter(|c| c <= max && n > 0) {
            Some(c) => Ok(std::mem::replace(count, c)),
            None => Err(ERROR_TOO_MANY_POSTS),
        },
        _ => Err(ERROR_INVALID_HANDLE),
    });
    cpu.eax = match r {
        Ok(old) => {
            if prev != 0 {
                m.set_u32(prev, old);
            }
            signal();
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
    let r = kernel(|k| match k.object_of(h).and_then(|o| k.objects.get_mut(&o)) {
        Some(Object::Mutex { owner, recursion }) if *owner == me => {
            *recursion -= 1;
            if *recursion == 0 {
                *owner = 0;
            }
            Ok(())
        }
        Some(Object::Mutex { .. }) => Err(ERROR_NOT_OWNER),
        _ => Err(ERROR_INVALID_HANDLE),
    });
    cpu.eax = match r {
        Ok(()) => {
            signal();
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
        Object::Event { set, .. } => *set,
        Object::Semaphore { count, .. } => *count > 0,
        Object::Mutex { owner, .. } => *owner == 0 || *owner == me,
        Object::Thread { exit, .. } => exit.is_some(),
    }
}

/// The effect of a satisfied wait.
fn consume(o: &mut Object, me: u32) {
    match o {
        Object::Event { manual: false, set } => *set = false,
        Object::Semaphore { count, .. } => *count -= 1,
        Object::Mutex { owner, recursion } => {
            *owner = me;
            *recursion += 1;
        }
        _ => {}
    }
}

/// Wait for `handles` (all of them, or any), up to `ms` milliseconds.
pub fn wait(handles: &[u32], all: bool, ms: u32) -> u32 {
    let me = current_tid();
    let deadline = (ms != INFINITE).then(|| Instant::now() + Duration::from_millis(ms as u64));
    loop {
        let r = kernel(|k| {
            let objs: Option<Vec<u32>> = handles.iter().map(|&h| k.object_of(h)).collect();
            let Some(objs) = objs else { return Some(WAIT_FAILED) };
            if all {
                if objs.iter().all(|o| signalled(&k.objects[o], me)) {
                    for o in &objs {
                        consume(k.objects.get_mut(o).unwrap(), me);
                    }
                    return Some(WAIT_OBJECT_0);
                }
            } else if let Some(i) = objs.iter().position(|o| signalled(&k.objects[o], me)) {
                consume(k.objects.get_mut(&objs[i]).unwrap(), me);
                return Some(WAIT_OBJECT_0 + i as u32);
            }
            None
        });
        match r {
            Some(WAIT_FAILED) => {
                set_error(ERROR_INVALID_HANDLE);
                return WAIT_FAILED;
            }
            Some(r) => return r,
            None if deadline.is_some_and(|d| Instant::now() >= d) => return WAIT_TIMEOUT,
            None => block(deadline),
        }
    }
}

/// WaitForSingleObject(handle, ms).
pub fn wait_for_single_object(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, ms) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = wait(&[h], false, ms);
    ret(m, cpu, 8);
}

/// WaitForMultipleObjects(count, handles*, wait all, ms).
pub fn wait_for_multiple_objects(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (n, p, all, ms) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2) != 0, arg(m, cpu, 3));
    let handles: Vec<u32> = (0..n.min(64)).map(|k| m.u32(p + 4 * k)).collect();
    cpu.eax = wait(&handles, all, ms);
    ret(m, cpu, 16);
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
            block(None);
        },
        _ => sleep_unlocked(Duration::from_millis(ms as u64)),
    }
}
/// Queue an asynchronous procedure call (an I/O completion routine) to
/// guest thread `tid`; it runs at that thread's next alertable wait.
pub(crate) fn queue_apc(tid: u32, routine: u32, args: [u32; 3]) {
    kernel(|k| k.apcs.entry(tid).or_default().push_back((routine, args)));
    signal();
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
            block(deadline);
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
    let mut s = rt.lock.lock();
    s.signals += 1;
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
