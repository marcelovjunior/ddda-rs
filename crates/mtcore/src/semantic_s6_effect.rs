//! L187 s6: semantic ports of the effect system's update routines (the
//! highest unported self times of the combo47 profile).
//!
//! - [`effect_update`], 0x00E0BED0: an effect's `update(dt)`: run its
//!   command queue through the virtual command handlers, then step its
//!   clock.
//! - [`effect_draw_children`], 0x00E75EA0 (`uEffect::vf11`): draw an
//!   effect's child list.
//!
//! Both are glue around virtual calls and callees that stay guest calls
//! (same ECX, arguments, stack layout and return addresses as the original);
//! what the port removes is the lifted code around them.
//!
//! Translated from 0x00E0BED0 and 0x00E75EA0 (DDDA.exe 2017-12-13 Steam).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{self, call_ext, Cpu};

use crate::semantic::call_from;

/// The guest function `slot` bytes into `object`'s vtable.
fn method(m: &dyn Mem, pg: Pages, object: u32, slot: u32) -> u32 {
    let vtable = pg.u32(m, object);
    pg.u32(m, vtable.wrapping_add(slot))
}

/// A thiscall of the virtual method `slot` of `object`.
fn call_virtual(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, object: u32, slot: u32, args: &[u32], back: u32) -> u32 {
    let target = method(m, pg, object, slot);
    call_from(m, c, pg, frame, object, args, back, |m, c| call_ext(m, c, target))
}

/// A thiscall of the lifted guest function at the fixed address `target`:
/// through the runtime's cached lookup, as the lifted code's own direct
/// calls go (not the slower path for untranslated code).
fn call_function(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, this: u32, target: u32, args: &[u32], back: u32) -> u32 {
    call_from(m, c, pg, frame, this, args, back, |m, c| x86::call_lifted(m, c, target))
}

