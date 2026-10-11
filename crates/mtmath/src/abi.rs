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
use crate::euler;
use crate::matrix::{self, ONE};
use crate::random::Random;
use crate::scalar::F32;
use crate::types::{Mat44, Plane, Vec3};
use crate::vector;
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};

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

fn load_vec3(m: &dyn Mem, addr: u32) -> Vec3 {
    let f = |k: u32| F32::from_bits(m.u32(addr.wrapping_add(4 * k)));
    Vec3::new(f(0), f(1), f(2))
}

/// A float the original spills to its stack frame.
fn spill(m: &mut dyn Mem, addr: u32, v: F32) {
    m.set_u32(addr, v.to_bits());
}

/// The frame of `push ebp; mov ebp, esp; and esp, -16; sub esp, size`,
/// relative to the ESP at entry.
fn aligned_frame(cpu: &Cpu, size: u32) -> u32 {
    (cpu.esp.wrapping_sub(4) & !0xF).wrapping_sub(size)
}

/// An xmm register holding one float (`movss` from memory zeroes the
/// upper lanes, and every later write keeps them zero).
fn scalar(v: F32) -> [u32; 4] {
    [v.to_bits(), 0, 0, 0]
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

/// thiscall `MtMatrix *lookAtRH/LH(const MtVector3 *eye, const
/// MtVector3 *at, const MtVector3 *up)`, `ret 0xC`. The original builds
/// an aligned frame and spills the eye, the axes and some products to it;
/// EAX ends as `up`. Every xmm register ends holding one float.
fn look_at(m: &mut dyn Mem, cpu: &mut Cpu, left_handed: bool) {
    let (pe, pa, pu) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let (eye, at, up) = (load_vec3(m, pe), load_vec3(m, pa), load_vec3(m, pu));
    let la = if left_handed { matrix::look_at_lh(eye, at, up) } else { matrix::look_at_rh(eye, at, up) };
    store_matrix(m, cpu.ecx, &la.matrix);
    let (x, y, z) = (la.x, la.y, la.z);
    let f = aligned_frame(cpu, 0x40);
    pushed(m, cpu, 1, cpu.ebp);
    for (off, v) in [(4, eye.x), (8, eye.y), (0xC, eye.z), (0x10, z.x), (0x14, z.y), (0x18, z.z)] {
        spill(m, f + off, v);
    }
    if la.x_normalized {
        spill(m, f + 0x34, x.y);
    }
    let sign = F32::from_bits(0x8000_0000);
    if left_handed {
        spill(m, f + 0x28, x.z);
        spill(m, f + 0x38, eye.z);
        let ey = eye.y * y.y + eye.x * y.x + eye.z * y.z;
        cpu.xmm = [ONE, eye.z * z.z, z.x, z.y, z.z, eye.x * z.x, sign, -ey].map(scalar);
    } else {
        spill(m, f + 0x20, eye.x);
        spill(m, f + 0x28, eye.z);
        spill(m, f + 0x38, x.z);
        cpu.xmm = [ONE, sign, eye.x * z.x, eye.z * z.z, eye.z * y.z, -z.x, -z.y, -z.z].map(scalar);
    }
    cpu.eax = pu;
    ret(cpu, 0xC);
}

pub fn f_00cfd5e0(m: &mut dyn Mem, cpu: &mut Cpu) {
    look_at(m, cpu, false);
}

pub fn f_00cfd890(m: &mut dyn Mem, cpu: &mut Cpu) {
    look_at(m, cpu, true);
}

/// thiscall `MtMatrix *reflect(const MtPlane *plane)`, `ret 4`. EAX ends
/// as `plane`; every xmm register ends holding one float (xmm3 zero).
pub fn f_00cfdb10(m: &mut dyn Mem, cpu: &mut Cpu) {
    let pp = arg(m, cpu, 0);
    let f = |k: u32| F32::from_bits(m.u32(pp.wrapping_add(4 * k)));
    let plane = Plane { a: f(0), b: f(1), c: f(2), d: f(3) };
    let r = matrix::reflect(plane);
    store_matrix(m, cpu.ecx, &r.matrix);
    let k = Vec3::new(r.n.x * matrix::MINUS_TWO, r.n.y * matrix::MINUS_TWO, r.n.z * matrix::MINUS_TWO);
    let nz2 = r.n.z * matrix::TWO * r.n.z;
    cpu.xmm = [k.x * r.d, k.y * r.d, k.z * r.d, F32::ZERO, ONE, r.d, nz2, ONE - nz2].map(scalar);
    cpu.eax = pp;
    ret(cpu, 4);
}

/// A call the original makes to code that is not ported: pushes the
/// return address and goes where the lifted body's call goes
/// (`x86::call_ext`: the runtime's dispatcher, or a recorded replay in
/// tests), with the machine in the state the original has at that call.
fn call_out(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, return_to: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, return_to);
    x86::call_ext(m, cpu, target);
}

/// The CRT's double-precision atan2 (SSE2 entry: y in xmm0, x in xmm1,
/// result in xmm0), not ported (platform code).
const CRT_ATAN2: u32 = 0x012F_1109;

