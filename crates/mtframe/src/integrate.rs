//! Character integration, 0x0084CCC0 (docs/FRAME_RATE.md 4.3).
//!
//! Called with the character in esi. Each frame:
//! 1. Frozen (status bit 0x800) scales the horizontal velocity by
//!    max(0, 1 − `+0x2864`), Petrify (0x1000) by `+0x27B0`: per frame,
//!    with no step (frame-counted).
//! 2. The position moves by velocity × step, by the push accumulated this
//!    frame × a factor × step, and by an attraction toward a point.
//! 3. The velocity gains acceleration × step (explicit Euler: after the
//!    position) and is clamped per axis.

use crate::fpu;
use ddnative_base::mem::Mem;
use ddnative_base::x87::X87;
use mtmath::scalar::F32;

/// The character's fields.
pub const STEP: u32 = 0x14;
pub const PARENT: u32 = 0x30;
pub const POSITION: u32 = 0x40;
pub const GROUND: u32 = 0xE0;
pub const FLAGS_236C: u32 = 0x236C;
pub const VELOCITY: u32 = 0x2530;
pub const ACCELERATION: u32 = 0x2540;
pub const VELOCITY_MAX: u32 = 0x2550;
pub const VELOCITY_MIN: u32 = 0x2560;
/// The status bits (64-bit; the low word holds Frozen 0x800, Petrify 0x1000).
pub const STATUS: u32 = 0x26A0;
pub const FROZEN: u32 = 0x800;
pub const PETRIFIED: u32 = 0x1000;
pub const PETRIFY_FACTOR: u32 = 0x27B0;
pub const FROZEN_DAMPING: u32 = 0x2864;
/// +0x2A20 the push factor this frame; +0x2A30 the push accumulated since
/// last frame (consumed: reset to `ORIGIN`); +0x2A40 last frame's push.
pub const PUSH_FACTOR: u32 = 0x2A20;
pub const PUSH: u32 = 0x2A30;
pub const LAST_PUSH: u32 = 0x2A40;
/// The attraction: +0x2D80 its point, +0x2D90 the movement it gave this
/// frame, +0x2DA0 its strength, +0x2DA4 its radius, +0x2DA8 the index of
/// its scale's key in `ATTRACTION_KEYS`, +0x2DAC active (u8, cleared
/// every frame), +0x2DAD suppressed (u8).
pub const ATTRACT_POINT: u32 = 0x2D80;
pub const ATTRACTED: u32 = 0x2D90;
pub const ATTRACT_STRENGTH: u32 = 0x2DA0;
pub const ATTRACT_RADIUS: u32 = 0x2DA4;
pub const ATTRACT_KEY: u32 = 0x2DA8;
pub const ATTRACT_ACTIVE: u32 = 0x2DAC;
pub const ATTRACT_SUPPRESSED: u32 = 0x2DAD;
/// `+0x236C & 0x40000`: no push factor (it is 0).
pub const NO_PUSH_FACTOR: u32 = 0x4_0000;

/// A global vector the code reads as the reset value (zero in practice).
pub const ORIGIN: u32 = 0x018C_FE80;
/// The manager whose `+0xB8754` object scales the attraction.
pub const MANAGER: u32 = 0x018F_A4BC;
pub const MANAGER_SCALER: u32 = 0xB_8754;
/// The keys the attraction scale is looked up by (u32 each).
pub const ATTRACTION_KEYS: u32 = 0x0159_E1A0;

const ONE: F32 = F32(1.0); // [0x015319B8]

/// The calls the integration makes and what it shows its caller.
pub trait Calls {
    /// The character's virtual +0x1EC: the push factor (an x87 result).
    fn push_factor(&mut self, m: &mut dyn Mem, character: u32) -> F32;
    /// The scaler's virtual +0x3C(key): the attraction scale (x87).
    fn attraction_scale(&mut self, m: &mut dyn Mem, scaler: u32, key: u32) -> F32;
    /// The x87 the push is copied through.
    fn fpu(&mut self) -> &mut X87;
    /// Observes the frame's movement before the push factor is taken.
    fn moving(&mut self, _m: &mut dyn Mem, _f: &Frame) {}
    /// Observes the push scaled by factor and step.
    fn pushed(&mut self, _m: &mut dyn Mem, _f: &Frame) {}
    /// Observes the attraction before its scale is taken.
    fn attracting(&mut self, _m: &mut dyn Mem, _f: &Frame, _a: &Attraction) {}
}

