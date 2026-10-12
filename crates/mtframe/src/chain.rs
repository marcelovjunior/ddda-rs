//! Long chains, `uChain::vf09` (docs/FRAME_RATE.md 3: dt-approximate).
//!
//! Each frame the chain takes the unit step (at most 2 base frames) times
//! its time scale as this frame's time, and splits it into a fixed number
//! of substeps (`+0x88`): the substep is `time / count`. The solver then
//! runs `count` substeps per frame, so at a higher frame rate it runs more
//! substeps per second, each shorter.

use crate::fpu;
use ddnative_base::mem::Mem;
use ddnative_base::x87::X87;
use mtmath::scalar::F32;

/// uChain: +0x14 the unit step, +0x30 the joint it follows (position at
/// +0xE0, orientation quaternion at +0x50), +0x55 disabled (u8), +0x78
/// this frame's time, +0x7C the substep, +0x88 substeps per frame (u32),
/// +0xCC the time scale, +0xD8 settling steps still to run, +0xE0 the
/// joint's position last frame, +0xF0 its movement this frame, +0x100 its
/// rotation (4×4, rows), +0x1A8 (set to −1 every frame).
pub const UNIT_STEP: u32 = 0x14;
pub const JOINT: u32 = 0x30;
pub const DISABLED: u32 = 0x55;
pub const TIME: u32 = 0x78;
pub const SUBSTEP: u32 = 0x7C;
pub const SUBSTEPS: u32 = 0x88;
pub const TIME_SCALE: u32 = 0xCC;
pub const SETTLE: u32 = 0xD8;
pub const LAST_POSITION: u32 = 0xE0;
pub const MOVEMENT: u32 = 0xF0;
pub const ROTATION: u32 = 0x100;
pub const RESET_1A8: u32 = 0x1A8;
/// The joint's position and orientation.
pub const JOINT_POSITION: u32 = 0xE0;
pub const JOINT_ORIENTATION: u32 = 0x50;

/// The longest step a frame takes, in base frames (`[0x01406CEC]`).
pub const MAX_STEP: F32 = F32(2.0);
const TWO: F32 = F32(2.0); // [0x01406CEC]
const ONE: F32 = F32(1.0); // [0x01406CE0]

/// The products of a unit quaternion (x, y, z, w) a rotation is built
/// from, in the original's order (`x2` = x × 2, ...).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Products {
    pub xx: F32,
    pub xy: F32,
    pub xz: F32,
    pub yy: F32,
    pub yz: F32,
    pub zz: F32,
    pub wx: F32,
    pub wy: F32,
    pub wz: F32,
}

pub fn products([x, y, z, w]: [F32; 4]) -> Products {
    let (x2, y2, z2) = (x * TWO, y * TWO, z * TWO);
    Products {
        xx: x2 * x,
        xy: y2 * x,
        xz: z2 * x,
        yy: y * y2,
        yz: y * z2,
        zz: z2 * z,
        wx: w * x2,
        wy: w * y2,
        wz: w * z2,
    }
}

/// The rotation (rows) of a unit quaternion's products.
pub fn rotation(p: &Products) -> [[F32; 3]; 3] {
    [
        [ONE - (p.zz + p.yy), p.xy + p.wz, p.xz - p.wy],
        [p.xy - p.wz, ONE - (p.zz + p.xx), p.yz + p.wx],
        [p.xz + p.wy, p.yz - p.wx, ONE - (p.yy + p.xx)],
    ]
}

/// The calls the update makes, and what it shows its caller.
pub trait Calls {
    /// 0x00F0BDC0: refreshes the chain's links; whether it can run.
    fn ready(&mut self, m: &mut dyn Mem) -> bool;
    /// 0x011259E0 (`uJointAnimator::vf09`): the joint animation.
    fn animate(&mut self, m: &mut dyn Mem);
    /// Observes the joint's orientation as quaternion, products and
    /// rotation, before the rotation is stored.
    fn oriented(&mut self, _m: &mut dyn Mem, _q: [F32; 4], _p: &Products, _rows: &[[F32; 3]; 3]) {}
    /// 0x00F0BEB0: runs `steps` settling steps of one base frame each.
    fn settle(&mut self, m: &mut dyn Mem, steps: u32);
    /// The x87 the substep is divided on.
    fn fpu(&mut self) -> &mut X87;
    /// Observes this frame's step (clamped), time and substep count.
    fn stepped(&mut self, _m: &mut dyn Mem, _step: F32, _time: F32, _count: u32) {}
    /// 0x00F07270: prepares the substeps.
    fn prepare(&mut self, m: &mut dyn Mem);
    /// The chain's virtual +0x5C and +0x58 (in this order).
    fn virtual_5c(&mut self, m: &mut dyn Mem);
    fn virtual_58(&mut self, m: &mut dyn Mem);
    /// 0x00F01390: the solver, `count` substeps.
    fn solve(&mut self, m: &mut dyn Mem);
    /// Observes the joint and its position, stored as last position.
    fn followed(&mut self, _joint: u32, _position: [F32; 3]) {}
}