/// 0x00E0BED0, `effect->update(dt)` (thiscall, `ret 4`, `dt` a float).
///
/// Only when the effect's flags (`+0x164`) bit 0 are set. The queue: a
/// loop that clears the "reset" flag (bit 8: `+0x134` = 0, `+0x138` = -1.0),
/// asks the virtual `current_command()` (+0x9c) for the head command and
/// hands it to the handler its kind (`command[1]`, 0..=5) selects among
/// virtual slots +0xa0, +0xa4, +0xac, +0xbc, +0xc0 and +0xc4 (any other kind
/// calls +0x70(0)); a handler answering 1 lets the loop go on, and the loop
/// also continues over commands that the next head command makes
/// redundant, tracked by the id of the last kind-4 command. Afterwards, if
/// the effect has a timeline object (`+0x100`): call its 0x01077240 with the
/// clock delta, build a 16-byte transform (the effect's, or the model
/// parent's `+0x70/+0x74`, or the global at 0x018D08F4 `+0xb8/+0xbc`;
/// zeros when bit 10 is set, after the virtual calls +0x50 and +0x54 when
/// bit 11 is too), apply it with 0x010784B0 and clear bit 16 of the words
/// of the `+0x10c` objects at `+0x118`. Finally the clocks advance:
/// `+0x130 = +0x12c; +0x12c += dt; +0x138 = +0x134; +0x134 += dt`.
pub fn effect_update(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let done = |m: &mut dyn Mem, c: &mut Cpu| {
        let _ = m;
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    if pg.u8(m, this.wrapping_add(0x164)) & 1 == 0 {
        return done(m, c);
    }
    // SUB ESP, 0x10; ESI, EBX, EBP, EDI pushed
    let frame = entry.wrapping_sub(0x20);
    c.esi = this;
    let mut last_id = u32::MAX; // EBP
    loop {
        let flags = pg.u32(m, this.wrapping_add(0x164));
        if (flags >> 8) & 1 != 0 {
            pg.set_u32(m, this.wrapping_add(0x134), 0);
            let minus_one = pg.u32(m, 0x0161_c20c);
            pg.set_u32(m, this.wrapping_add(0x138), minus_one);
            pg.set_u32(m, this.wrapping_add(0x164), flags & 0xffff_feff);
        }
        let command = call_virtual(m, c, pg, frame, this, 0x9c, &[], 0xe0bf32);
        if command == 0 {
            break;
        }
        let kind = pg.u32(m, command.wrapping_add(4));
        if kind > 5 {
            call_virtual(m, c, pg, frame, this, 0x70, &[0], 0xe0bff3);
        } else {
            let slot = [0xa0, 0xa4, 0xac, 0xbc, 0xc0, 0xc4][kind as usize];
            let answer = call_virtual(m, c, pg, frame, this, slot, &[command], 0xe0bf5c);
            if answer & 0xff != 1 {
                break;
            }
        }
        // 0x00E0BF64
        if pg.u32(m, command.wrapping_add(4)) == 0 {
            continue;
        }
        let next = call_virtual(m, c, pg, frame, this, 0x9c, &[], 0xe0bf75);
        if next != 0 {
            let next_kind = pg.u32(m, next.wrapping_add(4));
            if next_kind == 4 && pg.u32(m, next) != last_id {
                continue;
            }
            if next_kind == 5 {
                continue;
            }
        }
        if pg.u32(m, command.wrapping_add(4)) != 4 {
            break;
        }
        if pg.u32(m, next.wrapping_add(4)) != 2 {
            break;
        }
        last_id = pg.u32(m, command);
    }
    // EDI popped
    c.esp = entry.wrapping_sub(0x1c);
    let frame = c.esp;
    let timeline = pg.u32(m, this.wrapping_add(0x100));
    if timeline != 0 {
        let delta = x86::subss(pg.u32(m, this.wrapping_add(0x12c)), pg.u32(m, this.wrapping_add(0x130)));
        call_function(m, c, pg, frame, timeline, 0x0107_7240, &[delta], 0xe0c022);
        let flags = pg.u32(m, this.wrapping_add(0x164));
        let local = entry.wrapping_sub(0x10);
        for k in 0..4 {
            pg.set_u32(m, local.wrapping_add(4 * k), 0);
        }
        if (flags >> 10) & 1 == 0 {
            let source = pg.u32(m, this.wrapping_add(0xfc));
            let (x, y) = if source != 0 && (flags >> 2) & 1 != 0 {
                (pg.u32(m, source.wrapping_add(0x70)), pg.u32(m, source.wrapping_add(0x74)))
            } else {
                let global = pg.u32(m, 0x018d_08f4);
                (pg.u32(m, global.wrapping_add(0xb8)), pg.u32(m, global.wrapping_add(0xbc)))
            };
            pg.set_u32(m, local.wrapping_add(8), x);
            pg.set_u32(m, local.wrapping_add(12), y);
            pg.set_u32(m, local, 0);
            pg.set_u32(m, local.wrapping_add(4), 0);
        } else if (flags >> 11) & 1 != 0 {
            call_virtual(m, c, pg, frame, this, 0x50, &[], 0xe0c053);
            call_virtual(m, c, pg, frame, this, 0x54, &[], 0xe0c05c);
        }
        let timeline = pg.u32(m, this.wrapping_add(0x100));
        call_function(m, c, pg, frame, timeline, 0x0107_84b0, &[0x0162_95c0, local], 0xe0c0b3);
        let mut k = 0u32;
        if pg.u32(m, this.wrapping_add(0x10c)) > 0 {
            loop {
                let list = pg.u32(m, this.wrapping_add(0x118));
                let object = pg.u32(m, list.wrapping_add(4 * k));
                if object != 0 {
                    let word = pg.u32(m, object.wrapping_add(8)) & 0xfffe_ffff;
                    pg.set_u32(m, object.wrapping_add(8), word);
                }
                k += 1;
                if k >= pg.u32(m, this.wrapping_add(0x10c)) {
                    break;
                }
            }
        }
    }
    // the clocks: previous := current; current += dt
    let dt = pg.u32(m, entry.wrapping_add(4));
    let now = pg.u32(m, this.wrapping_add(0x12c));
    pg.set_u32(m, this.wrapping_add(0x130), now);
    pg.set_u32(m, this.wrapping_add(0x12c), x86::addss(now, dt));
    let now = pg.u32(m, this.wrapping_add(0x134));
    pg.set_u32(m, this.wrapping_add(0x138), now);
    pg.set_u32(m, this.wrapping_add(0x134), x86::addss(now, dt));
    done(m, c);
}

/// 0x00E75EA0, `effect->draw_children(context)` (thiscall, `ret 4`; the
/// original's `uEffect::vf11`): walk the effect's child list (`+0x1f0`,
/// linked through `+8`) and call each visible child's virtual method +0x50
/// with (context, frame count, shader constants).
///
/// Nothing happens unless the effect has a resource (`+0xf4`), the virtual
/// +0x60 says it is not hidden, its flags (`+0xf0`) do not have bit 19, the
/// optional occluder checks (`+0x1d0`; [`occluder_matches`] and
/// [`occluder_visible`]) pass and 0x00FDF6A0 does not reject the context.
/// The frame count is the float `effect[0x180 + 4 k]` (`k` the context's
/// byte `+0x168`, 0 when 8 or more) times the float at 0x01430E64,
/// truncated to an integer by `_ftol` on the guest x87 stack; zero ends it.
/// A child is visited when its flags (`+0xc` bit 1) are set and its mask
/// (`+0x40`) meets the context's byte `+0x16a`. If `+0x210` is set the child
/// is marked (`+0x90 |= 2`) and the word `+0x14` of its object at `+0x30`
/// cleared; a transform queued in the global at 0x018D1D20 (rare) is
/// handed to the child before the call.
///
/// The small callees of the original are done in place: 0x00E6C220,
/// 0x0049ACE0, 0x00E6BC60 (the float load), 0x00FB5400 (the constants
/// block), 0x00E75B00 and 0x0041D1E0 (the local transform list's
/// construction and release), 0x0041B140 (a lazily allocated block: a guest
/// virtual call the first time), 0x00E75B20 (a guest call only when the
/// global has queued transforms), 0x00E6E640, 0x00E6BE50 and `_ftol`
/// ([`crate::semantic_s7_ftol`]). The hidden check, 0x00FDF6A0 and the
/// children's method stay guest calls.
pub fn effect_draw_children(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let leave = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(8);
    };
    c.edi = this;
    if pg.u32(m, this.wrapping_add(0xf4)) == 0 {
        return leave(c);
    }
    // SUB ESP, 0x30; EBX and EDI pushed
    let hidden = call_virtual(m, c, pg, entry.wrapping_sub(0x38), this, 0x60, &[], 0xe75ebc);
    if hidden & 0xff != 0 || pg.u32(m, this.wrapping_add(0xf0)) & 0x8_0000 != 0 {
        return leave(c);
    }
    let occluder = pg.u32(m, this.wrapping_add(0x1d0));
    if occluder != 0 && occluder_matches(m, pg, this, occluder) && !occluder_visible(m, pg, occluder) {
        return leave(c);
    }
    // ESI and EBP pushed
    let frame = entry.wrapping_sub(0x40);
    let context = pg.u32(m, entry.wrapping_add(4));
    if call_function(m, c, pg, frame, this, 0x00fd_f6a0, &[context], 0xe75f06) & 0xff != 0 {
        return leave(c);
    }
    // the frame count: the effect's float, times the constant, _ftol
    let selector = pg.u8(m, context.wrapping_add(0x168)) as u32;
    let slot = if selector < 8 { selector } else { 0 };
    c.st.real_memory::<0x00d9, 4, false>(m, this.wrapping_add(0x180 + 4 * slot));
    c.st.real_memory::<0x08d8, 4, false>(m, 0x0143_0e64);
    let frames = call_from(m, c, pg, frame, this, &[], 0xe75f28, crate::semantic_s7_ftol::ftol2_sse);
    if frames == 0 {
        return leave(c);
    }
    let mut constants = 0u32;
    let shader = pg.u32(m, this.wrapping_add(0x1c0));
    if shader != 0 {
        // 0x00FB5400 on the global owner: `shader` entries of the block
        let owner = pg.u32(m, 0x018d_2818);
        let source = pg.u32(m, context.wrapping_add(0x58));
        let base = pg.u32(m, owner.wrapping_add(0x140));
        if base != 0 {
            let limit = pg.u32(m, owner.wrapping_add(0x144));
            if shader <= limit {
                constants = limit.wrapping_mul(source).wrapping_add(base);
            }
        }
    }
    // the local transform list at ESP + 0x28 (0x00E75B00: empty)
    let local = frame.wrapping_add(0x28);
    for offset in [0u32, 4, 8, 12, 16] {
        pg.set_u32(m, local.wrapping_add(offset), 0);
    }
    let mut has_transform = false;
    let resource = pg.u32(m, this.wrapping_add(0xf4));
    if resource != 0 {
        let first = pg.u32(m, resource.wrapping_add(0x58));
        let second = pg.u32(m, resource.wrapping_add(0x5c));
        // 0x0041B140 leaves its three arguments on the stack, 0x00E75B20
        // (`ret 0xc`) reads and pops them
        let found = shared_block(m, c, pg, frame, [first, second, local]);
        let global = pg.u32(m, 0x018d_1d20);
        if pg.u32(m, global.wrapping_add(0x1c)) == 0 {
            c.esp = frame; // 0x00E75B20 answers false and pops them
        } else {
            let below = c.esp;
            has_transform = call_function(m, c, pg, below, found, 0x00e7_5b20, &[], 0xe75f86) & 0xff != 0;
        }
    }
    let mut child = pg.u32(m, this.wrapping_add(0x1f0));
    while child != 0 {
        if (pg.u32(m, child.wrapping_add(0xc)) >> 1) & 1 != 0 && pg.u8(m, child.wrapping_add(0x40)) & pg.u8(m, context.wrapping_add(0x16a)) != 0 {
            if pg.u8(m, this.wrapping_add(0x210)) != 0 {
                let object = pg.u32(m, child.wrapping_add(0x30));
                let word = pg.u32(m, child.wrapping_add(0x90)) | 2;
                pg.set_u32(m, child.wrapping_add(0x90), word);
                if object != 0 {
                    pg.set_u16(m, object.wrapping_add(0x14), 0);
                }
            }
            if has_transform {
                let out = frame.wrapping_add(0x18);
                if queued_transform(m, pg, local, out) {
                    // 0x00E6BE50: mark the child and copy the 16 bytes
                    pg.set_u8(m, child.wrapping_add(0xb0), 1);
                    for k in 0..4 {
                        let word = pg.u32(m, out.wrapping_add(4 * k));
                        pg.set_u32(m, child.wrapping_add(0xb4 + 4 * k), word);
                    }
                }
            }
            call_virtual(m, c, pg, frame, child, 0x50, &[context, frames, constants], 0xe75fff);
        }
        child = pg.u32(m, child.wrapping_add(8));
    }
    // 0x0041D1E0: release the local list
    release_list(m, c, pg, frame, local);
    leave(c);
}

