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
    assert_eq!(
        cpu.esp,
        entry_esp + 8,
        "ret 4 popped the return address and the argument"
    );
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
        let mut cpu = Cpu {
            esp: STACK - 0x100,
            ..Cpu::default()
        };
        cpu.esp -= 4;
        m.set_u32(cpu.esp, n);
        cpu.esp -= 4;
        m.set_u32(cpu.esp, RETURN);
        registry.call(&mut m, &mut cpu, CODE).expect("emulated");
        assert_eq!(cpu.eax, n + 1);
    }
}

fn check_and_change_state(_: &mut dyn Mem, cpu: &mut Cpu) {
    use ddnative_base::x86;
    assert_eq!(cpu.fl.snapshot().0 & 0x8d5, 0x845);
    assert!(cpu.df);
    assert_eq!(cpu.st.depth(), 2);
    assert_eq!(cpu.st.top_extended(), Some((0x8000_0000_0000_0001, 0x3fff)));
    assert_eq!(x86::stmxcsr(), 0x3fa0);
    for k in 0..8 {
        assert_eq!(cpu.xmm[k], [k as u32 + 1, 0x80000000, 0x7fc01234, 0xffffffff]);
    }
    cpu.fl = x86::Flags::new(x86::RAW, 0, 0, 0x890, 32);
    cpu.df = false;
    cpu.st.register::<0xd8dd>();
    for k in 0..8 {
        cpu.xmm[k] = [0x12345678, k as u32, 0xabcdef00, 0x76543210];
    }
    cpu.esp += 4;
    x86::ldmxcsr(0x5fa0);
}

#[test]
fn complete_state_survives_emulator_and_callout_boundaries() {
    use ddnative_base::x86;
    let mut m = GuestMemory::new();
    m.map(CODE, 0x2000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    let mut code = vec![0xe8];
    code.extend((SERVICE.wrapping_sub(CODE + 5)).to_le_bytes());
    code.push(0xc3);
    m.poke(CODE, &code).unwrap();
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, RETURN);
    cpu.fl = x86::Flags::new(x86::RAW, 0, 0, 0x845, 32);
    cpu.df = true;
    cpu.st.push(2f32.to_bits());
    let mut value = [0u8; 10];
    value[..8].copy_from_slice(&0x8000000000000001u64.to_le_bytes());
    value[8..].copy_from_slice(&0x3fffu16.to_le_bytes());
    cpu.st.exec::<0x28db>(&mut value);
    for k in 0..8 {
        cpu.xmm[k] = [k as u32 + 1, 0x80000000, 0x7fc01234, 0xffffffff];
    }
    let mut registry = Registry::new();
    registry
        .insert(SERVICE, Binding::Lifted(check_and_change_state))
        .unwrap();
    x86::ldmxcsr(0x3fa0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ddruntime::emu::run(&registry, &mut m, &mut cpu, CODE)
    }));
    let mxcsr = x86::stmxcsr();
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
    assert_eq!(mxcsr, 0x5fa0);
    assert_eq!(cpu.fl.snapshot().0 & 0x8d5, 0x890);
    assert!(!cpu.df);
    assert_eq!(cpu.st.depth(), 1);
    assert_eq!(cpu.exit().st0, Some(2f32.to_bits()));
    for k in 0..8 {
        assert_eq!(cpu.xmm[k], [0x12345678, k as u32, 0xabcdef00, 0x76543210]);
    }
}

#[test]
fn observation_counts_unchanged_stores_and_excludes_state_stubs() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.poke(CODE, &[0xc6, 0x01, 0, 0xc3]).unwrap(); // mov byte [ecx],0; ret
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        ecx: STACK - 0x200,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, RETURN);
    let writes = std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeSet::new()));
    ddruntime::emu::run_observed(
        &Registry::new(),
        &mut m,
        &mut cpu,
        CODE,
        writes.clone(),
        std::time::Duration::from_secs(1),
    );
    assert_eq!(*writes.borrow(), [STACK - 0x200].into_iter().collect());
    // The differential harness restores/replaces code on the same page table.
    // Observed entries must discard translations from the preceding case.
    m.poke(CODE, &[0xc6, 0x01, 1, 0xc3]).unwrap();
    cpu.esp = STACK - 0x100;
    writes.borrow_mut().clear();
    ddruntime::emu::run_observed(
        &Registry::new(),
        &mut m,
        &mut cpu,
        CODE,
        writes.clone(),
        std::time::Duration::from_secs(1),
    );
    assert_eq!(m.u8(STACK - 0x200), 1);
    assert_eq!(*writes.borrow(), [STACK - 0x200].into_iter().collect());
}

