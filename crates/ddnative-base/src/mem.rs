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
/// Page permission bits returned by [`PageTable::perm`].
pub const READ: u8 = 1;
pub const WRITE: u8 = 2;

use std::sync::atomic::{AtomicUsize, AtomicU16, AtomicU32, AtomicU8, Ordering};

/// Where a page's bytes live: a zeroed host allocation of PAGE bytes,
/// PAGE-aligned (so a guest-aligned access is host-aligned).
fn page_layout() -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(PAGE as usize, PAGE as usize).expect("page layout")
}

fn alloc_zero_page() -> *mut u8 {
    // SAFETY: a non-zero size layout
    let p = unsafe { std::alloc::alloc_zeroed(page_layout()) };
    if p.is_null() {
        std::alloc::handle_alloc_error(page_layout());
    }
    p
}

const POINTER_MASK: usize = !(PAGE as usize - 1);
// Bits 8/9 certify both permission and non-null backing. The low byte
// retains the full public permission value even for unbacked lazy pages.
const FAST_SHIFT: usize = 8;

#[inline]
fn tagged_page(pointer: usize, perm: u8) -> usize {
    pointer | perm as usize | if pointer != 0 { ((perm & (READ | WRITE)) as usize) << FAST_SHIFT } else { 0 }
}

/// A software page table (the runtime's guest address space keeps its
/// pages here): per page, its bytes if backed and its permission bits.
/// A fixed-size table, so a page index (`addr / PAGE`) needs no bounds
/// check and the entries never move. Backed-access bits are published in
/// the same atomic entry as the pointer and permission byte.
///
/// Every entry is atomic, so guest threads running at the same time
/// (ddruntime's parallel mode, brief L42) may read and write the table
/// and the pages: a page pointer is published with Release after its
/// bytes are zeroed, and a page becomes backed exactly once (a
/// compare-exchange; [`PageTable::backed_page`]). Page bytes are accessed
/// through atomics with acquire/release order ([`load_at`], [`store_at`]),
/// plain moves on x86-64. A page taken away ([`PageTable::unback`]) is
/// retired, not freed, until the table is dropped, because another thread
/// may still be reading it.
pub struct PageTable {
    // PAGE-aligned pointer, permission byte and backed-access bits below.
    // One acquire observes both backing and permissions; zero pointer bits
    // still mean unbacked even when a lazy page has permission bits set.
    entries: Box<[AtomicUsize; PAGES]>,
    retired: std::sync::Mutex<Vec<usize>>,
}

/// A zero-filled heap value of type `T`, in one allocation (the OS hands
/// out zero pages lazily, so a large table costs nothing until touched).
///
/// # Safety
/// All-zero bytes must be a valid `T`.
unsafe fn zeroed_box<T>() -> Box<T> {
    let layout = std::alloc::Layout::new::<T>();
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut T;
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(p)
    }
}

impl Default for PageTable {
    fn default() -> Self {
        PageTable {
            // SAFETY: zero is a valid AtomicUsize.
            entries: unsafe { zeroed_box() },
            retired: Default::default(),
        }
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        let retired = std::mem::take(self.retired.get_mut().unwrap_or_else(|e| e.into_inner()));
        let live = self.entries.iter().map(|p| p.load(Ordering::Relaxed) & POINTER_MASK).filter(|&p| p != 0);
        for p in live.chain(retired) {
            // SAFETY: every non-null page came from alloc_zero_page and is
            // freed once (a retired page is no longer in the table)
            unsafe { std::alloc::dealloc(p as *mut u8, page_layout()) };
        }
    }
}

