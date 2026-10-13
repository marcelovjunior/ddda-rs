//! The runtime policy behind the lifted code's slow paths (L74): what a
//! lifted call does on its way to the callee (preemption points, the
//! dispatch cache) and where a guest's x87 state lives while it is not in
//! the FPU. crates/ddnative-base declares these as hooks
//! (`ddnative_base::hooks`) and the shards compile against those
//! declarations only, so a change here rebuilds this crate and the
//! runtime, not the shards.
//!
//! Call [`install`] once before lifted code runs (the runtime, tools and
//! every test that executes lifted code do).
use ddnative_base::{
    hooks::{self, Hooks},
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};
use std::sync::atomic::{AtomicBool, Ordering};

pub mod x87;

/// Install this crate's hooks into ddnative-base (idempotent). Panics if
/// different hooks were installed first.
pub fn install() {
    let ours = hooks::install(Hooks {
        call_lifted,
        x87_live: x87::live_fallback,
        x87_acquire: x87::acquire,
        x87_image: x87::image,
        x87_release: x87::release,
        x87_park: x87::park,
    });
    assert!(ours || std::ptr::fn_addr_eq(hooks::get().call_lifted, call_lifted as fn(&mut dyn Mem, &mut Cpu, u32)),
            "other ddnative-base hooks are already installed");
}

/// Set by the runtime while another guest thread waits for the guest lock
/// (serial mode). Lifted calls are then preemption points: without them a
/// thread whose loops make no service call (the game's Vorbis decoder)
/// holds the lock for seconds and starves the main loop. The real OS may
/// switch threads at any instruction, so a call boundary is a legal point.
pub static PREEMPT: AtomicBool = AtomicBool::new(false);
static PREEMPT_HOOK: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// The runtime's preemption check (it yields when the time slice is used).
pub fn set_preempt_hook(hook: fn()) {
    let _ = PREEMPT_HOOK.set(hook);
}

/// The preemption point: the runtime's hook decides (its own rate limit,
/// the clock, the lock), so tuning it rebuilds only the runtime (L60).
#[cold]
#[inline(never)]
fn preempt_point() {
    if let Some(hook) = PREEMPT_HOOK.get() {
        hook();
    }
}

/// `x86::call_lifted`: a preemption point, then the target through the
/// dispatch cache. The hit path saves no registers and ends in a tail
/// jump to the callee; everything else is out of line.
fn call_lifted(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    #[cfg(feature = "private-replay")]
    if x86::private_calls::dispatch(m, cpu, target) { return; }
    if PREEMPT.load(Ordering::Relaxed) {
        return call_lifted_slow(m, cpu, target);
    }
    match cached(target) {
        Some(f) => f(m, cpu),
        None => call_lifted_slow(m, cpu, target),
    }
}

/// A preemption point or a cache miss. The function pointer is copied out
/// before the call, so nested calls are reentrant.
#[cold]
#[inline(never)]
fn call_lifted_slow(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    if PREEMPT.load(Ordering::Relaxed) {
        preempt_point();
    }
    let f = lifted_target(target);
    f(m, cpu);
}

/// Slots of the per-thread cache of resolved lifted targets.
const RESOLVED_SLOTS: usize = 4096;

/// Lifted targets already resolved on a thread, direct-mapped by address:
/// a cross-shard call resolves its target once instead of searching the
/// shard's table every time. The program's lookup never changes once
/// installed (x86::set_lifted_lookup), so entries never go stale.
type Resolved = [std::cell::Cell<(u32, Option<LiftFn>)>; RESOLVED_SLOTS];

thread_local! {
    /// This thread's cache, where the TEB slot below is unavailable.
    static RESOLVED: Resolved = const { [const { std::cell::Cell::new((0, None)) }; RESOLVED_SLOTS] };
}

/// The thread's cache is found through a Windows TLS slot read straight
/// from the TEB (TlsSlots at gs:[0x1480], 64 slots), one load on the call
/// path: a `thread_local!` here cost 3 ns of a 5 ns call (L74, hookbench).
/// u32::MAX: not allocated yet; 64 or more: unavailable.
static RESOLVED_SLOT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// This thread's cache.
#[cfg(all(windows, target_arch = "x86_64"))]
#[inline(always)]
fn resolved() -> &'static Resolved {
    let slot = RESOLVED_SLOT.load(Ordering::Relaxed);
    if slot < 64 {
        let p: *const Resolved;
        // SAFETY: TEB TlsSlots[slot] of the current thread, a slot this
        // process allocated; it holds null or this thread's leaked cache
        unsafe { std::arch::asm!("mov {p}, qword ptr gs:[{off}]", p = out(reg) p, off = in(reg) 0x1480 + 8 * slot as u64,
                                 options(nostack, readonly, preserves_flags)) }
        if !p.is_null() {
            // SAFETY: leaked, so 'static; only this thread uses it (Cells)
            return unsafe { &*p };
        }
    }
    resolved_slow()
}

