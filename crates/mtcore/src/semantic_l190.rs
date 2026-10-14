//! L190: ports of the per-unit draw leaves that the shadow job's units run
//! after the base model draw (0x00EA3130). Same ABI-level conventions as
//! `semantic_p2c.rs`; oracles in `crates/mtcore/tests/semantic_l190.rs`
//! (vectors from `research/native/record_l190_*.py`).
use crate::semantic::{call_from, call_guest};
use crate::semantic_s10::StateSlots;
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

fn guest(target: u32) -> impl FnOnce(&mut dyn Mem, &mut Cpu) {
    move |m, c| call_guest(m, c, target)
}

/// Push an undo-log entry {code, a, b} when the context's log is on
/// (`context + 0x5c` bit 7), as the original's read-modify-write sequences
/// leave it: word 0 is exactly `word0`, then the count grows. `words` are
/// stored at +4 and +8 of the entry when given.
#[inline]
fn log_entry(m: &mut dyn Mem, slots: &StateSlots, word0: u32, words: [Option<u32>; 2]) {
    let n = slots.field(m, 0x6c);
    let entry = n.wrapping_mul(12).wrapping_add(0x30c7c);
    slots.set_field(m, entry, word0);
    if let Some(v) = words[0] {
        slots.set_field(m, entry.wrapping_add(4), v);
    }
    if let Some(v) = words[1] {
        slots.set_field(m, entry.wrapping_add(8), v);
    }
    slots.set_field(m, 0x6c, n.wrapping_add(1));
}