impl PageTable {
    /// Page `p`'s permission bits.
    #[inline(always)]
    pub fn perm(&self, p: usize) -> u8 {
        self.entries[p].load(Ordering::Acquire) as u8
    }
    /// Set page `p`'s permission bits (after its page pointer, if any).
    pub fn set_perm(&self, p: usize, v: u8) {
        // Preserve backing if another guest thread materialises concurrently.
        let mut entry = self.entries[p].load(Ordering::Relaxed);
        loop {
            match self.entries[p].compare_exchange_weak(entry, tagged_page(entry & POINTER_MASK, v),
                Ordering::Release, Ordering::Relaxed) {
                Ok(_) => return,
                Err(changed) => entry = changed,
            }
        }
    }
    /// Page `p`'s bytes, or null while it is not backed.
    #[inline(always)]
    pub fn page(&self, p: usize) -> *mut u8 {
        (self.entries[p].load(Ordering::Acquire) & POINTER_MASK) as *mut u8
    }
    /// Page `p`'s bytes, backing it with zeros first if needed. When two
    /// threads race, one page wins and the other allocation is freed.
    #[inline(always)]
    pub fn backed_page(&self, p: usize) -> *mut u8 {
        let cur = self.page(p);
        if !cur.is_null() {
            return cur;
        }
        self.materialise(p)
    }
    #[cold]
    #[inline(never)]
    fn materialise(&self, p: usize) -> *mut u8 {
        let fresh = alloc_zero_page();
        // Converting the allocation to an integer exposes its provenance;
        // masking the tag off later recovers an address in that allocation.
        let mut entry = self.entries[p].load(Ordering::Acquire);
        loop {
            if entry & POINTER_MASK != 0 {
                // SAFETY: fresh was never published.
                unsafe { std::alloc::dealloc(fresh, page_layout()) };
                return (entry & POINTER_MASK) as *mut u8;
            }
            match self.entries[p].compare_exchange_weak(entry, tagged_page(fresh as usize, entry as u8),
                Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return fresh,
                Err(changed) => entry = changed,
            }
        }
    }

    /// Take page `p`'s bytes away (it reads as unbacked again); the old
    /// bytes are kept until the table is dropped.
    pub fn unback(&self, p: usize) {
        let old = self.entries[p].fetch_and(u8::MAX as usize, Ordering::AcqRel) & POINTER_MASK;
        if old != 0 {
            self.retired.lock().unwrap_or_else(|e| e.into_inner()).push(old);
        }
    }
    /// How many pages are backed (diagnostics).
    pub fn backed_count(&self) -> usize {
        self.entries.iter().filter(|p| p.load(Ordering::Relaxed) & POINTER_MASK != 0).count()
    }
}

/// Whether guest memory accesses must be atomic: set (for the rest of the
/// process) once a runtime runs guest threads in parallel. Until then one
/// guest thread accesses memory at a time, its hand-over of the guest lock
/// orders the accesses, and plain copies are exact and faster.
pub static ATOMIC_ACCESSES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads `width` (1, 2 or 4) little-endian bytes at `at`, which lies with
/// all `width` bytes inside one backed page. Once [`ATOMIC_ACCESSES`] is
/// set: aligned accesses as one atomic load (Acquire), others byte by byte.
/// Aligned accesses use that same load in serial mode; only unaligned
/// serial accesses use a plain copy.
///
/// # Safety
/// `at .. at + width` must be inside a page of a live table.
#[inline(always)]
pub unsafe fn load_at(at: *mut u8, width: usize) -> u32 {
    unsafe {
        // Aligned Acquire loads have the same bytes and x86-64 instruction in
        // serial and parallel mode. Only unaligned accesses need the mode bit.
        match width {
            1 => (*(at as *const AtomicU8)).load(Ordering::Acquire) as u32,
            2 if at as usize % 2 == 0 => u16::from_le((*(at as *const AtomicU16)).load(Ordering::Acquire)) as u32,
            4 if at as usize % 4 == 0 => u32::from_le((*(at as *const AtomicU32)).load(Ordering::Acquire)),
            _ if !ATOMIC_ACCESSES.load(Ordering::Relaxed) => {
                let mut v = [0u8; 4];
                std::ptr::copy_nonoverlapping(at, v.as_mut_ptr(), width);
                u32::from_le_bytes(v)
            }
            _ => (0..width).fold(0, |v, k| v | ((*(at.add(k) as *const AtomicU8)).load(Ordering::Acquire) as u32) << (8 * k)),
        }
    }
}