/// What the frame computed so far (the adapter's view).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub step: F32,
    /// velocity × step
    pub moved: [F32; 3],
    /// the push accumulated since last frame
    pub push: [F32; 3],
    pub factor: F32,
    /// push × factor × step
    pub pushed: [F32; 3],
    /// the attraction's movement (`ORIGIN` without attraction)
    pub toward: [F32; 3],
    /// the position after velocity × step
    pub moved_to: [F32; 3],
    /// the position after the push (x before the attraction is added)
    pub pushed_to: [F32; 3],
    /// the velocity after the acceleration, before the clamps
    pub velocity: [F32; 3],
    /// the velocity after the maximum (x, y, z) and the minimum (x, y)
    pub below_max: [F32; 3],
    pub above_min: [F32; 2],
}

/// The attraction this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attraction {
    /// point − ground, x and z
    pub dx: F32,
    pub dz: F32,
    /// the pull before the scale: toward the point beyond the radius,
    /// else `ORIGIN`
    pub pull: [F32; 3],
    /// beyond the radius
    pub outside: bool,
    /// dz × (1 / distance) × strength beyond the radius (else dz)
    pub z_term: F32,
}

fn f(m: &dyn Mem, a: u32) -> F32 {
    F32::from_bits(m.u32(a))
}

fn set(m: &mut dyn Mem, a: u32, v: F32) {
    m.set_u32(a, v.to_bits())
}

fn vec3(m: &dyn Mem, a: u32) -> [F32; 3] {
    std::array::from_fn(|k| f(m, a.wrapping_add(4 * k as u32)))
}

/// Scales the horizontal velocity by `k` (Frozen, Petrify): once per frame.
fn brake(m: &mut dyn Mem, c: u32, k: F32) {
    let (x, z) = (c.wrapping_add(VELOCITY), c.wrapping_add(VELOCITY + 8));
    let (vx, vz) = (k * f(m, x), k * f(m, z));
    set(m, x, vx);
    set(m, z, vz);
}

