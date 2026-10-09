//! In-memory patches with verification and an undo list. Nothing here ever
//! touches a file.

use std::ffi::c_void;
use std::sync::Mutex;

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

struct Saved {
    addr: u32,
    old: Vec<u8>,
    what: &'static str,
}

static UNDO: Mutex<Vec<Saved>> = Mutex::new(Vec::new());

pub unsafe fn raw_write(addr: u32, bytes: &[u8]) -> Result<(), String> {
    let mut old = 0u32;
    if VirtualProtect(addr as *const c_void, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return Err(format!("VirtualProtect {addr:#x} failed"));
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    let mut tmp = 0u32;
    VirtualProtect(addr as *const c_void, bytes.len(), old, &mut tmp);
    FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, bytes.len());
    Ok(())
}

/// Replaces `expect` at `addr` with `new` and remembers the old bytes.
/// Refuses if the current bytes are not `expect` (`None` skips the check).
pub unsafe fn apply(what: &'static str, addr: u32, expect: Option<&[u8]>, new: &[u8]) -> Result<(), String> {
    let cur = std::slice::from_raw_parts(addr as *const u8, new.len()).to_vec();
    if let Some(e) = expect {
        if e != &cur[..] {
            return Err(format!("{what}: bytes at {addr:#x} are {:02x?}, expected {:02x?}", cur, e));
        }
    }
    raw_write(addr, new)?;
    UNDO.lock().unwrap().push(Saved { addr, old: cur, what });
    crate::log(&format!("patch {what} at {addr:#x}"));
    Ok(())
}

/// Undoes one named patch (latest first).
#[allow(dead_code)]
pub unsafe fn revert(what: &str) -> bool {
    let mut u = UNDO.lock().unwrap();
    if let Some(i) = u.iter().rposition(|s| s.what == what) {
        let s = u.remove(i);
        let _ = raw_write(s.addr, &s.old);
        crate::log(&format!("revert {} at {:#x}", s.what, s.addr));
        true
    } else {
        false
    }
}

pub fn is_applied(what: &str) -> bool {
    UNDO.lock().unwrap().iter().any(|s| s.what == what)
}

/// Undoes everything, newest first.
pub unsafe fn revert_all() {
    let mut u = UNDO.lock().unwrap();
    while let Some(s) = u.pop() {
        let _ = raw_write(s.addr, &s.old);
        crate::log(&format!("revert {} at {:#x}", s.what, s.addr));
    }
}

pub fn list() -> Vec<String> {
    UNDO.lock().unwrap().iter().map(|s| format!("{}@{:#x}", s.what, s.addr)).collect()
}
