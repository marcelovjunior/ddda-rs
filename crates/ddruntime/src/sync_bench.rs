// Included in threads::tests. Uses APIs available on both sides of L153p2
// so the identical benchmark can be compiled against the parent revision.
#[test]
#[ignore = "paired CPU microbenchmark; explicit play-profile run only"]
fn sync_thread_access_benchmark() {
    let (rt, mut m) = sync_runtime();
    let mut c = Cpu { esp: DATA + 0x800, ..Cpu::default() };
    m.set_u32(c.esp, 0x1234);
    crate::win32::tls_alloc(&mut m, &mut c);
    let index = c.eax;
    let imports: Vec<_> = ["TlsGetValue", "TlsSetValue", "ReleaseSemaphore", "WaitForSingleObject"]
        .iter().enumerate().map(|(i, &name)| crate::image::Import {
            dll: "KERNEL32.dll".into(), name: name.into(), slot: 0, token: 0x72000000 + i as u32 * 16,
        }).collect();
    let mut registry = Registry::new();
    crate::services::bind_apis(&mut registry, &imports).unwrap();
    let registry = Arc::new(registry);
    // Create via API, keeping this source identical to the baseline.
    c.esp = DATA + 0x800;
    for (j, v) in [0x1234, 0, 0, 1, 0].into_iter().enumerate() { m.set_u32(c.esp + j as u32 * 4, v); }
    create_semaphore(&mut m, &mut c);
    let semaphore = c.eax;
    let n = 50_000u32;
    for round in 0..5 {
        let t = Instant::now();
        for _ in 0..n * 10 {
            let pin = runtime::pin();
            std::hint::black_box(&*pin);
            let _scope = ServiceScope::enter();
            std::hint::black_box(crate::d3d9dev::holds_device_lock());
        }
        eprintln!("L153P2 local round={round} ns={:.3}", t.elapsed().as_nanos() as f64 / (n * 10) as f64);
        let t = Instant::now();
        for _ in 0..n * 10 {
            let guard = crate::d3d9dev::device_lock();
            std::hint::black_box(&guard);
        }
        eprintln!("L153P2 device round={round} ns={:.3}", t.elapsed().as_nanos() as f64 / (n * 10) as f64);
        for threads in [1, 4] {
            let elapsed = run_guest_threads(&rt, &registry, &m, threads, Arc::new(move |k, g| {
                for v in 0..n {
                    assert_eq!(g.call(0x72000010, &[index, v + k as u32]), 1);
                    assert_eq!(g.call(0x72000000, &[index]), v + k as u32);
                }
            }));
            eprintln!("L153P2 tls round={round} threads={threads} ns_pair={:.3}", elapsed.as_nanos() as f64 / (n as usize * threads) as f64);
        }
        let elapsed = run_guest_threads(&rt, &registry, &m, 1, Arc::new(move |_, g| {
            for _ in 0..n {
                assert_eq!(g.call(0x72000020, &[semaphore, 1, 0]), 1);
                assert_eq!(g.call(0x72000030, &[semaphore, 0]), WAIT_OBJECT_0);
            }
        }));
        eprintln!("L153P2 semaphore round={round} ns_pair={:.3}", elapsed.as_nanos() as f64 / n as f64);
    }
}
