//! Instruction bodies here are authored fixtures matching seh_oracle.c;
//! they are not generated game lifts. This tests the L27b producer contract.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};
use ddruntime::{
    executor::{self, Executor, Flow, Stop},
    memory::{GuestMemory, READ, WRITE},
    seh,
};

const RETURN: u32 = 0xffff_0000;
const HANDLER: u32 = 0x7000;
fn fixture() -> (GuestMemory, Cpu) {
    ddruntime::runtime::enter(std::sync::Arc::new(ddruntime::runtime::Runtime::default()));
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    let fs = seh::initialize_teb(&mut m, ddruntime::services::MAIN_THREAD_ID, 0x10000, 0x20000);
    m.set_u32(fs, 0x1f000);
    m.set_u32(0x1f000, seh::END);
    m.set_u32(0x1f004, HANDLER);
    m.set_u32(0x1e000, RETURN);
    (
        m,
        Cpu {
            esp: 0x1e000,
            fs_base: fs,
            ..Cpu::default()
        },
    )
}
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        x86::set_ext_calls(None);
    }
}
fn handler(f: impl Fn(&mut dyn Mem, &mut Cpu, u32, u32) + 'static) -> Reset {
    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
        assert_eq!(t, HANDLER);
        let record = m.u32(c.esp + 4);
        let context = m.u32(c.esp + 12);
        f(m, c, record, context);
        c.eax = 0; // ExceptionContinueExecution
        c.esp += 4; // cdecl handler
    })));
    Reset
}
fn ret(m: &mut dyn Mem, c: &mut Cpu) -> Flow {
    let target = m.u32(c.esp);
    c.esp += 4;
    Flow::Return(target)
}

#[test]
fn windows_divide_context_and_modified_interior_resume() {
    let (mut m, mut c) = fixture();
    c.eax = 1;
    c.df = true;
    c.fl = x86::Flags::new(x86::LOGIC, 0, 0, 0, 32);
    let _reset = handler(|m, hc, r, p| {
        assert_eq!(m.u32(r), 0xc0000094);
        assert_eq!(m.u32(r + 12), 0x4000);
        assert_eq!(m.u32(p + 184), 0x4000);
        assert_eq!([m.u32(p + 176), m.u32(p + 172), m.u32(p + 168)], [1, 0, 0]);
        assert_eq!(m.u32(p + 192) & 0x8c5, 0x44);
        assert_ne!(m.u32(p + 192) & 0x400, 0);
        assert!(!hc.df);
        m.set_u32(p + 176, 42);
        m.set_u32(p + 184, 0x4002);
        m.set_u32(p + 192, m.u32(p + 192) & !0x400);
    });
    let mut e = Executor::default();
    e.insert(0x4000, |_, c| {
        let (q, r) = executor::divide(c.eax, c.edx, c.ecx, 32, false);
        c.eax = q;
        c.edx = r;
        Flow::Next(0x4002)
    });
    e.insert(0x4002, ret);
    e.run(&mut m, &mut c, 0x4000, RETURN, 3).unwrap();
    assert_eq!(c.eax, 42);
    assert!(!c.df);
    assert_eq!(c.esp, 0x1e004);
}

#[test]
fn windows_access_fault_can_retry_same_instruction_with_changed_register() {
    let (mut m, mut c) = fixture();
    c.eax = 0xdead;
    let _reset = handler(|m, _, r, p| {
        assert_eq!(m.u32(r), 0xc0000005);
        assert_eq!([m.u32(r + 20), m.u32(r + 24)], [1, 0xdead]);
        assert_eq!(m.u32(p + 184), 0x4100);
        m.set_u32(p + 176, 0x11000); // retry MOV [EAX],1 at the same EIP
    });
    let mut e = Executor::default();
    e.insert(0x4100, |m, c| {
        m.set_u32(c.eax, 1);
        Flow::Next(0x4106)
    });
    e.insert(0x4106, ret);
    e.run(&mut m, &mut c, 0x4100, RETURN, 3).unwrap();
    assert_eq!(m.u32(0x11000), 1);
}

#[test]
fn failed_push_and_multiwrite_leave_preinstruction_state() {
    let (mut m, mut c) = fixture();
    c.esp = 0x10002;
    c.xmm[4] = [1, 2, 3, 4];
    let before = format!("{c:?}");
    let fault = executor::instruction(&mut m, &mut c, 0x4200, |m, c| {
        c.esp -= 4;
        c.xmm[4] = [9; 4];
        m.set_u32(0x11000, 99);
        m.set_u32(c.esp, c.eax);
        Flow::Next(0x4201)
    })
    .unwrap_err();
    assert_eq!(fault.address, 0x4200);
    assert_eq!(fault.parameters, vec![1, 0xfffe]);
    assert_eq!(format!("{c:?}"), before);
    assert_eq!(m.u32(0x11000), 0);
}

