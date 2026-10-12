//! `MtDTI`: the engine's run-time type descriptors (docs/DTI.md,
//! docs/CORE.md). Every MtObject class has one; they form a tree by parent
//! and are found by name or by id (`hash(name) & 0x7FFFFFFF`, see
//! [`crate::hash`]) through 256 hash buckets.

use core::mem::offset_of;
use ddnative_base::mem::Mem;

/// The descriptor's layout (0x20 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct DtiLayout {
    /// +0x00: the descriptor's own vtable (slot 1 creates an instance)
    pub vtable: u32,
    /// +0x04: the class name (a C string)
    pub name: u32,
    /// +0x08: the next sibling under the same parent
    pub next: u32,
    /// +0x0C: the first child class
    pub child: u32,
    /// +0x10: the parent class's descriptor
    pub parent: u32,
    /// +0x14: the next descriptor in the same hash bucket
    pub chain: u32,
    /// +0x18: the instance size (bits 0..22), the allocator index (bits
    /// 23..28) and flags (bits 29..31)
    pub size_flags: u32,
    /// +0x1C: the id
    pub id: u32,
}

/// The allocator index's bits in `size_flags`.
pub const ALLOCATOR_MASK: u32 = 0x1F80_0000;
/// The allocator index's position in `size_flags`.
pub const ALLOCATOR_SHIFT: u32 = 23;

/// The table of allocator objects a descriptor's index selects (64
/// entries, 0x01876628).
pub const ALLOCATORS: u32 = 0x0187_6628;
/// The 256 hash buckets of descriptors, indexed by `id & 0xFF`
/// (0x018768A0).
pub const BUCKETS: u32 = 0x0187_68A0;

/// A descriptor in guest memory (its address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dti(pub u32);

macro_rules! field {
    ($get:ident, $field:ident) => {
        pub fn $get(self, m: &dyn Mem) -> u32 {
            m.u32(self.0.wrapping_add(offset_of!(DtiLayout, $field) as u32))
        }
    };
}

impl Dti {
    field!(name, name);
    field!(next_raw, next);
    field!(child_raw, child);
    field!(parent_raw, parent);
    field!(chain_raw, chain);
    field!(size_flags, size_flags);
    field!(id, id);

    pub fn set_size_flags(self, m: &mut dyn Mem, v: u32) {
        m.set_u32(self.0.wrapping_add(offset_of!(DtiLayout, size_flags) as u32), v);
    }
    /// The first child, if any.
    pub fn child(self, m: &dyn Mem) -> Option<Dti> {
        non_null(self.child_raw(m))
    }
    /// The next sibling, if any.
    pub fn next(self, m: &dyn Mem) -> Option<Dti> {
        non_null(self.next_raw(m))
    }
    /// The allocator index (bits 23..28 of `size_flags`).
    pub fn allocator_index(self, m: &dyn Mem) -> u32 {
        (self.size_flags(m) >> ALLOCATOR_SHIFT) & 0x3F
    }
}

fn non_null(a: u32) -> Option<Dti> {
    (a != 0).then_some(Dti(a))
}

/// The allocator object instances of `dti`'s class come from: entry
/// `allocator_index` of [`ALLOCATORS`]. Also returns the index (ECX in the
/// original).
// Translated from 0x00CF5E20 (DDDA.exe 2017-12-13 Steam), MtDTI::allocator.
pub fn allocator_of(m: &dyn Mem, dti: Dti) -> (u32, u32) {
    let index = dti.allocator_index(m);
    (m.u32(ALLOCATORS + 4 * index), index)
}

/// The descriptor with this id: bucket `id & 0xFF`, then its chain.
// Translated from 0x00CF55B0 (DDDA.exe 2017-12-13 Steam), MtDTI::find(id).
pub fn find_by_id(m: &dyn Mem, id: u32) -> Option<Dti> {
    let mut d = m.u32(BUCKETS + 4 * (id & 0xFF));
    while d != 0 {
        if Dti(d).id(m) == id {
            return Some(Dti(d));
        }
        d = Dti(d).chain_raw(m);
    }
    None
}

/// How the original compares two names (a `strcmp` unrolled by two): the
/// sign of the first differing byte (unsigned), and where the loop left
/// its pointers and its last byte (the ABI adapters need those).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameCompare {
    /// 0 equal, 1 if `a` sorts after `b`, -1 before.
    pub order: i32,
    pub a_end: u32,
    pub b_end: u32,
    /// The last byte of `a` the loop loaded.
    pub last: u8,
}

