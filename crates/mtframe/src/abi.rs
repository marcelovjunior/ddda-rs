//! The ports behind the lifted calling convention (docs/FRAME_RATE.md,
//! "Readable ports"): each `f_X` takes the place of the lifted body of the
//! function at X. `research/native/lift.py gen` imports every adapter
//! listed in [`PORTS`] over its lifted body.
//!
//! The contract is the one of `mtmath::abi` and `mtcore::abi`: every
//! general register, every xmm register (all lanes; the callers are
//! virtual or not traced, so nothing is left out), the x87 stack and
//! every byte written (stack pushes and spills included) end as the
//! original leaves them, and every call is made with the machine as the
//! original has it. Calls go where the lifted body's calls go:
//! `x86::call_ext` for indirect calls, imports and functions that call
//! out; `x86::call_lifted` for other lifted bodies (which live in the
//! shards this crate cannot depend on).

use crate::chain::{self, Products};
use crate::effect::{self, Effects};
use crate::lunge::{self, Step};
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu, LiftFn};
use ddnative_base::x87::X87;
use mtmath::scalar::F32;
use mtcore::machine::{arg, call_out, pop, push, ret};
use std::cell::RefCell;

/// A direct call to another lifted body: pushes the return address and
/// runs the body through the resolver, as the lifted caller's cross-shard
/// call does.
fn call_body(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, return_to: u32) {
    push(m, cpu, return_to);
    x86::call_lifted(m, cpu, target);
}

/// `push ebp; mov ebp, esp; and esp, -align; sub esp, locals`.
fn enter_aligned(m: &mut dyn Mem, cpu: &mut Cpu, align: u32, locals: u32) {
    let ebp = cpu.ebp;
    push(m, cpu, ebp);
    cpu.ebp = cpu.esp;
    cpu.esp = (cpu.esp & !(align - 1)).wrapping_sub(locals);
}

/// `mov esp, ebp; pop ebp; ret pop`.
fn leave(m: &mut dyn Mem, cpu: &mut Cpu, pop_bytes: u32) {
    cpu.esp = cpu.ebp;
    cpu.ebp = pop(m, cpu);
    ret(cpu, pop_bytes);
}

/// sEffect::vf06 (`this` in ecx).
pub fn f_00fb64d0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = cpu.ecx;
    enter_aligned(m, cpu, 16, 0x1C);
    let esi = cpu.esi;
    push(m, cpu, esi);
    cpu.esi = this;
    let effects = Effects(this);
    // the original loads sMain into eax once it finds effects running
    if effects.running(m) {
        cpu.eax = m.u32(crate::smain::INSTANCE);
    }
    let cell = RefCell::new(&mut *cpu);
    effect::update(m, effects, |m, step| {
        let mut g = cell.borrow_mut();
        let cpu: &mut Cpu = &mut g;
        push(m, cpu, this); // push ecx: room for the float argument
        m.set_u32(cpu.esp, step);
        cpu.xmm[0] = [step, 0, 0, 0];
        let time = cpu.esp.wrapping_add(0x14);
        if effects.own_step(m) {
            cpu.eax = time;
        } else {
            cpu.ecx = time;
        }
        push(m, cpu, time);
        cpu.ecx = this.wrapping_add(effect::TIME);
        call_out(m, cpu, 0x011D_4F30, 0x00FB_6531);
        let eax = cpu.eax;
        push(m, cpu, eax);
        cpu.ecx = this;
        call_body(m, cpu, 0x00FB_5E90, 0x00FB_6539);
    });
    let cpu = cell.into_inner();
    cpu.esi = pop(m, cpu);
    leave(m, cpu, 0);
}

/// The element time input (`this` = the element in ecx; the parameter on
/// the stack).
pub fn f_011e48e0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (element, param) = (cpu.ecx, arg(m, cpu, 0));
    let source = effect::time_source(m, param);
    cpu.eax = effect::element_time(m, element, param);
    match source {
        2 | 3 => cpu.edx = m.u32(element),
        effect::SOURCE_FRAME_COUNTER => cpu.ecx = m.u32(effect::INSTANCE),
        _ => {}
    }
    ret(cpu, 4);
}

/// The emitter time input (`this` = the unit in ecx; the parameter and
/// the emitter on the stack).
pub fn f_00e6c260(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (unit, param) = (cpu.ecx, arg(m, cpu, 0));
    let source = effect::time_source(m, param);
    // the original reads the emitter argument only when it uses it
    let emitter = match source {
        2..=4 => 0,
        _ => arg(m, cpu, 1),
    };
    cpu.eax = effect::emitter_time(m, unit, param, emitter);
    match source {
        2 | 3 => {}
        effect::SOURCE_FRAME_COUNTER => cpu.edx = m.u32(effect::INSTANCE),
        _ => cpu.ecx = emitter,
    }
    ret(cpu, 8);
}

