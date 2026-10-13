// Included in threads::tests: CPU-only follow-up regressions for L153.

fn direct_api(m: &mut GuestMemory, api: LiftFn, args: &[u32]) -> u32 {
    let mut c = Cpu { esp: DATA + 0x800, ..Cpu::default() };
    m.set_u32(c.esp, 0x12345678);
    for (i, &v) in args.iter().enumerate() { m.set_u32(c.esp + 4 + 4 * i as u32, v); }
    api(m, &mut c);
    assert_eq!(c.esp, DATA + 0x804 + 4 * args.len() as u32);
    c.eax
}

#[test]
fn tls_reallocation_thread_identity_sessions_and_last_error() {
    use crate::win32::*;
    let (a, mut m) = sync_runtime();
    let i = direct_api(&mut m, tls_alloc, &[]);
    for value in [0, 0x12345678, u32::MAX] {
        direct_api(&mut m, set_last_error, &[777]);
        assert_eq!(direct_api(&mut m, tls_set_value, &[i, value]), 1);
        assert_eq!(direct_api(&mut m, get_last_error, &[]), 777);
        assert_eq!(direct_api(&mut m, tls_get_value, &[i]), value);
        assert_eq!(direct_api(&mut m, get_last_error, &[]), 0);
    }
    let original_tid = current_tid();
    set_test_tid(original_tid + 4);
    assert_eq!(direct_api(&mut m, tls_get_value, &[i]), 0);
    direct_api(&mut m, tls_set_value, &[i, 222]);
    direct_api(&mut m, set_last_error, &[22]);
    set_test_tid(original_tid);
    assert_eq!(direct_api(&mut m, tls_get_value, &[i]), u32::MAX);
    direct_api(&mut m, set_last_error, &[11]);
    let b = Arc::new(Runtime::default());
    runtime::enter(b.clone());
    assert_eq!(direct_api(&mut m, get_last_error, &[]), 0);
    assert_eq!(direct_api(&mut m, tls_alloc, &[]), i);
    assert_eq!(direct_api(&mut m, tls_get_value, &[i]), 0);
    direct_api(&mut m, tls_set_value, &[i, 333]);
    runtime::enter(a);
    assert_eq!(direct_api(&mut m, get_last_error, &[]), 11);
    assert_eq!(direct_api(&mut m, tls_free, &[i]), 1);
    assert_eq!(direct_api(&mut m, tls_alloc, &[]), i);
    assert_eq!(direct_api(&mut m, tls_get_value, &[i]), 0);
    set_test_tid(original_tid + 4);
    assert_eq!(direct_api(&mut m, get_last_error, &[]), 22);
    assert_eq!(direct_api(&mut m, tls_get_value, &[i]), 0, "reuse clears other threads too");
    set_test_tid(original_tid);
    for index in [1088, u32::MAX] {
        assert_eq!(direct_api(&mut m, tls_get_value, &[index]), 0);
        assert_eq!(direct_api(&mut m, get_last_error, &[]), 87);
        assert_eq!(direct_api(&mut m, tls_set_value, &[index, 1]), 0);
        assert_eq!(direct_api(&mut m, get_last_error, &[]), 87);
        assert_eq!(direct_api(&mut m, tls_free, &[index]), 0);
    }
    // Host Win32 accepts every in-range index, even without allocation.
    for index in [63, 64, 1000, 1087] {
        assert_eq!(direct_api(&mut m, tls_set_value, &[index, index + 1]), 1);
        assert_eq!(direct_api(&mut m, tls_get_value, &[index]), index + 1);
    }
    for expected in 1..1088 { assert_eq!(direct_api(&mut m, tls_alloc, &[]), expected); }
    assert_eq!(direct_api(&mut m, tls_alloc, &[]), u32::MAX);
    assert_eq!(direct_api(&mut m, get_last_error, &[]), 8);
    runtime::enter(b);
    assert_eq!(direct_api(&mut m, tls_get_value, &[i]), 333);
}

