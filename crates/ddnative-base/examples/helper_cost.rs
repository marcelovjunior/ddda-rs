//! Criterion-free helper timings. Run with --profile play; optional iterations
//! (default 2 million). Synthetic stack/object addresses, not a gameplay trace.
use ddnative_base::{mem::{self, Mem, PageTable, PAGE, READ, WRITE}, x86::*};
use std::{hint::black_box as bb, ptr::NonNull, sync::atomic::Ordering, time::Instant};

struct Memory(Box<PageTable>);
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> { Some(NonNull::from(self.0.as_ref())) }
    fn u8(&self, a: u32) -> u8 {
        unsafe { mem::load_at(self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        unsafe { mem::store_at(self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1, v as u32) }
    }
}
fn bench(mut f: impl FnMut(u32), n: u32, name: &str) {
    for i in 0..10_000 { f(bb(i)); }
    let mut samples = [0.0f64; 7];
    for sample in &mut samples {
        let start = Instant::now();
        for i in 0..n { f(bb(i)); }
        *sample = start.elapsed().as_secs_f64() * 1e9 / n as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!("{name:28} {:8.3} ns/call (min {:8.3}, max {:8.3})", samples[3], samples[0], samples[6]);
}
fn main() {
    let n = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(2_000_000);
    println!("iterations={n}, samples=7, Flags={} bytes", std::mem::size_of::<Flags>());
    let mut memory = Memory(Box::default());
    for p in 0x100..0x201 { memory.0.backed_page(p); memory.0.set_perm(p, READ | WRITE); }
    let m: &mut dyn Mem = bb(&mut memory);
    let pt = m.page_table();
    for atomic in [false, true] {
        mem::ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
        println!("atomic={atomic}");
        bench(|_| { bb(m.page_table()); }, n, "page_table");
        for (label, offset) in [("aligned", 0), ("unaligned", 1)] {
            bench(|i| { bb(m.fast_u32(pt, 0x100000 + ((i * 4) & 0xffffc) + offset)); }, n, &format!("fast_u32/{label}"));
            bench(|i| m.fast_set_u32(pt, 0x100000 + ((i * 4) & 0xffffc) + offset, i), n, &format!("fast_set_u32/{label}"));
        }
        bench(|i| { bb(m.fast_u32(pt, 0x100fff + ((i & 255) * PAGE))); }, n, "fast_u32/straddle");
        bench(|i| { bb(m.fast_u32(None, 0x100000 + ((i * 4) & 0xffffc))); }, n, "fast_u32/trait");
    }
    let mut fl = Flags::default();
    bench(|i| { bb(Flags::new(SUB, i, 100, i.wrapping_sub(100), 32)); }, n, "Flags::new");
    for bits in [32, 8, 16] {
        bench(|i| { bb(fl.alu_sub(i, bb(100), bb(bits))); }, n, &format!("alu_sub/{bits}"));
        bench(|i| { bb(fl.alu_add(i, bb(100), bb(bits))); }, n, &format!("alu_add/{bits}"));
        bench(|i| { bb(fl.alu_and(i, bb(255), bb(bits))); }, n, &format!("alu_and/{bits}"));
    }
    bench(|i| fl.inc_dec(INC, i, i.wrapping_add(1), 32), n, "inc_dec");
    let flags: Vec<_> = (0..256u32).map(|i| Flags::new(SUB, i, 100, i.wrapping_sub(100), 32)).collect();
    bench(|i| { bb(flags[(i & 255) as usize].zf()); }, n, "zf/sub");
    bench(|i| { bb(flags[(i & 255) as usize].cf()); }, n, "cf/sub");
    bench(|i| { bb(flags[(i & 255) as usize].cond(bb("le"))); }, n, "cond/le/sub");
    bench(|i| { fl.alu_sub(i, 100, 32); bb(fl.cond(bb("be"))); }, n, "sub+cond/be");
    bb(fl);
}