/// `cvtps2pd`/`cvtss2sd` of a single float into a cleared register: the
/// double in lanes 0..1, zero above.
fn double_lanes(v: f64) -> [u32; 4] {
    let b = v.to_bits();
    [b as u32, (b >> 32) as u32, 0, 0]
}

/// Calls the CRT atan2 from inside `push ebp; mov ebp, esp; and esp, -8`
/// and returns its result (xmm0 lanes 0..1).
fn atan2_out(m: &mut dyn Mem, cpu: &mut Cpu, y: f64, x: f64, return_to: u32) -> f64 {
    cpu.xmm[0] = double_lanes(y);
    cpu.xmm[1] = double_lanes(x);
    call_out(m, cpu, CRT_ATAN2, return_to);
    f64::from_bits(cpu.xmm[0][0] as u64 | (cpu.xmm[0][1] as u64) << 32)
}

/// The registers the shared tail of the yaw helpers leaves, from the
/// angle the CRT returned, then `mov esp, ebp; pop ebp; ret`.
fn yaw_tail(cpu: &mut Cpu, angle: f64, entry_esp: u32, ebp: u32) {
    let a = F32(angle as f32);
    let w = angle::wrap_angle(a);
    cpu.xmm[0] = scalar(w.result);
    cpu.xmm[1] = scalar(a);
    cpu.xmm[2] = scalar(angle::TWO_PI);
    cpu.xmm[3] = scalar(F32(w.turns as f32) * angle::TWO_PI);
    cpu.eax = w.turns as u32;
    cpu.esp = entry_esp;
    cpu.ebp = ebp;
    ret(cpu, 0);
}

/// Enters the yaw helpers' frame: `push ebp; mov ebp, esp; and esp, -8`.
fn yaw_frame(m: &mut dyn Mem, cpu: &mut Cpu) -> (u32, u32) {
    let (entry_esp, ebp) = (cpu.esp, cpu.ebp);
    pushed(m, cpu, 1, ebp);
    cpu.ebp = entry_esp.wrapping_sub(4);
    cpu.esp = cpu.ebp & !7;
    (entry_esp, ebp)
}

fn load_xz(m: &dyn Mem, addr: u32) -> Vec3 {
    let f = |k: u32| F32::from_bits(m.u32(addr.wrapping_add(4 * k)));
    Vec3::new(f(0), F32::ZERO, f(2))
}

/// `float yaw_between(const MtVector3 *from /* ecx */, const MtVector3
/// *to /* eax */)`, result in xmm0; reads only x and z. Calls the CRT
/// atan2; leaves the whole turns it removed in EAX.
pub fn f_007ad9d0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (from, to) = (load_xz(m, cpu.ecx), load_xz(m, cpu.eax));
    let (entry_esp, ebp) = yaw_frame(m, cpu);
    let mut angle = 0.0;
    angle::yaw_between(from, to, |y, x| {
        angle = atan2_out(m, cpu, y, x, 0x007A_D9F3);
        angle
    });
    yaw_tail(cpu, angle, entry_esp, ebp);
}

/// `float yaw_of_direction(const MtVector3 *from /* ecx */, const
/// MtVector3 *to /* eax */)`, result in xmm0. Calls the CRT atan2; leaves
/// the whole turns it removed in EAX.
pub fn f_007ad7a0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (from, to) = (load_vec3(m, cpu.ecx), load_vec3(m, cpu.eax));
    let (entry_esp, ebp) = yaw_frame(m, cpu);
    let mut angle = 0.0;
    angle::yaw_of_direction(from, to, |y, x| {
        // before the call: xmm2 holds dx, xmm3 the z the yaw is taken of
        let (_, z) = angle::direction_xz(from, to);
        cpu.xmm[2] = scalar(from.x - to.x);
        cpu.xmm[3] = scalar(z);
        angle = atan2_out(m, cpu, y, x, 0x007A_D818);
        angle
    });
    yaw_tail(cpu, angle, entry_esp, ebp);
}

/// The CRT's float sine and cosine (SSE2 entries: x in xmm0, result in
/// xmm0), not ported (platform code).
const CRT_SINF: u32 = 0x012F_0906;
const CRT_COSF: u32 = 0x012F_0A55;
/// The global vector 0x007AD8E0 rotates (`base`).
const ROTATE_BASE: u32 = 0x018C_FE80;

