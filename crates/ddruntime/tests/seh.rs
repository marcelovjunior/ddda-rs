//! The handler decisions mirror research/runtime/seh_oracle.c. Pointer
//! values are guest-specific; record/layout/order/flags are compared.
use ddnative_base::{
    mem::Mem,
    x86::{self, Cpu},
};
use ddruntime::{
    memory::{GuestMemory, READ, WRITE},
    seh,
};
use std::{cell::RefCell, rc::Rc};

fn fixture() -> (GuestMemory, Cpu) {
    ddruntime::runtime::enter(std::sync::Arc::new(ddruntime::runtime::Runtime::default()));
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    let fs = seh::initialize_teb(&mut m, ddruntime::services::MAIN_THREAD_ID, 0x10000, 0x20000);
    m.set_u32(fs, 0x1f000);
    m.set_u32(0x1f000, 0x1f100);
    m.set_u32(0x1f004, 1);
    m.set_u32(0x1f100, seh::END);
    m.set_u32(0x1f104, 2);
    (
        m,
        Cpu {
            esp: 0x1e000,
            fs_base: fs,
            eax: 0x12345678,
            ebx: 0xaabbccdd,
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

#[test]
fn windows_layout_order_and_modified_context() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
        let rp = m.u32(c.esp + 4);
        let cp = m.u32(c.esp + 12);
        assert_eq!(m.u32(rp), 0xe0420001);
        assert_eq!(m.u32(cp + 176), 0x12345678);
        assert_eq!(m.u32(cp + 164), 0xaabbccdd);
        assert_eq!(m.u32(cp + 184), 0x401234);
        assert_eq!(m.u32(cp + 196), 0x1e000);
        assert_eq!(m.u32(cp), 0x10003);
        log.borrow_mut().push((t, m.u32(rp + 4)));
        if t == 2 {
            m.set_u32(cp + 176, 0xabcdef01);
            m.set_u32(cp + 184, 0x405678);
        }
        c.eax = if t == 2 { 0 } else { 1 };
        c.esp += 4;
    })));
    assert_eq!(seh::RECORD_SIZE, 80);
    assert_eq!(seh::CONTEXT_SIZE, 716);
    let e = seh::Exception::new(0xe0420001, 0x401234);
    assert_eq!(seh::dispatch(&mut m, &mut c, &e, 0x401234), Ok(0x405678));
    assert_eq!(c.eax, 0xabcdef01);
    assert_eq!(*seen.borrow(), [(1, 0), (2, 0)]);
}

#[test]
fn windows_nested_raise_order_flags_and_null_link() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
        let rp = m.u32(c.esp + 4);
        let code = m.u32(rp);
        log.borrow_mut().push((t, code, m.u32(rp + 4)));
        assert_eq!(m.u32(rp + 8), 0);
        if t == 1 && code == 0xe0420001 {
            let e = seh::Exception::new(0xe0420002, 0x402000);
            assert_eq!(seh::dispatch(m, c, &e, 0x402000), Ok(0x402000));
        }
        c.eax = if t == 2 { 0 } else { 1 };
        c.esp += 4;
    })));
    let e = seh::Exception::new(0xe0420001, 0x401000);
    assert_eq!(seh::dispatch(&mut m, &mut c, &e, 0x401000), Ok(0x401000));
    assert_eq!(
        *seen.borrow(),
        [
            (1, 0xe0420001, 0),
            (1, 0xe0420002, 16),
            (2, 0xe0420002, 0),
            (2, 0xe0420001, 0)
        ]
    );
}

#[test]
fn windows_target_unwind_excludes_target_and_sets_return_value() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
        let rp = m.u32(c.esp + 4);
        let cp = m.u32(c.esp + 12);
        log.borrow_mut()
            .push((t, m.u32(rp), m.u32(rp + 4), m.u32(cp + 176)));
        m.set_u32(cp + 176, 0xbeef);
        c.eax = 1;
        c.esp += 4;
    })));
    for (i, v) in [0x401234, 0x1f100, 0, 0, 0x1234].into_iter().enumerate() {
        m.set_u32(c.esp + 4 * i as u32, v);
    }
    seh::rtl_unwind(&mut m, &mut c);
    assert_eq!(c.eax, 0xbeef);
    assert_eq!(c.esp, 0x1e014);
    assert_eq!(m.u32(c.fs_base), 0x1f100);
    assert_eq!(*seen.borrow(), [(1, 0xc0000027, 2, 0x1234)]);
}

