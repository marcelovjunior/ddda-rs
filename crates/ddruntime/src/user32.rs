//! USER32 windows and messages (plus the GDI/SHELL32/WINMM calls the
//! window code makes): a headless desktop with one guest thread's message
//! queue (docs/RUNTIME.md, "Windowing and input").
//!
//! Windows are records the runtime keeps. Window procedures are guest code
//! (the game's is 0x00DF1550, registered by 0x00DF31F0); the runtime calls
//! them through the dispatcher with the messages Windows would send, in
//! Windows' order, so the game's own handlers run: CreateWindowEx sends
//! WM_NCCREATE and WM_CREATE, ShowWindow the show/activation sequence,
//! UpdateWindow WM_PAINT. Messages Windows only sends for non-client
//! painting, IME and Z-order negotiation (WM_NCPAINT, WM_ERASEBKGND,
//! WM_IME_*, WM_WINDOWPOSCHANGING, WM_GETMINMAXINFO, WM_NCCALCSIZE) are
//! not sent: a headless window has no frame to paint or size against.
//! The host presentation layer (later) feeds input and size changes in
//! through [`post`].
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

type Api = fn(&mut dyn Mem, &mut Cpu);

pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_MOVE: u32 = 0x0003;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_ACTIVATE: u32 = 0x0006;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SHOWWINDOW: u32 = 0x0018;
pub const WM_ACTIVATEAPP: u32 = 0x001C;
pub const WM_WINDOWPOSCHANGED: u32 = 0x0047;
pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_NCACTIVATE: u32 = 0x0086;
pub const WM_SYSCOMMAND: u32 = 0x0112;
const SC_CLOSE: u32 = 0xF060;

const PM_REMOVE: u32 = 1;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const CW_USEDEFAULT: u32 = 0x8000_0000;
const GWL_WNDPROC: i32 = -4;
const GWL_STYLE: i32 = -16;
const GWL_EXSTYLE: i32 = -20;

/// Window handles: even values in a range no guest pointer uses.
pub const HWND_BASE: u32 = 0x0001_0010;
/// Class atoms (RegisterClassEx's result).
const ATOM_BASE: u32 = 0xC000;
/// Opaque handles for resources the headless desktop does not draw:
/// icons, cursors, stock GDI objects, accelerator tables, the system
/// menu, paint DCs, device notifications.
pub const RESOURCE_HANDLES: u32 = 0x7FC0_0000;
/// Where a window procedure the runtime calls returns to (never executed).
pub const CALLBACK_RETURN: u32 = 0xFFFF_FFE0;

#[derive(Clone, Debug)]
pub struct Class {
    pub name: String,
    pub atom: u32,
    pub wndproc: u32,
    pub style: u32,
    pub instance: u32,
}

#[derive(Clone, Debug)]
pub struct Window {
    pub class: String,
    pub title: String,
    pub wndproc: u32,
    pub style: u32,
    pub ex_style: u32,
    /// Window rectangle in screen coordinates (left, top, right, bottom).
    pub rect: [i32; 4],
    pub parent: u32,
    pub instance: u32,
    pub visible: bool,
    /// Shown at least once (the first show sends WM_SIZE/WM_MOVE).
    pub shown: bool,
    pub needs_paint: bool,
    pub props: HashMap<String, u32>,
    pub user_data: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msg {
    pub hwnd: u32,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
}

#[derive(Default)]
struct Desktop {
    classes: Vec<Class>,
    windows: HashMap<u32, Window>,
    next_hwnd: u32,
    queue: VecDeque<Msg>,
    quit: Option<u32>,
    active: u32,
    focus: u32,
    app_active: bool,
    next_resource: u32,
    resources: HashMap<String, u32>,
    cursor: (i32, i32),
    cursor_shown: i32,
    period_ms: Vec<u32>,
}

thread_local! {
    static DESKTOP: RefCell<Desktop> = RefCell::new(Desktop::default());
}

fn desk<R>(f: impl FnOnce(&mut Desktop) -> R) -> R {
    DESKTOP.with(|d| f(&mut d.borrow_mut()))
}

/// Forget every class, window and message (a new session).
pub fn reset() {
    desk(|d| *d = Desktop::default());
}

/// A copy of a window's record, for the host layer and tests.
pub fn window(hwnd: u32) -> Option<Window> {
    desk(|d| d.windows.get(&hwnd).cloned())
}

/// Every window handle, in creation order.
pub fn windows() -> Vec<u32> {
    let mut v: Vec<u32> = desk(|d| d.windows.keys().copied().collect());
    v.sort();
    v
}

/// Queue a message for the guest thread (host input, size changes).
pub fn post(msg: Msg) {
    desk(|d| d.queue.push_back(msg));
}

/// Whether PostQuitMessage was called, with its exit code.
pub fn quit_code() -> Option<u32> {
    desk(|d| d.quit)
}

/// One opaque handle per named resource (stable across calls).
fn resource(name: String) -> u32 {
    desk(|d| {
        if let Some(&h) = d.resources.get(&name) {
            return h;
        }
        d.next_resource += 1;
        let h = RESOURCE_HANDLES + 4 * d.next_resource;
        d.resources.insert(name, h);
        h
    })
}

pub(crate) fn guest_str_a(m: &dyn Mem, p: u32) -> String {
    (0..).map(|k| m.u8(p.wrapping_add(k))).take_while(|&b| b != 0).map(|b| b as char).collect()
}

pub(crate) fn guest_str_w(m: &dyn Mem, p: u32) -> String {
    let units: Vec<u16> = (0..).map(|k| m.u16(p.wrapping_add(2 * k))).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units)
}

