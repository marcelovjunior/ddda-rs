//! `MtPropertyList`: the list of property descriptions an object fills in
//! for the engine's reflection (`getProperties`); every class adds its
//! fields with [`add`], which makes it the second most-called core
//! function after the DTI allocator lookup.

use core::mem::offset_of;
use ddnative_base::mem::Mem;

/// The list (8 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ListLayout {
    /// +0x00
    pub vtable: u32,
    /// +0x04: the most recently added node
    pub head: u32,
}

/// A list node (0x28 bytes, from the global node pool): the property's
/// 32 bytes, copied verbatim, then the links.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct NodeLayout {
    /// +0x00: the property (name, type, offset, accessors...)
    pub property: [u32; 8],
    /// +0x20: the node added after this one (toward the head)
    pub prev: u32,
    /// +0x24: the node added before this one
    pub next: u32,
}

/// MtPropertyList's vtable (0x01406E74).
pub const VTABLE: u32 = 0x0140_6E74;
/// The head of the global free list of nodes (0x01877DA0).
pub const FREE_LIST: u32 = 0x0187_7DA0;
/// The critical section around the node pool (0x018C7DA8).
pub const POOL_LOCK: u32 = 0x018C_7DA8;

const PREV: u32 = offset_of!(NodeLayout, prev) as u32;
const NEXT: u32 = offset_of!(NodeLayout, next) as u32;
const HEAD: u32 = offset_of!(ListLayout, head) as u32;

/// An empty list.
// Translated from 0x00CF7340 (DDDA.exe 2017-12-13 Steam), MtPropertyList::MtPropertyList.
pub fn construct(m: &mut dyn Mem, this: u32) {
    m.set_u32(this, VTABLE);
    m.set_u32(this.wrapping_add(HEAD), 0);
}

/// Adds a copy of the property at `property` (a whole 0x28-byte node:
/// the description and its own links, which are then reset) at the head
/// of the list, in a node from `alloc_node` (the node pool, 0x00CF73F0),
/// and returns the node.
// Translated from 0x004027A0 (DDDA.exe 2017-12-13 Steam), MtPropertyList::add.
pub fn add(m: &mut dyn Mem, this: u32, property: u32, alloc_node: impl FnOnce(&mut dyn Mem) -> u32) -> u32 {
    let node = alloc_node(m);
    for k in 0..10u32 {
        // the original copies five quadwords: the property and the links
        m.set_u32(node.wrapping_add(4 * k), m.u32(property.wrapping_add(4 * k)));
    }
    m.set_u32(node.wrapping_add(PREV), 0);
    m.set_u32(node.wrapping_add(NEXT), 0);
    let head = m.u32(this.wrapping_add(HEAD));
    if head != 0 {
        m.set_u32(head.wrapping_add(PREV), node);
        m.set_u32(node.wrapping_add(NEXT), m.u32(this.wrapping_add(HEAD)));
    }
    m.set_u32(this.wrapping_add(HEAD), node);
    node
}

/// Returns every node to the pool's free list (each pushed at its front)
/// and empties the list, inside `lock`/`unlock` (EnterCriticalSection /
/// LeaveCriticalSection on [`POOL_LOCK`]).
// Translated from 0x00CF74E0 (DDDA.exe 2017-12-13 Steam), MtPropertyList::clear.
pub fn clear(m: &mut dyn Mem, this: u32, lock: impl FnOnce(&mut dyn Mem), unlock: impl FnOnce(&mut dyn Mem)) {
    lock(m);
    return_nodes(m, this);
    unlock(m);
}

/// The part of [`clear`] inside the lock; returns the last node it moved.
pub fn return_nodes(m: &mut dyn Mem, this: u32) -> Option<u32> {
    let mut node = m.u32(this.wrapping_add(HEAD));
    let mut last = None;
    while node != 0 {
        last = Some(node);
        let next = m.u32(node.wrapping_add(NEXT));
        m.set_u32(node.wrapping_add(PREV), 0);
        let free = m.u32(FREE_LIST);
        if free != 0 {
            m.set_u32(node.wrapping_add(NEXT), free);
            m.set_u32(m.u32(FREE_LIST).wrapping_add(PREV), node);
        } else {
            m.set_u32(node.wrapping_add(NEXT), 0);
        }
        m.set_u32(FREE_LIST, node);
        node = next;
    }
    m.set_u32(this.wrapping_add(HEAD), 0);
    last
}

/// MtPropertyList's destructor body: clears the list and leaves a plain
/// MtObject.
// Translated from 0x00CF76C0 (DDDA.exe 2017-12-13 Steam), MtPropertyList::~MtPropertyList.
pub fn destroy(m: &mut dyn Mem, this: u32, clear: impl FnOnce(&mut dyn Mem, u32)) {
    m.set_u32(this, VTABLE);
    clear(m, this);
    m.set_u32(this, crate::object::VTABLE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_original() {
        assert_eq!(core::mem::size_of::<NodeLayout>(), 0x28);
        assert_eq!(PREV, 0x20);
        assert_eq!(NEXT, 0x24);
        assert_eq!(HEAD, 4);
    }
}