#[test]
fn tls_and_semaphore_imports_complete_while_another_thread_holds_service_lock() {
    let (rt, mut m) = sync_runtime();
    let index = direct_api(&mut m, crate::win32::tls_alloc, &[]);
    let semaphore = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(0, 2)));
    let names = ["TlsGetValue", "TlsSetValue", "GetLastError", "SetLastError", "ReleaseSemaphore", "WaitForSingleObject"];
    let imports: Vec<_> = names.iter().enumerate().map(|(i, &name)| crate::image::Import {
        dll: "KERNEL32.dll".into(), name: name.into(), slot: 0, token: 0x71000000 + i as u32 * 16,
    }).collect();
    let mut registry = Registry::new();
    assert_eq!(crate::services::bind_apis(&mut registry, &imports).unwrap(), names.len());
    for import in &imports { assert!(matches!(registry.get(import.token), Some(Binding::LockFree(_)))); }
    let (send, recv) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        runtime::enter(rt);
        set_test_tid(0x7788);
        let mut c = Cpu::default();
        for (token, args, expected) in [
            (3, vec![777], None), (1, vec![index, 444], Some(1)),
            (2, vec![], Some(777)), (0, vec![index], Some(444)),
            (2, vec![], Some(0)), (4, vec![semaphore, 2, DATA], Some(1)),
            (5, vec![semaphore, 0], Some(WAIT_OBJECT_0)),
            (5, vec![semaphore, 0], Some(WAIT_OBJECT_0)),
            (5, vec![semaphore, 0], Some(WAIT_TIMEOUT)),
            (4, vec![0xdead, 1, 0], Some(0)), (2, vec![], Some(6)),
            (4, vec![0xdead, 0, 0], Some(0)), (2, vec![], Some(87)),
            (0, vec![1088], Some(0)), (2, vec![], Some(87)),
        ] {
            call_service(&registry, &mut m, &mut c, DATA + 0x800, 0x71000000 + token * 16, &args);
            if let Some(expected) = expected { assert_eq!(c.eax, expected); }
        }
        assert_eq!(m.u32(DATA), 0);
        send.send(()).unwrap();
    });
    // The creator keeps the guest service lock throughout, intentionally.
    let completed = recv.recv_timeout(Duration::from_secs(5));
    // Always release before joining, even if a regression took that lock.
    without_lock(|| worker.join().unwrap());
    completed.expect("TLS or semaphore wrongly waited for the service lock");
}

#[test]
fn parallel_tls_values_and_error_codes_are_thread_owned() {
    let (rt, mut m) = sync_runtime();
    let i = direct_api(&mut m, crate::win32::tls_alloc, &[]);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    run_guest_threads(&rt, &sync_registry(true), &m, 8, Arc::new(move |k, _g| {
        let mut private = GuestMemory::new();
        private.map(DATA, 0x1000, READ | WRITE).unwrap();
        for n in 0..1000 {
            let value = ((k as u32 + 1) << 24) | n;
            assert_eq!(direct_api(&mut private, crate::win32::tls_set_value, &[i, value]), 1);
            runtime::set_last_error(value);
            barrier.wait();
            assert_eq!(runtime::last_error(), value);
            assert_eq!(direct_api(&mut private, crate::win32::tls_get_value, &[i]), value);
            assert_eq!(runtime::last_error(), 0);
            barrier.wait();
        }
    }));
}

#[test]
fn tls_free_and_reallocation_reset_all_running_threads() {
    let (rt, mut m) = sync_runtime();
    let index = direct_api(&mut m, crate::win32::tls_alloc, &[]);
    let barrier = Arc::new(std::sync::Barrier::new(4));
    run_guest_threads(&rt, &sync_registry(true), &m, 4, Arc::new(move |k, _g| {
        let mut private = GuestMemory::new();
        private.map(DATA, 0x1000, READ | WRITE).unwrap();
        for round in 0..200 {
            assert_eq!(direct_api(&mut private, crate::win32::tls_get_value, &[index]), 0);
            assert_eq!(direct_api(&mut private, crate::win32::tls_set_value, &[index, round + k as u32 + 1]), 1);
            barrier.wait();
            if k == 0 {
                assert_eq!(direct_api(&mut private, crate::win32::tls_free, &[index]), 1);
                assert_eq!(direct_api(&mut private, crate::win32::tls_alloc, &[]), index);
            }
            barrier.wait();
        }
    }));
}

#[test]
fn semaphore_single_wait_ping_pong_and_deadlines_lose_no_wakes() {
    let (rt, m) = sync_runtime();
    let pairs: Vec<_> = (0..2).map(|_| (
        create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(1, 1))),
        create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(0, 1))),
    )).collect();
    run_guest_threads(&rt, &sync_registry(true), &m, 4, Arc::new(move |k, g| {
        let (a, b) = pairs[k / 2];
        let (take, give) = if k % 2 == 0 { (a, b) } else { (b, a) };
        for _ in 0..2000 {
            assert_eq!(g.call(S_WAIT1, &[take, 5000]), WAIT_OBJECT_0);
            assert_eq!(g.call(S_RELEASE_SEM, &[give, 1, 0]), 1);
        }
    }));
    let empty = SemaphoreCell::new(0, 1);
    assert_eq!(wait_semaphore(0x7777, &empty, 2), WAIT_TIMEOUT);
}

