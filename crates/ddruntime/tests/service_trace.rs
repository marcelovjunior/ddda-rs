//! The service-call trace (`DDRUNTIME_TRACE_SERVICES`, dispatch.rs): its
//! own test binary, since the trace file is chosen once per process.
use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::{
    dispatch::{Binding, Registry},
    memory::{GuestMemory, READ, WRITE},
    runtime,
};
use std::sync::Arc;

const STACK: u32 = 0x1001_0000;

/// A stdcall service of two arguments returning their sum.
fn add2(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (a, b) = (m.u32(cpu.esp + 4), m.u32(cpu.esp + 8));
    cpu.eax = a.wrapping_add(b);
    cpu.esp += 12; // ret 8
}

#[test]
fn named_service_calls_are_traced_with_caller_arguments_and_result() {
    let path = std::env::temp_dir().join(format!("ddruntime-service-trace-{}.txt", std::process::id()));
    std::env::set_var("DDRUNTIME_TRACE_SERVICES", &path);
    std::env::set_var("DDRUNTIME_TRACE_SERVICES_MAX", "2");
    runtime::enter(Arc::new(runtime::Runtime::default()));
    let mut m = GuestMemory::new();
    m.map(STACK - 0x10000, 0x10000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    r.insert(0x7000_0010, Binding::Service(add2)).unwrap();
    r.name(0x7000_0010, "TEST.dll!Add2");
    r.insert(0x7000_0020, Binding::Service(add2)).unwrap(); // unnamed: not traced
    let call = |m: &mut GuestMemory, target: u32, a: u32, b: u32| {
        let esp = STACK - 0x100;
        for (k, v) in [0x0040_1234, a, b].into_iter().enumerate() {
            m.set_u32(esp + 4 * k as u32, v);
        }
        let mut cpu = Cpu { esp, ..Cpu::default() };
        r.call(m, &mut cpu, target).unwrap();
        cpu.eax
    };
    assert_eq!(call(&mut m, 0x7000_0010, 2, 3), 5);
    assert_eq!(call(&mut m, 0x7000_0020, 4, 5), 9);
    assert_eq!(call(&mut m, 0x7000_0010, 6, 7), 13);
    assert_eq!(call(&mut m, 0x7000_0010, 8, 9), 17, "past the cap: still called, not traced");
    // the cap's last line flushes the file
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    let tid = ddruntime::threads::current_tid();
    assert_eq!(lines[0], format!("{tid} TEST.dll!Add2 ret=0x401234 args=0x2 0x3 0x0 0x0 -> 0x5"));
    assert_eq!(lines[1], format!("{tid} TEST.dll!Add2 ret=0x401234 args=0x6 0x7 0x0 0x0 -> 0xd"));
}
