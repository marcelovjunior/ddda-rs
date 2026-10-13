//! Semantic ports (L176) of the nDraw material's parameter lookup, on the
//! shadow and main-pass model-draw path (under uBaseModel::vf26
//! 0x00EA5320). They keep the original's ABI-visible result (EAX, the
//! callee-saved registers, ESP, every memory write above the returned
//! stack) rather than its instructions; ECX, EDX, the flags and the popped
//! stack scratch are not reproduced (MSVC callers never read them after a
//! call). Checked against the original's recorded vectors in
//! tests/material.rs; opt-in through crates/ddports
//! (`DDRUNTIME_MATERIAL=1`).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

/// The binding table the found parameter is filled from (a pointer).
const BINDINGS: u32 = 0x018D_12CC;

// Translated from 0x01066590 (DDDA.exe 2017-12-13 Steam).
/// 0x01066590, `this->find(slot, key) -> entry` (thiscall, `ret 8`): the
/// 12-byte parameter entry of `slot` whose key (bits 20..31 of its first
/// word) is `key & 0xfff`, or 0.
pub fn find_parameter(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let slot = pg.u32(m, c.esp.wrapping_add(4));
    c.eax = find(m, pg, c.ecx, slot, Key::At(c.esp.wrapping_add(8)));
    c.esp = c.esp.wrapping_add(12);
}

/// The key searched for: read from its argument slot only once the count
/// is non-zero, as the original does, or already read by the caller.
#[derive(Clone, Copy)]
enum Key {
    At(u32),
    Value(u32),
}

/// The search itself. `this + 0x78 + 4*slot` points at `count` (at
/// `this + 0x88 + 4*slot`) entry indices; entries are 12 bytes from
/// `this + 0x30`. Up to two indices are searched in order; more by a
/// binary search over the indices' keys. All arithmetic wraps as the
/// original's: when the upper bound passes below index 0 the midpoint
/// becomes 0x7FFFFFFF and the search reads the word before the list.
fn find(m: &dyn Mem, pg: Pages, this: u32, slot: u32, key: Key) -> u32 {
    let at = this.wrapping_add(slot.wrapping_mul(4));
    let count = pg.u32(m, at.wrapping_add(0x88));
    if count == 0 {
        return 0;
    }
    let list = pg.u32(m, at.wrapping_add(0x78));
    let key = match key {
        Key::At(a) => pg.u32(m, a),
        Key::Value(v) => v,
    } & 0xfff;
    let entries = pg.u32(m, this.wrapping_add(0x30));
    // the i-th listed entry and its key
    let entry = |i: u32| {
        let index = pg.u32(m, list.wrapping_add(i.wrapping_mul(4)));
        let e = entries.wrapping_add(index.wrapping_mul(12));
        (e, pg.u32(m, e) >> 20)
    };
    if count <= 2 {
        for i in 0..count {
            let (e, k) = entry(i);
            if k == key {
                return e;
            }
        }
        return 0;
    }
    let (mut lo, mut hi) = (0u32, count - 1);
    loop {
        let mid = hi.wrapping_add(lo) >> 1;
        let (e, k) = entry(mid);
        if k == key {
            return e;
        }
        if lo >= hi {
            return 0;
        }
        if k >= key {
            hi = mid.wrapping_sub(1);
        } else {
            lo = mid.wrapping_add(1);
        }
    }
}

// Translated from 0x01066B10 (DDDA.exe 2017-12-13 Steam).
/// 0x01066B10, `this->bind(key, value) -> found` (thiscall, `ret 8`):
/// find `key` in slot 0; if found, set the entry's second word from the
/// binding table (`[0x018D12CC] + 8*(value & 0xfff) + 0x3c`) and its third
/// to `value`'s 12-bit index under `key`'s high bits. AL is 1 when found
/// (EAX keeps the entry's upper bytes, as the original's `mov al, 1`), 0
/// otherwise.
pub fn bind_parameter(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let key = pg.u32(m, c.esp.wrapping_add(4));
    let e = find(m, pg, c.ecx, 0, Key::Value(key));
    c.eax = if e == 0 {
        0
    } else {
        let value = pg.u32(m, c.esp.wrapping_add(8));
        let table = pg.u32(m, BINDINGS);
        let index = value & 0xfff;
        let bound = pg.u32(m, table.wrapping_add(index.wrapping_mul(8)).wrapping_add(0x3c));
        pg.set_u32(m, e.wrapping_add(4), bound);
        pg.set_u32(m, e.wrapping_add(8), ((key ^ value) & 0xffff_f000) | index);
        (e & !0xff) | 1
    };
    c.esp = c.esp.wrapping_add(12);
}