/// Writes the low `width` bytes of `v` little-endian at `at`, as
/// [`load_at`] reads them (Release).
///
/// # Safety
/// As for [`load_at`].
#[inline(always)]
pub unsafe fn store_at(at: *mut u8, width: usize, v: u32) {
    unsafe {
        match width {
            1 => (*(at as *const AtomicU8)).store(v as u8, Ordering::Release),
            2 if at as usize % 2 == 0 => (*(at as *const AtomicU16)).store((v as u16).to_le(), Ordering::Release),
            4 if at as usize % 4 == 0 => (*(at as *const AtomicU32)).store(v.to_le(), Ordering::Release),
            _ if !ATOMIC_ACCESSES.load(Ordering::Relaxed) => {
                std::ptr::copy_nonoverlapping(v.to_le_bytes().as_ptr(), at, width);
            }
            _ => {
                for k in 0..width {
                    (*(at.add(k) as *const AtomicU8)).store((v >> (8 * k)) as u8, Ordering::Release);
                }
            }
        }
    }
}

/// The guest word at `at` as an atomic, for read-modify-write services
/// (InterlockedCompareExchange and the others); None if `at` is not
/// 4-aligned (Windows requires Interlocked operands to be).
///
/// # Safety
/// `at .. at + 4` must be inside a page of a live table.
#[inline(always)]
pub unsafe fn atomic_u32_at<'a>(at: *mut u8) -> Option<&'a AtomicU32> {
    (at as usize % 4 == 0).then(|| unsafe { &*(at as *const AtomicU32) })
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
    // move.
    let t = unsafe { t.as_ref() };
    let entry = t.entries[p].load(Ordering::Acquire);
    if entry & ((perm as usize) << FAST_SHIFT) == 0 {
        return None;
    }
    // SAFETY: the pointer bits name a backed PAGE-byte allocation.
    Some(unsafe { ((entry & POINTER_MASK) as *mut u8).add(off) })
}

