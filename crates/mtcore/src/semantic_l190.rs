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

/// The CVTTSS2SI of a float: truncation; NaN and out-of-range give 0x80000000.
#[inline]
fn truncate(x: f32) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x < -2147483648.0 {
        i32::MIN
    } else {
        x as i32
    }
}

// Translated from 0x00b89790 (DDDA.exe 2017-12-13 Steam), uPlayerBase::vf208.
/// 0x00B89790, `player->draw_unit_parts(context, record)` (thiscall, `ret 8`;
/// void): the player's per-unit draw, the analogue of the base model draw
/// 0x00EA3130 with its parts (skin, weapon, attachments) drawn either as
/// ordinary parts (`record` zero: 0x00B89FE0) or as records (nonzero: the
/// part's virtual +0x24 test, then 0x007FFCC0).
///
/// Nothing happens when the model's intensity (`this[0x158]`) equals the
/// constant at 0x01521690, the visibility mask (0x00EA1EE0) is negative, or
/// the context flags (u16 `context[0x16a]`) have bit 5 and the view depth
/// (bounds against the view matrix 0x00FED940, negated) is beyond
/// `this[0xd84]`. Otherwise: virtual +0x64(context), the shadow state
/// (0x00EA1FD0), the detail level (model bits 9..12 as signed 4 bits; when
/// negative, the distance to the context's eye (+0x184) against the mesh's
/// limits +0xe0 / +0xe4, scaled by the global mode at [0x018D1D20]+0x38), the
/// depth key `(0x7fff - clamp(depth + this[0x154] + this[0x16c])) << 12`, the
/// bias `clamp(depth + this[0x16c])` and a flag from [0x018FA4BC]+0xbe448 /
/// `this[0x5820]` go to the parts: the four slots at +0x57d0 when the
/// context's pass object (+0x1410) is set and flags have 0x104; else the
/// body (virtual +0x198 and 0x00FFC350, or the embedded object at +0x5140),
/// then the part lists at +0x5738 and +0x5798 selected by their bit masks.
/// The state-slot tail is the base model draw's.
pub fn player_draw_parts(m: &mut dyn Mem, c: &mut Cpu) {
    use crate::semantic_p2c::{fresh_parameter_block, model_bounds, select_shadow_state, visibility_mask};
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let context = pg.u32(m, entry.wrapping_add(4));
    let param = pg.u8(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(12);
    };
    let f = |bits: u32| f32::from_bits(bits);
    // the frame: EBP = entry - 4, ESP aligned to 16, 0x34 locals, EBX, ESI, EDI pushed
    let s = (entry.wrapping_sub(4) & !15).wrapping_sub(0x34).wrapping_sub(12);
    if f(pg.u32(m, this.wrapping_add(0x158))) == f(pg.u32(m, 0x0152_1690)) {
        return finish(c);
    }
    let mask = visibility_mask(m, c, pg, s, this, context);
    if mask < 0 {
        return finish(c);
    }
    let bounds = s.wrapping_add(0x30);
    model_bounds(m, c, pg, s, this, bounds);
    let cf = u32::from(pg.u16(m, context.wrapping_add(0x16a)));
    let view = call_from(m, c, pg, s, context, &[], 0x00b8_97ea, guest(0x00fe_d940));
    let (bx, by, bz) = (f(pg.u32(m, bounds)), f(pg.u32(m, bounds.wrapping_add(4))), f(pg.u32(m, bounds.wrapping_add(8))));
    let mut depth = bx * f(pg.u32(m, view.wrapping_add(8)));
    depth += by * f(pg.u32(m, view.wrapping_add(0x18)));
    depth += bz * f(pg.u32(m, view.wrapping_add(0x28)));
    depth += f(pg.u32(m, view.wrapping_add(0x38)));
    let depth = f(depth.to_bits() ^ pg.u32(m, 0x0161_c280));
    if cf & 0x20 != 0 && depth > f(pg.u32(m, this.wrapping_add(0xd84))) {
        return finish(c);
    }
    let method = pg.u32(m, pg.u32(m, this).wrapping_add(0x64));
    call_from(m, c, pg, s, this, &[context], 0x00b8_9843, guest(method));
    let state = select_shadow_state(m, c, pg, this, context, s);
    let (bx, by, bz) = (f(pg.u32(m, bounds)), f(pg.u32(m, bounds.wrapping_add(4))), f(pg.u32(m, bounds.wrapping_add(8))));
    let dx = bx - f(pg.u32(m, context.wrapping_add(0x184)));
    let dy = by - f(pg.u32(m, context.wrapping_add(0x188)));
    let dz = bz - f(pg.u32(m, context.wrapping_add(0x18c)));
    let mesh = pg.u32(m, this.wrapping_add(0xf0));
    let detail = (pg.u32(m, this.wrapping_add(0x108)) << 19) as i32 >> 28;
    let distance = truncate((dy * dy + dx * dx + dz * dz).sqrt());
    let level: u32 = if detail >= 0 {
        detail as u32
    } else {
        let mode = pg.u32(m, pg.u32(m, 0x018d_1d20).wrapping_add(0x38));
        if mode == 3 {
            1
        } else {
            let scale: i32 = if mode == 2 { 2 } else { 1 };
            let far = (pg.u32(m, mesh.wrapping_add(0xe4)) as i32).wrapping_mul(scale);
            if distance > far {
                4
            } else {
                let near = (pg.u32(m, mesh.wrapping_add(0xe0)) as i32).wrapping_mul(scale);
                u32::from(distance > near) + 1
            }
        }
    };
    let bias = f(pg.u32(m, this.wrapping_add(0x16c)));
    let key = truncate(((pg.u32(m, this.wrapping_add(0x154)) as i32 as f32) + depth) + bias).clamp(0, 0x7fff);
    let a = ((0x7fff - key) as u32) << 12;
    let b = truncate(bias + depth).clamp(0, 0x07ff_ffff) as u32;
    let global = pg.u32(m, 0x018f_a4bc);
    let equal = pg.u32(m, global.wrapping_add(0xbe448)) == 1;
    let flag3 = match pg.u32(m, this.wrapping_add(0x5820)) {
        1 => 0,
        2 => 1,
        _ => u32::from(equal),
    };
    let skin = cf & 1 != 0;
    let record = param != 0;
    let part = |m: &mut dyn Mem, c: &mut Cpu, obj: u32, back: u32| {
        // 0x00B89FE0(this, part, part[0x1c], part[0x20], level, a, b, flag); ESI = the context
        c.esi = context;
        let (x, y) = (pg.u32(m, obj.wrapping_add(0x1c)), pg.u32(m, obj.wrapping_add(0x20)));
        call_from(m, c, pg, s, this, &[this, obj, x, y, level, a, b, flag3], back, guest(0x00b8_9fe0));
    };
    // 0x007FFCC0 takes the context in ESI and the part in EDI
    let submit = |m: &mut dyn Mem, c: &mut Cpu, obj: u32, x: u32, back: u32| {
        c.esi = context;
        c.edi = obj;
        call_from(m, c, pg, s, this, &[this, x, level, a, b, flag3], back, guest(0x007f_fcc0));
    };
    // `obj`'s virtual +0x24 (is it drawable); true when AL is set
    let drawable = |m: &mut dyn Mem, c: &mut Cpu, obj: u32, back: u32| {
        let method = pg.u32(m, pg.u32(m, obj).wrapping_add(0x24));
        call_from(m, c, pg, s, obj, &[], back, guest(method)) & 0xff != 0
    };
    // 0x00B8A770 takes the part in EDI
    let skin_prepare = |m: &mut dyn Mem, c: &mut Cpu, obj: u32, back: u32| {
        c.edi = obj;
        call_from(m, c, pg, s, this, &[this, context], back, guest(0x00b8_a770));
    };

    let special = cf & 0x104 != 0 && {
        let pass = pg.u32(m, context.wrapping_add(0x1410)) & !15;
        pass != 0 && pg.u32(m, pass) != 0
    };
    if special {
        let mut slot = this.wrapping_add(0x57d0);
        for _ in 0..4 {
            let obj = pg.u32(m, slot);
            if obj != 0 {
                if record {
                    let x = pg.u32(m, obj.wrapping_add(0x1c));
                    if drawable(m, c, obj, 0x00b8_9a1b) {
                        submit(m, c, obj, x, 0x00b8_9a3e);
                    }
                } else {
                    part(m, c, obj, 0x00b8_9a69);
                }
            }
            slot = slot.wrapping_add(4);
        }
    } else {
        if record {
            if skin {
                skin_prepare(m, c, this.wrapping_add(0x5140), 0x00b8_9a95);
            }
            let x = pg.u32(m, this.wrapping_add(0xf8));
            let embedded = this.wrapping_add(0x5140);
            if drawable(m, c, embedded, 0x00b8_9ab2) {
                submit(m, c, embedded, x, 0x00b8_9ad5);
            }
        } else {
            let method = pg.u32(m, pg.u32(m, this).wrapping_add(0x198));
            call_from(m, c, pg, s, this, &[context], 0x00b8_9ae4, guest(method));
            let bank = pg.u32(m, this.wrapping_add(0xf8));
            let mesh = pg.u32(m, this.wrapping_add(0xf0));
            call_from(m, c, pg, s, this, &[context, mesh, bank, bounds, mask as u32, state], 0x00b8_9b05, guest(0x00ff_c350));
        }
        let global = pg.u32(m, 0x018f_a4bc);
        if pg.u32(m, global.wrapping_add(0xb8844)) & 0x0600_0000 != 0 || cf & 0x208 == 0 {
            let count = pg.u32(m, this.wrapping_add(0x5788));
            if count != 0 {
                let mut k = 0u32;
                let mut slot = this.wrapping_add(0x5738);
                loop {
                    if pg.u32(m, this.wrapping_add(0x578c)) & (1u32 << (k & 31)) != 0 {
                        let obj = pg.u32(m, slot);
                        if obj != 0 {
                            if skin {
                                skin_prepare(m, c, obj, if record { 0x00b8_9b7f } else { 0x00b8_9c15 });
                            }
                            let obj = pg.u32(m, slot);
                            if record {
                                let x = pg.u32(m, obj.wrapping_add(0x1c));
                                if drawable(m, c, obj, 0x00b8_9b95) {
                                    submit(m, c, obj, x, 0x00b8_9bb8);
                                }
                            } else {
                                part(m, c, obj, 0x00b8_9c3e);
                            }
                        }
                    }
                    k += 1;
                    slot = slot.wrapping_add(4);
                    if k >= pg.u32(m, this.wrapping_add(0x5788)) {
                        break;
                    }
                }
            }
            let count = pg.u32(m, this.wrapping_add(0x57b0));
            if count != 0 {
                let mut k = 0u32;
                let mut slot = this.wrapping_add(0x5798);
                loop {
                    if pg.u32(m, this.wrapping_add(0x57b4)) & (1u32 << (k & 31)) != 0 {
                        let obj = pg.u32(m, slot);
                        if obj != 0 {
                            if skin {
                                skin_prepare(m, c, obj, 0x00b8_9ca4);
                            }
                            let obj = pg.u32(m, slot);
                            if record {
                                let x = pg.u32(m, obj.wrapping_add(0x1c));
                                if drawable(m, c, obj, 0x00b8_9cc0) {
                                    submit(m, c, obj, x, 0x00b8_9ce3);
                                }
                            } else {
                                part(m, c, obj, 0x00b8_9d0e);
                            }
                        }
                    }
                    k += 1;
                    slot = slot.wrapping_add(4);
                    if k >= pg.u32(m, this.wrapping_add(0x57b0)) {
                        break;
                    }
                }
            }
        }
    }
    // the base model draw's state-slot tail
    let slots = StateSlots { pg, model: context };
    slots.set_value(m, 0x2e08, 0x2c10, 0x582, 0x582);
    slots.set_resource(m, 0x1d90, 0, 0x373, 0);
    if (pg.u32(m, this.wrapping_add(0x108)) >> 0x1e) & 1 != 0 {
        slots.set_value(m, 0x2eb0, 0x2cc8, 0x597, 0x950a_6599);
        fresh_parameter_block(m, c, pg, s, &slots, 0x00b8_9f8e);
    }
    finish(c);
}

