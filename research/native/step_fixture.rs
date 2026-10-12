// Authored assertions; test_steps.py appends production-emitted instructions.
#![allow(unused_imports, dead_code, unused_parens)]
use ddnative_base::{
    execution::Flow,
    mem::Mem,
    x86::{self, Cpu, Flags},
};
use ddruntime::{
    dispatch::{Binding, Registry},
    memory::{GuestMemory, READ, WRITE},
    seh, services,
    session::{Outcome, Session, STACK_SIZE, STACK_TOP},
};
use std::sync::Arc;
const ROOT: u32 = STACK_TOP - 0x104;
const HANDLER: u32 = 0x7000;
fn machine(handler: x86::LiftFn, service: x86::LiftFn) -> Session {
    ddruntime::runtime::enter(Arc::new(ddruntime::runtime::Runtime::default()));
    let mut mem = GuestMemory::new();
    mem.map(0x10000, 0x2000, READ | WRITE).unwrap();
    mem.map_lazy(STACK_TOP - STACK_SIZE, STACK_SIZE, READ | WRITE)
        .unwrap();
    let fs = seh::initialize_teb(
        &mut mem,
        services::MAIN_THREAD_ID,
        STACK_TOP - STACK_SIZE,
        STACK_TOP,
    );
    mem.set_u32(fs, STACK_TOP - 0x80);
    mem.set_u32(STACK_TOP - 0x80, seh::END);
    mem.set_u32(STACK_TOP - 0x7c, HANDLER);
    let mut r = Registry::new();
    r.set_instruction_lookup(lookup);
    r.insert(HANDLER, Binding::Service(handler)).unwrap();
    r.insert(0x9000, Binding::Service(service)).unwrap();
    r.insert(0x9100, Binding::Service(seh::debug_break)).unwrap();
    r.insert(0x9200, Binding::Service(inner_service)).unwrap();
    let registry = Arc::new(r);
    registry.install();
    Session {
        mem,
        registry,
        imports: vec![],
        exe_path: String::new(),
    }
}
fn returned(s: &mut Session, ip: u32) -> Cpu {
    match s.call(ip, 0, &[]) {
        Outcome::Returned(c) => c,
        x => panic!("{x:?}"),
    }
}
fn unused(_: &mut dyn Mem, _: &mut Cpu) {
    panic!("unexpected call")
}
fn context(m: &dyn Mem, c: &Cpu) -> u32 {
    m.u32(c.esp + 12)
}
fn handled(c: &mut Cpu) {
    c.eax = 0;
    c.esp += 4;
}

#[test]
fn emitted_divide_matches_windows_context_and_modified_resume() {
    let mut s = machine(
        |m, c| {
            let r = m.u32(c.esp + 4);
            let p = context(m, c);
            assert_eq!(m.u32(r), 0xc0000094);
            assert_eq!(m.u32(p + 184), 0x40000a);
            assert_eq!([m.u32(p + 176), m.u32(p + 172), m.u32(p + 168)], [1, 0, 0]);
            assert_eq!(m.u32(p + 192) & 0xcc5, 0x444);
            assert!(!c.df);
            m.set_u32(p + 176, 42);
            m.set_u32(p + 184, 0x40000c);
            handled(c);
        },
        unused,
    );
    let c = returned(&mut s, 0x400000);
    assert_eq!(c.eax, 42);
    assert!(!c.df);
    assert_eq!(c.esp, ROOT + 4);
}

#[test]
fn emitted_access_retries_exact_mov_with_repaired_register() {
    let mut s = machine(
        |m, c| {
            let r = m.u32(c.esp + 4);
            let p = context(m, c);
            assert_eq!(m.u32(r), 0xc0000005);
            assert_eq!(m.u32(r + 24), 0xdead);
            assert_eq!(m.u32(p + 184), 0x401005);
            m.set_u32(p + 176, 0x11000);
            handled(c);
        },
        unused,
    );
    returned(&mut s, 0x401000);
    assert_eq!(s.mem.u32(0x11000), 1);
}

fn abandon_to_outer(m: &mut dyn Mem, c: &mut Cpu) {
    let p = context(m, c);
    m.set_u32(p + 196, ROOT);
    m.set_u32(p + 184, 0x40200c);
    m.set_u32(p + 176, 77);
    handled(c);
}
fn inner_service(m: &mut dyn Mem, c: &mut Cpu) {
    services::call_guest(m, c, 0x403000, &[123]);
    panic!("abandoned inner service continued");
}
#[test]
fn emitted_fault_escapes_two_services_and_callbacks_to_outer_body() {
    let mut s = machine(abandon_to_outer, |m, c| {
        services::call_guest(m, c, 0x408000, &[456]);
        panic!("abandoned outer service continued");
    });
    let c = returned(&mut s, 0x402000);
    assert_eq!(c.eax, 77);
    assert_eq!(c.esp, ROOT + 4);
    assert_eq!(s.mem.u32(seh::TEB_BASE), STACK_TOP - 0x80);
}

#[test]
fn emitted_software_raise_crosses_callback_service_boundary() {
    let mut s = machine(abandon_to_outer, |m, c| {
        services::call_guest(m, c, 0x407000, &[]);
        panic!("abandoned software-raise service continued");
    });
    assert_eq!(returned(&mut s, 0x402000).eax, 77);
}

