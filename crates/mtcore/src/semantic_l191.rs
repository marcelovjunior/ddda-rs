//! L191: ports of the per-model draw-prep functions (uObjModel::vf08/vf09/vf11
//! and uCharacterBase::vf09). Same ABI-level conventions as `semantic_l190.rs`;
//! oracles in `crates/mtcore/tests/semantic_l191.rs` (vectors from
//! `research/native/record_l191_*.py`).
use crate::semantic::{call_from, call_guest};
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

/// Model field offsets (from the model object, ECX).
const MODEL_FLAGS: u32 = 0x108;
const MODEL_LEVEL: u32 = 0x2514;
const MODEL_FADE: u32 = 0x158;
/// The embedded state record of the model.
const MODEL_STATE: u32 = 0x1f1c;

/// Bit 30 of the model flags: set when the level is above the threshold.
const FLAG_ABOVE: u32 = 0x4000_0000;

// Translated from 0x00886880 (DDDA.exe 2017-12-13 Steam), uObjModel::vf11.
/// 0x00886880, `uObjModel::vf11(arg)` (thiscall, `ret 4`; void). The model's
/// level (+0x2514) is copied to +0x158, and bit 30 of the flags records
/// whether it is above the constant at 0x015319B8. When the embedded state
/// has a pending block (+0x24 and +0x28 set, byte +0x5A set), the state's
/// handler 0x007A6640 runs on `arg` first; then the call is tail-called to
/// 0x00FFE5C0 with the same argument.
pub fn obj_model_vf11(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let arg = pg.u32(m, entry.wrapping_add(4));
    // push ESI
    let saved_esi = c.esi;
    pg.set_u32(m, entry.wrapping_sub(4), saved_esi);

    let level = pg.u32(m, this.wrapping_add(MODEL_LEVEL));
    let threshold = f32::from_bits(pg.u32(m, 0x0153_19b8));
    // comiss: "above" is false for an unordered (NaN) level as well
    let above = threshold > f32::from_bits(level);
    let flags = pg.u32(m, this.wrapping_add(MODEL_FLAGS));
    let flags = if above { flags | FLAG_ABOVE } else { flags & !FLAG_ABOVE };
    pg.set_u32(m, this.wrapping_add(MODEL_FLAGS), flags);
    let state = this.wrapping_add(MODEL_STATE);
    pg.set_u32(m, this.wrapping_add(MODEL_FADE), level);

    let pending = pg.u32(m, state.wrapping_add(0x24)) != 0
        && pg.u32(m, state.wrapping_add(0x28)) != 0
        && pg.u8(m, state.wrapping_add(0x5a)) != 0;
    if pending {
        // The state handler runs with the argument as its only stack word.
        let frame = entry.wrapping_sub(4);
        call_from(m, c, pg, frame, this, &[arg], 0x0088_68d6, |m, c| call_guest(m, c, 0x007a_6640));
        c.esi = pg.u32(m, entry.wrapping_sub(4));
        c.esp = entry.wrapping_add(8);
    } else {
        // Tail call: the callee returns to our caller with its own `ret 4`.
        c.esi = pg.u32(m, entry.wrapping_sub(4));
        c.esp = entry;
        call_guest(m, c, 0x00ff_e5c0);
    }
}

/// Model flag words of the draw prep (see `obj_model_vf08`).
const MODEL_PREPARED: u32 = 0x1f9c;
const MODEL_PASSES: u32 = 0x1f98;