#[test]
fn semaphore_alias_limits_close_and_competing_wait_all_are_exact() {
    let (rt, mut m) = sync_runtime();
    m.set_u8(DATA, b's'); m.set_u8(DATA + 1, 0);
    let first = create_named(&m, DATA, || Object::Semaphore(SemaphoreCell::new(1, 2)));
    let alias = create_named(&m, DATA, || panic!("named object already exists"));
    assert!(Arc::ptr_eq(&semaphore_of(first).unwrap().1, &semaphore_of(alias).unwrap().1));
    for count in [0, 2, u32::MAX] {
        m.set_u32(DATA + 16, 0xabcdef01);
        assert_eq!(direct_api(&mut m, release_semaphore, &[alias, count, DATA + 16]), 0);
        assert_eq!(runtime::last_error(), if (count as i32) <= 0 { 87 } else { ERROR_TOO_MANY_POSTS });
        assert_eq!(m.u32(DATA + 16), 0xabcdef01);
    }
    assert_eq!(direct_api(&mut m, release_semaphore, &[alias, 1, DATA + 16]), 1);
    assert_eq!(m.u32(DATA + 16), 1);
    assert!(close(first)); assert!(semaphore_of(first).is_none());
    assert_eq!(direct_api(&mut m, wait_for_single_object, &[alias, 0]), WAIT_OBJECT_0);
    assert_eq!(direct_api(&mut m, wait_for_single_object, &[alias, 0]), WAIT_OBJECT_0);
    let a = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(4000, 4000)));
    let b = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(4000, 4000)));
    m.set_u32(DATA + 0x200, a); m.set_u32(DATA + 0x204, b);
    let counts = Arc::new([std::sync::atomic::AtomicU32::new(0), std::sync::atomic::AtomicU32::new(0)]);
    let count = counts.clone();
    run_guest_threads(&rt, &sync_registry(true), &m, 4, Arc::new(move |k, g| {
        loop {
            let result = if k < 2 { g.call(S_WAITN, &[2, DATA + 0x200, 1, 0]) }
                         else { g.call(S_WAIT1, &[if k == 2 { a } else { b }, 0]) };
            if result == WAIT_TIMEOUT { break; }
            assert_eq!(result, WAIT_OBJECT_0);
            if k != 3 { count[0].fetch_add(1, SeqCst); }
            if k != 2 { count[1].fetch_add(1, SeqCst); }
        }
    }));
    assert_eq!(counts[0].load(SeqCst), 4000);
    assert_eq!(counts[1].load(SeqCst), 4000);
}

fn abandonment_worker(m: &mut dyn Mem, cpu: &mut Cpu) {
    let mutex = arg(m, cpu, 0);
    { let _guard = service();
      assert_eq!(wait(&[mutex], false, 0), WAIT_OBJECT_0);
      assert_eq!(wait(&[mutex], false, 0), WAIT_OBJECT_0); }
    cpu.eax = 123;
    ret(m, cpu, 4);
}

#[test]
fn actual_guest_thread_exit_abandons_recursive_mutex_and_wakes_waiter() {
    for parallel in [false, true] {
        let (_, mut m) = sync_runtime();
        set_parallel(parallel);
        m.map_lazy(STACKS_BASE, STACKS_SIZE, READ | WRITE).unwrap();
        let mut registry = Registry::new();
        registry.insert(0x71000100, Binding::Lifted(abandonment_worker)).unwrap();
        set_machine(m.share(), Arc::new(registry));
        let mutex = direct_api(&mut m, create_mutex, &[0, 0, 0]);
        let thread = direct_api(&mut m, create_thread, &[0, 0, 0x71000100, mutex, 0, 0]);
        assert_eq!(wait(&[thread], false, 5000), WAIT_OBJECT_0);
        assert_eq!(wait(&[mutex], false, 0), WAIT_ABANDONED_0);
        assert_eq!(direct_api(&mut m, release_mutex, &[mutex]), 1, "abandonment resets recursion");
        assert_eq!(direct_api(&mut m, release_mutex, &[mutex]), 0);
        assert_eq!(runtime::last_error(), ERROR_NOT_OWNER);
        assert!(faults().is_empty());
    }
}

