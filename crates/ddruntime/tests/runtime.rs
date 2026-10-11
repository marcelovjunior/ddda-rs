//! Stage R03 tests (docs/RUNTIME.md): guest memory semantics, image
//! loading, and the dispatcher (reentrancy, conflicts, unbound targets).
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use ddruntime::dispatch::{Binding, Registry, Unbound};
use ddruntime::memory::{GuestMemory, PAGE, READ, WRITE};
use std::rc::Rc;

fn fault_message(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let e = std::panic::catch_unwind(f).expect_err("expected a guest fault");
    e.downcast_ref::<String>().cloned().unwrap_or_default()
}

#[test]
fn unmapped_and_readonly_accesses_fault() {
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x1000, READ).unwrap();
    assert_eq!(m.u32(0x10000), 0, "fresh pages are zero");
    let msg = fault_message(std::panic::AssertUnwindSafe(|| { m.u32(0x20000); }));
    assert!(msg.contains("read of 4 byte(s) at 0x00020000: unmapped page"), "{msg}");
    let msg = fault_message(std::panic::AssertUnwindSafe(|| m.set_u8(0x10010, 1)));
    assert!(msg.contains("not writable"), "{msg}");
    // the null page is never mapped
    let msg = fault_message(std::panic::AssertUnwindSafe(|| { m.u8(0); }));
    assert!(msg.contains("0x00000000"), "{msg}");
}

#[test]
fn unaligned_page_crossing_and_wrapping_accesses_are_byte_exact() {
    let mut m = GuestMemory::new();
    m.map(0x20000, 2 * PAGE, READ | WRITE).unwrap();
    m.set_u32(0x20FFE, 0xA1B2_C3D4); // crosses into the second page
    assert_eq!(m.u8(0x20FFE), 0xD4);
    assert_eq!(m.u8(0x21001), 0xA1);
    assert_eq!(m.u32(0x20FFE), 0xA1B2_C3D4);
    assert_eq!(m.u16(0x20FFF), 0xB2C3);
    // a crossing access into an unmapped page faults as a whole, writing nothing
    let msg = fault_message(std::panic::AssertUnwindSafe(|| m.set_u32(0x21FFE, 0xFFFF_FFFF)));
    assert!(msg.contains("unmapped"), "{msg}");
    assert_eq!(m.u16(0x21FFE), 0, "a faulting store writes nothing");
    // addresses wrap at 0xFFFFFFFF like the CPU's
    m.map(0xFFFF_F000, PAGE, READ | WRITE).unwrap();
    m.map(0, PAGE, READ | WRITE).unwrap();
    m.set_u32(0xFFFF_FFFE, 0x1122_3344);
    assert_eq!(m.u16(0xFFFF_FFFE), 0x3344);
    assert_eq!(m.u16(0), 0x1122);
}

#[test]
fn mappings_cannot_overlap() {
    let mut m = GuestMemory::new();
    m.map(0x30000, 0x2000, READ).unwrap();
    assert_eq!(m.map(0x31800, 0x10, READ).unwrap_err().page, 0x31000);
}

// --- dispatcher ---------------------------------------------------------

fn ret(m: &mut dyn Mem, cpu: &mut Cpu) {
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
}
fn push_and_call_ext(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, 0x0040_1234);
    x86::call_ext(m, cpu, target);
}
// lifted-style functions: 0x1000 calls 0x2000 through call_ext, which calls
// 0x3000 the same way (nested), which counts down eax recursively via 0x3000
fn f1000(m: &mut dyn Mem, cpu: &mut Cpu) {
    push_and_call_ext(m, cpu, 0x2000);
    cpu.ebx += 1;
    ret(m, cpu);
}
fn f2000(m: &mut dyn Mem, cpu: &mut Cpu) {
    push_and_call_ext(m, cpu, 0x3000);
    cpu.ebx += 10;
    ret(m, cpu);
}
fn f3000(m: &mut dyn Mem, cpu: &mut Cpu) {
    if cpu.eax > 0 {
        cpu.eax -= 1;
        push_and_call_ext(m, cpu, 0x3000);
        cpu.ebx += 100;
    }
    ret(m, cpu);
}
fn f4000(m: &mut dyn Mem, cpu: &mut Cpu) {
    push_and_call_ext(m, cpu, 0xDEAD_0000);
    ret(m, cpu);
}

fn stack() -> (GuestMemory, Cpu) {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    let cpu = Cpu { esp: 0x0011_0000, ..Cpu::default() };
    (m, cpu)
}

#[test]
fn nested_and_recursive_ext_calls_reenter_the_dispatcher() {
    let mut r = Registry::new();
    for (a, f) in [(0x1000u32, f1000 as fn(&mut dyn Mem, &mut Cpu)), (0x2000, f2000), (0x3000, f3000)] {
        r.insert(a, Binding::Lifted(f)).unwrap();
    }
    let r = Rc::new(r);
    r.install();
    let (mut m, mut cpu) = stack();
    cpu.eax = 3;
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0);
    r.call(&mut m, &mut cpu, 0x1000).unwrap();
    assert_eq!(cpu.ebx, 1 + 10 + 3 * 100);
    assert_eq!(cpu.esp, 0x0011_0000, "every level popped its return address");
    x86::set_ext_calls(None);
}

