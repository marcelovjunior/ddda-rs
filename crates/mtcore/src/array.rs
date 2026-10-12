//! `MtArray`: the engine's growable array of object pointers.

use core::mem::offset_of;
use ddnative_base::mem::Mem;

/// MtArray's layout (0x14 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ArrayLayout {
    /// +0x00
    pub vtable: u32,
    /// +0x04: elements in use
    pub count: u32,
    /// +0x08: elements allocated
    pub capacity: u32,
    /// +0x0C: whether the array deletes its elements when cleared or
    /// destroyed
    pub auto_delete: u8,
    pub _pad: [u8; 3],
    /// +0x10: the element pointers
    pub data: u32,
}

/// MtArray's vtable (0x01558298).
pub const VTABLE: u32 = 0x0155_8298;
/// MtArray's DTI (0x01877D24): its allocator holds the element storage.
pub const DTI: u32 = 0x0187_7D24;

/// An MtArray in guest memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Array(pub u32);

macro_rules! field {
    ($get:ident, $set:ident, $field:ident) => {
        pub fn $get(self, m: &dyn Mem) -> u32 {
            m.u32(self.0.wrapping_add(offset_of!(ArrayLayout, $field) as u32))
        }
        pub fn $set(self, m: &mut dyn Mem, v: u32) {
            m.set_u32(
                self.0.wrapping_add(offset_of!(ArrayLayout, $field) as u32),
                v,
            )
        }
    };
}

impl Array {
    field!(vtable, set_vtable, vtable);
    field!(count, set_count, count);
    field!(capacity, set_capacity, capacity);
    field!(data, set_data, data);

    pub fn auto_delete(self, m: &dyn Mem) -> bool {
        m.u8(self.0.wrapping_add(offset_of!(ArrayLayout, auto_delete) as u32)) != 0
    }
    /// Element `i` (no bounds check, as the original).
    pub fn get(self, m: &dyn Mem, i: u32) -> u32 {
        m.u32(self.data(m).wrapping_add(4 * i))
    }
}

/// Empties the array: `slot(index, element)` is called for every slot,
/// and the original deletes each non-null element there (its virtual
/// deleting destructor, slot 0, with argument 1); then, with
/// `free_storage`, frees the storage (`free(data)`, the array allocator's
/// free; skipped for null storage) and zeroes data and capacity. The count
/// and the storage are re-read every round, as the original does (a
/// destructor may change them).
// Translated from 0x004025C0 (DDDA.exe 2017-12-13 Steam), MtArray::clear.
pub fn clear(
    m: &mut dyn Mem,
    a: Array,
    free_storage: bool,
    mut slot: impl FnMut(&mut dyn Mem, u32, Option<u32>),
    free: impl FnOnce(&mut dyn Mem, u32),
) {
    let mut i = 0;
    while i < a.count(m) {
        let e = a.get(m, i);
        slot(m, i, (e != 0).then_some(e));
        i += 1;
    }
    if free_storage {
        let data = a.data(m);
        if data != 0 {
            free(m, data);
        }
        a.set_data(m, 0);
        a.set_capacity(m, 0);
    }
    a.set_count(m, 0);
}

/// Reallocates the storage for `capacity` elements: a zeroed block of
/// `capacity * 4` bytes (`alloc(size, 16)`), the old elements copied over
/// (`memcpy`), the old block freed (also when null), then data and
/// capacity set. The count is unchanged; nothing checks that it fits.
// Translated from 0x00402620 (DDDA.exe 2017-12-13 Steam), MtArray::reserve.
pub fn reserve(
    m: &mut dyn Mem,
    a: Array,
    capacity: u32,
    alloc: impl FnOnce(&mut dyn Mem, u32, u32) -> u32,
    memset: impl FnOnce(&mut dyn Mem, u32, u32, u32),
    memcpy: impl FnOnce(&mut dyn Mem, u32, u32, u32),
    free: impl FnOnce(&mut dyn Mem, u32),
) {
    let size = capacity.wrapping_mul(4);
    let block = alloc(m, size, 0x10);
    memset(m, block, 0, size);
    let (count, data) = (a.count(m), a.data(m));
    memcpy(m, block, data, count.wrapping_mul(4));
    free(m, data);
    a.set_data(m, block);
    a.set_capacity(m, capacity);
}

/// MtArray's destructor body: clears with storage if the array owns its
/// elements, frees any remaining storage, zeroes the fields and leaves a
/// plain MtObject.
// Translated from 0x00402530 (DDDA.exe 2017-12-13 Steam), MtArray::~MtArray.
pub fn destroy(
    m: &mut dyn Mem,
    a: Array,
    clear_owned: impl FnOnce(&mut dyn Mem, Array),
    free: impl FnOnce(&mut dyn Mem, u32),
) {
    let owns = a.auto_delete(m);
    a.set_vtable(m, VTABLE);
    if owns {
        clear_owned(m, a);
    }
    let data = a.data(m);
    if data != 0 {
        free(m, data);
    }
    a.set_data(m, 0);
    a.set_capacity(m, 0);
    a.set_count(m, 0);
    a.set_vtable(m, crate::object::VTABLE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_original() {
        assert_eq!(core::mem::size_of::<ArrayLayout>(), 0x14);
        assert_eq!(offset_of!(ArrayLayout, auto_delete), 0x0C);
        assert_eq!(offset_of!(ArrayLayout, data), 0x10);
    }
}
