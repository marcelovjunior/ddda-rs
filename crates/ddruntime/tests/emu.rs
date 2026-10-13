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

fn nested_emulation(m: &mut dyn Mem, cpu: &mut Cpu) {
    Registry::new().call(m, cpu, CODE + 0x100).expect("nested emulation");
}

#[test]
fn nested_fallback_keeps_the_outer_engine_state() {
    std::env::set_var("DDRUNTIME_EMU_FALLBACK", "1");
    let mut m = GuestMemory::new();
    m.map(CODE, 0x3000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // mov eax,1; call SERVICE; add eax,3; ret. The service reenters
    // fallback while the outer engine is suspended at the call boundary.
    let mut code = vec![0xB8, 1, 0, 0, 0, 0xE8];
    code.extend((SERVICE - (CODE + 10)).to_le_bytes());
    code.extend([0x83, 0xC0, 3, 0xC3]);
    for (i, b) in code.into_iter().enumerate() { m.set_u8(CODE + i as u32, b); }
    for (i, b) in [0x83, 0xC0, 2, 0xC3].into_iter().enumerate() {
        m.set_u8(CODE + 0x100 + i as u32, b);
    }
    let mut registry = Registry::new();
    registry.insert(SERVICE, Binding::Service(nested_emulation)).unwrap();
    for _ in 0..32 {
        let mut cpu = Cpu { esp: STACK - 0x100, ..Cpu::default() };
        m.set_u32(cpu.esp, RETURN);
        registry.call(&mut m, &mut cpu, CODE).expect("outer emulation");
        assert_eq!(cpu.eax, 6);
        assert_eq!(cpu.esp, STACK - 0xFC);
    }
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
    assert!(ddruntime::emu::stats().iter().any(|&(a, n)| a == CODE && n >= 1)); // tests share the counts
}

#[test]
fn repeated_calls_reuse_one_emulator() {
    // a call per frame must not allocate an emulator per call (the first
    // boot run ran out of translation buffers after a few minutes)
    std::env::set_var("DDRUNTIME_EMU_FALLBACK", "1");
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    for (k, b) in [0x8B, 0x44, 0x24, 0x04, 0x40, 0xC3].iter().enumerate() {
        m.set_u8(CODE + k as u32, *b); // mov eax, [esp+4]; inc eax; ret
    }
    let registry = Registry::new();
    for n in 0..20_000u32 {
        let mut cpu = Cpu { esp: STACK - 0x100, ..Cpu::default() };
        cpu.esp -= 4;
        m.set_u32(cpu.esp, n);
        cpu.esp -= 4;
        m.set_u32(cpu.esp, RETURN);
        registry.call(&mut m, &mut cpu, CODE).expect("emulated");
        assert_eq!(cpu.eax, n + 1);
    }
}

#[test]
fn a_hot_emulated_loop_runs_fast() {
    // 3 million iterations of a 2-instruction loop: the per-block hook must
    // stay cheap (L56: the audio decoder's emulated loops starved the game)
    std::env::set_var("DDRUNTIME_EMU_FALLBACK", "1");
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // mov ecx, 3000000; l: dec ecx; jnz l; mov eax, 7; ret
    let code = [0xB9, 0xC0, 0xC6, 0x2D, 0x00, 0x49, 0x75, 0xFD, 0xB8, 0x07, 0x00, 0x00, 0x00, 0xC3];
    for (k, b) in code.iter().enumerate() {
        m.set_u8(CODE + k as u32, *b);
    }
    let registry = Registry::new();
    let mut cpu = Cpu { esp: STACK - 0x100, ..Cpu::default() };
    cpu.esp -= 4;
    m.set_u32(cpu.esp, RETURN);
    let t0 = std::time::Instant::now();
    registry.call(&mut m, &mut cpu, CODE).expect("emulated");
    let took = t0.elapsed();
    assert_eq!((cpu.eax, cpu.ecx), (7, 0));
    eprintln!("3M-iteration loop: {took:?}");
}

#[test]
fn many_short_emulated_calls_are_cheap() {
    // a decoder calls short untranslated functions per packet: the cost of
    // entering the emulator must stay small (L56)
    std::env::set_var("DDRUNTIME_EMU_FALLBACK", "1");
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    for (k, b) in [0x8B, 0x44, 0x24, 0x04, 0x40, 0xC3].iter().enumerate() {
        m.set_u8(CODE + k as u32, *b); // mov eax, [esp+4]; inc eax; ret
    }
    let registry = Registry::new();
    let t0 = std::time::Instant::now();
    for n in 0..100_000u32 {
        let mut cpu = Cpu { esp: STACK - 0x100, ..Cpu::default() };
        cpu.esp -= 4;
        m.set_u32(cpu.esp, n);
        cpu.esp -= 4;
        m.set_u32(cpu.esp, RETURN);
        registry.call(&mut m, &mut cpu, CODE).expect("emulated");
        assert_eq!(cpu.eax, n + 1);
    }
    eprintln!("100k short calls: {:?}", t0.elapsed());
}

#[test]
fn x87_and_sse_state_cross_the_emulator() {
    // fld1; fldpi; faddp st1 (st0 = 1 + pi); addps xmm0, xmm0; ret
    std::env::set_var("DDRUNTIME_EMU_FALLBACK", "1");
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    for (k, b) in [0xD9, 0xE8, 0xD9, 0xEB, 0xDE, 0xC1, 0x0F, 0x58, 0xC0, 0xC3].iter().enumerate() {
        m.set_u8(CODE + k as u32, *b);
    }
    let registry = Registry::new();
    let mut cpu = Cpu { esp: STACK - 0x100, ..Cpu::default() };
    cpu.xmm[0] = [1.5f32.to_bits(), 2.0f32.to_bits(), (-3.0f32).to_bits(), 0];
    cpu.xmm[3] = [7, 8, 9, 10];
    cpu.esp -= 4;
    m.set_u32(cpu.esp, RETURN);
    registry.call(&mut m, &mut cpu, CODE).expect("emulated");
    assert_eq!(cpu.xmm[0], [3.0f32.to_bits(), 4.0f32.to_bits(), (-6.0f32).to_bits(), 0], "addps");
    assert_eq!(cpu.xmm[3], [7, 8, 9, 10], "untouched XMM survives");
    assert_eq!(cpu.st.depth(), 1, "one value left on the x87 stack");
    let top = f32::from_bits(cpu.st.top_f32().expect("st0"));
    assert!((top - (1.0 + std::f32::consts::PI)).abs() < 1e-6, "st0 = 1 + pi, got {top}");
}
