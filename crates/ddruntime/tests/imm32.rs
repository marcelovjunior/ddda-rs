//! Brief L43: IMM32 for a window with the default input context and no
//! IME open, as Windows 11 26200 answers in a 32-bit process
//! (research/runtime/windows/Imm.cs), with the game's own arguments.
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use ddruntime::dispatch::{Binding, Registry};
use ddruntime::memory::{GuestMemory, READ, WRITE};
use ddruntime::{input, services, user32};
use std::sync::Arc;

const STACK: u32 = 0x0011_0000;
const DATA: u32 = 0x0030_0000;
const ERROR_INVALID_HANDLE: u32 = 6;

fn wndproc(m: &mut dyn Mem, cpu: &mut Cpu) {
    user32::def_window_proc(m, cpu);
}

fn call(f: fn(&mut dyn Mem, &mut Cpu), m: &mut GuestMemory, args: &[u32]) -> u32 {
    let mut cpu = Cpu { esp: STACK, ..Cpu::default() };
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_5678);
    f(m, &mut cpu);
    assert_eq!(cpu.esp, STACK, "stdcall: {} argument(s) popped", args.len());
    cpu.eax
}

fn last_error(m: &mut GuestMemory) -> u32 {
    call(ddruntime::win32::get_last_error, m, &[])
}

fn set_last_error(m: &mut GuestMemory, e: u32) {
    call(ddruntime::win32::set_last_error, m, &[e]);
}

