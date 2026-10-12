//! A byte-addressed view of the original's objects, for ports of code that
//! follows pointers between objects. A port over `Mem` performs the same
//! reads and writes, at the same addresses, as the original; the
//! differential tests run it over the byte image the original ran on and
//! compare the whole result. (docs/NATIVE_PIPELINE.md, "Memory-model ports")
pub trait Mem {
    /// Preflight an instruction access without reading or publishing bytes.
    /// Returns the first inaccessible byte. Runtime adapters must forward
    /// this hook; legacy vector memories deliberately do not opt in.
    fn probe(&self, _addr: u32, _width: u32, _write: bool) -> Result<(), u32> {
        panic!("this Mem does not support restartable instructions")
    }
    fn u8(&self, addr: u32) -> u8;
    fn set_u8(&mut self, addr: u32, v: u8);
    fn u16(&self, addr: u32) -> u16 {
        u16::from_le_bytes([0, 1].map(|i| self.u8(addr.wrapping_add(i))))
    }
    fn u32(&self, addr: u32) -> u32 {
        u32::from_le_bytes([0, 1, 2, 3].map(|i| self.u8(addr.wrapping_add(i))))
    }
    fn set_u32(&mut self, addr: u32, v: u32) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.set_u8(addr.wrapping_add(i as u32), b);
        }
    }
    fn set_u16(&mut self, addr: u32, v: u16) {
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.set_u8(addr.wrapping_add(i as u32), b);
        }
    }
    /// `out.len()` bytes from `addr` (wrapping), exactly as reading each
    /// byte with [`u8`](Self::u8) would; implementations may copy in bulk.
    fn read_bytes(&self, addr: u32, out: &mut [u8]) {
        for (k, b) in out.iter_mut().enumerate() {
            *b = self.u8(addr.wrapping_add(k as u32));
        }
    }
    /// A fresh block of `size` bytes (the platform allocator models in
    /// crate::platform call this instead of the game's allocators).
    fn alloc(&mut self, size: u32) -> u32 {
        let _ = size;
        panic!("this Mem has no allocator")
    }
    /// The page table behind this memory, for the lifted code's inline
    /// fast path (`fast_u32` and the others on `dyn Mem`); None (the
    /// default) sends every access through the trait.
    /// An implementation returning Some guarantees the table outlives every
    /// lifted call made with it and never moves, and that an access the
    /// table allows (page backed, permission bit set) means exactly the
    /// bytes in the table.
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        None
    }
}

/// Guest pages: 4 KiB, 2^20 of them.
pub const PAGE: u32 = 0x1000;
pub const PAGES: usize = 1 << 20;
/// Page permission bits in [`PageTable::perms`].
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;

/// One page of guest memory.
pub type Page = Box<[u8; PAGE as usize]>;

/// A software page table (the runtime's guest address space keeps its
/// pages here): per page, its bytes if backed and its permission bits.
/// Fixed-size tables, so a page index (`addr / PAGE`) needs no bounds
/// check and the tables never move.
pub struct PageTable {
    pub pages: Box<[Option<Page>; PAGES]>,
    pub perms: Box<[u8; PAGES]>,
}

impl Default for PageTable {
    fn default() -> Self {
        let pages: Box<[Option<Page>]> = (0..PAGES).map(|_| None).collect();
        let perms: Box<[u8]> = vec![0; PAGES].into_boxed_slice();
        PageTable {
            pages: pages.try_into().unwrap_or_else(|_| unreachable!()),
            perms: perms.try_into().unwrap_or_else(|_| unreachable!()),
        }
    }
}

/// A zero-filled page.
pub fn zero_page() -> Page {
    vec![0u8; PAGE as usize].into_boxed_slice().try_into().expect("a page")
}

/// The bytes of an access the table allows: within one page, backed, with
/// `perm` set. None sends the access through the trait (lazy or unmapped
/// pages, page-crossing accesses, faults).
#[inline(always)]
fn allowed(pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, width: usize, perm: u8) -> Option<*mut u8> {
    let t = pt?;
    let off = (addr % PAGE) as usize;
    if off + width > PAGE as usize {
        return None;
    }
    let p = (addr / PAGE) as usize;
    // SAFETY: page_table()'s contract: the table is alive and does not
    // move; no reference into it is held across this access.
    unsafe {
        let t = t.as_ptr();
        if (*t).perms[p] & perm == 0 {
            return None;
        }
        let page = (*t).pages[p].as_mut()?;
        Some(page.as_mut_ptr().add(off))
    }
}

