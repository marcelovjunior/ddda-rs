//! L172 P1b (s6): which guest threads are parked in our wait services, and
//! the exclusive window that holds them there.
//!
//! The parallel shadow runs inside a job of the outer batch. It may redirect
//! the global scheduler only while no other job participant can run guest
//! code. Every participant is either a worker blocked on its start event or
//! the main thread blocked on the done events of the outer join; both are
//! blocked in `WaitForSingleObject`/`WaitForMultipleObjects`, which register
//! here ([`blocked`]).
//!
//! * Detector ([`Registry::parked_others`], [`verdict`]): a thread counts as
//!   parked only while its wait has not returned. A wait that returned but
//!   has not left the registry (it is being held at the gate, or has not yet
//!   run its Drop) is not parked: it is runnable the moment the window ends.
//! * Hold: while a window is open, a wait on one of the window's handles
//!   that returns does not return to the guest until the window closes (the
//!   gate, in [`Blocked`]'s Drop).
//! * Violation: a guarded wait that returns, or a thread that begins a
//!   guarded wait, while the window is open means a participant woke (or
//!   ran) in the window. The owner polls [`Window::violated`] before any
//!   redirect and aborts. The woken thread is held regardless, so the
//!   violation can never reach guest code before the window closes.
//!
//! * Helpers (P1, s8): the window's owner can run tasks ([`help`]) on the
//!   threads it holds: a thread parked in a guarded wait stops waiting
//!   ([`WAIT_HELP`], the wait services' early return), runs published tasks
//!   without the guest lock on a CPU of its own (its TEB, an empty x87
//!   stack, a stack below its own), then waits on with the time that is
//!   left. A thread held at the gate (its wait returned) takes tasks too.
//!   Helper wakes only poke the guarded events (they recheck and wait on);
//!   no event is ever set or consumed for a helper.
//!
//! The registry is a value so tests drive it with explicit thread ids; the
//! game uses the global one through the free functions. Inactive (one
//! relaxed load per wait) unless publish mode enabled it.

use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::{Relaxed, SeqCst}};
use std::sync::{Condvar, Mutex, MutexGuard};

pub(crate) static ENABLED: AtomicBool = AtomicBool::new(false);

struct Entry {
    tid: u32,
    handles: Box<[u32]>,
    /// the wait returned (the thread is leaving; it may be held at the gate)
    woken: bool,
}

#[derive(Default)]
struct State {
    entries: Vec<Entry>,
    /// the window's owner (0: none) and the handles it guards
    owner: u32,
    guarded: Vec<u32>,
    /// guarded waits that returned or began since the window opened
    violations: u32,
}

pub(crate) struct Registry {
    state: Mutex<State>,
    closed: Condvar,
}

fn intersects(a: &[u32], b: &[u32]) -> bool {
    a.iter().any(|h| b.contains(h))
}

impl Registry {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(State { entries: Vec::new(), owner: 0, guarded: Vec::new(), violations: 0 }),
            closed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `tid` is about to block on `handles`.
    pub(crate) fn enter(&self, tid: u32, handles: &[u32]) {
        let mut st = self.lock();
        if st.owner != 0 && st.owner != tid && intersects(handles, &st.guarded) {
            // a participant was running guest code while the window was open
            st.violations += 1;
        }
        st.entries.push(Entry { tid, handles: handles.into(), woken: false });
    }

    /// `tid`'s wait returned: held here while a window guarding one of its
    /// handles is open (owned by another thread), then removed.
    pub(crate) fn leave(&self, tid: u32) {
        self.leave_helping(tid, None)
    }

