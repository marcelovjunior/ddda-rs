//! L146 synthetic FLD/FSTP pairs; reference is the generic byte-buffer path.
//! Caller opt0/opt1, optimized base/host, dynamic Mem and alternating pairs.
use ddnative_base::mem::{self, Mem, PageTable, PAGE, READ, WRITE};
use std::{hint::black_box as bb, ptr::NonNull, sync::atomic::Ordering, time::Instant};
struct Memory(Box<PageTable>);
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(self.0.as_ref()))
    }
    fn u8(&self, a: u32) -> u8 {
        unsafe {
            mem::load_at(
                self.0
                    .backed_page((a / PAGE) as usize)
                    .add((a % PAGE) as usize),
                1,
            ) as u8
        }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        unsafe {
            mem::store_at(
                self.0
                    .backed_page((a / PAGE) as usize)
                    .add((a % PAGE) as usize),
                1,
                v as u32,
            )
        }
    }
}

#[inline(never)]
fn reference(st: &mut ddnative_base::x87::X87, m: &mut dyn Mem, a: u32, wide: bool) {
    if wide {
        st.memory::<0x00dd, 8, false>(m, a);
        st.memory::<0x18dd, 8, true>(m, a);
    } else {
        st.memory::<0x00d9, 4, false>(m, a);
        st.memory::<0x18d9, 4, true>(m, a);
    }
}
#[inline(never)]
fn candidate(st: &mut ddnative_base::x87::X87, m: &mut dyn Mem, a: u32, wide: bool) {
    if wide {
        st.real_memory::<0x00dd, 8, false>(m, a);
        st.real_memory::<0x18dd, 8, true>(m, a);
    } else {
        st.real_memory::<0x00d9, 4, false>(m, a);
        st.real_memory::<0x18d9, 4, true>(m, a);
    }
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
    assert_ne!(
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut value) },
        0
    );
    Some(value)
}
#[cfg(not(windows))]
fn cycles() -> Option<u64> {
    None
}
fn main() {
    ddnative_host::install();
    let mut st = ddnative_base::x87::X87::default();
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
    let m: &mut dyn Mem = bb(&mut memory as &mut dyn Mem);
    mem::ATOMIC_ACCESSES.store(true, Ordering::Relaxed);
    for wide in [false, true] {
        for offset in [0, 1, 4092] {
            for round in 0..11 {
                for index in 0..2 {
                    let new = (round + index) % 2 == 0;
                    let f = bb(if new { candidate } else { reference });
                    for i in 0..1000 {
                        f(&mut st, m, 0x100000 + ((i & 255) * PAGE) + offset, wide);
                    }
                    let c = cycles();
                    let t = Instant::now();
                    for i in 0..n {
                        f(
                            &mut st,
                            m,
                            bb(0x100000 + ((i & 255) * PAGE) + offset),
                            bb(wide),
                        );
                    }
                    let ns = t.elapsed().as_secs_f64() * 1e9 / n as f64;
                    let cy = cycles()
                        .zip(c)
                        .map(|(b, a)| (b - a) as f64 / n as f64)
                        .unwrap_or(f64::NAN);
                    println!(
                        "{} {offset} {round} {} {ns:.3} {cy:.3}",
                        if wide { "m64" } else { "m32" },
                        if new { "candidate" } else { "reference" }
                    );
                }
            }
        }
    }
    ddnative_base::x87::park();
}