// Translated from 0x00885DC0 (DDDA.exe 2017-12-13 Steam), uObjModel::vf08.
/// 0x00885DC0, `uObjModel::vf08()` (thiscall, plain `ret`; void). Clears the
/// draw counter at +0x209C; the first time (+0x1F9C clear) the model sets
/// +0x1F9C and calls its vtable slot 0x80. When +0x1F98 is 1, the vtable
/// slots 0x168, 0x16C and 0x170 are called in turn, and slot 0x174 is
/// tail-called. Every vtable call is made on the model (ECX).
pub fn obj_model_vf08(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    // push ESI
    pg.set_u32(m, entry.wrapping_sub(4), c.esi);
    pg.set_u16(m, this.wrapping_add(0x209c), 0);

    // A virtual method of the model, called thiscall with no arguments.
    let method = |m: &mut dyn Mem, c: &mut Cpu, slot: u32, back: u32| {
        let vtable = pg.u32(m, this);
        let target = pg.u32(m, vtable.wrapping_add(slot));
        call_from(m, c, pg, entry.wrapping_sub(4), this, &[], back, |m, c| call_guest(m, c, target));
    };

    if pg.u32(m, this.wrapping_add(MODEL_PREPARED)) == 0 {
        pg.set_u32(m, this.wrapping_add(MODEL_PREPARED), 1);
        method(m, c, 0x80, 0x0088_5de8);
    }
    if pg.u32(m, this.wrapping_add(MODEL_PASSES)) == 1 {
        method(m, c, 0x168, 0x0088_5dfd);
        method(m, c, 0x16c, 0x0088_5e09);
        method(m, c, 0x170, 0x0088_5e15);
        // Tail call through slot 0x174 with the return address still in place.
        let vtable = pg.u32(m, this);
        let target = pg.u32(m, vtable.wrapping_add(0x174));
        c.esi = pg.u32(m, entry.wrapping_sub(4));
        c.esp = entry;
        call_guest(m, c, target);
        return;
    }
    c.esi = pg.u32(m, entry.wrapping_sub(4));
    c.esp = entry.wrapping_add(4);
}

/// Character field offsets (from the character object, ECX).
const CHARACTER_MODE_FLAG: u32 = 0x2f84; // byte: clear selects bit 0x200000 of +0x1B58
const CHARACTER_FLAGS: u32 = 0x1b58;
const CHARACTER_MODEL_BLOCK: u32 = 0x23bc;
const CHARACTER_FLAGS_2ECC: u32 = 0x2ecc;
const CHARACTER_SPEED: u32 = 0x2360;
const CHARACTER_MODE: u32 = 0x2748;