/// `MtVector4 *rotate_about_y(float distance, float angle)`, output
/// through ESI (also returned in EAX); the caller pops the arguments.
/// Builds a 16-aligned 0x20-byte frame, spills the base vector, the sine
/// and the result's z; calls the CRT sinf and cosf.
pub fn f_007ad8e0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (entry_esp, ebp) = (cpu.esp, cpu.ebp);
    let (distance, angle) = (arg_f32(m, cpu, 0), arg_f32(m, cpu, 1));
    let base = [F32::from_bits(m.u32(ROTATE_BASE)), F32::from_bits(m.u32(ROTATE_BASE + 4))];
    let f = aligned_frame(cpu, 0x20);
    pushed(m, cpu, 1, ebp);
    cpu.ebp = entry_esp.wrapping_sub(4);
    cpu.esp = f;
    spill(m, f + 0x10, base[0]);
    spill(m, f + 0x14, base[1]);
    let out = angle::rotate_about_y(
        base,
        distance,
        angle,
        |a| {
            cpu.xmm[0] = scalar(a);
            call_out(m, cpu, CRT_SINF, 0x007A_D90F);
            let s = F32::from_bits(cpu.xmm[0][0]);
            spill(m, f + 8, s);
            cpu.xmm[0] = scalar(a);
            call_out(m, cpu, CRT_COSF, 0x007A_D91F);
            (s, F32::from_bits(cpu.xmm[0][0]))
        },
    );
    for (k, v) in out.into_iter().enumerate() {
        m.set_u32(cpu.esi.wrapping_add(4 * k as u32), v.to_bits());
    }
    // registers and spills as the original's tail leaves them
    let s = F32::from_bits(m.u32(f + 8));
    let o = F32::ZERO;
    spill(m, f + 8, base[1] * o);
    spill(m, f + 0xC, out[2]);
    let sign = 0x8000_0000;
    cpu.xmm[0] = [out[2].to_bits(), sign, sign, sign];
    cpu.xmm[1] = cpu.xmm[0];
    cpu.xmm[2] = scalar(out[3]);
    cpu.xmm[3] = scalar(base[0] * o);
    cpu.xmm[4] = scalar(distance * o);
    cpu.xmm[5] = scalar(out[1]);
    cpu.xmm[6] = scalar(s * distance);
    cpu.xmm[7] = [0; 4];
    cpu.eax = cpu.esi;
    cpu.esp = entry_esp;
    cpu.ebp = ebp;
    ret(cpu, 0);
}

/// The CRT's float arcsine (x in xmm0, result in xmm0), not ported.
const CRT_ASINF: u32 = 0x012F_0D4C;
const SIGN: u32 = 0x8000_0000;

/// `cvtps2pd` of a value that was loaded with `movss` (upper lanes zero)
/// and, if `negated`, flipped with `xorps` against four sign bits (upper
/// lanes -0.0): the double in lanes 0..1, the widened lane 1 above.
fn widened(v: F32, negated: bool) -> [u32; 4] {
    let mut r = double_lanes(v.0 as f64);
    if negated {
        r[3] = SIGN; // (-0.0f32) as f64
    }
    r
}

/// The return addresses of each conversion's CRT calls: main branch
/// (first atan2, asin, last atan2), low pole, high pole.
const EULER_CALLS: [[u32; 5]; 6] = [
    [0x00CF_CA31, 0x00CF_CA4B, 0x00CF_CA75, 0x00CF_CAA1, 0x00CF_CAC8],
    [0x00CF_CB4A, 0x00CF_CB6B, 0x00CF_CB8B, 0x00CF_CBB7, 0x00CF_CBEC],
    [0x00CF_CC6A, 0x00CF_CC8C, 0x00CF_CCAC, 0x00CF_CCD7, 0x00CF_CD0C],
    [0x00CF_CD90, 0x00CF_CDAB, 0x00CF_CDD6, 0x00CF_CE01, 0x00CF_CE29],
    [0x00CF_CEA9, 0x00CF_CECB, 0x00CF_CEEC, 0x00CF_CF17, 0x00CF_CF4D],
    [0x00CF_CFD1, 0x00CF_CFEC, 0x00CF_D016, 0x00CF_D041, 0x00CF_D068],
];

/// The machine side of a conversion: elements from guest memory, CRT
/// calls through `x86::call_ext` with the registers the original sets up.
struct EulerMachine<'a> {
    m: &'a mut dyn Mem,
    cpu: &'a mut Cpu,
    matrix: u32,
    order: usize,
    branch: Option<euler::EulerBranch>,
    calls: usize,
}

impl EulerMachine<'_> {
    /// The return address of the next CRT call: the branch is known from
    /// the first call (a pole branch makes exactly one).
    fn return_to(&mut self) -> u32 {
        let o = &euler::ORDERS[self.order];
        if self.calls == 0 {
            let p = euler::EulerInput::elem(self, o.pivot);
            self.branch = Some(if !(ONE > p) {
                euler::EulerBranch::High
            } else if !(p > F32(-1.0)) {
                euler::EulerBranch::Low
            } else {
                euler::EulerBranch::Main
            });
        }
        let k = match self.branch {
            Some(euler::EulerBranch::Low) => 3,
            Some(euler::EulerBranch::High) => 4,
            _ => self.calls,
        };
        self.calls += 1;
        EULER_CALLS[self.order][k]
    }
}