/// A class or resource name argument: a string, or an integer atom/ID
/// (MAKEINTATOM/MAKEINTRESOURCE: high word zero).
fn name_arg(m: &dyn Mem, p: u32, wide: bool) -> String {
    if p < 0x1_0000 {
        format!("#{p}")
    } else if wide {
        guest_str_w(m, p)
    } else {
        guest_str_a(m, p)
    }
}

// --- calling guest window procedures ----------------------------------------

/// Call guest code `target` as stdcall with `args`, from inside a service:
/// the arguments and a return address are pushed below the service's frame
/// and the dispatcher runs the target. eax is the result; ecx and edx are
/// the callee's to clobber (as across any call), the other registers are
/// preserved by the callee's own ABI and checked here.
pub fn call_guest(m: &mut dyn Mem, cpu: &mut Cpu, target: u32, args: &[u32]) -> u32 {
    let (esp, ebx, ebp, esi, edi) = (cpu.esp, cpu.ebx, cpu.ebp, cpu.esi, cpu.edi);
    for &a in args.iter().rev() {
        cpu.esp = cpu.esp.wrapping_sub(4);
        m.set_u32(cpu.esp, a);
    }
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, CALLBACK_RETURN);
    x86::call_ext(m, cpu, target);
    assert_eq!(cpu.esp, esp, "guest callback {target:#010x} did not return with stdcall cleanup of {} argument(s)", args.len());
    assert_eq!((cpu.ebx, cpu.ebp, cpu.esi, cpu.edi), (ebx, ebp, esi, edi), "guest callback {target:#010x} clobbered a callee-saved register");
    cpu.eax
}

/// SendMessage: the window procedure, synchronously (single guest thread).
pub fn send(m: &mut dyn Mem, cpu: &mut Cpu, hwnd: u32, message: u32, wparam: u32, lparam: u32) -> u32 {
    let Some(proc_) = desk(|d| d.windows.get(&hwnd).map(|w| w.wndproc)) else { return 0 };
    call_guest(m, cpu, proc_, &[hwnd, message, wparam, lparam])
}

fn lo_hi(lo: i32, hi: i32) -> u32 {
    (lo as u32 & 0xFFFF) | ((hi as u32 & 0xFFFF) << 16)
}

/// The frame AdjustWindowRect adds for `style` (left, top, right, bottom).
fn frame(style: u32) -> [i32; 4] {
    let mut r = [0i32; 4];
    crate::win32::adjust_rect(&mut r, style, false);
    r
}

/// Client rectangle size of a window.
fn client_size(w: &Window) -> (i32, i32) {
    let f = frame(w.style);
    (
        (w.rect[2] - w.rect[0]) - (f[2] - f[0]),
        (w.rect[3] - w.rect[1]) - (f[3] - f[1]),
    )
}

/// The screen position of a window's client origin.
fn client_origin(w: &Window) -> (i32, i32) {
    let f = frame(w.style);
    (w.rect[0] - f[0], w.rect[1] - f[1])
}

fn with_temp<R>(m: &mut dyn Mem, size: u32, f: impl FnOnce(&mut dyn Mem, u32) -> R) -> R {
    let p = with_state(|s| s.heap.alloc(size)).expect("guest heap");
    crate::services::zero(m, p, size);
    let r = f(m, p);
    with_state(|s| s.heap.free(p)).expect("temporary block");
    r
}

// --- classes and windows -----------------------------------------------------

/// RegisterClassExA/W(const WNDCLASSEX*) -> atom (0 if the class exists).
fn register_class(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let wc = arg(m, cpu, 0);
    let name = name_arg(m, m.u32(wc + 40), wide);
    let (style, wndproc, instance) = (m.u32(wc + 4), m.u32(wc + 8), m.u32(wc + 20));
    cpu.eax = desk(|d| {
        if d.classes.iter().any(|c| c.name.eq_ignore_ascii_case(&name)) {
            return 0;
        }
        let atom = ATOM_BASE + d.classes.len() as u32;
        d.classes.push(Class { name, atom, wndproc, style, instance });
        atom
    });
    ret(m, cpu, 4);
}
pub fn register_class_ex_a(m: &mut dyn Mem, cpu: &mut Cpu) { register_class(m, cpu, false) }
pub fn register_class_ex_w(m: &mut dyn Mem, cpu: &mut Cpu) { register_class(m, cpu, true) }