// Translated from 0x0084C720 (DDDA.exe 2017-12-13 Steam), uCharacterBase::vf09.
/// 0x0084C720, `uCharacterBase::vf09()` (thiscall, plain `ret`; void). Sets
/// bit 0x200000 of +0x1B58 unless the byte at +0x2F84 is set, then runs the
/// model draw prep 0x00886530 on the character. When +0x23BC is set, the
/// character's vtable slot 0x94 is called; the object it returns decides the
/// mode bit 0x8000 of +0x2ECC (set when +0x2360 has bit 0x20000000) and the
/// slot 0x94 object's byte +0x206D sets the mode in EDI (1 or 2). The stdcall
/// 0x00CA5930 then gets six arguments. A null result leaves EDI at 0.
pub fn character_vf09(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let (saved_ebp, saved_ebx, saved_esi, saved_edi) = (c.ebp, c.ebx, c.esi, c.edi);
    // Prologue: EBP, then the frame aligned to 8 with ECX (this), EBX, ESI and EDI pushed.
    let aligned = entry.wrapping_sub(4) & !7;
    pg.set_u32(m, entry.wrapping_sub(4), saved_ebp);
    pg.set_u32(m, aligned.wrapping_sub(4), this);
    pg.set_u32(m, aligned.wrapping_sub(8), saved_ebx);
    pg.set_u32(m, aligned.wrapping_sub(12), saved_esi);
    pg.set_u32(m, aligned.wrapping_sub(16), saved_edi);
    let frame = aligned.wrapping_sub(16);

    // Bit 21 of the flags follows the mode byte.
    let mode_clear = pg.u8(m, this.wrapping_add(CHARACTER_MODE_FLAG)) == 0;
    let flags = pg.u32(m, this.wrapping_add(CHARACTER_FLAGS));
    let flags = if mode_clear { flags | 0x0020_0000 } else { flags & !0x0020_0000 };
    pg.set_u32(m, this.wrapping_add(CHARACTER_FLAGS), flags);

    // 0x008586E0 gets the character in EAX.
    c.eax = this;
    call_from(m, c, pg, frame, this, &[], 0x0084_c754, |m, c| call_guest(m, c, 0x0085_86e0));
    // 0x00886530: the model draw prep, on the character.
    call_from(m, c, pg, frame, this, &[], 0x0084_c75b, |m, c| call_guest(m, c, 0x0088_6530));

    if pg.u32(m, this.wrapping_add(CHARACTER_MODEL_BLOCK)) != 0 {
        // The 0x2ECC mode bit follows the speed word's bit 0x20000000.
        let speed = pg.u32(m, this.wrapping_add(CHARACTER_SPEED)) & 0x2000_0000;
        let mode = pg.u32(m, this.wrapping_add(CHARACTER_FLAGS_2ECC));
        let mode = if speed == 0 { mode & !0x8000 } else { mode | 0x8000 };
        pg.set_u32(m, this.wrapping_add(CHARACTER_FLAGS_2ECC), mode);

        // Vtable slot 0x94 (thiscall); its result is an object or null.
        let vtable = pg.u32(m, this);
        let target = pg.u32(m, vtable.wrapping_add(0x94));
        c.ebx = pg.u32(m, this.wrapping_add(CHARACTER_FLAGS_2ECC));
        c.edi = 0;
        let object = call_from(m, c, pg, frame, this, &[], 0x0084_c79f, |m, c| call_guest(m, c, target));
        if object != 0 {
            let byte = pg.u8(m, object.wrapping_add(0x206d));
            c.edi = if byte & 2 != 0 {
                1
            } else if byte & 4 != 0 {
                2
            } else {
                0
            };
        }
        // 0x00CA5930(block, this + 0x23c0, EBX, EDI, speed2, speed3), stdcall.
        let block = pg.u32(m, this.wrapping_add(CHARACTER_MODEL_BLOCK));
        let (a5, a6) = (pg.u32(m, this.wrapping_add(0x26a0)), pg.u32(m, this.wrapping_add(0x26a4)));
        call_from(
            m,
            c,
            pg,
            frame,
            block,
            &[block, this.wrapping_add(0x23c0), c.ebx, c.edi, a5, a6],
            0x0084_c7e0,
            |m, c| call_guest(m, c, 0x00ca_5930),
        );
    }

    // 0x0084C7E0: the mode word keeps only bit 0x20 of the 0x2748 flags.
    let mode = pg.u32(m, this.wrapping_add(CHARACTER_MODE));
    pg.set_u32(m, this.wrapping_add(CHARACTER_MODE), (mode & !0x40) | 0x20);
    // epilogue: the saved registers come back from the frame
    c.edi = pg.u32(m, aligned.wrapping_sub(16));
    c.esi = pg.u32(m, aligned.wrapping_sub(12));
    c.ebx = pg.u32(m, aligned.wrapping_sub(8));
    c.ebp = pg.u32(m, entry.wrapping_sub(4));
    c.esp = entry.wrapping_add(4);
}

/// Calls the vtable slot `slot` of `object` (thiscall, no arguments).
fn vmethod(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, object: u32, slot: u32, back: u32) {
    let vtable = pg.u32(m, object);
    let target = pg.u32(m, vtable.wrapping_add(slot));
    call_from(m, c, pg, frame, object, &[], back, |m, c| call_guest(m, c, target));
}