    /// [`leave`](Self::leave); while held, `help` (if any) runs whenever
    /// tasks are published, with the registry unlocked.
    pub(crate) fn leave_helping(&self, tid: u32, mut help: Option<&mut dyn FnMut()>) {
        let mut st = self.lock();
        let Some(i) = st.entries.iter().position(|e| e.tid == tid) else { return };
        st.entries[i].woken = true;
        let mut counted = false;
        loop {
            let held = st.owner != 0
                && st.owner != tid
                && st.entries.iter().find(|e| e.tid == tid).is_some_and(|e| intersects(&e.handles, &st.guarded));
            if !held {
                break;
            }
            if !counted {
                st.violations += 1;
                counted = true;
            }
            if let Some(help) = help.as_mut() {
                if PENDING.load(SeqCst) > 0 && TASKS_RUNTIME.load(SeqCst) == runtime_identity() {
                    drop(st);
                    help();
                    st = self.lock();
                    continue;
                }
            }
            st = self.closed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        if let Some(i) = st.entries.iter().position(|e| e.tid == tid) {
            st.entries.swap_remove(i);
        }
    }

    /// Whether a window is open that guards one of `handles`.
    fn guards(&self, handles: &[u32]) -> bool {
        let st = self.lock();
        st.owner != 0 && intersects(handles, &st.guarded)
    }

    /// Every handle of the parked threads whose wait the open window guards.
    fn guarded_waiters(&self) -> Vec<Box<[u32]>> {
        let st = self.lock();
        st.entries.iter().filter(|e| !e.woken && intersects(&e.handles, &st.guarded)).map(|e| e.handles.clone()).collect()
    }

    /// Wake the gated threads (new tasks).
    fn notify_closed(&self) {
        let _st = self.lock();
        self.closed.notify_all();
    }

    /// The handles each other thread is parked on now (waits not yet returned).
    pub(crate) fn parked_others(&self, me: u32) -> Vec<(u32, Box<[u32]>)> {
        self.lock().entries.iter().filter(|e| e.tid != me && !e.woken).map(|e| (e.tid, e.handles.clone())).collect()
    }

    /// Open the window for `me` guarding `handles` (None: another is open).
    pub(crate) fn open(&self, me: u32, handles: &[u32]) -> Option<Window<'_>> {
        let mut st = self.lock();
        if st.owner != 0 {
            return None;
        }
        st.owner = me;
        st.guarded = handles.to_vec();
        st.violations = 0;
        Some(Window { registry: self, me })
    }
}

/// An open exclusive window; closing (drop) releases every held thread.
pub(crate) struct Window<'a> {
    registry: &'a Registry,
    me: u32,
}

impl Window<'_> {
    /// A participant woke or began a guarded wait since the window opened.
    pub(crate) fn violated(&self) -> bool {
        self.registry.lock().violations != 0
    }

    /// The threads parked (and held) now: the helpers' candidates.
    pub(crate) fn parked(&self) -> Vec<(u32, Box<[u32]>)> {
        self.registry.parked_others(self.me)
    }

    pub(crate) fn check<'b>(&self, starts: &'b [u32], dones: &'b [u32]) -> Option<Vec<u32>> {
        verdict(&self.parked(), starts, dones)
    }
}

impl Drop for Window<'_> {
    fn drop(&mut self) {
        let mut st = self.registry.lock();
        st.owner = 0;
        st.guarded.clear();
        self.registry.closed.notify_all();
    }
}

/// Whether the job participants are all parked: `starts` the workers' start
/// events, `dones` their done events (the main thread's join), `others` the
/// parked threads other than the caller. Exclusive when every worker is
/// parked on its start event and the caller is the main thread (it is not
/// parked, so no done wait is registered), or all but one are and the
/// caller is that worker with the main thread parked in the join. Returns the
/// handles to guard.
pub(crate) fn verdict(others: &[(u32, Box<[u32]>)], starts: &[u32], dones: &[u32]) -> Option<Vec<u32>> {
    // a start event with a parked thread, distinct threads
    let parked_starts = starts.iter().filter(|s| others.iter().any(|(_, h)| h.contains(s))).count();
    let unparked = starts.len() - parked_starts;
    let main_parked = others.iter().any(|(_, h)| !h.is_empty() && h.iter().all(|x| dones.contains(x)));
    let ok = (unparked == 0 && !main_parked) || (unparked == 1 && main_parked);
    ok.then(|| starts.iter().chain(dones).copied().collect())
}

/// Debug (L172 trace): every registered wait and the window, one line.
pub(crate) fn dump() -> String {
    let st = GLOBAL.lock();
    let mut out = format!("owner={:#x} guarded={:x?} pending={}", st.owner, st.guarded, PENDING.load(SeqCst));
    for e in &st.entries {
        let states: Vec<String> = e.handles.iter().map(|&h| format!("{h:#x}:{}", crate::threads::handle_state(h))).collect();
        out += &format!(" | {:#x}{} [{}]", e.tid, if e.woken { " woken" } else { "" }, states.join(","));
    }
    out
}

static GLOBAL: Registry = Registry::new();

/// Registers the current thread as blocked on `handles` until dropped; the
/// drop passes the window's gate first.
pub(crate) struct Blocked(Option<u32>);

pub(crate) fn blocked(handles: &[u32]) -> Blocked {
    if !ENABLED.load(Relaxed) {
        return Blocked(None);
    }
    let tid = crate::threads::current_tid();
    GLOBAL.enter(tid, handles);
    Blocked(Some(tid))
}

