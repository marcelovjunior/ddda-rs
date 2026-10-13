//! Opt-in flat guest view. The writable alias is never guest-accessible:
//! privileged loader writes and retired page pointers keep their old contract.
//! SEC_RESERVE pages cannot be decommitted, matching PageTable's retirement.
use crate::mem::FlatBase as Base;
use crate::mem::{PAGE, PAGES, READ, WRITE};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, OnceLock,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS},
        Memory::*,
    },
};

const SIZE: usize = 1usize << 32;
// Handler-side lookup is bounded and lock-free. More simultaneous spaces use
// the checked backend; no global "current guest" or thread-local singleton.
static VIEWS: [AtomicUsize; 64] = [const { AtomicUsize::new(0) }; 64];
static HANDLER: OnceLock<bool> = OnceLock::new();

pub(crate) struct Arena {
    base: usize,
    alias: usize,
    slot: usize,
    // 0 = unused, 1 = live section page, 2 = permanently retired slot.
    state: Mutex<Box<[u8]>>,
}

impl Arena {
    pub(crate) fn new() -> std::io::Result<Self> {
        if !*HANDLER.get_or_init(|| unsafe { !AddVectoredExceptionHandler(1, Some(recover)).is_null() }) {
            return Err(std::io::Error::other("flat VEH registration failed"));
        }
        unsafe {
            let section = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE | SEC_RESERVE,
                1,
                0,
                std::ptr::null(),
            );
            if section.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let guest = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, SIZE);
            let alias = MapViewOfFile(section, FILE_MAP_WRITE, 0, 0, SIZE);
            let error = std::io::Error::last_os_error();
            CloseHandle(section);
            if guest.Value.is_null() || alias.Value.is_null() {
                if !guest.Value.is_null() {
                    UnmapViewOfFile(guest);
                }
                if !alias.Value.is_null() {
                    UnmapViewOfFile(alias);
                }
                return Err(error);
            }
            let base = guest.Value as usize;
            let slot = VIEWS.iter().position(|v| {
                v.compare_exchange(0, base, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
            });
            let Some(slot) = slot else {
                UnmapViewOfFile(guest);
                UnmapViewOfFile(alias);
                return Err(std::io::Error::other("flat view registry full"));
            };
            Ok(Self {
                base,
                alias: alias.Value as usize,
                slot,
                state: Mutex::new(vec![0; PAGES].into_boxed_slice()),
            })
        }
    }

    pub(crate) fn base(&self) -> Base {
        Base(std::num::NonZeroUsize::new(self.base).expect("mapped view"))
    }
    pub(crate) fn owns(&self, ptr: usize) -> bool {
        ptr.wrapping_sub(self.alias) < SIZE
    }

    /// Called under PageTable's mapping lock, never in the exception handler.
    pub(crate) fn materialise(&self, page: usize, perm: u8) -> Option<*mut u8> {
        let mut states = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if states[page] == 2 {
            return None;
        }
        let ptr = (self.alias + page * PAGE as usize) as *mut u8;
        if states[page] == 0 {
            // Commit through the guest view with NOACCESS first. Committing
            // through its writable alias would briefly expose RW guest bytes.
            let committed = unsafe {
                VirtualAlloc(
                    (self.base + page * PAGE as usize) as _,
                    PAGE as usize,
                    MEM_COMMIT,
                    PAGE_NOACCESS,
                )
            };
            assert!(
                !committed.is_null(),
                "flat page commit failed: {}",
                std::io::Error::last_os_error()
            );
            states[page] = 1;
        }
        self.protect_page(page, perm);
        Some(ptr)
    }

    pub(crate) fn protect(&self, page: usize, perm: u8) {
        let states = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if states[page] == 1 {
            self.protect_page(page, perm);
        }
    }

    pub(crate) fn retire(&self, page: usize) {
        let mut states = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if states[page] == 1 {
            self.protect_page(page, 0);
        }
        states[page] = 2;
    }

    fn protect_page(&self, page: usize, perm: u8) {
        // Windows cannot express write-only memory: both operations use the
        // original slow path for that permission combination.
        let protection = match perm & (READ | WRITE) {
            READ => PAGE_READONLY,
            3 => PAGE_READWRITE,
            _ => PAGE_NOACCESS,
        };
        let mut previous = 0;
        assert_ne!(
            unsafe {
                VirtualProtect(
                    (self.base + page * PAGE as usize) as _,
                    PAGE as usize,
                    protection,
                    &mut previous,
                )
            },
            0,
            "flat page protection failed: {}",
            std::io::Error::last_os_error()
        );
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // The owning PageTable is no longer borrowed by any guest thread.
        VIEWS[self.slot].store(0, Ordering::Release);
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.base as _,
            });
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.alias as _,
            });
        }
    }
}

