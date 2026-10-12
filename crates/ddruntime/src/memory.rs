//! The guest address space (docs/RUNTIME.md, "Memory, object sharing and
//! machine state").
//!
//! A software page table: 2^20 pages of 4 KiB, each either unmapped or a
//! zero-initialised buffer with read/write permissions. Every access is
//! checked; an access to an unmapped page or a write to a read-only page is
//! a [`Fault`] (raised as a panic through the `Mem` trait, whose accessors
//! cannot return errors). Addresses wrap at 0xFFFFFFFF like the CPU's, and
//! unaligned and page-crossing accesses are byte-exact little-endian.
//!
//! This is the checked, portable backing RUNTIME.md proposes to start with;
//! a reserved host arena can replace it later behind the same interface,
//! once measured. The guest's threads share one address space: each holds
//! its own [`GuestMemory`] handle ([`GuestMemory::share`]). The table and
//! the pages are atomic (ddnative-base's `PageTable`), so threads may
//! access them at the same time (crate::threads' parallel mode): every
//! guest load is an acquire load and every store a release store of the
//! accessed bytes, which on x86-64 are plain moves and give x86's own
//! (TSO) order for single accesses.
use ddnative_base::mem::{load_at, store_at, Mem, PageTable, PAGES};
use std::fmt;

/// Pages and permission bits: ddnative-base's, which the lifted code's
/// inline fast path reads (Mem::page_table).
pub use ddnative_base::mem::{PAGE, READ, WRITE};
/// Reserved but not yet backed: reads as zero, gets a buffer on first write.
const LAZY: u8 = 4;

/// An access the guest is not allowed to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub addr: u32,
    pub width: u32,
    pub write: bool,
    pub reason: &'static str,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "guest {} of {} byte(s) at {:#010x}: {}",
               if self.write { "write" } else { "read" }, self.width, self.addr, self.reason)
    }
}

/// A mapping request that overlaps memory already mapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlap {
    pub page: u32,
}

/// A handle to a guest address space: ddnative-base's page table
/// (fixed-size atomic tables, so a page index needs no bounds check and
/// the tables never move).
pub struct GuestMemory {
    space: std::sync::Arc<PageTable>,
}

impl Default for GuestMemory {
    fn default() -> Self {
        GuestMemory { space: std::sync::Arc::new(PageTable::default()) }
    }
}

