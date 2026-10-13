//! L135: real audited service bodies preserve the complete guest FP state.
#![cfg(all(windows, target_arch = "x86_64"))]
use ddnative_base::{execution::{Flow, Step}, mem::Mem, x86::{self, Cpu}, x87};
use ddruntime::{dispatch::{Binding, Registry}, image::Import, memory::{GuestMemory, READ, WRITE}, runtime, services};
use std::sync::Arc;

const STACK: u32 = 0x18000;
const OPERAND: u32 = 0x19000;
const SERVICE: u32 = 0x70000000;
const ENTRY: u32 = 0x5000;

struct Restore(u32);
impl Drop for Restore {
    fn drop(&mut self) { x87::park(); x86::ldmxcsr(self.0); }
}

fn lookup(ip: u32) -> Option<Step> {
    match ip {
        ENTRY => Some(|_, _, _| Flow::Next(SERVICE)),
        _ => None,
    }
}

#[test]
fn audited_imports_preserve_live_extended_state_and_every_mxcsr_mode() {
    let _restore = Restore(x86::stmxcsr());
    runtime::enter(Arc::new(runtime::Runtime::default()));
    ddnative_host::install();
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    // Aligned host atomic and unaligned guest-access fallback, direct call
    // and stepped dispatch, empty through full guest x87 stacks.
    for name in services::INTEGER_APIS {
        for stepped in [false, true] {
            let mut r = Registry::new();
            services::bind_apis(&mut r, &[Import { dll: "kernel32.DLL".into(), name: (*name).into(), slot: 0, token: SERVICE }]).unwrap();
            assert!(matches!(r.get(SERVICE), Some(Binding::Integer(_))));
            if stepped { r.set_instruction_lookup(lookup); }
            for depth in 0..=8 {
                for mode in 0..4 {
                    for offset in [0, 1] {
                        x87::park();
                        let mut c = Cpu { esp: STACK, ..Cpu::default() };
                        let cw = (0x037fu16 | (mode << 10)).to_le_bytes();
                        let mut data = [0u8; 10];
                        data[..2].copy_from_slice(&cw);
                        c.st.exec::<0x28d9>(&mut data);
                        for _ in 0..depth { c.st.register::<0xebd9>(); } // extended pi
                        // Snapshot normalizes only instruction/data pointers.
                        let before = c.st.clone().state;
                        let live = x87::live_id();
                        m.set_u32(STACK, 0xffff0000);
                        m.set_u32(STACK + 4, OPERAND + offset);
                        m.set_u32(STACK + 8, 17);
                        m.set_u32(STACK + 12, 9);
                        m.set_u32(OPERAND + offset, 9);
                        // All sticky flags, FTZ and DAZ, all rounding modes.
                        let mxcsr = 0x9fffu32 | ((mode as u32) << 13);
                        x86::ldmxcsr(mxcsr);
                        r.call(&mut m, &mut c, if stepped { ENTRY } else { SERVICE }).unwrap();
                        assert_eq!(x86::stmxcsr(), mxcsr, "{name}");
                        assert_eq!(c.mxcsr.0, mxcsr);
                        assert_eq!(x87::live_id(), live, "{name}: unexpectedly parked");
                        assert_eq!(c.st.clone().state, before, "{name}: changed x87");
                        let expected = match *name {
                            "EncodePointer" | "DecodePointer" => OPERAND + offset,
                            "GetCurrentThreadId" => ddruntime::threads::current_tid(),
                            "InterlockedIncrement" => 10,
                            "InterlockedDecrement" => 8,
                            _ => 9,
                        };
                        assert_eq!(c.eax, expected, "{name}");
                    }
                }
            }
        }
    }
}

#[test]
fn ordinary_services_still_park_and_isolate_host_rounding_and_flags() {
    fn host(_: &mut dyn Mem, c: &mut Cpu) {
        assert_eq!(x87::live_id(), x87::NONE);
        assert_eq!(x86::stmxcsr(), x86::MXCSR_DEFAULT);
        x86::ldmxcsr(0x3fa0);
        c.esp += 4;
    }
    let _restore = Restore(x86::stmxcsr());
    runtime::enter(Arc::new(runtime::Runtime::default()));
    ddnative_host::install();
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    r.insert(SERVICE, Binding::LockFree(host)).unwrap();
    let mut c = Cpu { esp: STACK, ..Cpu::default() };
    c.st.register::<0xebd9>();
    let before = c.st.clone().state;
    x86::ldmxcsr(0xdfff);
    r.call(&mut m, &mut c, SERVICE).unwrap();
    assert_eq!(x86::stmxcsr(), 0xdfff);
    assert_eq!(c.st.clone().state, before);
}