/// Take the process's TLS slot for the dispatch cache (L103b): first thing
/// in main, with x87::reserve_slot, before the GPU driver's DLLs use up the
/// TEB's 64 direct slots. Taken lazily it was past them, and every
/// cross-shard call went through resolved_slow and a thread_local.
#[cfg(all(windows, target_arch = "x86_64"))]
pub fn reserve_resolved_slot() -> u32 {
    let slot = RESOLVED_SLOT.load(Ordering::Relaxed);
    if slot != u32::MAX {
        return slot;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn TlsAlloc() -> u32;
    }
    // SAFETY: plain Win32 call; one slot for the process, never freed
    let new = unsafe { TlsAlloc() };
    let new = if new < 64 { new } else { 64 }; // 64+: the thread_local fallback
    match RESOLVED_SLOT.compare_exchange(u32::MAX, new, Ordering::Relaxed, Ordering::Relaxed) {
        Ok(_) => new,
        Err(won) => won,
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn reserve_resolved_slot() -> u32 {
    u32::MAX
}

/// Allocate the slot and this thread's cache (leaked: a thread's cache
/// lives as long as the thread, and guest threads are few).
#[cfg(all(windows, target_arch = "x86_64"))]
#[cold]
#[inline(never)]
fn resolved_slow() -> &'static Resolved {
    let slot = reserve_resolved_slot();
    if slot >= 64 {
        // SAFETY: the thread_local lives as long as the thread; the
        // reference does not escape a call on it
        return RESOLVED.with(|r| unsafe { &*(r as *const Resolved) });
    }
    let cache: &'static Resolved = Box::leak(Box::new([const { std::cell::Cell::new((0, None)) }; RESOLVED_SLOTS]));
    // SAFETY: TEB TlsSlots[slot] of the current thread, allocated above
    unsafe { std::arch::asm!("mov qword ptr gs:[{off}], {v}", v = in(reg) cache as *const Resolved, off = in(reg) 0x1480 + 8 * slot as u64,
                             options(nostack, preserves_flags)) }
    cache
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn resolved() -> &'static Resolved {
    // SAFETY: as in resolved_slow
    RESOLVED.with(|r| unsafe { &*(r as *const Resolved) })
}

fn slot(target: u32) -> usize {
    // functions are mostly 16-byte aligned: drop those bits, fold the high ones in
    ((target >> 4) ^ (target >> 16)) as usize & (RESOLVED_SLOTS - 1)
}

/// The cached function at `target`, if this thread resolved it before.
#[inline]
fn cached(target: u32) -> Option<LiftFn> {
    match resolved()[slot(target)].get() {
        (t, Some(f)) if t == target => Some(f),
        _ => None,
    }
}

/// The lifted function at `target` (what `call_lifted` calls): an override
/// registered there, else the lifted body.
#[inline]
pub fn lifted_target(target: u32) -> LiftFn {
    cached(target).unwrap_or_else(|| {
        let f = x86::lifted_target_uncached(target);
        resolved()[slot(target)].set((target, Some(f)));
        f
    })
}

#[cfg(test)]
mod private_call_measurement {
    use super::*;
    struct Empty;
    impl Mem for Empty {
        fn u8(&self, _: u32) -> u8 { unreachable!() }
        fn set_u8(&mut self, _: u32, _: u8) { unreachable!() }
    }
    #[inline(never)]
    fn body(_: &mut dyn Mem, cpu: &mut Cpu) { cpu.eax = cpu.eax.wrapping_add(1); }
    fn lookup(_: u32) -> Option<LiftFn> { Some(body) }
    #[inline(never)]
    fn previous_path(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
        if PREEMPT.load(Ordering::Relaxed) { return call_lifted_slow(m, cpu, target); }
        match cached(target) { Some(f) => f(m, cpu), None => call_lifted_slow(m, cpu, target) }
    }

    /// Isolated runtime microbenchmark, not game performance. Run at opt3.
    #[test]
    #[ignore = "opt3 timing comparison, run explicitly on a quiet host"]
    fn default_private_resolver_cost() {
        use std::{hint::black_box, time::Instant};
        install();
        x86::set_lifted_lookup(lookup);
        let mut cpu = Cpu::default();
        call_lifted(&mut Empty, &mut cpu, 0x400010);
        let sample = |entry: LiftEntry, cpu: &mut Cpu| {
            let start = Instant::now();
            for _ in 0..1_000_000 { black_box(entry)(&mut Empty, cpu, black_box(0x400010)); }
            start.elapsed().as_nanos() as f64 / 1_000_000.0
        };
        type LiftEntry = fn(&mut dyn Mem, &mut Cpu, u32);
        for pair in 0..9 {
            let (old, new) = if pair % 2 == 0 {
                (sample(previous_path, &mut cpu), sample(call_lifted, &mut cpu))
            } else {
                let new = sample(call_lifted, &mut cpu);
                (sample(previous_path, &mut cpu), new)
            };
            eprintln!("private-resolver pair={pair} previous_ns={old:.3} default_ns={new:.3} ratio={:.3}", new / old);
        }
        black_box(cpu.eax);
    }
}