impl GuestMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Another handle to the same address space (for another guest thread).
    pub fn share(&self) -> GuestMemory {
        GuestMemory { space: self.space.clone() }
    }

    fn s(&self) -> &PageTable {
        &self.space
    }

    /// Map [addr, addr + len) rounded out to whole pages, zero-filled, with
    /// `perm`. Mapping over an already mapped page is refused.
    pub fn map(&mut self, addr: u32, len: u32, perm: u8) -> Result<(), Overlap> {
        let pages = page_range(addr, len);
        if let Some(p) = pages.clone().find(|&p| self.mapped(p)) {
            return Err(Overlap { page: (p as u32) * PAGE });
        }
        for p in pages {
            self.s().backed_page(p);
            self.s().set_perm(p, perm);
        }
        Ok(())
    }

    /// Reserve [addr, addr + len) as zero pages that get host memory only
    /// when first written (heaps and stacks: large, mostly untouched).
    pub fn map_lazy(&mut self, addr: u32, len: u32, perm: u8) -> Result<(), Overlap> {
        let pages = page_range(addr, len);
        if let Some(p) = pages.clone().find(|&p| self.mapped(p)) {
            return Err(Overlap { page: (p as u32) * PAGE });
        }
        for p in pages {
            self.s().set_perm(p, perm | LAZY);
        }
        Ok(())
    }

    fn mapped(&self, p: usize) -> bool {
        !self.s().page(p).is_null() || self.s().perm(p) & LAZY != 0
    }

    /// Change the permissions of mapped pages in [addr, addr + len).
    pub fn protect(&mut self, addr: u32, len: u32, perm: u8) {
        for p in page_range(addr, len) {
            assert!(self.mapped(p), "protect of unmapped page {:#010x}", p as u32 * PAGE);
            let lazy = self.s().perm(p) & LAZY;
            self.s().set_perm(p, perm | lazy);
        }
    }

    /// Unmap the pages of [addr, addr + len) that are mapped, dropping their
    /// contents (VirtualFree's decommit: the pages fault again).
    pub fn unmap(&mut self, addr: u32, len: u32) {
        for p in page_range(addr, len) {
            self.s().unback(p);
            self.s().set_perm(p, 0);
        }
    }

    pub fn is_mapped(&self, addr: u32) -> bool {
        self.mapped((addr / PAGE) as usize)
    }

    pub fn perm(&self, addr: u32) -> u8 {
        self.s().perm((addr / PAGE) as usize) & (READ | WRITE)
    }

    /// Pages that have host memory behind them (diagnostics).
    pub fn backed_pages(&self) -> usize {
        self.s().backed_count()
    }

    /// Privileged copy into mapped pages, ignoring permissions (the loader
    /// filling read-only sections). Fails on unmapped pages.
    pub fn poke(&mut self, addr: u32, data: &[u8]) -> Result<(), Fault> {
        for (k, &b) in data.iter().enumerate() {
            let a = addr.wrapping_add(k as u32);
            if !self.mapped((a / PAGE) as usize) {
                return Err(Fault { addr: a, width: 1, write: true, reason: "unmapped page" });
            }
            self.set_byte(a, b);
        }
        Ok(())
    }

    /// Check an access of `width` bytes at `addr` (wrapping) without doing it.
    pub fn check(&self, addr: u32, width: u32, write: bool) -> Result<(), Fault> {
        for k in 0..width {
            let p = (addr.wrapping_add(k) / PAGE) as usize;
            let need = if write { WRITE } else { READ };
            let reason = if !self.mapped(p) {
                "unmapped page"
            } else if self.s().perm(p) & need == 0 {
                if write { "page is not writable" } else { "page is not readable" }
            } else {
                continue;
            };
            return Err(Fault { addr, width, write, reason });
        }
        Ok(())
    }

    /// Checked read of `len` bytes.
    pub fn read(&self, addr: u32, len: u32) -> Result<Vec<u8>, Fault> {
        self.check(addr, len, false)?;
        Ok((0..len).map(|k| self.byte(addr.wrapping_add(k))).collect())
    }

    fn byte(&self, a: u32) -> u8 {
        let page = self.s().page((a / PAGE) as usize);
        if page.is_null() {
            return 0; // a lazy page not written yet
        }
        // SAFETY: a backed page holds PAGE bytes
        unsafe { load_at(page.add((a % PAGE) as usize), 1) as u8 }
    }

    fn set_byte(&self, a: u32, v: u8) {
        let page = self.s().backed_page((a / PAGE) as usize);
        // SAFETY: as for `byte`
        unsafe { store_at(page.add((a % PAGE) as usize), 1, v as u32) }
    }

    /// A load of `width` (1, 2 or 4) bytes. Within one page that is
    /// readable: one permission test and one slice read. Everything else
    /// (page-crossing, faults) takes the checked byte-wise path.
    #[inline(always)]
    fn load(&self, addr: u32, width: u32) -> u32 {
        let off = (addr % PAGE) as usize;
        if off + width as usize <= PAGE as usize {
            let p = (addr / PAGE) as usize;
            let s = self.s();
            // READ is only ever set on mapped pages (map, map_lazy, protect)
            if s.perm(p) & READ != 0 {
                let page = s.page(p);
                if page.is_null() {
                    return 0; // lazy: zero until written
                }
                // SAFETY: width bytes inside a backed page
                return unsafe { load_at(page.add(off), width as usize) };
            }
        }
        self.load_checked(addr, width)
    }

    #[cold]
    #[inline(never)]
    fn load_checked(&self, addr: u32, width: u32) -> u32 {
        if let Err(f) = self.check(addr, width, false) {
            panic!("{f}");
        }
        let off = addr % PAGE;
        if off + width <= PAGE {
            // within one page: one access (a lazy page reads zero)
            let page = self.s().page((addr / PAGE) as usize);
            if page.is_null() {
                return 0;
            }
            // SAFETY: width bytes inside a backed page
            unsafe { load_at(page.add(off as usize), width as usize) }
        } else {
            (0..width).fold(0, |v, k| v | (self.byte(addr.wrapping_add(k)) as u32) << (8 * k))
        }
    }

    /// A store of `width` bytes; the fast path as for [`load`](Self::load).
    #[inline(always)]
    fn store(&mut self, addr: u32, width: u32, v: u32) {
        let off = (addr % PAGE) as usize;
        if off + width as usize <= PAGE as usize {
            let p = (addr / PAGE) as usize;
            let s = self.s();
            // WRITE is only ever set on mapped pages
            if s.perm(p) & WRITE != 0 {
                let page = s.backed_page(p);
                // SAFETY: width bytes inside a backed page
                unsafe { store_at(page.add(off), width as usize, v) };
                return;
            }
        }
        self.store_checked(addr, width, v)
    }

    #[cold]
    #[inline(never)]
    fn store_checked(&mut self, addr: u32, width: u32, v: u32) {
        if let Err(f) = self.check(addr, width, true) {
            panic!("{f}");
        }
        for k in 0..width {
            self.set_byte(addr.wrapping_add(k), (v >> (8 * k)) as u8);
        }
    }
}