/// The lunge's calls, made on the machine as the original makes them.
struct LungeMachine<'a> {
    cpu: &'a mut Cpu,
    action: u32,
}

impl lunge::Calls for LungeMachine<'_> {
    fn ramp(&mut self, m: &mut dyn Mem, enemy: u32) {
        let cpu = &mut *self.cpu;
        let ramp_time = m.u32(enemy.wrapping_add(lunge::RAMP_TIME));
        cpu.xmm[0] = [ramp_time, 0, 0, 0];
        cpu.xmm[3] = [m.u32(enemy.wrapping_add(lunge::TOP_SPEED)), 0, 0, 0];
        // eax = the enemy, xmm3 = the top speed; push ecx (the motion)
        // makes room for the ramp time
        let ecx = cpu.ecx;
        push(m, cpu, ecx);
        m.set_u32(cpu.esp, ramp_time);
        call_body(m, cpu, 0x00A0_0BF0, 0x00A3_6E56);
    }
    fn facing(&mut self, m: &mut dyn Mem, owner: u32) -> u32 {
        let cpu = &mut *self.cpu;
        cpu.ecx = owner;
        cpu.eax = m.u32(owner);
        cpu.edx = m.u32(cpu.eax.wrapping_add(0x58));
        push(m, cpu, 0xFFFF_FFFF);
        let target = cpu.edx;
        call_out(m, cpu, target, 0x00A3_6E62);
        cpu.eax
    }
    fn lunged(&mut self, step: &Step) {
        let vz = step.velocity[2];
        let [x, y, z] = step.position.map(|c| c.to_bits());
        let cpu = &mut *self.cpu;
        cpu.xmm[0] = [x, 0, 0, 0];
        cpu.xmm[1] = [y, 0, 0, 0];
        cpu.xmm[2] = [vz.to_bits(), 0, 0, 0];
        cpu.xmm[3] = [z, 0, 0, 0];
        cpu.xmm[4] = [0; 4];
    }
    fn motion_ended(&mut self, m: &mut dyn Mem, motion: u32) -> bool {
        let cpu = &mut *self.cpu;
        cpu.eax = m.u32(self.action.wrapping_add(lunge::ENEMY));
        cpu.ecx = motion;
        call_body(m, cpu, 0x0105_70F0, 0x00A3_6F11);
        cpu.eax & 0xFF != 0
    }
}

/// cEm5501ActAtckRushBite::vf06 (`this` in ecx).
pub fn f_00a36df0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let action = cpu.ecx;
    enter_aligned(m, cpu, 8, 0);
    let (ecx, esi) = (cpu.ecx, cpu.esi);
    push(m, cpu, ecx);
    push(m, cpu, esi);
    cpu.esi = action;
    let enemy = m.u32(action.wrapping_add(lunge::ENEMY));
    cpu.eax = enemy;
    if enemy != 0 {
        // the window test leaves its operands in ecx, xmm0 and xmm1
        cpu.ecx = enemy.wrapping_add(lunge::MOTION);
        if cpu.ecx != 0 {
            let frame = m.u32(cpu.ecx.wrapping_add(lunge::MOTION_FRAME));
            cpu.xmm[0] = [frame, 0, 0, 0];
            if f32::from_bits(frame) > lunge::WINDOW.0 {
                cpu.xmm[1] = [lunge::WINDOW.1.to_bits(), 0, 0, 0];
            }
        }
    }
    lunge::update(m, action, &mut LungeMachine { cpu: &mut *cpu, action });
    if enemy != 0 {
        let enemy = m.u32(action.wrapping_add(lunge::ENEMY));
        cpu.eax = 0x101;
        cpu.ecx = m.u32(enemy.wrapping_add(lunge::FLAGS[0] + 4));
        cpu.edx = m.u32(enemy.wrapping_add(lunge::FLAGS[1] + 4));
    }
    cpu.esi = pop(m, cpu);
    leave(m, cpu, 0);
}

/// The chain's calls, made on the machine as the original makes them.
struct ChainMachine<'a> {
    cpu: &'a mut Cpu,
    chain: u32,
    /// esp after the prologue (the spills are above it)
    frame: u32,
}

impl ChainMachine<'_> {
    fn spill(&self, m: &mut dyn Mem, offset: u32, v: F32) {
        m.set_u32(self.frame.wrapping_add(offset), v.to_bits());
    }
    fn call_virtual(&mut self, m: &mut dyn Mem, slot: u32, return_to: u32) {
        let cpu = &mut *self.cpu;
        cpu.edx = m.u32(self.chain);
        cpu.eax = m.u32(cpu.edx.wrapping_add(slot));
        cpu.ecx = self.chain;
        let target = cpu.eax;
        call_out(m, cpu, target, return_to);
    }
}

