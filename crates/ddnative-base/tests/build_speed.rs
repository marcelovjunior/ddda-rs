//! Accessor equivalence across the page-table fast path and trait fallback.
use ddnative_base::mem::{self, Mem, PageTable, PAGE, READ, WRITE};
use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::atomic::Ordering;

struct Memory {
    table: Box<PageTable>,
    fallback: Cell<usize>,
    expose: bool,
}
impl Memory {
    fn new() -> Self {
        let table = Box::<PageTable>::default();
        for page in [0, 1, 2, (u32::MAX / PAGE) as usize] {
            table.backed_page(page);
            table.set_perm(page, READ | WRITE);
        }
        Self { table, fallback: Cell::new(0), expose: true }
    }
    fn at(&self, address: u32) -> *mut u8 {
        let page = self.table.backed_page((address / PAGE) as usize);
        unsafe { page.add((address % PAGE) as usize) }
    }
}
impl Mem for Memory {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        self.expose.then(|| NonNull::from(self.table.as_ref()))
    }
    fn u8(&self, address: u32) -> u8 {
        self.fallback.set(self.fallback.get() + 1);
        unsafe { mem::load_at(self.at(address), 1) as u8 }
    }
    fn set_u8(&mut self, address: u32, value: u8) {
        self.fallback.set(self.fallback.get() + 1);
        unsafe { mem::store_at(self.at(address), 1, value as u32) }
    }
}

#[test]
fn accessors_preserve_aligned_unaligned_cross_page_and_wrapping_bytes() {
    // One test owns this process-global transition; production only sets it.
    for atomic in [false, true] {
        mem::ATOMIC_ACCESSES.store(atomic, Ordering::Relaxed);
        let mut memory = Memory::new();
        for expose in [false, true] {
            memory.expose = expose;
            for permissions in [0, READ, WRITE, READ | WRITE] {
                memory.table.set_perm(1, permissions);
                for address in (PAGE - 4..PAGE + 8).chain(u32::MAX - 3..=u32::MAX) {
                    for value in [0, 0x12345678, u32::MAX] {
                        let m: &mut dyn Mem = &mut memory;
                        let pt = m.page_table();
                        m.fast_set_u32(pt, address, value);
                        assert_eq!(m.u32(address), value);
                        assert_eq!(m.fast_u32(pt, address), value);
                        m.set_u32(address, !value);
                        assert_eq!(m.fast_u32(pt, address), !value);
                        m.fast_set_u16(pt, address, value as u16);
                        assert_eq!(m.u16(address), value as u16);
                        assert_eq!(m.fast_u16(pt, address), value as u16);
                        m.fast_set_u8(pt, address, value as u8);
                        assert_eq!(m.u8(address), value as u8);
                        assert_eq!(m.fast_u8(pt, address), value as u8);
                    }
                }
            }
        }
        // Missing permissions and an unbacked page must retain trait dispatch.
        memory.expose = true;
        memory.table.set_perm(1, 0);
        memory.fallback.set(0);
        let pt = memory.page_table();
        let m: &mut dyn Mem = &mut memory;
        m.fast_set_u32(pt, PAGE, 0x12345678);
        assert_eq!(m.fast_u32(pt, PAGE), 0x12345678);
        assert_eq!(memory.fallback.get(), 8);
        memory.table.set_perm(2, READ | WRITE);
        memory.table.unback(2);
        memory.fallback.set(0);
        let m: &mut dyn Mem = &mut memory;
        assert_eq!(m.fast_u32(pt, PAGE * 2), 0);
        assert_eq!(memory.fallback.get(), 4);
    }
}
