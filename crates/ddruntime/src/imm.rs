//! IMM32 services: the input method calls the game makes when it creates
//! its window. 0x00DF1200 calls 0x01106C80 right after CreateWindowExW
//! (before ShowWindow): ImmGetDefaultIMEWnd, ImmGetContext,
//! ImmSetCompositionWindow (CFS_RECT), ImmSetCandidateWindow
//! (CFS_EXCLUDE), ImmReleaseContext, and through 0x01106B30 (when its IME
//! flag is set) ImmSetOpenStatus(FALSE). The composition-string calls run
//! only on WM_IME_* messages, which the headless desktop does not send.
//!
//! Answers as measured on Windows 11 26200 (research/runtime/windows/
//! Imm.cs, a GUI process making the same calls on a 1286x749 window):
//! every thread with a window has one default IME window (class "IME",
//! style 0x8C000000, hidden, owned by the first top-level window) and one
//! default input context, which ImmGetContext returns on every call. The
//! context-setting calls notify the window that has the focus with
//! WM_IME_NOTIFY; before the window is shown and focused nothing is sent.
//! The IME is closed (an en-US desktop without an IME layout).
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_INVALID_WINDOW_HANDLE: u32 = 1400;
const WM_IME_NOTIFY: u32 = 0x0282;
const IMN_SETCANDIDATEPOS: u32 = 0x0009;
const IMN_SETCOMPOSITIONWINDOW: u32 = 0x000B;
/// The default input context's handle (any value the guest only passes
/// back; distinct from the other handle ranges).
const DEFAULT_HIMC: u32 = 0x00E1_0004;
/// The default IME window's style, as measured: WS_POPUP | WS_DISABLED |
/// WS_CLIPSIBLINGS.
const IME_WINDOW_STYLE: u32 = 0x8C00_0000;

/// IMM state (in crate::services::State).
#[derive(Default)]
pub struct Ime {
    window: u32,
    composition_form: [u32; 7],
    candidate_forms: [[u32; 8]; 4],
}

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
}

fn valid_window(hwnd: u32) -> bool {
    hwnd != 0 && crate::user32::window(hwnd).is_some()
}

/// ImmGetDefaultIMEWnd(hWnd): the thread's default IME window, created the
/// first time (owned by `hWnd`); NULL with ERROR_INVALID_WINDOW_HANDLE for
/// a handle that is no window. The last error is otherwise left alone.
pub fn imm_get_default_ime_wnd(m: &mut dyn Mem, cpu: &mut Cpu) {
    let hwnd = arg(m, cpu, 0);
    cpu.eax = if !valid_window(hwnd) {
        set_error(ERROR_INVALID_WINDOW_HANDLE);
        0
    } else {
        let existing = with_state(|s| s.ime.window);
        if existing != 0 && crate::user32::window(existing).is_some() {
            existing
        } else {
            let w = crate::user32::create_system_window("IME", hwnd, IME_WINDOW_STYLE);
            with_state(|s| s.ime.window = w);
            w
        }
    };
    ret(m, cpu, 4);
}

/// ImmGetContext(hWnd): the default input context (the same HIMC on every
/// call); NULL with ERROR_INVALID_WINDOW_HANDLE for a handle that is no
/// window.
pub fn imm_get_context(m: &mut dyn Mem, cpu: &mut Cpu) {
    let hwnd = arg(m, cpu, 0);
    cpu.eax = if valid_window(hwnd) {
        DEFAULT_HIMC
    } else {
        set_error(ERROR_INVALID_WINDOW_HANDLE);
        0
    };
    ret(m, cpu, 4);
}

/// ImmReleaseContext(hWnd, hIMC): TRUE, the last error left alone.
pub fn imm_release_context(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 8);
}

/// Check the context argument: FALSE with ERROR_INVALID_HANDLE unless it is
/// the default context (measured for NULL).
fn context_ok(himc: u32) -> bool {
    if himc != DEFAULT_HIMC {
        set_error(ERROR_INVALID_HANDLE);
    }
    himc == DEFAULT_HIMC
}

/// Tell the focused window, as Windows does once the window is active.
fn notify(m: &mut dyn Mem, cpu: &mut Cpu, wparam: u32, lparam: u32) {
    let focus = crate::user32::focus();
    if focus != 0 {
        crate::user32::send(m, cpu, focus, WM_IME_NOTIFY, wparam, lparam);
    }
}

/// ImmSetCompositionWindow(hIMC, COMPOSITIONFORM*): stores the form (style,
/// position, area: 7 DWORDs), sets the last error to 0, notifies the focused
/// window with WM_IME_NOTIFY(IMN_SETCOMPOSITIONWINDOW); TRUE.
pub fn imm_set_composition_window(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (himc, form) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = if context_ok(himc) {
        let f: [u32; 7] = std::array::from_fn(|k| m.u32(form + 4 * k as u32));
        with_state(|s| s.ime.composition_form = f);
        set_error(0);
        notify(m, cpu, IMN_SETCOMPOSITIONWINDOW, 0);
        1
    } else {
        0
    };
    ret(m, cpu, 8);
}

/// ImmSetCandidateWindow(hIMC, CANDIDATEFORM*): stores the form for its
/// dwIndex (0..3; 8 DWORDs), sets the last error to 0, notifies the focused
/// window with WM_IME_NOTIFY(IMN_SETCANDIDATEPOS, 1 << dwIndex); TRUE.
/// Another index is not modelled (stop).
pub fn imm_set_candidate_window(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (himc, form) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = if context_ok(himc) {
        let f: [u32; 8] = std::array::from_fn(|k| m.u32(form + 4 * k as u32));
        let index = f[0];
        assert!(index < 4, "ImmSetCandidateWindow with dwIndex {index} is not modelled (crate::imm)");
        with_state(|s| s.ime.candidate_forms[index as usize] = f);
        set_error(0);
        notify(m, cpu, IMN_SETCANDIDATEPOS, 1 << index);
        1
    } else {
        0
    };
    ret(m, cpu, 8);
}

/// ImmGetOpenStatus(hIMC): FALSE (the IME is closed), last error 0.
pub fn imm_get_open_status(m: &mut dyn Mem, cpu: &mut Cpu) {
    let himc = arg(m, cpu, 0);
    if context_ok(himc) {
        set_error(0);
    }
    cpu.eax = 0;
    ret(m, cpu, 4);
}

/// ImmSetOpenStatus(hIMC, fOpen): closing the closed IME is TRUE with last
/// error 0 and no message (measured). Opening it is not modelled (stop).
pub fn imm_set_open_status(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (himc, open) = (arg(m, cpu, 0), arg(m, cpu, 1));
    assert!(open == 0, "ImmSetOpenStatus(TRUE) is not modelled (crate::imm)");
    cpu.eax = if context_ok(himc) {
        set_error(0);
        1
    } else {
        0
    };
    ret(m, cpu, 8);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("IMM32.dll", "ImmGetDefaultIMEWnd", imm_get_default_ime_wnd),
    ("IMM32.dll", "ImmGetContext", imm_get_context),
    ("IMM32.dll", "ImmReleaseContext", imm_release_context),
    ("IMM32.dll", "ImmSetCompositionWindow", imm_set_composition_window),
    ("IMM32.dll", "ImmSetCandidateWindow", imm_set_candidate_window),
    ("IMM32.dll", "ImmGetOpenStatus", imm_get_open_status),
    ("IMM32.dll", "ImmSetOpenStatus", imm_set_open_status),
];
