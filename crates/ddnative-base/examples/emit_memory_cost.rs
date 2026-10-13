//! Alternating synthetic store128 timings, including caller/loop overhead.
//! research/native/bench_emit_memory.py selects caller opt0/opt1 with base opt3.
//! The reference is the pre-L139 loop, not a gameplay distribution or FPS test.
//! Output: alignment, pair, path, wall ns/store, thread cycles/store.
//! Windows thread cycles exclude unscheduled time; other platforms report NaN.
//! Both paths include an indirect benchmark call and caller-loop overhead.
use ddnative_base::{
    emit,
    mem::{self, Mem, PageTable, PAGE, READ, WRITE},
};
use std::{hint::black_box as bb, ptr::NonNull, sync::atomic::Ordering, time::Instant};
struct Memory(Box<PageTable>);
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(self.0.as_ref()))
    }
    fn u8(&self, a: u32) -> u8 {
        unsafe {
            mem::load_at(
                self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize),
                1,
            ) as u8
        }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        unsafe {
            mem::store_at(
                self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize),
                1,
                v as u32,
            )
        }
    }
}

#[inline(never)]
fn reference(x: [u32; 4], m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) {
    // Exact pre-L139 emit::store128 body, compiled at the caller's level.
    for k in 0..4u32 {
        m.fast_set_u32(pt, a.wrapping_add(4 * k), x[k as usize]);
    }
}
#[inline(never)]
fn candidate(x: [u32; 4], m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) {
    emit::store128(x, m, pt, a);
}
#[cfg(windows)]
fn cycles() -> Option<u64> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn QueryThreadCycleTime(thread: *mut std::ffi::c_void, cycles: *mut u64) -> i32;
    }
    let mut value = 0;
    // The pseudo-handle needs no closing; the output points to a live u64.
    assert_ne!(unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut value) }, 0);
    Some(value)
}
#[cfg(not(windows))]
fn cycles() -> Option<u64> {
    None
}
fn main() {
    let n: u32 = std::env::args()
        .nth(1)
        .unwrap_or("100000".into())
        .parse()
        .unwrap();
    assert!(n > 0);
    let mut memory = Memory(Box::default());
    for p in 0x100..0x202 {
        memory.0.backed_page(p);
        memory.0.set_perm(p, READ | WRITE);
    }
    let m: &mut dyn Mem = bb(&mut memory);
    let pt = m.page_table();
    mem::ATOMIC_ACCESSES.store(true, Ordering::Relaxed);
    for (label, offset) in [("aligned", 0), ("unaligned", 1), ("straddle", 4092)] {
        for round in 0..11 {
            for index in 0..2 {
                let use_new = (round + index) % 2 == 0;
                let f = bb(if use_new { candidate } else { reference });
                for i in 0..10000 {
                    f(bb([i, 2, 3, 4]), m, pt, 0x100000 + ((i & 255) * PAGE) + offset);
                }
                let start_cycles = cycles();
                let start = Instant::now();
                for i in 0..n {
                    f(
                        bb([i, 2, 3, 4]),
                        m,
                        pt,
                        bb(0x100000 + ((i & 255) * PAGE) + offset),
                    );
                }
                let ns = start.elapsed().as_secs_f64() * 1e9 / n as f64;
                let cycles_per_store = cycles()
                    .zip(start_cycles)
                    .map(|(end, start)| (end - start) as f64 / n as f64)
                    .unwrap_or(f64::NAN);
                println!(
                    "{label} {round} {} {:.3} {:.3}",
                    if use_new { "candidate" } else { "reference" },
                    ns,
                    cycles_per_store
                );
            }
        }
    }
}
