//! L74 hook cost: a lifted cross-shard call through ddnative-base's hook
//! table (new) vs the inline dispatch cache it replaced (old, copied from
//! ddnative-base before L74). This crate stands in for a shard: its
//! opt-level is the shard's (Cargo.toml).
use ddnative_base::mem::{Mem, Sparse};
use ddnative_base::x86::{self, Cpu, LiftFn};
use std::time::Instant;

fn leaf(_: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = cpu.eax.wrapping_add(1);
}
fn lookup(_: u32) -> Option<LiftFn> {
    Some(leaf)
}

mod old {
    use super::*;
    pub static PREEMPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    #[cold]
    #[inline(never)]
    fn preempt_point() {}
    #[inline]
    pub fn call_lifted(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
        if PREEMPT.load(std::sync::atomic::Ordering::Relaxed) {
            preempt_point();
        }
        let f = lifted_target(target);
        f(m, cpu);
    }
    const RESOLVED_SLOTS: usize = 4096;
    thread_local! {
        static RESOLVED: [std::cell::Cell<(u32, Option<LiftFn>)>; RESOLVED_SLOTS] =
            const { [const { std::cell::Cell::new((0, None)) }; RESOLVED_SLOTS] };
    }
    #[inline]
    pub fn cached(target: u32) -> Option<LiftFn> {
        let slot = ((target >> 4) ^ (target >> 16)) as usize & (RESOLVED_SLOTS - 1);
        RESOLVED.with(|r| match r[slot].get() {
            (t, Some(f)) if t == target => Some(f),
            _ => None,
        })
    }
    #[inline]
    pub fn lifted_target(target: u32) -> LiftFn {
        let slot = ((target >> 4) ^ (target >> 16)) as usize & (RESOLVED_SLOTS - 1);
        let hit = RESOLVED.with(|r| match r[slot].get() {
            (t, Some(f)) if t == target => Some(f),
            _ => None,
        });
        hit.unwrap_or_else(|| {
            let f = x86::lifted_target_uncached(target);
            RESOLVED.with(|r| r[slot].set((target, Some(f))));
            f
        })
    }
}

// The call sites as the shards emit them (a closure per call).
#[inline(never)]
fn run_old(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| old::call_lifted(m, cpu, t))(m, cpu);
    }
}
#[inline(never)]
fn run_new(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| x86::call_lifted(m, cpu, t))(m, cpu);
    }
}

static H: std::sync::OnceLock<fn(&mut dyn Mem, &mut Cpu, u32)> = std::sync::OnceLock::new();
#[inline(never)]
fn old_out_of_line(m: &mut dyn Mem, cpu: &mut Cpu, t: u32) { old::call_lifted(m, cpu, t) }
#[inline(never)]
fn run_local_hook(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| (H.get().unwrap())(m, cpu, t))(m, cpu);
    }
}
#[inline(never)]
fn run_out_of_line(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| old_out_of_line(m, cpu, t))(m, cpu);
    }
}
#[inline(never)]
fn run_host_direct(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| ddnative_host::lifted_target(t)(m, cpu))(m, cpu);
    }
}

static H2: std::sync::OnceLock<fn(&mut dyn Mem, &mut Cpu, u32)> = std::sync::OnceLock::new();
#[inline(never)]
fn via_host_target(m: &mut dyn Mem, cpu: &mut Cpu, t: u32) { ddnative_host::lifted_target(t)(m, cpu) }
#[inline(never)]
fn run_hook_host_target(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| (H2.get().unwrap())(m, cpu, t))(m, cpu);
    }
}
#[inline(never)]
fn run_base_hooks_local(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| (ddnative_base::hooks::get().x87_park)())(m, cpu);
        let _ = t;
    }
}

static H3: std::sync::OnceLock<fn(&mut dyn Mem, &mut Cpu, u32)> = std::sync::OnceLock::new();
#[inline(never)]
fn host_shape(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    if old::PREEMPT.load(std::sync::atomic::Ordering::Relaxed) {
        return old_out_of_line(m, cpu, target);
    }
    match old::cached(target) {
        Some(f) => f(m, cpu),
        None => old_out_of_line(m, cpu, target),
    }
}
#[inline(never)]
fn run_host_shape(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| (H3.get().unwrap())(m, cpu, t))(m, cpu);
    }
}

#[inline(never)]
fn run_leaf_only(m: &mut dyn Mem, cpu: &mut Cpu, targets: &[u32], n: usize) {
    for k in 0..n {
        let t = targets[k & (targets.len() - 1)];
        (|m: &mut dyn Mem, cpu: &mut Cpu| { let _ = t; std::hint::black_box(leaf as LiftFn)(m, cpu) })(m, cpu);
    }
}

fn main() {
    let _ = H3.set(host_shape);
    let _ = H2.set(via_host_target);
    let _ = H.set(old_out_of_line);
    ddnative_host::install();
    x86::set_lifted_lookup(lookup);
    let targets: Vec<u32> = (0..1024u32).map(|k| 0x0040_1000 + k * 0x10).collect();
    let mut mem = Sparse::default();
    let n = 10_000_000;
    let variants: [(&str, fn(&mut dyn Mem, &mut Cpu, &[u32], usize)); 9] = [
        ("harness + leaf only", run_leaf_only),
        ("hook->bench host shape", run_host_shape),
        ("hook->host target", run_hook_host_target), ("base hook park()", run_base_hooks_local),
        ("old inline cache", run_old), ("old out of line", run_out_of_line), ("local hook", run_local_hook),
        ("host cache direct", run_host_direct), ("new hook", run_new)];
    let mut best = [f64::MAX; 9];
    for _ in 0..15 {
        for (k, (_, f)) in variants.iter().enumerate() {
            let mut cpu = Cpu::default();
            let t = Instant::now();
            f(&mut mem, &mut cpu, &targets, n);
            best[k] = best[k].min(t.elapsed().as_secs_f64() * 1e9 / n as f64);
        }
    }
    for (k, (name, _)) in variants.iter().enumerate() {
        println!("{name:>17}: {:.2} ns/call (best of 15)", best[k]);
    }
}