#[test]
fn selector_reads_match_the_guest_cpu() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // MOV r32, ES/CS/SS/DS/FS/GS, then RET.
    m.poke(
        CODE,
        &[
            0x8c, 0xc0, 0x8c, 0xcb, 0x8c, 0xd1, 0x8c, 0xda, 0x8c, 0xe6, 0x8c, 0xef, 0xc3,
        ],
    )
    .unwrap();
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        fs_base: TEB,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, RETURN);
    let s = cpu.seg;
    ddruntime::emu::run(&Registry::new(), &mut m, &mut cpu, CODE);
    assert_eq!(
        [cpu.eax, cpu.ebx, cpu.ecx, cpu.edx, cpu.esi, cpu.edi],
        [s.es, s.cs, s.ss, s.ds, s.fs, s.gs].map(u32::from)
    );
}

#[test]
fn popfd_obeys_the_guest_user_privilege_level() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // Try to set IOPL=3 and clear IF: CPL3 must preserve both fields.
    m.poke(CODE, &[0x68, 0x02, 0x30, 0, 0, 0x9d, 0x9c, 0x58, 0xc3])
        .unwrap();
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, RETURN);
    ddruntime::emu::run(&Registry::new(), &mut m, &mut cpu, CODE);
    assert_eq!(cpu.eax & 0x3200, 0x200);
}

#[test]
fn changed_selector_returns_to_guest_cpu() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // XOR EAX,EAX; MOV GS,AX; RET.
    m.poke(CODE, &[0x31, 0xc0, 0x8e, 0xe8, 0xc3]).unwrap();
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        fs_base: TEB,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, RETURN);
    ddruntime::emu::run(&Registry::new(), &mut m, &mut cpu, CODE);
    assert_eq!(cpu.seg.gs, 0);
    assert_eq!(cpu.fs_base, TEB);
}

#[test]
fn operands_cross_partly_mapped_pages() {
    const DATA: u32 = 0x0200_0000;
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.map(DATA, 0x4000, READ | WRITE).unwrap();
    // Read a byte to map the first page, then read across its boundary.
    // Do the same for a store crossing two other pages.
    m.poke(
        CODE,
        &[
            0x8a, 0x41, 0xfe, 0x8b, 0x01, 0xc6, 0x42, 0xfe, 0, 0x89, 0x02, 0xc3,
        ],
    )
    .unwrap();
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        ecx: DATA + 0xffe,
        edx: DATA + 0x2ffe,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, RETURN);
    m.set_u32(cpu.ecx, 0x12345678);
    ddruntime::emu::run(&Registry::new(), &mut m, &mut cpu, CODE);
    assert_eq!(cpu.eax, 0x12345678);
    assert_eq!(m.u32(cpu.edx), 0x12345678);
}

#[test]
fn zero_call_token_and_zero_return_are_boundaries() {
    fn call(_: &mut dyn Mem, c: &mut Cpu) {
        c.eax = 41;
        c.esp += 4;
    }
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // XOR EAX,EAX; CALL EAX; INC EAX; RET to zero.
    m.poke(CODE, &[0x31, 0xc0, 0xff, 0xd0, 0x40, 0xc3]).unwrap();
    let mut cpu = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    m.set_u32(cpu.esp, 0);
    let mut registry = Registry::new();
    registry.insert(0, Binding::Lifted(call)).unwrap();
    ddruntime::emu::run(&registry, &mut m, &mut cpu, CODE);
    assert_eq!(cpu.eax, 42);
    assert_eq!(cpu.esp, STACK - 0xfc);
}

#[test]
fn private_state_relocates_around_guest_pages_and_observes_high_stores() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.map(0xfffe0000, 0x20000, READ | WRITE).unwrap();
    m.poke(CODE, &[0xc6, 0x01, 0x5a, 0xc3]).unwrap();
    for (i, target) in [0xffff1000, 0xffff86f8, 0xfffefaf8].into_iter().enumerate() {
        if i == 1 {
            // Occupy the previous instance's relocated private range too.
            m.map(0xfffc0000, 0x20000, READ | WRITE).unwrap();
        }
        let mut cpu = Cpu {
            esp: STACK - 0x100,
            ecx: target,
            ..Cpu::default()
        };
        m.set_u32(cpu.esp, RETURN);
        let writes = std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeSet::new()));
        ddruntime::emu::run_observed(
            &Registry::new(),
            &mut m,
            &mut cpu,
            CODE,
            writes.clone(),
            std::time::Duration::from_secs(1),
        );
        assert_eq!(m.u8(target), 0x5a);
        assert_eq!(*writes.borrow(), [target].into_iter().collect());
    }
}

