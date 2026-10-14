//! L190: the stack window helpers read and write exactly the bytes the page table serves, inside
//! the window, at its edges, unaligned and outside; an empty window never reaches the host block.
use ddnative_base::emit;
use ddnative_base::mem::{Mem, PageTable};
use ddnative_base::x86::Cpu;

#[derive(Default)]
struct Paged {
    table: PageTable,
}
impl Mem for Paged {
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        Some(std::ptr::NonNull::from(&self.table))
    }
    fn u8(&self, a: u32) -> u8 {
        let p = self.table.page((a >> 12) as usize);
        assert!(!p.is_null(), "unmapped {a:x}");
        unsafe { ddnative_base::mem::load_at(p.add((a & 4095) as usize), 1) as u8 }
    }
    fn set_u8(&mut self, a: u32, v: u8) {
        let page = (a >> 12) as usize;
        let p = self.table.backed_page(page);
        self.table.set_perm(page, 3);
        unsafe { ddnative_base::mem::store_at(p.add((a & 4095) as usize), 1, v as u32) }
    }
}

const LO: u32 = 0x0F00_0000;
const LEN: u32 = 0x4000;

fn fixture() -> (Paged, Cpu, *mut u8) {
    let mut m = Paged::default();
    let layout = std::alloc::Layout::from_size_align(LEN as usize, 4096).unwrap();
    let block = unsafe { std::alloc::alloc_zeroed(layout) };
    assert!(unsafe { m.table.install_external((LO >> 12) as usize, block, (LEN >> 12) as usize, 3) });
    // pages outside the window, through the ordinary path
    for a in (LO + LEN..LO + LEN + 0x2000).step_by(4).chain((LO - 0x2000..LO).step_by(4)) {
        m.set_u8(a, (a >> 3) as u8);
    }
    let mut c = Cpu::default();
    unsafe { c.set_stack_window(LO, LEN, block) };
    (m, c, block)
}

#[test]
#[cfg(feature = "stack-window")]
fn window_and_page_table_agree() {
    let (mut m, c, _block) = fixture();
    let pt = m.page_table();
    let dm: &mut dyn Mem = &mut m;
    let mut x = 0x1234_5678u32;
    let mut rnd = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    let edges = [LO, LO + 4, LO + LEN - 4, LO + LEN - 8, LO + 1, LO + LEN - 3, LO + LEN - 2, LO - 4, LO - 1, LO + LEN, LO + LEN + 4, LO + 0xffc, LO + 0x1000, LO + 0xffd];
    for step in 0..6000 {
        let addr = if step < edges.len() { edges[step] } else { LO - 0x1000 + (rnd() % (LEN + 0x2000)) };
        let v = rnd();
        // write through the helper, read through both
        emit::stack_set_u32(dm, pt, &c, addr, v);
        assert_eq!(dm.fast_u32(pt, addr), v, "store {addr:#x} seen by the page table");
        assert_eq!(emit::stack_u32(dm, pt, &c, addr), v, "load {addr:#x}");
        // write through the page table, read through the helper
        let w = rnd();
        dm.fast_set_u32(pt, addr, w);
        assert_eq!(emit::stack_u32(dm, pt, &c, addr), w, "load after table store {addr:#x}");
    }
}

#[test]
fn push_pop_round_trip() {
    let (mut m, mut c, _block) = fixture();
    let pt = m.page_table();
    let dm: &mut dyn Mem = &mut m;
    c.esp = LO + LEN - 16;
    for v in [1u32, 0xdead_beef, 7] {
        emit::push_s(v, dm, pt, &mut c);
    }
    assert_eq!(c.esp, LO + LEN - 28);
    assert_eq!(dm.fast_u32(pt, c.esp), 7);
    assert_eq!(emit::pop_s(dm, pt, &mut c), 7);
    assert_eq!(emit::pop_s(dm, pt, &mut c), 0xdead_beef);
    assert_eq!(emit::pop_s(dm, pt, &mut c), 1);
}

#[test]
fn an_empty_window_never_reaches_the_host() {
    let mut m = Paged::default();
    for a in (0x1000u32..0x3000).step_by(4) {
        m.set_u8(a, 5);
    }
    let pt = m.page_table();
    let c = Cpu::default(); // span 0, host 0
    let dm: &mut dyn Mem = &mut m;
    for a in [0u32, 4, 0x1000, 0x1ffc, 0x2000] {
        emit::stack_set_u32(dm, pt, &c, a, 0x0101_0101);
        assert_eq!(emit::stack_u32(dm, pt, &c, a), 0x0101_0101);
    }
}