/// The lifted code's memory accesses (lift.py lowers `m.u32(a)` to
/// `m.fast_u32(pt, a)` with `pt = m.page_table()` read at function
/// entry): the table's bytes when it allows the access, else the trait
/// method, so faults, lazy pages and fixtures behave exactly as through
/// the trait.
// `#[inline]`, not `inline(always)`: release builds inline these anyway, and
// in debug builds (opt-level 0) `inline(always)` copied the page-table check
// into every one of the lifted code's memory accesses, making the shard
// crates four times larger and the runtime examples too big to link.
impl dyn Mem + '_ {
    #[inline]
    pub fn fast_u8(&self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32) -> u8 {
        match allowed(pt, addr, 1, READ) {
            // SAFETY: allowed() points at 1 byte inside a backed page
            Some(p) => unsafe { *p },
            None => self.u8(addr),
        }
    }
    #[inline]
    pub fn fast_u16(&self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32) -> u16 {
        match allowed(pt, addr, 2, READ) {
            // SAFETY: 2 bytes inside a backed page
            Some(p) => u16::from_le_bytes(unsafe { p.cast::<[u8; 2]>().read() }),
            None => self.u16(addr),
        }
    }
    #[inline]
    pub fn fast_u32(&self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32) -> u32 {
        match allowed(pt, addr, 4, READ) {
            // SAFETY: 4 bytes inside a backed page
            Some(p) => u32::from_le_bytes(unsafe { p.cast::<[u8; 4]>().read() }),
            None => self.u32(addr),
        }
    }
    #[inline]
    pub fn fast_set_u8(&mut self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, v: u8) {
        match allowed(pt, addr, 1, WRITE) {
            // SAFETY: 1 byte inside a backed page
            Some(p) => unsafe { *p = v },
            None => self.set_u8(addr, v),
        }
    }
    #[inline]
    pub fn fast_set_u16(&mut self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, v: u16) {
        match allowed(pt, addr, 2, WRITE) {
            // SAFETY: 2 bytes inside a backed page
            Some(p) => unsafe { p.cast::<[u8; 2]>().write(v.to_le_bytes()) },
            None => self.set_u16(addr, v),
        }
    }
    #[inline]
    pub fn fast_set_u32(&mut self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, v: u32) {
        match allowed(pt, addr, 4, WRITE) {
            // SAFETY: 4 bytes inside a backed page
            Some(p) => unsafe { p.cast::<[u8; 4]>().write(v.to_le_bytes()) },
            None => self.set_u32(addr, v),
        }
    }
}

/// Where Sparse hands out fresh blocks: research/native/harness.py uses the
/// same base and rounding, so a replay gets the addresses the vector saw.
pub const HEAP_BASE: u32 = 0x0C00_0000;

/// A Mem over one contiguous image starting at `base` (tests, tools).
pub struct Image {
    pub base: u32,
    pub bytes: Vec<u8>,
}

impl Mem for Image {
    fn u8(&self, addr: u32) -> u8 {
        self.bytes[(addr - self.base) as usize]
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        let i = (addr - self.base) as usize;
        self.bytes[i] = v;
    }
}

/// A Mem holding only the bytes a vector recorded: reading any other byte
/// panics (the port read something the original did not).
#[derive(Default)]
pub struct Sparse {
    pub bytes: std::collections::HashMap<u32, u8>,
    pub written: std::collections::BTreeMap<u32, u8>,
    /// Next fresh block (0 = HEAP_BASE): a bump allocator, 16-byte steps.
    pub heap: u32,
}

impl Mem for Sparse {
    fn u8(&self, addr: u32) -> u8 {
        if let Some(v) = self.written.get(&addr) {
            return *v;
        }
        *self.bytes.get(&addr).unwrap_or_else(|| panic!("read of {addr:#010x}, which the original did not read"))
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        self.written.insert(addr, v);
    }
    fn alloc(&mut self, size: u32) -> u32 {
        if self.heap == 0 {
            self.heap = HEAP_BASE;
        }
        let a = self.heap;
        self.heap = self.heap.wrapping_add(size.max(1)).wrapping_add(15) & !15;
        a
    }
}