/// A desktop and the game's window (CreateWindowExW as 0x00DF1200 makes it).
fn game_window() -> (GuestMemory, Arc<Registry>, u32) {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    m.map_lazy(services::HEAP_BASE, services::HEAP_SIZE, READ | WRITE).unwrap();
    m.map(DATA, 0x2000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    r.insert(0x0050_0000, Binding::Lifted(wndproc)).unwrap();
    let r = Arc::new(r);
    r.install();
    user32::reset();
    let (wc, name) = (DATA, DATA + 0x100);
    for (k, u) in "MT\0".encode_utf16().enumerate() {
        m.set_u16(name + 2 * k as u32, u);
    }
    m.set_u32(wc, 0x30);
    m.set_u32(wc + 8, 0x0050_0000);
    m.set_u32(wc + 40, name);
    call(user32::register_class_ex_w, &mut m, &[wc]);
    assert_eq!(call(user32::imm_get_default_ime_wnd, &mut m, &[0]), 0, "no IME window before the first top-level window");
    let hwnd = call(user32::create_window_ex_w, &mut m, &[0, name, 0, 0x00CA_0000, 32, 32, 1286, 749, 0, 0, 0x0040_0000, 0]);
    assert_ne!(hwnd, 0);
    (m, r, hwnd)
}

#[test]
fn the_window_setup_0x01106c80_succeeds_on_the_default_context() {
    let (mut m, _r, hwnd) = game_window();
    let ime = call(user32::imm_get_default_ime_wnd, &mut m, &[hwnd]);
    assert_ne!(ime, 0, "the default IME window");
    assert_ne!(ime, hwnd);
    assert_eq!(call(user32::imm_get_default_ime_wnd, &mut m, &[0]), ime, "NULL: the thread's");
    assert_eq!(ime, user32::ime_window());
    assert!(user32::window(ime).is_none(), "not one of the game's windows");
    let c = call(user32::imm_get_context, &mut m, &[hwnd]);
    assert_ne!(c, 0);
    assert_eq!(call(user32::imm_get_context, &mut m, &[hwnd]), c, "the same context each time");
    assert_eq!(call(user32::imm_get_context, &mut m, &[0]), 0);
    // COMPOSITIONFORM {1, (0,0), (1,1,0,0)} and CANDIDATEFORM {0, 0x80, (0,0), (0,0x437,1,0x437)}
    let (comp, cand) = (DATA + 0x200, DATA + 0x240);
    for (k, v) in [1u32, 0, 0, 1, 1, 0, 0].into_iter().enumerate() {
        m.set_u32(comp + 4 * k as u32, v);
    }
    for (k, v) in [0u32, 0x80, 0, 0, 0, 0x437, 1, 0x437].into_iter().enumerate() {
        m.set_u32(cand + 4 * k as u32, v);
    }
    set_last_error(&mut m, 0);
    assert_eq!(call(user32::imm_set_composition_window, &mut m, &[c, comp]), 1);
    assert_eq!(call(user32::imm_set_candidate_window, &mut m, &[c, cand]), 1);
    assert_eq!(call(user32::imm_release_context, &mut m, &[hwnd, c]), 1);
    assert_eq!(call(user32::imm_release_context, &mut m, &[hwnd, c]), 1, "a second release too");
    assert_eq!(call(user32::imm_release_context, &mut m, &[hwnd, 0]), 1, "and NULL");
    assert_eq!(last_error(&mut m), 0);
    // a NULL context: FALSE, ERROR_INVALID_HANDLE
    assert_eq!(call(user32::imm_set_composition_window, &mut m, &[0, comp]), 0);
    assert_eq!(last_error(&mut m), ERROR_INVALID_HANDLE);
    set_last_error(&mut m, 0);
    assert_eq!(call(user32::imm_set_candidate_window, &mut m, &[0, cand]), 0);
    assert_eq!(last_error(&mut m), ERROR_INVALID_HANDLE);
    x86::set_ext_calls(None);
}

#[test]
fn no_ime_is_open_and_there_is_no_composition_or_candidate() {
    let (mut m, _r, hwnd) = game_window();
    let c = call(user32::imm_get_context, &mut m, &[hwnd]);
    set_last_error(&mut m, 0);
    assert_eq!(call(user32::imm_get_open_status, &mut m, &[c]), 0);
    assert_eq!(call(user32::imm_set_open_status, &mut m, &[c, 0]), 1, "0x01106680 closes it");
    assert_eq!(call(user32::imm_get_open_status, &mut m, &[c]), 0);
    assert_eq!(call(user32::imm_get_open_status, &mut m, &[0]), 0);
    assert_eq!(last_error(&mut m), 0);
    assert_eq!(call(user32::imm_set_open_status, &mut m, &[0, 0]), 0);
    assert_eq!(last_error(&mut m), ERROR_INVALID_HANDLE);
    // composition strings: 0, buffer untouched (GCS_COMPSTR, GCS_COMPATTR, GCS_RESULTSTR)
    let buf = DATA + 0x400;
    for k in 0..0x40 {
        m.set_u8(buf + k, 0xCC);
    }
    for f in [user32::imm_get_composition_string] {
        for index in [0x8u32, 0x10, 0x800] {
            for himc in [c, 0] {
                assert_eq!(call(f, &mut m, &[himc, index, buf, 0x40]), 0);
                assert_eq!(call(f, &mut m, &[himc, index, 0, 0]), 0, "the size asked for: 0");
            }
        }
    }
    assert!((0..0x40).all(|k| m.u8(buf + k) == 0xCC), "buffer untouched");
    // candidates
    let count = DATA + 0x500;
    m.set_u32(count, 0xCCCC_CCCC);
    assert_eq!(call(user32::imm_get_candidate_list_count_a, &mut m, &[c, count]), 144, "Windows' answer for the empty list");
    assert_eq!(m.u32(count), 0);
    m.set_u32(count, 0xCCCC_CCCC);
    assert_eq!(call(user32::imm_get_candidate_list_count_a, &mut m, &[0, count]), 0);
    assert_eq!(m.u32(count), 0);
    let list = DATA + 0x600;
    for k in 0..0x100 {
        m.set_u8(list + k, 0xCC);
    }
    assert_eq!(call(user32::imm_get_candidate_list_w, &mut m, &[c, 0, list, 0x1000]), 0);
    assert_eq!(call(user32::imm_get_candidate_list_w, &mut m, &[c, 0, 0, 0]), 0);
    assert!((0..0x100).all(|k| m.u8(list + k) == 0xCC));
    // the last key, polled each frame by 0x01106720: VK_PROCESSKEY
    assert_eq!(call(input::imm_virtual_key, &mut m, &[hwnd]), 0xE5);
    assert_eq!(call(input::imm_virtual_key, &mut m, &[0]), 0xE5);
    x86::set_ext_calls(None);
}

#[test]
fn every_imm32_import_of_the_game_is_bound() {
    let names = ["ImmGetDefaultIMEWnd", "ImmGetContext", "ImmReleaseContext", "ImmSetCompositionWindow", "ImmSetCandidateWindow",
        "ImmSetOpenStatus", "ImmGetOpenStatus", "ImmGetCompositionStringA", "ImmGetCompositionStringW",
        "ImmGetCandidateListCountA", "ImmGetCandidateListW", "ImmGetVirtualKey"];
    for n in names {
        let bound = user32::APIS.iter().chain(input::APIS).any(|(d, f, _)| d.eq_ignore_ascii_case("IMM32.dll") && *f == n);
        assert!(bound, "IMM32.dll!{n}");
    }
}

thread_local! {
    static IME_LOG: std::cell::RefCell<Vec<(u32, u32, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Logs the IME messages (WM_IME_SETCONTEXT, WM_IME_NOTIFY) the window gets.
fn logging_wndproc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (msg, wparam, lparam) = (m.u32(cpu.esp + 8), m.u32(cpu.esp + 12), m.u32(cpu.esp + 16));
    if msg == user32::WM_IME_SETCONTEXT || msg == user32::WM_IME_NOTIFY {
        IME_LOG.with(|l| l.borrow_mut().push((msg, wparam, lparam)));
    }
    user32::def_window_proc(m, cpu);
}

fn ime_log() -> Vec<(u32, u32, u32)> {
    IME_LOG.with(|l| l.borrow_mut().drain(..).collect())
}

#[test]
fn a_shown_focused_window_is_told_of_context_changes_and_errors_follow_windows() {
    let (mut m, _r, hwnd) = game_window();
    // rebind the class's window procedure to the logging one
    let mut r = Registry::new();
    r.insert(0x0050_0000, Binding::Lifted(logging_wndproc)).unwrap();
    let r = Arc::new(r);
    r.install();
    let c = call(user32::imm_get_context, &mut m, &[hwnd]);
    let (comp, cand) = (DATA + 0x200, DATA + 0x240);
    for (k, v) in [1u32, 0, 0, 1, 1, 0, 0].into_iter().enumerate() {
        m.set_u32(comp + 4 * k as u32, v);
    }
    for (k, v) in [0u32, 0x80, 0, 0, 0, 0x437, 1, 0x437].into_iter().enumerate() {
        m.set_u32(cand + 4 * k as u32, v);
    }
    // before ShowWindow (as 0x01106C80 calls them): nothing is sent
    call(user32::imm_set_composition_window, &mut m, &[c, comp]);
    call(user32::imm_set_candidate_window, &mut m, &[c, cand]);
    assert!(ime_log().is_empty());
    // showing activates the window: the context becomes its own
    call(user32::show_window, &mut m, &[hwnd, 1]);
    assert_eq!(ime_log(), [(0x281, 1, 0xC000_000F), (0x282, 2, 0)], "WM_IME_SETCONTEXT(ISC_SHOWUIALL), IMN_OPENSTATUSWINDOW");
    // now each change is notified, and success clears the last error
    set_last_error(&mut m, 0xDEAD);
    assert_eq!(call(user32::imm_set_composition_window, &mut m, &[c, comp]), 1);
    assert_eq!(last_error(&mut m), 0);
    assert_eq!(ime_log(), [(0x282, 0xB, 0)], "IMN_SETCOMPOSITIONWINDOW");
    assert_eq!(call(user32::imm_set_candidate_window, &mut m, &[c, cand]), 1);
    assert_eq!(ime_log(), [(0x282, 9, 1)], "IMN_SETCANDIDATEPOS, 1 << dwIndex");
    set_last_error(&mut m, 0xDEAD);
    assert_eq!(call(user32::imm_set_open_status, &mut m, &[c, 0]), 1);
    assert_eq!(last_error(&mut m), 0);
    assert!(ime_log().is_empty(), "no change, no notification");
    assert_eq!(call(user32::imm_set_open_status, &mut m, &[c, 1]), 1);
    assert_eq!(call(user32::imm_get_open_status, &mut m, &[c]), 1);
    assert_eq!(call(user32::imm_set_open_status, &mut m, &[c, 0]), 1);
    assert_eq!(call(user32::imm_get_open_status, &mut m, &[c]), 0);
    assert_eq!(ime_log(), [(0x282, 8, 0), (0x282, 8, 0)], "IMN_SETOPENSTATUS on each change");
    // getting and releasing the context leave the last error alone
    set_last_error(&mut m, 0xDEAD);
    call(user32::imm_get_context, &mut m, &[hwnd]);
    call(user32::imm_release_context, &mut m, &[hwnd, c]);
    assert_eq!(last_error(&mut m), 0xDEAD);
    // a handle that is no window: NULL, ERROR_INVALID_WINDOW_HANDLE
    set_last_error(&mut m, 0);
    assert_eq!(call(user32::imm_get_context, &mut m, &[0x1234_5678]), 0);
    assert_eq!(last_error(&mut m), 1400);
    set_last_error(&mut m, 0);
    assert_eq!(call(user32::imm_get_default_ime_wnd, &mut m, &[0x1234_5678]), 0);
    assert_eq!(last_error(&mut m), 1400);
    x86::set_ext_calls(None);
}
