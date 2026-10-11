//! The readable ports behind the lifted program's calling convention.
//!
//! Every `f_XXXXXXXX` here has the signature of a lifted function
//! (`fn(&mut dyn Mem, &mut Cpu)`, entered with the return address at
//! `[esp]`) and replaces the lifted body of the original at that address:
//! `research/native/lift.py gen` makes each shard's `generated/mod.rs`
//! import it over the lifted one, so direct calls, the resolver
//! (`x86::call_lifted`) and the runtime's dispatcher all reach the port.
//!
//! An adapter reads the arguments where the original does, calls the port
//! and leaves the machine as the original's `ret` would, as far as the
//! lifted contract observes it (docs/MATH.md, "Wiring"):
//!
//! - all eight general registers, including the scratch values the
//!   original leaves in EAX/ECX/EDX;
//! - every byte the original writes, including its pushes below ESP;
//! - xmm0 (all four lanes) and the x87 state.
//!
//! Where the original leaves scratch values in other xmm registers, the
//! adapter either reproduces them too (the mask in [`PORTS`]) or leaves
//! them as they were; `research/native/math_liveness.py` checks every
//! direct call site of every function here: no caller reads a lane the
//! adapter does not reproduce after the call before writing it.

use crate::angle;
use crate::matrix;
use crate::random::Random;
use crate::scalar::F32;
use crate::types::Mat44;
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// The 32-bit stack argument `k` (0 = the first, at `[esp + 4]`).
fn arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

fn arg_f32(m: &dyn Mem, cpu: &Cpu, k: u32) -> F32 {
    F32::from_bits(arg(m, cpu, k))
}

/// `ret pop`: drops the return address and `pop` bytes of arguments.
fn ret(cpu: &mut Cpu, pop: u32) {
    cpu.esp = cpu.esp.wrapping_add(4 + pop);
}

/// A push the original makes and pops again: the value stays in memory
/// below the caller's ESP.
fn pushed(m: &mut dyn Mem, cpu: &Cpu, depth: u32, value: u32) {
    m.set_u32(cpu.esp.wrapping_sub(4 * depth), value);
}

fn store_matrix(m: &mut dyn Mem, addr: u32, mat: &Mat44) {
    for i in 0..16 {
        m.set_u32(addr.wrapping_add(4 * i as u32), mat.at(i).to_bits());
    }
}

fn load_random(m: &dyn Mem, addr: u32) -> Random {
    let w = |k: u32| m.u32(addr.wrapping_add(4 * k));
    Random { x: w(0), y: w(1), z: w(2), w: w(3) }
}

fn store_random(m: &mut dyn Mem, addr: u32, r: &Random) {
    for (k, v) in [r.x, r.y, r.z, r.w].into_iter().enumerate() {
        m.set_u32(addr.wrapping_add(4 * k as u32), v);
    }
}

fn xmm0_scalar(cpu: &mut Cpu, v: F32) {
    cpu.xmm[0] = [v.to_bits(), 0, 0, 0];
}

/// thiscall `u32 MtRandom::next()`: ECX = the state. Leaves t in EDX;
/// ESI is pushed and popped.
pub fn f_00d00690(m: &mut dyn Mem, cpu: &mut Cpu) {
    let mut r = load_random(m, cpu.ecx);
    let d = r.next();
    store_random(m, cpu.ecx, &r);
    pushed(m, cpu, 1, cpu.esi);
    cpu.eax = d.value;
    cpu.edx = d.t;
    ret(cpu, 0);
}

/// thiscall `void MtRandom::seed(u32)`, `ret 4`. The original pushes EBX,
/// EBP, ESI, EDI and counts its loop in the argument's stack slot, which
/// ends at 0. Leaves the last scrambled seed in EAX, its t in ECX and the
/// new `y` in EDX.
pub fn f_00d006c0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let seed = arg(m, cpu, 0);
    let mut r = Random::default();
    let s = r.seed(seed);
    store_random(m, cpu.ecx, &r);
    for (depth, v) in [cpu.ebx, cpu.ebp, cpu.esi, cpu.edi].into_iter().enumerate() {
        pushed(m, cpu, depth as u32 + 1, v);
    }
    m.set_u32(cpu.esp.wrapping_add(4), 0);
    cpu.eax = s.scrambled;
    cpu.ecx = s.t;
    cpu.edx = r.y;
    ret(cpu, 4);
}