impl euler::EulerInput for EulerMachine<'_> {
    fn elem(&mut self, i: usize) -> F32 {
        F32::from_bits(self.m.u32(self.matrix.wrapping_add(4 * i as u32)))
    }
    fn atan2(&mut self, term: &euler::Atan2, y: F32, x: F32) -> f64 {
        let back = self.return_to();
        if term.negate_x {
            // 0x00CFCF2A negates x in xmm2 and widens it from there
            self.cpu.xmm[2] = [x.to_bits(), SIGN, SIGN, SIGN];
        }
        self.cpu.xmm[0] = widened(y, term.negate_y);
        self.cpu.xmm[1] = widened(x, term.negate_x);
        call_out(self.m, self.cpu, CRT_ATAN2, back);
        f64::from_bits(self.cpu.xmm[0][0] as u64 | (self.cpu.xmm[0][1] as u64) << 32)
    }
    fn asin(&mut self, v: F32) -> F32 {
        let back = self.return_to();
        let negated = euler::ORDERS[self.order].negate_pivot;
        self.cpu.xmm[0] = if negated { [v.to_bits(), SIGN, SIGN, SIGN] } else { scalar(v) };
        call_out(self.m, self.cpu, CRT_ASINF, back);
        F32::from_bits(self.cpu.xmm[0][0])
    }
}

/// thiscall `MtVector3 *toEuler(MtVector3 *out)` on a matrix, `ret 4`.
/// Pushes ESI and EDI, overwrites its argument slot with the pivot
/// element, returns `out` in EAX.
fn to_euler(m: &mut dyn Mem, cpu: &mut Cpu, order: usize) {
    let (entry_esp, esi, edi) = (cpu.esp, cpu.esi, cpu.edi);
    let out_ptr = arg(m, cpu, 0);
    let o = euler::ORDERS[order];
    pushed(m, cpu, 1, esi);
    pushed(m, cpu, 2, edi);
    m.set_u32(out_ptr.wrapping_add(0xC), 0);
    let pivot = m.u32(cpu.ecx.wrapping_add(4 * o.pivot as u32));
    m.set_u32(entry_esp.wrapping_add(4), pivot);
    (cpu.esi, cpu.edi, cpu.esp) = (out_ptr, cpu.ecx, entry_esp.wrapping_sub(8));
    cpu.xmm[1] = scalar(ONE);
    let matrix = cpu.ecx;
    let mut machine = EulerMachine { m: &mut *m, cpu: &mut *cpu, matrix, order, branch: None, calls: 0 };
    let (out, branch) = euler::to_euler(&o, &mut machine);
    for (slot, v) in out.iter().take(3).enumerate() {
        m.set_u32(out_ptr.wrapping_add(4 * slot as u32), v.to_bits());
    }
    if branch == euler::EulerBranch::Main {
        // the last angle: cvtsd2ss into the CRT's xmm0, then xorps
        let x = &mut cpu.xmm[0];
        *x = [out[o.last.slot].to_bits(), x[1] ^ SIGN, x[2] ^ SIGN, x[3] ^ SIGN];
    } else {
        cpu.xmm[0] = [0; 4];
    }
    (cpu.eax, cpu.esi, cpu.edi, cpu.esp) = (out_ptr, esi, edi, entry_esp);
    ret(cpu, 4);
}

pub fn f_00cfc9e0(m: &mut dyn Mem, cpu: &mut Cpu) {
    to_euler(m, cpu, 0);
}
pub fn f_00cfcb00(m: &mut dyn Mem, cpu: &mut Cpu) {
    to_euler(m, cpu, 1);
}
pub fn f_00cfcc20(m: &mut dyn Mem, cpu: &mut Cpu) {
    to_euler(m, cpu, 2);
}
pub fn f_00cfcd40(m: &mut dyn Mem, cpu: &mut Cpu) {
    to_euler(m, cpu, 3);
}
pub fn f_00cfce60(m: &mut dyn Mem, cpu: &mut Cpu) {
    to_euler(m, cpu, 4);
}
pub fn f_00cfcf80(m: &mut dyn Mem, cpu: &mut Cpu) {
    to_euler(m, cpu, 5);
}

/// The CRT's float tangent (x in xmm0, result in xmm0), not ported.
const CRT_TANF: u32 = 0x012F_0BAC;