#[test]
fn emitted_normal_callback_returns_to_its_service() {
    let mut s = machine(unused, |m, c| {
        assert_eq!(services::call_guest(m, c, 0x409000, &[42]), 42);
        assert_eq!(services::call_guest(m, c, 0x409000, &[43]), 43);
        c.esp += 4;
    });
    assert_eq!(returned(&mut s, 0x402000).eax, 0xeeeeeeee);
}

#[test]
fn emitted_rep_commits_only_completed_iterations() {
    let mut s = machine(
        |m, c| {
            let p = context(m, c);
            assert_eq!(m.u32(p + 184), 0x40400f);
            assert_eq!(m.u32(p + 172), 1);
            assert_eq!(m.u32(p + 156), 0x12000);
            assert_eq!(m.u32(0x11ffc), 0x12345678);
            m.set_u32(p + 156, 0x11000);
            handled(c);
        },
        unused,
    );
    let c = returned(&mut s, 0x404000);
    assert_eq!(c.ecx, 0);
    assert_eq!(c.edi, 0x11004);
    assert_eq!(s.mem.u32(0x11000), 0x12345678);
}

#[test]
fn emitted_call_reads_target_before_pushing() {
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x1000, READ | WRITE).unwrap();
    m.set_u32(0x10800, 0x12345678);
    let mut c = Cpu {
        esp: 0x10800,
        ..Cpu::default()
    };
    let f = lookup(0x405000).unwrap();
    assert_eq!(
        ddruntime::executor::instruction(&mut m, &mut c, 0x405000, |m, c| f(m, c, 0x405000)).unwrap(),
        Flow::Next(0x12345678)
    );
    assert_eq!(c.esp, 0x107fc);
    assert_eq!(m.u32(c.esp), 0x405003);
    assert!(lookup(0x405001).is_none());
}

#[test]
fn emitted_split_page_store_matches_windows_atomicity() {
    let mut s = machine(
        |m, c| {
            let r = m.u32(c.esp + 4);
            let p = context(m, c);
            assert_eq!(m.u32(r + 24), 0x12000);
            assert_eq!(m.u16(0x11ffe), 0x1234);
            m.set_u32(p + 184, 0x40600b);
            handled(c);
        },
        unused,
    );
    s.mem.set_u16(0x11ffe, 0x1234);
    returned(&mut s, 0x406000);
    assert_eq!(s.mem.u16(0x11ffe), 0x1234);
}

#[test]
fn emitted_cdecl_handler_reads_context_through_checked_adapter() {
    let mut s = machine(unused, unused);
    s.mem.set_u32(STACK_TOP - 0x7c, 0x410000);
    returned(&mut s, 0x401000);
    assert_eq!(s.mem.u32(0x11000), 1);
}

#[test]
fn emitted_nonreturning_handler_restores_stack_and_jumps_to_body() {
    let mut s = machine(unused, |m, c| {
        services::call_guest(m, c, 0x403000, &[123]);
        panic!("service continued after nonreturning handler");
    });
    s.mem.set_u32(STACK_TOP - 0x7c, 0x411000);
    assert_eq!(returned(&mut s, 0x402000).eax, 77);
    assert_eq!(s.mem.u32(seh::TEB_BASE), STACK_TOP - 0x80);
}

#[test]
fn emitted_naked_epilogues_do_not_abandon_the_service() {
    let mut s = machine(unused, |m, c| {
        services::call_guest(m, c, 0x412000, &[]);
        services::call_guest(m, c, 0x413000, &[]);
        c.esp += 4;
    });
    assert_eq!(returned(&mut s, 0x402000).eax, 0xeeeeeeee);
}

#[test]
#[cfg(feature = "original-fixture")]
fn original_eh4_transfer_and_notification_reach_outer_body() {
    // Actual bodies 0x01303CB9 / 0x013070E5 (DDDA.exe 2017-12-13 Steam)
    // are emitted at test time from the player's private analysis image.
    let mut s = machine(
        |m, c| {
            let target = m.u32(c.esp + 8);
            let before = c.esp;
            c.esp -= 20;
            for (k, v) in [0xffffabd0, target, 0, 0, 0].into_iter().enumerate() {
                m.set_u32(c.esp + 4 * k as u32, v);
            }
            seh::rtl_unwind(m, c);
            assert_eq!(c.esp, before);
            assert_eq!(m.u32(c.fs_base), target);
            c.ecx = 0x415000;
            c.edx = ROOT;
            c.esp -= 4;
            m.set_u32(c.esp, 0xffffabc0);
            x86::call_ext(m, c, 0x01303cb9);
            panic!("original nonreturning transfer returned to handler");
        },
        |m, c| {
            services::call_guest(m, c, 0x403000, &[123]);
            panic!("original transfer returned to abandoned service");
        },
    );
    s.mem.map(0x0163a000, 0x1000, READ | WRITE).unwrap();
    assert_eq!(returned(&mut s, 0x402000).eax, 77);
    assert_eq!(s.mem.u32(0x0163acd4), 0x415000);
    assert_eq!(s.mem.u32(0x0163acd8), 1);
    assert_eq!(s.mem.u32(0x0163acdc), ROOT);
    assert_eq!(s.mem.u32(seh::TEB_BASE), STACK_TOP - 0x80);
}
