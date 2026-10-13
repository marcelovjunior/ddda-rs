"""Compile two repository Mem versions into one alternating CPU benchmark.

No game data, dependencies, generated shards or GPU. Run from the repo root:
  python scripts/bench_memory_accessors.py --baseline 941f5d2c
"""
import argparse
from pathlib import Path
import subprocess


SOURCE = r'''
use std::{hint::black_box as bb, time::Instant, sync::atomic::Ordering};
macro_rules! memory {
    ($name:ident, $lib:ident) => {
        mod $name {
            pub use $lib as mem;
            use mem::{Mem, PageTable, PAGE, READ, WRITE};
            pub struct Memory(pub Box<PageTable>);
            impl Memory {
                pub fn new() -> Self {
                    let m = Self(Box::default());
                    for p in 0x100..0x201 { m.0.backed_page(p); m.0.set_perm(p, READ | WRITE); }
                    m
                }
            }
            impl Mem for Memory {
                fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
                    Some(std::ptr::NonNull::from(self.0.as_ref()))
                }
                fn u8(&self, a: u32) -> u8 {
                    unsafe { mem::load_at(self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1) as u8 }
                }
                fn set_u8(&mut self, a: u32, v: u8) {
                    unsafe { mem::store_at(self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1, v as u32) }
                }
            }
        }
    }
}
memory!(before, memory_before);
memory!(after, memory_after);
fn cycles() -> u64 {
    #[cfg(windows)] unsafe {
        #[link(name = "kernel32")]
        extern "system" { fn GetCurrentThread() -> *mut (); fn QueryThreadCycleTime(t: *mut (), cycles: *mut u64) -> i32; }
        let mut value = 0;
        assert_ne!(QueryThreadCycleTime(GetCurrentThread(), &mut value), 0);
        value
    }
    #[cfg(not(windows))] { 0 }
}
fn sample(f: &mut impl FnMut(u32), n: u32) -> (f64, f64) {
    let start_cycles = cycles();
    let start = Instant::now();
    for i in 0..n { f(bb(i)); }
    (start.elapsed().as_secs_f64() * 1e9 / n as f64, (cycles() - start_cycles) as f64 / n as f64)
}
fn pair(name: &str, n: u32, mut old: impl FnMut(u32), mut new: impl FnMut(u32)) {
    for i in 0..10000 { old(bb(i)); new(bb(i)); }
    let (mut a, mut b) = ([(0f64, 0f64); 11], [(0f64, 0f64); 11]);
    for k in 0..11 {
        if k % 2 == 0 { a[k] = sample(&mut old, n); b[k] = sample(&mut new, n); }
        else { b[k] = sample(&mut new, n); a[k] = sample(&mut old, n); }
    }
    let (mut ac, mut bc) = (a.map(|s| s.1), b.map(|s| s.1));
    let mut ratios: Vec<_> = a.iter().zip(&b).map(|(old, new)| new.0 / old.0).collect();
    ratios.sort_by(f64::total_cmp);
    ac.sort_by(f64::total_cmp); bc.sort_by(f64::total_cmp);
    a.sort_by(|x,y| x.0.total_cmp(&y.0)); b.sort_by(|x,y| x.0.total_cmp(&y.0));
    println!("{name:30} {:.3} -> {:.3} ns/access (min {:.3}/{:.3}; max {:.3}/{:.3}); thread cycles {:.3} -> {:.3}; paired ratio {:.3}", a[5].0, b[5].0, a[0].0, b[0].0, a[10].0, b[10].0, ac[5], bc[5], ratios[5]);
}
fn main() {
    #[cfg(windows)] unsafe {
        #[link(name = "kernel32")]
        extern "system" { fn GetCurrentProcess() -> *mut (); fn SetProcessAffinityMask(p: *mut (), mask: usize) -> i32; }
        assert_ne!(SetProcessAffinityMask(GetCurrentProcess(), 4), 0);
    }
    let n = std::env::args().nth(1).unwrap().parse::<u32>().unwrap();
    let (mut old, mut new) = (before::Memory::new(), after::Memory::new());
    let a: &mut dyn before::mem::Mem = bb(&mut old);
    let b: &mut dyn after::mem::Mem = bb(&mut new);
    let (ap, bp) = (a.page_table(), b.page_table());
    for atomic in [false, true] {
        before::mem::ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
        after::mem::ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
        println!("atomic={atomic}; iterations={n}; 11 alternating sample pairs; CPU mask=4");
        pair("page_table control", n, |_| { bb(a.page_table()); }, |_| { bb(b.page_table()); });
        for (label, offset) in [("aligned", 0), ("unaligned", 1), ("straddle", 0xfff)] {
            let address = |i: u32| if offset == 0xfff { 0x100fff + ((i & 255) * 4096) } else { 0x100000 + ((i * 4) & 0xffffc) + offset };
            pair(&format!("fast_u32/{label}"), n, |i| { bb(a.fast_u32(ap, address(i))); }, |i| { bb(b.fast_u32(bp, address(i))); });
            pair(&format!("fast_set_u32/{label}"), n, |i| a.fast_set_u32(ap, address(i), i), |i| b.fast_set_u32(bp, address(i), i));
        }
        pair("fast_u8/hit", n, |i| { bb(a.fast_u8(ap, 0x100000 + (i & 0xfffff))); }, |i| { bb(b.fast_u8(bp, 0x100000 + (i & 0xfffff))); });
        for table in [false, true] {
            unsafe { ap.unwrap().as_ref().set_perm(0x100, 0); bp.unwrap().as_ref().set_perm(0x100, 0); }
            let (ap, bp) = if table { (ap, bp) } else { (None, None) };
            let label = if table { "permission-miss" } else { "trait" };
            pair(&format!("fast_u32/{label}"), n, |i| { bb(a.fast_u32(ap, 0x100000 + ((i * 4) & 0xffc))); }, |i| { bb(b.fast_u32(bp, 0x100000 + ((i * 4) & 0xffc))); });
            pair(&format!("fast_set_u32/{label}"), n, |i| a.fast_set_u32(ap, 0x100000 + ((i * 4) & 0xffc), i), |i| b.fast_set_u32(bp, 0x100000 + ((i * 4) & 0xffc), i));
            pair(&format!("fast_u8/{label}"), n, |i| { bb(a.fast_u8(ap, 0x100000 + (i & 0xfff))); }, |i| { bb(b.fast_u8(bp, 0x100000 + (i & 0xfff))); });
        }
        unsafe { ap.unwrap().as_ref().set_perm(0x100, 3); bp.unwrap().as_ref().set_perm(0x100, 3); }
    }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True)
    parser.add_argument("--out", type=Path, default=Path("C:/ddda-targets/L132b/paired"))
    parser.add_argument("--iterations", type=int, default=2_000_000)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    relative = "crates/ddnative-base/src/mem.rs"
    sources = {
        "before": subprocess.check_output(["git", "show", f"{args.baseline}:{relative}"]),
        "after": Path(relative).read_bytes(),
    }
    links = []
    for label, source in sources.items():
        name = f"memory_{label}"
        path = args.out / f"{name}.rs"
        path.write_bytes(source)
        lib = args.out / f"lib{name}.rlib"
        subprocess.run(["rustc", "--edition=2021", "--crate-type=rlib", "--crate-name", name,
                        "-C", "opt-level=3", "-C", "codegen-units=64", str(path), "-o", str(lib)], check=True)
        links += ["--extern", f"{name}={lib}"]
    main_rs = args.out / "paired.rs"
    main_rs.write_text(SOURCE, encoding="utf-8")
    exe = args.out / "paired.exe"
    subprocess.run(["rustc", "--edition=2021", "-C", "opt-level=3", "-C", "codegen-units=64",
                    str(main_rs), *links, "-o", str(exe)], check=True)
    subprocess.run([str(exe.resolve()), str(args.iterations)], check=True)


if __name__ == "__main__":
    main()
