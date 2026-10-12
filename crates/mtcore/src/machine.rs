//! Helpers the ABI adapters share: arguments, pushes and calls on the
//! guest machine, done exactly as the original's instructions do them.

use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};

/// The 32-bit stack argument `k` (0 = the first, at `[esp + 4]`).
pub fn arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u32 {
    m.u32(cpu.esp.wrapping_add(4 + 4 * k))
}

/// `push value`.
pub fn push(m: &mut dyn Mem, cpu: &mut Cpu, value: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, value);
}

/// `pop`: the value at `[esp]`.
pub fn pop(m: &dyn Mem, cpu: &mut Cpu) -> u32 {
    let v = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
    v
}

/// `ret pop`: drops the return address and `pop` bytes of arguments.
pub fn ret(cpu: &mut Cpu, pop: u32) {
    cpu.esp = cpu.esp.wrapping_add(4 + pop);
}

/// A call to code that is not ported (an indirect call, an import, a
/// virtual function): pushes the return address and goes where the lifted
/// body's call goes, `x86::call_ext` (the runtime's dispatcher, or a
/// recorded replay in tests).
pub fn call_out(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, return_to: u32) {
    push(m, cpu, return_to);
    x86::call_ext(m, cpu, target);
}

/// A direct call to another port (or to a platform model): pushes the
/// return address and runs `f` on the machine, as the lifted body's
/// direct call does.
pub fn call(m: &mut dyn Mem, cpu: &mut Cpu, f: x86::LiftFn, return_to: u32) {
    push(m, cpu, return_to);
    f(m, cpu);
}
