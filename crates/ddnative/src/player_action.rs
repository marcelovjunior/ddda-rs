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

/// The player fields `cPlAction::vf05` reads and writes. The action's
/// owner (+0x44, used by `cActionBase::vf05`) and player (+0x58) are the
/// same uPlayer, so the base entry's fields are here too.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnterPlayer {
    pub category_206c: u32,
    pub word_2370: u32,
    pub word_2374: u32,
    /// +0x273C body flags; +0x2740 the saved copy (bit 0x2 restored).
    pub flags_273c: u32,
    pub saved_2740: u32,
    pub status_2e34: u32,
    pub pending_2e38: u32,
    pub window_4be1: u8,
    /// +0x3558: a state word; its top byte 8 sets body flag 0x800.
    pub word_3558: u32,
    pub word_35e8: u32,
    pub byte_49c0: u8,
    pub word_2fb0: u32,
    pub word_3068: u32,
    pub float_306c: f32,
    pub word_3064: u32,
    pub value_2654: f32,
    /// +0x1DF0: push-out rank (the action sets 13).
    pub push_rank_1df0: u32,
    /// +0x1AC0..+0x1ACC: a vector copied from globals 0x018CFE80..88, w = 0.
    pub vec_1ac0: [f32; 4],
    /// +0x1E68: capsule height (adjust 0xD3); +0x1E6C and +0x31B4: radius (0xD4).
    pub capsule_1e68: f32,
    pub radius_31b4: f32,
    pub radius_1e6c: f32,
    pub byte_31b3: u8,
}

/// `cPlAction::vf05`, the entry of every player action. Returns the
/// action's (+0x50 byte, +0x5C, +0x60, +0x64). `globals` is the vector at
/// 0x018CFE80..88; `adjust(id)` is the player's adjust table getter
/// (`[player+0x3348]` vf15), returning the value it stores.
// Translated from 0x00AC4F70 (DDDA.exe 2017-12-13 Steam), cPlAction::vf05.
pub fn enter(p: &mut EnterPlayer, globals: [f32; 3], mut adjust: impl FnMut(u32) -> f32)
    -> (u8, u32, f32, u32) {
    let mut owner = crate::action::Owner {
        category_206c: p.category_206c, word_2370: p.word_2370, word_2374: p.word_2374,
        flags_273c: p.flags_273c, status_2e34: p.status_2e34, pending_2e38: p.pending_2e38,
    };
    let b50 = crate::action::base_enter(&mut owner);
    (p.word_2370, p.word_2374, p.flags_273c, p.status_2e34) =
        (owner.word_2370, owner.word_2374, owner.flags_273c, owner.status_2e34);
    p.flags_273c ^= (p.saved_2740 ^ p.flags_273c) & 2;
    if p.window_4be1 == 0 && p.flags_273c & 2 != 0 {
        p.flags_273c |= 0x200;
        p.flags_273c |= 0x400;
        let v = p.word_3558;
        // sar 24 & 0xFF, then signed < 0x11 and == 8
        if v != u32::MAX && ((v as i32 >> 24) & 0xFF) == 8 {
            p.flags_273c |= 0x800;
        } else {
            p.flags_273c &= !0x800;
        }
    }
    p.flags_273c &= !0x2000;
    p.flags_273c &= !0x4000;
    p.word_35e8 = 0;
    p.byte_49c0 = 0;
    p.word_2fb0 = 0;
    p.word_3068 = 0;
    p.float_306c = 0.0;
    p.word_3064 = 0;
    let value_60 = crate::x87_copy(p.value_2654);
    p.push_rank_1df0 = 0xD;
    p.vec_1ac0 = [globals[0], globals[1], globals[2], 0.0];
    p.capsule_1e68 = adjust(0xD3);
    p.radius_31b4 = adjust(0xD4);
    p.radius_1e6c = crate::x87_copy(p.radius_31b4);
    p.byte_31b3 = 0;
    (b50, 0, value_60, 0)
}