/// The four projection builders: thiscall `(Mat44 *this, float width,
/// float height, float near, float far)`, `ret 0x10`.
fn projection(m: &mut dyn Mem, cpu: &mut Cpu, build: fn(F32, F32, F32, F32) -> Mat44) -> [F32; 4] {
    let a = [0, 1, 2, 3].map(|k| arg_f32(m, cpu, k));
    store_matrix(m, cpu.ecx, &build(a[0], a[1], a[2], a[3]));
    ret(cpu, 0x10);
    a
}

pub fn f_00cfc770(m: &mut dyn Mem, cpu: &mut Cpu) {
    projection(m, cpu, matrix::perspective_rh);
    xmm0_scalar(cpu, F32::ZERO);
}

pub fn f_00cfc810(m: &mut dyn Mem, cpu: &mut Cpu) {
    projection(m, cpu, matrix::perspective_lh);
    xmm0_scalar(cpu, F32::ZERO);
}

pub fn f_00cfc8b0(m: &mut dyn Mem, cpu: &mut Cpu) {
    projection(m, cpu, matrix::ortho_rh);
    xmm0_scalar(cpu, F32::ZERO);
}

/// Leaves `near - far` in xmm0.
pub fn f_00cfc940(m: &mut dyn Mem, cpu: &mut Cpu) {
    let [_, _, near, far] = projection(m, cpu, matrix::ortho_lh);
    xmm0_scalar(cpu, near - far);
}

/// `float angle_difference(float a /* xmm0 */, float b /* xmm1 */)`,
/// result in xmm0. Pushes EBP (an aligned frame it never uses).
///
/// Callers go on to the CRT's SSE2 `atan2`/`pow`, whose packed operations
/// carry the upper lanes of xmm2 and xmm4 along, so this adapter rebuilds
/// all of xmm0..xmm4 as the original leaves them.
pub fn f_007ad870(m: &mut dyn Mem, cpu: &mut Cpu) {
    use angle::AnglePath::*;
    let lanes = |v: F32, upper: [u32; 3]| [v.to_bits(), upper[0], upper[1], upper[2]];
    let (a, b) = (F32::from_bits(cpu.xmm[0][0]), F32::from_bits(cpu.xmm[1][0]));
    let up0: [u32; 3] = cpu.xmm[0][1..].try_into().unwrap();
    let up1: [u32; 3] = cpu.xmm[1][1..].try_into().unwrap();
    let t = angle::angle_difference_traced(a, b);
    pushed(m, cpu, 1, cpu.ebp);
    let two_pi = lanes(angle::TWO_PI, [0; 3]);
    if t.path == SameSide {
        cpu.xmm[0] = lanes(t.result, up0);
        cpu.xmm[2] = [0; 4];
        cpu.xmm[3] = lanes(a * b, up0);
    } else {
        let gap = lanes(t.a - t.b, up0);
        cpu.xmm[2] = gap;
        cpu.xmm[3] = two_pi;
        cpu.xmm[4] = gap.map(|v| v & 0x7FFF_FFFF);
        (cpu.xmm[0], cpu.xmm[1]) = match t.path {
            Shifted => (lanes(t.result, up0), lanes(t.b, up1)),
            WrappedUp => (lanes(t.result, [0; 3]), lanes(t.b - t.a, up1)),
            _ => (lanes(t.result, up0), two_pi),
        };
    }
    ret(cpu, 0);
}

/// Every port above with its original address and the xmm registers it
/// leaves exactly as the original does (bit k = xmmk; the others keep their
/// entry values, see the module comment).
pub const PORTS: &[(u32, ddnative_base::x86::LiftFn, u8)] = &[
    (0x007A_D870, f_007ad870, 0b0001_1111),
    (0x00CF_C770, f_00cfc770, 0b0000_0001),
    (0x00CF_C810, f_00cfc810, 0b0000_0001),
    (0x00CF_C8B0, f_00cfc8b0, 0b0000_0001),
    (0x00CF_C940, f_00cfc940, 0b0000_0001),
    (0x00D0_0690, f_00d00690, 0b1111_1111),
    (0x00D0_06C0, f_00d006c0, 0b1111_1111),
];
