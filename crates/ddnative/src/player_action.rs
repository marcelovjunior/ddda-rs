//! `cPlAction`: the base of every player action.

/// The player fields `cPlAction::vf06` reads and writes (uPlayer, at the
/// offsets given; the action reaches them through `action+0x58`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player {
    /// +0x273C: body state flags (bit 0x1 and bit 0x4 tested here).
    pub flags_273c: u32,
    /// +0x2534: f32 compared against 0.0 (velocity component).
    pub value_2534: f32,
    /// +0x2654: f32 the action copies each update.
    pub value_2654: f32,
    /// +0x31EA: dead marker (byte).
    pub dead_31ea: u8,
    /// +0x4B20: cleared each update unless flags bit 0x4 is set.
    pub value_4b20: u32,
    /// +0x4BE1: window byte, closed by the rules below.
    pub window_4be1: u8,
    /// +0x4C18: held effect handle (0 = none).
    pub effect_4c18: u32,
}

/// Callee the action reaches but does not own: 0x00427560 releases an
/// effect handle (ESI = handle) for the owner at global 0x018D2818 with
/// mode EDI = 2.
pub trait Effects {
    fn release(&mut self, handle: u32, mode: u32);
}

/// `cPlAction::vf06`, the base update of every player action. Returns the
/// value stored in the action's `+0x60`.
// Translated from 0x00AC5100 (DDDA.exe 2017-12-13 Steam), cPlAction::vf06.
pub fn base_update(p: &mut Player, effects: &mut impl Effects) -> f32 {
    if p.flags_273c & 4 == 0 {
        p.value_4b20 = 0;
    }
    if p.dead_31ea == 0 {
        let handle = p.effect_4c18;
        if handle != 0 {
            effects.release(handle, 2);
            // the original clears +0x4C18 twice (0x00AC5142, 0x00AC514B)
            p.effect_4c18 = 0;
        }
    }
    if p.window_4be1 != 0 {
        let flags = p.flags_273c;
        // comiss/jbe: an unordered (NaN) compare keeps the window open
        if flags & 4 != 0 || (flags & 1 == 0 && p.value_2534 > 0.0) {
            p.window_4be1 = 0;
        }
    }
    crate::x87_copy(p.value_2654)
}
