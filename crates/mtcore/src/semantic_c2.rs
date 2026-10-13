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
pub const PORTS: &[(u32, LiftFn)] = &[(0x0106_6590, find_keyed_entry), (0x0105_4700, update_elements), (0x0100_8200, find_hashed_record), (0x0106_58B0, apply_pass_state)];

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

// Translated from 0x01008200 (DDDA.exe 2017-12-13 Steam), FUN_01008200.
/// `this->find(key) -> record` (thiscall, `ret 4`): the 36-byte record of
/// `key` in `this`'s hash table (`this[0x84 / 4]`), or 0 (also for key 0).
///
/// The table holds 4096 bucket heads at +0x1C, indexed by the key's low 12
/// bits, each the address of a 16-byte node: its key, a word, the next
/// node's index (into the node array at table +0x18; 0 ends the chain) and
/// its record index (negative: no record). The records start at table
/// +0x401C.
pub fn find_hashed_record(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let key = pg.u32(m, c.esp.wrapping_add(4));
    c.eax = if key == 0 {
        0
    } else {
        let table = pg.u32(m, c.ecx.wrapping_add(0x84));
        hashed_record(m, pg, table, key)
    };
    c.esp = c.esp.wrapping_add(4 + 4);
}

fn hashed_record(m: &dyn Mem, pg: Pages, table: u32, key: u32) -> u32 {
    let mut node = pg.u32(m, table.wrapping_add(0x1C).wrapping_add((key & 0xFFF).wrapping_mul(4)));
    if node == 0 {
        return 0;
    }
    while pg.u32(m, node) != key {
        let next = pg.u32(m, node.wrapping_add(8));
        if next == 0 {
            return 0;
        }
        node = (next << 4).wrapping_add(pg.u32(m, table.wrapping_add(0x18)));
        if node == 0 {
            return 0;
        }
    }
    let record = pg.u32(m, node.wrapping_add(12));
    if (record as i32) < 0 {
        return 0;
    }
    table.wrapping_add(0x401C).wrapping_add((record & 0x7FFF_FFFF).wrapping_mul(9).wrapping_mul(4))
}

