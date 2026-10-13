//! L134: discarded host EFLAGS do not change x87 state or memory behavior.
use ddnative_base::{
    mem::Mem,
    x87::{park, X87},
};
use std::cell::{Cell, RefCell};
#[derive(Clone)]
struct Memory {
    bytes: [u8; 256],
    polls: Cell<usize>,
    accesses: RefCell<Vec<(bool, u32)>>,
}
impl Mem for Memory {
    fn page_table(&self) -> Option<std::ptr::NonNull<ddnative_base::mem::PageTable>> {
        self.polls.set(self.polls.get() + 1);
        None
    }
    fn u8(&self, a: u32) -> u8 {
        self.accesses.borrow_mut().push((false, a));
        self.bytes[a as usize]
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        self.accesses.borrow_mut().push((true, a));
        self.bytes[a as usize] = v;
    }
}
fn memory<const OP: u16, const SIZE: usize, const WRITE: bool>(
    st: &mut X87,
    m: &mut Memory,
    addr: u32,
    capture: bool,
) {
    if capture {
        m.page_table();
        let mut data = [0u8; 10];
        if !WRITE {
            for (k, byte) in data[..SIZE].iter_mut().enumerate() {
                *byte = m.u8(addr + k as u32);
            }
        }
        // The original public path still captures host flags.
        std::hint::black_box(st.exec::<OP>(&mut data));
        if WRITE {
            for (k, byte) in data[..SIZE].iter().enumerate() {
                m.set_u8(addr + k as u32, *byte);
            }
        }
    } else {
        st.memory::<OP, SIZE, WRITE>(m, addr);
    }
}
fn reg<const OP: u16>(st: &mut X87, capture: bool) {
    if capture {
        std::hint::black_box(st.exec::<OP>(&mut [0; 10]));
    } else {
        st.register::<OP>();
    }
}
fn run(mut m: Memory, capture: bool, parked: bool) -> Memory {
    let mut st = X87::default();
    memory::<0x28d9, 2, false>(&mut st, &mut m, 16, capture); // FLDCW
    memory::<0x00d9, 4, false>(&mut st, &mut m, 0, capture); // FLD real32
    if parked {
        park();
    }
    memory::<0x00dd, 8, false>(&mut st, &mut m, 8, capture); // FLD real64
    reg::<0xc1de>(&mut st, capture); // FADDP
    if parked {
        park();
    }
    reg::<0xc0d9>(&mut st, capture); // FLD ST0
    memory::<0x08d8, 4, false>(&mut st, &mut m, 0, capture); // FMUL real32
    reg::<0xfad9>(&mut st, capture); // FSQRT, including masked invalid
    memory::<0x18dd, 8, true>(&mut st, &mut m, 32, capture); // FSTP real64
    if capture {
        let mut data = [0; 10];
        data[..4].copy_from_slice(&0x3f800000u32.to_le_bytes());
        st.exec::<0x00d9>(&mut data);
        st.exec::<0xd8dd>(&mut data);
    } else {
        st.push(0x3f800000);
        st.pop();
    }
    // Full 108-byte architectural image with identical guest pointer metadata.
    st.fnsave(&mut m, 64, 0, 0, 0);
    park();
    m
}
#[test]
fn discarded_flags_preserve_full_image_access_order_and_poll_count() {
    ddnative_host::install();
    let inputs = [
        0, 0x80000000, 1, 0x007fffff, 0x00800000, 0x3dcccccd, 0x3f800000, 0xbf800000, 0x7f7fffff, 0x7f800000,
        0xff800000, 0x7fc12345, 0x7f812345,
    ];
    for precision in [0, 0x200, 0x300] {
        for rounding in [0, 0x400, 0x800, 0xc00] {
            for bits in inputs {
                for parked in [false, true] {
                    let mut m = Memory {
                        bytes: [0xa5; 256],
                        polls: Cell::new(0),
                        accesses: RefCell::default(),
                    };
                    m.bytes[..4].copy_from_slice(&u32::to_le_bytes(bits));
                    m.bytes[8..16].copy_from_slice(&0x3fd5555555555555u64.to_le_bytes());
                    m.bytes[16..18].copy_from_slice(&u16::to_le_bytes(0x7f | precision | rounding));
                    let old = run(m.clone(), true, parked);
                    let new = run(m, false, parked);
                    assert_eq!(
                        new.bytes, old.bytes,
                        "{bits:x}/{precision:x}/{rounding:x}/{parked}"
                    );
                    assert_eq!(new.accesses, old.accesses);
                    assert_eq!(new.polls.get(), 5);
                    assert_eq!(new.polls, old.polls);
                }
            }
        }
    }
}
#[test]
fn comparison_still_captures_integer_flags() {
    ddnative_host::install();
    for (a, b, expected) in [
        (1.0f32, 2.0f32, (true, false, false)),
        (2.0, 1.0, (false, false, false)),
        (1.0, 1.0, (false, true, false)),
        (f32::from_bits(0x7fc12345), 1.0, (true, true, true)),
    ] {
        let mut st = X87::default();
        st.push(b.to_bits());
        st.push(a.to_bits());
        let flags = st.compare_flags::<0xf1db>(); // FCOMI ST0, ST1
        assert_eq!((flags.cf(), flags.zf(), flags.pf()), expected);
        assert_eq!(st.depth(), 2);
        park();
    }
}

#[test]
fn discarded_flags_keep_opcode_and_exception_mask_guards() {
    ddnative_host::install();
    assert!(std::panic::catch_unwind(|| X87::default().register::<0xffff>()).is_err());
    assert!(std::panic::catch_unwind(|| X87::default().exec::<0xffff>(&mut [0; 10])).is_err());
    let mut m = Memory {
        bytes: [0; 256],
        polls: Cell::new(0),
        accesses: RefCell::default(),
    };
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        X87::default().memory::<0x28d9, 2, false>(&mut m, 0);
    }))
    .is_err());
    assert!(std::panic::catch_unwind(|| X87::default().exec::<0x28d9>(&mut [0; 10])).is_err());
    park();
}