/// The three field-of-view builders, thiscall `(MtMatrix *this, float
/// near, float far, float fovy, float aspect[, float sx, float sy])`.
/// Each stores `q` over its `far` argument, pushes ESI, copies the base
/// matrix (all 16 elements, or 15 for the left-handed one), and calls
/// the CRT tanf.
fn perspective_fov(m: &mut dyn Mem, cpu: &mut Cpu, kind: u32) {
    let (entry_esp, esi, this) = (cpu.esp, cpu.esi, cpu.ecx);
    let (near, far, fovy, aspect) = (arg_f32(m, cpu, 0), arg_f32(m, cpu, 1), arg_f32(m, cpu, 2), arg_f32(m, cpu, 3));
    let (sx, sy) = if kind == 1 { (arg_f32(m, cpu, 4), arg_f32(m, cpu, 5)) } else { (F32::ZERO, F32::ZERO) };
    let q = far / (far - near);
    m.set_u32(entry_esp.wrapping_add(8), q.to_bits());
    let copied = if kind == 2 { 15 } else { 16 };
    let mut base = Mat44::ZERO;
    for i in 0..copied {
        base.m[i / 4][i % 4] = F32::from_bits(m.u32(matrix::BASE_MATRIX + 4 * i as u32));
    }
    pushed(m, cpu, 1, esi);
    (cpu.esi, cpu.esp) = (this, entry_esp.wrapping_sub(4));
    let back = [0x00CF_C44E, 0x00CF_C5AE, 0x00CF_C711][kind as usize];
    let tan = |v: F32| {
        cpu.xmm[0] = scalar(v);
        cpu.xmm[1] = scalar(far - near);
        call_out(m, cpu, CRT_TANF, back);
        F32::from_bits(cpu.xmm[0][0])
    };
    let mat = match kind {
        0 => matrix::perspective_fov_rh(&base, near, far, fovy, aspect, tan),
        1 => matrix::perspective_fov_rh_scaled(&base, near, far, fovy, aspect, sx, sy, tan),
        _ => matrix::perspective_fov_lh(&base, near, far, fovy, aspect, tan),
    };
    store_matrix(m, this, &mat);
    let z = -(q * near);
    if kind == 2 {
        cpu.xmm[0] = [z.to_bits(), 0x8000_0000, 0x8000_0000, 0x8000_0000];
        cpu.xmm[1] = scalar(ONE);
    } else {
        cpu.xmm[0] = scalar(z);
        cpu.xmm[1] = [0x8000_0000, 0, 0, 0];
    }
    cpu.xmm[2] = [0; 4];
    (cpu.esi, cpu.esp) = (esi, entry_esp);
    ret(cpu, if kind == 1 { 0x18 } else { 0x10 });
}

pub fn f_00cfc350(m: &mut dyn Mem, cpu: &mut Cpu) {
    perspective_fov(m, cpu, 0);
}
pub fn f_00cfc4b0(m: &mut dyn Mem, cpu: &mut Cpu) {
    perspective_fov(m, cpu, 1);
}
pub fn f_00cfc620(m: &mut dyn Mem, cpu: &mut Cpu) {
    perspective_fov(m, cpu, 2);
}

/// thiscall `MtMatrix *fromEuler(const MtVector3 *angles, const
/// MtVector3 *translation)`, `ret 8`: a 16-aligned 0x38-byte frame plus
/// pushes of ESI and EDI; spills the angles, three sines and two cosines;
/// six CRT calls; the translation through the x87.
pub fn f_00cfc1d0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (entry_esp, ebp, esi, edi, this) = (cpu.esp, cpu.ebp, cpu.esi, cpu.edi, cpu.ecx);
    let (pa, pt) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let angles = load_vec3(m, pa);
    let f = aligned_frame(cpu, 0x38);
    let local = |off: u32| f.wrapping_sub(8).wrapping_add(off);
    pushed(m, cpu, 1, ebp);
    m.set_u32(f.wrapping_sub(4), esi);
    m.set_u32(f.wrapping_sub(8), edi);
    (cpu.ebp, cpu.esp, cpu.esi, cpu.edi) = (entry_esp.wrapping_sub(4), f.wrapping_sub(8), this, pa);
    for (off, v) in [(0x14, angles.x), (0x18, angles.y), (0x1C, angles.z)] {
        spill(m, local(off), v);
    }
    // return addresses and spill slots of the six calls, in order
    const CALLS: [(u32, u32); 6] = [(0x00CF_C1EF, 0x20), (0x00CF_C205, 0x24), (0x00CF_C21B, 0x28),
                                    (0x00CF_C22C, 0x30), (0x00CF_C23D, 0x34), (0x00CF_C24E, 0)];
    let mut k = 0;
    let mat = matrix::from_euler(angles, Vec3::default(), |which, a| {
        let target = if which == matrix::Trig::Sin { CRT_SINF } else { CRT_COSF };
        cpu.xmm[0] = scalar(a);
        call_out(m, cpu, target, CALLS[k].0);
        let v = F32::from_bits(cpu.xmm[0][0]);
        if CALLS[k].1 != 0 {
            spill(m, local(CALLS[k].1), v);
        }
        k += 1;
        v
    });
    // the rotation part; the translation goes through the x87 as in the
    // original (fld/fstp: exact x87 state, the same bytes as x87_copy)
    for i in 0..12 {
        m.set_u32(this.wrapping_add(4 * i as u32), mat.at(i).to_bits());
    }
    for k in 0..3u32 {
        cpu.st.memory::<0x00d9, 4, false>(m, pt.wrapping_add(4 * k));
        cpu.st.memory::<0x18d9, 4, true>(m, this.wrapping_add(0x30 + 4 * k));
    }
    m.set_u32(this.wrapping_add(0x3C), ONE.to_bits());
    let r = |i: usize| F32::from_bits(m.u32(local(0x20 + 4 * i as u32)));
    let (sx, sz) = (r(0), r(2));
    let cx = F32::from_bits(m.u32(local(0x30)));
    let cy = F32::from_bits(m.u32(local(0x34)));
    let upper = cpu.xmm[0]; // the last cosf's xmm0, whose upper lanes xmm1 keeps
    cpu.xmm = [
        scalar(ONE),
        [mat.at(8).to_bits(), upper[1], upper[2], upper[3]],
        scalar(mat.at(9)),
        scalar(cx * cy),
        scalar(sx),
        scalar(sz),
        scalar(mat.at(5)),
        scalar(sx * sz),
    ];
    (cpu.eax, cpu.esi, cpu.edi, cpu.ebp, cpu.esp) = (pt, esi, edi, ebp, entry_esp);
    ret(cpu, 8);
}

