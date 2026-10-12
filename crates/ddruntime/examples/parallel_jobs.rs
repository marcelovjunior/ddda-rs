//! A synthetic job workload for the parallel guest threads (brief L42),
//! serial vs parallel mode. Light: no lifted program needed.
//!
//!     cargo run --release -p ddruntime --no-default-features --example parallel_jobs [serial|parallel]...
//!
//! Each "frame", the main guest thread hands out `JOBS` jobs to `WORKERS`
//! worker threads, like sMain's job threads: a worker waits on a semaphore,
//! runs a job of translated-code work (guest memory reads and writes in its
//! own area, `WORK` accesses), counts it with InterlockedIncrement and
//! signals the frame's event when the last job of the frame is done.
use ddnative_base::{mem::Mem, x86, x86::Cpu};
use ddruntime::{
    dispatch::{Binding, Registry},
    memory::{GuestMemory, READ, WRITE},
    threads, win32,
};
use std::sync::Arc;
use std::time::Instant;

const DATA: u32 = 0x0030_0000;
const DONE: u32 = DATA + 0x100; // jobs done in this frame
const SEM: u32 = DATA + 0x104; // job semaphore handle
const EVENT: u32 = DATA + 0x108; // frame-done event handle
const QUIT: u32 = DATA + 0x10C;
const HANDLES: u32 = DATA + 0x200;
const AREA: u32 = 0x0100_0000; // 64 KiB per worker

const WORKER: u32 = 0x0060_0000;
const S_INC: u32 = 0x0070_0000;
const S_CREATE_THREAD: u32 = 0x0070_0010;
const S_WAIT: u32 = 0x0070_0020;
const S_WAIT_ALL: u32 = 0x0070_0030;
const S_RELEASE_SEM: u32 = 0x0070_0040;
const S_SET_EVENT: u32 = 0x0070_0050;
const S_CREATE_SEM: u32 = 0x0070_0060;
const S_CREATE_EVENT: u32 = 0x0070_0070;

const WORKERS: u32 = 7;
const JOBS: u32 = 64;
const FRAMES: u32 = 60;
const WORK: u32 = 40_000;

fn guest_call(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, args: &[u32]) -> u32 {
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0xFFFF_FFF0);
    x86::call_ext(m, cpu, target);
    cpu.eax
}

/// One job: translated-code style work on the worker's own memory.
fn job(m: &mut dyn Mem, base: u32) {
    let mut x = 0u32;
    for k in 0..WORK {
        let a = base + 4 * (k.wrapping_mul(2654435761) % 4096);
        x = x.wrapping_add(m.u32(a)).rotate_left(3) ^ k;
        m.set_u32(a, x);
    }
}

fn worker(m: &mut dyn Mem, cpu: &mut Cpu) {
    let index = m.u32(cpu.esp + 4);
    let base = AREA + 0x1_0000 * index;
    loop {
        let sem = m.u32(SEM);
        guest_call(m, cpu, S_WAIT, &[sem, threads::INFINITE]);
        if m.u32(QUIT) != 0 {
            break;
        }
        job(m, base);
        if guest_call(m, cpu, S_INC, &[DONE]) == JOBS {
            let ev = m.u32(EVENT);
            guest_call(m, cpu, S_SET_EVENT, &[ev]);
        }
    }
    cpu.eax = 0;
    let _ = m.u32(cpu.esp);
    cpu.esp += 8;
}

fn run(parallel: bool) -> f64 {
    ddruntime::runtime::enter(Arc::new(ddruntime::runtime::Runtime::default()));
    threads::set_parallel(parallel);
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    m.map(DATA, 0x1000, READ | WRITE).unwrap();
    m.map(AREA, 0x1_0000 * WORKERS, READ | WRITE).unwrap();
    m.map_lazy(threads::STACKS_BASE, threads::STACKS_SIZE, READ | WRITE)
        .unwrap();
    let mut r = Registry::new();
    for (a, b) in [
        (WORKER, Binding::Lifted(worker)),
        (S_INC, Binding::Service(win32::interlocked_increment)),
        (S_CREATE_THREAD, Binding::Service(threads::create_thread)),
        (S_WAIT, Binding::Service(threads::wait_for_single_object)),
        (S_WAIT_ALL, Binding::Service(threads::wait_for_multiple_objects)),
        (S_RELEASE_SEM, Binding::Service(threads::release_semaphore)),
        (S_SET_EVENT, Binding::Service(threads::set_event)),
        (S_CREATE_SEM, Binding::Service(threads::create_semaphore)),
        (S_CREATE_EVENT, Binding::Service(threads::create_event)),
    ] {
        r.insert(a, b).unwrap();
    }
    let r = Arc::new(r);
    r.install();
    threads::set_machine(m.share(), r);
    let mut cpu = Cpu {
        esp: 0x0011_0000,
        ..Cpu::default()
    };
    let c = &mut cpu;
    let sem = guest_call(&mut m, c, S_CREATE_SEM, &[0, 0, JOBS * 2, 0]);
    m.set_u32(SEM, sem);
    let ev = guest_call(&mut m, c, S_CREATE_EVENT, &[0, 0, 0, 0]);
    m.set_u32(EVENT, ev);
    for k in 0..WORKERS {
        let h = guest_call(&mut m, c, S_CREATE_THREAD, &[0, 0, WORKER, k, 0, 0]);
        m.set_u32(HANDLES + 4 * k, h);
    }
    let t = Instant::now();
    for _ in 0..FRAMES {
        m.set_u32(DONE, 0);
        guest_call(&mut m, c, S_RELEASE_SEM, &[sem, JOBS, 0]);
        guest_call(&mut m, c, S_WAIT, &[ev, threads::INFINITE]);
    }
    let elapsed = t.elapsed().as_secs_f64();
    m.set_u32(QUIT, 1);
    guest_call(&mut m, c, S_RELEASE_SEM, &[sem, WORKERS, 0]);
    guest_call(&mut m, c, S_WAIT_ALL, &[WORKERS, HANDLES, 1, threads::INFINITE]);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
    x86::set_ext_calls(None);
    elapsed
}

fn main() {
    let host = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!(
        "{WORKERS} workers, {JOBS} jobs/frame of {WORK} guest accesses, {FRAMES} frames, {host} host threads"
    );
    // modes from the arguments ("serial", "parallel"); once a parallel run
    // has happened, guest memory accesses stay atomic for the process
    let modes: Vec<bool> = std::env::args().skip(1).map(|a| a == "parallel").collect();
    let modes = if modes.is_empty() {
        vec![false, false, true, true]
    } else {
        modes
    };
    for parallel in modes {
        let s = run(parallel);
        println!(
            "{}: {:.3} s, {:.2} ms/frame",
            if parallel { "parallel" } else { "serial  " },
            s,
            s * 1000.0 / FRAMES as f64
        );
    }
}
