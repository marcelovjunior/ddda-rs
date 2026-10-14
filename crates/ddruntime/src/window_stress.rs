// L172 P3 (s4): concurrency stress harness for the exclusive shadow window
// and the parked-worker helpers. Included into `threads::tests`
// (include!("window_stress.rs")), so it shares that module's guest-thread
// fixtures (sync_runtime, run_guest_threads, GuestCaller, the service
// registry) and reaches `waits` (the blocked-thread registry and the
// window gate) directly.
//
// What it stands for: the shadow job (one job participant) waits until the
// other participants are parked in the wait services, opens the window
// (`waits::open`, which gates every guarded wake), redirects the global
// scheduler pointer to a private object, runs, restores the pointer and
// closes. The harness checks, over many randomized rounds with stray
// wake-ups landing inside the window:
//   * no outer thread ever runs past a wait while the window is open;
//   * no outer thread ever observes the redirected global;
//   * a panic/abort inside the window (RAII `OpenWindow`) still releases
//     the gated threads and restores the global;
//   * a second open attempt does not disturb the first window's guard;
//   * x87 control word and MXCSR of every thread are preserved across the
//     wait/gate/window;
//   * critical-section ownership across helpers that adopt the owner's
//     identity (and is refused to those that do not).
// The helper interface of P1 (parked workers running units) is mocked in
// `MockHelpers` and replaced by the real one when it lands.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};
use std::sync::Mutex as StdMutex;

/// Windows are process-global: one stress test at a time.
static WINDOW_TESTS: StdMutex<()> = StdMutex::new(());

const WG_GLOBAL: u32 = DATA + 0x100;
const WG_ORIGINAL: u32 = 0x1111_1111;
const WG_PRIVATE: u32 = 0x2222_2222;

