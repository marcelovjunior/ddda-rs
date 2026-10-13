//! L146: architectural x87 images, polling and fault-position comparisons.
use ddnative_base::{
    mem::Mem,
    x87::{park, X87},
};
use std::cell::{Cell, RefCell};
#[derive(Clone)]
struct Memory {
    bytes: [u8; 64],
    polls: Cell<u32>,
    accesses: RefCell<Vec<(bool, u32)>>,
    parked: bool,
    fail: Option<(bool, u32)>,
}
impl Mem for Memory {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        self.polls.set(self.polls.get() + 1);
        if self.parked {
            park();
        }
        None
    }
    fn u8(&self, a: u32) -> u8 {
        self.accesses.borrow_mut().push((false, a));
        assert_ne!(self.fail, Some((false, a)), "read fault");
        self.bytes[a as usize]
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.accesses.borrow_mut().push((true, a));
        assert_ne!(self.fail, Some((true, a)), "write fault");
        self.bytes[a as usize] = v;
    }
}
fn execute(st: &mut X87, m: &mut Memory, kind: usize, new: bool) {
    macro_rules! op {
        ($name:ident,$op:literal,$size:literal,$write:literal) => {
            if new {
                st.real_memory::<$op, $size, $write>(m, 0)
            } else {
                st.memory::<$op, $size, $write>(m, 0)
            }
        };
    }
    match kind {
        0 => op!(load_f32, 0xd9, 4, false),
        1 => op!(store_f32, 0x10d9, 4, true),
        2 => op!(pop_f32, 0x18d9, 4, true),
        3 => op!(load_f64, 0xdd, 8, false),
        4 => op!(store_f64, 0x10dd, 8, true),
        5 => op!(pop_f64, 0x18dd, 8, true),
        6 => op!(mul_f32, 0x08d8, 4, false),
        7 => op!(add_f32, 0xd8, 4, false),
        _ => unreachable!(),
    }
}
#[test]
fn full_state_and_accesses() {
    ddnative_host::install();
    let mut count = 0;
    for precision in [0, 0x200, 0x300] {
        for rounding in [0, 0x400, 0x800, 0xc00] {
            for depth in [0, 1, 7, 8] {
                for bits in [
                    0u64,
                    0x8000000000000000,
                    1,
                    0x007fffff,
                    0x7f812345,
                    0x7fc12345,
                    0x7f800000,
                    0x3f800000,
                    0x7ff0000000000001,
                    0x7ff8000000001234,
                    0x7ff0000000000000,
                    0x3fd5555555555555,
                ] {
                    for parked in [false, true] {
                        for kind in 0..8 {
                            let mut st = X87::default();
                            let mut cw = [0u8; 10];
                            cw[..2].copy_from_slice(&u16::to_le_bytes(0x7f | precision | rounding));
                            st.exec::<0x28d9>(&mut cw);
                            for i in 0..depth {
                                st.push(if i % 2 == 0 { 0x3dcccccd } else { 0xbf800000 });
                            }
                            let mut other = st.clone();
                            let mut m = Memory {
                                bytes: [0xa5; 64],
                                polls: Cell::new(0),
                                accesses: RefCell::default(),
                                parked,
                                fail: None,
                            };
                            m.bytes[..8].copy_from_slice(&bits.to_le_bytes());
                            let mut n = m.clone();
                            execute(&mut st, &mut m, kind, false);
                            execute(&mut other, &mut n, kind, true);
                            assert_eq!(
                                st.clone().state,
                                other.clone().state,
                                "state {kind}/{bits:x}/{depth}/{precision:x}/{rounding:x}/{parked}"
                            );
                            assert_eq!(m.bytes, n.bytes);
                            assert_eq!(m.accesses, n.accesses);
                            assert_eq!(m.polls, n.polls);
                            assert_eq!(m.polls.get(), 1);
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    park();
    assert_eq!(count, 9216);
}
#[test]
fn faults_leave_same_partial_state() {
    ddnative_host::install();
    let mut faults = 0;
    for kind in 0..8 {
        for at in 0..8 {
            let write = matches!(kind, 1 | 2 | 4 | 5);
            let mut a = X87::default();
            a.push(0x3dcccccd);
            let mut b = a.clone();
            let mut m = Memory {
                bytes: [0xa5; 64],
                polls: Cell::new(0),
                accesses: RefCell::default(),
                parked: false,
                fail: Some((write, at)),
            };
            let mut n = m.clone();
            let x = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(&mut a, &mut m, kind, false)
            }));
            let y = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(&mut b, &mut n, kind, true)
            }));
            assert_eq!(x.is_err(), y.is_err());
            faults += x.is_err() as usize;
            assert_eq!(a.clone().state, b.clone().state);
            assert_eq!(m.bytes, n.bytes);
            assert_eq!(m.accesses, n.accesses);
            assert_eq!(m.polls, n.polls);
        }
    }
    park();
    assert_eq!(faults, 44);
}
