//! Allocation-policy exactness and bounded synthetic locality diagnostics.
//! No game is run; synthetic timings are not gameplay or hardware miss rates.
use ddnative_base::mem::{load_at, store_at, BackingMode, Mem, PageTable, PAGE, READ, WRITE};
use std::{
    ptr::NonNull,
    sync::{Arc, Barrier},
};

struct Memory(PageTable);
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(&self.0))
    }
    fn u8(&self, a: u32) -> u8 {
        assert_ne!(self.0.perm((a / PAGE) as usize) & READ, 0);
        unsafe {
            load_at(
                self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize),
                1,
            ) as u8
        }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        assert_ne!(self.0.perm((a / PAGE) as usize) & WRITE, 0);
        unsafe {
            store_at(
                self.0.backed_page((a / PAGE) as usize).add((a % PAGE) as usize),
                1,
                v as u32,
            );
        }
    }
}

#[test]
fn region_alignment_sparse_zero_remap_and_wrap() {
    for mode in [BackingMode::Regions, BackingMode::Large] {
        let mut m = Memory(PageTable::with_backing(mode));
        for page in [511usize, 1, (1 << 20) - 1, 512, 0] {
            m.0.set_perm(page, READ | WRITE);
            let p = m.0.backed_page(page);
            assert_eq!(p as usize % (2 << 20), page % 512 * 4096);
            assert_eq!(unsafe { load_at(p, 4) }, 0);
        }
        assert_eq!(m.0.page(1) as usize - m.0.page(0) as usize, 4096);
        assert!(m.0.page(2).is_null());
        let pt = m.page_table();
        let mem: &mut dyn Mem = &mut m;
        mem.fast_set_u32(pt, u32::MAX - 1, 0x12345678);
        assert_eq!(mem.fast_u32(pt, u32::MAX - 1), 0x12345678);
        let old = m.0.page(0);
        let old_value = unsafe { load_at(old, 4) };
        m.0.unback(0);
        let fresh = m.0.backed_page(0);
        assert_ne!(fresh, old);
        assert_eq!(unsafe { load_at(fresh, 4) }, 0);
        assert_eq!(unsafe { load_at(old, 4) }, old_value);
        assert_eq!(m.0.perm(0), READ | WRITE);
        assert_eq!(m.0.backed_count(), 5);
    }
}

#[test]
fn region_publication_races_and_retirement() {
    let table = Arc::new(PageTable::with_backing(BackingMode::Regions));
    let barrier = Arc::new(Barrier::new(4));
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let (table, barrier) = (table.clone(), barrier.clone());
                s.spawn(move || {
                    barrier.wait();
                    (0..128)
                        .map(|p| {
                            table.set_perm(p, READ | WRITE);
                            table.backed_page(p) as usize
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let pointers: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(pointers.windows(2).all(|p| p[0] == p[1]));
    });
    for p in 0..128 {
        let old = table.page(p);
        unsafe { store_at(old, 4, p as u32 + 1) };
        table.unback(p);
        assert_eq!(unsafe { load_at(table.backed_page(p), 4) }, 0);
        assert_eq!(unsafe { load_at(old, 4) }, p as u32 + 1);
    }
    // Readers may hold a pointer across another thread's unback/reback.
    std::thread::scope(|s| {
        for _ in 0..4 {
            let table = table.clone();
            s.spawn(move || {
                for _ in 0..128 {
                    let old = table.backed_page(256);
                    table.unback(256);
                    assert_eq!(unsafe { load_at(old, 4) }, 0);
                }
            });
        }
    });
}

#[test]
#[ignore = "bounded synthetic CPU benchmark; not a real-job replay"]
fn paired_locality_cost() {
    use std::{hint::black_box, time::Instant};
    const COUNT: usize = 8192; // 32 MiB guest data per policy, one thread.
    let mut memories = [BackingMode::Pages, BackingMode::Regions].map(|mode| {
        let m = Memory(PageTable::with_backing(mode));
        for page in 0..COUNT {
            m.0.set_perm(page, READ | WRITE);
            let p = m.0.backed_page(page);
            for word in 0..1024 {
                unsafe {
                    store_at(p.add(word * 4), 4, (page ^ word) as u32);
                }
            }
        }
        m
    });
    for pattern in ["sequential", "page-walk", "read-write"] {
        for round in 0..7 {
            for step in 0..4 {
                // Rotate all four variants so prefetch comparisons do not
                // systematically run later than the no-prefetch baseline.
                let variant = (round + step) % 4;
                let policy = variant % 2;
                let prefetch = variant >= 2;
                let m = &mut memories[policy];
                let pt = m.page_table();
                let mut sum = 0u64;
                let start = Instant::now();
                const ACCESSES: usize = 1 << 22;
                for i in 0..ACCESSES {
                    let address = |i: usize| -> u32 {
                        if pattern == "page-walk" {
                            (((i * 4051) % COUNT) * 4096 + (i % 64) * 64) as u32
                        } else {
                            ((i * 64) % (COUNT * 4096)) as u32
                        }
                    };
                    if prefetch {
                        let next = address(i + 16);
                        let p = m.0.page((next / PAGE) as usize);
                        #[cfg(target_arch = "x86_64")]
                        unsafe {
                            std::arch::x86_64::_mm_prefetch(
                                p.add((next % PAGE) as usize).cast(),
                                std::arch::x86_64::_MM_HINT_T0,
                            );
                        }
                    }
                    let mem: &mut dyn Mem = m;
                    let a = black_box(address(i));
                    let value = mem.fast_u32(pt, a);
                    sum = sum.wrapping_add(value as u64);
                    if pattern == "read-write" {
                        mem.fast_set_u32(pt, a, value);
                    }
                }
                black_box(sum);
                eprintln!("locality pattern={pattern} prefetch={prefetch} round={round} policy={policy} ns={:.3} sum={sum}", start.elapsed().as_nanos() as f64 / ACCESSES as f64);
            }
        }
    }
}