/// 0x00E6C220: the occluder's kind (`+4` & 7) is 1 or 2, its stamp
/// (`+0x1d4`) equals the effect's, and its word `+0x1e0` is non-zero.
fn occluder_matches(m: &dyn Mem, pg: Pages, this: u32, occluder: u32) -> bool {
    let kind = pg.u32(m, occluder.wrapping_add(4)) & 7;
    (kind == 2 || kind == 1)
        && pg.u32(m, occluder.wrapping_add(0x1d4)) == pg.u32(m, this.wrapping_add(0x1d4))
        && pg.u16(m, occluder.wrapping_add(0x1e0)) != 0
}

/// 0x0049ACE0: kind 2 with bits 11 and 14 of the word `+4` set.
fn occluder_visible(m: &dyn Mem, pg: Pages, occluder: u32) -> bool {
    let word = pg.u32(m, occluder.wrapping_add(4));
    word & 7 == 2 && word & 0x4800 == 0x4800
}

/// 0x0041B140: the global block at 0x018D188C, allocated on first use with
/// the virtual allocator at 0x0187662C (`alloc(0x10, 0x10)`, zeroed
/// header); the three arguments stay pushed, as the original leaves them.
fn shared_block(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, args: [u32; 3]) -> u32 {
    c.esp = frame;
    for &v in args.iter().rev() {
        c.esp = c.esp.wrapping_sub(4);
        pg.set_u32(m, c.esp, v);
    }
    let block = pg.u32(m, 0x018d_188c);
    if block != 0 {
        return block;
    }
    // inside the callee: its return address is on the stack
    let inside = c.esp.wrapping_sub(4);
    pg.set_u32(m, inside, 0xe75f7f);
    let allocator = pg.u32(m, 0x0187_662c);
    let block = call_virtual(m, c, pg, inside, allocator, 0x20, &[0x10, 0x10], 0x41b15a);
    if block == 0 {
        pg.set_u32(m, 0x018d_188c, 0);
        c.esp = inside.wrapping_add(4);
        return 0;
    }
    for k in 0..3 {
        pg.set_u32(m, block.wrapping_add(4 * k), 0);
    }
    pg.set_u32(m, 0x018d_188c, block);
    c.esp = inside.wrapping_add(4);
    block
}

