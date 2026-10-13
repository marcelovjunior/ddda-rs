//! Lifted dyn-Mem fast helpers versus runtime checked accesses, including
//! a cached table across protection, lazy backing, unmap and remap.
use ddnative_base::mem::{Mem, ATOMIC_ACCESSES};
use ddruntime::memory::{GuestMemory, PAGE, READ, WRITE};
use std::{panic::{catch_unwind, AssertUnwindSafe}, sync::atomic::Ordering};

fn outcome(f: impl FnOnce() -> u32) -> Result<u32, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|e| *e.downcast::<String>().expect("guest fault string"))
}

#[test]
fn cached_fast_table_preserves_values_faults_and_lazy_transitions() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(|| {
        for atomic in [false, true] {
            ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
            let mut fast = GuestMemory::new();
            let mut checked = GuestMemory::new();
            let pt = fast.page_table();
            for phase in 0..5 {
                for m in [&mut fast, &mut checked] {
                    match phase {
                        0 => {
                            m.map(0, PAGE, READ | WRITE).unwrap();
                            m.map(0xfffff000, PAGE, READ | WRITE).unwrap();
                            m.map_lazy(0x10000, 2 * PAGE, READ | WRITE).unwrap();
                            m.map(0x12000, PAGE, READ).unwrap();
                        }
                        1 => { m.protect(0x10000, PAGE, READ); m.protect(0x11000, PAGE, WRITE); }
                        2 => { m.unmap(0x10000, 2 * PAGE); }
                        3 => { m.map_lazy(0x10000, 2 * PAGE, READ | WRITE).unwrap(); }
                        _ => { m.protect(0x10000, PAGE, 0); }
                    }
                }
                for a in [0, 1, 0xffe, 0xfff, 0xffff, 0x10000, 0x10001, 0x10ffd,
                    0x10ffe, 0x10fff, 0x11000, 0x11fff, 0x12000, 0x12fff, 0x13000,
                    0xfffffffc, 0xfffffffd, 0xfffffffe, 0xffffffff] {
                    for width in [1, 2, 4] {
                        // Read before writing to verify lazy zero reads do not back.
                        let got = outcome(|| {
                            let m: &dyn Mem = &fast;
                            match width { 1 => m.fast_u8(pt, a) as u32, 2 => m.fast_u16(pt, a) as u32, _ => m.fast_u32(pt, a) }
                        });
                        let want = outcome(|| match width { 1 => checked.u8(a) as u32, 2 => checked.u16(a) as u32, _ => checked.u32(a) });
                        assert_eq!(got, want, "read {a:x}/{width} phase {phase} atomic {atomic}");
                        let v = a.wrapping_mul(0x9e3779b1);
                        let got = outcome(|| {
                            let m: &mut dyn Mem = &mut fast;
                            match width { 1 => m.fast_set_u8(pt, a, v as u8), 2 => m.fast_set_u16(pt, a, v as u16), _ => m.fast_set_u32(pt, a, v) }
                            0
                        });
                        let want = outcome(|| {
                            match width { 1 => checked.set_u8(a, v as u8), 2 => checked.set_u16(a, v as u16), _ => checked.set_u32(a, v) }
                            0
                        });
                        assert_eq!(got, want, "write {a:x}/{width} phase {phase} atomic {atomic}");
                    }
                }
                assert_eq!(fast.backed_pages(), checked.backed_pages());
                // Includes unchanged bytes preceding a straddling fault.
                for a in [0, 0x10000, 0x11000, 0x12000, 0xfffff000] {
                    assert_eq!(fast.read(a, PAGE), checked.read(a, PAGE));
                }
            }
        }
    }));
    std::panic::set_hook(hook);
    if let Err(e) = result { std::panic::resume_unwind(e); }
}
