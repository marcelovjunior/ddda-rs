// Included in threads::tests (L185): the latency of waking a blocked waiter
// through the runtime's emulated events against raw Windows events.
//
//   cargo test --release -p ddruntime --no-default-features --lib sync_latency -- --ignored --nocapture
//
// Two shapes: a ping-pong between two threads that hand an event back and
// forth (a hot hand-off, the waiter is still spinning), and the job system's
// fork-join: one thread sets the start events of 6 parked workers, each of
// which sets its own done event, and waits (WaitForMultipleObjects, all) for
// them; the workers sleep 200 us between joins, as the game's do.

#[cfg(windows)]
mod raw_events {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateEventW(attrs: *const c_void, manual: i32, initial: i32, name: *const u16) -> *mut c_void;
        pub fn SetEvent(h: *mut c_void) -> i32;
        pub fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
        pub fn WaitForMultipleObjects(n: u32, h: *const *mut c_void, all: i32, ms: u32) -> u32;
    }
    #[derive(Clone, Copy)]
    pub struct Handle(pub *mut c_void);
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}
    impl Handle {
        pub fn get(self) -> *mut c_void { self.0 }
    }
    pub fn event() -> Handle {
        Handle(unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) })
    }
}

fn busy_wait(d: Duration) {
    let t = Instant::now();
    while t.elapsed() < d {
        std::hint::spin_loop();
    }
}

fn percentiles(mut v: Vec<f64>) -> (f64, f64, f64) {
    eprintln!("  mean {:.1} us over {} joins", v.iter().sum::<f64>() / v.len() as f64, v.len());
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (v[v.len() / 2], v[v.len() * 9 / 10], v[v.len() * 99 / 100])
}

#[cfg(windows)]
#[test]
#[ignore = "latency microbenchmark"]
fn sync_latency_raw_windows_events() {
    use raw_events::*;
    let rounds = 20_000;
    let (a, b) = (event(), event());
    let t = std::thread::spawn(move || {
        for _ in 0..rounds {
            unsafe {
                WaitForSingleObject(a.get(), u32::MAX);
                SetEvent(b.get());
            }
        }
    });
    let start = Instant::now();
    for _ in 0..rounds {
        unsafe {
            SetEvent(a.get());
            WaitForSingleObject(b.get(), u32::MAX);
        }
    }
    t.join().unwrap();
    eprintln!("raw Windows events: ping-pong {:.2} us per round trip", start.elapsed().as_secs_f64() * 1e6 / rounds as f64);

    // fork-join over 6 parked workers
    let joins = 3000;
    let starts: Vec<Handle> = (0..6).map(|_| event()).collect();
    let dones: Vec<Handle> = (0..6).map(|_| event()).collect();
    let workers: Vec<_> = (0..6).map(|k| {
        let (s, d) = (starts[k], dones[k]);
        std::thread::spawn(move || {
            for _ in 0..joins {
                unsafe {
                    WaitForSingleObject(s.get(), u32::MAX);
                    SetEvent(d.get());
                }
            }
        })
    }).collect();
    let done_handles: Vec<*mut std::ffi::c_void> = dones.iter().map(|h| h.get()).collect();
    let mut samples = Vec::new();
    for _ in 0..joins {
        busy_wait(Duration::from_micros(200));
        let t = Instant::now();
        for s in &starts {
            unsafe { SetEvent(s.get()) };
        }
        unsafe { WaitForMultipleObjects(6, done_handles.as_ptr(), 1, u32::MAX) };
        samples.push(t.elapsed().as_secs_f64() * 1e6);
    }
    for w in workers {
        w.join().unwrap();
    }
    let (p50, p90, p99) = percentiles(samples);
    eprintln!("raw Windows events: fork-join of 6 parked workers p50 {p50:.1} us, p90 {p90:.1} us, p99 {p99:.1} us");
}

#[test]
#[ignore = "latency microbenchmark"]
fn sync_latency_emulated_events() {
    let (rt, m) = sync_runtime();
    let r = sync_registry(true);
    // ping-pong
    let rounds = 20_000u32;
    let (ping, pong) = (
        create_named(&m, 0, || Object::Event(EventCell::new(false, false))),
        create_named(&m, 0, || Object::Event(EventCell::new(false, false))),
    );
    let t = run_guest_threads(&rt, &r, &m, 2, Arc::new(move |k, g: &mut GuestCaller| {
        for _ in 0..rounds {
            if k == 0 {
                g.call(S_SET, &[ping]);
                g.call(S_WAIT1, &[pong, 10_000]);
            } else {
                g.call(S_WAIT1, &[ping, 10_000]);
                g.call(S_SET, &[pong]);
            }
        }
    }));
    eprintln!("emulated events: ping-pong {:.2} us per round trip", t.as_secs_f64() * 1e6 / rounds as f64);

    // fork-join over 6 parked workers (thread 0 is the forking thread)
    let joins = 3000u32;
    let starts: Vec<u32> = (0..6).map(|_| create_named(&m, 0, || Object::Event(EventCell::new(false, false)))).collect();
    let dones: Vec<u32> = (0..6).map(|_| create_named(&m, 0, || Object::Event(EventCell::new(false, false)))).collect();
    let mut mm = m.share();
    for (i, d) in dones.iter().enumerate() {
        mm.set_u32(DATA + 0x200 + 4 * i as u32, *d);
    }
    let samples = Arc::new(Mutex::new(Vec::new()));
    let s2 = samples.clone();
    let (st, dn) = (starts.clone(), dones.clone());
    run_guest_threads(&rt, &r, &m, 7, Arc::new(move |k, g: &mut GuestCaller| {
        if k == 0 {
            let mut local = Vec::new();
            for _ in 0..joins {
                busy_wait(Duration::from_micros(200));
                let t = Instant::now();
                for s in &st {
                    g.call(S_SET, &[*s]);
                }
                g.call(S_WAITN, &[6, DATA + 0x200, 1, 10_000]);
                local.push(t.elapsed().as_secs_f64() * 1e6);
            }
            *s2.lock().unwrap() = local;
        } else {
            for _ in 0..joins {
                g.call(S_WAIT1, &[st[k - 1], 10_000]);
                g.call(S_SET, &[dn[k - 1]]);
            }
        }
    }));
    let (p50, p90, p99) = percentiles(samples.lock().unwrap().clone());
    eprintln!("emulated events: fork-join of 6 parked workers p50 {p50:.1} us, p90 {p90:.1} us, p99 {p99:.1} us");
}
