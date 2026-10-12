//! Single-operation costs behind the lifted code's hot paths (brief L25),
//! without the lifted program (a light build):
//!
//!     cargo run --release -p ddruntime --no-default-features --example perf_micro
//!
//! - a `call_ext` round trip through the runtime's Registry to a service
//!   that only returns (what an indirect call or an API call costs before
//!   the callee does anything);
//! - a guest u32 read on `GuestMemory` through `&dyn Mem`, through a direct
//!   (static) call, and through the lifted code's inline fast path
//!   (`fast_u32` with the page table), reads within one mapped page.
//!
//! (`ddperf micro` adds the lifted call-target cache, which needs the
//! lifted program.)
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use ddruntime::dispatch::{Binding, Registry};
use ddruntime::memory::{GuestMemory, READ, WRITE};
use std::time::Instant;

const N: u32 = 20_000_000;

/// `ret`: the cheapest service.
fn just_return(m: &mut dyn Mem, cpu: &mut Cpu) {
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
}

#[inline(never)]
fn read_dyn(m: &dyn Mem, base: u32, n: u32) -> u32 {
    (0..n).fold(0u32, |acc, k| acc.wrapping_add(m.u32(base.wrapping_add((k & 0x3FF) * 4))))
}

#[inline(never)]
fn read_direct(m: &GuestMemory, base: u32, n: u32) -> u32 {
    (0..n).fold(0u32, |acc, k| acc.wrapping_add(Mem::u32(m, base.wrapping_add((k & 0x3FF) * 4))))
}

#[inline(never)]
fn read_fast(m: &dyn Mem, base: u32, n: u32) -> u32 {
    let pt = m.page_table();
    (0..n).fold(0u32, |acc, k| acc.wrapping_add(m.fast_u32(pt, base.wrapping_add((k & 0x3FF) * 4))))
}

#[inline(never)]
fn write_dyn(m: &mut dyn Mem, base: u32, n: u32) {
    for k in 0..n {
        m.set_u32(base.wrapping_add((k & 0x3FF) * 4), k);
    }
}

#[inline(never)]
fn write_fast(m: &mut dyn Mem, base: u32, n: u32) {
    let pt = m.page_table();
    for k in 0..n {
        m.fast_set_u32(pt, base.wrapping_add((k & 0x3FF) * 4), k);
    }
}

fn ns(t: Instant, n: u32) -> f64 {
    t.elapsed().as_secs_f64() * 1e9 / n as f64
}

fn main() {
    let mut mem = GuestMemory::new();
    mem.map(0x1000_0000, 0x10_0000, READ | WRITE).unwrap();

    let mut registry = Registry::new();
    registry.insert(0x0100_0000, Binding::Service(just_return)).unwrap();
    let registry = std::sync::Arc::new(registry);
    registry.install();
    let mut cpu = Cpu { esp: 0x1008_0000, ..Cpu::default() };
    let t = Instant::now();
    for _ in 0..N {
        cpu.esp -= 4;
        mem.set_u32(cpu.esp, 0x1234_5678);
        x86::call_ext(&mut mem, &mut cpu, 0x0100_0000);
    }
    println!("call_ext -> Registry -> service (push, ret): {:.2} ns per call", ns(t, N));
    x86::set_ext_calls(None);

    for round in 0..2 {
        let mut sum = 0u32;
        let t = Instant::now();
        sum = sum.wrapping_add(read_dyn(&mem, 0x1000_0000, N));
        let a = ns(t, N);
        let t = Instant::now();
        sum = sum.wrapping_add(read_direct(&mem, 0x1000_0000, N));
        let b = ns(t, N);
        let t = Instant::now();
        sum = sum.wrapping_add(read_fast(&mem, 0x1000_0000, N));
        let c = ns(t, N);
        let t = Instant::now();
        write_dyn(&mut mem, 0x1000_0000, N);
        let d = ns(t, N);
        let t = Instant::now();
        write_fast(&mut mem, 0x1000_0000, N);
        let e = ns(t, N);
        if round == 1 {
            println!("guest u32 read: dyn Mem {a:.2} ns, direct call {b:.2} ns, inline fast path {c:.2} ns ({sum:#x})");
            println!("guest u32 write: dyn Mem {d:.2} ns, inline fast path {e:.2} ns");
        }
    }
}
