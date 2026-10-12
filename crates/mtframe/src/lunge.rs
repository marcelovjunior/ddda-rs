//! em5501's rush-bite lunge, `cEm5501ActAtckRushBite::vf06`
//! (docs/FRAME_RATE.md 3: frame-counted).
//!
//! While the attack motion is between frames 30 and 60, the enemy moves
//! along its facing by its current lunge speed **every frame**, with no
//! step: at twice the frame rate the lunge covers twice the distance in
//! the same motion window. The speed itself ramps up with the unit step
//! (0x00A00BF0, dt-correct).

use mtmath::scalar::F32;
use ddnative_base::mem::Mem;

/// The action: +0x44 its owner (vtable +0x58: the facing matrix),
/// +0x50 finished (u8), +0x74 the enemy.
pub const OWNER: u32 = 0x44;
pub const FINISHED: u32 = 0x50;
pub const ENEMY: u32 = 0x74;

/// The enemy: +0x40 position (x, y, z, w), +0x380 its motion (frame at
/// motion+0x4C), +0x74D0 lunge velocity (x, y, z, w), +0x74E0 lunge speed,
/// +0x7590 its top speed, +0x7594 the ramp time, +0xF0C the motion whose
/// end finishes the action (at +0x380 of it), +0x2360 and +0x2370 two
/// 64-bit flag words.
pub const POSITION: u32 = 0x40;
pub const MOTION: u32 = 0x380;
pub const MOTION_FRAME: u32 = 0x4C;
pub const VELOCITY: u32 = 0x74D0;
pub const SPEED: u32 = 0x74E0;
pub const TOP_SPEED: u32 = 0x7590;
pub const RAMP_TIME: u32 = 0x7594;
pub const END_MOTION: u32 = 0xF0C;
pub const FLAGS: [u32; 2] = [0x2360, 0x2370];

/// The lunge window in motion frames: `[0x014EA160]` = 30.0 and
/// `[0x014EA2D8]` = 60.0 (both exclusive).
pub const WINDOW: (f32, f32) = (30.0, 60.0);

/// What one lunging frame did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    /// The velocity stored at +0x74D0 (w = 0).
    pub velocity: [F32; 3],
    /// The new position (w = 0).
    pub position: [F32; 3],
}

/// The calls the update makes.
pub trait Calls {
    /// 0x00A00BF0: ramps the lunge speed toward the top speed by the
    /// unit step (the enemy, its top speed and its ramp time).
    fn ramp(&mut self, m: &mut dyn Mem, enemy: u32);
    /// The owner's facing matrix (its virtual +0x58, argument −1).
    fn facing(&mut self, m: &mut dyn Mem, owner: u32) -> u32;
    /// Observes a lunging frame (after the position is stored).
    fn lunged(&mut self, _step: &Step) {}
    /// 0x010570F0: whether the motion has ended (bit 2 of its byte 6).
    fn motion_ended(&mut self, m: &mut dyn Mem, motion: u32) -> bool;
}

fn f(m: &dyn Mem, a: u32) -> F32 {
    F32::from_bits(m.u32(a))
}

/// One frame of the rush-bite action. Without an enemy the action ends.
/// Inside the lunge window the enemy is pushed along its facing (xz only)
/// by the lunge speed, one full speed per frame. The action ends when the
/// end motion has; both flag words get bits 0x101 every frame.
// Translated from 0x00A36DF0 (DDDA.exe 2017-12-13 Steam), cEm5501ActAtckRushBite::vf06.
pub fn update(m: &mut dyn Mem, action: u32, calls: &mut impl Calls) {
    let enemy = m.u32(action.wrapping_add(ENEMY));
    if enemy == 0 {
        m.set_u8(action.wrapping_add(FINISHED), 1);
        return;
    }
    let motion = enemy.wrapping_add(MOTION);
    if motion != 0 {
        let frame = f(m, motion.wrapping_add(MOTION_FRAME)).0;
        if frame > WINDOW.0 && WINDOW.1 > frame {
            calls.ramp(m, enemy);
            let owner = m.u32(action.wrapping_add(OWNER));
            let matrix = calls.facing(m, owner);
            // the facing's z axis, flattened: (m[2][0], m[2][2])
            let (x, z) = (f(m, matrix.wrapping_add(0x20)), f(m, matrix.wrapping_add(0x28)));
            let enemy = m.u32(action.wrapping_add(ENEMY));
            let length = (z * z + x * x).sqrt();
            let scale = F32::ONE / length * f(m, enemy.wrapping_add(SPEED));
            let velocity = [x * scale, scale * F32::ZERO, z * scale];
            let v = enemy.wrapping_add(VELOCITY);
            m.set_u32(v + 0xC, 0);
            for (k, c) in velocity.iter().enumerate() {
                m.set_u32(v.wrapping_add(4 * k as u32), c.to_bits());
            }
            let enemy = m.u32(action.wrapping_add(ENEMY));
            let p = enemy.wrapping_add(POSITION);
            let position: [F32; 3] = std::array::from_fn(|k| f(m, p.wrapping_add(4 * k as u32)) + velocity[k]);
            for (k, c) in position.iter().enumerate() {
                m.set_u32(p.wrapping_add(4 * k as u32), c.to_bits());
            }
            m.set_u32(p.wrapping_add(0xC), 0);
            calls.lunged(&Step { velocity, position });
        }
    }
    let end = m.u32(m.u32(action.wrapping_add(ENEMY)).wrapping_add(END_MOTION)).wrapping_add(MOTION);
    if calls.motion_ended(m, end) {
        m.set_u8(action.wrapping_add(FINISHED), 1);
    }
    let enemy = m.u32(action.wrapping_add(ENEMY));
    // a 64-bit `|= 0x101`: the high words are read first and written back
    let high = FLAGS.map(|w| m.u32(enemy.wrapping_add(w + 4)));
    for (w, high) in FLAGS.into_iter().zip(high) {
        let low = enemy.wrapping_add(w);
        m.set_u32(low, m.u32(low) | 0x101);
        m.set_u32(low.wrapping_add(4), high);
    }
}
