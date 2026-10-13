use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::{fastmem, memory::{GuestMemory, READ, WRITE}};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, time::Instant};
struct Counter;
thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; static BYTES: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ACTIVE.with(|on| if on.get() { BYTES.with(|v| v.set(v.get() + layout.size())) });
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ACTIVE.with(|on| if on.get() { BYTES.with(|v| v.set(v.get() + layout.size())) });
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { unsafe { System.dealloc(p, layout) } }
}
#[global_allocator] static ALLOCATOR: Counter = Counter;

#[test]
fn crt_scratch_byte_identity_and_requested_bytes() {
    let on = std::env::var("DDRUNTIME_CRT_SCRATCH").as_deref() == Ok("1");
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x900000, READ | WRITE).unwrap();
    let source = 0x100000;
    let output = 0x500000;
    for size in [0usize, 1, 4097, 131072, 1382400, (2 << 20) + 1] {
        let data: Vec<_> = (0..size).map(|i| i.wrapping_mul(31) as u8).collect();
        fastmem::write_bulk(&mut m, source, &data);
        for (label, f, input) in [("copy", fastmem::memcpy as fn(&mut dyn Mem, &mut Cpu), source), ("fill", fastmem::memset, 0xa5)] {
            m.set_u32(0x10000, 0x12345678);
            m.set_u32(0x10004, output);
            m.set_u32(0x10008, input);
            m.set_u32(0x1000c, size as u32);
            let call = |m: &mut GuestMemory| {
                let mut cpu = Cpu { esp: 0x10000, ..Cpu::default() };
                f(m, &mut cpu);
                assert_eq!((cpu.eax, cpu.esp), (output, 0x10004));
            };
            call(&mut m); // warm pages and retained storage before accounting
            BYTES.with(|v| v.set(0)); ACTIVE.with(|v| v.set(true));
            let start = Instant::now();
            for _ in 0..20 { call(&mut m); }
            let ns = start.elapsed().as_nanos() / 20;
            ACTIVE.with(|v| v.set(false));
            let requested = BYTES.with(Cell::get);
            // L170: up to 256 bytes on the stack, scratch or not
            let expected = if size <= 256 || (on && (label == "fill" || size <= 2 << 20)) { 0 } else { size * 20 };
            assert_eq!(requested, expected, "{label} size={size}");
            let mut actual = vec![0; size]; m.read_bytes(output, &mut actual);
            if label == "copy" { assert_eq!(actual, data); } else { assert!(actual.iter().all(|&b| b == 0xa5)); }
            eprintln!("scratch={on} operation={label} size={size} ns_per_call={ns} requested_bytes={requested}");
        }
    }
}