/// One frame of a character's integration; returns what it computed.
// Translated from 0x0084CCC0 (DDDA.exe 2017-12-13 Steam).
pub fn update(m: &mut dyn Mem, c: u32, calls: &mut impl Calls) -> Frame {
    let at = |o: u32| c.wrapping_add(o);
    let step = f(m, at(STEP));
    if m.u32(at(STATUS)) & FROZEN != 0 {
        // max(0, 1 − damping): `comiss 0, k` + `jbe` (NaN stays)
        let k = ONE - f(m, at(FROZEN_DAMPING));
        let k = if F32::ZERO.0 > k.0 { F32::ZERO } else { k };
        brake(m, c, k);
    }
    if m.u32(at(STATUS)) & PETRIFIED != 0 {
        let k = f(m, at(PETRIFY_FACTOR));
        brake(m, c, k);
    }
    let v = vec3(m, at(VELOCITY));
    let mut frame = Frame {
        step,
        moved: v.map(|c| c * step),
        push: vec3(m, at(PUSH)),
        ..Frame::default()
    };
    calls.moving(m, &frame);
    frame.factor = if m.u32(at(FLAGS_236C)) & NO_PUSH_FACTOR == 0 {
        calls.push_factor(m, c)
    } else {
        F32::ZERO
    };
    set(m, at(PUSH_FACTOR), frame.factor);
    // last frame's push is kept (copied through the x87), the push consumed
    for k in 0..3 {
        let bits = m.u32(at(PUSH + 4 * k));
        let fpu = calls.fpu();
        fpu::load_f32(fpu, bits);
        let copy = fpu::store_f32(fpu);
        m.set_u32(at(LAST_PUSH + 4 * k), copy);
    }
    m.set_u32(at(LAST_PUSH + 0xC), 0);
    for k in 0..3 {
        m.set_u32(at(PUSH + 4 * k), m.u32(ORIGIN + 4 * k));
    }
    m.set_u32(at(PUSH + 0xC), 0);
    frame.pushed = frame.push.map(|p| p * frame.factor * step);
    calls.pushed(m, &frame);
    // the attraction toward a point (xz), outside its radius
    frame.toward = vec3(m, ORIGIN);
    if m.u32(at(PARENT)) == 0 && m.u8(at(ATTRACT_SUPPRESSED)) == 0 && m.u8(at(ATTRACT_ACTIVE)) != 0 {
        let dx = f(m, at(ATTRACT_POINT)) - f(m, at(GROUND));
        let dz = f(m, at(ATTRACT_POINT + 8)) - f(m, at(GROUND + 8));
        let distance = (dz * dz + dx * dx).sqrt();
        let outside = distance.0 > f(m, at(ATTRACT_RADIUS)).0;
        let (pull, z_term) = if outside {
            let inv = ONE / distance;
            let strength = f(m, at(ATTRACT_STRENGTH));
            let z = dz * inv * strength;
            ([inv * dx * strength, inv * F32::ZERO * strength, z], z)
        } else {
            (vec3(m, ORIGIN), dz)
        };
        let attraction = Attraction { dx, dz, pull, outside, z_term };
        calls.attracting(m, &frame, &attraction);
        let key = m.u32(ATTRACTION_KEYS.wrapping_add(m.u32(at(ATTRACT_KEY)).wrapping_mul(4)));
        let scaler = m.u32(m.u32(MANAGER).wrapping_add(MANAGER_SCALER));
        let scale = if scaler != 0 { calls.attraction_scale(m, scaler, key) } else { F32::ZERO };
        frame.toward = [pull[0] * scale * step, scale * pull[1] * step, pull[2] * scale * step];
        for (k, t) in frame.toward.iter().enumerate() {
            set(m, at(ATTRACTED + 4 * k as u32), *t);
        }
        m.set_u32(at(ATTRACTED + 0xC), 0);
    }
    m.set_u8(at(ATTRACT_ACTIVE), 0);
    // position: velocity, then push, then attraction
    for k in 0..3 {
        let p = at(POSITION + 4 * k as u32);
        frame.moved_to[k] = frame.moved[k] + f(m, p);
        set(m, p, frame.moved_to[k]);
    }
    frame.pushed_to[0] = frame.pushed[0] + f(m, at(POSITION));
    for k in 1..3 {
        let p = at(POSITION + 4 * k as u32);
        frame.pushed_to[k] = frame.pushed[k] + f(m, p);
        set(m, p, frame.pushed_to[k]);
    }
    for k in 1..3 {
        let p = at(POSITION + 4 * k as u32);
        let v = frame.toward[k] + f(m, p);
        set(m, p, v);
    }
    set(m, at(POSITION), frame.toward[0] + frame.pushed_to[0]);
    // velocity += acceleration × step (z added the other way round)
    let a = vec3(m, at(ACCELERATION));
    let vx = a[0] * step + f(m, at(VELOCITY));
    let vy = a[1] * step + f(m, at(VELOCITY + 4));
    let vz = f(m, at(VELOCITY + 8)) + a[2] * step;
    frame.velocity = [vx, vy, vz];
    set(m, at(VELOCITY + 8), vz);
    set(m, at(VELOCITY), vx);
    set(m, at(VELOCITY + 4), vy);
    // clamped per axis to the maximum (`comiss max, v` + `ja` keeps v; NaN
    // takes the maximum), then to the minimum (`comiss v, min` + `jbe`
    // takes the minimum; NaN too); each axis re-read from memory
    let below = |v: F32, max: F32| if max.0 > v.0 { v } else { max };
    let above = |v: F32, min: F32| if v.0 > min.0 { v } else { min };
    let vx = below(vx, f(m, at(VELOCITY_MAX)));
    let vy = below(f(m, at(VELOCITY + 4)), f(m, at(VELOCITY_MAX + 4)));
    set(m, at(VELOCITY), vx);
    let vz = below(f(m, at(VELOCITY + 8)), f(m, at(VELOCITY_MAX + 8)));
    set(m, at(VELOCITY + 4), vy);
    frame.below_max = [vx, vy, vz];
    let vx = above(vx, f(m, at(VELOCITY_MIN)));
    set(m, at(VELOCITY + 8), vz);
    set(m, at(VELOCITY), vx);
    let vy = above(vy, f(m, at(VELOCITY_MIN + 4)));
    set(m, at(VELOCITY + 4), vy);
    frame.above_min = [vx, vy];
    let vz = above(vz, f(m, at(VELOCITY_MIN + 8)));
    set(m, at(VELOCITY + 8), vz);
    frame
}