fn lane0(v: F32) -> [u32; 4] {
    [v.to_bits(), 0, 0, 0]
}

impl chain::Calls for ChainMachine<'_> {
    fn ready(&mut self, m: &mut dyn Mem) -> bool {
        call_out(m, self.cpu, 0x00F0_BDC0, 0x00F0_C17B);
        self.cpu.eax & 0xFF != 0
    }
    fn animate(&mut self, m: &mut dyn Mem) {
        self.cpu.ecx = self.chain;
        call_body(m, self.cpu, 0x0112_59E0, 0x00F0_C18A);
    }
    fn oriented(&mut self, m: &mut dyn Mem, q: [F32; 4], p: &Products, rows: &[[F32; 3]; 3]) {
        // the original spills z and four products, and leaves these in
        // the registers
        self.spill(m, 0x28, q[2]);
        self.spill(m, 0x1C, p.wx);
        self.spill(m, 0x14, p.wy);
        self.spill(m, 0x10, p.wz);
        self.spill(m, 0x18, p.xx);
        let cpu = &mut *self.cpu;
        cpu.eax = m.u32(self.chain.wrapping_add(chain::JOINT));
        cpu.xmm[0] = [0; 4];
        cpu.xmm[1] = lane0(F32::ONE);
        cpu.xmm[2] = lane0(rows[2][2]);
        cpu.xmm[3] = lane0(rows[1][0]);
        cpu.xmm[4] = lane0(rows[2][0]);
        cpu.xmm[5] = lane0(p.yy + p.xx);
        cpu.xmm[6] = lane0(rows[2][1]);
        cpu.xmm[7] = lane0(rows[1][2]);
    }
    fn settle(&mut self, m: &mut dyn Mem, steps: u32) {
        let cpu = &mut *self.cpu;
        cpu.eax = steps;
        push(m, cpu, steps);
        cpu.ecx = self.chain;
        call_out(m, cpu, 0x00F0_BEB0, 0x00F0_C392);
    }
    fn fpu(&mut self) -> &mut X87 {
        &mut self.cpu.st
    }
    fn stepped(&mut self, m: &mut dyn Mem, step: F32, time: F32, count: u32) {
        self.spill(m, 0x1C, time);
        let cpu = &mut *self.cpu;
        cpu.eax = count;
        cpu.xmm[0] = lane0(time);
        cpu.xmm[1] = lane0(step);
    }
    fn prepare(&mut self, m: &mut dyn Mem) {
        self.cpu.ecx = self.chain;
        call_out(m, self.cpu, 0x00F0_7270, 0x00F0_C3EC);
    }
    fn virtual_5c(&mut self, m: &mut dyn Mem) {
        self.call_virtual(m, 0x5C, 0x00F0_C3F5);
    }
    fn virtual_58(&mut self, m: &mut dyn Mem) {
        self.call_virtual(m, 0x58, 0x00F0_C3FE);
    }
    fn solve(&mut self, m: &mut dyn Mem) {
        self.cpu.ecx = self.chain;
        call_body(m, self.cpu, 0x00F0_1390, 0x00F0_C405);
    }
    fn followed(&mut self, joint: u32, position: [F32; 3]) {
        let cpu = &mut *self.cpu;
        cpu.eax = joint;
        cpu.xmm[0] = [0; 4];
        cpu.xmm[1] = lane0(position[1]);
        cpu.xmm[2] = lane0(position[2]);
    }
}

/// uChain::vf09 (`this` in ecx).
pub fn f_00f0c160(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = cpu.ecx;
    enter_aligned(m, cpu, 16, 0x2C);
    let esi = cpu.esi;
    push(m, cpu, esi);
    cpu.esi = this;
    let frame = cpu.esp;
    chain::update(m, this, &mut ChainMachine { cpu: &mut *cpu, chain: this, frame });
    cpu.esi = pop(m, cpu);
    leave(m, cpu, 0);
}

/// The ports the lifted program calls instead of the lifted bodies, with
/// the mask of xmm registers each reproduces exactly (all of them).
pub const PORTS: &[(u32, LiftFn, u8)] = &[
    (0x00A3_6DF0, f_00a36df0, 0b1111_1111),
    (0x00E6_C260, f_00e6c260, 0b1111_1111),
    (0x00F0_C160, f_00f0c160, 0b1111_1111),
    (0x00FB_64D0, f_00fb64d0, 0b1111_1111),
    (0x011E_48E0, f_011e48e0, 0b1111_1111),
];