fn f(m: &dyn Mem, a: u32) -> F32 {
    F32::from_bits(m.u32(a))
}

fn set(m: &mut dyn Mem, a: u32, v: F32) {
    m.set_u32(a, v.to_bits())
}

/// One frame of a long chain.
// Translated from 0x00F0C160 (DDDA.exe 2017-12-13 Steam), uChain::vf09.
pub fn update(m: &mut dyn Mem, chain: u32, calls: &mut impl Calls) {
    let at = |o: u32| chain.wrapping_add(o);
    if m.u8(at(DISABLED)) != 0 || !calls.ready(m) {
        return;
    }
    calls.animate(m);
    // the joint's movement since last frame
    let joint = m.u32(at(JOINT));
    let p: [F32; 3] = std::array::from_fn(|k| f(m, joint.wrapping_add(JOINT_POSITION + 4 * k as u32)));
    set(m, at(MOVEMENT + 4), p[1]);
    set(m, at(MOVEMENT + 8), p[2]);
    set(m, at(MOVEMENT), p[0]);
    m.set_u32(at(MOVEMENT + 0xC), 0);
    for k in 0..3 {
        let (moved, last) = (at(MOVEMENT + 4 * k), at(LAST_POSITION + 4 * k));
        let d = f(m, moved) - f(m, last);
        set(m, moved, d);
    }
    // its orientation as a rotation
    let joint = m.u32(at(JOINT));
    let q: [F32; 4] = std::array::from_fn(|k| f(m, joint.wrapping_add(JOINT_ORIENTATION + 4 * k as u32)));
    let products = products(q);
    let rows = rotation(&products);
    calls.oriented(m, q, &products, &rows);
    for (i, row) in rows.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            set(m, at(ROTATION + 16 * i as u32 + 4 * j as u32), *v);
        }
    }
    for i in 0..3 {
        m.set_u32(at(ROTATION + 16 * i + 0xC), 0);
    }
    for j in 0..3 {
        m.set_u32(at(ROTATION + 0x30 + 4 * j), 0);
    }
    set(m, at(ROTATION + 0x3C), ONE);
    m.set_u32(at(RESET_1A8), 0xFFFF_FFFF);
    let settle = m.u32(at(SETTLE));
    if settle != 0 {
        calls.settle(m, settle);
        m.set_u32(at(SETTLE), 0);
    }
    // this frame's time, split into the substeps
    let step = f(m, at(UNIT_STEP));
    let step = if step.0 > MAX_STEP.0 { MAX_STEP } else { step };
    let count = m.u32(at(SUBSTEPS));
    let time = f(m, at(TIME_SCALE)) * step;
    set(m, at(TIME), time);
    let fpu = calls.fpu();
    fpu::load_u32(fpu, count);
    fpu::divr_f32(fpu, time.to_bits());
    let substep = fpu::store_f32(fpu);
    m.set_u32(at(SUBSTEP), substep);
    calls.stepped(m, step, time, count);
    calls.prepare(m);
    calls.virtual_5c(m);
    calls.virtual_58(m);
    calls.solve(m);
    // the joint's position, for next frame's movement
    let joint = m.u32(at(JOINT));
    let p: [F32; 3] = std::array::from_fn(|k| f(m, joint.wrapping_add(JOINT_POSITION + 4 * k as u32)));
    for (k, v) in p.iter().enumerate() {
        set(m, at(LAST_POSITION + 4 * k as u32), *v);
    }
    m.set_u32(at(LAST_POSITION + 0xC), 0);
    calls.followed(joint, p);
}