/// The float block of `obj_model_vf09` (flag 0x400000): the model's offsets
/// from the shape at +0xE70.. are stored, and the angle of the lateral
/// offset is taken by the CRT atan2 (native, bit exact).
fn angle_block(m: &mut dyn Mem, pg: Pages, this: u32) {
    let f = |m: &dyn Mem, a: u32| f32::from_bits(pg.u32(m, a));
    let (a1, a0, a2) = (f(m, this + 0x44) - f(m, this + 0xe74), f(m, this + 0x40) - f(m, this + 0xe70), f(m, this + 0x48) - f(m, this + 0xe78));
    pg.set_u32(m, this + 0xf70, a0.to_bits());
    pg.set_u32(m, this + 0xf74, a1.to_bits());
    pg.set_u32(m, this + 0xf78, a2.to_bits());
    pg.set_u32(m, this + 0xf7c, 0);
    let (b1, b0, b2) = (f(m, this + 0xe4) - f(m, this + 0xe84), f(m, this + 0xe0) - f(m, this + 0xe80), f(m, this + 0xe8) - f(m, this + 0xe88));
    pg.set_u32(m, this + 0xf80, b0.to_bits());
    pg.set_u32(m, this + 0xf84, b1.to_bits());
    pg.set_u32(m, this + 0xf88, b2.to_bits());
    pg.set_u32(m, this + 0xf8c, 0);
    let (c2, c0) = (f(m, this + 0xe78) - f(m, this + 0x48), f(m, this + 0xe70) - f(m, this + 0x40));
    let length = ((c2 * c2) + (c0 * c0)).sqrt();
    pg.set_u32(m, this + 0xf90, length.to_bits());
    let (d2, d0) = (f(m, this + 0x48) - f(m, this + 0xe78), f(m, this + 0x40) - f(m, this + 0xe70));
    let tables = crate::semantic_s6_crt::Tables { memory: &*m, pages: pg };
    let angle = crate::semantic_s6_crt::atan2(&tables, d0 as f64, d2 as f64) as f32;
    pg.set_u32(m, this + 0xf94, angle.to_bits());
}

