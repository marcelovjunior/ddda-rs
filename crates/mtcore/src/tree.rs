//! MSVC's red-black tree (`std::_Tree`, the node-based `std::set` and
//! `std::map` of the 2010-era library DDDA.exe links) over guest memory.
//!
//! A node: `_Left` +0x0, `_Parent` +0x4, `_Right` +0x8, the key +0xC,
//! `_Color` +0x10, `_Isnil` +0x11. The tree object holds `_Myhead` at +0x4:
//! a sentinel node with `_Isnil` set, whose `_Parent` is the root.
use ddnative_base::mem::Mem;

const LEFT: u32 = 0x0;
const PARENT: u32 = 0x4;
const RIGHT: u32 = 0x8;
const KEY: u32 = 0xC;
const ISNIL: u32 = 0x11;

/// The nodes `equal_range` walks, read as the original reads them.
struct Nodes<'a> {
    m: &'a dyn Mem,
    pt: Option<std::ptr::NonNull<ddnative_base::mem::PageTable>>,
}

impl Nodes<'_> {
    fn u32(&self, a: u32) -> u32 {
        self.m.fast_u32(self.pt, a)
    }
    fn nil(&self, node: u32) -> bool {
        self.m.fast_u8(self.pt, node.wrapping_add(ISNIL)) != 0
    }
}

/// What 0x00DB82C0 computes and leaves: the range, and the registers the
/// walk leaves behind (ECX is the key once the second walk ran).
pub struct EqualRange {
    pub lower: u32,
    pub upper: u32,
    pub second_walk: bool,
    pub key: u32,
}

/// `equal_range(key)` of a tree of u32 keys (0x00DB82C0, the QLOC
/// "Alive set" of live objects: every tracked object's destructor erases
/// itself through it). The lower bound walk also narrows the upper bound
/// while it can (the first node above the key seen while the upper bound
/// is still the head); the upper bound walk then starts from that node's
/// left child, or from the root. Reads in the original's order.
pub fn equal_range(m: &dyn Mem, tree: u32, key_ptr: u32) -> EqualRange {
    let n = Nodes { m, pt: m.page_table() };
    let head = n.u32(tree.wrapping_add(4));
    let mut node = n.u32(head.wrapping_add(PARENT));
    let (mut lower, mut upper) = (head, head);
    if !n.nil(node) {
        let key = n.u32(key_ptr);
        loop {
            let k = n.u32(node.wrapping_add(KEY));
            if k < key {
                node = n.u32(node.wrapping_add(RIGHT));
            } else {
                if n.nil(upper) && key < k {
                    upper = node;
                }
                lower = node;
                node = n.u32(node.wrapping_add(LEFT));
            }
            if n.nil(node) {
                break;
            }
        }
    }
    let mut node = if n.nil(upper) {
        n.u32(n.u32(tree.wrapping_add(4)).wrapping_add(PARENT))
    } else {
        n.u32(upper.wrapping_add(LEFT))
    };
    let mut r = EqualRange { lower, upper, second_walk: false, key: 0 };
    if !n.nil(node) {
        let key = n.u32(key_ptr);
        (r.second_walk, r.key) = (true, key);
        loop {
            if key < n.u32(node.wrapping_add(KEY)) {
                r.upper = node;
                node = n.u32(node.wrapping_add(LEFT));
            } else {
                node = n.u32(node.wrapping_add(RIGHT));
            }
            if n.nil(node) {
                break;
            }
        }
    }
    r
}
