//! Synthetic x87 helper timing; no guest image or game process.
use ddnative_base::{
    mem::{Mem, PageTable, READ, WRITE},
    x87::{park, X87},
};
use std::{hint::black_box as bb, ptr::NonNull, time::Instant};
struct Memory(Box<PageTable>);
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(self.0.as_ref()))
    }
    fn u8(&self, _: u32) -> u8 {
        panic!("unexpected slow read")
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        panic!("unexpected slow write")
    }
}
fn bench(mut op: impl FnMut(), n: u32, name: &str) {
    for _ in 0..10_000 {
        op();
    }
    park();
    let mut samples = [0.0; 7];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..n {
            op();
        }
        park(); // host clock and floating-point work outside guest ownership
        let elapsed = start.elapsed();
        *sample = elapsed.as_secs_f64() * 1e9 / n as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{name}: {:.3} ns/pair [{:.3}, {:.3}]",
        samples[3], samples[0], samples[6]
    );
}
fn main() {
    ddnative_host::install();
    let n = std::env::args()
        .nth(1)
        .map(|s| s.parse().unwrap())
        .unwrap_or(2_000_000);
    let mut st = X87::default();
    bench(
        || {
            bb(&mut st).register::<0xe8d9>();
            bb(&mut st).pop();
        },
        n,
        "fld1/pop",
    );
    bench(
        || {
            bb(&mut st).push(bb(0x3f800000));
            bb(&mut st).pop();
        },
        n,
        "push/pop",
    );
    let mut m = Memory(Box::default());
    m.0.backed_page(1);
    m.0.set_perm(1, READ | WRITE);
    let mem: &mut dyn Mem = bb(&mut m);
    mem.fast_set_u32(mem.page_table(), 4096, 0x3f800000);
    bench(
        || {
            bb(&mut st).memory::<0x00d9, 4, false>(mem, bb(4096));
            bb(&mut st).memory::<0x18d9, 4, true>(mem, bb(4100));
        },
        n,
        "fld32/fstp32",
    );
    assert_eq!(mem.fast_u32(mem.page_table(), 4100), 0x3f800000);
    assert_eq!(st.depth(), 0);
    park();
}