#[test]
fn an_unbound_target_stops_with_its_address_and_caller() {
    let mut r = Registry::new();
    r.insert(0x4000, Binding::Lifted(f4000)).unwrap();
    r.name(0xDEAD_0000, "sNowhere::vf03");
    let r = Rc::new(r);
    r.install();
    let (mut m, mut cpu) = stack();
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0);
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.call(&mut m, &mut cpu, 0x4000)))
        .expect_err("the call must stop");
    let u = e.downcast_ref::<Unbound>().expect("an Unbound report");
    assert_eq!(u.target, 0xDEAD_0000);
    assert_eq!(u.return_to, Some(0x0040_1234));
    assert_eq!(u.name.as_deref(), Some("sNowhere::vf03"));
    assert!(u.to_string().contains("untranslated code at 0xdead0000 (sNowhere::vf03)"));
    // the handler stays installed after the panic (nothing was taken out)
    assert!(r.call(&mut m, &mut cpu, 0x5000).is_err());
    x86::set_ext_calls(None);
}

#[test]
fn one_binding_per_address() {
    let mut r = Registry::new();
    r.insert(0x1000, Binding::Lifted(f1000)).unwrap();
    let c = r.insert(0x1000, Binding::Service(f2000)).unwrap_err();
    assert_eq!((c.addr, c.existing, c.new), (0x1000, "lifted", "service"));
}

// --- the real image -----------------------------------------------------

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

#[test]
fn the_installed_exe_maps_at_its_virtual_addresses() {
    let Some(exe) = installed_exe() else { return };
    let mut m = GuestMemory::new();
    let mapped = ddruntime::image::load(&mut m, &exe).expect("load");
    let image = exe.image().unwrap();
    // every file-backed byte of every section reads back through the guest
    for s in image.sections() {
        let len = s.virtual_size.min(s.raw_size).min(0x100);
        assert_eq!(m.read(s.va, len).unwrap(), image.bytes(s.va, len as usize).unwrap(), "{}", s.name);
    }
    let data = mapped.iter().find(|s| s.name == ".data").expect(".data");
    assert_eq!(data.perm, READ | WRITE);
    // .data's virtual tail beyond the file (BSS) is zero
    let s = image.sections().iter().find(|s| s.name == ".data").unwrap();
    assert!(s.virtual_size > s.raw_size, ".data has a zero-filled tail in this build");
    assert_eq!(m.u32(s.va + s.raw_size + 0x100), 0);
    // code is readable, not writable
    let text = image.sections().iter().find(|s| s.name == ".text").unwrap();
    assert_eq!(m.perm(text.va), READ);
}

// --- services -------------------------------------------------------------

use ddruntime::services;

fn call_service(f: fn(&mut dyn Mem, &mut Cpu), m: &mut GuestMemory, args: &[u32]) -> Cpu {
    let mut cpu = Cpu { esp: 0x0011_0000, ..Cpu::default() };
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_5678);
    f(m, &mut cpu);
    cpu
}

#[test]
fn heap_blocks_are_aligned_distinct_zeroed_and_reused_by_size() {
    let mut h = services::Heap::default();
    let a = h.alloc(10).unwrap();
    let b = h.alloc(10).unwrap();
    assert_eq!(a % 16, 0);
    assert_ne!(a, b);
    assert_eq!(h.size_of(a), Some(16));
    h.free(a).unwrap();
    assert_eq!(h.free(a), Err(a), "double free is reported");
    assert_eq!(h.alloc(12).unwrap(), a, "a freed block of the same size is reused");
    let (mut m, _) = stack();
    let mut r = Registry::new();
    services::install(&mut m, &mut r).unwrap();
    let cpu = call_service(services::calloc_crt, &mut m, &[3, 8]);
    assert!(cpu.eax >= services::HEAP_BASE);
    assert_eq!(m.read(cpu.eax, 24).unwrap(), vec![0; 24]);
    assert_eq!(cpu.esp, 0x0011_0000 - 8, "cdecl: the caller pops the arguments");
}

#[test]
fn critical_sections_count_recursion_and_release() {
    let (mut m, _) = stack();
    let cs = 0x0010_8000;
    call_service(services::initialize_critical_section, &mut m, &[cs]);
    assert_eq!(m.u32(cs + 4), u32::MAX, "LockCount starts at -1");
    call_service(services::enter_critical_section, &mut m, &[cs]);
    call_service(services::enter_critical_section, &mut m, &[cs]);
    assert_eq!((m.u32(cs + 8), m.u32(cs + 12)), (2, services::MAIN_THREAD_ID));
    call_service(services::leave_critical_section, &mut m, &[cs]);
    let cpu = call_service(services::leave_critical_section, &mut m, &[cs]);
    assert_eq!((m.u32(cs + 4), m.u32(cs + 8), m.u32(cs + 12)), (u32::MAX, 0, 0));
    assert_eq!(cpu.esp, 0x0011_0000, "stdcall: the callee pops its argument");
}

#[test]
fn every_import_gets_a_distinct_named_token() {
    let Some(exe) = installed_exe() else { return };
    let mut m = GuestMemory::new();
    ddruntime::image::load(&mut m, &exe).unwrap();
    let imports = ddruntime::image::bind_imports(&mut m, &exe).unwrap();
    assert_eq!(imports.len(), 273, "R01 counted 273 normal imports");
    let tokens: std::collections::HashSet<u32> = imports.iter().map(|i| i.token).collect();
    assert_eq!(tokens.len(), imports.len());
    for i in &imports {
        assert_eq!(m.u32(i.slot), i.token, "{}", i.full_name());
    }
    assert!(imports.iter().any(|i| i.full_name() == "KERNEL32.dll!EncodePointer"));
}
