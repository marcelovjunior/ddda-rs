//! Host code's own accesses to guest memory (L144): the readable ports
//! (crates/mtdraw and the others) and the runtime's services read the
//! game's structures through `&dyn Mem`, a virtual call and a page lookup
//! per access. [`Pages`] takes the memory's page table once and makes the
//! access inline, as the lifted code's `fast_u32` does: the same check
//! ([`allowed`](super::allowed)), the same bytes and the same atomicity;
//! anything the table does not allow (lazy or unmapped pages, page-crossing
//! accesses, a memory without a table) goes through the trait, so faults
//! and fixtures behave exactly as before.
//!
//! When guest memory becomes one flat reservation (L142), [`Pages`] is the
//! one place host accesses change.
use super::{allowed, load_at, store_at, Mem, PageTable, READ, WRITE};
use std::ptr::NonNull;

/// A memory's page table, taken once (`Mem::page_table`, the lifted
/// code's function-entry read), for inline accesses to that memory.
/// Valid as long as the memory: page_table()'s contract.
#[derive(Clone, Copy)]
pub struct Pages(Option<NonNull<PageTable>>);

impl Pages {
    #[inline]
    pub fn of(m: &dyn Mem) -> Pages {
        Pages(m.page_table())
    }

    /// `len` bytes at `addr` if the table allows them (one page, backed,
    /// `perm` set): where they are.
    #[inline(always)]
    pub fn bytes(self, addr: u32, len: usize, perm: u8) -> Option<*mut u8> {
        allowed(self.0, addr, len, perm)
    }

    /// `width` (1, 2 or 4) bytes at `addr`, little-endian.
    #[inline(always)]
    pub fn load(self, m: &dyn Mem, addr: u32, width: usize) -> u32 {
        match allowed(self.0, addr, width, READ) {
            // SAFETY: allowed returns `width` bytes inside a backed page
            Some(p) => unsafe { load_at(p, width) },
            None => slow_load(m, addr, width),
        }
    }

    /// The low `width` bytes of `v` at `addr`, little-endian.
    #[inline(always)]
    pub fn store(self, m: &mut dyn Mem, addr: u32, width: usize, v: u32) {
        match allowed(self.0, addr, width, WRITE) {
            // SAFETY: allowed returns `width` bytes inside a backed page
            Some(p) => unsafe { store_at(p, width, v) },
            None => slow_store(m, addr, width, v),
        }
    }

    #[inline(always)]
    pub fn u8(self, m: &dyn Mem, addr: u32) -> u8 {
        self.load(m, addr, 1) as u8
    }
    #[inline(always)]
    pub fn u16(self, m: &dyn Mem, addr: u32) -> u16 {
        self.load(m, addr, 2) as u16
    }
    #[inline(always)]
    pub fn u32(self, m: &dyn Mem, addr: u32) -> u32 {
        self.load(m, addr, 4)
    }
    #[inline(always)]
    pub fn set_u8(self, m: &mut dyn Mem, addr: u32, v: u8) {
        self.store(m, addr, 1, v as u32)
    }
    #[inline(always)]
    pub fn set_u16(self, m: &mut dyn Mem, addr: u32, v: u16) {
        self.store(m, addr, 2, v as u32)
    }
    #[inline(always)]
    pub fn set_u32(self, m: &mut dyn Mem, addr: u32, v: u32) {
        self.store(m, addr, 4, v)
    }
}

#[cold]
#[inline(never)]
fn slow_load(m: &dyn Mem, addr: u32, width: usize) -> u32 {
    match width {
        1 => m.u8(addr) as u32,
        2 => m.u16(addr) as u32,
        _ => m.u32(addr),
    }
}

#[cold]
#[inline(never)]
fn slow_store(m: &mut dyn Mem, addr: u32, width: usize, v: u32) {
    match width {
        1 => m.set_u8(addr, v as u8),
        2 => m.set_u16(addr, v as u16),
        _ => m.set_u32(addr, v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::PAGE;

    /// A memory over a page table (as the runtime's), counting the
    /// accesses that reach the trait.
    struct Tabled {
        table: Box<PageTable>,
        trait_calls: std::cell::Cell<u32>,
    }

    impl Tabled {
        fn byte(&self, addr: u32, write: bool) -> *mut u8 {
            self.trait_calls.set(self.trait_calls.get() + 1);
            let p = (addr / PAGE) as usize;
            let need = if write { WRITE } else { READ };
            assert!(self.table.perm(p) & need != 0, "fault at {addr:#x}");
            // SAFETY: inside the page
            unsafe { self.table.backed_page(p).add((addr % PAGE) as usize) }
        }
    }

    impl Mem for Tabled {
        fn u8(&self, addr: u32) -> u8 {
            // SAFETY: a byte inside a backed page
            unsafe { *self.byte(addr, false) }
        }
        fn set_u8(&mut self, addr: u32, v: u8) {
            // SAFETY: a byte inside a backed page
            unsafe { *self.byte(addr, true) = v }
        }
        fn page_table(&self) -> Option<NonNull<PageTable>> {
            Some(NonNull::from(&*self.table))
        }
    }

    /// Pages reads and writes what the trait reads and writes: inside a
    /// backed page without the trait; across a page end, on a lazy page
    /// and on a read-only page through it; a fault faults.
    #[test]
    fn pages_access_the_bytes_the_trait_accesses() {
        let table = Box::<PageTable>::default();
        table.backed_page(0x10);
        table.set_perm(0x10, READ | WRITE);
        table.backed_page(0x11);
        table.set_perm(0x11, READ | WRITE);
        table.set_perm(0x12, READ | WRITE); // lazy: not backed
        table.backed_page(0x13);
        table.set_perm(0x13, READ);
        let mut m = Tabled { table, trait_calls: Default::default() };
        let g = Pages::of(&m);

        g.set_u32(&mut m, 0x10FFC, 0x4433_2211);
        g.set_u16(&mut m, 0x10001, 0xBBAA);
        g.set_u8(&mut m, 0x10003, 0xCC);
        assert_eq!(m.trait_calls.get(), 0, "inside a backed page, no trait call");
        assert_eq!(g.u32(&m, 0x10FFC), 0x4433_2211);
        assert_eq!(g.u32(&m, 0x10000), 0xCCBB_AA00);
        assert_eq!(g.u16(&m, 0x10FFD), 0x3322);
        assert_eq!(g.u8(&m, 0x10FFF), 0x44);
        assert_eq!(m.trait_calls.get(), 0);

        g.set_u32(&mut m, 0x10FFE, 0x8877_6655); // across the page end
        assert_eq!(m.trait_calls.get(), 4);
        assert_eq!(g.u32(&m, 0x10FFE), 0x8877_6655);
        assert_eq!([g.u8(&m, 0x10FFF), g.u8(&m, 0x11000)], [0x66, 0x77]);
        assert_eq!(g.u32(&m, 0x12000), 0, "a lazy page reads zero");
        assert_eq!(g.u32(&m, 0x13000), 0, "a read-only page reads");
        let calls = m.trait_calls.get();
        let write = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.set_u32(&mut m, 0x13000, 1)));
        assert!(write.is_err(), "a read-only page faults on a write");
        assert!(m.trait_calls.get() > calls, "the fault came from the trait");
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.u32(&m, 0x20000)));
        assert!(read.is_err(), "an unmapped page faults");
    }
}
