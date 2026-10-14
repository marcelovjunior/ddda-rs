//! L185: the per-instruction helpers as the opt-level-0 shards call them.
//! Build the example at opt-level 0 and this crate (and ddnative-host) at 3:
//!
//!   CARGO_PROFILE_DEV_PACKAGE_DDNATIVE_BASE_OPT_LEVEL=3 \
//!   CARGO_PROFILE_DEV_PACKAGE_DDNATIVE_HOST_OPT_LEVEL=3 \
//!   cargo run --example shard_cost -p ddnative-base
//!
//! The generic x87 methods are instantiated in the example (as in a shard);
//! the flag helpers are the crate's out-of-line copies. Synthetic data.
use ddnative_base::{
    mem::{self, Mem, PageTable, PAGE, READ, WRITE},
    x86::*,
};
use std::{hint::black_box as bb, ptr::NonNull, time::Instant};

struct Memory(Box<PageTable>);
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(self.0.as_ref()))
    }
    fn u8(&self, a: u32) -> u8 {
        unsafe { mem::load_at(self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        unsafe { mem::store_at(self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1, v as u32) }
    }
}

/// 16 repetitions per call, so the harness's own cost is a small share.
macro_rules! rep { ($b:block) => { for _ in 0..16 { $b } }; }
fn bench(mut f: impl FnMut(u32), n: u32, name: &str) {
    let n = n / 8;
    for i in 0..10_000 {
        f(bb(i));
    }
    let mut samples = [0.0f64; 7];
    for sample in &mut samples {
        let start = Instant::now();
        for i in 0..n {
            f(bb(i));
        }
        *sample = start.elapsed().as_secs_f64() * 1e9 / n as f64;
    }
    samples.sort_by(f64::total_cmp);
    println!("{name:34} {:8.2} ns per repetition (min {:8.2}, max {:8.2})", samples[3] / 16.0, samples[0] / 16.0, samples[6] / 16.0);
}

fn main() {
    ddnative_host::x87::reserve_slot();
    ddnative_host::install();
    let n = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(2_000_000);
    let mut memory = Memory(Box::default());
    for p in 0x100..0x110 {
        memory.0.backed_page(p);
        memory.0.set_perm(p, READ | WRITE);
    }
    let m: &mut dyn Mem = bb(&mut memory);
    let mut cpu = Cpu::default();
    bench(|_| rep!({ bb(ddnative_base::x87::live_id()); }), n, "live_id");
    println!("LIVE_SLOT = {}", ddnative_base::x87::LIVE_SLOT.load(std::sync::atomic::Ordering::Relaxed));
    // x87 (a lifted function loads and stores floats through the page table)
    bench(|i| rep!({
        let a = 0x100000 + ((i * 8) & 0xfff8);
        cpu.st.real_memory::<0x00d9, 4, false>(m, a);
        cpu.st.real_memory::<0x18d9, 4, true>(m, a + 4);
    }), n, "fld m32 + fstp m32");
    bench(|i| rep!({
        let a = 0x100000 + ((i * 8) & 0xfff8);
        cpu.st.real_memory::<0x00dd, 8, false>(m, a);
        cpu.st.real_memory::<0x18dd, 8, true>(m, a);
    }), n, "fld m64 + fstp m64");
    bench(|i| rep!({
        let a = 0x100000 + ((i * 8) & 0xfff8);
        cpu.st.real_memory::<0x00d9, 4, false>(m, a);
        cpu.st.real_memory::<0x00d8, 4, false>(m, a + 4);
        cpu.st.real_memory::<0x18d9, 4, true>(m, a);
    }), n, "fld + fadd m32 + fstp");
    bench(|i| rep!({
        let a = 0x100000 + ((i * 8) & 0xfff8);
        cpu.st.memory::<0x00db, 4, false>(m, a);
        cpu.st.real_memory::<0x18d9, 4, true>(m, a + 4);
    }), n, "fild m32 + fstp m32");
    bench(|_| rep!({
        cpu.st.register::<0xe8d9>(); // fld1
        cpu.st.register::<0xd8dd>(); // fstp st0
    }), n, "fld1 + fstp st0");
    bench(|_| rep!({
        cpu.st.register::<0xe8d9>();
        cpu.st.register::<0xe8d9>();
        cpu.st.register::<0xc9de>(); // fmulp st1, st0
        cpu.st.register::<0xd8dd>();
    }), n, "fld1 x2 + fmulp + fstp st0");
    // the floor: the same instructions with no helper at all (live FPU of this thread)
    {
        let mut buf = [0u32; 4];
        let p = bb(buf.as_mut_ptr());
        bench(|_| rep!({ unsafe { std::arch::asm!("fld dword ptr [{p}]", "fstp dword ptr [{p} + 4]", p = in(reg) p) }; }), n, "floor: raw fld+fstp m32");
        bench(|_| rep!({ unsafe { std::arch::asm!("fld1", "fstp st(0)") }; }), n, "floor: raw fld1+fstp st0");
    }
    // flags
    let mut fl = Flags::default();
    bench(|i| rep!({ bb(fl.alu_sub(i, bb(100), bb(32))); bb(fl.zf()); }), n, "sub+zf");
    bench(|i| rep!({ bb(fl.alu_sub(i, bb(100), bb(32))); bb(fl.cf()); }), n, "sub+cf");
    bench(|i| rep!({ bb(fl.alu_and(i, bb(255), bb(32))); bb(fl.zf()); }), n, "and+zf");
    bench(|i| rep!({ bb(fl.alu_add(i, bb(4), bb(32))); }), n, "add");
    bench(|i| rep!({ bb(fl.alu_xor(i, i, bb(32))); }), n, "xor");
    bench(|i| rep!({ fl.inc_dec(INC, i, i.wrapping_add(1), 32) }), n, "inc_dec");
    bench(|i| rep!({ fl.alu_sub(i, bb(100), bb(32)); bb(fl.cf()); fl.inc_dec(INC, i, i.wrapping_add(1), 32); bb(fl.zf()); }), n, "sub, cf, inc, zf");
    bench(|_| rep!({}), n, "(empty repetition)");
    bb(fl);
    bb(cpu.st.is_empty());
}