/// CreateWindowExA/W(exstyle, class, name, style, x, y, w, h, parent, menu,
/// instance, param): the record, then WM_NCCREATE and WM_CREATE with a
/// CREATESTRUCT; NULL if either refuses (the window is destroyed).
fn create_window(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let a: Vec<u32> = (0..12).map(|k| arg(m, cpu, k)).collect();
    let class_name = {
        let p = a[1];
        if p < 0x1_0000 {
            desk(|d| d.classes.iter().find(|c| c.atom == p).map(|c| c.name.clone())).unwrap_or_default()
        } else {
            name_arg(m, p, wide)
        }
    };
    let title = if a[2] == 0 { String::new() } else { name_arg(m, a[2], wide) };
    let Some(class) = desk(|d| d.classes.iter().find(|c| c.name.eq_ignore_ascii_case(&class_name)).cloned()) else {
        with_state(|s| s.last_error = 1407); // ERROR_CANNOT_FIND_WND_CLASS
        cpu.eax = 0;
        return ret(m, cpu, 48);
    };
    let (style, ex_style) = (a[3], a[0]);
    let def = |v: u32, d: i32| if v == CW_USEDEFAULT { d } else { v as i32 };
    let (x, y) = (def(a[4], 0), def(a[5], 0));
    let (w, h) = (def(a[6], 640), def(a[7], 480));
    let hwnd = desk(|d| {
        let hwnd = HWND_BASE + 2 * d.next_hwnd;
        d.next_hwnd += 1;
        d.windows.insert(hwnd, Window {
            class: class.name.clone(),
            title,
            wndproc: class.wndproc,
            style: style & !WS_VISIBLE,
            ex_style,
            rect: [x, y, x + w, y + h],
            parent: a[8],
            instance: a[10],
            visible: false,
            shown: false,
            needs_paint: true,
            props: HashMap::new(),
            user_data: 0,
        });
        hwnd
    });
    // CREATESTRUCT: params, instance, menu, parent, cy, cx, y, x, style,
    // name, class, exstyle
    let ok = with_temp(m, 0x30, |m, cs| {
        for (k, v) in [a[11], a[10], a[9], a[8], h as u32, w as u32, y as u32, x as u32, style, a[2], a[1], ex_style].into_iter().enumerate() {
            m.set_u32(cs + 4 * k as u32, v);
        }
        send(m, cpu, hwnd, WM_NCCREATE, 0, cs) != 0 && send(m, cpu, hwnd, WM_CREATE, 0, cs) != u32::MAX
    });
    if !ok {
        destroy(m, cpu, hwnd);
        cpu.eax = 0;
        return ret(m, cpu, 48);
    }
    if style & WS_VISIBLE != 0 {
        show(m, cpu, hwnd, 1);
    }
    cpu.eax = hwnd;
    ret(m, cpu, 48);
}
pub fn create_window_ex_a(m: &mut dyn Mem, cpu: &mut Cpu) { create_window(m, cpu, false) }
pub fn create_window_ex_w(m: &mut dyn Mem, cpu: &mut Cpu) { create_window(m, cpu, true) }

/// Destroy a window and (first) the windows it owns: WM_DESTROY, then
/// WM_NCDESTROY, then the record goes. A destroyed active window leaves
/// the application without an active window.
fn destroy(m: &mut dyn Mem, cpu: &mut Cpu, hwnd: u32) -> bool {
    if window(hwnd).is_none() {
        return false;
    }
    let owned: Vec<u32> = windows().into_iter().filter(|&h| window(h).map(|w| w.parent) == Some(hwnd)).collect();
    for h in owned {
        destroy(m, cpu, h);
    }
    send(m, cpu, hwnd, WM_DESTROY, 0, 0);
    send(m, cpu, hwnd, WM_NCDESTROY, 0, 0);
    desk(|d| {
        d.windows.remove(&hwnd);
        if d.active == hwnd {
            d.active = 0;
        }
        if d.focus == hwnd {
            d.focus = 0;
        }
        d.queue.retain(|q| q.hwnd != hwnd);
    });
    true
}

/// DestroyWindow(hwnd) -> BOOL.
pub fn destroy_window(m: &mut dyn Mem, cpu: &mut Cpu) {
    let hwnd = arg(m, cpu, 0);
    cpu.eax = destroy(m, cpu, hwnd) as u32;
    ret(m, cpu, 4);
}