#[test]
fn rejects_invalid_chain_and_noncontinuable_resume() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    x86::set_ext_calls(Some(Box::new(|_, c, _| {
        c.eax = 0;
        c.esp += 4;
    })));
    let mut e = seh::Exception::new(0xdeadbeef, 0x401000);
    e.flags = seh::NONCONTINUABLE;
    assert_eq!(
        seh::dispatch(&mut m, &mut c, &e, 0x401000),
        Err(seh::Stop::Noncontinuable { code: 0xdeadbeef })
    );
    m.set_u32(c.fs_base, 0x1ffff);
    assert!(matches!(
        seh::dispatch(&mut m, &mut c, &e, 0x401000),
        Err(seh::Stop::InvalidStack { .. })
    ));
    m.set_u32(c.fs_base, 0x1f000);
    m.set_u32(0x1f000, 0x1f000);
    x86::set_ext_calls(Some(Box::new(|_, c, _| {
        c.eax = 1;
        c.esp += 4;
    })));
    assert!(matches!(
        seh::dispatch(&mut m, &mut c, &e, 0x401000),
        Err(seh::Stop::InvalidStack { .. })
    ));
}

#[test]
fn unsupported_resume_stops_instead_of_returning_to_rust_caller() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    x86::set_ext_calls(Some(Box::new(|m, c, _| {
        let cp = m.u32(c.esp + 12);
        m.set_u32(cp + 184, 0x401999);
        c.eax = 0;
        c.esp += 4;
    })));
    for (i, v) in [0x401234, 0xe0420001, 0, 0, 0].into_iter().enumerate() {
        m.set_u32(c.esp + 4 * i as u32, v);
    }
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        seh::raise_exception(&mut m, &mut c)
    }))
    .unwrap_err();
    assert!(matches!(
        e.downcast_ref::<seh::Stop>(),
        Some(seh::Stop::UnsupportedResume {
            address: 0x401999,
            ..
        })
    ));
}

#[test]
fn exception_parameters_and_exact_fault_checkpoint() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    c.eax = 1;
    c.edx = 0;
    c.ecx = 0;
    c.df = true;
    c.fl = x86::Flags::new(x86::LOGIC, 0, 0, 0, 32); // oracle XOR ECX,ECX before DIV
    x86::set_ext_calls(Some(Box::new(|m, c, _| {
        let rp = m.u32(c.esp + 4);
        let cp = m.u32(c.esp + 12);
        assert_eq!(m.u32(rp), 0xc0000094);
        assert!(!c.df, "Windows clears DF before entering the handler");
        assert_eq!(m.u32(cp + 192) & 0x400, 0x400);
        assert_eq!(m.u32(cp + 192) & 0x8c5, 0x44);
        m.set_u32(cp + 192, m.u32(cp + 192) & !0x400);
        assert_eq!(m.u32(cp + 184), 0x404000);
        assert_eq!((m.u32(cp + 176), m.u32(cp + 172), m.u32(cp + 168)), (1, 0, 0));
        m.set_u32(cp + 184, 0x404002);
        m.set_u32(cp + 176, 42);
        c.eax = 0;
        c.esp += 4;
    })));
    assert_eq!(
        seh::dispatch(
            &mut m,
            &mut c,
            &seh::Exception::new(0xc0000094, 0x404000),
            0x404000
        ),
        Ok(0x404002)
    );
    assert_eq!(c.eax, 42);
    assert!(!c.df);
    let fault = ddruntime::memory::Fault {
        addr: 0xdead,
        width: 4,
        write: true,
        reason: "unmapped page",
    };
    let av = seh::Exception::access_violation(&fault, 0x400010);
    assert_eq!(
        (av.code, av.address, av.parameters.clone()),
        (0xc0000005, 0x400010, vec![1, 0xdead])
    );
    x86::set_ext_calls(Some(Box::new(|m, c, _| {
        let rp = m.u32(c.esp + 4);
        let cp = m.u32(c.esp + 12);
        assert_eq!(
            (m.u32(rp), m.u32(rp + 12), m.u32(rp + 16)),
            (0xc0000005, 0x400010, 2)
        );
        assert_eq!((m.u32(rp + 20), m.u32(rp + 24)), (1, 0xdead));
        m.set_u32(cp + 184, 0x400016);
        c.eax = 0;
        c.esp += 4;
    })));
    assert_eq!(seh::dispatch(&mut m, &mut c, &av, 0x400010), Ok(0x400016));
}