/// 0x00E6E640: the queued transform of `list` (a vector of 16-byte entries
/// at `list + 8`) when it holds exactly one with first dword -1: copy its
/// 16 bytes to `out`; `out` is cleared either way.
fn queued_transform(m: &mut dyn Mem, pg: Pages, list: u32, out: u32) -> bool {
    for k in 0..4 {
        pg.set_u32(m, out.wrapping_add(4 * k), 0);
    }
    let begin = pg.u32(m, list.wrapping_add(8));
    let end = pg.u32(m, list.wrapping_add(0xc));
    if end.wrapping_sub(begin) & 0xffff_fff0 != 0x10 || pg.u32(m, begin) != u32::MAX {
        return false;
    }
    for k in 0..4 {
        let word = pg.u32(m, begin.wrapping_add(4 * k));
        pg.set_u32(m, out.wrapping_add(4 * k), word);
    }
    true
}

/// 0x0041D1E0: free the list's block (the virtual `free` at +0x24 of the
/// global allocator) if it has one, and clear its three fields.
fn release_list(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, list: u32) {
    let block = pg.u32(m, list.wrapping_add(8));
    if block != 0 {
        let allocator = pg.u32(m, 0x0187_662c);
        // inside the callee: its return address and the saved ESI
        call_virtual(m, c, pg, frame.wrapping_sub(8), allocator, 0x24, &[block], 0x41d1f8);
        c.esp = frame;
    }
    for offset in [8u32, 12, 16] {
        pg.set_u32(m, list.wrapping_add(offset), 0);
    }
}