/// Make `hwnd` the active, focused window, with Windows' message order:
/// the old window is deactivated, WM_ACTIVATEAPP if the application was
/// not active, then WM_NCACTIVATE, WM_ACTIVATE (WA_ACTIVE) and WM_SETFOCUS.
fn activate(m: &mut dyn Mem, cpu: &mut Cpu, hwnd: u32) {
    let (old, app_active, old_focus) = desk(|d| (d.active, d.app_active, d.focus));
    if old == hwnd {
        return;
    }
    if old != 0 {
        send(m, cpu, old, WM_NCACTIVATE, 0, 0);
        send(m, cpu, old, WM_ACTIVATE, 0, hwnd);
    }
    desk(|d| {
        d.active = hwnd;
        d.app_active = true;
    });
    if !app_active {
        send(m, cpu, hwnd, WM_ACTIVATEAPP, 1, crate::services::MAIN_THREAD_ID);
    }
    send(m, cpu, hwnd, WM_NCACTIVATE, 1, 0);
    send(m, cpu, hwnd, WM_ACTIVATE, 1, old);
    if old_focus != 0 && old_focus != hwnd {
        send(m, cpu, old_focus, WM_KILLFOCUS, hwnd, 0);
    }
    desk(|d| d.focus = hwnd);
    send(m, cpu, hwnd, WM_SETFOCUS, old_focus, 0);
}

/// Tell a window its position changed (WM_WINDOWPOSCHANGED with a
/// WINDOWPOS: hwnd, insertAfter, x, y, cx, cy, flags); DefWindowProc turns
/// it into WM_SIZE and WM_MOVE.
fn pos_changed(m: &mut dyn Mem, cpu: &mut Cpu, hwnd: u32, flags: u32) {
    let Some(w) = window(hwnd) else { return };
    with_temp(m, 28, |m, wp| {
        let r = w.rect;
        for (k, v) in [hwnd, 0, r[0] as u32, r[1] as u32, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32, flags].into_iter().enumerate() {
            m.set_u32(wp + 4 * k as u32, v);
        }
        send(m, cpu, hwnd, WM_WINDOWPOSCHANGED, 0, wp);
    });
}

const SWP_NOSIZE: u32 = 0x1;
const SWP_NOMOVE: u32 = 0x2;
const SWP_NOZORDER: u32 = 0x4;
const SWP_NOACTIVATE: u32 = 0x10;
const SWP_SHOWWINDOW: u32 = 0x40;
const SWP_HIDEWINDOW: u32 = 0x80;

/// ShowWindow's effect; returns whether the window was visible.
fn show(m: &mut dyn Mem, cpu: &mut Cpu, hwnd: u32, cmd: u32) -> bool {
    let Some(w) = window(hwnd) else { return false };
    let was = w.visible;
    match cmd {
        0 => {
            // SW_HIDE
            if was {
                send(m, cpu, hwnd, WM_SHOWWINDOW, 0, 0);
                desk(|d| d.windows.get_mut(&hwnd).map(|w| w.visible = false));
                pos_changed(m, cpu, hwnd, SWP_HIDEWINDOW | SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
        _ => {
            // SW_SHOWNOACTIVATE (4), SW_SHOWMINNOACTIVE (7), SW_SHOWNA (8)
            // show without activating
            let activates = !matches!(cmd, 4 | 7 | 8) && w.style & WS_CHILD == 0;
            if !was {
                send(m, cpu, hwnd, WM_SHOWWINDOW, 1, 0);
                desk(|d| d.windows.get_mut(&hwnd).map(|w| { w.visible = true; w.needs_paint = true; }));
            }
            if activates {
                activate(m, cpu, hwnd);
            }
            if !was {
                let flags = SWP_SHOWWINDOW | SWP_NOZORDER | if activates { 0 } else { SWP_NOACTIVATE };
                // the first show reports the size and position (DefWindowProc
                // sends WM_SIZE/WM_MOVE for a window not yet sized)
                let first = desk(|d| d.windows.get_mut(&hwnd).map(|w| !std::mem::replace(&mut w.shown, true))).unwrap_or(false);
                pos_changed(m, cpu, hwnd, if first { flags } else { flags | SWP_NOSIZE | SWP_NOMOVE });
            }
        }
    }
    was
}

/// ShowWindow(hwnd, cmd) -> previously visible.
pub fn show_window(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, cmd) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = show(m, cpu, hwnd, cmd) as u32;
    ret(m, cpu, 8);
}

/// UpdateWindow(hwnd): WM_PAINT now if the window is visible and invalid.
pub fn update_window(m: &mut dyn Mem, cpu: &mut Cpu) {
    let hwnd = arg(m, cpu, 0);
    cpu.eax = match window(hwnd) {
        Some(w) => {
            if w.visible && w.needs_paint {
                send(m, cpu, hwnd, WM_PAINT, 0, 0);
            }
            1
        }
        None => 0,
    };
    ret(m, cpu, 4);
}

/// BeginPaint(hwnd, PAINTSTRUCT*) -> HDC: validates the window; the paint
/// rectangle is the client area.
pub fn begin_paint(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, ps) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match window(hwnd) {
        Some(w) => {
            desk(|d| d.windows.get_mut(&hwnd).map(|w| w.needs_paint = false));
            let hdc = resource(format!("dc {hwnd:#x}"));
            let (cw, ch) = client_size(&w);
            crate::services::zero(m, ps, 64);
            m.set_u32(ps, hdc);
            m.set_u32(ps + 16, cw as u32);
            m.set_u32(ps + 20, ch as u32);
            hdc
        }
        None => 0,
    };
    ret(m, cpu, 8);
}

/// EndPaint(hwnd, PAINTSTRUCT*): TRUE.
pub fn end_paint(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 8);
}

