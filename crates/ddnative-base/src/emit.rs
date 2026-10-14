//! Idiom helpers the lifted shards call (research/native/lean_emission.py,
//! L73): one call per push or pop instead of the inline template, which
//! cuts the source the shard crates type-check and borrow-check. Same
//! semantics as the templates in lift.py, in the same order: the pushed
//! value is evaluated by the caller before ESP moves, and a pop reads at the
//! old ESP before the destination store.
//!
//! `#[inline]`, not `#[inline(always)]` (docs/tasks/AGENT-RULES.md): at
//! opt-level 0 each shard keeps one out-of-line copy; optimized shards
//! inline them.
use crate::mem::{Mem, PageTable};
use crate::x86::Cpu;
use std::ptr::NonNull;

#[path = "emit/flags.rs"]
mod flags;

/// `push v`: ESP -= 4, then the dword store at the new ESP.
// L190 (feature `lean-access`): one optimised out-of-line copy with the store
// inlined, not an unoptimised copy per opt-level-0 shard that calls
// `fast_set_u32` (measured in HOT_PATHS.md section 10).
#[cfg_attr(feature = "lean-access", inline(never))]
#[cfg_attr(not(feature = "lean-access"), inline)]
pub fn push(v: u32, m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &mut Cpu) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    #[cfg(feature = "lean-access")]
    m.fast_set_u32_inner(pt, cpu.esp, v);
    #[cfg(not(feature = "lean-access"))]
    m.fast_set_u32(pt, cpu.esp, v);
}

/// `pop`: the dword at ESP, then ESP += 4; the caller stores the value.
#[cfg_attr(feature = "lean-access", inline(never))]
#[cfg_attr(not(feature = "lean-access"), inline)]
pub fn pop(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &mut Cpu) -> u32 {
    #[cfg(feature = "lean-access")]
    let v = m.fast_u32_inner(pt, cpu.esp);
    #[cfg(not(feature = "lean-access"))]
    let v = m.fast_u32(pt, cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
    v
}

/// MOVUPS/MOVAPS-style 16-byte load: the four dwords at a, a+4, a+8, a+12,
/// read in that order.
#[inline]
pub fn load128(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) -> [u32; 4] {
    m.fast_u128_words(pt, a)
}

/// 8-byte load into the low half (MOVQ/MOVSD xmm, m64): upper lanes zero.
#[inline]
pub fn load64(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) -> [u32; 4] {
    let [lo, hi] = m.fast_u64_words(pt, a);
    [lo, hi, 0, 0]
}

/// 16-byte store: lanes 0..3 to a, a+4, a+8, a+12, in that order.
/// Compile the loop once at the base crate's optimization level, including
/// for opt-level-0 callers. Each lane still checks the current page entry
/// and permissions separately; a fallback may change the following lane.
#[inline(never)]
pub fn store128(x: [u32; 4], m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) {
    m.fast_set_u128_words(pt, a, x);
}

/// XORPS/XORPD/PXOR: lane-wise d ^ s.
#[inline]
pub fn xorps(d: [u32; 4], s: [u32; 4]) -> [u32; 4] {
    [d[0] ^ s[0], d[1] ^ s[1], d[2] ^ s[2], d[3] ^ s[3]]
}

/// ANDPS/ANDPD/PAND: lane-wise d & s.
#[inline]
pub fn andps(d: [u32; 4], s: [u32; 4]) -> [u32; 4] {
    [d[0] & s[0], d[1] & s[1], d[2] & s[2], d[3] & s[3]]
}

/// ORPS/ORPD/POR: lane-wise d | s.
#[inline]
pub fn orps(d: [u32; 4], s: [u32; 4]) -> [u32; 4] {
    [d[0] | s[0], d[1] | s[1], d[2] | s[2], d[3] | s[3]]
}

/// ANDNPS/ANDNPD/PANDN: lane-wise !d & s.
#[inline]
pub fn andnps(d: [u32; 4], s: [u32; 4]) -> [u32; 4] {
    [!d[0] & s[0], !d[1] & s[1], !d[2] & s[2], !d[3] & s[3]]
}

/// 8-byte store of the low half (MOVQ/MOVSD m64, xmm): lane 0 to a, then
/// lane 1 to a+4.
#[inline]
pub fn store64(x: [u32; 4], m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) {
    m.fast_set_u64_words(pt, a, [x[0], x[1]]);
}

/// A dword load from the guest stack (L190): with feature `stack-window`, inside the CPU's stack
/// window one host load, else (and without the feature) the page-table access. For addresses
/// formed from ESP or EBP.
#[inline(always)]
pub fn stack_u32(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &Cpu, addr: u32) -> u32 {
    #[cfg(feature = "stack-window")]
    {
        let off = addr.wrapping_sub(cpu.stack_lo);
        if off < cpu.stack_span && addr & 3 == 0 {
            // SAFETY: the window is a live contiguous host block (Cpu::set_stack_window), the
            // dword aligned inside it; same bytes the page table serves
            return u32::from_le(unsafe { (*((cpu.stack_host + off as usize) as *const std::sync::atomic::AtomicU32)).load(std::sync::atomic::Ordering::Relaxed) });
        }
    }
    let _ = cpu;
    m.fast_u32(pt, addr)
}

/// A dword store to the guest stack, as [`stack_u32`].
#[inline(always)]
pub fn stack_set_u32(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &Cpu, addr: u32, v: u32) {
    #[cfg(feature = "stack-window")]
    {
        let off = addr.wrapping_sub(cpu.stack_lo);
        if off < cpu.stack_span && addr & 3 == 0 {
            // SAFETY: as stack_u32
            return unsafe { (*((cpu.stack_host + off as usize) as *const std::sync::atomic::AtomicU32)).store(v.to_le(), std::sync::atomic::Ordering::Relaxed) };
        }
    }
    let _ = cpu;
    m.fast_set_u32(pt, addr, v)
}

/// `push v` with the stack window.
#[inline(always)]
pub fn push_s(v: u32, m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &mut Cpu) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    let esp = cpu.esp;
    stack_set_u32(m, pt, cpu, esp, v);
}

/// `pop` with the stack window.
#[inline(always)]
pub fn pop_s(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &mut Cpu) -> u32 {
    let v = stack_u32(m, pt, cpu, cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
    v
}