fn page_range(addr: u32, len: u32) -> std::ops::Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let first = (addr / PAGE) as usize;
    let last = ((addr as u64 + len as u64 - 1) / PAGE as u64) as usize;
    assert!(last < PAGES, "mapping past the 32-bit address space");
    first..last + 1
}

impl Mem for GuestMemory {
    fn probe(&self, addr: u32, width: u32, write: bool) -> Result<(), u32> {
        self.check(addr, width, write).map_err(|_| {
            (0..width).map(|k| addr.wrapping_add(k))
                .find(|&a| self.check(a, 1, write).is_err())
                .expect("mapping changed during instruction preflight")
        })
    }
    fn u8(&self, addr: u32) -> u8 {
        self.load(addr, 1) as u8
    }
    fn set_u8(&mut self, addr: u32, v: u8) {
        self.store(addr, 1, v as u32)
    }
    fn u16(&self, addr: u32) -> u16 {
        self.load(addr, 2) as u16
    }
    fn u32(&self, addr: u32) -> u32 {
        self.load(addr, 4)
    }
    fn set_u16(&mut self, addr: u32, v: u16) {
        self.store(addr, 2, v as u32)
    }
    fn set_u32(&mut self, addr: u32, v: u32) {
        self.store(addr, 4, v)
    }
    /// Page by page: one permission test and one copy per page; a page
    /// that is not readable faults like the byte-wise reads would (at the
    /// first byte of that page the read touches).
    fn read_bytes(&self, addr: u32, out: &mut [u8]) {
        let mut k = 0usize;
        while k < out.len() {
            let a = addr.wrapping_add(k as u32);
            let (p, off) = ((a / PAGE) as usize, (a % PAGE) as usize);
            let n = (PAGE as usize - off).min(out.len() - k);
            let s = self.s();
            if s.perm(p) & READ == 0 {
                out[k] = self.load_checked(a, 1) as u8; // faults
                k += 1;
                continue;
            }
            let page = s.page(p);
            if page.is_null() {
                out[k..k + n].fill(0);
            } else {
                for (j, b) in out[k..k + n].iter_mut().enumerate() {
                    // SAFETY: inside a backed page
                    *b = unsafe { load_at(page.add(off + j), 1) as u8 };
                }
            }
            k += n;
        }
    }
    /// The table itself: an access it allows (backed page, permission set)
    /// is exactly what load/store do; READ and WRITE are only ever set on
    /// mapped pages, lazy pages stay unbacked until their first write (the
    /// fast path leaves those to store), and the table lives as long as any
    /// handle to this address space and never moves.
    fn page_table(&self) -> Option<std::ptr::NonNull<PageTable>> {
        Some(std::ptr::NonNull::from(&*self.space))
    }
    // `alloc` keeps the trait's default (panic): the fixture bump allocator
    // must not become the production allocator (RUNTIME.md, "Dispatcher").
}
