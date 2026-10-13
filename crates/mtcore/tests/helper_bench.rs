//! L185: micro-benchmarks of the ddnative-base helpers the generated shards
//! call, in the shapes the lifter emits (release: `cargo test --release -p
//! mtcore --test helper_bench -- --ignored --nocapture --test-threads=1`).
//! Each line is ns per call (best of 7 rounds), loop overhead included
//! (`empty loop` is the floor).
use ddnative_base::emit;
use ddnative_base::mem::{Mem, PageTable};
use ddnative_base::x86::{self, Cpu};
use std::hint::black_box;

#[derive(Default)]
struct Paged {
    table: PageTable,
}
impl Mem for Paged {
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        Some(std::ptr::NonNull::from(&self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        let p = self.table.page((a >> 12) as usize);
        assert!(!p.is_null());
        // SAFETY: this table owns the backed page
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        // SAFETY: this table owns the backed page
        unsafe { ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32) }
    }
}

const N: u32 = 100_000;

fn bench(name: &str, mut body: impl FnMut(u32)) {
    let mut best = f64::MAX;
    for _ in 0..200 {
        let start = std::time::Instant::now();
        for i in 0..N {
            body(black_box(i));
        }
        best = best.min(start.elapsed().as_nanos() as f64 / N as f64);
    }
    eprintln!("{name:<28} {best:6.2} ns");
}