/// `bool unit_normal(a /* eax */, b /* stack */, d /* edx */, e /* esi */,
/// out /* edi */)`, result in AL (the rest of EAX keeps `a`'s pointer);
/// the caller pops `b`. Leaves `b`'s pointer in ECX; spills the
/// fallback's squared y to its frame.
pub fn f_007aeed0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (entry_esp, ebp) = (cpu.esp, cpu.ebp);
    let a = load_vec3(m, cpu.eax);
    cpu.ecx = arg(m, cpu, 0);
    let b = load_vec3(m, cpu.ecx);
    pushed(m, cpu, 1, ebp);
    let (pd, pe) = (cpu.edx, cpu.esi);
    let t = vector::unit_normal(a, b, || (load_vec3(m, pd), load_vec3(m, pe)));
    let eps = matrix::NORMALIZE_EPSILON;
    let main_sq = vector::squared_length_xyz(t.main);
    cpu.xmm[7] = scalar(main_sq);
    cpu.xmm[4] = scalar(eps);
    let chosen = t.fallback.unwrap_or(t.main);
    if let Some(f) = t.fallback {
        let frame = (entry_esp.wrapping_sub(4) & !7).wrapping_sub(8);
        spill(m, frame + 4, f.y * f.y);
    }
    match t.result {
        Some(n) => {
            for (k, v) in [n.x, n.y, n.z, F32::ZERO].into_iter().enumerate() {
                m.set_u32(cpu.edi.wrapping_add(4 * k as u32), v.to_bits());
            }
            cpu.xmm[0] = [0; 4];
            cpu.xmm[1] = scalar(n.y);
            cpu.xmm[2] = scalar(n.x);
            cpu.xmm[3] = scalar(chosen.x);
            cpu.xmm[5] = scalar(n.z);
            cpu.xmm[6] = scalar(vector::squared_length_xyz(chosen));
            cpu.eax = cpu.eax & !0xFF | 1;
        }
        None => {
            let f = chosen;
            cpu.xmm[0] = scalar(vector::squared_length_xyz(f));
            cpu.xmm[1] = scalar(f.y);
            cpu.xmm[2] = scalar(f.z * f.z);
            cpu.xmm[3] = scalar(f.x);
            cpu.xmm[5] = scalar(f.z);
            cpu.xmm[6] = scalar(f.x * f.x);
            cpu.eax &= !0xFF;
        }
    }
    (cpu.ebp, cpu.esp) = (ebp, entry_esp);
    ret(cpu, 0);
}

/// `bool basis_from_forward(z /* edx */, hint /* eax */, e /* esi */,
/// MtMatrix *out, const MtVector3 *t)`, result in AL; the caller pops the
/// two arguments. Calls 0x007AEED0 (the port above) for row 0, then reads
/// and writes in the original's order.
pub fn f_007af5b0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (entry_esp, ebp, ebx, edi) = (cpu.esp, cpu.ebp, cpu.ebx, cpu.edi);
    let (out, pt) = (arg(m, cpu, 0), arg(m, cpu, 1));
    pushed(m, cpu, 1, ebp);
    let frame = entry_esp.wrapping_sub(4) & !7;
    m.set_u32(frame.wrapping_sub(4), ebx);
    m.set_u32(frame.wrapping_sub(8), edi);
    m.set_u32(frame.wrapping_sub(12), cpu.eax); // the hint, 0x007AEED0's stack argument
    (cpu.ebp, cpu.ebx, cpu.edi) = (entry_esp.wrapping_sub(4), pt, out);
    cpu.eax = cpu.edx;
    cpu.esp = frame.wrapping_sub(16);
    m.set_u32(cpu.esp, 0x007A_F5C6);
    f_007aeed0(m, cpu);
    cpu.esp = cpu.esp.wrapping_add(4);
    if cpu.eax & 0xFF != 0 {
        let x = load_vec3(m, out);
        let z = load_vec3(m, cpu.edx);
        let y = vector::basis_y(z, x);
        for (k, v) in [y.x, y.y, y.z, F32::ZERO].into_iter().enumerate() {
            m.set_u32(out.wrapping_add(0x10 + 4 * k as u32), v.to_bits());
        }
        let z2 = vector::basis_z(load_vec3(m, cpu.edx));
        for (k, v) in [z2.x, z2.y, z2.z, F32::ZERO].into_iter().enumerate() {
            m.set_u32(out.wrapping_add(0x20 + 4 * k as u32), v.to_bits());
        }
        let t = load_vec3(m, pt);
        for (k, v) in [t.x, t.y, t.z, ONE].into_iter().enumerate() {
            m.set_u32(out.wrapping_add(0x30 + 4 * k as u32), v.to_bits());
        }
        cpu.xmm = [
            scalar(t.x), scalar(t.y), scalar(t.z), scalar(z2.z),
            [0; 4], scalar(ONE), scalar(matrix::NORMALIZE_EPSILON), scalar(z.x * x.z),
        ];
        cpu.eax = cpu.eax & !0xFF | 1;
    }
    (cpu.ebx, cpu.edi, cpu.ebp, cpu.esp) = (ebx, edi, ebp, entry_esp);
    ret(cpu, 0);
}