impl Blocked {
    /// The gate, for a wait that is returning: held (registered as parked)
    /// while a window guarding its handles is open, running the tasks the
    /// window's owner publishes; call before returning to the guest.
    pub(crate) fn pass(&self, m: &mut dyn Mem, cpu: &Cpu) {
        if let Some(tid) = self.0 {
            GLOBAL.leave_helping(tid, Some(&mut || crate::threads::without_lock(|| run_pending(m, cpu))));
        }
    }
}

impl Drop for Blocked {
    fn drop(&mut self) {
        if let Some(tid) = self.0 {
            GLOBAL.leave(tid);
        }
    }
}

// ---- helpers: tasks run on the parked threads ------------------------------------

/// A unit of work for a thread held in a guarded wait (or the window's
/// owner): it runs with that thread's own memory view and a CPU prepared
/// for it (its TEB, a stack below the thread's own, an empty x87 stack).
pub(crate) type Task = Box<dyn FnOnce(&mut dyn Mem, &mut Cpu) -> Result<(), String> + Send>;

/// Tasks published by [`help`] and their results.
struct Board {
    queue: VecDeque<(usize, Task)>,
    results: Vec<Option<Result<(), String>>>,
    outstanding: usize,
    /// a [`help`] call is running (one at a time)
    active: bool,
}

static BOARD: Mutex<Board> = Mutex::new(Board { queue: VecDeque::new(), results: Vec::new(), outstanding: 0, active: false });
static DONE: Condvar = Condvar::new();
/// Tasks published and not yet taken: the cheap test of the wait loops.
static PENDING: AtomicU32 = AtomicU32::new(0);
/// The runtime (its guest lock's identity) the published tasks belong to: a
/// thread of another runtime (tests run several in one process) never takes
/// them, whatever its handle values.
static TASKS_RUNTIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn runtime_identity() -> u64 {
    crate::runtime::pin().lock.identity()
}

/// What a wait returns, internally, when it stopped to let its thread run
/// tasks (never a Win32 wait result).
pub(crate) const WAIT_HELP: u32 = 0x7FFF_0001;

/// How far below its own stack pointer a task starts its stack.
const STACK_MARGIN: u32 = 0x400;

/// Whether a wait on `handles` should stop to run tasks: some are published
/// and the open window guards the wait.
#[inline]
pub(crate) fn help_due(handles: &[u32]) -> bool {
    PENDING.load(SeqCst) > 0 && TASKS_RUNTIME.load(SeqCst) == runtime_identity() && GLOBAL.guards(handles)
}

/// The CPU a task starts with: the service's registers and TEB, an empty x87
/// stack, and a stack of its own below the thread's stack pointer.
fn task_cpu(service: &Cpu) -> Cpu {
    let mut cpu = service.clone();
    cpu.st = Default::default();
    cpu.esp = service.esp.wrapping_sub(STACK_MARGIN) & !0xf;
    cpu
}

fn take() -> Option<(usize, Task)> {
    let mut b = BOARD.lock().unwrap_or_else(|e| e.into_inner());
    let t = b.queue.pop_front();
    if t.is_some() {
        PENDING.fetch_sub(1, SeqCst);
    }
    t
}

fn complete(index: usize, result: Result<(), String>) {
    let mut b = BOARD.lock().unwrap_or_else(|e| e.into_inner());
    b.results[index] = Some(result);
    b.outstanding -= 1;
    if b.outstanding == 0 {
        DONE.notify_all();
    }
}

/// Run published tasks, one at a time, until none is left to take, on this
/// thread with its own `m` and a CPU derived from `service` (the CPU of the
/// service it is in, or the owner's).
pub(crate) fn run_pending(m: &mut dyn Mem, service: &Cpu) {
    while let Some((index, task)) = take() {
        let mut cpu = task_cpu(service);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(m, &mut cpu)));
        // x87 and MXCSR as the service found them: the task's x87 state goes
        // (the thread's own was parked when the service began) and the host
        // default MXCSR is back whatever the task left
        if cpu.st.home != 0 {
            (ddnative_base::hooks::get().x87_release)(&mut cpu.st);
        }
        ddnative_base::x87::park();
        if ddnative_base::x86::stmxcsr() != ddnative_base::x86::MXCSR_DEFAULT {
            ddnative_base::x86::ldmxcsr(ddnative_base::x86::MXCSR_DEFAULT);
        }
        complete(index, match result {
            Ok(r) => r,
            Err(payload) => Err(payload.downcast_ref::<&str>().map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "task panicked".into())),
        });
    }
}