#[test]
fn invalid_disposition_and_wrong_handler_abi_are_explicit() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    let e = seh::Exception::new(1, 2);
    x86::set_ext_calls(Some(Box::new(|_, c, _| {
        c.eax = 99;
        c.esp += 4;
    })));
    assert_eq!(
        seh::dispatch(&mut m, &mut c, &e, 2),
        Err(seh::Stop::InvalidDisposition {
            handler: 1,
            value: 99
        })
    );
    assert_eq!(m.u32(c.fs_base), 0x1f000); // the nested guard was removed
    x86::set_ext_calls(Some(Box::new(|_, c, _| {
        c.eax = 0;
        c.esp += 20;
    })));
    assert!(matches!(
        seh::dispatch(&mut m, &mut c, &e, 2),
        Err(seh::Stop::BadHandlerReturn { .. })
    ));
}

#[test]
fn teb_is_per_thread_and_stack_checked_before_dispatch() {
    let (mut m, mut c) = fixture();
    let second = seh::initialize_teb(&mut m, ddruntime::services::MAIN_THREAD_ID + 4, 0x30000, 0x40000);
    assert_ne!(second, c.fs_base);
    assert_eq!(m.u32(second), seh::END);
    assert_eq!(m.u32(second + 24), second);
    assert_eq!(m.u32(second + 8), 0x30000);
    c.esp = 0x10010;
    assert!(matches!(
        seh::dispatch(&mut m, &mut c, &seh::Exception::new(1, 2), 2),
        Err(seh::Stop::InvalidStack { .. })
    ));
}

#[test]
fn unhandled_filter_calls_registered_stdcall_guest_filter() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    ddruntime::runtime::enter(std::sync::Arc::new(ddruntime::runtime::Runtime::default()));
    m.set_u32(c.esp, 0x401000);
    m.set_u32(c.esp + 4, 0x1234);
    ddruntime::services::set_unhandled_exception_filter(&mut m, &mut c);
    c.esp = 0x1e000;
    m.set_u32(c.esp + 4, 0x1d000);
    x86::set_ext_calls(Some(Box::new(|m, c, t| {
        assert_eq!(t, 0x1234);
        assert_eq!(m.u32(c.esp + 4), 0x1d000);
        c.eax = u32::MAX;
        c.esp += 8;
    })));
    seh::unhandled_exception_filter(&mut m, &mut c);
    assert_eq!(c.eax, u32::MAX);
    assert_eq!(c.esp, 0x1e008);
}

#[cfg(feature = "game")]
#[test]
fn original_except_handler4_calls_filter_with_establisher_frame() {
    // The fixture layout follows 0x012F7240: encoded scope-table pointer
    // at frame+8, try level +12, EBP=frame+16, exception pointers at EBP-20.
    // The handler, cookie check and filter-call helper are actual lifts.
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    let cookie = 0x12345678;
    let table = 0x11000;
    let frame = 0x1f000;
    let filter = 0x70000001;
    m.map(0x01639000, 0x1000, READ | WRITE).unwrap();
    m.set_u32(0x01639844, cookie);
    m.set_u32(frame, seh::END);
    m.set_u32(frame + 4, 0x012f7240);
    m.set_u32(frame + 8, table ^ cookie);
    m.set_u32(frame + 12, 0);
    m.set_u32(frame + 16, cookie ^ (frame + 16));
    for (i, value) in [0xfffffffe, 0, 0, 0, 0xfffffffe, filter, 0]
        .into_iter()
        .enumerate()
    {
        m.set_u32(table + i as u32 * 4, value);
    }
    let registry = ddruntime::dispatch::Registry::with_lifted_program();
    x86::set_ext_calls(Some(Box::new(move |m, c, target| {
        if target == filter {
            assert_eq!(c.ebp, frame + 16);
            let pointers = m.u32(c.ebp - 20);
            let record = m.u32(pointers);
            let context = m.u32(pointers + 4);
            assert_eq!(m.u32(record), 0xe0420001);
            m.set_u32(context + 176, 0x55aa);
            c.eax = u32::MAX; // EXCEPTION_CONTINUE_EXECUTION (filter result)
            c.esp += 4;
        } else {
            registry.call(m, c, target).unwrap();
        }
    })));
    assert_eq!(
        seh::dispatch(
            &mut m,
            &mut c,
            &seh::Exception::new(0xe0420001, 0x401000),
            0x401000
        ),
        Ok(0x401000)
    );
    assert_eq!(c.eax, 0x55aa);
}