#[cfg(windows)]
#[test]
fn native_win32_abandonment_and_tls_oracle() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(a: *const (), own: i32, name: *const u16) -> usize;
        fn CreateEventW(a: *const (), manual: i32, set: i32, name: *const u16) -> usize;
        fn WaitForSingleObject(h: usize, ms: u32) -> u32;
        fn WaitForMultipleObjects(n: u32, h: *const usize, all: i32, ms: u32) -> u32;
        fn ReleaseMutex(h: usize) -> i32;
        fn CloseHandle(h: isize) -> i32;
        fn TlsAlloc() -> u32;
        fn TlsFree(i: u32) -> i32;
        fn TlsGetValue(i: u32) -> usize;
        fn TlsSetValue(i: u32, v: usize) -> i32;
        fn GetLastError() -> u32;
        fn SetLastError(e: u32);
        fn CreateSemaphoreW(a: *const (), initial: i32, max: i32, name: *const u16) -> usize;
        fn ReleaseSemaphore(h: usize, n: i32, previous: *mut i32) -> i32;
    }
    let (_, mut m) = sync_runtime();
    // SAFETY: handles are created here, the owner is joined before waiting,
    // every wait is bounded, and every handle/index is released in this test.
    unsafe {
        for all in [false, true] {
            let mutex = std::thread::spawn(|| CreateMutexW(std::ptr::null(), 1, std::ptr::null())).join().unwrap();
            assert_ne!(mutex, 0);
            let event = CreateEventW(std::ptr::null(), 1, i32::from(all), std::ptr::null());
            let native = WaitForMultipleObjects(2, [event, mutex].as_ptr(), i32::from(all), 100);
            let guest_event = create_named(&m, 0, || Object::Event(EventCell::new(true, all)));
            let guest_mutex = create_named(&m, 0, || Object::Mutex { owner: 0x8888, recursion: 2, abandoned: false });
            kernel(|k| abandon_mutexes(k, 0x8888));
            assert_eq!(wait(&[guest_event, guest_mutex], all, 0), native);
            assert_eq!(wait(&[guest_mutex], false, 0), WaitForSingleObject(mutex, 0));
            assert_ne!(ReleaseMutex(mutex), 0); assert_ne!(ReleaseMutex(mutex), 0);
            CloseHandle(mutex as isize); CloseHandle(event as isize);
        }
        let host = TlsAlloc();
        let guest = direct_api(&mut m, crate::win32::tls_alloc, &[]);
        assert_ne!(host, u32::MAX);
        for value in [0, 0x12345678, u32::MAX] {
            SetLastError(777); runtime::set_last_error(777);
            assert_eq!(direct_api(&mut m, crate::win32::tls_set_value, &[guest, value]), TlsSetValue(host, value as usize) as u32);
            assert_eq!(runtime::last_error(), GetLastError());
            assert_eq!(direct_api(&mut m, crate::win32::tls_get_value, &[guest]), TlsGetValue(host) as u32);
            assert_eq!(runtime::last_error(), GetLastError());
        }
        assert_ne!(TlsFree(host), 0);
        assert_eq!(direct_api(&mut m, crate::win32::tls_free, &[guest]), 1);
        assert_eq!(direct_api(&mut m, crate::win32::tls_get_value, &[guest]), TlsGetValue(host) as u32);
        assert_eq!(runtime::last_error(), GetLastError());
        for index in [1088, u32::MAX] {
            assert_eq!(direct_api(&mut m, crate::win32::tls_get_value, &[index]), TlsGetValue(index) as u32);
            assert_eq!(runtime::last_error(), GetLastError());
        }
        let host_sem = CreateSemaphoreW(std::ptr::null(), 1, 2, std::ptr::null());
        assert_ne!(host_sem, 0);
        let guest_sem = create_named(&m, 0, || Object::Semaphore(SemaphoreCell::new(1, 2)));
        for valid in [true, false] {
            for count in [0, u32::MAX, 2, 1] {
                let mut previous = 777;
                SetLastError(555);
                let result = ReleaseSemaphore(if valid { host_sem } else { 0x1234 }, count as i32, &mut previous);
                let error = GetLastError();
                runtime::set_last_error(555);
                m.set_u32(DATA + 16, 777);
                let guest_result = direct_api(&mut m, release_semaphore, &[if valid { guest_sem } else { 0x1234 }, count, DATA + 16]);
                assert_eq!((guest_result, runtime::last_error(), m.u32(DATA + 16)), (result as u32, error, previous as u32));
            }
        }
        CloseHandle(host_sem as isize);
    }
}