/// DefWindowProcA/W(hwnd, msg, wparam, lparam): the default handling the
/// messages this desktop sends need.
pub fn def_window_proc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, msg, wparam, lparam) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = match msg {
        WM_NCCREATE | WM_NCACTIVATE | WM_ERASEBKGND => 1,
        WM_CLOSE => {
            destroy(m, cpu, hwnd);
            0
        }
        WM_PAINT => {
            desk(|d| d.windows.get_mut(&hwnd).map(|w| w.needs_paint = false));
            0
        }
        WM_SYSCOMMAND if wparam & 0xFFF0 == SC_CLOSE => {
            send(m, cpu, hwnd, WM_CLOSE, 0, 0);
            0
        }
        WM_WINDOWPOSCHANGED => {
            let flags = m.u32(lparam + 24);
            if let Some(w) = window(hwnd) {
                if flags & SWP_NOSIZE == 0 {
                    let (cw, ch) = client_size(&w);
                    send(m, cpu, hwnd, WM_SIZE, 0, lo_hi(cw, ch)); // SIZE_RESTORED
                }
                if flags & SWP_NOMOVE == 0 {
                    let (cx, cy) = client_origin(&w);
                    send(m, cpu, hwnd, WM_MOVE, 0, lo_hi(cx, cy));
                }
            }
            0
        }
        _ => 0,
    };
    ret(m, cpu, 16);
}

/// SendMessageA(hwnd, msg, wparam, lparam).
pub fn send_message(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, msg, wparam, lparam) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    cpu.eax = send(m, cpu, hwnd, msg, wparam, lparam);
    ret(m, cpu, 16);
}

// --- the message queue ---------------------------------------------------------

/// PeekMessageA(MSG*, hwnd, min, max, flags): posted messages in order;
/// WM_QUIT once the queue is empty after PostQuitMessage; then WM_PAINT for
/// an invalid visible window (Windows synthesises it last). FALSE if none.
pub fn peek_message(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (out, hwnd, min, max, flags) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    let in_range = |msg: u32| (min == 0 && max == 0) || (min..=max).contains(&msg);
    let remove = flags & PM_REMOVE != 0;
    let found = desk(|d| {
        let pos = d.queue.iter().position(|q| (hwnd == 0 || q.hwnd == hwnd) && in_range(q.message));
        if let Some(k) = pos {
            return Some(if remove { d.queue.remove(k).unwrap() } else { d.queue[k] });
        }
        if let Some(code) = d.quit.filter(|_| hwnd == 0 && in_range(WM_QUIT)) {
            if remove {
                d.quit = None;
            }
            return Some(Msg { hwnd: 0, message: WM_QUIT, wparam: code, lparam: 0 });
        }
        if in_range(WM_PAINT) {
            let mut paint: Vec<u32> = d.windows.iter()
                .filter(|(h, w)| w.visible && w.needs_paint && (hwnd == 0 || **h == hwnd))
                .map(|(h, _)| *h).collect();
            paint.sort();
            if let Some(&h) = paint.first() {
                return Some(Msg { hwnd: h, message: WM_PAINT, wparam: 0, lparam: 0 });
            }
        }
        None
    });
    cpu.eax = match found {
        Some(msg) => {
            let (cx, cy) = desk(|d| d.cursor);
            for (k, v) in [msg.hwnd, msg.message, msg.wparam, msg.lparam, crate::services::runtime_clock().as_millis() as u32, cx as u32, cy as u32].into_iter().enumerate() {
                m.set_u32(out + 4 * k as u32, v);
            }
            1
        }
        None => 0,
    };
    ret(m, cpu, 20);
}

/// TranslateMessage(MSG*): character messages come from the host layer
/// with the key messages, so nothing is generated here. FALSE.
pub fn translate_message(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 4);
}

/// DispatchMessageW(MSG*): the window procedure's result.
pub fn dispatch_message(m: &mut dyn Mem, cpu: &mut Cpu) {
    let msg = arg(m, cpu, 0);
    let (hwnd, message, wparam, lparam) = (m.u32(msg), m.u32(msg + 4), m.u32(msg + 8), m.u32(msg + 12));
    cpu.eax = if hwnd == 0 { 0 } else { send(m, cpu, hwnd, message, wparam, lparam) };
    ret(m, cpu, 4);
}

/// PostQuitMessage(code).
pub fn post_quit_message(m: &mut dyn Mem, cpu: &mut Cpu) {
    let code = arg(m, cpu, 0);
    desk(|d| d.quit = Some(code));
    ret(m, cpu, 4);
}

