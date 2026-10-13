//! Opt-in foreground participation through the original property setter.
//! No CPU report, pool allocation, or scheduler cap is changed.
#![cfg_attr(not(feature = "game"), allow(dead_code))]
use ddnative_base::{mem::Mem, x86::{Cpu, LiftFn}};
use std::sync::OnceLock;

const CONSTRUCTOR: u32 = 0x00dbe810;
const SETTER: u32 = 0x00dbd620;
static HOOK: OnceLock<(LiftFn, LiftFn, u32)> = OnceLock::new();

fn requested(value: Option<&str>) -> Result<Option<u32>, &'static str> {
    match value {
        None => Ok(None),
        Some(s) => s.parse::<u32>().ok().filter(|n| (1..=8).contains(n))
            .map(Some).ok_or("DDRUNTIME_JOB_PARTICIPANTS must be an integer from 1 through 8"),
    }
}

pub(crate) fn install(ports: &mut Vec<(u32, LiftFn)>, bodies: &[(u32, LiftFn)]) {
    let value = std::env::var_os("DDRUNTIME_JOB_PARTICIPANTS");
    let value = value.as_ref().map(|v| v.to_str().expect("DDRUNTIME_JOB_PARTICIPANTS must be Unicode"));
    let count = match requested(value) {
        Ok(None) => return,
        Ok(Some(n)) => n,
        Err(message) => panic!("{message}"),
    };
    let find = |address| ports.iter().chain(bodies).find(|(a, _)| *a == address)
        .map(|(_, f)| *f).expect("job participant option requires original constructor and setter bodies");
    let hook = (find(CONSTRUCTOR), find(SETTER), count);
    HOOK.set(hook).expect("job participant hook installed once before overrides freeze");
    ports.retain(|(a, _)| *a != CONSTRUCTOR);
    ports.push((CONSTRUCTOR, construct));
}

fn construct(m: &mut dyn Mem, cpu: &mut Cpu) {
    let &(original, setter, count) = HOOK.get().expect("participant hook installed");
    construct_with(m, cpu, original, setter, count);
}

fn construct_with(m: &mut dyn Mem, cpu: &mut Cpu, original: LiftFn, setter: LiftFn, count: u32) {
    let object = cpu.ecx;
    original(m, cpu);
    // Original ABI: DBE810 returns this in EAX; DBD620 takes this in ECX,
    // one stack argument, ret 4. The setter clamps against +1C040 and eight,
    // writes +9C; DBFA90 promotes +9C to active +98 at frame start.
    // Invoke the existing body, never duplicate its logic or patch +98.
    assert_eq!(cpu.eax, object, "sMain constructor return contract");
    let mut call_cpu = cpu.clone();
    let stack = cpu.esp.wrapping_sub(8);
    let saved = [m.u32(stack), m.u32(stack.wrapping_add(4))];
    call_cpu.ecx = object;
    call_cpu.esp = stack;
    m.set_u32(stack, 0); // Host-invoked body returns to Rust, no guest continuation.
    m.set_u32(stack.wrapping_add(4), count);
    setter(m, &mut call_cpu);
    assert_eq!(call_cpu.esp, cpu.esp, "job participant setter ret 4 contract");
    m.set_u32(stack, saved[0]);
    m.set_u32(stack.wrapping_add(4), saved[1]);
    ddnative_base::x87::park();
    let mxcsr = ddnative_base::x86::stmxcsr();
    eprintln!("job participants: requested={count} configured={} active={} cpus={} (original 00DBD620; active updates at frame start)",
        m.u32(object.wrapping_add(0x9c)), m.u32(object.wrapping_add(0x98)),
        m.u32(object.wrapping_add(0x1c040)));
    ddnative_base::x86::ldmxcsr(mxcsr);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddnative_base::mem::Sparse;

    #[test]
    fn option_is_strict_and_disabled_when_absent() {
        assert_eq!(requested(None), Ok(None));
        for n in 1..=8 { assert_eq!(requested(Some(&n.to_string())), Ok(Some(n))); }
        for s in ["", "0", "9", "16", "-1", "abc", " 8", "4294967296"] {
            assert!(requested(Some(s)).is_err(), "{s}");
        }
    }

    fn constructor(m: &mut dyn Mem, cpu: &mut Cpu) {
        m.set_u32(cpu.ecx + 0x98, 7);
        m.set_u32(cpu.ecx + 0x9c, 7);
        cpu.eax = cpu.ecx;
        cpu.ecx = 0xdeadbeef;
        cpu.esp += 4;
    }

    // ABI spy, deliberately not an implementation of the game setter.
    fn setter(m: &mut dyn Mem, cpu: &mut Cpu) {
        assert_eq!(cpu.ecx, 0x1000);
        assert_eq!(m.u32(cpu.ecx + 0x9c), 7); // called after constructor
        assert_eq!(m.u32(cpu.esp + 4), 8);
        m.set_u32(cpu.ecx + 0x9c, 3); // stand-in result must remain authoritative
        cpu.eax = 99;
        cpu.xmm[0] = [99; 4];
        cpu.esp += 8;
    }

    #[test]
    fn invokes_setter_after_init_preserving_constructor_cpu_and_stack() {
        let mut m = Sparse::default();
        m.set_u32(0x7ffc, 0x12345678);
        m.set_u32(0x8000, 0xabcdef01);
        m.set_u32(0x1000 + 0x1c040, 16);
        let mut cpu = Cpu { ecx: 0x1000, esp: 0x8000, ..Cpu::default() };
        let mut expected = cpu.clone();
        constructor(&mut m, &mut expected);
        construct_with(&mut m, &mut cpu, constructor, setter, 8);
        assert_eq!(format!("{cpu:?}"), format!("{expected:?}"));
        assert_eq!(m.u32(0x7ffc), 0x12345678);
        assert_eq!(m.u32(0x8000), 0xabcdef01);
        assert_eq!(m.u32(0x1098), 7); // never patch active count
        assert_eq!(m.u32(0x109c), 3); // original setter owns clamping
    }
}
