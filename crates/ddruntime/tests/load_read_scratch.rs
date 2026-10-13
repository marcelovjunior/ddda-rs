//! CPU-only synthetic file service measurement; never opens game data.
use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::{
    files,
    memory::{GuestMemory, READ, WRITE},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Instant,
};

struct CountAlloc;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
// SAFETY: every operation delegates the same allocation/layout to System.
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        COUNTING.with(|c| {
            if c.get() {
                ALLOCS.with(|n| n.set(n.get() + 1))
            }
        });
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        COUNTING.with(|c| {
            if c.get() {
                ALLOCS.with(|n| n.set(n.get() + 1))
            }
        });
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        COUNTING.with(|c| {
            if c.get() {
                ALLOCS.with(|n| n.set(n.get() + 1))
            }
        });
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOC: CountAlloc = CountAlloc;

const NAME: u32 = 0x10000;
const OUT: u32 = 0x11000;
const BUF: u32 = 0x20000;
const STACK: u32 = 0x140000;

fn call(m: &mut GuestMemory, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
    let mut cpu = Cpu {
        esp: STACK,
        ..Cpu::default()
    };
    m.set_u32(STACK, 0);
    for (i, &arg) in args.iter().enumerate() {
        m.set_u32(STACK + 4 + i as u32 * 4, arg);
    }
    f(m, &mut cpu);
    cpu.eax
}

struct Fixture {
    root: std::path::PathBuf,
    m: GuestMemory,
    h: u32,
}
impl Fixture {
    fn new(label: &str, size: usize) -> Self {
        let root = std::env::temp_dir().join(format!("ddruntime-L109-{}-{label}", std::process::id()));
        std::fs::create_dir_all(root.join("install")).unwrap();
        std::fs::write(
            root.join("install/input.bin"),
            (0..size).map(|i| (i % 251) as u8).collect::<Vec<_>>(),
        )
        .unwrap();
        files::set_roots(&root.join("install"), &root.join("sandbox"));
        let mut m = GuestMemory::new();
        m.map(NAME, STACK + 4096 - NAME, READ | WRITE).unwrap();
        ddruntime::fastmem::write_bulk(&mut m, NAME, b"input.bin\0");
        let h = call(
            &mut m,
            files::create_file_a,
            &[NAME, 0x80000000, 1, 0, 3, 0x80, 0],
        );
        assert_ne!(h, u32::MAX);
        Self { root, m, h }
    }
    fn seek(&mut self, offset: u32) {
        assert_eq!(
            call(&mut self.m, files::set_file_pointer, &[self.h, offset, 0, 0]),
            offset
        );
    }
    fn read(&mut self, size: u32) -> u32 {
        assert_eq!(
            call(&mut self.m, files::read_file, &[self.h, BUF, size, OUT, 0]),
            1
        );
        self.m.u32(OUT)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        call(&mut self.m, files::close_handle, &[self.h]);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn changing_read_sizes_short_reads_and_eof_preserve_bytes() {
    let mut f = Fixture::new("semantics", 300_001);
    for (offset, requested) in [
        (0, 131072),
        (130000, 11),
        (300000, 100),
        (300001, 64),
        (0, 0),
        (77, 262144),
    ] {
        f.seek(offset);
        ddruntime::fastmem::write_bulk(&mut f.m, BUF, &vec![0xcc; requested as usize + 1]);
        let n = f.read(requested);
        assert_eq!(n, requested.min(300001 - offset));
        for i in 0..n {
            assert_eq!(f.m.u8(BUF + i), ((offset + i) % 251) as u8);
        }
        assert_eq!(f.m.u8(BUF + n), 0xcc, "short reads never copy stale tail bytes");
    }
}

#[test]
#[ignore = "CPU-only before/after file service benchmark; run explicitly"]
fn read_service_cost() {
    let mut f = Fixture::new("benchmark", 1 << 20);
    for (size, iterations) in [(1024, 2000), (131072, 256), (1048576, 64)] {
        f.seek(0);
        assert_eq!(f.read(size), size); // populate guest pages and filesystem cache
        for sample in 0..5 {
            ALLOCS.with(|n| n.set(0));
            let mut elapsed = std::time::Duration::ZERO;
            for _ in 0..iterations {
                f.seek(0);
                COUNTING.with(|c| c.set(true));
                let start = Instant::now();
                let got = f.read(size);
                elapsed += start.elapsed();
                COUNTING.with(|c| c.set(false));
                assert_eq!(got, size);
                std::hint::black_box(f.m.u8(BUF + size - 1));
            }
            eprintln!(
                "read-cost size={size} sample={sample} calls={iterations} ns_per_call={} allocations={}",
                elapsed.as_nanos() / iterations as u128,
                ALLOCS.with(Cell::get)
            );
        }
    }
}