/// `bool basis_from_up(hint /* eax */, d /* edx */, MtMatrix *out, const
/// MtVector3 *up, const MtVector3 *t)`, result in AL; the caller pops the
/// three arguments. Calls 0x007AEED0 (the port above) for row 0, then reads
/// and writes in the original's order.
pub fn f_007af450(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (entry_esp, ebp, ebx, esi, edi) = (cpu.esp, cpu.ebp, cpu.ebx, cpu.esi, cpu.edi);
    let (out, pu, pt) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    pushed(m, cpu, 1, ebp);
    let frame = entry_esp.wrapping_sub(4) & !7;
    for (k, v) in [cpu.ecx, ebx, esi, edi, pu].into_iter().enumerate() {
        m.set_u32(frame.wrapping_sub(4 * (k as u32 + 1)), v); // the last is 0x007AEED0's argument
    }
    (cpu.ebp, cpu.ebx, cpu.esi, cpu.edi) = (entry_esp.wrapping_sub(4), pt, pu, out);
    cpu.esp = frame.wrapping_sub(24);
    m.set_u32(cpu.esp, 0x007A_F469);
    f_007aeed0(m, cpu);
    cpu.esp = cpu.esp.wrapping_add(4);
    if cpu.eax & 0xFF != 0 {
        let x = load_vec3(m, out);
        let side = vector::basis_side(x, load_vec3(m, pu));
        for (k, v) in [side.x, side.y, side.z, F32::ZERO].into_iter().enumerate() {
            m.set_u32(out.wrapping_add(0x20 + 4 * k as u32), v.to_bits());
        }
        let up = load_vec3(m, pu);
        let row1 = vector::basis_up(up);
        for (k, v) in [row1.x, row1.y, row1.z, F32::ZERO].into_iter().enumerate() {
            m.set_u32(out.wrapping_add(0x10 + 4 * k as u32), v.to_bits());
        }
        let t = load_vec3(m, pt);
        for (k, v) in [t.x, t.y, t.z, ONE].into_iter().enumerate() {
            m.set_u32(out.wrapping_add(0x30 + 4 * k as u32), v.to_bits());
        }
        let x7 = cpu.xmm[7]; // as 0x007AEED0 left it
        cpu.xmm = [
            scalar(t.x), scalar(t.y), scalar(t.z), [0; 4],
            scalar(up.x * up.x), scalar(ONE), scalar(matrix::NORMALIZE_EPSILON), x7,
        ];
        cpu.eax = cpu.eax & !0xFF | 1;
    }
    (cpu.ebx, cpu.esi, cpu.edi, cpu.ebp, cpu.esp) = (ebx, esi, edi, ebp, entry_esp);
    ret(cpu, 0);
}

/// The CRT's float arccosine (x in xmm0, result in xmm0), not ported.
const CRT_ACOSF: u32 = 0x012F_161F;

/// `float angle_between(const MtVector3 *a /* eax */, const MtVector3 *b
/// /* ecx */)`: the CRT acosf's result in xmm0 (a tail of
/// `call acosf; mov esp, ebp; pop ebp; ret`).
pub fn f_007adc00(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (a, b) = (load_vec3(m, cpu.eax), load_vec3(m, cpu.ecx));
    let (entry_esp, ebp) = (cpu.esp, cpu.ebp);
    pushed(m, cpu, 1, ebp);
    cpu.ebp = entry_esp.wrapping_sub(4);
    cpu.esp = cpu.ebp & !7;
    angle::angle_between(a, b, |c| {
        // the registers at the call, as the original leaves its scratch
        let sq = |v: Vec3| v.y * v.y + v.x * v.x + v.z * v.z;
        let raw = (a.y * b.y + a.x * b.x + b.z * a.z) / (sq(b).sqrt() * sq(a).sqrt());
        cpu.xmm = [
            scalar(c),
            scalar(if raw > ONE { ONE } else { F32(-1.0) }),
            scalar(sq(a).sqrt()),
            scalar(sq(a)),
            scalar(a.z * a.z),
            scalar(sq(b)),
            scalar(b.z * b.z),
            scalar(b.z * a.z),
        ];
        call_out(m, cpu, CRT_ACOSF, 0x007A_DC99);
        F32::from_bits(cpu.xmm[0][0])
    });
    (cpu.ebp, cpu.esp) = (ebp, entry_esp);
    ret(cpu, 0);
}

