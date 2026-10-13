#![cfg(all(windows, target_arch = "x86_64", feature = "flat-memory"))]
use ddnative_base::mem::{self, Mem, PAGE, READ, WRITE};
use ddruntime::memory::GuestMemory;
use std::sync::{Arc, Barrier};

fn panic_text(f: impl FnOnce()) -> String {
    let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).expect_err("guest fault");
    if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        p.downcast_ref::<&str>().unwrap().to_string()
    }
}

#[test]
fn lazy_reads_do_not_back_and_writes_share_the_committed_bytes() {
    let mut m = GuestMemory::new_flat().unwrap();
    m.map_lazy(PAGE, PAGE, READ | WRITE).unwrap();
    let flat = m.flat_base();
    let pt = m.access_context();
    let alias = m.share();
    assert_eq!((&m as &dyn Mem).flat_u32(flat, pt, PAGE), 0);
    assert_eq!(m.backed_pages(), 0);
    (&mut m as &mut dyn Mem).flat_set_u32(flat, pt, PAGE, 0xdeadbeef);
    assert_eq!(m.backed_pages(), 1);
    assert_eq!((&m as &dyn Mem).flat_u32(flat, pt, PAGE), 0xdeadbeef);
    assert_eq!(alias.u32(PAGE), 0xdeadbeef);
}

#[test]
fn permission_fault_payloads_match_and_poke_never_grants_guest_writes() {
    let mut m = GuestMemory::new_flat().unwrap();
    m.map(PAGE, PAGE, 0).unwrap();
    let flat = m.flat_base();
    let pt = m.access_context();
    for permissions in [0, READ, WRITE, READ | WRITE] {
        m.protect(PAGE, PAGE, permissions);
        m.poke(PAGE, &[0x78, 0x56, 0x34, 0x12]).unwrap();
        for width in [1, 2, 4] {
            if permissions & READ == 0 {
                let expected = panic_text(|| match width {
                    1 => {
                        m.u8(PAGE);
                    }
                    2 => {
                        m.u16(PAGE);
                    }
                    _ => {
                        m.u32(PAGE);
                    }
                });
                let got = panic_text(|| {
                    let m = &m as &dyn Mem;
                    match width {
                        1 => {
                            m.flat_u8(flat, pt, PAGE);
                        }
                        2 => {
                            m.flat_u16(flat, pt, PAGE);
                        }
                        _ => {
                            m.flat_u32(flat, pt, PAGE);
                        }
                    }
                });
                assert_eq!(got, expected);
            }
            if permissions & WRITE == 0 {
                let expected = panic_text(|| match width {
                    1 => m.set_u8(PAGE, 1),
                    2 => m.set_u16(PAGE, 1),
                    _ => m.set_u32(PAGE, 1),
                });
                let got = panic_text(|| {
                    let m = &mut m as &mut dyn Mem;
                    match width {
                        1 => m.flat_set_u8(flat, pt, PAGE, 1),
                        2 => m.flat_set_u16(flat, pt, PAGE, 1),
                        _ => m.flat_set_u32(flat, pt, PAGE, 1),
                    }
                });
                assert_eq!(got, expected);
            }
        }
    }
    m.unmap(PAGE, PAGE);
    assert_eq!(
        panic_text(|| {
            m.u32(PAGE);
        }),
        panic_text(|| {
            (&m as &dyn Mem).flat_u32(flat, pt, PAGE);
        })
    );
    m.map(PAGE, PAGE, READ | WRITE).unwrap();
    assert_eq!((&m as &dyn Mem).flat_u32(flat, pt, PAGE), 0);
    (&mut m as &mut dyn Mem).flat_set_u32(flat, pt, PAGE, 0xabcdef01);
    assert_eq!(m.u32(PAGE), 0xabcdef01);
}

#[test]
fn crossing_wrap_and_failed_store_are_byte_exact() {
    let mut m = GuestMemory::new_flat().unwrap();
    m.map(0, 3 * PAGE, READ | WRITE).unwrap();
    m.map(u32::MAX - PAGE + 1, PAGE, READ | WRITE).unwrap();
    let flat = m.flat_base();
    let pt = m.access_context();
    for a in (PAGE - 4..PAGE + 4).chain(u32::MAX - 3..=u32::MAX) {
        (&mut m as &mut dyn Mem).flat_set_u32(flat, pt, a, 0x12345678);
        assert_eq!((&m as &dyn Mem).flat_u32(flat, pt, a), 0x12345678);
        assert_eq!(m.u32(a), 0x12345678);
        (&mut m as &mut dyn Mem).flat_set_u16(flat, pt, a, 0x9876);
        assert_eq!((&m as &dyn Mem).flat_u16(flat, pt, a), 0x9876);
    }
    m.protect(PAGE, PAGE, READ);
    let before = m.read(PAGE - 4, 8).unwrap();
    let expected = panic_text(|| m.set_u32(PAGE - 2, 0));
    let got = panic_text(|| (&mut m as &mut dyn Mem).flat_set_u32(flat, pt, PAGE - 2, 0));
    assert_eq!(got, expected);
    assert_eq!(m.read(PAGE - 4, 8).unwrap(), before);
}

#[test]
fn concurrent_lazy_publication_aligned_and_unaligned_guest_accesses() {
    mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut m = GuestMemory::new_flat().unwrap();
    m.map_lazy(PAGE, PAGE, READ | WRITE).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let jobs: Vec<_> = (0..8)
        .map(|id| {
            let mut m = m.share();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let pt = m.access_context();
                let flat = m.flat_base();
                barrier.wait();
                for round in 0..2000 {
                    let addr = PAGE + id * 16 + (round & 1);
                    let m: &mut dyn Mem = &mut m;
                    m.flat_set_u32(flat, pt, addr, round);
                    assert_eq!(m.flat_u32(flat, pt, addr), round);
                }
            })
        })
        .collect();
    for job in jobs {
        job.join().unwrap();
    }
    assert_eq!(m.backed_pages(), 1);
}

#[test]
fn contended_flat_and_checked_alias_accesses_never_tear_aligned_words() {
    mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut memory = GuestMemory::new_flat().unwrap();
    memory.map(PAGE, PAGE, READ | WRITE).unwrap();
    memory.set_u32(PAGE, 0xaaaaaaaa);
    let barrier = Arc::new(Barrier::new(4));
    let jobs: Vec<_> = (0..4)
        .map(|id| {
            let mut m = memory.share();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let pt = m.access_context();
                let flat = m.flat_base();
                barrier.wait();
                for _ in 0..20_000 {
                    if id == 0 {
                        (&mut m as &mut dyn Mem).flat_set_u32(flat, pt, PAGE, 0xaaaaaaaa);
                    } else if id == 1 {
                        m.set_u32(PAGE, 0x55555555);
                    } else {
                        let v = if id == 2 {
                            (&m as &dyn Mem).flat_u32(flat, pt, PAGE)
                        } else {
                            m.u32(PAGE)
                        };
                        assert!(v == 0xaaaaaaaa || v == 0x55555555, "torn word {v:#x}");
                    }
                }
            })
        })
        .collect();
    for job in jobs {
        job.join().unwrap();
    }
}
