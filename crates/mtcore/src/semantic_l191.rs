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