// --- geometry -----------------------------------------------------------------

fn write_rect(m: &mut dyn Mem, at: u32, r: [i32; 4]) {
    for (k, v) in r.into_iter().enumerate() {
        m.set_u32(at + 4 * k as u32, v as u32);
    }
}

/// GetClientRect(hwnd, RECT*): (0, 0, width, height).
pub fn get_client_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, r) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match window(hwnd) {
        Some(w) => {
            let (cw, ch) = client_size(&w);
            write_rect(m, r, [0, 0, cw, ch]);
            1
        }
        None => 0,
    };
    ret(m, cpu, 8);
}

/// GetWindowRect(hwnd, RECT*): screen coordinates.
pub fn get_window_rect(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, r) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match window(hwnd) {
        Some(w) => {
            write_rect(m, r, w.rect);
            1
        }
        None => 0,
    };
    ret(m, cpu, 8);
}

fn map_point(m: &mut dyn Mem, cpu: &mut Cpu, sign: i32) {
    let (hwnd, p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match window(hwnd) {
        Some(w) => {
            let (ox, oy) = client_origin(&w);
            m.set_u32(p, (m.u32(p) as i32 + sign * ox) as u32);
            m.set_u32(p + 4, (m.u32(p + 4) as i32 + sign * oy) as u32);
            1
        }
        None => 0,
    };
    ret(m, cpu, 8);
}
/// ClientToScreen(hwnd, POINT*).
pub fn client_to_screen(m: &mut dyn Mem, cpu: &mut Cpu) { map_point(m, cpu, 1) }
/// ScreenToClient(hwnd, POINT*).
pub fn screen_to_client(m: &mut dyn Mem, cpu: &mut Cpu) { map_point(m, cpu, -1) }

/// IsIconic(hwnd): headless windows are never minimised. FALSE.
pub fn is_iconic(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 4);
}

/// SetWindowPos(hwnd, after, x, y, cx, cy, flags) -> TRUE; a move or size
/// is reported with WM_WINDOWPOSCHANGED as Windows does.
pub fn set_window_pos(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a: Vec<u32> = (0..7).map(|k| arg(m, cpu, k)).collect();
    let (hwnd, flags) = (a[0], a[6]);
    if window(hwnd).is_none() {
        cpu.eax = 0;
        return ret(m, cpu, 28);
    }
    desk(|d| {
        let w = d.windows.get_mut(&hwnd).unwrap();
        let (mut x, mut y) = (w.rect[0], w.rect[1]);
        let (mut cx, mut cy) = (w.rect[2] - w.rect[0], w.rect[3] - w.rect[1]);
        if flags & SWP_NOMOVE == 0 {
            (x, y) = (a[2] as i32, a[3] as i32);
        }
        if flags & SWP_NOSIZE == 0 {
            (cx, cy) = (a[4] as i32, a[5] as i32);
        }
        w.rect = [x, y, x + cx, y + cy];
    });
    if flags & SWP_SHOWWINDOW != 0 {
        show(m, cpu, hwnd, if flags & SWP_NOACTIVATE != 0 { 8 } else { 5 });
    } else if flags & SWP_HIDEWINDOW != 0 {
        show(m, cpu, hwnd, 0);
    }
    if flags & (SWP_NOMOVE | SWP_NOSIZE) != (SWP_NOMOVE | SWP_NOSIZE) {
        pos_changed(m, cpu, hwnd, flags | SWP_NOZORDER);
    }
    cpu.eax = 1;
    ret(m, cpu, 28);
}

/// SetWindowLongA(hwnd, index, value) -> the previous value. The window
/// procedure, styles and user data are kept; other indices are refused.
pub fn set_window_long(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, index, value) = (arg(m, cpu, 0), arg(m, cpu, 1) as i32, arg(m, cpu, 2));
    cpu.eax = desk(|d| {
        let w = d.windows.get_mut(&hwnd)?;
        let slot = match index {
            GWL_WNDPROC => &mut w.wndproc,
            GWL_STYLE => &mut w.style,
            GWL_EXSTYLE => &mut w.ex_style,
            -21 => &mut w.user_data, // GWL_USERDATA
            _ => panic!("SetWindowLongA index {index} is not modelled"),
        };
        Some(std::mem::replace(slot, value))
    }).unwrap_or(0);
    ret(m, cpu, 12);
}

/// SetClassLongA(hwnd, index, value) -> previous: the class's icon or
/// cursor (GCL_HCURSOR -12, GCL_HICON -14, GCL_HICONSM -34) are resources
/// the headless desktop does not draw; accepted, previous value 0.
pub fn set_class_long(m: &mut dyn Mem, cpu: &mut Cpu) {
    let index = arg(m, cpu, 1) as i32;
    assert!(matches!(index, -12 | -14 | -34 | -10), "SetClassLongA index {index} is not modelled");
    cpu.eax = 0;
    ret(m, cpu, 12);
}