// Translated from 0x00fa5860 (DDDA.exe 2017-12-13 Steam), uSwingModel::vf11.
/// 0x00FA5860, `model->draw_unit(context)` for swing models (thiscall, `ret 4`;
/// void): the base draw 0x00EA3130, then the context's pending slots are
/// retired: the resource at 0x1dd0 (with its tag at 0x1dd4) and the three
/// plain slots 0x1c0 / 0x1c4 / 0x1c8 are undo-logged (when the log is on)
/// and cleared, marking the context dirty; the three cleared-field groups
/// {0x13c, 0x128, byte 0x135}, {0x140, 0x12c, 0x136}, {0x144, 0x130, 0x137}
/// reset together (and set the flag at 0x150) when any member is set.
pub fn swing_model_draw_unit(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let context = pg.u32(m, entry.wrapping_add(4));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original pushed EBX, ESI and EDI before calling the base draw
    call_from(m, c, pg, entry.wrapping_sub(12), this, &[context], 0x00fa_586d, guest(0x00ea_3130));
    let slots = StateSlots { pg, model: context };
    let logging = |m: &dyn Mem| pg.u8(m, context.wrapping_add(0x5c)) & 0x80 != 0;

    if slots.field(m, 0x1dd0) != 0 {
        if logging(m) {
            let (value, tag) = (slots.field(m, 0x1dd0), slots.field(m, 0x1dd4));
            log_entry(m, &slots, 0x37b, [Some(value), Some(tag)]);
        }
        slots.set_field(m, 0x1dd0, 0);
        if slots.field(m, 0x1dd4) != 0 {
            slots.set_field(m, 0x1dd4, 0);
            slots.mark_dirty(m, 2);
        }
        slots.mark_dirty(m, 1);
    }
    for (code, slot) in [(1u32, 0x1c0u32), (2, 0x1c4), (3, 0x1c8)] {
        let value = slots.field(m, slot);
        if value == 0 {
            continue;
        }
        if logging(m) {
            log_entry(m, &slots, 0x8000_0000 | code, [None, Some(value)]);
        }
        slots.set_field(m, slot, 0);
        slots.mark_dirty(m, 3);
    }
    for (word_a, word_b, byte) in [(0x13cu32, 0x128u32, 0x135u32), (0x140, 0x12c, 0x136), (0x144, 0x130, 0x137)] {
        if slots.field(m, word_a) != 0 || slots.field(m, word_b) != 0 || pg.u8(m, context.wrapping_add(byte)) != 0 {
            slots.set_field(m, word_b, 0);
            pg.set_u8(m, context.wrapping_add(byte), 0);
            slots.set_field(m, word_a, 0);
            slots.set_field(m, 0x150, 1);
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(8);
}

/// Fill `bytes` guest bytes at `at` with `value` (the CRT `memset` 0x012EFE20),
/// whole pages through the page table where it serves them.
fn fill_guest(m: &mut dyn Mem, pg: Pages, at: u32, value: u8, bytes: u32) {
    use ddnative_base::mem::{PAGE, WRITE};
    let mut done = 0u32;
    while done < bytes {
        let to = at.wrapping_add(done);
        let n = (bytes - done).min(PAGE - to % PAGE);
        match pg.bytes(to, n as usize, WRITE) {
            // SAFETY: `n` bytes inside one backed page
            Some(p) => unsafe { std::ptr::write_bytes(p, value, n as usize) },
            None => (0..n).for_each(|k| pg.set_u8(m, to.wrapping_add(k), value)),
        }
        done += n;
    }
}

// Translated from 0x00b8b800 (DDDA.exe 2017-12-13 Steam), a helper of 0x00B8B430.
/// 0x00B8B800, rebuild a player model's skin joint remap (ESI = model, plain
/// `ret`; AL = 1, the rest of EAX is the allocation address as the
/// original's `memcpy` leaves it). The joint table at `this[0x364]` has
/// `this[0x360]` records of 0xa0 bytes: byte +0x90 flags (bit 0 = the joint
/// is skinned; bit 4 is set on a joint without bit 0), byte +0x91 the parent.
/// 1. `this + 0x4f40` becomes the identity map;
/// 2. each skinned joint with a parent follows its skinned parent chain to
///    the last skinned ancestor and points every joint up the chain at it,
///    until a joint already points there (each change counts as a merge);
/// 3. the table's size is `count - merges`; `this + 0x5040` receives a copy
///    of the old compact table (`+0x4d40`), which is cleared with the index
///    table (`+0x4e40`) and rebuilt from the bit 4 joints;
/// 4. the allocator (global 0x01876658; virtual +0x24 frees, +0x20 allocates
///    `count * 64` bytes aligned 16) replaces the pose buffer (`+0x5700`) by
///    a copy of the mesh's matrices (`mesh[0x6c]`), and `+0x56d0` gets bit 3.
pub fn rebuild_skin_remap(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.esi;
    let saved = (c.ebx, c.ebp, c.edi);
    let count = pg.u32(m, this.wrapping_add(0x360));
    let map = this.wrapping_add(0x4f40);
    let joint = |base: u32, k: u32| base.wrapping_add(k.wrapping_mul(0xa0));
    // 1. identity (a byte counter, as the original's AL)
    if count != 0 {
        let mut al = 0u8;
        loop {
            pg.set_u8(m, map.wrapping_add(u32::from(al)), al);
            al = al.wrapping_add(1);
            if u32::from(al) >= count {
                break;
            }
        }
    }
    // 2. merge skinned chains
    let mut merges = 0u32;
    for i in 0..count {
        let base = pg.u32(m, this.wrapping_add(0x364));
        let node = joint(base, i);
        let flags = pg.u32(m, node.wrapping_add(0x90));
        if flags & 1 == 0 {
            pg.set_u8(m, node.wrapping_add(0x90), flags as u8 | 0x10);
        }
        if pg.u8(m, node.wrapping_add(0x91)) == 0xff || pg.u8(m, node.wrapping_add(0x90)) & 1 == 0 {
            continue;
        }
        let base = pg.u32(m, this.wrapping_add(0x364));
        let mut node = joint(base, i);
        let mut top = u32::from(pg.u8(m, node.wrapping_add(0x91)));
        let mut at = joint(base, top);
        while at != 0 && pg.u8(m, at.wrapping_add(0x90)) & 1 != 0 {
            let parent = u32::from(pg.u8(m, at.wrapping_add(0x91)));
            if parent == 0xff {
                break;
            }
            top = parent;
            at = joint(base, parent);
        }
        if node == 0 {
            continue;
        }
        let mut slot = i;
        while pg.u8(m, map.wrapping_add(slot)) != top as u8 {
            pg.set_u8(m, map.wrapping_add(slot), top as u8);
            slot = u32::from(pg.u8(m, node.wrapping_add(0x91)));
            merges = merges.wrapping_add(1);
            if slot == 0xff {
                break;
            }
            node = joint(pg.u32(m, this.wrapping_add(0x364)), slot);
            if pg.u8(m, node.wrapping_add(0x90)) & 1 == 0 {
                break;
            }
        }
    }
    // 3. the compact tables
    let live = pg.u32(m, this.wrapping_add(0x360)).wrapping_sub(merges);
    pg.set_u32(m, this.wrapping_add(0x4d34), live);
    let compact = this.wrapping_add(0x4d40);
    let index = this.wrapping_add(0x4e40);
    crate::semantic::copy_guest(m, pg, this.wrapping_add(0x5040), compact, live);
    let live = pg.u32(m, this.wrapping_add(0x4d34));
    fill_guest(m, pg, compact, 0, live);
    let total = pg.u32(m, this.wrapping_add(0x360));
    fill_guest(m, pg, index, 0, total);
    let mut next = 0u32;
    let mut i = 0u32;
    while i < pg.u32(m, this.wrapping_add(0x360)) {
        let base = pg.u32(m, this.wrapping_add(0x364));
        if pg.u8(m, joint(base, i).wrapping_add(0x90)) & 0x10 == 0 {
            pg.set_u8(m, index.wrapping_add(i), 0);
        } else {
            let old = pg.u8(m, map.wrapping_add(i));
            pg.set_u8(m, compact.wrapping_add(next), old);
            pg.set_u8(m, index.wrapping_add(i), next as u8);
            next += 1;
        }
        i += 1;
    }
    // 4. a fresh pose buffer: the original's frame is 0xc locals and EBX, EBP, EDI
    let frame = entry.wrapping_sub(24);
    let mesh = pg.u32(m, this.wrapping_add(0xf0));
    let matrices = pg.u32(m, mesh.wrapping_add(0x6c));
    let heap = pg.u32(m, 0x0187_6658);
    let release = pg.u32(m, pg.u32(m, heap).wrapping_add(0x24));
    let old = pg.u32(m, this.wrapping_add(0x5700));
    call_from(m, c, pg, frame, heap, &[old], 0x00b8_b9d4, guest(release));
    let heap = pg.u32(m, 0x0187_6658);
    let allocate = pg.u32(m, pg.u32(m, heap).wrapping_add(0x20));
    let bytes = count << 6;
    let buffer = call_from(m, c, pg, frame, heap, &[bytes, 0x10], 0x00b8_b9e9, guest(allocate));
    pg.set_u32(m, this.wrapping_add(0x5700), buffer);
    crate::semantic::copy_guest(m, pg, buffer, matrices, bytes);
    let flags = pg.u32(m, this.wrapping_add(0x56d0));
    pg.set_u32(m, this.wrapping_add(0x56d0), flags | 8);
    (c.ebx, c.ebp, c.edi) = saved;
    c.eax = (buffer & !0xff) | 1;
    c.esp = entry.wrapping_add(4);
}