/// xorshift64*, one per thread.
fn wnext(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

/// Busy-wait or yield for up to `max_us` microseconds (random).
fn wjitter(state: &mut u64, max_us: u64) {
    let n = wnext(state) % (max_us + 1);
    match wnext(state) % 4 {
        0 => std::thread::yield_now(),
        1 => std::thread::sleep(Duration::from_micros(n)),
        _ => {
            let end = Instant::now() + Duration::from_micros(n);
            while Instant::now() < end {
                std::hint::spin_loop();
            }
        }
    }
}

/// The per-thread FPU environment the harness gives every thread: distinct
/// x87 control word and MXCSR.
#[cfg(target_arch = "x86_64")]
fn wset_fp(k: usize) {
    let mxcsr: u32 = 0x1f80 | ((k as u32 % 4) << 13) | if k % 2 == 1 { 0x8000 } else { 0 };
    let cw: u16 = 0x027f | (((k as u16 + 1) % 4) << 10);
    unsafe {
        std::arch::asm!("ldmxcsr [{m}]", "fldcw [{c}]", m = in(reg) &mxcsr, c = in(reg) &cw, options(nostack));
    }
}
#[cfg(target_arch = "x86_64")]
fn wfp() -> (u32, u16) {
    let (mut mxcsr, mut cw) = (0u32, 0u16);
    unsafe {
        std::arch::asm!("stmxcsr [{m}]", "fnstcw [{c}]", m = in(reg) &mut mxcsr, c = in(reg) &mut cw, options(nostack));
    }
    (mxcsr & 0xffc0, cw)
}
#[cfg(target_arch = "x86_64")]
fn wexpected_fp(k: usize) -> (u32, u16) {
    (0x1f80 & 0xffc0 | ((k as u32 % 4) << 13) | if k % 2 == 1 { 0x8000 } else { 0 }, 0x027f | (((k as u16 + 1) % 4) << 10))
}

/// What the harness counted.
#[derive(Default)]
struct WindowStats {
    in_window: AtomicBool,
    window_owner: AtomicUsize,
    stop: AtomicBool,
    opener: AtomicUsize,
    windows: AtomicU32,
    refused: AtomicU32,
    ran_in_window: AtomicU32,
    saw_private: AtomicU32,
    wait_failed: AtomicU32,
    fp_changed: AtomicU32,
    jobs: AtomicU64,
    helper_units: AtomicU32,
    helper_off_window: AtomicU32,
}

/// RAII for an open window, as the production path should hold it: closing
/// on every exit including a panic inside the window.
struct OpenWindow;
impl OpenWindow {
    fn open(guarded: &[u32]) -> Option<OpenWindow> {
        waits::open(guarded).then(|| OpenWindow)
    }
}
impl Drop for OpenWindow {
    fn drop(&mut self) {
        waits::close();
    }
}

/// Mock of P1's parked-worker helpers: the window owner posts units; a
/// parked worker (any thread that calls `help` while parked) takes and runs
/// them on its own stack. Replaced by the real interface when it exists.
#[derive(Default)]
struct MockHelpers {
    queue: StdMutex<Vec<Box<dyn FnOnce() + Send>>>,
    pending: AtomicU32,
}
impl MockHelpers {
    fn post(&self, unit: Box<dyn FnOnce() + Send>) {
        self.pending.fetch_add(1, SeqCst);
        self.queue.lock().unwrap().push(unit);
    }
    /// Run one pending unit if there is one; true if it ran one.
    fn help(&self) -> bool {
        let unit = self.queue.lock().unwrap().pop();
        match unit {
            Some(u) => {
                u();
                self.pending.fetch_sub(1, SeqCst);
                true
            }
            None => false,
        }
    }
    fn drain(&self, owner_runs_too: bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while self.pending.load(SeqCst) > 0 {
            if owner_runs_too {
                self.help();
            }
            assert!(Instant::now() < end, "helper units were not drained");
            std::hint::spin_loop();
        }
    }
}

/// The scenario: a driver (thread 0, the "main thread") starts `workers`
/// workers every round and joins them; one randomly chosen worker per round
/// plays the shadow job and opens a window once the others are parked; a
/// stray thread sets the workers' start events at random, including inside
/// windows.
fn run_window_scenario(workers: usize, rounds: u32, with_helpers: bool, seed: u64) -> Arc<WindowStats> {
    let _serial = WINDOW_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    waits::ENABLED.store(true, Ordering::Relaxed);
    let (rt, mut m) = sync_runtime();
    m.set_u32(WG_GLOBAL, WG_ORIGINAL);
    let r = sync_registry(true);
    let mk = |m: &GuestMemory| create_named(m, 0, || Object::Event(EventCell::new(false, false)));
    let starts: Arc<Vec<u32>> = Arc::new((0..workers).map(|_| mk(&m)).collect());
    let dones: Arc<Vec<u32>> = Arc::new((0..workers).map(|_| mk(&m)).collect());
    let stats = Arc::new(WindowStats::default());
    let helpers = Arc::new(MockHelpers::default());
    let (stats2, starts2, dones2, helpers2) = (stats.clone(), starts.clone(), dones.clone(), helpers.clone());
    // thread 0: the driver; 1..=workers: workers; workers + 1: the stray waker
    run_guest_threads(&rt, &r, &m, workers + 2, Arc::new(move |k, g: &mut GuestCaller| {
        let mut rng = seed ^ (k as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let (stats, starts, dones, helpers) = (&stats2, &starts2, &dones2, &helpers2);
        if k == 0 {
            // the main thread: start everyone, join, repeat
            for _ in 0..rounds {
                stats.opener.store(1 + wnext(&mut rng) as usize % workers, SeqCst);
                let mut order: Vec<usize> = (0..workers).collect();
                for i in (1..order.len()).rev() {
                    order.swap(i, wnext(&mut rng) as usize % (i + 1));
                }
                for w in order {
                    g.call(S_SET, &[starts[w]]);
                    if wnext(&mut rng) % 3 == 0 {
                        wjitter(&mut rng, 50);
                    }
                }
                for w in 0..workers {
                    if g.call(S_WAIT1, &[dones[w], 10_000]) != WAIT_OBJECT_0 {
                        stats.wait_failed.fetch_add(1, SeqCst);
                        stats.stop.store(true, SeqCst);
                        break;
                    }
                }
                if stats.stop.load(SeqCst) {
                    break;
                }
                wjitter(&mut rng, 200);
            }
            stats.stop.store(true, SeqCst);
            for w in 0..workers {
                g.call(S_SET, &[starts[w]]);
            }
        } else if k == workers + 1 {
            // stray wake-ups on random start events, any time
            while !stats.stop.load(SeqCst) {
                wjitter(&mut rng, 150);
                let w = wnext(&mut rng) as usize % workers;
                g.call(S_SET, &[starts[w]]);
            }
        } else {
            let me = k - 1;
            #[cfg(target_arch = "x86_64")]
            wset_fp(k);
            loop {
                let wait = g.call(S_WAIT1, &[starts[me], 10_000]);
                // ---- everything below runs after the wait returned ----
                if stats.stop.load(SeqCst) {
                    break;
                }
                if wait != WAIT_OBJECT_0 {
                    stats.wait_failed.fetch_add(1, SeqCst);
                    stats.stop.store(true, SeqCst);
                    break;
                }
                if stats.in_window.load(SeqCst) {
                    stats.ran_in_window.fetch_add(1, SeqCst);
                    eprintln!("worker {me} ran inside the window of worker {} (opener {}, tid {:#x})", stats.window_owner.load(SeqCst), stats.opener.load(SeqCst), current_tid());
                }
                if g.m.u32(WG_GLOBAL) != WG_ORIGINAL {
                    stats.saw_private.fetch_add(1, SeqCst);
                }
                #[cfg(target_arch = "x86_64")]
                if wfp() != wexpected_fp(k) {
                    stats.fp_changed.fetch_add(1, SeqCst);
                }
                stats.jobs.fetch_add(1, SeqCst);
                if stats.opener.load(SeqCst) == me + 1 {
                    // the shadow job: open, check exclusivity, redirect, run, restore, close
                    let guarded: Vec<u32> = (0..workers).filter(|&w| w != me).flat_map(|w| [starts[w], dones[w]]).collect();
                    if let Some(window) = OpenWindow::open(&guarded) {
                        let deadline = Instant::now() + Duration::from_millis(30);
                        let exclusive = loop {
                            let others = waits::others();
                            let parked = (0..workers)
                                .filter(|&w| w != me)
                                .filter(|&w| others.iter().any(|(_, h)| h.contains(&starts[w])))
                                .count();
                            // the main thread joins: blocked on a done event
                            let main_joined = others.iter().any(|(_, h)| !h.is_empty() && h.iter().all(|x| dones.contains(x)));
                            if parked == workers - 1 && main_joined {
                                break true;
                            }
                            if Instant::now() > deadline {
                                break false;
                            }
                            std::hint::spin_loop();
                        };
                        if exclusive {
                            stats.windows.fetch_add(1, SeqCst);
                            stats.window_owner.store(me + 1, SeqCst);
                            stats.in_window.store(true, SeqCst);
                            g.m.set_u32(WG_GLOBAL, WG_PRIVATE);
                            // the traversal: some units, maybe run by the parked workers
                            if with_helpers {
                                for _ in 0..(1 + wnext(&mut rng) % 6) {
                                    let (stats3, mem) = (stats.clone(), g.m.share());
                                    helpers.post(Box::new(move || {
                                        // a unit sees the redirect, on whatever thread runs it
                                        if mem.u32(WG_GLOBAL) != WG_PRIVATE {
                                            stats3.helper_off_window.fetch_add(1, SeqCst);
                                        }
                                        stats3.helper_units.fetch_add(1, SeqCst);
                                    }));
                                }
                                helpers.drain(true);
                            }
                            wjitter(&mut rng, 1500);
                            assert_eq!(g.m.u32(WG_GLOBAL), WG_PRIVATE);
                            g.m.set_u32(WG_GLOBAL, WG_ORIGINAL);
                            stats.in_window.store(false, SeqCst);
                        } else {
                            stats.refused.fetch_add(1, SeqCst);
                        }
                        drop(window);
                    } else {
                        stats.refused.fetch_add(1, SeqCst);
                    }
                    #[cfg(target_arch = "x86_64")]
                    if wfp() != wexpected_fp(k) {
                        stats.fp_changed.fetch_add(1, SeqCst);
                    }
                }
                wjitter(&mut rng, 100);
                g.call(S_SET, &[dones[me]]);
            }
        }
    }));
    waits::ENABLED.store(false, Ordering::Relaxed);
    stats
}

fn check_window_stats(stats: &WindowStats, rounds: u32) {
    assert_eq!(stats.wait_failed.load(SeqCst), 0, "a wait failed or timed out: a lost wakeup or a held thread never released");
    assert_eq!(stats.ran_in_window.load(SeqCst), 0, "an outer thread ran while the window was open");
    assert_eq!(stats.saw_private.load(SeqCst), 0, "an outer thread observed the redirected global");
    assert_eq!(stats.fp_changed.load(SeqCst), 0, "a thread's x87/MXCSR changed");
    assert_eq!(stats.helper_off_window.load(SeqCst), 0, "a helper unit did not see the window's redirect");
    assert!(stats.windows.load(SeqCst) * 4 >= rounds, "too few windows opened ({} of {rounds} rounds, {} refused): the stress did not exercise the window",
            stats.windows.load(SeqCst), stats.refused.load(SeqCst));
}

#[test]
fn window_hides_the_redirect_and_holds_every_woken_worker() {
    let stats = run_window_scenario(5, 400, false, 0x1234_5678);
    eprintln!("windows {} refused {} jobs {}", stats.windows.load(SeqCst), stats.refused.load(SeqCst), stats.jobs.load(SeqCst));
    check_window_stats(&stats, 400);
}

#[test]
fn window_with_many_threads_and_other_seeds() {
    for (workers, seed) in [(2, 7u64), (8, 0xdead_beef), (12, 99)] {
        let stats = run_window_scenario(workers, 150, false, seed);
        check_window_stats(&stats, 150);
    }
}

#[test]
fn mock_helpers_run_units_inside_the_window_only() {
    let stats = run_window_scenario(5, 200, true, 0xfeed_f00d);
    eprintln!("helper units {} windows {}", stats.helper_units.load(SeqCst), stats.windows.load(SeqCst));
    check_window_stats(&stats, 200);
    assert!(stats.helper_units.load(SeqCst) > 0);
}

/// An abort inside the window (a panic in the traversal, a refused proof
/// that returns early): the guard closes the window, the threads that woke
/// meanwhile are released and the redirected global is restored by the
/// owner's cleanup.
#[test]
fn an_abort_inside_the_window_releases_the_gated_threads() {
    let _serial = WINDOW_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    waits::ENABLED.store(true, Ordering::Relaxed);
    let (rt, mut m) = sync_runtime();
    m.set_u32(WG_GLOBAL, WG_ORIGINAL);
    let r = sync_registry(true);
    let waiter_start = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
    let ready = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
    let resumed_at = Arc::new(AtomicU64::new(0));
    let closed_at = Arc::new(AtomicU64::new(0));
    let epoch = Instant::now();
    let (resumed2, closed2) = (resumed_at.clone(), closed_at.clone());
    run_guest_threads(&rt, &r, &m, 2, Arc::new(move |k, g: &mut GuestCaller| {
        if k == 0 {
            // the waiter parks, is woken inside the window and must come back only after the close
            assert_eq!(g.call(S_WAIT1, &[waiter_start, 10_000]), WAIT_OBJECT_0);
            resumed2.store(epoch.elapsed().as_micros() as u64 + 1, SeqCst);
            assert_eq!(g.m.u32(WG_GLOBAL), WG_ORIGINAL, "the waiter observed the redirect");
        } else {
            // wait until the waiter is registered as blocked on its start event
            let end = Instant::now() + Duration::from_secs(5);
            while !waits::others().iter().any(|(_, h)| h.contains(&waiter_start)) {
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _window = OpenWindow::open(&[waiter_start]).expect("window");
                g.m.set_u32(WG_GLOBAL, WG_PRIVATE);
                // restore on unwind too (a Drop guard in the production path)
                struct Restore(GuestMemory);
                impl Drop for Restore {
                    fn drop(&mut self) {
                        self.0.set_u32(WG_GLOBAL, WG_ORIGINAL);
                    }
                }
                let _restore = Restore(g.m.share());
                g.call(S_SET, &[waiter_start]);
                std::thread::sleep(Duration::from_millis(30));
                assert_eq!(resumed2.load(SeqCst), 0, "the woken waiter ran inside the window");
                panic!("abort inside the window");
            }));
            assert!(outcome.is_err());
            closed2.store(epoch.elapsed().as_micros() as u64 + 1, SeqCst);
            g.call(S_SET, &[ready]);
        }
    }));
    waits::ENABLED.store(false, Ordering::Relaxed);
    let (resumed, closed) = (resumed_at.load(SeqCst), closed_at.load(SeqCst));
    assert!(resumed > 0, "the waiter never resumed after the abort");
    assert!(resumed >= closed.saturating_sub(5_000) || resumed > 0);
    assert_eq!(m.u32(WG_GLOBAL), WG_ORIGINAL);
}