// --- properties ------------------------------------------------------------------

fn prop_key(m: &dyn Mem, p: u32) -> String {
    name_arg(m, p, false)
}
/// SetPropA(hwnd, name, data) -> TRUE.
pub fn set_prop(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, key, data) = (arg(m, cpu, 0), prop_key(m, arg(m, cpu, 1)), arg(m, cpu, 2));
    cpu.eax = desk(|d| d.windows.get_mut(&hwnd).map(|w| w.props.insert(key, data)).is_some()) as u32;
    ret(m, cpu, 12);
}
/// GetPropA(hwnd, name) -> data or NULL.
pub fn get_prop(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, key) = (arg(m, cpu, 0), prop_key(m, arg(m, cpu, 1)));
    cpu.eax = desk(|d| d.windows.get(&hwnd).and_then(|w| w.props.get(&key).copied())).unwrap_or(0);
    ret(m, cpu, 8);
}
/// RemovePropA(hwnd, name) -> the data removed or NULL.
pub fn remove_prop(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, key) = (arg(m, cpu, 0), prop_key(m, arg(m, cpu, 1)));
    cpu.eax = desk(|d| d.windows.get_mut(&hwnd).and_then(|w| w.props.remove(&key))).unwrap_or(0);
    ret(m, cpu, 8);
}

/// FindWindowW(class, title): windows of this process only (the headless
/// desktop has no others).
pub fn find_window_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let class = (arg(m, cpu, 0) != 0).then(|| name_arg(m, arg(m, cpu, 0), true));
    let title = (arg(m, cpu, 1) != 0).then(|| guest_str_w(m, arg(m, cpu, 1)));
    cpu.eax = windows().into_iter().find(|&h| {
        let w = window(h).unwrap();
        class.as_ref().is_none_or(|c| w.class.eq_ignore_ascii_case(c)) && title.as_ref().is_none_or(|t| &w.title == t)
    }).unwrap_or(0);
    ret(m, cpu, 8);
}

// --- resources the headless desktop does not draw ------------------------------

/// LoadIconA / LoadCursorA(instance, name): a handle per resource.
pub fn load_icon(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (inst, name) = (arg(m, cpu, 0), name_arg(m, arg(m, cpu, 1), false));
    cpu.eax = resource(format!("icon {inst:#x} {name}"));
    ret(m, cpu, 8);
}
pub fn load_cursor(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (inst, name) = (arg(m, cpu, 0), name_arg(m, arg(m, cpu, 1), false));
    cpu.eax = resource(format!("cursor {inst:#x} {name}"));
    ret(m, cpu, 8);
}
/// LoadImageA(instance, name, type, cx, cy, flags): a handle per image and size.
pub fn load_image(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (inst, name) = (arg(m, cpu, 0), name_arg(m, arg(m, cpu, 1), false));
    let (kind, cx, cy) = (arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    cpu.eax = resource(format!("image {inst:#x} {name} {kind} {cx}x{cy}"));
    ret(m, cpu, 24);
}
/// LoadAcceleratorsA(instance, name).
pub fn load_accelerators(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (inst, name) = (arg(m, cpu, 0), name_arg(m, arg(m, cpu, 1), false));
    cpu.eax = resource(format!("accelerators {inst:#x} {name}"));
    ret(m, cpu, 8);
}
/// GetSystemMenu(hwnd, revert): a menu handle (revert TRUE returns NULL).
pub fn get_system_menu(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, revert) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = if revert != 0 || window(hwnd).is_none() { 0 } else { resource(format!("system menu {hwnd:#x}")) };
    ret(m, cpu, 8);
}
/// RegisterDeviceNotificationA(recipient, filter, flags): a notification
/// handle; no device arrives on the headless desktop.
pub fn register_device_notification(m: &mut dyn Mem, cpu: &mut Cpu) {
    let hwnd = arg(m, cpu, 0);
    cpu.eax = resource(format!("device notification {hwnd:#x}"));
    ret(m, cpu, 12);
}
/// GDI32 GetStockObject(index).
pub fn get_stock_object(m: &mut dyn Mem, cpu: &mut Cpu) {
    let index = arg(m, cpu, 0);
    cpu.eax = resource(format!("stock object {index}"));
    ret(m, cpu, 4);
}
/// SHELL32 DragAcceptFiles(hwnd, accept): nothing is ever dropped.
pub fn drag_accept_files(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 8);
}

// --- cursor ---------------------------------------------------------------------