#[test]
fn software_raises_continue_at_known_api_boundary() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    x86::set_ext_calls(Some(Box::new(|m, c, _| {
        let rp = m.u32(c.esp + 4);
        let cp = m.u32(c.esp + 12);
        match m.u32(rp) {
            0xe0420000 => {
                assert_eq!(m.u32(rp + 16), 1);
                assert_eq!(m.u32(rp + 20), 0x12345678);
            }
            0x80000003 => assert_eq!(m.u32(rp + 16), 0),
            code => panic!("unexpected exception {code:#x}"),
        }
        m.set_u32(cp + 176, 42);
        c.eax = 0;
        c.esp += 4;
    })));
    m.set_u32(0x12000, 0x12345678);
    for (i, v) in [0x401000, 0xe0420000, 0, 1, 0x12000].into_iter().enumerate() {
        m.set_u32(c.esp + 4 * i as u32, v);
    }
    seh::raise_exception(&mut m, &mut c);
    assert_eq!((c.eax, c.esp), (42, 0x1e014));
    m.set_u32(c.esp, 0x401123);
    seh::debug_break(&mut m, &mut c);
    assert_eq!((c.eax, c.esp), (42, 0x1e018));
}

#[test]
fn windows_new_handler_precedes_nested_guard() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
        let rp = m.u32(c.esp + 4);
        let code = m.u32(rp);
        log.borrow_mut().push((t, code, m.u32(rp + 4)));
        if t == 1 && code == 0xe0420001 {
            let old = m.u32(c.fs_base);
            let node = c.esp - 32;
            m.set_u32(node, old);
            m.set_u32(node + 4, 3);
            m.set_u32(c.fs_base, node);
            let mut child = c.clone();
            child.esp = node - 16;
            assert_eq!(
                seh::dispatch(
                    m,
                    &mut child,
                    &seh::Exception::new(0xe0420002, 0x402000),
                    0x402000
                ),
                Ok(0x402000)
            );
            m.set_u32(c.fs_base, old);
        }
        c.eax = if t == 2 { 0 } else { 1 };
        c.esp += 4;
    })));
    assert_eq!(
        seh::dispatch(
            &mut m,
            &mut c,
            &seh::Exception::new(0xe0420001, 0x401000),
            0x401000
        ),
        Ok(0x401000)
    );
    assert_eq!(
        *seen.borrow(),
        [
            (1, 0xe0420001, 0),
            (3, 0xe0420002, 0),
            (1, 0xe0420002, 16),
            (2, 0xe0420002, 0),
            (2, 0xe0420001, 0)
        ]
    );
}

#[test]
fn windows_collided_unwind_skips_completed_registration() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    m.set_u32(0x1f100, 0x1f200);
    m.set_u32(0x1f200, seh::END);
    m.set_u32(0x1f204, 3);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let log = calls.clone();
    x86::set_ext_calls(Some(Box::new(move |m, c, t| {
        log.borrow_mut().push(t);
        let rp = m.u32(c.esp + 4);
        let cp = m.u32(c.esp + 12);
        let dp = m.u32(c.esp + 16);
        assert_eq!(
            (m.u32(rp), m.u32(rp + 4), m.u32(cp + 176)),
            (0xc0000027, 2, 0x1234)
        );
        m.set_u32(cp + 176, 0xbeef);
        m.set_u32(dp, 0x1f100);
        c.eax = 3;
        c.esp += 4;
    })));
    for (i, v) in [0x401000, 0x1f200, 0x12345678, 0, 0x1234].into_iter().enumerate() {
        m.set_u32(c.esp + 4 * i as u32, v);
    }
    seh::rtl_unwind(&mut m, &mut c);
    assert_eq!(*calls.borrow(), [1]);
    assert_eq!(m.u32(c.fs_base), 0x1f200);
    assert_eq!((c.eax, c.esp), (0xbeef, 0x1e014));
}

#[test]
fn unavailable_context_groups_stop_and_restore_registration_chain() {
    let _reset = Reset;
    let (mut m, mut c) = fixture();
    x86::set_ext_calls(Some(Box::new(|m, c, _| {
        let cp = m.u32(c.esp + 12);
        let _ = m.u32(cp + 28); // FLOATING_SAVE_AREA, not advertised by ContextFlags
    })));
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        seh::dispatch(&mut m, &mut c, &seh::Exception::new(1, 2), 2)
    }))
    .unwrap_err();
    assert!(matches!(
        failure.downcast_ref::<seh::Stop>(),
        Some(seh::Stop::UnavailableContextRead { offset: 28, .. })
    ));
    assert_eq!(m.u32(c.fs_base), 0x1f000);
}