// Translated from 0x00b8a770 (DDDA.exe 2017-12-13 Steam), a helper of 0x00B89790.
/// 0x00B8A770, `prepare_part_blocks(model, context)` (stdcall, `ret 8`; EDI =
/// the part): when the model has a block sink (`model[0x22b8]` and a nonzero
/// `model[0x22bc]`), every entry k of the part's list (`part[0x1c]`, count
/// `part[0x24]`) is applied with 0x00C85A00(sink, entry, context); where the
/// global mode ([0x018FA4BC]+0xbe448 == 1 or `model[0x5820]` == 2) and the
/// alternate list `part[0x20]` has a nonzero entry, that one is used.
pub fn prepare_part_blocks(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let part = c.edi;
    let model = pg.u32(m, entry.wrapping_add(4));
    let context = pg.u32(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.esi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.esi) = saved;
        c.edi = part;
        c.esp = entry.wrapping_add(12);
    };
    let sink = pg.u32(m, model.wrapping_add(0x22b8));
    if sink == 0 || pg.u32(m, model.wrapping_add(0x22bc)) == 0 || pg.u32(m, part.wrapping_add(0x24)) == 0 {
        return finish(c);
    }
    // the original pushed ECX, EBX and ESI
    let frame = entry.wrapping_sub(12);
    let mut k = 0u32;
    loop {
        let global = pg.u32(m, 0x018f_a4bc);
        let mode = pg.u32(m, global.wrapping_add(0xbe448)) == 1;
        let mut chosen = pg.u32(m, pg.u32(m, part.wrapping_add(0x1c)).wrapping_add(k * 4));
        if mode || pg.u32(m, model.wrapping_add(0x5820)) == 2 {
            let alternate = pg.u32(m, pg.u32(m, part.wrapping_add(0x20)).wrapping_add(k * 4));
            if alternate != 0 {
                chosen = alternate;
            }
        }
        if chosen != 0 {
            call_from(m, c, pg, frame, 0, &[sink, chosen, context], 0x00b8_a7da, guest(0x00c8_5a00));
        }
        k += 1;
        if k >= pg.u32(m, part.wrapping_add(0x24)) {
            break;
        }
    }
    finish(c);
}

