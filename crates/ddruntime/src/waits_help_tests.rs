// Included in threads::tests (L172 P1): tasks run on threads held in guarded
// waits, behind the window's gate.

/// A guest thread of a test, started now (the caller keeps running).
fn spawn_guest_thread(rt: &Arc<Runtime>, r: &Arc<Registry>, m: &GuestMemory, k: usize,
                      body: impl FnOnce(&mut GuestCaller) + Send + 'static) -> std::thread::JoinHandle<()> {
    let (rt, r, mut m) = (rt.clone(), r.clone(), m.share());
    std::thread::spawn(move || {
        runtime::enter(rt);
        runtime::local(|l| l.guest_tid.set(0x5000 + 4 * k as u32));
        let stack = 0x0300_0000 + k as u32 * 0x1_0000;
        let _ = m.map(stack, 0x1_0000, READ | WRITE);
        let mut caller = GuestCaller { r, m, c: Cpu::default(), top: stack + 0xF000 };
        body(&mut caller);
    })
}

/// Wait (up to 5 s) until `n` threads are blocked in guarded waits.
fn wait_for_blocked(n: usize) {
    let t = Instant::now();
    while waits::others().len() < n {
        assert!(t.elapsed() < Duration::from_secs(5), "only {} threads blocked", waits::others().len());
        std::thread::sleep(Duration::from_millis(1));
    }
    // (a thread registers just before it parks)
    std::thread::sleep(Duration::from_millis(20));
}

/// The window, the task board and the enable flag are process-wide: these
/// tests take turns.
static WINDOW_TESTS: Mutex<()> = Mutex::new(());

