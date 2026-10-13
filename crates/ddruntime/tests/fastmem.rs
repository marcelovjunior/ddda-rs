//! L61: the page-wise memcpy/memset services equal ddnative-base's
//! byte-wise models (overlap, page crossings, lazy pages).
use ddnative_base::mem::{Mem, READ, WRITE};
use ddnative_base::x86::Cpu;
use ddruntime::memory::GuestMemory;

const STACK: u32 = 0x0080_0000;

fn setup() -> GuestMemory {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x6000, READ | WRITE).unwrap();
    m.map_lazy(0x0020_0000, 0x3000, READ | WRITE).unwrap();
    m.map(STACK - 0x1000, 0x1000, READ | WRITE).unwrap();
    for k in 0..0x6000u32 {
        m.set_u8(0x0010_0000 + k, (k.wrapping_mul(31) ^ (k >> 7)) as u8);
    }
    m
}

fn call(m: &mut GuestMemory, f: fn(&mut dyn Mem, &mut Cpu), args: [u32; 3]) -> Cpu {
    let mut cpu = Cpu { esp: STACK - 0x100, ..Cpu::default() };
    for a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, *a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x1234_5678);
    f(m, &mut cpu);
    cpu
}

fn snapshot(m: &GuestMemory) -> Vec<u8> {
    let mut v = vec![0u8; 0x6000 + 0x3000];
    m.read_bytes(0x0010_0000, &mut v[..0x6000]);
    m.read_bytes(0x0020_0000, &mut v[0x6000..]);
    v
}

#[test]
fn bulk_memcpy_and_memset_equal_the_byte_wise_models() {
    let cases: [(fn(&mut dyn Mem, &mut Cpu), fn(&mut dyn Mem, &mut Cpu), [u32; 3]); 15] = [
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_3000, 0x0010_0100, 0x2345]), // page crossing
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_0200, 0x0010_0100, 0x1800]), // overlap, dst > src
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_0100, 0x0010_0200, 0x1800]), // overlap, dst < src
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0020_0FF0, 0x0010_0000, 0x1020]), // into lazy pages
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_4000, 0x0020_1000, 0x800]),  // from a lazy page
        (ddruntime::fastmem::memset, ddnative_base::platform::memset, [0x0010_0FFE, 0x1A5, 0x2003]),
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_0000, 0x0010_0000, 0]),
        // L170: the stack buffer's sizes (up to 256 bytes) and its edge
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_0FF0, 0x0010_2FE0, 0x40]),  // both crossing pages
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_0110, 0x0010_0100, 100]),   // overlap, dst > src
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_0100, 0x0010_0110, 100]),   // overlap, dst < src
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0020_0FC0, 0x0010_0000, 256]),   // into lazy pages
        (ddruntime::fastmem::memcpy, ddnative_base::platform::memcpy, [0x0010_4000, 0x0010_0003, 257]),
        (ddruntime::fastmem::memset, ddnative_base::platform::memset, [0x0010_0FF0, 0x7F, 0x40]),
        (ddruntime::fastmem::memset, ddnative_base::platform::memset, [0x0020_1F00, 0x1C3, 256]),
        (ddruntime::fastmem::memset, ddnative_base::platform::memset, [0x0010_0001, 0x5A, 1]),
    ];
    for (fast, model, args) in cases {
        let (mut a, mut b) = (setup(), setup());
        let ca = call(&mut a, fast, args);
        let cb = call(&mut b, model, args);
        assert_eq!((ca.eax, ca.esp), (cb.eax, cb.esp), "{args:x?}");
        assert!(snapshot(&a) == snapshot(&b), "memory differs for {args:x?}");
    }
}
