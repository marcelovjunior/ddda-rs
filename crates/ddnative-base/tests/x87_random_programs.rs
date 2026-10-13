//! L185: random x87 programs through the helpers (`real_memory`, `memory`,
//! `register`, which read guest memory directly or through a buffer and run
//! their checks in one optimised call) against the plain definition of each
//! instruction: copy the operand into a ten-byte buffer, `exec::<OP>`, copy
//! it back. Two guest states run interleaved on one thread (each displaces
//! the other from the FPU), over a page table with unaligned and
//! page-crossing operands. The final images (FNSAVE) and memory must match.
use ddnative_base::{
    mem::{Mem, PageTable, PAGE, READ, WRITE},
    x87::X87,
};
use std::ptr::NonNull;

#[derive(Default)]
struct Paged {
    table: PageTable,
}
impl Paged {
    fn new() -> Self {
        let p = Self::default();
        for page in 0x100..0x103 {
            p.table.backed_page(page);
            p.table.set_perm(page, READ | WRITE);
        }
        p
    }
}
impl Mem for Paged {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(&self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        unsafe { ddnative_base::mem::load_at(self.table.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        unsafe { ddnative_base::mem::store_at(self.table.backed_page((a / PAGE) as usize).add((a % PAGE) as usize), 1, v as u32) }
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

/// Run instruction number `kind` through the helper (`new`) or through the buffer definition.
fn step(st: &mut X87, m: &mut Paged, kind: usize, addr: u32, new: bool) {
    macro_rules! reference {
        ($op:literal, $size:literal, $write:literal) => {{
            let mut data = [0u8; 10];
            if !$write {
                for k in 0..$size {
                    data[k] = m.u8(addr.wrapping_add(k as u32));
                }
            }
            st.exec::<$op>(&mut data);
            if $write {
                for k in 0..$size {
                    m.set_u8(addr.wrapping_add(k as u32), data[k]);
                }
            }
        }};
    }
    macro_rules! real {
        ($op:literal, $size:literal, $write:literal) => {
            if new { st.real_memory::<$op, $size, $write>(m, addr) } else { reference!($op, $size, $write) }
        };
    }
    macro_rules! mem {
        ($op:literal, $size:literal, $write:literal) => {
            if new { st.memory::<$op, $size, $write>(m, addr) } else { reference!($op, $size, $write) }
        };
    }
    macro_rules! reg {
        ($op:literal) => {
            if new { st.register::<$op>() } else { st.exec::<$op>(&mut [0; 10]); }
        };
    }
    match kind {
        0 => real!(0x00d9, 4, false),
        1 => real!(0x00d8, 4, false),
        2 => real!(0x08d8, 4, false),
        3 => real!(0x10d9, 4, true),
        4 => real!(0x18d9, 4, true),
        5 => real!(0x00dd, 8, false),
        6 => real!(0x10dd, 8, true),
        7 => real!(0x18dd, 8, true),
        8 => mem!(0x00db, 4, false),
        9 => mem!(0x18db, 4, true),
        10 => mem!(0x38df, 8, true),
        11 => mem!(0x20d8, 4, false),
        12 => mem!(0x38d9, 2, true),
        13 => mem!(0x00dc, 8, false),
        14 => reg!(0xe8d9),
        15 => reg!(0xeed9),
        16 => reg!(0xc1de),
        17 => reg!(0xc9de),
        18 => reg!(0xc9d9),
        19 => reg!(0xd8dd),
        20 => reg!(0xe0d9),
        21 => reg!(0xe1d9),
        22 => reg!(0xc0d9),
        _ => unreachable!(),
    }
}
const KINDS: u32 = 23;

fn image(st: &mut X87, m: &mut Paged) -> Vec<u8> {
    st.fnsave(m, 0x102000, 0, 0, 0);
    (0..108).map(|k| m.u8(0x102000 + k)).collect()
}

#[test]
fn random_programs_match_the_buffer_definition() {
    ddnative_host::x87::reserve_slot();
    ddnative_host::install();
    let mut rng = Rng(0x5851f42d4c957f2d);
    for program in 0..300 {
        let (mut a, mut b) = (Paged::new(), Paged::new());
        // the same random memory for both runs; mostly ordinary numbers
        for k in 0..0x2000u32 {
            let v = if k % 4 == 0 && rng.next() % 3 == 0 { 0x3f80_0000 | (rng.next() & 0x007f_ffff) } else { rng.next() };
            for (j, byte) in v.to_le_bytes().into_iter().enumerate() {
                if k % 4 == 0 {
                    a.set_u8(0x100000 + k + j as u32, byte);
                    b.set_u8(0x100000 + k + j as u32, byte);
                }
            }
        }
        let (mut new, mut old) = ([X87::default(), X87::default()], [X87::default(), X87::default()]);
        let length = 20 + rng.next() % 200;
        for _ in 0..length {
            let which = (rng.next() & 1) as usize;
            let kind = (rng.next() % KINDS) as usize;
            let addr = match rng.next() % 8 {
                0 => 0x101000 - 1 - rng.next() % 6,           // across the page boundary
                1 => 0x100000 + rng.next() % 0x1ff0 | 1,      // unaligned
                _ => 0x100000 + (rng.next() % 0x7fc) * 4,     // aligned
            };
            step(&mut new[which], &mut a, kind, addr, true);
            step(&mut old[which], &mut b, kind, addr, false);
        }
        for which in 0..2 {
            assert_eq!(image(&mut new[which], &mut a), image(&mut old[which], &mut b), "program {program} state {which}");
        }
        for k in 0..0x2000 {
            assert_eq!(a.u8(0x100000 + k), b.u8(0x100000 + k), "program {program} memory {k:#x}");
        }
    }
}

#[test]
fn a_control_word_that_unmasks_exceptions_is_refused() {
    ddnative_host::x87::reserve_slot();
    ddnative_host::install();
    let mut m = Paged::new();
    let mut st = X87::default();
    m.set_u8(0x100000, 0x7f);
    m.set_u8(0x100001, 0x03);
    st.memory::<0x28d9, 2, false>(&mut m, 0x100000); // fldcw 037f: fine
    m.set_u8(0x100000, 0x00);
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| st.memory::<0x28d9, 2, false>(&mut m, 0x100000)));
    assert!(refused.is_err());
}
