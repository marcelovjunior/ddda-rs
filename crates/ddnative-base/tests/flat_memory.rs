#![cfg(all(windows, target_arch = "x86_64", feature = "flat-memory"))]
use ddnative_base::mem::{self, Mem, PageTable, PAGE, READ, WRITE};
use std::{cell::Cell, ptr::NonNull};

struct Watched {
    table: PageTable,
    calls: Cell<usize>,
}
impl Mem for Watched {
    fn page_table(&self) -> Option<NonNull<PageTable>> {
        Some(NonNull::from(&self.table))
    }
    fn flat_base(&self) -> Option<mem::FlatBase> {
        self.table.flat_base()
    }
    fn u8(&self, _: u32) -> u8 {
        self.calls.set(self.calls.get() + 1);
        0x12
    }
    fn u16(&self, _: u32) -> u16 {
        self.calls.set(self.calls.get() + 1);
        0x1234
    }
    fn u32(&self, _: u32) -> u32 {
        self.calls.set(self.calls.get() + 1);
        0x12345678
    }
    fn set_u8(&mut self, _: u32, _: u8) {
        self.calls.set(self.calls.get() + 1);
    }
    fn set_u16(&mut self, _: u32, _: u16) {
        self.calls.set(self.calls.get() + 1);
    }
    fn set_u32(&mut self, _: u32, _: u32) {
        self.calls.set(self.calls.get() + 1);
    }
}

#[test]
fn flat_view_permissions_fault_recovery_alias_and_retirement() {
    let mut memory = Watched {
        table: PageTable::new_flat().unwrap(),
        calls: Cell::new(0),
    };
    let base = memory.flat_base();
    let pt = memory.page_table();
    let m: &mut dyn Mem = &mut memory;
    // All six marked fault sites return to their original trait method.
    assert_eq!(m.flat_u8(base, pt, PAGE), 0x12);
    assert_eq!(m.flat_u16(base, pt, PAGE), 0x1234);
    assert_eq!(m.flat_u32(base, pt, PAGE), 0x12345678);
    m.flat_set_u8(base, pt, PAGE, 1);
    m.flat_set_u16(base, pt, PAGE, 2);
    m.flat_set_u32(base, pt, PAGE, 3);
    assert_eq!(memory.calls.get(), 6);
    let alias = memory.table.backed_page(1);
    for perm in [0, READ, WRITE, READ | WRITE] {
        memory.table.set_perm(1, perm);
        // Privileged alias never changes the guest view's permissions.
        unsafe {
            mem::store_at(alias, 4, 0xaabbccdd);
        }
        memory.calls.set(0);
        let m: &mut dyn Mem = &mut memory;
        let got = m.flat_u32(base, pt, PAGE);
        assert_eq!(got, if perm & READ != 0 { 0xaabbccdd } else { 0x12345678 });
        m.flat_set_u32(base, pt, PAGE, 0x87654321);
        // Write-only uses the checked path, because Windows has no such mode.
        assert_eq!(
            memory.calls.get(),
            usize::from(perm & READ == 0) + usize::from(perm != 3)
        );
    }
    memory.calls.set(0);
    let m: &mut dyn Mem = &mut memory;
    m.flat_set_u8(base, pt, PAGE + 1, 0xfe);
    assert_eq!(m.flat_u8(base, pt, PAGE + 1), 0xfe);
    m.flat_set_u16(base, pt, PAGE + 3, 0xabcd);
    assert_eq!(m.flat_u16(base, pt, PAGE + 3), 0xabcd);
    m.flat_o_set_u32(base, PAGE, 0x87654321);
    assert_eq!(m.flat_o_u32(base, PAGE), 0x87654321);
    assert_eq!(memory.calls.get(), 0);
    memory.table.unback(1);
    memory.table.set_perm(1, 3);
    let fresh = memory.table.backed_page(1);
    assert_ne!(alias, fresh);
    assert_eq!(unsafe { mem::load_at(alias, 4) }, 0x87654321);
    assert_eq!(unsafe { mem::load_at(fresh, 4) }, 0);
    memory.calls.set(0);
    assert_eq!((&memory as &dyn Mem).flat_u32(base, pt, PAGE), 0x12345678);
    assert_eq!(memory.calls.get(), 1);
}

#[test]
fn ordinary_fixture_does_not_opt_in() {
    let mut m = mem::Image {
        base: 0,
        bytes: vec![0; 8],
    };
    let m: &mut dyn Mem = &mut m;
    assert!(m.flat_base().is_none());
    m.flat_set_u32(None, None, 1, 0x12345678);
    assert_eq!(m.flat_u32(None, None, 1), 0x12345678);
}
