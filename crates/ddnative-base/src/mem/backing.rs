//! Opt-in host backing policy. Guest permissions and publication stay in PageTable.
use super::{alloc_zero_page, page_layout, PAGE};
use std::collections::BTreeMap;

const REGION: usize = 2 << 20;
const SLOTS: usize = REGION / PAGE as usize;

/// Host allocation only; guest page permissions and address mapping are identical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackingMode {
    Pages,
    Regions,
    Large,
}
impl BackingMode {
    pub(super) fn from_env() -> Self {
        match std::env::var("DDNATIVE_BACKING").as_deref() {
            Ok("regions") => Self::Regions,
            Ok("large") => Self::Large,
            _ => Self::Pages,
        }
    }
}

pub(super) struct Regions {
    mode: BackingMode,
    regions: BTreeMap<usize, Region>,
    remapped: Vec<usize>,
    large_failed: bool,
}
impl Regions {
    pub fn new(mode: BackingMode) -> Self {
        Self {
            mode,
            regions: BTreeMap::new(),
            remapped: Vec::new(),
            large_failed: false,
        }
    }
    pub fn page(&mut self, page: usize) -> *mut u8 {
        let region = self.regions.entry(page / SLOTS).or_insert_with(|| {
            Region::new(
                self.mode == BackingMode::Large && !self.large_failed,
                &mut self.large_failed,
            )
        });
        let slot = page % SLOTS;
        if region.used[slot / 64] & (1 << (slot % 64)) != 0 {
            // A retired page may still be read by another thread. Never zero
            // or reuse its slot; fresh re-mappings retain independent storage.
            let p = alloc_zero_page();
            self.remapped.push(p as usize);
            return p;
        }
        let p = region.commit(slot);
        region.used[slot / 64] |= 1 << (slot % 64);
        p
    }
}
impl Drop for Regions {
    fn drop(&mut self) {
        for p in self.remapped.drain(..) {
            unsafe { std::alloc::dealloc(p as *mut u8, page_layout()) };
        }
    }
}

struct Region {
    allocation: usize,
    aligned: usize,
    used: [u64; SLOTS / 64],
    #[cfg(windows)]
    large: bool,
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut u8, size: usize, kind: u32, protection: u32) -> *mut u8;
    fn VirtualFree(address: *mut u8, size: usize, kind: u32) -> i32;
    fn GetLargePageMinimum() -> usize;
    fn GetLastError() -> u32;
}

impl Region {
    #[cfg(windows)]
    fn new(large: bool, failed: &mut bool) -> Self {
        // Microsoft Large-Page Support: MEM_RESERVE | MEM_COMMIT |
        // MEM_LARGE_PAGES, size/alignment multiples of GetLargePageMinimum.
        // https://learn.microsoft.com/windows/win32/memory/large-page-support
        // No account-policy changes or implicit privilege elevation.
        if large {
            let minimum = unsafe { GetLargePageMinimum() };
            let p = if minimum == REGION {
                unsafe { VirtualAlloc(std::ptr::null_mut(), REGION, 0x20003000, 4) }
            } else {
                std::ptr::null_mut()
            };
            let error = if minimum == REGION && p.is_null() {
                unsafe { GetLastError() }
            } else {
                0
            };
            if !p.is_null() {
                return Self {
                    allocation: p as usize,
                    aligned: p as usize,
                    used: [0; SLOTS / 64],
                    large: true,
                };
            }
            *failed = true;
            eprintln!(
                "DDNATIVE_BACKING=large unavailable: minimum={minimum} required={REGION} winerror={error}; using aligned regions"
            );
        }
        // Reserve twice the region size so an aligned interior always fits.
        // Commit only touched 4 KiB slots; sparse guest regions cost no 2 MiB
        // physical allocation. Alignment alone does NOT imply large TLB pages.
        let p = unsafe { VirtualAlloc(std::ptr::null_mut(), REGION * 2, 0x2000, 4) };
        assert!(!p.is_null(), "guest region reservation failed: {}", unsafe {
            GetLastError()
        });
        Self {
            allocation: p as usize,
            aligned: (p as usize + REGION - 1) & !(REGION - 1),
            used: [0; SLOTS / 64],
            large: false,
        }
    }
    #[cfg(not(windows))]
    fn new(_large: bool, _failed: &mut bool) -> Self {
        let layout = std::alloc::Layout::from_size_align(REGION, REGION).unwrap();
        let p = unsafe { std::alloc::alloc_zeroed(layout) };
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Self {
            allocation: p as usize,
            aligned: p as usize,
            used: [0; SLOTS / 64],
        }
    }
    fn commit(&self, slot: usize) -> *mut u8 {
        let p = (self.aligned + slot * PAGE as usize) as *mut u8;
        #[cfg(windows)]
        if !self.large {
            let committed = unsafe { VirtualAlloc(p, PAGE as usize, 0x1000, 4) };
            assert_eq!(committed, p, "guest region commit failed: {}", unsafe {
                GetLastError()
            });
        }
        p
    }
}
impl Drop for Region {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            VirtualFree(self.allocation as *mut u8, 0, 0x8000);
        }
        #[cfg(not(windows))]
        unsafe {
            std::alloc::dealloc(
                self.allocation as *mut u8,
                std::alloc::Layout::from_size_align(REGION, REGION).unwrap(),
            );
        }
    }
}