/// The body of `obj_model_vf09` between its prologue and epilogue.
fn draw_prep_body(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, this: u32, frame: u32) {
    // The byte flag at +0x20FC == 1 calls one virtual method and returns.
    if pg.u8(m, this.wrapping_add(0x20fc)) == 1 {
        vmethod(m, c, pg, frame, this, 0x3c, 0x0088_654e);
        return;
    }
    // 0x008871A0 (EAX = this): a non-zero low byte of its result ends the call.
    c.eax = this;
    let query = call_from(m, c, pg, frame, this, &[], 0x0088_655c, |m, c| call_guest(m, c, 0x0088_71a0));
    if query & 0xff != 0 {
        return;
    }
    // The pass word at +0x1F98 must be 1 for the rest.
    if pg.u32(m, this.wrapping_add(0x1f98)) != 1 {
        return;
    }
    call_from(m, c, pg, frame, this, &[], 0x0088_6578, |m, c| call_guest(m, c, 0x00ea_1b20));
    if pg.u32(m, this.wrapping_add(0x2398)) & 0x0001_0000 != 0 {
        vmethod(m, c, pg, frame, this, 0x17c, 0x0088_6590);
    }
    if pg.u8(m, this.wrapping_add(0x2398)) & 4 != 0 {
        // the sub-object at +0x10E0; EAX = the object at +0x40
        c.eax = this.wrapping_add(0x40);
        call_from(m, c, pg, frame, this.wrapping_add(0x10e0), &[], 0x0088_65a7, |m, c| call_guest(m, c, 0x0076_d6b0));
        if pg.u8(m, this.wrapping_add(0x2398)) & 4 != 0 {
            // bit 2 of the byte at +0x20E4 goes to the byte at +0x1B6F
            let flag = (pg.u8(m, this.wrapping_add(0x20e4)) >> 2) & 1;
            pg.set_u8(m, this.wrapping_add(0x1b6f), flag);
            pg.set_u32(m, this.wrapping_add(0x1b68), 0);
            let tail = pg.u8(m, this.wrapping_add(0xec0)) as u32;
            let args = [this.wrapping_add(0x40), this.wrapping_add(0xe70), this.wrapping_add(0xe90), this, tail];
            c.eax = this.wrapping_add(0x10e0);
            call_from(m, c, pg, frame, this.wrapping_add(0x40), &args, 0x0088_65f1, |m, c| call_guest(m, c, 0x0076_c540));
            pg.set_u8(m, this.wrapping_add(0x1ed4), 0);
        }
    }
    if pg.u32(m, this.wrapping_add(0x2398)) & 0x0008_0000 != 0 {
        vmethod(m, c, pg, frame, this, 0x180, 0x0088_6610);
    }
    if pg.u32(m, this.wrapping_add(0x2398)) & 0x0010_0000 != 0 {
        vmethod(m, c, pg, frame, this, 0x184, 0x0088_6628);
    }
    if pg.u32(m, this.wrapping_add(0x2398)) & 0x0020_0000 != 0 {
        // the two child objects' method 0x24, each only when set
        let first = pg.u32(m, this.wrapping_add(0x1ef8));
        if first != 0 {
            vmethod(m, c, pg, frame, first, 0x24, 0x0088_664a);
        }
        let second = pg.u32(m, this.wrapping_add(0x1efc));
        if second != 0 {
            vmethod(m, c, pg, frame, second, 0x24, 0x0088_6660);
        }
    }
    // The bounds test: the squared distance from the global reference point
    // (its object at +0x34) against the threshold at 0x0150929C.
    if pg.u32(m, this.wrapping_add(0x1f80)) != 0 {
        let mut call_helper = true;
        if pg.u8(m, this.wrapping_add(0x206d)) & 1 != 0 && pg.u8(m, this.wrapping_add(0x206e)) & 0x10 == 0 {
            call_helper = false;
            let global = pg.u32(m, 0x018d_0b6c);
            let reference = pg.u32(m, global.wrapping_add(0x34));
            if reference != 0 {
                let p = call_from(m, c, pg, frame, this, &[frame.wrapping_add(0x10)], 0x0088_6694, |m, c| call_guest(m, c, 0x004c_6b50));
                let coord = |a: u32| f32::from_bits(pg.u32(m, a));
                let (x, y, z) = (coord(p) - coord(reference + 0x40), coord(p + 4) - coord(reference + 0x44), coord(p + 8) - coord(reference + 0x48));
                let squared = ((y * y) + (x * x)) + (z * z);
                call_helper = f32::from_bits(pg.u32(m, 0x0150_929c)) > squared;
            }
        }
        if call_helper {
            c.esi = pg.u32(m, this.wrapping_add(0x1f80));
            call_from(m, c, pg, frame, this, &[], 0x0088_66d9, |m, c| call_guest(m, c, 0x0082_1750));
        }
    }
    if pg.u32(m, this.wrapping_add(0x2398)) & 0x0040_0000 != 0 {
        angle_block(m, pg, this);
    }
    if pg.u8(m, this.wrapping_add(0x2398)) & 4 != 0 {
        let block = this.wrapping_add(0x10e0);
        call_from(m, c, pg, frame, this, &[block], 0x0088_67fb, |m, c| call_guest(m, c, 0x0076_b300));
        call_from(m, c, pg, frame, this, &[block], 0x0088_6801, |m, c| call_guest(m, c, 0x0076_b660));
    }
    if pg.u32(m, this.wrapping_add(0x23b4)) != 0 && pg.u8(m, this.wrapping_add(0x23b8)) & 2 == 0 {
        let count = pg.u32(m, this.wrapping_add(0x23a4));
        if (count as i32) > 0 {
            // each non-null element of the table at +0x23B0 is released
            let mut i = 0u32;
            loop {
                let table = pg.u32(m, this.wrapping_add(0x23b0));
                let element = pg.u32(m, table.wrapping_add(i.wrapping_mul(4)));
                if element != 0 {
                    call_from(m, c, pg, frame, this, &[element], 0x0088_6833, |m, c| call_guest(m, c, 0x005b_0570));
                }
                i = i.wrapping_add(1);
                if (i as i32) >= (count as i32) {
                    break;
                }
            }
        }
    }
    let flags = pg.u32(m, this.wrapping_add(0x20c4)) & 0xffff_fff3;
    pg.set_u32(m, this.wrapping_add(0x20c4), flags);
    if pg.u8(m, this.wrapping_add(0x2398)) & 0x10 != 0 {
        call_from(m, c, pg, frame, this, &[this], 0x0088_684e, |m, c| call_guest(m, c, 0x0088_7360));
    }
    c.eax = this;
    call_from(m, c, pg, frame, this, &[], 0x0088_6855, |m, c| call_guest(m, c, 0x0088_8750));
    if pg.u32(m, this.wrapping_add(0x1f40)) == 0 || pg.u32(m, this.wrapping_add(0x1f44)) == 0 {
        return;
    }
    if pg.u8(m, this.wrapping_add(0x1f76)) == 0 {
        return;
    }
    call_from(m, c, pg, frame, this, &[0], 0x0088_6879, |m, c| call_guest(m, c, 0x00ea_1cc0));
}

