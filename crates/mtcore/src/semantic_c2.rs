//! Semantic ports (L170, claude-2; the pattern of L171's semantic.rs):
//! native rewrites of hot game functions that keep the original's
//! ABI-visible result (return value, callee-saved registers, ESP, and every
//! guest memory write at or above the returned stack) instead of replaying
//! its instructions. Caller-saved ECX and EDX, the flags and the popped
//! stack scratch are not reproduced: MSVC-compiled callers never read them
//! after a call. Opt-in with `DDRUNTIME_SEMANTIC=1` (crates/ddports);
//! checked against the original's recorded vectors and, on random inputs,
//! against its lifted body (crates/mtcore/tests/semantic_c2.rs).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{Cpu, LiftFn};

/// The ports: (address, adapter).
pub const PORTS: &[(u32, LiftFn)] = &[(0x0106_6590, find_keyed_entry)];

// Translated from 0x01066590 (DDDA.exe 2017-12-13 Steam), FUN_01066590.
/// `this->find(list, key) -> entry` (thiscall, `ret 8`): the entry with
/// 12-bit id `key & 0xFFF` among list `list`'s, or 0.
///
/// List `list` has `this[0x88 / 4 + list]` entries, indices into the
/// 12-byte entries at `this + 0x30`, whose first word holds the id in its
/// top 12 bits; the indices are at `this[0x78 / 4 + list]`. Up to two
/// entries are searched in order, more by bisection over the indices (ids
/// ascending), with the original's 32-bit wrapping arithmetic.
pub fn find_keyed_entry(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let list = pg.u32(m, c.esp.wrapping_add(4));
    let key = pg.u32(m, c.esp.wrapping_add(8)) & 0xFFF;
    let this = c.ecx;
    let count = pg.u32(m, this.wrapping_add(list.wrapping_mul(4)).wrapping_add(0x88));
    c.eax = if count == 0 {
        0
    } else {
        let indices = pg.u32(m, this.wrapping_add(list.wrapping_mul(4)).wrapping_add(0x78));
        let entries = pg.u32(m, this.wrapping_add(0x30));
        // the entry an index slot names, and its id
        let entry = |m: &mut dyn Mem, slot: u32| {
            let index = pg.u32(m, indices.wrapping_add(slot.wrapping_mul(4)));
            let at = entries.wrapping_add(index.wrapping_mul(12));
            (at, pg.u32(m, at) >> 20)
        };
        if count <= 2 {
            (0..count).map(|slot| entry(m, slot)).find(|e| e.1 == key).map_or(0, |e| e.0)
        } else {
            let (mut low, mut high) = (0u32, count.wrapping_sub(1));
            loop {
                let middle = high.wrapping_add(low) >> 1;
                let (at, id) = entry(m, middle);
                if id == key {
                    break at;
                }
                if low >= high {
                    break 0;
                }
                if id < key {
                    low = middle.wrapping_add(1);
                } else {
                    high = middle.wrapping_sub(1);
                }
            }
        }
    };
    c.esp = c.esp.wrapping_add(4 + 8);
}