/// Compares the C strings at `a` and `b` as the original does: two bytes
/// per round, stopping at the first difference or at the end of `a`.
pub fn compare_names(m: &dyn Mem, a: u32, b: u32) -> NameCompare {
    let (mut pa, mut pb) = (a, b);
    loop {
        for k in 0..2u32 {
            let (x, y) = (m.u8(pa.wrapping_add(k)), m.u8(pb.wrapping_add(k)));
            if x != y {
                let order = if x < y { -1 } else { 1 };
                return NameCompare {
                    order,
                    a_end: pa,
                    b_end: pb,
                    last: x,
                };
            }
            if x == 0 {
                let (a_end, b_end) = if k == 0 {
                    (pa, pb)
                } else {
                    (pa.wrapping_add(2), pb.wrapping_add(2))
                };
                return NameCompare {
                    order: 0,
                    a_end,
                    b_end,
                    last: 0,
                };
            }
        }
        pa = pa.wrapping_add(2);
        pb = pb.wrapping_add(2);
    }
}

/// The first class named `name` below `parent` (depth first: each child,
/// then its subtree, then the next child). `descend` searches a child's
/// subtree; plain callers pass [`find_child_by_name_rec`]. `on_compare`
/// sees each name comparison (the ABI adapter's register state).
// Translated from 0x00CF55E0 (DDDA.exe 2017-12-13 Steam), MtDTI::findChild(name).
pub fn find_child_by_name(
    m: &mut dyn Mem,
    parent: Dti,
    name: u32,
    mut descend: impl FnMut(&mut dyn Mem, Dti) -> Option<Dti>,
    mut on_compare: impl FnMut(NameCompare),
) -> Option<Dti> {
    let mut c = parent.child(m);
    while let Some(child) = c {
        let cmp = compare_names(m, child.name(m), name);
        on_compare(cmp);
        if cmp.order == 0 {
            return Some(child);
        }
        if let Some(found) = descend(m, child) {
            return Some(found);
        }
        c = child.next(m);
    }
    None
}

/// [`find_child_by_name`] searching the whole subtree.
pub fn find_child_by_name_rec(m: &mut dyn Mem, parent: Dti, name: u32) -> Option<Dti> {
    find_child_by_name(m, parent, name, |m, c| find_child_by_name_rec(m, c, name), |_| {})
}

/// The class named `name` in the tree under `root` (`root` itself
/// included); `None` for a null name. `search` looks below `root` (plain
/// callers: [`find_child_by_name_rec`]).
// Translated from 0x00CF5870 (DDDA.exe 2017-12-13 Steam), MtDTI::find(name).
pub fn find_by_name(
    m: &mut dyn Mem,
    name: u32,
    root: Dti,
    search: impl FnOnce(&mut dyn Mem, Dti) -> Option<Dti>,
    on_compare: impl FnOnce(NameCompare),
) -> Option<Dti> {
    if name == 0 {
        return None;
    }
    let root_name = root.name(m);
    if root_name != 0 {
        let cmp = compare_names(m, root_name, name);
        on_compare(cmp);
        if cmp.order == 0 {
            return Some(root);
        }
    }
    search(m, root)
}

/// Gives every class below `dti` the allocator index of `dti` (bits 23..28
/// of `size_flags`), depth first. `descend(child, changed)` handles a
/// child's subtree; `changed` is the bits just flipped in the child (EAX in
/// the original). Plain callers: [`propagate_allocator_rec`].
// Translated from 0x00CF5750 (DDDA.exe 2017-12-13 Steam), MtDTI::propagateAllocator.
pub fn propagate_allocator(m: &mut dyn Mem, dti: Dti, mut descend: impl FnMut(&mut dyn Mem, Dti, u32)) {
    let mut c = dti.child(m);
    while let Some(child) = c {
        let changed = (child.size_flags(m) ^ dti.size_flags(m)) & ALLOCATOR_MASK;
        let flags = child.size_flags(m) ^ changed;
        child.set_size_flags(m, flags);
        descend(m, child, changed);
        c = child.next(m);
    }
}

pub fn propagate_allocator_rec(m: &mut dyn Mem, dti: Dti) {
    propagate_allocator(m, dti, |m, c, _| propagate_allocator_rec(m, c))
}

/// Sets `dti`'s allocator index and gives it to all classes below.
/// Returns the masked xor it applied (EAX in the original).
// Translated from 0x00CF5E40 (DDDA.exe 2017-12-13 Steam), MtDTI::setAllocator.
pub fn set_allocator(
    m: &mut dyn Mem,
    dti: Dti,
    index: u32,
    propagate: impl FnOnce(&mut dyn Mem, Dti),
) -> u32 {
    let mixed = ((index << ALLOCATOR_SHIFT) ^ dti.size_flags(m)) & ALLOCATOR_MASK;
    let flags = dti.size_flags(m) ^ mixed;
    dti.set_size_flags(m, flags);
    propagate(m, dti);
    mixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_original() {
        assert_eq!(core::mem::size_of::<DtiLayout>(), 0x20);
        assert_eq!(offset_of!(DtiLayout, size_flags), 0x18);
        assert_eq!(offset_of!(DtiLayout, id), 0x1C);
    }
}
