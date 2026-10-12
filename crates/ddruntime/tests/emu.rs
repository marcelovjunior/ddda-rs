//! The diagnostic emulator fallback (L51): untranslated code in .text runs in
//! Unicorn on the guest's own pages, with call-outs to bound functions.
#![cfg(feature = "emu")]

use ddnative_base::mem::{Mem, READ, WRITE};
use ddnative_base::x86::Cpu;
use ddruntime::dispatch::{Binding, Registry};
use ddruntime::memory::GuestMemory;

const CODE: u32 = 0x0040_1000;
const SERVICE: u32 = 0x0040_2000;
const STACK: u32 = 0x0080_0000;
const TEB: u32 = 0x0090_0000;
const RETURN: u32 = 0x0040_3000;

/// cdecl f(x) = 2x: a bound callee (the ABI of a lifted function: the
/// return address at [esp], ends with the original's `ret`).
fn double(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = m.u32(cpu.esp + 4).wrapping_mul(2);
    cpu.esp += 4;
}

#[test]
fn untranslated_code_runs_in_the_emulator_with_call_outs() {
    std::env::set_var("DDRUNTIME_EMU_FALLBACK", "1");
    let mut m = GuestMemory::new();
    m.map(CODE, 0x3000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.map(TEB, 0x1000, READ | WRITE).unwrap();
    m.set_u32(TEB + 0x18, TEB); // the TEB's self pointer, read through FS
    let call = (SERVICE as i32 - (CODE as i32 + 12)) as u32; // rel32 from the next instruction
    let mut code = vec![
        0x55, // push ebp
        0x8B, 0xEC, // mov ebp, esp
        0x8B, 0x45, 0x08, // mov eax, [ebp+8]
        0x50, // push eax
        0xE8, // call SERVICE
    ];
    code.extend(call.to_le_bytes());
    code.extend([
        0x83, 0xC4, 0x04, // add esp, 4
        0x40, // inc eax
        0x64, 0x8B, 0x0D, 0x18, 0x00, 0x00, 0x00, // mov ecx, fs:[0x18]
        0x5D, // pop ebp
        0xC2, 0x04, 0x00, // ret 4
    ]);
    for (k, b) in code.iter().enumerate() {
        m.set_u8(CODE + k as u32, *b);
    }
    let mut registry = Registry::new();
    registry.insert(SERVICE, Binding::Service(double)).unwrap();
    let mut cpu = Cpu::default();
    cpu.fs_base = TEB;
    cpu.esp = STACK - 0x100;
    cpu.ebp = 0x1234_5678;
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 21); // the argument
    cpu.esp -= 4;
    m.set_u32(cpu.esp, RETURN); // the caller's return address
    let entry_esp = cpu.esp;
    registry.call(&mut m, &mut cpu, CODE).expect("emulated");
    assert_eq!(cpu.eax, 43, "2 * 21 + 1, through the native call-out");
    assert_eq!(cpu.ecx, TEB, "FS reaches the guest TEB");
    assert_eq!(cpu.ebp, 0x1234_5678, "callee-saved EBP restored");
    assert_eq!(cpu.esp, entry_esp + 8, "ret 4 popped the return address and the argument");
    assert!(ddruntime::emu::stats().iter().any(|&(a, n)| a == CODE && n == 1));
}