// Translated from 0x00b89fe0 (DDDA.exe 2017-12-13 Steam), a helper of 0x00B89790.
/// 0x00B89FE0, `draw_part(model, part, table, alternates, levels, a, b,
/// use_alternates)` (stdcall, `ret 0x20`; ESI = the draw context): draw the
/// pass records (0x30 bytes from `part[8][0x70]`, count `[0x74]`) that are
/// visible to the model (bit tests in `model + 0x110`), flagged visible
/// (byte +4 bit 0), match `levels` and the context's mask (u16 +0x16a).
/// A record's group (bits 12..23) selects its resource from `table` (or
/// `alternates` when `use_alternates` and `alternates` is set); a group change
/// closes the previous group (0x00FF14D0) and opens the new one (0x00FED190
/// and the context's state: block +0x1a80 with the part's constants, +0x1a88
/// from `part + 0x170`). Per record 0x00EA23F0 binds the material, the
/// context's sort word (+0x164) is built from the record's byte, a pass offset
/// (`a`, `b` or, for weighted records of resource kinds 0x10 / 0xe, the
/// view depth of the record's joint position), 0x00FEEF20 commits, and a
/// 32-byte command with its sort key is queued unless the context is
/// recording (flags +0x5c bits 4 and 6 without bit 8); 0x00FF1920 ends it.
/// Finally the shader slot +0x30a0 is rebound.
pub fn player_draw_part(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let context = c.esi;
    let arg = |m: &dyn Mem, k: u32| pg.u32(m, entry.wrapping_add(4 + 4 * k));
    let (this, part, table, alternates) = (arg(m, 0), arg(m, 1), arg(m, 2), arg(m, 3));
    let (levels, a, b, mut use_alternates) = (arg(m, 4) as u8, arg(m, 5), arg(m, 6), arg(m, 7) as u8);
    let saved = (c.ebx, c.ebp, c.edi);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.edi) = saved;
        c.esi = context;
        c.esp = entry.wrapping_add(4 + 0x20);
    };
    // EBP = entry - 4, ESP aligned, 0x48 locals, EBX and EDI pushed
    let s = (entry.wrapping_sub(4) & !15).wrapping_sub(0x48).wrapping_sub(8);
    let f = |bits: u32| f32::from_bits(bits);
    let slots = StateSlots { pg, model: context };
    let sub = pg.u32(m, part.wrapping_add(8));
    let count = pg.u32(m, sub.wrapping_add(0x74));
    let records = pg.u32(m, sub.wrapping_add(0x70));
    let thirty = pg.u32(m, sub.wrapping_add(0xa0));
    let mask = u32::from(pg.u16(m, context.wrapping_add(0x16a)));
    let resource = pg.u32(m, sub.wrapping_add(0x9c));
    if slots.field(m, 0x14c) != resource {
        let stamp = pg.u32(m, 0x018d_2ce8).wrapping_add(1);
        pg.set_u32(m, resource.wrapping_add(4), stamp);
        slots.set_field(m, 0x14c, resource);
        slots.set_field(m, 0x150, 1);
    }
    let mut last: i32 = -1;
    if alternates == 0 {
        use_alternates = 0;
    }
    let bias: u32 = if pg.u8(m, part.wrapping_add(0x570)) != 0 { 10 } else { 0 };
    if count != 0 {
        let mut p = records.wrapping_add(4);
        for _ in 0..count {
            let word = pg.u32(m, p);
            let index = word & 0xfff;
            let visible = pg.u32(m, this.wrapping_add(0x110).wrapping_add((index >> 5) * 4)) & (1u32 << (index & 31)) != 0
                && pg.u8(m, p.wrapping_add(4)) & 1 != 0
                && levels & ((word >> 24) as u8) != 0
                && (pg.u32(m, p.wrapping_sub(4)) & mask) & 0xffff != 0;
            if visible {
                let group = (pg.u32(m, p) >> 12) & 0xfff;
                let current = pg.u32(m, table.wrapping_add(group * 4));
                if last != group as i32 {
                    if last >= 0 {
                        call_from(m, c, pg, s, context, &[], 0x00b8_a102, guest(0x00ff_14d0));
                    }
                    last = group as i32;
                    let mut chosen = current;
                    if use_alternates != 0 {
                        let other = pg.u32(m, alternates.wrapping_add(group * 4));
                        if other != 0 {
                            chosen = other;
                        }
                    }
                    call_from(m, c, pg, s, context, &[chosen, 0x18], 0x00b8_a133, guest(0x00fe_d190));
                    slots.set_value(m, 0x30a0, 0x5268, 0x5d5, 0xc687_5a4d);
                    let flags = slots.field(m, 0x5c);
                    slots.set_field(m, 0x5c, flags | 2);
                    slots.log_old(m, 0x1a80, 0x311);
                    let descriptor = pg.u32(m, slots.field(m, 0x18).wrapping_add(0xc44));
                    let size = (u32::from(pg.u16(m, descriptor.wrapping_add(0x18))).wrapping_mul(4).wrapping_add(0xf)) & !0xf;
                    if slots.field(m, 0x24).wrapping_add(size) > slots.field(m, 0x28) {
                        call_from(m, c, pg, s, context, &[size], 0x00b8_a25b, guest(0x00fe_d890));
                    }
                    let block = slots.field(m, 0x24);
                    slots.set_field(m, 0x24, block.wrapping_add(size));
                    slots.set_field(m, 0x1a80, block);
                    slots.mark_dirty(m, 1);
                    let part_constants = [pg.u32(m, sub.wrapping_add(0x100)), pg.u32(m, sub.wrapping_add(0x104)), pg.u32(m, sub.wrapping_add(0x108)), pg.u32(m, sub.wrapping_add(0xa4))];
                    let owner = pg.u32(m, this.wrapping_add(0xf0));
                    let owner_constants = [pg.u32(m, owner.wrapping_add(0x100)), pg.u32(m, owner.wrapping_add(0x104)), pg.u32(m, owner.wrapping_add(0x108))];
                    let inverse = (f(pg.u32(m, 0x0153_19b8)) / f(pg.u32(m, owner.wrapping_add(0xa4)))).to_bits();
                    if block != 0 {
                        for (k, v) in part_constants.into_iter().chain(owner_constants).chain([inverse]).enumerate() {
                            pg.set_u32(m, block.wrapping_add(4 * k as u32), v);
                        }
                    }
                    let flags = slots.field(m, 0x5c);
                    slots.set_field(m, 0x5c, flags & !2);
                    let mut target = part.wrapping_add(0x170);
                    if target == 0 {
                        target = pg.u32(m, slots.field(m, 0x20).wrapping_add(0x1890));
                    }
                    if !(slots.field(m, 0x1a88) & !0xf == target && slots.field(m, 0x1a8c) == 0) {
                        slots.log_old(m, 0x1a88, 0x312);
                        let tagged = slots.field(m, 0x1a8c) != 0;
                        slots.set_field(m, 0x1a88, target);
                        if tagged {
                            slots.set_field(m, 0x1a8c, 0);
                            slots.mark_dirty(m, 2);
                        }
                        slots.mark_dirty(m, 1);
                    }
                }
                call_from(m, c, pg, s, this, &[context, p.wrapping_sub(4)], 0x00b8_a3e0, guest(0x00ea_23f0));
                let owner_byte = u32::from(pg.u8(m, p.wrapping_add(6)));
                let key = pg.u32(m, p.wrapping_add(0xc));
                if slots.field(m, 0x138) != key || slots.field(m, 0x124) != thirty || u32::from(pg.u8(m, context.wrapping_add(0x134))) != owner_byte {
                    if thirty != 0 {
                        let stamp = pg.u32(m, 0x018d_2ce8).wrapping_add(1);
                        pg.set_u32(m, thirty.wrapping_add(4), stamp);
                    }
                    slots.set_field(m, 0x124, thirty);
                    pg.set_u8(m, context.wrapping_add(0x134), owner_byte as u8);
                    slots.set_field(m, 0x138, key);
                    slots.set_field(m, 0x150, 1);
                }
                let kind = (pg.u32(m, current.wrapping_add(0x14)) >> 0x1a) & 0x1f;
                if kind == 0x10 || kind == 0xe {
                    let pass = u32::from(pg.u8(m, p.wrapping_add(5)));
                    let sort = slots.field(m, 0x164);
                    if pg.u8(m, p.wrapping_add(4)) & 4 == 0 {
                        slots.set_field(m, 0x164, pass.wrapping_add(bias).wrapping_add(a).wrapping_shl(5) | (sort & 0x1f));
                    } else {
                        let weights = pg.u32(m, p.wrapping_add(0x28));
                        let (x, y, z) = (f(pg.u32(m, weights.wrapping_add(0x10))), f(pg.u32(m, weights.wrapping_add(0x14))), f(pg.u32(m, weights.wrapping_add(0x18))));
                        let row = pg.u32(m, this.wrapping_add(0x364)).wrapping_add(pg.u32(m, weights).wrapping_mul(0xa0));
                        let lane = |m: &dyn Mem, offset: u32, i: u32| f(pg.u32(m, row.wrapping_add(offset + 4 * i)));
                        let mut position = [0f32; 3];
                        for (i, out) in position.iter_mut().enumerate() {
                            let i = i as u32;
                            *out = ((lane(m, 0x20, i) * y + lane(m, 0x10, i) * x) + lane(m, 0x30, i) * z) + lane(m, 0x40, i);
                        }
                        let view = call_from(m, c, pg, s, context, &[], 0x00b8_a4e1, guest(0x00fe_d940));
                        let mut depth = f(pg.u32(m, view.wrapping_add(0x18))) * position[1];
                        depth += f(pg.u32(m, view.wrapping_add(8))) * position[0];
                        depth += f(pg.u32(m, view.wrapping_add(0x28))) * position[2];
                        depth += f(pg.u32(m, view.wrapping_add(0x38)));
                        let key = truncate((pg.u32(m, this.wrapping_add(0x154)) as i32 as f32) - depth).clamp(0, 0x7fff);
                        let offset = (((0x7fff - key) as u32) << 12).wrapping_add(bias);
                        let sort = slots.field(m, 0x164);
                        slots.set_field(m, 0x164, pass.wrapping_add(offset).wrapping_shl(5) | (sort & 0x1f));
                    }
                } else {
                    let sort = slots.field(m, 0x164);
                    slots.set_field(m, 0x164, (sort & 0x1f) | b.wrapping_shl(5));
                }
                call_from(m, c, pg, s, context, &[], 0x00b8_a59e, guest(0x00fe_ef20));
                let (w18, w1c, w14) = (pg.u32(m, p.wrapping_add(0x18)), pg.u32(m, p.wrapping_add(0x1c)), pg.u32(m, p.wrapping_add(0x14)));
                let flags = slots.field(m, 0x5c);
                if flags & 0x100 != 0 || flags & 0x50 == 0 {
                    if slots.field(m, 0x24).wrapping_add(0x20) > slots.field(m, 0x28) {
                        call_from(m, c, pg, s, context, &[0x20], 0x00b8_a5d5, guest(0x00fe_d890));
                    }
                    let command = slots.field(m, 0x24);
                    slots.set_field(m, 0x24, command.wrapping_add(0x20));
                    let recording = u32::from(slots.field(m, 0x5c) & 0x50 != 0);
                    let serial = slots.field(m, 0x74);
                    let low = (serial & 0x7fff) | 0x1_0000 | (recording << 15);
                    slots.set_field(m, 0x74, serial.wrapping_add(1));
                    let line = slots.field(m, 0x6c);
                    pg.set_u32(m, command, (line << 20) | low);
                    for (offset, field) in [(8u32, 0x194u32), (0xc, 0x198), (0x10, 0x19c), (4, 0x1a0)] {
                        let v = slots.field(m, field);
                        pg.set_u32(m, command.wrapping_add(offset), v);
                    }
                    pg.set_u32(m, command.wrapping_add(0x1c), w1c);
                    pg.set_u32(m, command.wrapping_add(0x14), w18);
                    pg.set_u32(m, command.wrapping_add(0x18), w14);
                    let n = slots.field(m, 0x48);
                    let entry_at = slots.field(m, 0x44).wrapping_add(n.wrapping_mul(8));
                    slots.set_field(m, 0x48, n.wrapping_add(1));
                    let sort = slots.field(m, 0x164);
                    let key = ((sort >> 5).wrapping_add(slots.field(m, 0x178)) & 0x07ff_ffff) | (sort << 27);
                    pg.set_u32(m, entry_at, key);
                    pg.set_u32(m, entry_at.wrapping_add(4), command);
                }
                call_from(m, c, pg, s, context, &[], 0x00b8_a6a3, guest(0x00ff_1920));
            }
            p = p.wrapping_add(0x30);
        }
        if last >= 0 {
            call_from(m, c, pg, s, context, &[], 0x00b8_a6c6, guest(0x00ff_14d0));
        }
    }
    slots.set_value(m, 0x30a0, 0x2ea8, 0x5d5, 0x5d5);
    finish(c);
}
