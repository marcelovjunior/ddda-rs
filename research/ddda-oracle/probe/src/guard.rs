//! Guarded execution: call an arbitrary game function (or touch arbitrary
//! memory) on the main thread, and turn a fault into an error instead of a
//! crash. A vectored exception handler catches access violations (and a few
//! other faults) raised on the guarded thread while a guarded call is
//! active, and resumes at `guard_landing`, which restores the stack saved
//! before the call.
//!
//! The game's state after a recovered fault can be inconsistent (a lock
//! taken, a half-written object). Recovery is a seatbelt, not a licence.

use core::arch::naked_asm;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, RemoveVectoredExceptionHandler, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

static mut G_SAVE_ESP: u32 = 0;
static mut G_TARGET: u32 = 0;
static G_ACTIVE: AtomicBool = AtomicBool::new(false);
static G_TID: AtomicU32 = AtomicU32::new(0);
static G_FAULT_CODE: AtomicU32 = AtomicU32::new(0);
static G_FAULT_EIP: AtomicU32 = AtomicU32::new(0);
static G_FAULT_ADDR: AtomicU32 = AtomicU32::new(0);
static mut VEH: *mut c_void = core::ptr::null_mut();

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct CallOut {
    pub eax: u32,
    pub edx: u32,
    pub st0: f64,
}

#[derive(Debug)]
pub struct Fault {
    pub code: u32,
    pub eip: u32,
    pub addr: u32,
}

/// cdecl: (func, ecx, edx, args, nargs, out, float_ret) -> 0 ok / 1 fault.
/// Pushes args[nargs-1]..args[0], loads ecx/edx, calls func, then restores
/// esp from the saved value, so cdecl, stdcall, thiscall and fastcall all
/// come back balanced (and so does a wrong argument count).
#[unsafe(naked)]
unsafe extern "C" fn guarded_call_raw(
    _func: u32,
    _ecx: u32,
    _edx: u32,
    _args: *const u32,
    _nargs: u32,
    _out: *mut CallOut,
    _float_ret: u32,
) -> u32 {
    naked_asm!(
        "push ebp",
        "push ebx",
        "push esi",
        "push edi",
        "mov dword ptr [{esp_slot}], esp",
        "mov eax, dword ptr [esp+20]",
        "mov dword ptr [{target}], eax",
        "mov esi, dword ptr [esp+32]",
        "mov edi, dword ptr [esp+36]",
        "mov ecx, dword ptr [esp+24]",
        "mov edx, dword ptr [esp+28]",
        "2:",
        "test edi, edi",
        "jz 3f",
        "push dword ptr [esi+edi*4-4]",
        "dec edi",
        "jmp 2b",
        "3:",
        "call dword ptr [{target}]",
        "mov esp, dword ptr [{esp_slot}]",
        "mov ecx, dword ptr [esp+40]",
        "mov dword ptr [ecx], eax",
        "mov dword ptr [ecx+4], edx",
        "cmp dword ptr [esp+44], 0",
        "je 4f",
        "fstp qword ptr [ecx+8]",
        "4:",
        "xor eax, eax",
        "pop edi",
        "pop esi",
        "pop ebx",
        "pop ebp",
        "ret",
        esp_slot = sym G_SAVE_ESP,
        target = sym G_TARGET,
    )
}

/// Resume point after a fault: back to the frame of guarded_call_raw.
#[unsafe(naked)]
unsafe extern "C" fn guard_landing() {
    naked_asm!(
        "mov esp, dword ptr [{esp_slot}]",
        "fnclex",
        "emms",
        "mov eax, 1",
        "pop edi",
        "pop esi",
        "pop ebx",
        "pop ebp",
        "ret",
        esp_slot = sym G_SAVE_ESP,
    )
}

unsafe extern "system" fn veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    const CONTINUE_SEARCH: i32 = 0;
    const CONTINUE_EXECUTION: i32 = -1;
    if !G_ACTIVE.load(Ordering::SeqCst) || GetCurrentThreadId() != G_TID.load(Ordering::SeqCst) {
        return CONTINUE_SEARCH;
    }
    let rec = &*(*info).ExceptionRecord;
    let code = rec.ExceptionCode as u32;
    match code {
        0xC000_0005 | 0xC000_001D | 0xC000_0096 | 0xC000_0094 | 0xC000_0095 | 0xC000_008C => {}
        _ => return CONTINUE_SEARCH,
    }
    G_FAULT_CODE.store(code, Ordering::SeqCst);
    G_FAULT_EIP.store(rec.ExceptionAddress as u32, Ordering::SeqCst);
    let addr = if rec.NumberParameters >= 2 { rec.ExceptionInformation[1] as u32 } else { 0 };
    G_FAULT_ADDR.store(addr, Ordering::SeqCst);
    G_ACTIVE.store(false, Ordering::SeqCst);
    let ctx = &mut *(*info).ContextRecord;
    ctx.Eip = guard_landing as *const () as usize as u32;
    ctx.Esp = G_SAVE_ESP;
    CONTINUE_EXECUTION
}

pub unsafe fn install() {
    VEH = AddVectoredExceptionHandler(1, Some(veh));
}

pub unsafe fn uninstall() {
    if !VEH.is_null() {
        RemoveVectoredExceptionHandler(VEH);
        VEH = core::ptr::null_mut();
    }
}

/// Calls `func` with ecx/edx and stack args (pushed right to left).
/// Not reentrant: main thread only.
pub unsafe fn call(func: u32, ecx: u32, edx: u32, args: &[u32], float_ret: bool) -> Result<CallOut, Fault> {
    let mut out = CallOut::default();
    G_TID.store(GetCurrentThreadId(), Ordering::SeqCst);
    G_ACTIVE.store(true, Ordering::SeqCst);
    let r = guarded_call_raw(func, ecx, edx, args.as_ptr(), args.len() as u32, &mut out, float_ret as u32);
    G_ACTIVE.store(false, Ordering::SeqCst);
    if r == 0 {
        Ok(out)
    } else {
        Err(Fault {
            code: G_FAULT_CODE.load(Ordering::SeqCst),
            eip: G_FAULT_EIP.load(Ordering::SeqCst),
            addr: G_FAULT_ADDR.load(Ordering::SeqCst),
        })
    }
}

extern "C" fn copy_bytes(dst: *mut u8, src: *const u8, n: usize) {
    unsafe {
        for i in 0..n {
            core::ptr::write_volatile(dst.add(i), core::ptr::read_volatile(src.add(i)));
        }
    }
}

/// Guarded memcpy on the current thread (main thread only).
pub unsafe fn copy(dst: u32, src: u32, n: usize) -> Result<(), Fault> {
    call(copy_bytes as *const () as usize as u32, 0, 0, &[dst, src, n as u32], false).map(|_| ())
}
