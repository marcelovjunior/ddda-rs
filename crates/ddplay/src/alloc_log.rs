//! Opt-in ownership counters around Rust's unchanged System allocator.
//! Driver/OS allocations and allocator bookkeeping are deliberately excluded.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering::Relaxed};

static MODE: AtomicU8 = AtomicU8::new(0);

fn enabled() -> bool {
    let mode = MODE.load(Relaxed);
    if mode != 0 { return mode == 2; }
    // This executes inside GlobalAlloc: never use Rust environment/string
    // APIs, formatting, locks or anything which may recursively allocate.
    #[cfg(windows)]
    let on = {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetEnvironmentVariableA(name: *const u8, value: *mut u8, size: u32) -> u32;
            fn GetLastError() -> u32;
            fn SetLastError(error: u32);
        }
        let mut value = [0u8; 2];
        unsafe {
            let saved_error = GetLastError();
            let length = GetEnvironmentVariableA(c"DDPLAY_ALLOC_LOG".as_ptr().cast(), value.as_mut_ptr(), 2);
            SetLastError(saved_error);
            length == 1 && value[0] == b'1'
        }
    };
    #[cfg(not(windows))]
    let on = false;
    MODE.store(if on { 2 } else { 1 }, Relaxed);
    on
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub live_bytes: u64,
    pub peak_live_bytes: u64,
    pub live_allocations: u64,
    pub aligned_page_bytes: u64,
    pub total_requested_bytes: u64,
}

struct CountingAllocator {
    live: AtomicU64,
    peak: AtomicU64,
    count: AtomicU64,
    pages: AtomicU64,
    requested: AtomicU64,
}

impl CountingAllocator {
    const fn new() -> Self {
        Self { live: AtomicU64::new(0), peak: AtomicU64::new(0), count: AtomicU64::new(0),
               pages: AtomicU64::new(0), requested: AtomicU64::new(0) }
    }

    fn add(&self, layout: Layout) {
        let size = layout.size() as u64;
        let live = self.live.fetch_add(size, Relaxed) + size;
        self.peak.fetch_max(live, Relaxed);
        self.count.fetch_add(1, Relaxed);
        self.requested.fetch_add(size, Relaxed);
        if layout.size() == 4096 && layout.align() == 4096 { self.pages.fetch_add(size, Relaxed); }
    }

    fn remove(&self, layout: Layout) {
        self.live.fetch_sub(layout.size() as u64, Relaxed);
        self.count.fetch_sub(1, Relaxed);
        if layout.size() == 4096 && layout.align() == 4096 { self.pages.fetch_sub(4096, Relaxed); }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { live_bytes: self.live.load(Relaxed), peak_live_bytes: self.peak.load(Relaxed),
            live_allocations: self.count.load(Relaxed), aligned_page_bytes: self.pages.load(Relaxed),
            total_requested_bytes: self.requested.load(Relaxed) }
    }
}

// SAFETY: every operation forwards the original pointer/layout unchanged to
// System. Accounting uses only atomics and is applied only after success.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if enabled() && !pointer.is_null() { self.add(layout); }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if enabled() && !pointer.is_null() { self.add(layout); }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if enabled() { self.remove(layout); }
        unsafe { System.dealloc(pointer, layout); }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if enabled() && !result.is_null() {
            self.remove(layout);
            // GlobalAlloc's caller guarantees a valid resized layout.
            self.add(unsafe { Layout::from_size_align_unchecked(size, layout.align()) });
        }
        result
    }
}

#[cfg_attr(not(test), global_allocator)]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

pub fn snapshot() -> Option<Snapshot> {
    enabled().then(|| ALLOCATOR.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_aligned_zeroed_and_reallocated_storage() {
        let allocator = CountingAllocator::new();
        let tracked = enabled();
        let small = Layout::from_size_align(64, 16).unwrap();
        unsafe {
            let first = allocator.alloc(small);
            assert!(!first.is_null());
            assert_eq!(first as usize % 16, 0);
            for i in 0..64 { first.add(i).write(i as u8 ^ 0xaa); }
            let grown = allocator.realloc(first, small, 257);
            assert!(!grown.is_null());
            assert_eq!(grown as usize % 16, 0);
            for i in 0..64 { assert_eq!(grown.add(i).read(), i as u8 ^ 0xaa); }
            assert_eq!(allocator.snapshot().live_bytes, if tracked { 257 } else { 0 });
            allocator.dealloc(grown, Layout::from_size_align(257, 16).unwrap());
            let page = Layout::from_size_align(4096, 4096).unwrap();
            let pointer = allocator.alloc_zeroed(page);
            assert!(!pointer.is_null());
            assert_eq!(pointer as usize % 4096, 0);
            assert!((0..4096).all(|i| pointer.add(i).read() == 0));
            assert_eq!(allocator.snapshot().aligned_page_bytes, if tracked { 4096 } else { 0 });
            allocator.dealloc(pointer, page);
        }
        let result = allocator.snapshot();
        assert_eq!(result.live_bytes, 0);
        assert_eq!(result.live_allocations, 0);
        assert_eq!(result.aligned_page_bytes, 0);
        assert_eq!(result.total_requested_bytes, if tracked { 64 + 257 + 4096 } else { 0 });
    }

    #[test]
    fn reads_the_opt_in_switch_without_allocator_recursion() {
        let expected = cfg!(windows) && std::env::var("DDPLAY_ALLOC_LOG").as_deref() == Ok("1");
        assert_eq!(enabled(), expected);
        assert_eq!(snapshot().is_some(), expected);
    }
}
