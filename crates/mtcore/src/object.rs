//! `MtObject`, the root class: its destructor body and its allocation.

use ddnative_base::mem::Mem;

/// MtObject's vtable (0x01558134). A destructor resets the object to it
/// after the derived class's part is torn down.
pub const VTABLE: u32 = 0x0155_8134;

/// The allocator `MtObject::operator new` uses: entry 1 of
/// `dti::ALLOCATORS` (0x0187662C).
pub const NEW_ALLOCATOR: u32 = 0x0187_662C;

/// The allocator vtable slots: allocate `(size, alignment)` at +0x20, free
/// `(block)` at +0x24 (both thiscall, popping their arguments).
pub const ALLOC_SLOT: u32 = 0x20;
pub const FREE_SLOT: u32 = 0x24;

/// MtObject's destructor body: the object becomes a plain MtObject.
// Translated from 0x004038F0 (DDDA.exe 2017-12-13 Steam), MtObject::~MtObject.
pub fn destroy(m: &mut dyn Mem, this: u32) {
    m.set_u32(this, VTABLE);
}

/// `MtObject::operator new(size)`: `size` bytes, 16-aligned, from the
/// allocator at [`NEW_ALLOCATOR`]. `alloc(allocator, size, alignment)` is
/// that allocator's virtual allocate (slot [`ALLOC_SLOT`]).
// Translated from 0x00CF5C00 (DDDA.exe 2017-12-13 Steam), MtObject::operator new.
pub fn new(m: &mut dyn Mem, size: u32, alloc: impl FnOnce(&mut dyn Mem, u32, u32, u32) -> u32) -> u32 {
    let allocator = m.u32(NEW_ALLOCATOR);
    alloc(m, allocator, size, 0x10)
}