/// `bool in_view_cone(eye /* edi */, target /* esi */, float yaw, float
/// half_angle)`, result in AL over the EAX the last CRT call left; the
/// caller pops the arguments. Spills the sine and then the dot product.
pub fn f_007adca0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (entry_esp, ebp) = (cpu.esp, cpu.ebp);
    let (yaw, half) = (arg_f32(m, cpu, 0), arg_f32(m, cpu, 1));
    pushed(m, cpu, 1, ebp);
    cpu.ebp = entry_esp.wrapping_sub(4);
    let slot = (cpu.ebp & !7).wrapping_sub(8).wrapping_add(4);
    cpu.esp = slot.wrapping_sub(4);
    let (pe, pt) = (cpu.edi, cpu.esi);
    let calls = [0x007A_DCB3, 0x007A_DCC3, 0x007A_DD40];
    let mut k = 0;
    let mut state: Option<(F32, F32)> = None; // (sin, cos) of yaw
    let inside = angle::in_view_cone(load_xz(m, pe), load_xz(m, pt), yaw, half, |which, v| {
        if k == 2 {
            // before the third call: the dot product is spilled and the
            // scratch registers hold the terms of the dot product
            let (s, c) = state.unwrap();
            let (eye, target) = (load_xz(m, pe), load_xz(m, pt));
            let (dx, dz) = (target.x - eye.x, target.z - eye.z);
            let inv = ONE / (dz * dz + dx * dx).sqrt();
            let dot = angle::view_cone_dot(eye, target, s, c).unwrap();
            spill(m, slot, dot);
            cpu.xmm[1] = scalar(dot);
            cpu.xmm[2] = scalar(inv);
            cpu.xmm[3] = scalar(inv * F32::ZERO * F32::ZERO);
            cpu.xmm[4] = scalar(dz * inv * c);
        }
        cpu.xmm[0] = scalar(v);
        let target = if which == matrix::Trig::Sin { CRT_SINF } else { CRT_COSF };
        call_out(m, cpu, target, calls[k]);
        let r = F32::from_bits(cpu.xmm[0][0]);
        match k {
            0 => {
                spill(m, slot, r);
                state = Some((r, F32::ZERO));
            }
            1 => state = Some((state.unwrap().0, r)),
            _ => {}
        }
        k += 1;
        r
    });
    if k == 2 {
        // the distance test failed after cos(yaw)
        let (eye, target) = (load_xz(m, pe), load_xz(m, pt));
        let (dx, dz) = (target.x - eye.x, target.z - eye.z);
        cpu.xmm[1] = scalar(dz * dz + dx * dx);
        cpu.xmm[2] = scalar(dx * dx);
        cpu.xmm[3] = scalar(dx);
        cpu.xmm[4] = scalar(dz);
    } else {
        cpu.xmm[1] = scalar(F32::from_bits(m.u32(slot)));
    }
    cpu.eax = cpu.eax & !0xFF | inside as u32;
    (cpu.ebp, cpu.esp) = (ebp, entry_esp);
    ret(cpu, 0);
}

/// Every port above with its original address and the xmm registers it
/// leaves exactly as the original does (bit k = xmmk; the others keep their
/// entry values, see the module comment).
pub const PORTS: &[(u32, ddnative_base::x86::LiftFn, u8)] = &[
    (0x007A_D7A0, f_007ad7a0, 0b1111_1111),
    (0x007A_D870, f_007ad870, 0b0001_1111),
    (0x007A_D8E0, f_007ad8e0, 0b1111_1111),
    (0x007A_D9D0, f_007ad9d0, 0b1111_1111),
    (0x007A_DC00, f_007adc00, 0b1111_1111),
    (0x007A_DCA0, f_007adca0, 0b1111_1111),
    (0x007A_EED0, f_007aeed0, 0b1111_1111),
    (0x007A_F450, f_007af450, 0b1111_1111),
    (0x007A_F5B0, f_007af5b0, 0b1111_1111),
    (0x00CF_C1D0, f_00cfc1d0, 0b1111_1111),
    (0x00CF_C350, f_00cfc350, 0b1111_1111),
    (0x00CF_C4B0, f_00cfc4b0, 0b1111_1111),
    (0x00CF_C620, f_00cfc620, 0b1111_1111),
    (0x00CF_C770, f_00cfc770, 0b0000_0001),
    (0x00CF_C810, f_00cfc810, 0b0000_0001),
    (0x00CF_C8B0, f_00cfc8b0, 0b0000_0001),
    (0x00CF_C940, f_00cfc940, 0b0000_0001),
    (0x00CF_C9E0, f_00cfc9e0, 0b1111_1111),
    (0x00CF_CB00, f_00cfcb00, 0b1111_1111),
    (0x00CF_CC20, f_00cfcc20, 0b1111_1111),
    (0x00CF_CD40, f_00cfcd40, 0b1111_1111),
    (0x00CF_CE60, f_00cfce60, 0b1111_1111),
    (0x00CF_CF80, f_00cfcf80, 0b1111_1111),
    (0x00CF_D5E0, f_00cfd5e0, 0b1111_1111),
    (0x00CF_D890, f_00cfd890, 0b1111_1111),
    (0x00CF_DB10, f_00cfdb10, 0b1111_1111),
    (0x00D0_0690, f_00d00690, 0b1111_1111),
    (0x00D0_06C0, f_00d006c0, 0b1111_1111),
];