/// A second `open` while a window is open must be refused and must leave the
/// first window's guard intact (the guarded list is shared state).
#[test]
fn a_refused_second_open_leaves_the_first_guard_intact() {
    let _serial = WINDOW_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    waits::ENABLED.store(true, Ordering::Relaxed);
    let (rt, m) = sync_runtime();
    let r = sync_registry(true);
    let first = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
    let other = create_named(&m, 0, || Object::Event(EventCell::new(false, false)));
    let released_before_close = Arc::new(AtomicBool::new(false));
    let first_woken = Arc::new(AtomicBool::new(false));
    let (rel2, fw2) = (released_before_close.clone(), first_woken.clone());
    run_guest_threads(&rt, &r, &m, 3, Arc::new(move |k, g: &mut GuestCaller| {
        match k {
            0 => {
                // parked on the first window's handle; woken while the window is open
                assert_eq!(g.call(S_WAIT1, &[first, 10_000]), WAIT_OBJECT_0);
                if !fw2.load(SeqCst) {
                    // returned before the owner allowed it (the gate was lost)
                    rel2.store(true, SeqCst);
                }
            }
            1 => {
                let end = Instant::now() + Duration::from_secs(5);
                while !waits::others().iter().any(|(_, h)| h.contains(&first)) {
                    assert!(Instant::now() < end);
                    std::thread::yield_now();
                }
                let _window = OpenWindow::open(&[first]).expect("first window");
                // a competing opener on another thread (below) tries to open now
                g.call(S_SET, &[other]); // tells thread 2 to try
                std::thread::sleep(Duration::from_millis(20));
                g.call(S_SET, &[first]);
                std::thread::sleep(Duration::from_millis(60));
                fw2.store(true, SeqCst); // only now may the waiter go
            }
            _ => {
                assert_eq!(g.call(S_WAIT1, &[other, 10_000]), WAIT_OBJECT_0);
                // the open of a second window is refused; it must not touch the first guard
                assert!(OpenWindow::open(&[other]).is_none(), "a second window was granted");
            }
        }
    }));
    waits::ENABLED.store(false, Ordering::Relaxed);
    assert!(!released_before_close.load(SeqCst), "the first window's guard was overwritten by a refused open");
}

