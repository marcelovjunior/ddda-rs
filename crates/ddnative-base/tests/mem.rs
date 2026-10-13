//! Fast accessor admission and atomic page publication without game data.
use ddnative_base::mem::{self, Mem, PageTable, PAGE, PAGES, READ, WRITE};
use std::{cell::Cell, ptr::NonNull, sync::{Arc, Barrier}};

struct Watched {
    table: PageTable,
    calls: Cell<usize>,
}
impl Mem for Watched {
    fn page_table(&self) -> Option<NonNull<PageTable>> { Some(NonNull::from(&self.table)) }
    fn u8(&self, _: u32) -> u8 { self.calls.set(self.calls.get() + 1); 0xab }
    fn u16(&self, _: u32) -> u16 { self.calls.set(self.calls.get() + 1); 0xabcd }
    fn u32(&self, _: u32) -> u32 { self.calls.set(self.calls.get() + 1); 0xabcdef01 }
    fn set_u8(&mut self, _: u32, _: u8) { self.calls.set(self.calls.get() + 1); }
    fn set_u16(&mut self, _: u32, _: u16) { self.calls.set(self.calls.get() + 1); }
    fn set_u32(&mut self, _: u32, _: u32) { self.calls.set(self.calls.get() + 1); }
}

#[test]
fn permissions_backing_widths_and_wrap_preserve_fallback() {
    let mut memory = Watched { table: PageTable::default(), calls: Cell::new(0) };
    // All eight bits survive both backed/unbacked states. Extra bits must
    // never grant READ/WRITE, nor make an unbacked lazy page dereferenceable.
    for p in [0, 0x100, PAGES - 1] {
        for backed in [false, true] {
            if backed { memory.table.backed_page(p); }
            for perm in 0..=255u8 {
                memory.table.set_perm(p, perm);
                assert_eq!(memory.table.perm(p), perm);
                for width in [1, 2, 4] {
                    for off in [0, 1, PAGE - 4, PAGE - 3, PAGE - 2, PAGE - 1] {
                        let addr = p as u32 * PAGE + off;
                        let pt = memory.page_table();
                        memory.calls.set(0);
                        let m: &mut dyn Mem = &mut memory;
                        match width {
                            1 => m.fast_set_u8(pt, addr, 0x12),
                            2 => m.fast_set_u16(pt, addr, 0x1234),
                            _ => m.fast_set_u32(pt, addr, 0x12345678),
                        }
                        let write_hit = backed && perm & WRITE != 0 && off + width <= PAGE;
                        assert_eq!(memory.calls.get(), usize::from(!write_hit));
                        memory.calls.set(0);
                        let m: &dyn Mem = &memory;
                        match width {
                            1 => { m.fast_u8(pt, addr); }
                            2 => { m.fast_u16(pt, addr); }
                            _ => { m.fast_u32(pt, addr); }
                        }
                        let read_hit = backed && perm & READ != 0 && off + width <= PAGE;
                        assert_eq!(memory.calls.get(), usize::from(!read_hit));
                    }
                }
            }
        }
        memory.table.unback(p);
    }
    let m: &mut dyn Mem = &mut memory;
    assert_eq!(m.fast_u32(None, 0), 0xabcdef01);
    assert_eq!(m.fast_u16(None, 0), 0xabcd);
    assert_eq!(m.fast_u8(None, 0), 0xab);
}

#[test]
fn unback_retires_bytes_and_reback_is_zero_with_permissions_intact() {
    let table = PageTable::default();
    table.set_perm(42, 0xfd);
    let old = table.backed_page(42);
    unsafe { mem::store_at(old, 4, 0x12345678); }
    table.unback(42);
    assert!(table.page(42).is_null());
    assert_eq!(table.perm(42), 0xfd);
    assert_eq!(table.backed_count(), 0);
    let fresh = table.backed_page(42);
    assert_ne!(old, fresh);
    assert_eq!(table.perm(42), 0xfd);
    assert_eq!(table.backed_count(), 1);
    unsafe {
        assert_eq!(mem::load_at(old, 4), 0x12345678);
        assert_eq!(mem::load_at(fresh, 4), 0);
    }
}

#[test]
fn concurrent_permission_changes_and_materialisation_lose_neither() {
    let table = Arc::new(PageTable::default());
    let barrier = Arc::new(Barrier::new(5));
    std::thread::scope(|scope| {
        for worker in 0..4 {
            let (table, barrier) = (table.clone(), barrier.clone());
            scope.spawn(move || {
                barrier.wait();
                for p in 0..256 {
                    if worker == 0 {
                        for perm in [READ, WRITE, 0, 0xff] { table.set_perm(p, perm); }
                    } else {
                        let first = table.backed_page(p);
                        assert_eq!(first, table.backed_page(p));
                    }
                }
            });
        }
        barrier.wait();
    });
    assert_eq!(table.backed_count(), 256);
    for p in 0..256 {
        assert_eq!(table.perm(p), 0xff);
        assert!(!table.page(p).is_null());
    }
}
