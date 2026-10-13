//! L146: typed x87 operands preserve the original ordered dword accesses.
use ddnative_base::mem::{self, Mem, PageTable, PAGE, PAGES, READ, WRITE};
use std::{cell::RefCell, ptr::NonNull, sync::atomic::Ordering};
struct Memory {
    table: Box<PageTable>,
    trace: RefCell<Vec<(bool, u32, u32)>>,
    change: bool,
    show_table: bool,
    fail: Option<u32>,
}
impl Memory {
    fn new(perm: u8, backed: bool, change: bool) -> Self {
        let m = Self {
            table: Box::default(),
            trace: RefCell::new(vec![]),
            change,
            show_table: true,
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
        if self.show_table {
            Some(NonNull::from(self.table.as_ref()))
        } else {
            None
        }
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
use ddnative_base::x87::{park, X87};
fn execute(st: &mut X87, m: &mut Memory, a: u32, kind: usize, new: bool) {
    macro_rules! op {
        ($op:literal,$size:literal,$write:literal) => {
            if new {
                st.real_memory::<$op, $size, $write>(m, a)
            } else {
                st.memory::<$op, $size, $write>(m, a)
            }
        };
    }
    match kind {
        0 => op!(0xd9, 4, false),
        1 => op!(0x10d9, 4, true),
        2 => op!(0x18d9, 4, true),
        3 => op!(0xdd, 8, false),
        4 => op!(0x10dd, 8, true),
        5 => op!(0x18dd, 8, true),
        6 => op!(0x08d8, 4, false),
        7 => op!(0xd8, 4, false),
        _ => unreachable!(),
    }
}
#[test]
fn typed_operands_keep_ordered_pages_permissions_and_wrap() {
    ddnative_host::install();
    let mut count = 0;
    for atomic in [false, true] {
        mem::ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
        for perm in [0, READ, WRITE, READ | WRITE] {
            for backed in [false, true] {
                for table in [false, true] {
                    for change in [false, true] {
                        let mut m = Memory::new(perm, backed, change);
                        let mut n = Memory::new(perm, backed, change);
                        m.show_table = table;
                        n.show_table = table;
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
                            for kind in 0..8 {
                                let mut st = X87::default();
                                st.push(0x3dcccccd);
                                let mut other = st.clone();
                                execute(&mut st, &mut m, a, kind, false);
                                execute(&mut other, &mut n, a, kind, true);
                                assert_eq!(
                                    st.clone().state,
                                    other.clone().state,
                                    "state {atomic}/{perm}/{backed}/{table}/{change}/{a:x}/{kind}"
                                );
                                assert_eq!(*m.trace.borrow(), *n.trace.borrow());
                                assert_eq!(m.snapshot(), n.snapshot());
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    park();
    assert_eq!(count, 4608);
}