#[test]
fn recorder_flat_selectors_are_preserved() {
    use ddnative_base::x86::Segments;
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.poke(CODE, &[0x8c, 0xc8, 0x8c, 0xd1, 0x8c, 0xe2, 0xc3]).unwrap();
    let mut c = Cpu {
        esp: STACK - 0x100,
        fs_base: TEB,
        ..Cpu::default()
    };
    c.seg = Segments {
        es: 0,
        cs: 0,
        ss: 8,
        ds: 0,
        fs: 16,
        gs: 0,
    };
    m.set_u32(c.esp, RETURN);
    ddruntime::emu::run(&Registry::new(), &mut m, &mut c, CODE);
    assert_eq!([c.eax, c.ecx, c.edx], [0, 8, 16]);
    assert_eq!(c.fs_base, TEB);
}

#[test]
fn stack_allocation_helper_returns_with_lower_esp() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    // Return address preserved while the callee adjusts ESP.
    m.poke(CODE, &[0x58, 0x83, 0xec, 0x10, 0xff, 0xe0]).unwrap();
    let mut c = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    m.set_u32(c.esp, RETURN);
    ddruntime::emu::run(&Registry::new(), &mut m, &mut c, CODE);
    assert_eq!(c.esp, STACK - 0x10c);
}

#[test]
fn floating_operations_preserve_hardware_exception_flags_and_results() {
    use ddnative_base::x86;
    let a = [1f32.to_bits(), 0, 1, 1e38f32.to_bits()];
    let b = [3f32.to_bits(), 0, 2f32.to_bits(), 1e-30f32.to_bits()];
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    let divided = x86::divps(a, b);
    let integer = x86::cvttps2dq([0; 4], divided);
    let mut compared = x86::compare_ps::<3>(divided, b);
    compared[0] = x86::addss(compared[0], 3f32.to_bits());
    let converted = x86::cvttss2si(compared[0]);
    let flags = x86::stmxcsr();
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.map(TEB, 0x1000, READ | WRITE).unwrap();
    m.set_u32(TEB + 68, 3f32.to_bits());
    // Packed arithmetic/comparison, then a scalar FS:[EAX+ECX*4-4]
    // operand and conversion to a general register.
    m.poke(
        CODE,
        &[
            0x0f, 0x5e, 0xc1, 0xf3, 0x0f, 0x5b, 0xd0, 0x0f, 0xc2, 0xc1, 3, 0x64, 0xf3, 0x0f, 0x58, 0x44,
            0x88, 0xfc, 0xf3, 0x0f, 0x2c, 0xc0, 0xc3,
        ],
    )
    .unwrap();
    let mut c = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    c.eax = 8;
    c.ecx = 16;
    c.fs_base = TEB;
    c.xmm[0] = a;
    c.xmm[1] = b;
    m.set_u32(c.esp, RETURN);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ddruntime::emu::run(&Registry::new(), &mut m, &mut c, CODE);
    }));
    let got = x86::stmxcsr();
    x86::ldmxcsr(x86::MXCSR_DEFAULT);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
    assert_eq!(got, flags);
    assert_eq!(c.xmm[0], compared);
    assert_eq!(c.xmm[2], integer);
    assert_eq!(c.eax, converted);
}

#[test]
fn x87_logarithm_matches_the_host_extended_result() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.poke(CODE, &[0xd9, 0xf1, 0xc3]).unwrap(); // FYL2X; RET
    let mut c = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    c.st.push(1.234567f32.to_bits());
    c.st.push(3.98765f32.to_bits());
    let mut expected = c.st.clone();
    expected.register::<0xf1d9>();
    m.set_u32(c.esp, RETURN);
    ddruntime::emu::run(&Registry::new(), &mut m, &mut c, CODE);
    assert_eq!(c.st.top_extended(), expected.top_extended());
    assert_eq!(c.st.status(), expected.status());
}

#[test]
fn observed_integer_loop_reaches_the_diagnostic_deadline() {
    let mut m = GuestMemory::new();
    m.map(CODE, 0x1000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    m.poke(CODE, &[0xeb, 0xfe]).unwrap(); // JMP to this instruction
    let mut c = Cpu {
        esp: STACK - 0x100,
        ..Cpu::default()
    };
    m.set_u32(c.esp, RETURN);
    let writes = std::rc::Rc::new(std::cell::RefCell::new(std::collections::BTreeSet::new()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ddruntime::emu::run_observed(
            &Registry::new(),
            &mut m,
            &mut c,
            CODE,
            writes,
            std::time::Duration::from_millis(5),
        );
    }));
    let error = result.expect_err("integer loop must stop");
    assert!(error
        .downcast_ref::<String>()
        .unwrap()
        .contains("diagnostic time limit"));
}