// Translated from 0x010658B0 (DDDA.exe 2017-12-13 Steam), FUN_010658B0.
/// `this->apply(state, unused, kind) -> bool` (thiscall, `ret 0xc`): write
/// this pass's settings into the draw state block `state`, marking what
/// changed, then run 0x01062F20 on it; false (nothing written but the
/// stamp) unless `kind` is 0xB, 0xC or 0x10.
///
/// - `this` +4 gets the global frame stamp (0x018D2CE8) plus one, always.
/// - The program pair (state +0x1CC/+0x1D0 = 0x17E15A66/0x53E9BA56) and its
///   variant (+0x169: 0 for kind 0xB, else 1), with +0x158 = 0xA66 and
///   +0x15A = the variant; set when any differs, marking +0x154 bits 0-1.
/// - Three cached bindings (+0x11C from +0xEC8 and +0x114's low byte; +0x118
///   from +0xD28 and +0x110 from the global 0x0162113C; +0x120 from the
///   table at +0x1418 by `this`'s object's level, or that object): each
///   change sets +0x114 to its low 12 bits | 0x1000.
/// - +0x154 takes four bit fields of `this` +0x14 and bit 0.
/// - `this` +0x30 lists `this[0x18 / 4] & 0x1FF` 12-byte entries: a slot id
///   (top 12 bits) and two words for the state's slot table at +0x1F8.
/// The original re-reads each source after its own writes, and so does
/// this (the blocks may overlap).
pub fn apply_pass_state(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    let stamp = pg.u32(m, 0x018D_2CE8).wrapping_add(1);
    pg.set_u32(m, this.wrapping_add(4), stamp);
    let kind = pg.u32(m, c.esp.wrapping_add(0xC));
    let variant = match kind {
        0xB => 0u8,
        0xC | 0x10 => 1,
        _ => {
            // AL = 0 over the original's kind - 0x10
            c.eax = kind.wrapping_sub(0x10) & !0xFF;
            c.esp = c.esp.wrapping_add(4 + 0xC);
            return;
        }
    };
    let state = pg.u32(m, c.esp.wrapping_add(4));
    set_program(m, pg, state, variant);
    // the cached bindings
    let mark = |m: &mut dyn Mem| {
        let at = state.wrapping_add(0x114);
        let v = pg.u32(m, at) & 0xFFF | 0x1000;
        pg.set_u32(m, at, v);
    };
    let binding = pg.u32(m, state.wrapping_add(0xEC8));
    if pg.u32(m, state.wrapping_add(0x11C)) != binding || pg.u8(m, state.wrapping_add(0x114)) != 0 {
        pg.set_u32(m, state.wrapping_add(0x11C), binding);
        pg.set_u8(m, state.wrapping_add(0x114), 0);
        mark(m);
    }
    let binding = pg.u32(m, state.wrapping_add(0xD28));
    let global = pg.u32(m, 0x0162_113C);
    if pg.u32(m, state.wrapping_add(0x118)) != binding || global != pg.u32(m, state.wrapping_add(0x110)) {
        pg.set_u32(m, state.wrapping_add(0x118), binding);
        let flags = pg.u32(m, state.wrapping_add(0x114)) & 0xFFF | 0x1000;
        pg.set_u32(m, state.wrapping_add(0x110), global);
        pg.set_u32(m, state.wrapping_add(0x114), flags);
    }
    let object = pg.u32(m, this.wrapping_add(0x24));
    let level = match object {
        0 => 2,
        _ => match pg.u32(m, object.wrapping_add(0x10)) >> 3 & 7 {
            l @ 0..=2 => l,
            _ => 2,
        },
    };
    let table = pg.u32(m, state.wrapping_add(0x1418)) & !0xF;
    let target = state.wrapping_add(0x120);
    if table == 0 {
        pg.set_u32(m, target, object);
        mark(m);
    } else {
        let selected = pg.u32(m, table.wrapping_add(level * 4));
        if pg.u32(m, target) != selected {
            pg.set_u32(m, target, selected);
            mark(m);
        }
    }
    set_pass_flags(m, pg, this, state);
    // the slot table
    let count = |m: &dyn Mem| pg.u32(m, this.wrapping_add(0x18)) & 0x1FF;
    let mut remaining = count(m) != 0;
    let mut entry = pg.u32(m, this.wrapping_add(0x30));
    let mut copied = 0u32;
    while remaining {
        let slot = state.wrapping_add((pg.u32(m, entry) >> 20).wrapping_mul(8));
        let first = pg.u32(m, entry.wrapping_add(4));
        pg.set_u32(m, slot.wrapping_add(0x1F8), first);
        let second = pg.u32(m, entry.wrapping_add(8));
        pg.set_u32(m, slot.wrapping_add(0x1FC), second);
        copied += 1;
        entry = entry.wrapping_add(12);
        remaining = copied < count(m);
    }
    let dirty = state.wrapping_add(0x154);
    let v = pg.u32(m, dirty) | 3;
    pg.set_u32(m, dirty, v);
    // 0x01062F20(state) on `this`, as the original calls it (its ret 4)
    for v in [c.ebx, c.esi, c.edi] {
        push(m, pg, c, v);
    }
    c.eax = state;
    push(m, pg, c, state);
    call_guest(m, pg, c, 0x0106_2F20, 0x0106_5AED);
    c.edi = pop(m, pg, c);
    c.esi = pop(m, pg, c);
    c.ebx = pop(m, pg, c);
    c.eax = c.eax & !0xFF | 1;
    c.esp = c.esp.wrapping_add(4 + 0xC);
}

