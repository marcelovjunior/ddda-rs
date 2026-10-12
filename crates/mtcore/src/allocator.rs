//! The registry of allocator objects: a table of up to N allocators at
//! 0x01876730 (slot 0 unused), its used length at 0x01876834, and each
//! allocator's own slot number at +0x58.

use ddnative_base::mem::Mem;

/// The table of registered allocators (0x01876730).
pub const REGISTRY: u32 = 0x0187_6730;
/// The table's used length (slot 0 included; 0x01876834).
pub const REGISTRY_LEN: u32 = 0x0187_6834;
/// An allocator's slot number in the table.
pub const SLOT_FIELD: u32 = 0x58;

fn slot(i: u32) -> u32 {
    REGISTRY.wrapping_add(4 * i)
}

/// Where [`register`] put the allocator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registered {
    /// In a free slot (1..len).
    Reused(u32),
    /// Appended; the table grew.
    Appended(u32),
}

/// Registers `allocator`: the first free slot from 1 on, else a new slot
/// at the end (no bound check). Its slot number goes to +0x58.
// Translated from 0x00CF5450 (DDDA.exe 2017-12-13 Steam), MtAllocator registry add.
pub fn register(m: &mut dyn Mem, allocator: u32) -> Registered {
    let len = m.u32(REGISTRY_LEN);
    let mut i = 1;
    while i < len {
        if m.u32(slot(i)) == 0 {
            m.set_u32(allocator.wrapping_add(SLOT_FIELD), i);
            m.set_u32(slot(i), allocator);
            return Registered::Reused(i);
        }
        i += 1;
    }
    m.set_u32(allocator.wrapping_add(SLOT_FIELD), len);
    let len = m.u32(REGISTRY_LEN);
    m.set_u32(slot(len), allocator);
    m.set_u32(REGISTRY_LEN, len.wrapping_add(1));
    Registered::Appended(len)
}

/// Unregisters `allocator`: clears the first slot (from 1 on) that holds
/// it. Returns the slot index where the search stopped.
// Translated from 0x00CF54A0 (DDDA.exe 2017-12-13 Steam), MtAllocator registry remove.
pub fn unregister(m: &mut dyn Mem, allocator: u32) -> u32 {
    let len = m.u32(REGISTRY_LEN);
    let mut i = 1;
    while i < len {
        if m.u32(slot(i)) == allocator {
            m.set_u32(slot(i), 0);
            return i;
        }
        i += 1;
    }
    i
}
