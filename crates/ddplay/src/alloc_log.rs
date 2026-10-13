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
        sites::event(layout.size(), false);
        if layout.size() == 4096 && layout.align() == 4096 { self.pages.fetch_add(size, Relaxed); }
    }

    fn remove(&self, layout: Layout) {
        sites::event(layout.size(), true);
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
    sites::report();
    enabled().then(|| ALLOCATOR.snapshot())
}

// Every successful allocation, including realloc's full new request, is
// attributed when DDPLAY_ALLOC_STACKS=1. Deliberately separate from the cheap
// ownership counter: stack walking is diagnostic overhead, not a timing run.
mod sites {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::{Acquire, Release, Relaxed}};
    const DEPTH: usize = 16;
    const SLOTS: usize = 65536;
    struct Site { key: AtomicU64, frames: [AtomicUsize; DEPTH], bytes: AtomicU64, calls: AtomicU64, free: super::AtomicU8 }
    impl Site {
        const fn new() -> Self { Self { key: AtomicU64::new(0), frames: [const { AtomicUsize::new(0) }; DEPTH], bytes: AtomicU64::new(0), calls: AtomicU64::new(0), free: super::AtomicU8::new(0) } }
    }
    static TABLE: [Site; SLOTS] = [const { Site::new() }; SLOTS];
    static DROPPED_CALLS: AtomicU64 = AtomicU64::new(0);
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static DROPPED_BYTES: AtomicU64 = AtomicU64::new(0);
    static MODE: super::AtomicU8 = super::AtomicU8::new(0);
    thread_local! { static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

    fn enabled() -> bool {
        let previous = MODE.load(Relaxed);
        if previous != 0 { return previous == 2; }
        #[cfg(windows)]
        let on = unsafe {
            unsafe extern "system" {
                fn GetEnvironmentVariableA(name: *const u8, value: *mut u8, size: u32) -> u32;
                fn GetLastError() -> u32;
                fn SetLastError(error: u32);
            }
            let error = GetLastError();
            let mut value = [0u8; 2];
            let length = GetEnvironmentVariableA(c"DDPLAY_ALLOC_STACKS".as_ptr().cast(), value.as_mut_ptr(), 2);
            SetLastError(error);
            length == 1 && value[0] == b'1'
        };
        #[cfg(not(windows))]
        let on = false;
        MODE.store(if on { 2 } else { 1 }, Relaxed);
        on
    }

    // Fixed buffers and OS calls only: this runs inside the global allocator.
    #[cfg(windows)]
    mod window {
        use super::*;
        unsafe extern "system" {
            fn GetEnvironmentVariableA(name: *const u8, value: *mut u8, size: u32) -> u32;
            fn GetTickCount64() -> u64;
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn GetThreadDescription(thread: *mut std::ffi::c_void, text: *mut *mut u16) -> i32;
            fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
            fn GetLastError() -> u32;
            fn SetLastError(error: u32);
        }
        pub struct Config { start: u64, delay: u64, duration: u64, name: [u8; 128], len: usize }
        static CONFIG: std::sync::OnceLock<Config> = std::sync::OnceLock::new();
        fn env(name: &std::ffi::CStr, out: &mut [u8]) -> usize {
            let n = unsafe { GetEnvironmentVariableA(name.as_ptr().cast(), out.as_mut_ptr(), out.len() as u32) } as usize;
            if n < out.len() { n } else { 0 }
        }
        fn number(name: &std::ffi::CStr) -> u64 {
            let mut text = [0u8; 32];
            let n = env(name, &mut text);
            text[..n].iter().try_fold(0u64, |v, b| {
                if !b.is_ascii_digit() { return None; }
                v.checked_mul(10)?.checked_add((b - b'0') as u64)
            }).unwrap_or(0)
        }
        fn config() -> &'static Config {
            CONFIG.get_or_init(|| {
                let error = unsafe { GetLastError() };
                let mut name = [0u8; 128];
                let len = env(c"DDPLAY_ALLOC_THREAD", &mut name);
                let c = Config { start: unsafe { GetTickCount64() },
                    delay: number(c"DDPLAY_ALLOC_DELAY_MS"), duration: number(c"DDPLAY_ALLOC_DURATION_MS"), name, len };
                unsafe { SetLastError(error); }
                c
            })
        }
        pub fn finished() -> bool {
            let c = config();
            c.duration != 0 && unsafe { GetTickCount64() }.saturating_sub(c.start) >= c.delay.saturating_add(c.duration)
        }
        pub fn timed() -> bool { config().duration != 0 }
        pub fn report() {
            let c = config();
            eprintln!("alloc-window start_tick_ms={} end_tick_ms={} thread={}",
                c.start.saturating_add(c.delay), c.start.saturating_add(c.delay).saturating_add(c.duration),
                std::str::from_utf8(&c.name[..c.len]).unwrap_or("<invalid UTF-8>"));
        }
        pub fn accepts() -> bool {
            let c = config();
            let elapsed = unsafe { GetTickCount64() }.saturating_sub(c.start);
            if elapsed < c.delay || (c.duration != 0 && elapsed >= c.delay.saturating_add(c.duration)) { return false; }
            if c.len == 0 { return true; }
            // Do not cache a negative result: a thread can set its name after
            // its first Rust allocation. LocalFree is the API's required owner.
            let error = unsafe { GetLastError() };
            let mut name = std::ptr::null_mut();
            let ok = unsafe { GetThreadDescription(GetCurrentThread(), &mut name) } >= 0 && !name.is_null();
            let matches = ok && (0..c.len).all(|i| unsafe { *name.add(i) } == c.name[i] as u16)
                && unsafe { *name.add(c.len) } == 0;
            if !name.is_null() { unsafe { LocalFree(name.cast()); } }
            unsafe { SetLastError(error); }
            matches
        }
    }

    fn record(frames: &[usize; DEPTH], bytes: u64, free: bool) {
        let hash = frames.iter().fold(0xcbf29ce484222325u64 ^ free as u64, |h, pc| (h ^ *pc as u64).wrapping_mul(0x100000001b3)).max(2);
        for probe in 0..64 {
            let slot = &TABLE[(hash as usize + probe) % SLOTS];
            let mut key = slot.key.load(Acquire);
            if key == 0 && slot.key.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
                for (destination, value) in slot.frames.iter().zip(frames) { destination.store(*value, Relaxed); }
                slot.free.store(free as u8, Relaxed);
                slot.key.store(hash, Release);
                key = hash;
            }
            // Never spin on an interrupted writer or take an allocator lock.
            if key == hash && slot.free.load(Relaxed) == free as u8 && slot.frames.iter().zip(frames).all(|(a, b)| a.load(Relaxed) == *b) {
                slot.bytes.fetch_add(bytes, Relaxed);
                slot.calls.fetch_add(1, Relaxed);
                return;
            }
        }
        DROPPED_BYTES.fetch_add(bytes, Relaxed);
        DROPPED_CALLS.fetch_add(1, Relaxed);
    }

    pub(super) fn event(bytes: usize, free: bool) {
        if !enabled() { return; }
        let _ = BUSY.try_with(|busy| {
            if busy.replace(true) { return; }
            #[cfg(windows)]
            if !window::accepts() { busy.set(false); return; }
            #[cfg(windows)]
            unsafe {
                #[link(name = "kernel32")]
                unsafe extern "system" {
                    fn RtlCaptureStackBackTrace(skip: u32, count: u32, frames: *mut *mut std::ffi::c_void, hash: *mut u32) -> u16;
                    fn GetLastError() -> u32;
                    fn SetLastError(error: u32);
                }
                let error = GetLastError();
                let mut frames = [0usize; DEPTH];
                RtlCaptureStackBackTrace(1, DEPTH as u32, frames.as_mut_ptr().cast(), std::ptr::null_mut());
                SetLastError(error);
                record(&frames, bytes as u64, free);
            }
            busy.set(false);
        });
    }

    pub(super) fn report() {
        if !enabled() { return; }
        #[cfg(windows)]
        if window::timed() && (!window::finished() || REPORTED.swap(true, Relaxed)) { return; }
        let _ = BUSY.try_with(|busy| {
            if busy.replace(true) { return; }
            #[cfg(windows)]
            window::report();
            let mut rows: Vec<_> = TABLE.iter().enumerate().filter_map(|(id, slot)| {
                (slot.key.load(Acquire) > 1).then(|| (slot.bytes.load(Relaxed), id, slot))
            }).collect();
            rows.sort_unstable_by_key(|row| std::cmp::Reverse(row.0));
            #[cfg(windows)]
            let base = unsafe {
                unsafe extern "system" { fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void; }
                GetModuleHandleW(std::ptr::null()) as usize
            };
            #[cfg(not(windows))]
            let base = 0usize;
            for (bytes, id, slot) in rows.into_iter() {
                let frames: Vec<_> = slot.frames.iter().map(|pc| format!("{:x}", pc.load(Relaxed))).collect();
                let kind = if slot.free.load(Relaxed) != 0 { "free" } else { "alloc" };
                eprintln!("{kind}-site id={id} bytes={bytes} calls={} base={base:x} pcs={}", slot.calls.load(Relaxed), frames.join(","));
            }
            eprintln!("alloc-site-overflow bytes={} calls={}", DROPPED_BYTES.load(Relaxed), DROPPED_CALLS.load(Relaxed));
            busy.set(false);
        });
    }
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
