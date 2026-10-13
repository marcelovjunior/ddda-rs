//! Batched helpers must preserve the original ordered dword accesses.
use ddnative_base::{
    emit,
    mem::{self, Mem, PageTable, PAGE, PAGES, READ, WRITE},
};
use std::{cell::RefCell, ptr::NonNull, sync::atomic::Ordering};
struct Memory {
    table: Box<PageTable>,
    trace: RefCell<Vec<(bool, u32, u32)>>,
    change: bool,
    fail: Option<u32>,
}
impl Memory {
    fn new(perm: u8, backed: bool, change: bool) -> Self {
        let m = Self {
            table: Box::default(),
            trace: RefCell::new(vec![]),
            change,
            fail: None,
        };
        for p in [0, 1, PAGES - 1] {
            if backed {
                let ptr = m.table.backed_page(p);
                for k in 0..PAGE {
                    unsafe {
                        mem::store_at(ptr.add(k as usize), 1, k.wrapping_mul(37));
                    }
                }
            }
            m.table.set_perm(p, perm);
        }
        m
    }
    fn change(&self, a: u32) {
        if self.change {
            let p = (a.wrapping_add(4) / PAGE) as usize;
            self.table.backed_page(p);
            self.table.set_perm(p, READ | WRITE);
        }
    }
    fn snapshot(&self) -> Vec<u8> {
        let mut out = vec![];
        for p in [0, 1, PAGES - 1] {
            let ptr = self.table.page(p);
            out.push(self.table.perm(p));
            out.push(!ptr.is_null() as u8);
            if !ptr.is_null() {
                for k in 0..PAGE {
                    out.push(unsafe { mem::load_at(ptr.add(k as usize), 1) as u8 });
                }
            }
        }
        out
    }
}
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(self.table.as_ref()))
    }
    fn u8(&self, _: u32) -> u8 {
        panic!("dword fallback must stay dword")
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        panic!("dword fallback must stay dword")
    }
    fn u32(&self, a: u32) -> u32 {
        let v = a.rotate_left(7) ^ 0xa1b2c3d4;
        self.trace.borrow_mut().push((false, a, v));
        self.change(a);
        v
    }
    fn set_u32(&mut self, a: u32, v: u32) {
        self.trace.borrow_mut().push((true, a, v));
        if self.fail == Some(a) {
            panic!("fixture write fault");
        }
        self.change(a);
    }
}
fn execute(m: &mut Memory, a: u32, kind: usize, table: bool, candidate: bool) -> [u32; 4] {
    let pt = if table { m.page_table() } else { None };
    let m: &mut dyn Mem = m;
    let values = [0x12345678, 0x90abcdef, 0xfedcba98, 0x76543210];
    if candidate {
        match kind {
            0 => emit::load128(m, pt, a),
            1 => emit::load64(m, pt, a),
            2 => {
                emit::store128(values, m, pt, a);
                [0; 4]
            }
            _ => {
                emit::store64(values, m, pt, a);
                [0; 4]
            }
        }
    } else {
        let count = if kind % 2 == 0 { 4 } else { 2 };
        let mut out = [0; 4];
        for k in 0..count {
            let addr = a.wrapping_add(4 * k as u32);
            if kind < 2 {
                out[k] = m.fast_u32(pt, addr);
            } else {
                m.fast_set_u32(pt, addr, values[k]);
            }
        }
        out
    }
}
#[test]
fn batched_words_match_ordered_accesses() {
    let mut cases = 0;
    for atomic in [false, true] {
        mem::ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
        for perm in [0, READ, WRITE, READ | WRITE] {
            for backed in [false, true] {
                for table in [false, true] {
                    for change in [false, true] {
                        let mut reference = Memory::new(perm, backed, change);
                        let mut candidate = Memory::new(perm, backed, change);
                        for a in [
                            0,
                            1,
                            PAGE - 16,
                            PAGE - 12,
                            PAGE - 4,
                            PAGE - 3,
                            PAGE - 1,
                            u32::MAX - 11,
                            u32::MAX - 2,
                        ] {
                            for kind in 0..4 {
                                assert_eq!(
                                    execute(&mut reference, a, kind, table, false),
                                    execute(&mut candidate, a, kind, table, true),
                                    "result {atomic} {perm} {a:x} {kind}"
                                );
                                assert_eq!(
                                    *reference.trace.borrow(),
                                    *candidate.trace.borrow(),
                                    "trace {a:x} {kind}"
                                );
                                assert_eq!(reference.snapshot(), candidate.snapshot(), "memory {a:x} {kind}");
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 2304);
}

#[test]
fn store_fault_preserves_prior_writes_and_stops_later_lanes() {
    let mut reference = Memory::new(READ | WRITE, true, false);
    let mut candidate = Memory::new(READ | WRITE, true, false);
    for m in [&mut reference, &mut candidate] {
        m.table.set_perm(1, READ);
        m.fail = Some(PAGE);
    }
    let before = reference.snapshot();
    for (m, candidate) in [(&mut reference, false), (&mut candidate, true)] {
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(
            m,
            PAGE - 8,
            2,
            true,
            candidate
        )))
        .is_err());
    }
    assert_ne!(reference.snapshot(), before);
    assert_eq!(reference.snapshot(), candidate.snapshot());
    assert_eq!(*reference.trace.borrow(), vec![(true, PAGE, 0xfedcba98)]);
    assert_eq!(*reference.trace.borrow(), *candidate.trace.borrow());
}
