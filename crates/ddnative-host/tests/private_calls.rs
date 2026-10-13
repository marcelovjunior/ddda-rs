#![cfg(feature = "private-replay")]
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu, LiftFn},
};

struct Empty;
impl Mem for Empty {
    fn u8(&self, _: u32) -> u8 {
        panic!("unexpected read")
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        panic!("unexpected write")
    }
}
fn original(_: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 10;
}
fn global(_: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 20;
}
fn private(_: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 30;
}
fn lookup(target: u32) -> Option<LiftFn> {
    (target == 0x400010).then_some(original)
}
fn isolated(target: u32) -> LiftFn {
    assert_eq!(target, 0x400010);
    private
}

#[test]
fn resolver_scope_is_thread_owned_and_restored_after_unwind() {
    fn nested(_: u32) -> LiftFn {
        global
    }
    let mut cpu = Cpu::default();
    assert!(!x86::private_calls::dispatch(&mut Empty, &mut cpu, 0x400010));
    let outer = x86::private_calls::enter(isolated);
    x86::private_calls::dispatch(&mut Empty, &mut cpu, 0x400010);
    assert_eq!(cpu.eax, 30);
    std::thread::spawn(|| {
        assert!(!x86::private_calls::dispatch(
            &mut Empty,
            &mut Cpu::default(),
            0x400010
        ))
    })
    .join()
    .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _inner = x86::private_calls::enter(nested);
        x86::private_calls::dispatch(&mut Empty, &mut cpu, 0x400010);
        assert_eq!(cpu.eax, 20);
        panic!("unwind private scope");
    }));
    assert!(result.is_err());
    x86::private_calls::dispatch(&mut Empty, &mut cpu, 0x400010);
    assert_eq!(cpu.eax, 30);
    drop(outer);
    assert!(!x86::private_calls::dispatch(&mut Empty, &mut cpu, 0x400010));
}

#[test]
fn private_resolution_precedes_warmed_cache_and_global_local_override() {
    ddnative_host::install();
    x86::set_lifted_lookup(lookup);
    x86::set_overrides(&[(0x400010, global)]);
    let mut cpu = Cpu::default();
    x86::call_lifted(&mut Empty, &mut cpu, 0x400010);
    assert_eq!(cpu.eax, 20);
    {
        let _scope = x86::private_calls::enter(isolated);
        x86::call_lifted(&mut Empty, &mut cpu, 0x400010);
        assert_eq!(cpu.eax, 30);
        x86::call_local(&mut Empty, &mut cpu, 0x400010, original);
        assert_eq!(cpu.eax, 30);
    }
    x86::call_lifted(&mut Empty, &mut cpu, 0x400010);
    assert_eq!(cpu.eax, 20);
    x86::call_local(&mut Empty, &mut cpu, 0x400010, original);
    assert_eq!(cpu.eax, 20);
}