/// Recover only our six marked MOV sites, inside a registered guest view.
/// Do not panic, allocate, lock, commit, or call guest code here. Returning
/// to the inline assembly's failure arm lets ordinary Rust run the exact
/// existing slow path (including its original panic payload and width).
unsafe extern "system" fn recover(info: *mut EXCEPTION_POINTERS) -> i32 {
    unsafe {
        let exception = &*(*info).ExceptionRecord;
        if exception.ExceptionCode != 0xc0000005u32 as i32 || exception.NumberParameters < 2 {
            return 0;
        }
        let cpu = &mut *(*info).ContextRecord;
        let base = cpu.R10 as usize;
        let address = exception.ExceptionInformation[1];
        if address.wrapping_sub(base) >= SIZE
            || address != cpu.R8 as usize
            || !VIEWS.iter().any(|v| v.load(Ordering::Acquire) == base)
        {
            return 0;
        }
        if cpu.R11 != 0 {
            return 0;
        }
        // Check instruction mapping before examining the recovery marker;
        // unrelated AVs must continue exception search, never be swallowed.
        let mut region: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        if VirtualQuery(cpu.Rip as _, &mut region, std::mem::size_of_val(&region)) == 0
            || region.State != MEM_COMMIT
            || region.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0
            || (cpu.Rip as usize) < region.BaseAddress as usize + 3
            || (cpu.Rip as usize + 12) > (region.BaseAddress as usize + region.RegionSize)
        {
            return 0;
        }
        let before = std::slice::from_raw_parts((cpu.Rip - 3) as *const u8, 3);
        if before != [0x45, 0x31, 0xdb] {
            return 0;
        } // xor r11d, r11d
        let code = std::slice::from_raw_parts(cpu.Rip as *const u8, 12);
        let (write, length) =
            if code.starts_with(&[0x41, 0x0f, 0xb6, 0x00]) || code.starts_with(&[0x41, 0x0f, 0xb7, 0x00]) {
                (false, 4)
            } else if code.starts_with(&[0x41, 0x8b, 0x00]) {
                (false, 3)
            } else if code.starts_with(&[0x41, 0x88, 0x00]) || code.starts_with(&[0x41, 0x89, 0x00]) {
                (true, 3)
            } else if code.starts_with(&[0x66, 0x41, 0x89, 0x00]) {
                (true, 4)
            } else {
                return 0;
            };
        if exception.ExceptionInformation[0] != usize::from(write)
            || code[length..length + 8] != [0x0f, 0x1f, 0x84, 0x00, 0x4c, 0x31, 0x34, 0x32]
        {
            return 0;
        }
        cpu.R11 = 1;
        cpu.Rip += length as u64;
        -1 // EXCEPTION_CONTINUE_EXECUTION
    }
}

// Fixed encodings make the exception-site contract independent of compiler
// register allocation. The asm is a compiler memory barrier, not pure or
// readonly: guest Acquire/Release accesses must not move across it.
macro_rules! read {
    ($name:ident, $instruction:literal) => {
        #[inline]
        pub(crate) unsafe fn $name(base: Base, addr: u32) -> Option<u32> {
            let value: u32;
            let failed: u32;
            unsafe { std::arch::asm!(
                "xor r11d, r11d", $instruction,
                // Eight-byte NOP carrying the L142 site signature. The VEH
                // skips only the MOV and sets failed; no guessed instruction
                // width, per-access TLS, or failure-label LEA/jump is needed.
                ".byte 0x0f, 0x1f, 0x84, 0x00, 0x4c, 0x31, 0x34, 0x32",
                in("r8") base.0.get() + addr as usize, in("r10") base.0.get(),
                lateout("eax") value, out("r11d") failed,
                options(nostack)); }
            (failed == 0).then_some(value)
        }
    };
}
macro_rules! write {
    ($name:ident, $instruction:literal) => {
        #[inline]
        pub(crate) unsafe fn $name(base: Base, addr: u32, value: u32) -> bool {
            let failed: u32;
            unsafe { std::arch::asm!(
                "xor r11d, r11d", $instruction,
                ".byte 0x0f, 0x1f, 0x84, 0x00, 0x4c, 0x31, 0x34, 0x32",
                in("r8") base.0.get() + addr as usize, in("r10") base.0.get(),
                in("eax") value, out("r11d") failed,
                options(nostack)); }
            failed == 0
        }
    };
}
read!(u8, ".byte 0x41, 0x0f, 0xb6, 0x00");
read!(u16, ".byte 0x41, 0x0f, 0xb7, 0x00");
read!(u32, ".byte 0x41, 0x8b, 0x00");
write!(set_u8, ".byte 0x41, 0x88, 0x00");
write!(set_u16, ".byte 0x66, 0x41, 0x89, 0x00");
write!(set_u32, ".byte 0x41, 0x89, 0x00");