// Translated from 0x00886530 (DDDA.exe 2017-12-13 Steam), uObjModel::vf09.
/// 0x00886530, `uObjModel::vf09()` (thiscall, plain `ret`; void): the model's
/// per-frame draw prep. A byte flag at +0x20FC == 1 calls the method at slot
/// 0x3C and returns. Otherwise a query (0x008871A0) decides whether the rest
/// runs; the flag word at +0x2398 selects the sub-object and child-object
/// method calls, the bounds test may call 0x00821750, the bit 0x400000 step
/// computes the angle block (`angle_block`), and the table at +0x23B0 is
/// released element by element when its flags allow.
pub fn obj_model_vf09(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebp, c.ebx, c.esi, c.edi);
    // Prologue: EBP, then the frame aligned to 16 with 0x14 bytes of locals,
    // then EBX, ESI and EDI pushed below them.
    let aligned = entry.wrapping_sub(4) & !15;
    let frame = aligned.wrapping_sub(0x20);
    pg.set_u32(m, entry.wrapping_sub(4), saved.0);
    pg.set_u32(m, aligned.wrapping_sub(0x18), saved.1);
    pg.set_u32(m, aligned.wrapping_sub(0x1c), saved.2);
    pg.set_u32(m, aligned.wrapping_sub(0x20), saved.3);

    draw_prep_body(m, c, pg, this, frame);

    // Epilogue: the saved registers come back from the frame.
    c.edi = pg.u32(m, aligned.wrapping_sub(0x20));
    c.esi = pg.u32(m, aligned.wrapping_sub(0x1c));
    c.ebx = pg.u32(m, aligned.wrapping_sub(0x18));
    c.ebp = pg.u32(m, entry.wrapping_sub(4));
    c.esp = entry.wrapping_add(4);
}

/// Calls the vtable slot `slot` of `object` (thiscall) with `args`; returns EAX.
fn vcall(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, frame: u32, object: u32, slot: u32, args: &[u32], back: u32) -> u32 {
    let vtable = pg.u32(m, object);
    let target = pg.u32(m, vtable.wrapping_add(slot));
    call_from(m, c, pg, frame, object, args, back, |m, c| call_guest(m, c, target))
}

