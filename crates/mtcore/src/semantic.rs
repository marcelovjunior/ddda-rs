//! Semantic ports (L171): native rewrites of hot game functions that keep
//! the ABI-visible result of the original (return value, callee-saved
//! registers, ESP, and every guest memory write above the returned stack)
//! instead of replaying its instructions. Caller-saved registers (ECX,
//! EDX), the arithmetic flags and the popped stack scratch are not
//! reproduced: MSVC-compiled callers never read them after a call.
//!
//! Each port is opt-in (`DDRUNTIME_SEMANTIC=1`, crates/ddports) and is
//! checked against the original's recorded vectors at that ABI level
//! (crates/mtcore/tests/semantic.rs).
use ddnative_base::mem::Pages;
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// 0x00E0EC50, `this->expand(packed, values, out) -> out_end` (thiscall,
/// `ret 0xc`): expand a packed dependency into `out`.
///
/// `this + 0x28` points at the descriptor bank: 8-byte descriptors whose
/// first word holds the recursive-child count in bits 20..25 and the
/// terminal count in bits 26..31, and whose second word points at the u16
/// indices of the children followed by the terminals. `values` is a bank
/// of 8-byte entries whose second word is a packed value: bits 0..11 a
/// descriptor index, bits 12..31 an optional output value. A packed value
/// emits its high part when non-zero, then its recursive children
/// (expanded in turn) and its non-zero terminal values. The original is
/// this recursion unrolled five levels deep, calling itself below that.
pub fn expand_dependencies(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let packed = pg.u32(m, c.esp.wrapping_add(4));
    let values = pg.u32(m, c.esp.wrapping_add(8));
    let out = pg.u32(m, c.esp.wrapping_add(0xc));
    let bank = pg.u32(m, c.ecx.wrapping_add(0x28));
    c.eax = expand(m, pg, bank, values, packed, out);
    c.esp = c.esp.wrapping_add(4 + 0xc);
}

fn expand(m: &mut dyn Mem, pg: Pages, bank: u32, values: u32, packed: u32, mut out: u32) -> u32 {
    let high = packed & 0xffff_f000;
    if high != 0 {
        pg.set_u32(m, out, high);
        out = out.wrapping_add(4);
    }
    let descriptor = bank.wrapping_add((packed & 0xfff).wrapping_mul(8));
    let mut index = pg.u32(m, descriptor.wrapping_add(4));
    // The original reloads the descriptor word for every loop test; keep
    // that, so a descriptor rewritten through `out` behaves the same.
    let mut i = 0u32;
    while i < (pg.u32(m, descriptor) >> 20) & 0x3f {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        out = expand(m, pg, bank, values, value, out);
        i += 1;
    }
    let mut i = 0u32;
    while i < pg.u32(m, descriptor) >> 26 {
        let child = pg.u16(m, index) as u32;
        index = index.wrapping_add(2);
        let value = pg.u32(m, values.wrapping_add(child.wrapping_mul(8)).wrapping_add(4));
        if value != 0 {
            pg.set_u32(m, out, value);
            out = out.wrapping_add(4);
        }
        i += 1;
    }
    out
}
