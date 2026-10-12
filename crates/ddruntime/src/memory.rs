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
//! its own [`GuestMemory`] handle ([`GuestMemory::share`]), and only the
//! holder of the guest lock (crate::runtime) accesses it.
use ddnative_base::mem::Mem;
use std::fmt;

pub const PAGE: u32 = 0x1000;
const PAGES: usize = 1 << 20;

/// Page permission bits.
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;
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

/// One page of guest memory.
type Page = Box<[u8; PAGE as usize]>;

fn zero_page() -> Page {
    vec![0u8; PAGE as usize].into_boxed_slice().try_into().expect("a page")
}

/// Fixed-size tables: a page index (`addr / PAGE`, below 2^20) needs no
/// bounds check, and a page is one thin pointer.
struct Space {
    pages: Box<[Option<Page>; PAGES]>,
    perms: Box<[u8; PAGES]>,
}

struct SharedSpace(std::cell::UnsafeCell<Space>);

// SAFETY: the address space is accessed only by the host thread holding the
// guest lock (crate::runtime); the lock's hand-over orders those accesses.
unsafe impl Sync for SharedSpace {}
unsafe impl Send for SharedSpace {}

/// A handle to a guest address space.
pub struct GuestMemory {
    space: std::sync::Arc<SharedSpace>,
}

impl Default for GuestMemory {
    fn default() -> Self {
        let pages: Box<[Option<Page>]> = (0..PAGES).map(|_| None).collect();
        let perms: Box<[u8]> = vec![0; PAGES].into_boxed_slice();
        let space = Space {
            pages: pages.try_into().unwrap_or_else(|_| unreachable!()),
            perms: perms.try_into().unwrap_or_else(|_| unreachable!()),
        };
        GuestMemory { space: std::sync::Arc::new(SharedSpace(std::cell::UnsafeCell::new(space))) }
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

    fn s(&self) -> &Space {
        // SAFETY: see SharedSpace; no handle's mutable access overlaps this
        // one, because only the guest-lock holder runs and it borrows the
        // space for single accesses only.
        unsafe { &*self.space.0.get() }
    }

    fn s_mut(&mut self) -> &mut Space {
        // SAFETY: as for `s`.
        unsafe { &mut *self.space.0.get() }
    }

    /// Map [addr, addr + len) rounded out to whole pages, zero-filled, with
    /// `perm`. Mapping over an already mapped page is refused.
    pub fn map(&mut self, addr: u32, len: u32, perm: u8) -> Result<(), Overlap> {
        let pages = page_range(addr, len);
        if let Some(p) = pages.clone().find(|&p| self.mapped(p)) {
            return Err(Overlap { page: (p as u32) * PAGE });
        }
        for p in pages {
            self.s_mut().pages[p] = Some(zero_page());
            self.s_mut().perms[p] = perm;
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
            self.s_mut().perms[p] = perm | LAZY;
        }
        Ok(())
    }

    fn mapped(&self, p: usize) -> bool {
        self.s().pages[p].is_some() || self.s().perms[p] & LAZY != 0
    }

    /// Change the permissions of mapped pages in [addr, addr + len).
    pub fn protect(&mut self, addr: u32, len: u32, perm: u8) {
        for p in page_range(addr, len) {
            assert!(self.mapped(p), "protect of unmapped page {:#010x}", p as u32 * PAGE);
            let lazy = self.s().perms[p] & LAZY;
            self.s_mut().perms[p] = perm | lazy;
        }
    }

    pub fn is_mapped(&self, addr: u32) -> bool {
        self.mapped((addr / PAGE) as usize)
    }

    pub fn perm(&self, addr: u32) -> u8 {
        self.s().perms[(addr / PAGE) as usize] & (READ | WRITE)
    }

    /// Pages that have host memory behind them (diagnostics).
    pub fn backed_pages(&self) -> usize {
        self.s().pages.iter().filter(|p| p.is_some()).count()
    }

    /// Privileged copy into mapped pages, ignoring permissions (the loader
    /// filling read-only sections). Fails on unmapped pages.
    pub fn poke(&mut self, addr: u32, data: &[u8]) -> Result<(), Fault> {
        for (k, &b) in data.iter().enumerate() {
            let a = addr.wrapping_add(k as u32);
            if !self.mapped((a / PAGE) as usize) {
                return Err(Fault { addr: a, width: 1, write: true, reason: "unmapped page" });
            }
            *self.byte_mut(a) = b;
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
            } else if self.s().perms[p] & need == 0 {
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
        match &self.s().pages[(a / PAGE) as usize] {
            Some(page) => page[(a % PAGE) as usize],
            None => 0, // a lazy page not written yet
        }
    }

    fn byte_mut(&mut self, a: u32) -> &mut u8 {
        let page = self.s_mut().pages[(a / PAGE) as usize].get_or_insert_with(zero_page);
        &mut page[(a % PAGE) as usize]
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
            if s.perms[p] & READ != 0 {
                let Some(page) = s.pages[p].as_ref() else { return 0 }; // lazy: zero until written
                let mut v = [0u8; 4];
                v[..width as usize].copy_from_slice(&page[off..off + width as usize]);
                return u32::from_le_bytes(v);
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
            // within one page: a single slice read (a lazy page reads zero)
            let Some(page) = self.s().pages[(addr / PAGE) as usize].as_ref() else { return 0 };
            let mut v = [0u8; 4];
            v[..width as usize].copy_from_slice(&page[off as usize..(off + width) as usize]);
            u32::from_le_bytes(v)
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
            let s = self.s_mut();
            // WRITE is only ever set on mapped pages
            if s.perms[p] & WRITE != 0 {
                let page = s.pages[p].get_or_insert_with(zero_page);
                page[off..off + width as usize].copy_from_slice(&v.to_le_bytes()[..width as usize]);
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
            *self.byte_mut(addr.wrapping_add(k)) = (v >> (8 * k)) as u8;
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
            if s.perms[p] & READ == 0 {
                out[k] = self.load_checked(a, 1) as u8; // faults
                k += 1;
                continue;
            }
            match s.pages[p].as_ref() {
                Some(page) => out[k..k + n].copy_from_slice(&page[off..off + n]),
                None => out[k..k + n].fill(0),
            }
            k += n;
        }
    }
    // `alloc` keeps the trait's default (panic): the fixture bump allocator
    // must not become the production allocator (RUNTIME.md, "Dispatcher").
}