/// Wake every thread in a wait the open window guards, so that it sees the
/// tasks (a poke: nothing is set, taken or consumed), and the gated ones.
fn poke_guarded() {
    for handles in GLOBAL.guarded_waiters() {
        for &h in handles.iter() {
            crate::threads::poke_handle(h);
        }
    }
    GLOBAL.notify_closed();
}

/// Run `tasks` on the threads held in the open window's guarded waits and on
/// the calling thread (the window's owner, with its `m` and `cpu`:
/// `cpu.mxcsr` is the guest MXCSR the tasks run under, their stacks start
/// below `cpu.esp`). Returns when every task has finished, with its result
/// in task order; a task that panics is an `Err`. With no helper the owner
/// runs them all.
pub(crate) fn help(m: &mut dyn Mem, cpu: &Cpu, tasks: Vec<Task>) -> Vec<Result<(), String>> {
    let n = tasks.len();
    if n == 0 {
        return Vec::new();
    }
    {
        let mut b = BOARD.lock().unwrap_or_else(|e| e.into_inner());
        assert!(!b.active, "waits::help is already running");
        b.active = true;
        b.queue = tasks.into_iter().enumerate().collect();
        b.results = (0..n).map(|_| None).collect();
        b.outstanding = n;
        TASKS_RUNTIME.store(runtime_identity(), SeqCst);
        PENDING.store(n as u32, SeqCst);
    }
    poke_guarded();
    run_pending(m, cpu);
    let mut b = BOARD.lock().unwrap_or_else(|e| e.into_inner());
    while b.outstanding > 0 {
        b = DONE.wait(b).unwrap_or_else(|e| e.into_inner());
    }
    b.active = false;
    std::mem::take(&mut b.results).into_iter().map(|r| r.expect("a finished task has a result")).collect()
}

impl Window<'_> {
    /// [`help`] for this window's owner.
    pub(crate) fn help(&self, m: &mut dyn Mem, cpu: &Cpu, tasks: Vec<Task>) -> Vec<Result<(), String>> {
        help(m, cpu, tasks)
    }
}

/// The handles each other thread is parked on now.
pub(crate) fn others() -> Vec<(u32, Box<[u32]>)> {
    GLOBAL.parked_others(crate::threads::current_tid())
}

