//! Standalone caller benchmark. Build ddnative-base at opt 3, then compile
//! this file with rustc -C opt-level=0 and =1 against that same rlib.
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows_benchmark {
    use ddnative_base::mem::{Mem, PageTable, PAGE, READ, WRITE};
    use std::{hint::black_box, ptr::NonNull, time::Instant};

    struct Memory(PageTable);
    impl Mem for Memory {
        fn page_table(&self) -> Option<NonNull<PageTable>> {
            Some(NonNull::from(&self.0))
        }
        fn flat_base(&self) -> Option<ddnative_base::mem::FlatBase> {
            self.0.flat_base()
        }
        fn u8(&self, _: u32) -> u8 {
            panic!("benchmark slow read")
        }
        fn set_u8(&mut self, _: u32, _: u8) {
            panic!("benchmark slow write")
        }
    }

    #[inline(never)]
    fn run(m: &mut dyn Mem, path: usize, write: bool, unaligned: bool, count: u32) -> u128 {
        let flat = m.flat_base();
        let pt = m.page_table();
        let start = Instant::now();
        if path == 2 {
            for k in 0..count {
                let a = black_box(PAGE + ((k & 255) * 8) + u32::from(unaligned));
                if write {
                    m.flat_o_set_u32(flat, a, black_box(k));
                } else {
                    black_box(m.flat_o_u32(flat, a));
                }
            }
        } else if path == 1 {
            for k in 0..count {
                let a = black_box(PAGE + ((k & 255) * 8) + u32::from(unaligned));
                if write {
                    m.flat_set_u32(flat, pt, a, black_box(k));
                } else {
                    black_box(m.flat_u32(flat, pt, a));
                }
            }
        } else {
            for k in 0..count {
                let a = black_box(PAGE + ((k & 255) * 8) + u32::from(unaligned));
                if write {
                    m.fast_set_u32(pt, a, black_box(k));
                } else {
                    black_box(m.fast_u32(pt, a));
                }
            }
        }
        start.elapsed().as_nanos()
    }

    pub fn main() {
        let mut m = Memory(PageTable::new_flat().unwrap());
        m.0.backed_page(1);
        m.0.set_perm(1, READ | WRITE);
        let count = 2_000_000;
        for unaligned in [false, true] {
            for write in [false, true] {
                let mut results = [vec![], vec![], vec![]];
                for round in 0..9 {
                    for k in 0..3 {
                        let index = (k + round) % 3;
                        results[index].push(
                            run(black_box(&mut m), index, write, unaligned, count) as f64 / count as f64,
                        );
                    }
                }
                for values in &mut results {
                    values.sort_by(f64::total_cmp);
                }
                println!(
                    "unaligned={unaligned} write={write} table_ns={:.3} inline_ns={:.3} outlined_ns={:.3}",
                    results[0][4], results[1][4], results[2][4]
                );
            }
        }
    }
}

fn main() {
    #[cfg(all(windows, target_arch = "x86_64"))]
    windows_benchmark::main();
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    eprintln!("flat_cost requires Windows x64");
}