/// The state's program pair and variant (state +0x1CC, +0x1D0, +0x169),
/// set with +0x158/+0x15A and +0x154 bits 0-1 when any differs.
fn set_program(m: &mut dyn Mem, pg: Pages, state: u32, variant: u8) {
    const PROGRAM: u32 = 0x17E1_5A66;
    const PAIR: u32 = 0x53E9_BA56;
    let current = pg.u32(m, state.wrapping_add(0x1CC)) == PROGRAM
        && pg.u8(m, state.wrapping_add(0x169)) == variant
        && pg.u32(m, state.wrapping_add(0x1D0)) == PAIR;
    if current {
        return;
    }
    pg.set_u32(m, state.wrapping_add(0x1D0), PAIR);
    pg.set_u16(m, state.wrapping_add(0x158), 0xA66);
    pg.set_u8(m, state.wrapping_add(0x169), variant);
    let dirty = pg.u32(m, state.wrapping_add(0x154)) | 3;
    pg.set_u32(m, state.wrapping_add(0x154), dirty);
    pg.set_u16(m, state.wrapping_add(0x15A), variant as u16);
    pg.set_u32(m, state.wrapping_add(0x1CC), PROGRAM);
}

/// State +0x154 takes `this` +0x14's bit 8 (as bit 20), bits 9-12 (as
/// 3-6), bits 14-21 (as 11-18) and bit 13 (as 19), then bit 0; written
/// after each step, `this` +0x14 read before each, as the original.
fn set_pass_flags(m: &mut dyn Mem, pg: Pages, this: u32, state: u32) {
    let source = this.wrapping_add(0x14);
    let flags = state.wrapping_add(0x154);
    let v = pg.u32(m, flags);
    let v = v ^ ((pg.u32(m, source) << 12 ^ v) & 0x10_0000);
    pg.set_u32(m, flags, v);
    let v = pg.u32(m, source) >> 6 & 0x78 | pg.u32(m, flags) & 0xFFFF_F807;
    pg.set_u32(m, flags, v);
    let v = ((pg.u32(m, source) >> 3 ^ v) & 0x7_F800) ^ v;
    pg.set_u32(m, flags, v);
    let v = (((pg.u32(m, source) << 6 ^ v) & 0x8_0000) ^ v) | 1;
    pg.set_u32(m, flags, v);
}