#[test]
#[ignore = "microbenchmark"]
fn helpers() {
    ddnative_host::install();
    let mut m = Paged::default();
    for a in (0x1000..0x3000).step_by(4) {
        m.set_u8(a, 1);
    }
    let pt = m.page_table();
    let mut mem: Box<dyn Mem> = Box::new(m);
    let m: &mut dyn Mem = &mut *mem;
    let mut m = m;
    let mut c = Cpu { esp: 0x2800, ..Cpu::default() };
    let mut sink = 0u32;
    let (a, b) = (0x3f80_0000u32, 0x4000_0000u32);

    bench("empty loop", |i| sink = sink.wrapping_add(i));
    bench("m.fast_u32", |i| sink = sink.wrapping_add(m.fast_u32(pt, 0x1000 + (i & 0x3ff) * 4)));
    bench("m.fast_set_u32", |i| m.fast_set_u32(pt, 0x1000 + (i & 0x3ff) * 4, i));
    bench("m.fast_u8", |i| sink = sink.wrapping_add(m.fast_u8(pt, 0x1000 + (i & 0xfff)) as u32));
    bench("m.fast_set_u8", |i| m.fast_set_u8(pt, 0x1000 + (i & 0xfff), i as u8));
    bench("m.fast_u16", |i| sink = sink.wrapping_add(m.fast_u16(pt, 0x1000 + (i & 0x7ff) * 2) as u32));
    bench("m.fast_set_u16", |i| m.fast_set_u16(pt, 0x1000 + (i & 0x7ff) * 2, i as u16));
    bench("emit::push + pop", |i| {
        emit::push(i, m, pt, &mut c);
        sink = sink.wrapping_add(emit::pop(m, pt, &mut c));
    });
    bench("emit::load64", |i| sink = sink.wrapping_add(emit::load64(m, pt, 0x1000 + (i & 0x3ff) * 4)[0]));
    bench("emit::store64", |i| emit::store64([i, i, 0, 0], m, pt, 0x1000 + (i & 0x3ff) * 4));
    bench("emit::load128", |i| sink = sink.wrapping_add(emit::load128(m, pt, 0x1000 + (i & 0x3ff) * 4)[0]));
    bench("emit::store128", |i| emit::store128([i; 4], m, pt, 0x1000 + (i & 0x3ff) * 4));
    // the same operations as the lifter's earlier helpers did them (one checked access per dword)
    bench("  old load64 (2 x fast_u32)", |i| {
        let a = 0x1000 + (i & 0x3ff) * 4;
        sink = sink.wrapping_add(m.fast_u32(pt, a)).wrapping_add(m.fast_u32(pt, a.wrapping_add(4)));
    });
    bench("  old store64 (2 x fast_set_u32)", |i| {
        let a = 0x1000 + (i & 0x3ff) * 4;
        m.fast_set_u32(pt, a, i);
        m.fast_set_u32(pt, a.wrapping_add(4), i);
    });
    bench("  old load128 (4 x fast_u32)", |i| {
        let a = 0x1000 + (i & 0x3ff) * 4;
        sink = sink.wrapping_add(m.fast_u32(pt, a) ^ m.fast_u32(pt, a.wrapping_add(4)) ^ m.fast_u32(pt, a.wrapping_add(8)) ^ m.fast_u32(pt, a.wrapping_add(12)));
    });
    bench("  old store128 (4 x fast_set_u32)", |i| {
        let a = 0x1000 + (i & 0x3ff) * 4;
        for k in 0..4 {
            m.fast_set_u32(pt, a.wrapping_add(4 * k), i);
        }
    });
    bench("  old comiss_hw (Flags::new)", |i| {
        let f = x86::comiss(a ^ (i & 1), b);
        sink = sink.wrapping_add(f.r);
    });
    bench("emit::xorps", |i| sink = sink.wrapping_add(emit::xorps([i; 4], [1, 2, 3, 4])[0]));
    bench("fl.alu_sub", |i| sink = sink.wrapping_add(c.fl.alu_sub(i, 7, 32)));
    bench("fl.alu_add", |i| sink = sink.wrapping_add(c.fl.alu_add(i, 7, 32)));
    bench("fl.alu_and", |i| sink = sink.wrapping_add(c.fl.alu_and(i, 7, 32)));
    bench("fl.alu_xor", |i| sink = sink.wrapping_add(c.fl.alu_xor(i, i, 32)));
    bench("fl.alu_or", |i| sink = sink.wrapping_add(c.fl.alu_or(i, 7, 32)));
    bench("fl.inc_dec", |i| sink = sink.wrapping_add({ c.fl.inc_dec(x86::INC, i, i.wrapping_add(1), 32); 0 }));
    c.fl.alu_sub(5, 7, 32);
    bench("fl.zf", |_| sink = sink.wrapping_add(c.fl.zf() as u32));
    bench("fl.cf", |_| sink = sink.wrapping_add(c.fl.cf() as u32));
    bench("fl.sf", |_| sink = sink.wrapping_add(c.fl.sf() as u32));
    bench("fl.of", |_| sink = sink.wrapping_add(c.fl.of() as u32));
    bench("fl.cond(\"le\")", |_| sink = sink.wrapping_add(c.fl.cond("le") as u32));
    bench("fl.a, fl.b (fused cmp)", |i| {
        c.fl.alu_sub(i, 9, 32);
        sink = sink.wrapping_add((c.fl.a != c.fl.b) as u32);
    });
    bench("x86::mulss", |i| sink = sink.wrapping_add(x86::mulss(a ^ (i & 1), b)));
    bench("x86::addss", |i| sink = sink.wrapping_add(x86::addss(a ^ (i & 1), b)));
    bench("x86::subss", |i| sink = sink.wrapping_add(x86::subss(a ^ (i & 1), b)));
    bench("x86::divss", |i| sink = sink.wrapping_add(x86::divss(a ^ (i & 1), b)));
    bench("x86::sqrtss", |i| sink = sink.wrapping_add(x86::sqrtss(a ^ (i & 1))));
    bench("x86::comiss_hw", |i| sink = sink.wrapping_add(x86::comiss_hw::<true>(a ^ (i & 1), b).r));
    bench("x86::cvtsi2ss", |i| sink = sink.wrapping_add(x86::cvtsi2ss(i)));
    bench("x86::cvttss2si", |i| sink = sink.wrapping_add(x86::cvttss2si(a ^ (i & 1))));
    bench("x86::mulps", |i| sink = sink.wrapping_add(x86::mulps([a ^ (i & 1); 4], [b; 4])[0]));
    bench("x86::addps", |i| sink = sink.wrapping_add(x86::addps([a ^ (i & 1); 4], [b; 4])[0]));
    bench("x86::call_local (nop fn)", |_| {
        fn nop(_: &mut dyn Mem, _: &mut Cpu) {}
        x86::call_local(m, &mut c, 0x00400000, nop);
    });
    x86::set_ext_calls(Some(Box::new(|_, _, _| {})));
    bench("x86::call_ext (nop)", |_| x86::call_ext(m, &mut c, 0x00400000));
    x86::set_ext_calls(None);
    bench("x87 real_memory fld+fstp", |i| {
        c.st.real_memory::<0x00d9, 4, false>(m, 0x1000 + (i & 0x3ff) * 4);
        c.st.real_memory::<0x18d9, 4, true>(m, 0x2000 + (i & 0x3ff) * 4);
    });
    black_box(sink);
}