#[test]
fn stepped_device_state_keeps_x87_live_but_isolates_mxcsr() {
    fn probe(_: &mut dyn Mem, c: &mut Cpu) {
        assert_eq!(x87::live_id(), c.st.id);
        assert_eq!(x86::stmxcsr(), x86::MXCSR_DEFAULT);
        x86::ldmxcsr(0x1fa0); // host sticky flags must not escape
        c.esp += 4;
    }
    let _restore = Restore(x86::stmxcsr());
    runtime::enter(Arc::new(runtime::Runtime::default()));
    ddnative_host::install();
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    m.set_u32(STACK, 0xffff0000);
    let mut r = Registry::new();
    r.insert(SERVICE, Binding::DeviceState(probe)).unwrap();
    r.set_instruction_lookup(lookup);
    let mut c = Cpu { esp: STACK, ..Cpu::default() };
    c.st.register::<0xebd9>();
    let before = c.st.clone().state;
    x86::ldmxcsr(0xdfff);
    r.call(&mut m, &mut c, ENTRY).unwrap();
    assert_eq!(x86::stmxcsr(), 0xdfff);
    assert_eq!(x87::live_id(), c.st.id);
    assert_eq!(c.st.clone().state, before);
}

#[test]
fn diagnostics_use_full_boundary_for_integer_services() {
    if std::env::var_os("L135_DIAGNOSTIC_CHILD").is_none() {
        for variable in ["DDRUNTIME_LOCK_STATS", "DDRUNTIME_TRACE_SERVICES"] {
            let path = std::env::temp_dir().join(format!("l135-fp-{}.log", std::process::id()));
            let value = if variable == "DDRUNTIME_LOCK_STATS" { "1".into() } else { path.as_os_str().to_owned() };
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "diagnostics_use_full_boundary_for_integer_services", "--test-threads=1"])
                .env("L135_DIAGNOSTIC_CHILD", "1").env(variable, value).status().unwrap();
            let _ = std::fs::remove_file(path);
            assert!(status.success(), "{variable}");
        }
        return;
    }
    fn probe(_: &mut dyn Mem, c: &mut Cpu) {
        assert_eq!(x87::live_id(), x87::NONE);
        assert_eq!(x86::stmxcsr(), x86::MXCSR_DEFAULT);
        c.esp += 4;
    }
    let _restore = Restore(x86::stmxcsr());
    runtime::enter(Arc::new(runtime::Runtime::default()));
    ddnative_host::install();
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    r.insert(SERVICE, Binding::Integer(probe)).unwrap();
    r.name(SERVICE, "TEST.dll!IntegerProbe");
    let r = Arc::new(r);
    r.install();
    let mut c = Cpu { esp: STACK, ..Cpu::default() };
    c.st.register::<0xebd9>();
    let before = c.st.clone().state;
    x86::ldmxcsr(0xdfff);
    // Second iteration exercises the installed ext-call binding cache.
    for _ in 0..2 {
        c.esp = STACK;
        x86::call_ext(&mut m, &mut c, SERVICE);
        assert_eq!(x86::stmxcsr(), 0xdfff);
        assert_eq!(c.st.clone().state, before);
    }
}

/// Paired same-process microbenchmark, no game/shards/GPU. Includes dispatch,
/// an actual atomic service and the next guest x87 instruction. Run explicitly
/// with --ignored --nocapture --test-threads=1, preferably --profile play.
#[test]
#[ignore]
fn paired_integer_boundary_cost() {
    #[link(name = "kernel32")]
    extern "system" { fn SetThreadAffinityMask(thread: isize, mask: usize) -> usize; }
    struct Affinity(usize);
    impl Drop for Affinity {
        fn drop(&mut self) { unsafe { SetThreadAffinityMask(-2, self.0); } }
    }
    let old = unsafe { SetThreadAffinityMask(-2, 4) }; // current thread, CPU 2
    assert_ne!(old, 0);
    let _affinity = Affinity(old);
    let _restore = Restore(x86::stmxcsr());
    runtime::enter(Arc::new(runtime::Runtime::default()));
    ddnative_host::install();
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    let mut c = Cpu::default();
    m.set_u32(STACK, 0xffff0000);
    m.set_u32(STACK + 4, OPERAND);
    let mut samples = [Vec::new(), Vec::new()];
    for pair in 0..12 {
        for k in 0..2 {
            let candidate = (pair + k) % 2;
            let f = ddruntime::win32::interlocked_increment;
            r.replace(SERVICE, if candidate == 1 { Binding::Integer(f) } else { Binding::LockFree(f) });
            x86::ldmxcsr(0x1fa0); // guest sticky precision flag
            let start = std::time::Instant::now();
            for _ in 0..20_000 {
                c.st.register::<0xe8d9>(); // fld1
                c.esp = STACK;
                r.call(&mut m, &mut c, SERVICE).unwrap();
                c.st.pop();
            }
            let elapsed = start.elapsed();
            x87::park();
            x86::ldmxcsr(x86::MXCSR_DEFAULT);
            if pair > 0 { samples[candidate].push(elapsed.as_nanos() as f64 / 20_000.0); }
        }
    }
    let mut ratios: Vec<_> = samples[0].iter().zip(&samples[1]).map(|(a,b)| b/a).collect();
    ratios.sort_by(f64::total_cmp);
    for s in &mut samples { s.sort_by(f64::total_cmp); }
    eprintln!("L135 ns/call baseline min={:.2} median={:.2}; integer min={:.2} median={:.2}; paired median ratio={:.4}", samples[0][0], samples[0][5], samples[1][0], samples[1][5], ratios[5]);
    assert_eq!(m.u32(OPERAND), 480_000);
}