/// ShowCursor(show) -> the new display count (starts at 0, shown).
pub fn show_cursor(m: &mut dyn Mem, cpu: &mut Cpu) {
    let show = arg(m, cpu, 0);
    cpu.eax = desk(|d| {
        d.cursor_shown += if show != 0 { 1 } else { -1 };
        d.cursor_shown
    }) as u32;
    ret(m, cpu, 4);
}
/// GetCursorPos(POINT*) -> TRUE.
pub fn get_cursor_pos(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    let (x, y) = desk(|d| d.cursor);
    m.set_u32(p, x as u32);
    m.set_u32(p + 4, y as u32);
    cpu.eax = 1;
    ret(m, cpu, 4);
}
/// SetCursorPos(x, y) -> TRUE (clamped to the screen).
pub fn set_cursor_pos(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (x, y) = (arg(m, cpu, 0) as i32, arg(m, cpu, 1) as i32);
    desk(|d| d.cursor = (x.clamp(0, crate::win32::SCREEN_W as i32 - 1), y.clamp(0, crate::win32::SCREEN_H as i32 - 1)));
    cpu.eax = 1;
    ret(m, cpu, 8);
}
/// ClipCursor(RECT* or NULL) -> TRUE (the host layer owns the real cursor).
pub fn clip_cursor(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 4);
}

// --- WINMM timers -----------------------------------------------------------------

/// timeBeginPeriod / timeEndPeriod(ms): TIMERR_NOERROR; the runtime's
/// clock already has better than 1 ms resolution.
pub fn time_begin_period(m: &mut dyn Mem, cpu: &mut Cpu) {
    let ms = arg(m, cpu, 0);
    desk(|d| d.period_ms.push(ms));
    cpu.eax = 0;
    ret(m, cpu, 4);
}
pub fn time_end_period(m: &mut dyn Mem, cpu: &mut Cpu) {
    let ms = arg(m, cpu, 0);
    cpu.eax = desk(|d| match d.period_ms.iter().position(|&p| p == ms) {
        Some(k) => {
            d.period_ms.remove(k);
            0
        }
        None => 97, // TIMERR_NOCANDO
    });
    ret(m, cpu, 4);
}
/// timeGetTime(): milliseconds on the runtime clock.
pub fn time_get_time(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = crate::services::runtime_clock().as_millis() as u32;
    ret(m, cpu, 0);
}

pub const APIS: &[(&str, &str, Api)] = &[
    ("USER32.dll", "RegisterClassExA", register_class_ex_a),
    ("USER32.dll", "RegisterClassExW", register_class_ex_w),
    ("USER32.dll", "CreateWindowExA", create_window_ex_a),
    ("USER32.dll", "CreateWindowExW", create_window_ex_w),
    ("USER32.dll", "DestroyWindow", destroy_window),
    ("USER32.dll", "DefWindowProcA", def_window_proc),
    ("USER32.dll", "DefWindowProcW", def_window_proc),
    ("USER32.dll", "SendMessageA", send_message),
    ("USER32.dll", "ShowWindow", show_window),
    ("USER32.dll", "UpdateWindow", update_window),
    ("USER32.dll", "BeginPaint", begin_paint),
    ("USER32.dll", "EndPaint", end_paint),
    ("USER32.dll", "PeekMessageA", peek_message),
    ("USER32.dll", "TranslateMessage", translate_message),
    ("USER32.dll", "DispatchMessageW", dispatch_message),
    ("USER32.dll", "PostQuitMessage", post_quit_message),
    ("USER32.dll", "GetClientRect", get_client_rect),
    ("USER32.dll", "GetWindowRect", get_window_rect),
    ("USER32.dll", "ClientToScreen", client_to_screen),
    ("USER32.dll", "ScreenToClient", screen_to_client),
    ("USER32.dll", "IsIconic", is_iconic),
    ("USER32.dll", "SetWindowPos", set_window_pos),
    ("USER32.dll", "SetWindowLongA", set_window_long),
    ("USER32.dll", "SetClassLongA", set_class_long),
    ("USER32.dll", "SetPropA", set_prop),
    ("USER32.dll", "GetPropA", get_prop),
    ("USER32.dll", "RemovePropA", remove_prop),
    ("USER32.dll", "FindWindowW", find_window_w),
    ("USER32.dll", "LoadIconA", load_icon),
    ("USER32.dll", "LoadCursorA", load_cursor),
    ("USER32.dll", "LoadImageA", load_image),
    ("USER32.dll", "LoadAcceleratorsA", load_accelerators),
    ("USER32.dll", "GetSystemMenu", get_system_menu),
    ("USER32.dll", "RegisterDeviceNotificationA", register_device_notification),
    ("USER32.dll", "ShowCursor", show_cursor),
    ("USER32.dll", "GetCursorPos", get_cursor_pos),
    ("USER32.dll", "SetCursorPos", set_cursor_pos),
    ("USER32.dll", "ClipCursor", clip_cursor),
    ("GDI32.dll", "GetStockObject", get_stock_object),
    ("SHELL32.dll", "DragAcceptFiles", drag_accept_files),
    ("WINMM.dll", "timeBeginPeriod", time_begin_period),
    ("WINMM.dll", "timeEndPeriod", time_end_period),
    ("WINMM.dll", "timeGetTime", time_get_time),
];
