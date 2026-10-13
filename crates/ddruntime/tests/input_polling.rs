//! L82 input convoy fixture: genuine guest worker/service dispatch, no game/GPU.
use ddnative_base::{mem::Mem, x86, x86::Cpu};
use ddruntime::{dispatch::{Binding, Registry}, input, memory::{GuestMemory, READ, WRITE}, runtime, services, threads, user32};
use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
use std::time::{Duration, Instant};

const CREATE: u32 = 0x7000_0100;
const WAIT: u32 = 0x7000_0110;
const WORKER: u32 = 0x7000_0120;
const BUSY: u32 = 0x7000_0130;
const BEFORE: u32 = 0x7000_0140;
const AFTER: u32 = 0x7000_0150;
const QUEUE: u32 = 0x7000_0160;
const PEEK: u32 = 0x7000_0170;
static REQUEST: AtomicU32 = AtomicU32::new(0);
static HELD: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicU32 = AtomicU32::new(0);

fn call(m: &mut dyn Mem, c: &mut Cpu, target: u32, args: &[u32]) -> u32 {
    let top = c.esp;
    for &a in args.iter().rev() { c.esp -= 4; m.set_u32(c.esp, a); }
    c.esp -= 4;
    m.set_u32(c.esp, 0xffff_fff0);
    x86::call_ext(m, c, target);
    assert_eq!(c.esp, top);
    c.eax
}

fn wait_for(value: &AtomicU32, target: u32) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while value.load(Ordering::Acquire) < target {
        assert!(Instant::now() < deadline, "worker handshake timed out");
        std::thread::yield_now();
    }
}

fn busy(_m: &mut dyn Mem, c: &mut Cpu) {
    assert!(threads::holds_guest_lock());
    HELD.store(REQUEST.load(Ordering::Acquire), Ordering::Release);
    std::thread::sleep(Duration::from_millis(4)); // synthetic host work
    c.esp += 4;
}

fn worker(m: &mut dyn Mem, c: &mut Cpu) {
    // The main thread removed KEYDOWN, but this thread's queue did not.
    assert_eq!(call(m, c, QUEUE, &[0x41]), 0);
    for frame in 1..=64 {
        wait_for(&REQUEST, frame);
        call(m, c, BUSY, &[]);
        DONE.store(frame, Ordering::Release);
    }
    c.eax = 0;
    c.esp += 8;
}

fn key(down: bool) -> input::Event {
    input::Event::Key { hwnd: 0, vk: 0x41, scan: 30, down, extended: false, system: false }
}

fn acquisitions(report: &str, service: &str) -> u64 {
    let mut lines = report.lines();
    while let Some(line) = lines.next() {
        if line.ends_with(service) && !line.contains("blocks tid=") {
            return lines.next().unwrap().trim().strip_prefix("acquisitions=").unwrap()
                .split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("missing {service}: {report}");
}

#[test]
fn idle_input_polls_do_not_wait_for_a_busy_guest_service() {
    std::env::set_var("DDRUNTIME_LOCK_STATS", "1");
    runtime::enter(Arc::new(runtime::Runtime::default()));
    threads::set_parallel(true);
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x20000, READ | WRITE).unwrap();
    m.map_lazy(threads::STACKS_BASE, threads::STACKS_SIZE, READ | WRITE).unwrap();
    let mut r = Registry::new();
    services::install(&mut m, &mut r).unwrap();
    for (target, binding) in [
        (CREATE, Binding::Service(threads::create_thread)),
        (WAIT, Binding::Service(threads::wait_for_single_object)),
        (WORKER, Binding::Lifted(worker)),
        (BUSY, Binding::Service(busy)),
        (BEFORE, Binding::Service(input::get_async_key_state)),
        (PEEK, Binding::Service(user32::peek_message)),
    ] { r.insert(target, binding).unwrap(); }
    let imports = [(AFTER, "GetAsyncKeyState"), (QUEUE, "GetKeyState")].map(|(token, name)| ddruntime::image::Import {
        dll: "USER32.dll".into(), name: name.into(), slot: 0, token,
    });
    assert_eq!(services::bind_apis(&mut r, &imports).unwrap(), 2);
    assert!(matches!(r.get(AFTER), Some(Binding::LockFree(_))));
    assert!(matches!(r.get(QUEUE), Some(Binding::LockFree(_))));
    r.name(BEFORE, "L82::LockedPoll");
    r.name(BUSY, "L82::InputBusyWorker");
    let r = Arc::new(r);
    r.install();
    threads::set_machine(m.share(), r);
    let host = input::connect();
    host.events.send(input::Event::Devices { keyboard: true, mouse: true }).unwrap();
    host.events.send(key(true)).unwrap();
    let mut c = Cpu { esp: 0x2F000, ..Cpu::default() };
    assert_eq!(call(&mut m, &mut c, AFTER, &[0x41]), 0x8001);
    assert_eq!(call(&mut m, &mut c, QUEUE, &[0x41]), 0);
    assert_eq!(call(&mut m, &mut c, PEEK, &[0x10000, 0, 0, 0, 1]), 1);
    assert_eq!(call(&mut m, &mut c, QUEUE, &[0x41]), 0x8000);
    let mut elapsed = [Duration::ZERO; 2];
    threads::without_lock(|| {
        let handle = call(&mut m, &mut c, CREATE, &[0, 0, WORKER, 0, 0, 0]);
        assert_ne!(handle, 0);
        for frame in 1..=64 {
            REQUEST.store(frame, Ordering::Release);
            wait_for(&HELD, frame);
            let after = frame > 32;
            let start = Instant::now();
            assert_eq!(call(&mut m, &mut c, if after { AFTER } else { BEFORE }, &[0x41]), 0x8000);
            elapsed[after as usize] += start.elapsed();
            if after { assert_eq!(call(&mut m, &mut c, QUEUE, &[0x41]), 0x8000); }
            wait_for(&DONE, frame);
        }
        assert_eq!(call(&mut m, &mut c, WAIT, &[handle, 8000]), threads::WAIT_OBJECT_0);
    });
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
    let report = threads::lock_stats().unwrap();
    assert_eq!(acquisitions(&report, "L82::LockedPoll"), 32);
    assert_eq!(acquisitions(&report, "USER32.dll!GetAsyncKeyState"), 0);
    eprintln!("L82 input 32 polls each, 4ms busy worker: before {:?}/call, after {:?}/call\n{report}", elapsed[0] / 32, elapsed[1] / 32);
    host.events.send(key(false)).unwrap();
    threads::without_lock(|| assert_eq!(call(&mut m, &mut c, AFTER, &[0x41]), 0));
    assert_eq!(call(&mut m, &mut c, QUEUE, &[0x41]), 0x8000, "queue state waits for removal");
    assert_eq!(call(&mut m, &mut c, PEEK, &[0x10000, 0, 0, 0, 1]), 1);
    assert_eq!(call(&mut m, &mut c, QUEUE, &[0x41]), 0);
    x86::set_ext_calls(None);
}