#[test]
fn split_page_store_does_not_publish_accessible_prefix() {
    let (mut m, mut c) = fixture();
    m.set_u16(0x1fffe, 0x1234);
    let fault = executor::instruction(&mut m, &mut c, 0x4300, |m, _| {
        m.set_u32(0x1fffe, 0xffffffff);
        Flow::Next(0x4301)
    })
    .unwrap_err();
    assert_eq!(fault.parameters, vec![1, 0x20000]);
    assert_eq!(m.u16(0x1fffe), 0x1234);
}

#[test]
fn nonlocal_context_discards_guest_calls_without_returning_to_their_continuations() {
    let (mut m, mut c) = fixture();
    let _reset = handler(|m, _, _, p| {
        assert_eq!(m.u32(p + 196), 0x1dff8); // two nested guest CALLs
        m.set_u32(p + 184, 0x4408); // outer __except-like interior body
        m.set_u32(p + 196, 0x1e000);
        m.set_u32(p + 176, 77);
    });
    let mut e = Executor::default();
    e.insert(0x4400, |m, c| {
        c.esp -= 4;
        m.set_u32(c.esp, 0x4405);
        Flow::Next(0x4500)
    });
    e.insert(0x4500, |m, c| {
        c.esp -= 4;
        m.set_u32(c.esp, 0x4505);
        Flow::Next(0x4600)
    });
    e.insert(0x4600, |m, _| {
        m.u32(0xdead);
        Flow::Next(0x4604)
    });
    e.insert(0x4405, |_, _| panic!("discarded outer continuation"));
    e.insert(0x4505, |_, _| panic!("discarded inner continuation"));
    e.insert(0x4408, ret);
    e.run(&mut m, &mut c, 0x4400, RETURN, 4).unwrap();
    assert_eq!(c.eax, 77);
    assert_eq!(c.esp, 0x1e004);
}

#[test]
fn returns_follow_guest_targets_and_stdcall_cleanup() {
    let (mut m, mut c) = fixture();
    let mut e = Executor::default();
    e.insert(0x4700, |m, c| {
        c.esp -= 4;
        m.set_u32(c.esp, 42);
        c.esp -= 4;
        m.set_u32(c.esp, 0x4705);
        Flow::Next(0x4800)
    });
    e.insert(0x4800, |m, c| {
        c.eax = m.u32(c.esp + 4);
        let t = m.u32(c.esp);
        c.esp += 8;
        Flow::Return(t)
    });
    e.insert(0x4705, ret);
    e.run(&mut m, &mut c, 0x4700, RETURN, 3).unwrap();
    assert_eq!(c.eax, 42);
    assert_eq!(c.esp, 0x1e004);
}

#[test]
fn quotient_overflow_is_guest_de_but_host_panics_are_not() {
    let (mut m, mut c) = fixture();
    c.eax = u32::MAX;
    c.edx = 1;
    let fault = executor::instruction(&mut m, &mut c, 0x4900, |_, c| {
        executor::divide(c.eax, c.edx, 1, 32, false);
        Flow::Next(0)
    })
    .unwrap_err();
    assert_eq!(fault.code, 0xc0000094);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        executor::instruction(&mut m, &mut c, 0x4900, |_, _| panic!("host bug"))
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"host bug"));
}

#[test]
fn missing_interior_entry_stops_at_exact_requested_address() {
    let (mut m, mut c) = fixture();
    let mut e = Executor::default();
    e.insert(0x5000, |_, _| Flow::Next(0x5001));
    assert_eq!(
        e.run(&mut m, &mut c, 0x5000, RETURN, 2),
        Err(Stop::MissingInstruction { ip: 0x5001 })
    );
}

#[test]
fn rep_restart_keeps_completed_iterations_and_faulting_iteration_registers() {
    let (mut m, mut c) = fixture();
    c.ecx = 2;
    c.edi = 0x1fffc;
    c.eax = 0x12345678;
    let _reset = handler(|m, _, r, p| {
        assert_eq!(m.u32(p + 184), 0x5100);
        assert_eq!(m.u32(p + 172), 1);
        assert_eq!(m.u32(p + 156), 0x20000);
        assert_eq!(m.u32(r + 24), 0x20000);
        assert_eq!(m.u32(0x1fffc), 0x12345678);
        m.set_u32(p + 156, 0x11000);
    });
    let mut e = Executor::default();
    e.insert(0x5100, |m, c| {
        // One restart unit of REP STOSD, not the whole REP loop.
        if c.ecx == 0 {
            return Flow::Next(0x5102);
        }
        m.set_u32(c.edi, c.eax);
        c.edi = c.edi.wrapping_add(if c.df { 0xffff_fffc } else { 4 });
        c.ecx -= 1;
        Flow::Next(if c.ecx == 0 { 0x5102 } else { 0x5100 })
    });
    e.insert(0x5102, ret);
    e.run(&mut m, &mut c, 0x5100, RETURN, 4).unwrap();
    assert_eq!(c.ecx, 0);
    assert_eq!(c.edi, 0x11004);
    assert_eq!(m.u32(0x1fffc), 0x12345678);
    assert_eq!(m.u32(0x11000), 0x12345678);
}