/// Open the global window for the current thread (None: another is open).
pub(crate) fn open(handles: &[u32]) -> Option<Window<'static>> {
    GLOBAL.open(crate::threads::current_tid(), handles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    const MAIN: u32 = 1;
    const W1: u32 = 2;
    const W2: u32 = 3;
    const START: [u32; 2] = [0x10, 0x11];
    const DONE: [u32; 2] = [0x20, 0x21];

    /// Polls until `n` threads are parked (they register before blocking).
    fn until_parked(r: &Registry, me: u32, n: usize) {
        for _ in 0..2000 {
            if r.parked_others(me).len() >= n {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("threads never parked");
    }

    #[test]
    fn verdict_requires_every_participant_parked() {
        let all_workers: Vec<(u32, Box<[u32]>)> = vec![(W1, [START[0]].into()), (W2, [START[1]].into())];
        // the main thread runs the window: both workers parked
        assert_eq!(verdict(&all_workers, &START, &DONE).map(|v| v.len()), Some(4));
        // one worker missing
        assert!(verdict(&all_workers[..1], &START, &DONE).is_none());
        // a worker runs the window, the other worker and the main thread parked
        let join: (u32, Box<[u32]>) = (MAIN, DONE.into());
        assert!(verdict(&[all_workers[1].clone(), join.clone()], &START, &DONE).is_some());
        // ... but the main thread is not parked: not exclusive
        assert!(verdict(&all_workers[1..], &START, &DONE).is_none());
        // two workers unparked
        assert!(verdict(&[join.clone()], &START, &DONE).is_none());
        // everyone parked and the main thread too: the caller cannot be a participant
        let mut everyone = all_workers.clone();
        everyone.push(join);
        assert!(verdict(&everyone, &START, &DONE).is_none());
        // the main thread blocked on something else is not the join
        let other: (u32, Box<[u32]>) = (MAIN, [0x99].into());
        assert!(verdict(&[all_workers[1].clone(), other], &START, &DONE).is_none());
    }

    #[test]
    fn parked_threads_are_listed_and_leave_clears_them() {
        let r = Arc::new(Registry::new());
        let release = Arc::new(Barrier::new(2));
        let t = {
            let (r, release) = (r.clone(), release.clone());
            std::thread::spawn(move || {
                r.enter(W1, &[START[0]]);
                release.wait();
                r.leave(W1);
            })
        };
        until_parked(&r, MAIN, 1);
        assert_eq!(r.parked_others(MAIN), vec![(W1, Box::from([START[0]]))]);
        assert!(r.parked_others(W1).is_empty(), "the caller is not its own other");
        release.wait();
        t.join().unwrap();
        assert!(r.parked_others(MAIN).is_empty());
    }

    /// A worker wakes mid-window: it is held until the window closes, the
    /// window reports the violation, and nothing runs before the close.
    #[test]
    fn wake_in_window_is_held_and_detected() {
        let r = Arc::new(Registry::new());
        let wake = Arc::new(Barrier::new(2));
        let returned = Arc::new(AtomicU32::new(0));
        let t = {
            let (r, wake, returned) = (r.clone(), wake.clone(), returned.clone());
            std::thread::spawn(move || {
                r.enter(W1, &[START[0]]);
                wake.wait(); // the event is signalled: the wait returns
                r.leave(W1);
                returned.store(1, SeqCst); // guest code would run here
            })
        };
        until_parked(&r, MAIN, 1);
        let window = r.open(MAIN, &[START[0], DONE[0]]).expect("window");
        assert!(!window.violated());
        assert_eq!(window.parked().len(), 1);
        wake.wait();
        // the woken worker is no longer parked and is flagged
        for _ in 0..2000 {
            if window.violated() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(window.violated(), "the wake must be detected");
        assert!(window.parked().is_empty(), "a woken thread is not parked");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(returned.load(SeqCst), 0, "held while the window is open");
        drop(window);
        t.join().unwrap();
        assert_eq!(returned.load(SeqCst), 1, "released by the close");
        assert!(r.parked_others(MAIN).is_empty());
    }

    #[test]
    fn unguarded_waits_pass_the_gate() {
        let r = Arc::new(Registry::new());
        let window = r.open(MAIN, &[START[0]]).expect("window");
        let (tx, rx) = mpsc::channel();
        let t = {
            let r = r.clone();
            std::thread::spawn(move || {
                r.enter(W2, &[0x77]);
                r.leave(W2);
                tx.send(()).unwrap();
            })
        };
        rx.recv_timeout(Duration::from_secs(5)).expect("an unrelated wait is not held");
        t.join().unwrap();
        assert!(!window.violated());
    }

    #[test]
    fn beginning_a_guarded_wait_in_the_window_is_a_violation() {
        let r = Arc::new(Registry::new());
        let window = r.open(MAIN, &[START[0]]).expect("window");
        r.enter(W1, &[START[0]]); // a worker finished a job and went back to its wait
        assert!(window.violated());
        drop(window);
        r.leave(W1);
    }

    #[test]
    fn one_window_at_a_time_and_a_failed_open_changes_nothing() {
        let r = Registry::new();
        let first = r.open(MAIN, &[START[0]]).expect("window");
        assert!(r.open(W1, &[START[1]]).is_none());
        // the second attempt must not have replaced the first's guard
        r.enter(W2, &[START[1]]);
        assert!(!first.violated(), "the failed open's handles are not guarded");
        r.leave(W2);
        drop(first);
        assert!(r.open(W1, &[START[1]]).is_some(), "closing frees the slot");
    }

    #[test]
    fn closing_releases_every_held_thread_and_the_window_can_reopen() {
        let r = Arc::new(Registry::new());
        let n = 4u32;
        let go = Arc::new(Barrier::new(n as usize + 1));
        let done = Arc::new(AtomicU32::new(0));
        let ts: Vec<_> = (0..n)
            .map(|k| {
                let (r, go, done) = (r.clone(), go.clone(), done.clone());
                std::thread::spawn(move || {
                    r.enter(10 + k, &[START[0]]);
                    go.wait();
                    r.leave(10 + k);
                    done.fetch_add(1, SeqCst);
                })
            })
            .collect();
        until_parked(&r, MAIN, n as usize);
        let window = r.open(MAIN, &[START[0]]).unwrap();
        go.wait();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(done.load(SeqCst), 0);
        drop(window);
        for t in ts {
            t.join().unwrap();
        }
        assert_eq!(done.load(SeqCst), n);
        let again = r.open(MAIN, &[START[0]]).expect("reopen");
        assert!(!again.violated(), "violations reset per window");
    }

    /// The owner's own waits are never held by its window.
    #[test]
    fn the_owner_is_not_gated() {
        let r = Registry::new();
        let window = r.open(MAIN, &[START[0]]).unwrap();
        r.enter(MAIN, &[START[0]]);
        r.leave(MAIN);
        assert!(!window.violated());
    }
}