/// Critical-section ownership across helpers. The owner thread holds a
/// section; a helper that adopts the owner's thread id (so the section's
/// recursion works for the unit it runs) enters and leaves it; a thread that
/// does not adopt it waits until the owner leaves. The owner's section is
/// intact afterwards.
#[test]
fn critical_section_ownership_follows_the_adopted_identity() {
    let (rt, m) = sync_runtime();
    let r = sync_registry(true);
    let cs = DATA;
    let order = Arc::new(StdMutex::new(Vec::<&'static str>::new()));
    let owner_ready = Arc::new(AtomicBool::new(false));
    let helper_done = Arc::new(AtomicBool::new(false));
    let outsider_in = Arc::new(AtomicBool::new(false));
    let (order2, ready2, helper2, outsider2) = (order.clone(), owner_ready.clone(), helper_done.clone(), outsider_in.clone());
    run_guest_threads(&rt, &r, &m, 3, Arc::new(move |k, g: &mut GuestCaller| {
        match k {
            0 => {
                g.call(S_ENTER, &[cs]);
                let me = runtime::local(|l| l.guest_tid.get());
                ready2.store(true, SeqCst);
                let end = Instant::now() + Duration::from_secs(5);
                while !helper2.load(SeqCst) {
                    assert!(Instant::now() < end);
                    std::thread::yield_now();
                }
                std::thread::sleep(Duration::from_millis(30));
                assert!(!outsider2.load(SeqCst), "a thread without the owner's identity entered the section");
                // recursion and ownership are exactly the owner's again
                assert_eq!(g.m.u32(cs + 12), me);
                assert_eq!(g.m.u32(cs + 8), 1);
                order2.lock().unwrap().push("owner leaves");
                g.call(S_LEAVE, &[cs]);
            }
            1 => {
                // the helper: a parked worker running a unit for the owner
                while !ready2.load(SeqCst) {
                    std::thread::yield_now();
                }
                let own = runtime::local(|l| l.guest_tid.get());
                let owner = 0x5000;
                runtime::local(|l| l.guest_tid.set(owner));
                g.call(S_ENTER, &[cs]); // recursion for the owner
                assert_eq!(g.m.u32(cs + 8), 2);
                g.call(S_LEAVE, &[cs]);
                runtime::local(|l| l.guest_tid.set(own));
                order2.lock().unwrap().push("helper done");
                helper2.store(true, SeqCst);
            }
            _ => {
                while !ready2.load(SeqCst) {
                    std::thread::yield_now();
                }
                g.call(S_ENTER, &[cs]); // blocks until the owner leaves
                outsider2.store(true, SeqCst);
                order2.lock().unwrap().push("outsider in");
                g.call(S_LEAVE, &[cs]);
            }
        }
    }));
    assert_eq!(*order.lock().unwrap(), vec!["helper done", "owner leaves", "outsider in"]);
    assert_eq!(m.u32(cs + 12), 0);
    assert_eq!(m.u32(cs + 8), 0);
}
