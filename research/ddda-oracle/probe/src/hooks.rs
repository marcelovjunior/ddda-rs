//! The probe's hooks into the game. All are in-memory and undone on unload.
//!
//! - Frame tick: the live sBbsRpgMain object gets a private copy of its
//!   vtable with slot 6 (per-frame vf06) and slot 11 (inactive idle vf11)
//!   pointing here. Installing and removing is one aligned pointer write.
//! - Present: slot 17 of the IDirect3DDevice9 vtable (the device's own
//!   heap copy), for frame capture.
//! - Cursor gate: the call at 0x00e04f66 is retargeted so the ClipCursor /
//!   SPI_SETMOUSE routine 0x00e04d60 only runs while the game window is
//!   really active (sApp+0x2566), so background running never grabs the
//!   maintainer's mouse.
//! - Background: sApp+0x20 = 1 makes the main loop run frames while
//!   inactive. sApp+0x2566 stays 0, so keyboard, mouse and pad input from
//!   the OS are still ignored by the game.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::addr::*;
use crate::{log, patch};

pub static FRAMES: AtomicU32 = AtomicU32::new(0);
pub static IDLES: AtomicU32 = AtomicU32::new(0);
pub static PRESENTS: AtomicU32 = AtomicU32::new(0);
pub static PRESENT_INFLIGHT: AtomicU32 = AtomicU32::new(0);
pub static PRESENT_TID: AtomicU32 = AtomicU32::new(0);
pub static MAIN_TID: AtomicU32 = AtomicU32::new(0);

const VT_COPY_LEN: usize = 40;
static mut VT_COPY: [u32; VT_COPY_LEN + 1] = [0; VT_COPY_LEN + 1];
static MAIN_OBJ: AtomicU32 = AtomicU32::new(0);
static ORIG_VT: AtomicU32 = AtomicU32::new(0);

static DEV_SLOT: AtomicU32 = AtomicU32::new(0);
static ORIG_PRESENT: AtomicU32 = AtomicU32::new(0);

static BG_ORIG: AtomicU32 = AtomicU32::new(0xffff_ffff);
pub static BG_WANTED: AtomicBool = AtomicBool::new(true);

type ThisFn = unsafe extern "thiscall" fn(*mut c_void);
type PresentFn = unsafe extern "system" fn(*mut c_void, *const c_void, *const c_void, *mut c_void, *const c_void) -> i32;

unsafe extern "thiscall" fn hook_frame(this: *mut c_void) {
    MAIN_TID.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::Relaxed);
    crate::trace::pre();
    crate::input::frame();
    crate::tick(true);
    let orig: ThisFn = core::mem::transmute(VF06_FRAME as usize);
    orig(this);
    FRAMES.fetch_add(1, Ordering::SeqCst);
    crate::trace::post();
}

unsafe extern "thiscall" fn hook_idle(this: *mut c_void) {
    MAIN_TID.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::Relaxed);
    IDLES.fetch_add(1, Ordering::Relaxed);
    crate::tick(false);
    let orig: ThisFn = core::mem::transmute(VF11_IDLE as usize);
    orig(this);
}

unsafe extern "system" fn hook_present(
    dev: *mut c_void,
    a: *const c_void,
    b: *const c_void,
    c: *mut c_void,
    d: *const c_void,
) -> i32 {
    PRESENT_INFLIGHT.fetch_add(1, Ordering::SeqCst);
    PRESENT_TID.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Ordering::Relaxed);
    PRESENTS.fetch_add(1, Ordering::SeqCst);
    crate::capture::on_present(dev);
    let orig: PresentFn = core::mem::transmute(ORIG_PRESENT.load(Ordering::SeqCst) as usize);
    let r = orig(dev, a, b, c, d);
    PRESENT_INFLIGHT.fetch_sub(1, Ordering::SeqCst);
    r
}

/// Replacement for the call to 0x00e04d60 (fastcall, ecx = sMouse).
unsafe extern "thiscall" fn cursor_gate(this: *mut c_void) {
    let app = sapp();
    if app != 0 && rd::<u8>(app + SAPP_ACTIVE) != 0 {
        let orig: ThisFn = core::mem::transmute(CURSOR_FN as usize);
        orig(this);
    }
}

