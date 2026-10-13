//! L170: the CRT memory and string services on the Integer path (session.rs)
//! leave the complete guest FP state as it was, as L135's audited imports
//! do: a live x87 stack and every MXCSR mode, through the dispatch.
#![cfg(all(windows, target_arch = "x86_64"))]
use ddnative_base::{mem::Mem, x86::{self, Cpu}, x87};
use ddruntime::{crtfast, dispatch::{Binding, Registry}, fastmem, memory::{GuestMemory, READ, WRITE}, runtime};
use std::sync::Arc;

const STACK: u32 = 0x18000;
const DATA: u32 = 0x19000;
const OUT: u32 = 0x1A000;
const SERVICE: u32 = 0x7000_0000;

struct Restore(u32);
impl Drop for Restore {
    fn drop(&mut self) {
        x87::park();
        x86::ldmxcsr(self.0);
    }
}

#[test]
fn crt_services_on_the_integer_path_preserve_the_guest_fp_state() {
    let _restore = Restore(x86::stmxcsr());
    runtime::enter(Arc::new(runtime::Runtime::default()));
    ddnative_host::install();
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x10000, READ | WRITE).unwrap();
    // "hello, world" and its UTF-16 form
    for (k, b) in b"hello, world\0".iter().enumerate() {
        m.set_u8(DATA + k as u32, *b);
        m.set_u16(DATA + 0x100 + 2 * k as u32, *b as u16);
    }
    type F = fn(&mut dyn Mem, &mut Cpu);
    let cases: [(&str, F, [u32; 3], u32); 6] = [
        ("memcpy", fastmem::memcpy, [OUT, DATA, 13], OUT),
        ("memset", fastmem::memset, [OUT + 0x40, 0x5A, 300], OUT + 0x40),
        ("strlen", crtfast::SERVICES.iter().find(|s| s.0 == crtfast::STRLEN).unwrap().1, [DATA, 0, 0], 12),
        ("wcslen", crtfast::SERVICES.iter().find(|s| s.0 == crtfast::WCSLEN).unwrap().1, [DATA + 0x100, 0, 0], 12),
        ("memchr", crtfast::SERVICES.iter().find(|s| s.0 == crtfast::MEMCHR).unwrap().1, [DATA, b'w' as u32, 13], DATA + 7),
        ("memmove", crtfast::SERVICES.iter().find(|s| s.0 == crtfast::MEMMOVE).unwrap().1, [OUT + 0x200, OUT, 13], OUT + 0x200),
    ];
    for (name, f, args, expected) in cases {
        let mut r = Registry::new();
        r.insert(SERVICE, Binding::Integer(f)).unwrap();
        for depth in [0, 3, 8] {
            for mode in 0..4u32 {
                x87::park();
                let mut c = Cpu { esp: STACK, ..Cpu::default() };
                let mut cw = [0u8; 10];
                cw[..2].copy_from_slice(&(0x037fu16 | (mode as u16) << 10).to_le_bytes());
                c.st.exec::<0x28d9>(&mut cw);
                for _ in 0..depth {
                    c.st.register::<0xebd9>(); // fldpi
                }
                let before = c.st.clone().state;
                let live = x87::live_id();
                m.set_u32(STACK, 0xffff_0000);
                for (k, a) in args.iter().enumerate() {
                    m.set_u32(STACK + 4 + 4 * k as u32, *a);
                }
                // all sticky flags, FTZ and DAZ, each rounding mode
                let mxcsr = 0x9fffu32 | mode << 13;
                x86::ldmxcsr(mxcsr);
                r.call(&mut m, &mut c, SERVICE).unwrap();
                assert_eq!(x86::stmxcsr(), mxcsr, "{name}");
                assert_eq!(x87::live_id(), live, "{name}: parked");
                assert_eq!(c.st.clone().state, before, "{name}: changed x87");
                assert_eq!((c.eax, c.esp), (expected, STACK + 4), "{name}");
            }
        }
    }
    let mut copied = [0u8; 13];
    for at in [OUT, OUT + 0x200] {
        m.read_bytes(at, &mut copied);
        assert_eq!(&copied, b"hello, world\0");
    }
    assert!((0..300).all(|k| m.u8(OUT + 0x40 + k) == 0x5A));
}