/// The lifted code's memory accesses (lift.py lowers `m.u32(a)` to
/// `m.fast_u32(pt, a)` with `pt = m.page_table()` read at function
/// entry): the table's bytes when it allows the access, else the trait
/// method, so faults, lazy pages and fixtures behave exactly as through
/// the trait.
// `#[inline(never)]` (L77): one copy, compiled with this crate's own
// opt-level (3 in the play profiles), called from the shards. An `#[inline]`
// accessor was instantiated inside every shard at the shard's opt-level, so
// at opt-level 0 each guest memory access became a chain of unoptimised calls
// (allowed, bool::then, the atomic loads); `inline(always)` instead copied
// the whole check into every access (shards four times larger, too big to
// link). One call into optimised code costs a few nanoseconds.
impl dyn Mem + '_ {
    /// The guest word at `addr` as a host atomic, for read-modify-write
    /// services (the Interlocked functions): Some when this memory has a
    /// page table, `addr` is 4-aligned and its page is readable and
    /// writable (a lazy page gets its bytes first); else None, and the
    /// caller uses the ordinary accessors, which fault where Windows would.
    pub fn atomic_u32(&self, addr: u32) -> Option<&AtomicU32> {
        // SAFETY: page_table()'s contract: the table outlives this borrow
        let t = unsafe { self.page_table()?.as_ref() };
        let p = (addr / PAGE) as usize;
        if addr % 4 != 0 || t.perm(p) & (READ | WRITE) != READ | WRITE {
            return None;
        }
        let page = t.backed_page(p);
        // SAFETY: 4 aligned bytes inside a backed page of a live table
        unsafe { atomic_u32_at(page.add((addr % PAGE) as usize)) }
    }
    // L114: inlinable into opt-level-1 shards with the inline-access feature
    #[cfg_attr(feature = "inline-access", inline)]
    #[cfg_attr(not(feature = "inline-access"), inline(never))]
    pub fn fast_u8(&self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32) -> u8 {
        let Some(p) = allowed(pt, addr, 1, READ) else { return self.slow_u8(addr); };
        // SAFETY: allowed returns 1 byte inside a backed page.
        unsafe { load_at(p, 1) as u8 }
    }
    // L114: inlinable into opt-level-1 shards with the inline-access feature
    #[cfg_attr(feature = "inline-access", inline)]
    #[cfg_attr(not(feature = "inline-access"), inline(never))]
    pub fn fast_u16(&self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32) -> u16 {
        let Some(p) = allowed(pt, addr, 2, READ) else { return self.slow_u16(addr); };
        // SAFETY: allowed returns 2 bytes inside a backed page.
        unsafe { load_at(p, 2) as u16 }
    }
    // L114: inlinable into opt-level-1 shards with the inline-access feature
    #[cfg_attr(feature = "inline-access", inline)]
    #[cfg_attr(not(feature = "inline-access"), inline(never))]
    pub fn fast_u32(&self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32) -> u32 {
        let Some(p) = allowed(pt, addr, 4, READ) else { return self.slow_u32(addr); };
        // SAFETY: allowed returns 4 bytes inside a backed page.
        unsafe { load_at(p, 4) }
    }
    // L114: inlinable into opt-level-1 shards with the inline-access feature
    #[cfg_attr(feature = "inline-access", inline)]
    #[cfg_attr(not(feature = "inline-access"), inline(never))]
    pub fn fast_set_u8(&mut self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, v: u8) {
        let Some(p) = allowed(pt, addr, 1, WRITE) else { return self.slow_set_u8(addr, v); };
        // SAFETY: allowed returns 1 byte inside a backed page.
        unsafe { store_at(p, 1, v as u32) }
    }
    // L114: inlinable into opt-level-1 shards with the inline-access feature
    #[cfg_attr(feature = "inline-access", inline)]
    #[cfg_attr(not(feature = "inline-access"), inline(never))]
    pub fn fast_set_u16(&mut self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, v: u16) {
        let Some(p) = allowed(pt, addr, 2, WRITE) else { return self.slow_set_u16(addr, v); };
        // SAFETY: allowed returns 2 bytes inside a backed page.
        unsafe { store_at(p, 2, v as u32) }
    }
    // L114: inlinable into opt-level-1 shards with the inline-access feature
    #[cfg_attr(feature = "inline-access", inline)]
    #[cfg_attr(not(feature = "inline-access"), inline(never))]
    pub fn fast_set_u32(&mut self, pt: Option<std::ptr::NonNull<PageTable>>, addr: u32, v: u32) {
        let Some(p) = allowed(pt, addr, 4, WRITE) else { return self.slow_set_u32(addr, v); };
        // SAFETY: allowed returns 4 bytes inside a backed page.
        unsafe { store_at(p, 4, v) }
    }
    #[cold]
    #[inline(never)]
    fn slow_u8(&self, addr: u32) -> u8 { self.u8(addr) }
    #[cold]
    #[inline(never)]
    fn slow_set_u8(&mut self, addr: u32, v: u8) { self.set_u8(addr, v) }

    #[cold]
    #[inline(never)]
    fn slow_u16(&self, addr: u32) -> u16 { self.u16(addr) }
    #[cold]
    #[inline(never)]
    fn slow_set_u16(&mut self, addr: u32, v: u16) { self.set_u16(addr, v) }

    #[cold]
    #[inline(never)]
    fn slow_u32(&self, addr: u32) -> u32 { self.u32(addr) }
    #[cold]
    #[inline(never)]
    fn slow_set_u32(&mut self, addr: u32, v: u32) { self.set_u32(addr, v) }
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
