//! L146 live flag predicates: keep the full defining ALU helper/state.
//! The reference uses the actual expanded predicates emitted by the lifter.
use ddnative_base::x86::Flags;
use std::{hint::black_box as bb, time::Instant};
#[inline(never)]
fn reference(f: &mut Flags, a: u32, b: u32, mode: u32) -> u32 {
    match mode {
        0 => {
            f.alu_sub(a, b, 32);
            f.zf() as u32
        }
        1 => {
            f.alu_sub(a, b, 32);
            (f.zf() || f.sf() != f.of()) as u32
        }
        _ => {
            f.alu_and(a, b, 32);
            (!f.zf()) as u32
        }
    }
}
#[inline(never)]
fn candidate(f: &mut Flags, a: u32, b: u32, mode: u32) -> u32 {
    match mode {
        0 => {
            f.alu_sub(a, b, 32);
            (f.a == f.b) as u32
        }
        1 => {
            f.alu_sub(a, b, 32);
            ((f.a as i32) <= (f.b as i32)) as u32
        }
        _ => {
            f.alu_and(a, b, 32);
            (f.a & f.b != 0) as u32
        }
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
    let mut f = Flags::default();
    let n: u32 = std::env::args()
        .nth(1)
        .unwrap_or("200000".into())
        .parse()
        .unwrap();
    assert!(n > 0);
    for mode in [0, 1, 2] {
        for round in 0..11 {
            for i in 0..2 {
                let new = (i + round) % 2 == 0;
                let op = bb(if new { candidate } else { reference });
                for j in 0..10000 {
                    bb(op(&mut f, bb(j), bb(0x80000000), bb(mode)));
                }
                let c = cycles();
                let t = Instant::now();
                for j in 0..n {
                    bb(op(&mut f, bb(j), bb(0x80000000), bb(mode)));
                }
                let ns = t.elapsed().as_secs_f64() * 1e9 / n as f64;
                let cy = cycles()
                    .zip(c)
                    .map(|(b, a)| (b - a) as f64 / n as f64)
                    .unwrap_or(f64::NAN);
                println!(
                    "{mode} {round} {} {ns:.3} {cy:.3}",
                    if new { "candidate" } else { "reference" }
                );
                bb(f);
            }
        }
    }
}
