//! Port-only registration is exercised without generated shards or emulator.
#![cfg(not(feature = "game"))]

use ddnative_base::{mem::Mem, x86::{self, Cpu}};
use ddruntime::{dispatch::{Binding, Registry, Unbound}, memory::{GuestMemory, READ, WRITE}};
use std::sync::Arc;

const CALLER: u32 = 0x0040_1000;
const PORT_ONLY: u32 = 0x0112_bb40;
const UNKNOWN: u32 = 0x0112_bb50;
const RETURN: u32 = CALLER + 5;

fn port(m: &mut dyn Mem, cpu: &mut Cpu) {
    assert_eq!(m.u32(cpu.esp), RETURN);
    cpu.eax += 1;
    cpu.esp += 4;
}

fn lifted_caller(m: &mut dyn Mem, cpu: &mut Cpu) {
    // The same ABI sequence lift.py emits for a call without a lifted body.
    cpu.esp -= 4;
    m.set_u32(cpu.esp, RETURN);
    x86::call_ext(m, cpu, PORT_ONLY);
    cpu.esp += 4;
}

#[test]
fn lifted_call_ext_reaches_registered_port_without_a_lifted_body() {
    x86::set_overrides(&[(PORT_ONLY, port)]);
    let mut registry = Registry::with_lifted_program();
    assert!(matches!(registry.get(PORT_ONLY), Some(Binding::Lifted(_))));
    registry.insert(CALLER, Binding::Lifted(lifted_caller)).unwrap();
    let registry = Arc::new(registry);
    registry.install();
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { x86::set_ext_calls(None); }
    }
    let _reset = Reset;
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x1000, READ | WRITE).unwrap();
    let mut cpu = Cpu { esp: 0x0010_0800, ..Cpu::default() };
    for expected in 1..=2 {
        cpu.esp = 0x0010_0800;
        registry.call(&mut m, &mut cpu, CALLER).unwrap();
        assert_eq!(cpu.eax, expected); // second call also exercises the cache
        assert_eq!(cpu.esp, 0x0010_0804);
    }
    // Unknown targets still reach the existing unbound/emulator decision.
    m.set_u32(cpu.esp, RETURN);
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        x86::call_ext(&mut m, &mut cpu, UNKNOWN);
    })).unwrap_err();
    let unbound = err.downcast::<Unbound>().unwrap();
    assert_eq!(unbound.target, UNKNOWN);
    assert_eq!(unbound.return_to, Some(RETURN));
}