// Translated from 0x01054700 (DDDA.exe 2017-12-13 Steam), FUN_01054700.
/// `this->update_elements(list, count, flags)` (thiscall, `ret 0xc`).
///
/// `this[0x364 / 4]` is a bank of 0xA0-byte elements; `list` holds `count`
/// byte indices into it. Each listed element that is enabled (+0x90 bit 5
/// clear) and, when `flags` bit 0 asks for marked elements only, marked
/// (+0x96 bit 0) gets:
/// - the handler `this`'s mode word (+0x378) selects, called with the
///   element (thiscall on `this`, `ret 4`); none in one mode;
/// - unless `flags` bit 1 is set or `this` +0x37B bit 0 is clear: the
///   element's object's (+4, if any) method +0x14, called with (element,
///   `this`) (`ret 8`).
/// First, if `this` +0xE19 is set, 0x01054690 runs (thiscall, no arguments)
/// and the flag is cleared. Two modes also set `this` +0x108 bit 6.
///
/// The callees see what the original's calls show them: the same stack
/// layout and return addresses, ECX, and the callee-saved registers the
/// original holds its loop state in (EDI `this`, EBX the handler, EBP the
/// position, ESI the element). After each call the loop reads again what a
/// callee may change (those registers, the bank pointer, the list, the
/// elements, its stack slots), and EAX ends as the original leaves it.
pub fn update_elements(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let this = c.ecx;
    push(m, pg, c, c.ecx);
    let flags = pg.u8(m, c.esp.wrapping_add(0x10));
    push(m, pg, c, c.ebx);
    push(m, pg, c, c.ebp);
    push(m, pg, c, c.edi);
    c.edi = this;
    // two stack slots of the original's frame: whether to call the element
    // objects (written over the flags argument), and marked elements only
    let object_calls = if flags & 2 != 0 { 0 } else { pg.u8(m, this.wrapping_add(0x37B)) & 1 };
    let marked_only = flags & 1;
    pg.set_u8(m, c.esp.wrapping_add(0x1C), object_calls);
    pg.set_u8(m, c.esp.wrapping_add(0xF), marked_only);
    c.eax = c.eax & !0xFF | marked_only as u32;
    if pg.u8(m, this.wrapping_add(0xE19)) != 0 {
        c.ecx = this;
        call_guest(m, pg, c, 0x0105_4690, 0x0105_4738);
        pg.set_u8(m, c.edi.wrapping_add(0xE19), 0);
    }
    c.eax = pg.u32(m, c.edi.wrapping_add(0x378));
    c.ebx = element_handler(m, pg, c.edi, c.eax);
    c.ebp = 0;
    if pg.u32(m, c.esp.wrapping_add(0x18)) != 0 {
        push(m, pg, c, c.esi);
        loop {
            let list = pg.u32(m, c.esp.wrapping_add(0x18));
            c.eax = pg.u8(m, list.wrapping_add(c.ebp)) as u32;
            let bank = pg.u32(m, c.edi.wrapping_add(0x364));
            c.esi = bank.wrapping_add(c.eax.wrapping_mul(0xA0));
            let element = c.esi;
            let enabled = pg.u8(m, element.wrapping_add(0x90)) & 0x20 == 0;
            let wanted = pg.u8(m, c.esp.wrapping_add(0x13)) == 0 || pg.u8(m, element.wrapping_add(0x96)) & 1 != 0;
            if enabled && wanted {
                if c.ebx != 0 {
                    push(m, pg, c, element);
                    c.ecx = c.edi;
                    call_guest(m, pg, c, c.ebx, 0x0105_47D6);
                }
                let object_calls = pg.u8(m, c.esp.wrapping_add(0x20)) != 0;
                if object_calls && pg.u32(m, c.esi.wrapping_add(4)) != 0 {
                    let object = pg.u32(m, c.esi.wrapping_add(4));
                    c.ecx = object;
                    c.edx = pg.u32(m, object);
                    c.eax = pg.u32(m, c.edx.wrapping_add(0x14));
                    push(m, pg, c, c.edi);
                    push(m, pg, c, c.esi);
                    call_guest(m, pg, c, c.eax, 0x0105_47EF);
                }
            }
            c.ebp = c.ebp.wrapping_add(1);
            if c.ebp >= pg.u32(m, c.esp.wrapping_add(0x1C)) {
                break;
            }
        }
        c.esi = pop(m, pg, c);
    }
    c.edi = pop(m, pg, c);
    c.ebp = pop(m, pg, c);
    c.ebx = pop(m, pg, c);
    c.ecx = pop(m, pg, c);
    c.esp = c.esp.wrapping_add(4 + 12);
}

/// The handler `this`'s mode word selects for each element (0: none);
/// two modes mark `this` (+0x108 bit 6).
fn element_handler(m: &mut dyn Mem, pg: Pages, this: u32, mode: u32) -> u32 {
    let mark = |m: &mut dyn Mem| {
        let at = this.wrapping_add(0x108);
        let v = pg.u32(m, at) | 0x40;
        pg.set_u32(m, at, v);
    };
    if mode & 0x0800_0000 != 0 {
        0
    } else if mode & 0x0400_0000 != 0 {
        0x0104_EF10
    } else if mode & 0x40 == 0 {
        0x0105_1B60
    } else if mode & 0x0010_0000 != 0 {
        mark(m);
        0x0105_1B60
    } else {
        let handler = if mode & 0x4000 != 0 { 0x0105_33A0 } else { 0x0105_2330 };
        mark(m);
        handler
    }
}

/// Push `value` on the guest stack.
fn push(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, value: u32) {
    c.esp = c.esp.wrapping_sub(4);
    pg.set_u32(m, c.esp, value);
}

/// Pop the guest stack's top word.
fn pop(m: &mut dyn Mem, pg: Pages, c: &mut Cpu) -> u32 {
    let value = pg.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    value
}

/// Call guest function `target` as the original's `call` returning to
/// `return_to` does (its arguments pushed by the caller).
fn call_guest(m: &mut dyn Mem, pg: Pages, c: &mut Cpu, target: u32, return_to: u32) {
    push(m, pg, c, return_to);
    ddnative_base::x86::call_ext(m, c, target);
}
