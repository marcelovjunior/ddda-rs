//! The effect manager's frame counter and the time inputs that read it
//! (docs/FRAME_RATE.md 4.5).
//!
//! `sEffect::vf06` counts every rendered frame at `sEffect+0x74`. Effect
//! parameters choose their time input with bits 0..2 of their byte 3;
//! source 4 is that counter, so a noise or random input in that mode
//! changes once per rendered frame, not per unit of game time.

use crate::smain::Main;
use ddnative_base::mem::Mem;

/// The global `sEffect*`.
pub const INSTANCE: u32 = 0x018D_2818;

/// `sEffect+0x74`: frames counted by `vf06`.
pub const FRAME_COUNTER: u32 = 0x74;
/// `sEffect+0x150`: effects run (u8 flag).
pub const RUNNING: u32 = 0x150;
/// `sEffect+0x151`: use `STEP` instead of `sMain`'s step (u8 flag).
pub const OWN_STEP: u32 = 0x151;
/// `sEffect+0x154`: the manager's own step (f32, base frames).
pub const STEP: u32 = 0x154;
/// `sEffect+0x160`: the time value the step is converted into.
pub const TIME: u32 = 0x160;

/// The time-source mode that reads the frame counter.
pub const SOURCE_FRAME_COUNTER: u8 = 4;

/// `sEffect` in guest memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effects(pub u32);

impl Effects {
    pub fn get(m: &dyn Mem) -> Effects {
        Effects(m.u32(INSTANCE))
    }
    pub fn frame_counter(self, m: &dyn Mem) -> u32 {
        m.u32(self.0.wrapping_add(FRAME_COUNTER))
    }
    pub fn running(self, m: &dyn Mem) -> bool {
        m.u8(self.0.wrapping_add(RUNNING)) != 0
    }
    pub fn own_step(self, m: &dyn Mem) -> bool {
        m.u8(self.0.wrapping_add(OWN_STEP)) != 0
    }
    pub fn step_bits(self, m: &dyn Mem) -> u32 {
        m.u32(self.0.wrapping_add(STEP))
    }
}

/// One frame of the effect manager: counts the frame, then, unless effects
/// are stopped or the game is halted, advances them by this frame's step
/// (the manager's own step if it has one, else `sMain.mDeltaTime`; passed
/// as f32 bits). `advance` is the original's two calls: 0x011D4F30
/// converts the step into the time value at `sEffect+0x160`, 0x00FB5E90
/// advances every effect by it.
///
/// The counter is incremented before anything else, also when halted.
// Translated from 0x00FB64D0 (DDDA.exe 2017-12-13 Steam), sEffect::vf06.
pub fn update(m: &mut dyn Mem, effects: Effects, advance: impl FnOnce(&mut dyn Mem, u32)) {
    let counter = effects.0.wrapping_add(FRAME_COUNTER);
    let frames = m.u32(counter).wrapping_add(1);
    m.set_u32(counter, frames);
    if !effects.running(m) || Main::get(m).halted(m) {
        return;
    }
    let step = if effects.own_step(m) {
        effects.step_bits(m)
    } else {
        Main::get(m).delta_time_bits(m)
    };
    advance(m, step);
}

/// A parameter's time source: bits 0..2 of its byte 3.
pub fn time_source(m: &dyn Mem, param: u32) -> u8 {
    m.u8(param.wrapping_add(3)) & 7
}

/// The time input of an element's parameter: sources 2 and 3 take the
/// owner's frame (`owner+0xFC`, the owner at `element+0`), 4 the frame
/// counter, every other source the element's own frame (`element+0xB0`).
// Translated from 0x011E48E0 (DDDA.exe 2017-12-13 Steam).
pub fn element_time(m: &dyn Mem, element: u32, param: u32) -> u32 {
    match time_source(m, param) {
        2 | 3 => m.u32(m.u32(element).wrapping_add(0xFC)),
        SOURCE_FRAME_COUNTER => Effects::get(m).frame_counter(m),
        _ => m.u32(element.wrapping_add(0xB0)),
    }
}

/// The time input of an emitter's parameter: source 2 takes the unit's
/// frame (`unit+0x10C`), 3 its parent's (`*(unit+0x1D0)+0x10C`, the
/// unit's own without a parent), 4 the frame counter, every other source
/// the emitter's frame (`emitter+0x88`).
// Translated from 0x00E6C260 (DDDA.exe 2017-12-13 Steam).
pub fn emitter_time(m: &dyn Mem, unit: u32, param: u32, emitter: u32) -> u32 {
    match time_source(m, param) {
        2 => m.u32(unit.wrapping_add(0x10C)),
        3 => match m.u32(unit.wrapping_add(0x1D0)) {
            0 => m.u32(unit.wrapping_add(0x10C)),
            parent => m.u32(parent.wrapping_add(0x10C)),
        },
        SOURCE_FRAME_COUNTER => Effects::get(m).frame_counter(m),
        _ => m.u32(emitter.wrapping_add(0x88)),
    }
}
