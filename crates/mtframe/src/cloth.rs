//! The cloth step schedule inside `uSimSoftBody::vf11` (0x00F20640;
//! docs/FRAME_RATE.md 4.2: locked, with a dropped remainder).
//!
//! **Observable-equivalent model, not wired; the high-frame-rate mode's
//! baseline.** vf11 (11 KB, about 40 calls) stays lifted: a strict port
//! would have to replay dead state. This models only its step schedule,
//! the instructions 0x00F20847..0x00F208FC, and is verified against
//! recordings of the original over exactly that range
//! (`crates/mtframe/tests/models.rs`): the fields it writes and the step
//! count it leaves in eax.
//!
//! The cloth runs fixed steps of `step` seconds (1/60, 1/150 at the
//! higher quality setting). Each frame it adds this frame's time (at most
//! 0.05 s) to an accumulator and takes as many whole steps as fit; when it
//! takes any, it **drops the remainder** (the accumulator becomes 0)
//! instead of keeping it, so cloth time falls behind real time unless the
//! frame time is a whole number of steps.

use crate::smain::Main;
use ddnative_base::mem::Mem;
use mtmath::scalar::{x87_copy, F32};

/// uSimSoftBody's fields: +0xC0 the step (s), +0xC4 this frame's time
/// (s), +0xCC last frame's time, +0x150 the accumulator, +0x158 the steps
/// taken this frame, +0x9AC this frame's time in 60 Hz frames.
pub const STEP: u32 = 0xC0;
pub const FRAME_TIME: u32 = 0xC4;
pub const LAST_FRAME_TIME: u32 = 0xCC;
pub const ACCUMULATOR: u32 = 0x150;
pub const STEPS: u32 = 0x158;
pub const FRAMES_60: u32 = 0x9AC;

/// The longest frame time the cloth takes (`[0x015592DC]` = 0.05 s).
pub const MAX_FRAME_TIME: F32 = F32(0.05);
const SIXTY: F32 = F32(60.0); // [0x01408A88]

fn f(m: &dyn Mem, a: u32) -> F32 {
    F32::from_bits(m.u32(a))
}

/// This frame's step schedule; returns the steps to take.
// Model of 0x00F20847..0x00F208FC (DDDA.exe 2017-12-13 Steam), in uSimSoftBody::vf11.
pub fn schedule(m: &mut dyn Mem, body: u32) -> u32 {
    let at = |o: u32| body.wrapping_add(o);
    let main = Main::get(m);
    // this frame's time in seconds, at most 0.05 (`comiss`/`jbe`: NaN kept)
    let frame = F32::from_bits(main.delta_time_bits(m)) / F32(main.fps(m));
    let frame = if frame.0 > MAX_FRAME_TIME.0 { MAX_FRAME_TIME } else { frame };
    let last = x87_copy(f32::from_bits(m.u32(at(FRAME_TIME))));
    m.set_u32(at(LAST_FRAME_TIME), last.to_bits());
    let step = f(m, at(STEP));
    let mut acc = frame + f(m, at(ACCUMULATOR));
    m.set_u32(at(ACCUMULATOR), acc.to_bits());
    m.set_u32(at(FRAME_TIME), frame.to_bits());
    m.set_u32(at(STEPS), 0);
    // whole steps (`comiss acc, step`/`jb`: NaN takes none)
    if !(acc.0 < step.0 || acc.0.is_nan() || step.0.is_nan()) {
        let mut n = 0u32;
        loop {
            n += 1;
            acc = acc - step;
            m.set_u32(at(STEPS), n);
            m.set_u32(at(ACCUMULATOR), acc.to_bits());
            // `comiss acc, [step]`/`jae` (re-read; NaN stops)
            if !(acc.0 >= f(m, at(STEP)).0) {
                break;
            }
        }
    }
    let steps = m.u32(at(STEPS));
    if steps != 0 {
        // the remainder is dropped
        m.set_u32(at(ACCUMULATOR), 0);
    }
    m.set_u32(at(FRAMES_60), (frame * SIXTY).to_bits());
    steps
}
