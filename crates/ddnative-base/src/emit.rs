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

/// `push v`: ESP -= 4, then the dword store at the new ESP.
#[inline]
pub fn push(v: u32, m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &mut Cpu) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.fast_set_u32(pt, cpu.esp, v);
}

/// `pop`: the dword at ESP, then ESP += 4; the caller stores the value.
#[inline]
pub fn pop(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, cpu: &mut Cpu) -> u32 {
    let v = m.fast_u32(pt, cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
    v
}

/// MOVUPS/MOVAPS-style 16-byte load: the four dwords at a, a+4, a+8, a+12,
/// read in that order.
#[inline]
pub fn load128(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) -> [u32; 4] {
    [m.fast_u32(pt, a), m.fast_u32(pt, a.wrapping_add(4)), m.fast_u32(pt, a.wrapping_add(8)), m.fast_u32(pt, a.wrapping_add(12))]
}

/// 8-byte load into the low half (MOVQ/MOVSD xmm, m64): upper lanes zero.
#[inline]
pub fn load64(m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) -> [u32; 4] {
    [m.fast_u32(pt, a), m.fast_u32(pt, a.wrapping_add(4)), 0, 0]
}

/// 16-byte store: lanes 0..3 to a, a+4, a+8, a+12, in that order.
#[inline]
pub fn store128(x: [u32; 4], m: &mut dyn Mem, pt: Option<NonNull<PageTable>>, a: u32) {
    for k in 0..4u32 {
        m.fast_set_u32(pt, a.wrapping_add(4 * k), x[k as usize]);
    }
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