/// L172 P1: the owner's tasks run on the threads parked in guarded waits
/// (own thread ids), with an empty x87 stack and the default MXCSR left
/// behind; the parked threads stay held until the window closes; a task
/// that panics is an Err and the others still run.
#[test]
fn helpers_run_tasks_on_parked_threads_behind_the_gate() {
    let _turn = WINDOW_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    ddnative_host::install();
    let (rt, mut m) = sync_runtime();
    m.map(0x02F0_0000, 0x20_0000, READ | WRITE).unwrap();
    waits::ENABLED.store(true, Ordering::SeqCst);
    let r = sync_registry(true);
    let workers = 4usize;
    let events: Vec<u32> = (0..workers).map(|_| create_named(&m, 0, || Object::Event(EventCell::new(false, false)))).collect();
    let returned = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let threads: Vec<_> = (0..workers).map(|k| {
        let (ev, returned) = (events[k], returned.clone());
        spawn_guest_thread(&rt, &r, &m, k, move |g| {
            let got = g.call(S_WAIT1, &[ev, 10_000]);
            assert_eq!(got, WAIT_OBJECT_0);
            returned.fetch_add(1, Ordering::SeqCst);
        })
    }).collect();
    wait_for_blocked(workers);

    // the owner: the test thread, as guest thread 0x4000
    runtime::local(|l| l.guest_tid.set(0x4000));
    let window = waits::open(&events).expect("window");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut tasks: Vec<waits::Task> = Vec::new();
    for i in 0..12u32 {
        let seen = seen.clone();
        tasks.push(Box::new(move |mem, cpu| {
            // a task owns a stack below its thread's and an empty x87 stack
            assert_eq!(cpu.st.depth(), 0, "task {i}: x87 stack not empty");
            let mut operand = [0u8; 10];
            operand[..4].copy_from_slice(&1.5f32.to_le_bytes());
            cpu.st.exec::<0x00d9>(&mut operand); // FLD: left on the stack on purpose
            mem.set_u32(DATA + 0x300 + 4 * i, current_tid());
            ddnative_base::x86::ldmxcsr(0x1FA0); // and a changed MXCSR
            seen.lock().unwrap().push(current_tid());
            if i < 3 {
                // the first tasks wait for a second thread to start one: only a
                // parked helper can be that thread
                let t = Instant::now();
                while seen.lock().unwrap().iter().collect::<std::collections::HashSet<_>>().len() < 2 {
                    assert!(t.elapsed() < Duration::from_secs(5), "no helper joined in");
                    std::thread::sleep(Duration::from_micros(200));
                }
            }
            std::thread::sleep(Duration::from_millis(3));
            if i == 7 {
                panic!("task seven fails");
            }
            Ok(())
        }));
    }
    let mut owner_mem = m.share();
    let owner_cpu = Cpu { esp: 0x0310_0000 - 0x100, ..Cpu::default() };
    let results = window.help(&mut owner_mem, &owner_cpu, tasks);
    assert_eq!(results.len(), 12);
    for (i, r) in results.iter().enumerate() {
        if i == 7 {
            assert!(r.as_ref().unwrap_err().contains("task seven fails"), "{r:?}");
        } else {
            assert!(r.is_ok(), "task {i}: {r:?}");
        }
    }
    let tids: std::collections::HashSet<u32> = seen.lock().unwrap().iter().copied().collect();
    assert!(tids.iter().any(|t| *t != 0x4000), "no helper ran a task: {tids:?}");
    assert!(tids.iter().any(|t| (0x5000..0x5000 + 4 * workers as u32).contains(t)), "{tids:?}");
    // every task ran exactly once
    for i in 0..12u32 {
        assert_ne!(owner_mem.u32(DATA + 0x300 + 4 * i), 0, "task {i} did not run");
    }
    // the parked threads are held: their events fire, yet they do not return
    for ev in &events {
        let mut caller = GuestCaller { r: r.clone(), m: m.share(), c: Cpu::default(), top: 0x02F0_F000 };
        caller.call(S_SET, &[*ev]);
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(returned.load(Ordering::SeqCst), 0, "a held thread returned to the guest before close()");
    // a second batch reaches the held (wait-satisfied) threads too
    let ran = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let again: Vec<waits::Task> = (0..8).map(|_| {
        let ran = ran.clone();
        Box::new(move |_: &mut dyn Mem, _: &mut Cpu| { std::thread::sleep(Duration::from_millis(2)); ran.fetch_add(1, Ordering::SeqCst); Ok(()) }) as waits::Task
    }).collect();
    assert!(window.help(&mut owner_mem, &owner_cpu, again).iter().all(|r| r.is_ok()));
    assert_eq!(ran.load(Ordering::SeqCst), 8);
    assert_eq!(returned.load(Ordering::SeqCst), 0);
    drop(window);
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(returned.load(Ordering::SeqCst), workers as u32);
    waits::ENABLED.store(false, Ordering::SeqCst);
}

/// L172 P1: with no helper (nothing is blocked) the owner runs every task,
/// in order of the results.
#[test]
fn the_owner_alone_completes_the_tasks() {
    let _turn = WINDOW_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    ddnative_host::install();
    let (_rt, mut m) = sync_runtime();
    waits::ENABLED.store(true, Ordering::SeqCst);
    runtime::local(|l| l.guest_tid.set(0x4000));
    let window = waits::open(&[0x7777]).expect("window");
    let tasks: Vec<waits::Task> = (0..5u32).map(|i| Box::new(move |mem: &mut dyn Mem, _: &mut Cpu| {
        mem.set_u32(DATA + 0x340 + 4 * i, i + 1);
        Ok(())
    }) as waits::Task).collect();
    let cpu = Cpu { esp: DATA + 0x800, ..Cpu::default() };
    let results = window.help(&mut m, &cpu, tasks);
    drop(window);
    assert!(results.iter().all(|r| r.is_ok()));
    for i in 0..5u32 {
        assert_eq!(m.u32(DATA + 0x340 + 4 * i), i + 1);
    }
    waits::ENABLED.store(false, Ordering::SeqCst);
}