pub unsafe fn install_frame_hook() -> Result<(), String> {
    let app = sapp();
    if app == 0 {
        return Err("sApp is null".into());
    }
    let obj = rd::<u32>(app + SAPP_MAIN);
    if obj == 0 {
        return Err("sApp+0x2558 (sMain) is null".into());
    }
    let vt = rd::<u32>(obj);
    if vt != VT_SBBSRPGMAIN {
        return Err(format!("sMain vtable is {vt:#x}, expected sBbsRpgMain {VT_SBBSRPGMAIN:#x}"));
    }
    if rd::<u32>(vt + 6 * 4) != VF06_FRAME || rd::<u32>(vt + 11 * 4) != VF11_IDLE {
        return Err("sBbsRpgMain vtable slots 6/11 are not the expected functions".into());
    }
    let copy = &raw mut VT_COPY;
    // [0] = the RTTI locator slot before the vtable, then the slots.
    for i in 0..=VT_COPY_LEN {
        (*copy)[i] = rd::<u32>(vt - 4 + 4 * i as u32);
    }
    (*copy)[1 + 6] = hook_frame as *const () as usize as u32;
    (*copy)[1 + 11] = hook_idle as *const () as usize as u32;
    MAIN_OBJ.store(obj, Ordering::SeqCst);
    ORIG_VT.store(vt, Ordering::SeqCst);
    let new_vt = (copy as *mut u32).add(1) as u32;
    core::ptr::write_volatile(obj as *mut u32, new_vt);
    log(&format!("frame hook: sBbsRpgMain {obj:#x} vtable {vt:#x} -> {new_vt:#x}"));
    Ok(())
}

pub unsafe fn remove_frame_hook() {
    let obj = MAIN_OBJ.swap(0, Ordering::SeqCst);
    if obj != 0 {
        core::ptr::write_volatile(obj as *mut u32, ORIG_VT.load(Ordering::SeqCst));
        log("frame hook removed");
    }
}

pub fn device() -> u32 {
    unsafe {
        let r = rd::<u32>(SRENDER_PTR);
        if r == 0 {
            0
        } else {
            rd::<u32>(r + RENDER_DEVICE)
        }
    }
}

pub unsafe fn install_present_hook() -> Result<(), String> {
    let dev = device();
    if dev == 0 {
        return Err("no D3D9 device".into());
    }
    let slot = rd::<u32>(dev) + 17 * 4;
    let orig = rd::<u32>(slot);
    ORIG_PRESENT.store(orig, Ordering::SeqCst);
    patch::raw_write(slot, &(hook_present as *const () as usize as u32).to_le_bytes())?;
    DEV_SLOT.store(slot, Ordering::SeqCst);
    log(&format!("present hook: device {dev:#x} slot {slot:#x} orig {orig:#x}"));
    Ok(())
}

pub unsafe fn remove_present_hook() {
    let slot = DEV_SLOT.swap(0, Ordering::SeqCst);
    if slot != 0 {
        let _ = patch::raw_write(slot, &ORIG_PRESENT.load(Ordering::SeqCst).to_le_bytes());
        log("present hook removed");
    }
}

/// Main thread only (the call site is executed by the main thread).
pub unsafe fn install_cursor_gate() -> Result<(), String> {
    if patch::is_applied("cursor_gate") {
        return Ok(());
    }
    let site = CURSOR_CALLSITE;
    let rel_orig = (CURSOR_FN as i32).wrapping_sub(site as i32 + 5);
    let rel_new = (cursor_gate as *const () as usize as i32).wrapping_sub(site as i32 + 5);
    let mut expect = vec![0xE8u8];
    expect.extend_from_slice(&rel_orig.to_le_bytes());
    let mut new = vec![0xE8u8];
    new.extend_from_slice(&rel_new.to_le_bytes());
    patch::apply("cursor_gate", site, Some(&expect), &new)
}

pub fn bg_enabled() -> bool {
    unsafe {
        let app = sapp();
        app != 0 && rd::<u8>(app + SAPP_BG) != 0
    }
}

/// Main thread only. Turns background running on or off.
pub unsafe fn set_bg(on: bool) -> Result<(), String> {
    let app = sapp();
    if app == 0 {
        return Err("sApp is null".into());
    }
    let p = (app + SAPP_BG) as *mut u8;
    if on {
        if !patch::is_applied("cursor_gate") {
            return Err("cursor gate not installed; refusing background mode".into());
        }
        if BG_ORIG.load(Ordering::SeqCst) == 0xffff_ffff {
            BG_ORIG.store(*p as u32, Ordering::SeqCst);
        }
        *p = 1;
    } else {
        let o = BG_ORIG.swap(0xffff_ffff, Ordering::SeqCst);
        if o != 0xffff_ffff {
            *p = o as u8;
        }
    }
    log(&format!("background mode {}", if on { "on" } else { "off" }));
    Ok(())
}
