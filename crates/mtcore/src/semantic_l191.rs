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