/// The body of `probe_search_step` between its prologue and epilogue.
/// `frame` is the original's ESP after its pushes (locals at +0x13..+0x2C).
/// Returns EAX: the last callee's result, or 0 on the early exits.
fn probe_body(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, entry: u32, frame: u32, state: u32, owner: u32, this: u32, index: u32, table: u32, entry_record: u32, l20: u32) -> u32 {
    // The 80-byte record of the entry, at the table's record base.
    let record = pg.u32(m, table.wrapping_add(0xbc)).wrapping_add(index.wrapping_mul(5).wrapping_shl(4));
    // Exits: an idle entry (+0xC clear), a busy one (+0x1F set), or the current one.
    if pg.u8(m, entry_record.wrapping_add(0xc)) == 0 || pg.u8(m, entry_record.wrapping_add(0x1f)) != 0 || l20 == entry_record {
        return 0;
    }
    if pg.u32(m, table.wrapping_add(0x68)) == 2 {
        let arg3 = pg.u32(m, entry.wrapping_add(0xc));
        if arg3 != 0 {
            // a cell of the table, indexed by the request's two words and the index
            let row = pg.u32(m, arg3.wrapping_add(4)).wrapping_add(index.wrapping_mul(15).wrapping_mul(2));
            let cell = pg.u32(m, arg3.wrapping_add(8)).wrapping_add(row.wrapping_mul(2));
            let slot = pg.u32(m, table.wrapping_add(0xb8)).wrapping_add(cell.wrapping_mul(4)).wrapping_add(0x1c);
            let value = pg.u32(m, arg3);
            if value == pg.u32(m, slot) {
                return 0;
            }
            pg.set_u32(m, slot, value);
            // the record is the argument; the vtable method 0x44 of the object decides
            let flagged = pg.u8(m, state.wrapping_add(0xb4)) != 0;
            let (object, back) = if flagged {
                (pg.u32(m, frame + 0x28), 0x0116_5c41)
            } else {
                (pg.u32(m, frame + 0x18), 0x0116_5c24)
            };
            if vcall(m, c, pg, frame, object, 0x44, &[record], back) as u8 == 0 {
                return 0;
            }
        }
    }

    // 0x00DE5B80 on the local block X (ECX = X).
    let x = frame + 0x2c;
    call_from(m, c, pg, frame, x, &[], 0x0116_5c52, |m, c| call_guest(m, c, 0x00de_5b80));
    let flag_a = pg.u32(m, state.wrapping_add(0xa8)) != 0;
    pg.set_u8(m, frame + 0x13, flag_a as u8);
    if flag_a {
        let object = pg.u32(m, state.wrapping_add(0xa8));
        call_from(m, c, pg, frame, object, &[index], 0x0116_5c6b, |m, c| call_guest(m, c, 0x00de_52a0));
    } else {
        // 0x00DE5C10(a, index, [this + 0x124 + 4 * slot], [this + 0xC0 + 4 * slot]) on X
        let slot_base = this.wrapping_add(pg.u32(m, frame + 0x1c).wrapping_mul(4));
        let c0 = pg.u32(m, slot_base.wrapping_add(0xc0));
        let v124 = pg.u32(m, slot_base.wrapping_add(0x124));
        let a8 = pg.u32(m, owner.wrapping_add(8));
        call_from(m, c, pg, frame, x, &[a8, index, v124, c0], 0x0116_5c8f, |m, c| call_guest(m, c, 0x00de_5c10));
        pg.set_u32(m, state.wrapping_add(0xa8), x);
        let first = pg.u32(m, state);
        if first != 0 {
            // an indirect method from the state's table, (ECX = the state's first word)
            let count = pg.u32(m, state.wrapping_add(0x50));
            let method = pg.u32(m, state.wrapping_add(count.wrapping_mul(24)).wrapping_add(0x10));
            if method != 0 {
                let l18 = pg.u32(m, frame + 0x18);
                let extra = pg.u32(m, state.wrapping_add(0x94));
                call_from(m, c, pg, frame, first, &[l18, x, extra], 0x0116_5cc0, |m, c| call_guest(m, c, method));
            }
        }
    }

    // 0x0116_5CC0: a select (the state's +0xC0 word when the flag at +0xB4 is set).
    let mut eax = pg.u32(m, frame + 0x18);
    if pg.u8(m, state.wrapping_add(0xb4)) != 0 {
        eax = pg.u32(m, state.wrapping_add(0xc0));
    }
    let l1c = pg.u32(m, frame + 0x1c);
    let args = [index, eax, this, 0x0116_58c0, state, l1c];
    let result = if pg.u8(m, entry.wrapping_add(0x10)) == 0 {
        // 0x010A8BB0 on the table (ECX = the table word at +0x14)
        let table_word = pg.u32(m, frame + 0x14);
        call_from(m, c, pg, frame, table_word, &args, 0x0116_5d0d, |m, c| call_guest(m, c, 0x010a_8bb0))
    } else {
        // 0x010A88A0 (ECX = the table word: the original reads [esp + 0x18]
        // after its push of L1C, which is the table word at +0x14)
        let table_word = pg.u32(m, frame + 0x14);
        call_from(m, c, pg, frame, table_word, &args, 0x0116_5cf4, |m, c| call_guest(m, c, 0x010a_88a0))
    };
    if pg.u8(m, frame + 0x13) == 0 {
        pg.set_u32(m, state.wrapping_add(0xa8), 0);
    }
    // 0x00DE5BD0 on X.
    call_from(m, c, pg, frame, x, &[], 0x0116_5d29, |m, c| call_guest(m, c, 0x00de_5bd0));
    result
}

