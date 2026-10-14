// L172 P3 (s4): the stress harness pointed at P1's real helper interface
// (`waits::help`, `Blocked::pass`, `Task`). Included into `threads::tests`
// after window_stress.rs.

/// The traversal of a window whose units run through `waits::help`: random
/// tasks, one of them failing sometimes; every task must see the redirect on
/// whatever thread runs it, start with an empty x87 stack, run exactly once
/// and leave the thread's FPU environment as the thread had it.
fn help_traversal(counted: Arc<AtomicU32>, extra_failures: Arc<AtomicU32>) -> Arc<Traversal> {
    Arc::new(move |g: &mut GuestCaller, stats: &Arc<WindowStats>, window: &OpenWindow, rng: &mut u64| {
        let n = 1 + wnext(rng) as usize % 9;
        let fail_at = (wnext(rng) % 5 == 0).then(|| wnext(rng) as usize % n);
        let mut tasks: Vec<waits::Task> = Vec::new();
        for i in 0..n {
            let (stats, counted) = (stats.clone(), counted.clone());
            tasks.push(Box::new(move |mem, cpu| {
                counted.fetch_add(1, SeqCst);
                stats.helper_units.fetch_add(1, SeqCst);
                if mem.u32(WG_GLOBAL) != WG_PRIVATE {
                    stats.helper_off_window.fetch_add(1, SeqCst);
                }
                if !stats.in_window.load(SeqCst) {
                    stats.helper_off_window.fetch_add(1, SeqCst);
                }
                assert_eq!(cpu.st.depth(), 0, "a task starts with an empty x87 stack");
                if current_tid() != stats.window_tid.load(SeqCst) {
                    stats.helper_foreign.fetch_add(1, SeqCst);
                }
                // leave the FPU as a careless guest unit would
                let mut operand = [0u8; 10];
                operand[..4].copy_from_slice(&1.5f32.to_le_bytes());
                cpu.st.exec::<0x00d9>(&mut operand);
                ddnative_base::x86::ldmxcsr(0x1fa0);
                std::thread::sleep(Duration::from_micros(200));
                if Some(i) == fail_at {
                    panic!("task {i} fails");
                }
                Ok(())
            }));
        }
        let owner_cpu = Cpu { esp: g.top, ..Cpu::default() };
        let results = window.0.help(&mut g.m, &owner_cpu, tasks);
        assert_eq!(results.len(), n);
        for (i, r) in results.iter().enumerate() {
            match (r, Some(i) == fail_at) {
                (Err(e), true) => assert!(e.contains("fails"), "{e}"),
                (Ok(()), false) => {}
                (other, expected_failure) => {
                    extra_failures.fetch_add(1, SeqCst);
                    eprintln!("task {i}: {other:?} (expected failure: {expected_failure})");
                }
            }
        }
    })
}

#[test]
fn real_helpers_run_every_unit_once_inside_the_window_on_parked_threads() {
    ddnative_host::install();
    let (counted, bad) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
    let stats = run_window_scenario(6, 300, Some(help_traversal(counted.clone(), bad.clone())), false, 0xabcd_ef01);
    eprintln!("windows {} units {} on other threads {}", stats.windows.load(SeqCst), counted.load(SeqCst), stats.helper_foreign.load(SeqCst));
    check_window_stats(&stats, 300);
    assert_eq!(bad.load(SeqCst), 0, "a task result was wrong");
    assert_eq!(counted.load(SeqCst), stats.helper_units.load(SeqCst));
    assert!(stats.helper_units.load(SeqCst) > 0);
    assert!(stats.helper_foreign.load(SeqCst) > 0, "no unit ever ran on a parked helper thread");
}

#[test]
fn real_helpers_with_few_and_many_threads() {
    ddnative_host::install();
    for (workers, seed) in [(2, 5u64), (9, 0x77), (13, 0x1234_9876)] {
        let (counted, bad) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
        let stats = run_window_scenario(workers, 120, Some(help_traversal(counted, bad.clone())), false, seed);
        check_window_stats(&stats, 120);
        assert_eq!(bad.load(SeqCst), 0);
    }
}
