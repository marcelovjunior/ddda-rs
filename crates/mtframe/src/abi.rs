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

use crate::effect::{self, Effects};
use crate::lunge::{self, Step};
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu, LiftFn};
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

/// The ports the lifted program calls instead of the lifted bodies, with
/// the mask of xmm registers each reproduces exactly (all of them).
pub const PORTS: &[(u32, LiftFn, u8)] = &[
    (0x00A3_6DF0, f_00a36df0, 0b1111_1111),
    (0x00E6_C260, f_00e6c260, 0b1111_1111),
    (0x00FB_64D0, f_00fb64d0, 0b1111_1111),
    (0x011E_48E0, f_011e48e0, 0b1111_1111),
];