// Translated from 0x01165B40 (DDDA.exe 2017-12-13 Steam), Global::FUN_01165b40.
/// 0x01165B40, the probe search step: ECX = the table owner, with four stack
/// words (the state, an index, a record and a flag byte), `ret 0x10`. The
/// state's pointers pick a record of 80 bytes (table +0xBC, index * 80); when
/// its flags allow, a vtable method decides whether to go on; a callback and
/// the table's lookups then run, and EAX is the last callee's result (0 on the
/// early exits). The callees are guest code, called through `call_from`.
pub fn probe_search_step(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // Prologue: EBX, EBP and ESI pushed, then EDI (the frame is entry - 0x88).
    pg.set_u32(m, entry.wrapping_sub(0x7c), saved.0);
    pg.set_u32(m, entry.wrapping_sub(0x80), saved.1);
    pg.set_u32(m, entry.wrapping_sub(0x84), saved.2);
    let state = pg.u32(m, entry.wrapping_add(4));
    let frame = entry.wrapping_sub(0x88);
    let l18 = pg.u32(m, state.wrapping_add(0x90));
    pg.set_u32(m, frame + 0x18, l18);
    let source = pg.u32(m, state.wrapping_add(0x98));
    let owner = pg.u32(m, state.wrapping_add(0xa0));
    let l1c = pg.u32(m, state.wrapping_add(0xe8));
    pg.set_u32(m, frame, saved.3);
    let index = pg.u32(m, entry.wrapping_add(8));
    pg.set_u32(m, frame + 0x1c, l1c);
    // the entry record: owner's array (+0x58) at the index, 64 bytes apiece
    let entry_record = pg.u32(m, owner.wrapping_add(0x58)).wrapping_add(index.wrapping_shl(6));
    let end_cell = pg.u32(m, state.wrapping_add(0xb8));
    let source_flag = pg.u32(m, source.wrapping_add(0x30)) == 1;
    pg.set_u32(m, frame + 0x28, end_cell);
    let table = pg.u32(m, owner.wrapping_add(0x54));
    pg.set_u32(m, frame + 0x24, this);
    pg.set_u32(m, frame + 0x14, table);
    let l20 = if source_flag { pg.u32(m, source.wrapping_add(0x38)) } else { 0 };
    pg.set_u32(m, frame + 0x20, l20);

    let result = probe_body(m, c, pg, entry, frame, state, owner, this, index, table, entry_record, l20);

    // Epilogue: the saved registers come back from the frame; ret 0x10.
    c.edi = pg.u32(m, frame);
    c.esi = pg.u32(m, frame + 4);
    c.ebp = pg.u32(m, frame + 8);
    c.ebx = pg.u32(m, frame + 0xc);
    c.eax = result;
    c.esp = entry.wrapping_add(0x14);
}
