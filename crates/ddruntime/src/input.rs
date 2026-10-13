//! Headless USER32, DirectInput 8A and XInput 1.3 contracts.
//!
//! Call-site census and SDK references: docs/tasks/L16-input-audio-services.md.
//! sKeyboard 0x00DF43E0, sMouse 0x00E05120, sPad 0x00DFA0C0.
//! No host device is opened. A host sends events through `connect`; only the
//! guest-lock holder drains them. Pads retain XInput's four independent ports.
//! Unchanged keyboard polls use atomic snapshots; pending events still run
//! the original locked pump so hooks and message-removal ordering survive.
use crate::dispatch::{Conflict, Registry};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, call_guest, ret, with_state, zero};
use crate::user32::{self, Msg};
use ddnative_base::{mem::Mem, x86::Cpu};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

/// Polling state lives outside GuestCell. The host publishes pending input;
/// the locked pump publishes accepted keys only AFTER low-level hooks have
/// run (and can suppress them). Idle polls never enter the guest lock.
pub(crate) struct PollState {
    pending: AtomicBool,
    keys: [AtomicU16; 256],
    queues: Mutex<HashMap<u32, Arc<[AtomicU16; 256]>>>,
}

impl Default for PollState {
    fn default() -> Self {
        Self { pending: AtomicBool::new(false), keys: std::array::from_fn(|_| AtomicU16::new(0)),
            queues: Mutex::new(HashMap::new()) }
    }
}

// A modifier family occupies ONE word: three down bits and three pressed
// bits. Generic polling atomically consumes all three edges, while polling
// left/right consumes only that key, as the old locked implementation did.
fn key_bits(vk: usize) -> (usize, u16, u16) {
    match vk {
        0x10..=0x12 => (vk, 0x700, 7),
        0xa0..=0xa5 => (0x10 + (vk - 0xa0) / 2, 0x200 << ((vk - 0xa0) % 2), 2 << ((vk - 0xa0) % 2)),
        _ => (vk, 0x8000, 1),
    }
}

#[cfg(test)]
mod polling_tests {
    use super::*;

    #[test]
    fn atomic_keys_match_the_previous_locked_key_model() {
        let poll = PollState::default();
        let mut keys = [false; 256];
        let mut pressed = [false; 256];
        let choices = [0x10, 0x11, 0x12, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0x41, 1, 2, 4];
        let mut seed = 1u32;
        for _ in 0..20_000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let vk = choices[(seed >> 8) as usize % choices.len()];
            if seed & 3 == 0 {
                let queried: &[usize] = match vk {
                    0x10 => &[0x10, 0xa0, 0xa1],
                    0x11 => &[0x11, 0xa2, 0xa3],
                    0x12 => &[0x12, 0xa4, 0xa5],
                    _ => std::slice::from_ref(&vk),
                };
                let expected = if queried.iter().any(|&i| keys[i]) { 0x8000 } else { 0 }
                    | u32::from(queried.iter().any(|&i| pressed[i]));
                for &i in queried { pressed[i] = false; }
                assert_eq!(poll.async_key(vk as u32), expected, "vk={vk:#x}");
            } else {
                let down = seed & 4 != 0;
                poll.key(vk, down, keys[vk]);
                pressed[vk] |= down && !keys[vk];
                keys[vk] = down;
            }
        }
        assert_eq!(poll.async_key(256), 0);
        assert_eq!(poll.async_key(u32::MAX), 0);
    }

    #[test]
    fn concurrent_modifier_polls_consume_each_press_only_once() {
        let poll = Arc::new(PollState::default());
        poll.key(0xa3, true, false);
        let workers: Vec<_> = (0..16).map(|i| {
            let poll = poll.clone();
            std::thread::spawn(move || poll.async_key(if i % 2 == 0 { 0x11 } else { 0xa3 }))
        }).collect();
        let values: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert!(values.iter().all(|v| v & 0x8000 != 0));
        assert_eq!(values.iter().filter(|v| **v & 1 != 0).count(), 1);
        poll.key(0xa3, false, true);
        assert_eq!(poll.async_key(0x11), 0);
    }
}

impl PollState {
    fn key(&self, vk: usize, down: bool, old: bool) {
        let (index, mut down_bit, mut pressed_bit) = key_bits(vk);
        if matches!(vk, 0x10..=0x12) {
            (down_bit, pressed_bit) = (0x100, 1);
        }
        if down {
            self.keys[index].fetch_or(down_bit | if old { 0 } else { pressed_bit }, Ordering::AcqRel);
        } else {
            self.keys[index].fetch_and(!down_bit, Ordering::AcqRel);
        }
    }

    fn async_key(&self, vk: u32) -> u32 {
        if vk >= 256 { return 0; }
        let (index, down, pressed) = key_bits(vk as usize);
        let value = self.keys[index].fetch_and(!pressed, Ordering::AcqRel);
        (if value & down != 0 { 0x8000 } else { 0 }) | u32::from(value & pressed != 0)
    }
}

fn queue_snapshot() -> Arc<[AtomicU16; 256]> {
    // Only a cache miss takes this snapshot's own mutex, never the guest
    // lock. Keeping the Arc prevents address reuse across sessions.
    thread_local! {
        static CACHE: std::cell::RefCell<Option<(Arc<PollState>, u32, Arc<[AtomicU16; 256]>)>> = const { std::cell::RefCell::new(None) };
    }
    let rt = crate::runtime::current();
    let tid = crate::threads::current_tid();
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((poll, owner, keys)) = cache.as_ref() {
            if Arc::ptr_eq(poll, &rt.input_poll) && *owner == tid { return keys.clone(); }
        }
        let keys = rt.input_poll.queues.lock().unwrap_or_else(|e| e.into_inner())
            .entry(tid).or_insert_with(|| Arc::new(std::array::from_fn(|_| AtomicU16::new(0)))).clone();
        *cache = Some((rt.input_poll.clone(), tid, keys.clone()));
        keys
    })
}

const INVALID: u32 = 0x8007_0057;
const NOT_REGISTERED: u32 = 0x8004_0154;
const NOT_ACQUIRED: u32 = 0x8007_000C;
const NOT_CONNECTED: u32 = 1167;
pub const IID_INPUT: [u32; 4] = [0xbf798030, 0x4da2483a, 0x645d99aa, 0x009736ed];
pub const IID_DEVICE: [u32; 4] = [0x54d41080, 0x4833dc15, 0x8f741ba4, 0x7981a373];
pub const IUNKNOWN: [u32; 4] = [0, 0, 0xc0, 0x46000000];
pub const MOUSE_GUID: [u32; 4] = [0x6f1d2b60, 0x11cfd5a0, 0x4544c7bf, 0x00005453];
pub const KEYBOARD_GUID: [u32; 4] = [0x6f1d2b61, 0x11cfd5a0, 0x4544c7bf, 0x00005453];

pub(crate) fn guid(m: &dyn Mem, p: u32) -> [u32; 4] {
    std::array::from_fn(|i| m.u32(p + i as u32 * 4))
}
pub(crate) fn write_guid(m: &mut dyn Mem, p: u32, g: [u32; 4]) {
    for (i, v) in g.into_iter().enumerate() {
        m.set_u32(p + i as u32 * 4, v);
    }
}
fn state<R>(f: impl FnOnce(&mut Input) -> R) -> R {
    crate::runtime::with(|r| r.input.with(f))
}
fn error(code: u32) {
    with_state(|s| s.last_error = code);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}
/// A host window that does not know the guest's window handles sends
/// `hwnd: 0`: while a guest window has the focus, the event goes to it and a
/// mouse event's x/y are client coordinates of that window; otherwise the
/// device state still changes and the messages go to the thread queue.
#[derive(Clone, Debug)]
pub enum Event {
    /// None disconnects this port. Changes increment its packet counter.
    Pad { port: u32, state: Option<Pad> },
    /// Keyboard/mouse enumeration is empty until explicitly connected.
    Devices { keyboard: bool, mouse: bool },
    Key {
        hwnd: u32,
        vk: u8,
        scan: u8,
        down: bool,
        extended: bool,
        system: bool,
    },
    /// Character translation belongs to the host keyboard layout/IME.
    Character { hwnd: u32, utf16: u16, system: bool },
    /// x/y are screen coordinates; dx/dy/wheel are relative device deltas.
    Mouse {
        hwnd: u32,
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
        wheel: i32,
        buttons: [bool; 8],
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rumble {
    pub port: u32,
    pub left: u16,
    pub right: u16,
}
pub struct HostInput {
    pub events: EventSender,
    pub rumble: Receiver<Rumble>,
}

/// Host-side publication seam. Publish AFTER enqueueing: a pump racing the
/// send can cause an extra drain, but cannot clear a not-yet-enqueued event.
#[derive(Clone)]
pub struct EventSender {
    sender: Sender<Event>,
    poll: Arc<PollState>,
}

impl EventSender {
    pub fn send(&self, event: Event) -> Result<(), mpsc::SendError<Event>> {
        self.sender.send(event)?;
        self.poll.pending.store(true, Ordering::Release);
        Ok(())
    }
}

/// Attach the window/backend to this session, without sharing guest memory.
pub fn connect() -> HostInput {
    let (tx, rx) = mpsc::channel();
    let (rtx, rrx) = mpsc::channel();
    state(|s| {
        s.events = Some(rx);
        s.rumble = Some(rtx);
    });
    HostInput {
        events: EventSender { sender: tx, poll: crate::runtime::current().input_poll.clone() },
        rumble: rrx,
    }
}

#[derive(Clone)]
struct Hook {
    handle: u32,
    callback: u32,
    tid: u32,
}
struct Device {
    kind: u32,
    refs: u32,
    acquired: bool,
    format: u32,
    fields: Vec<(u32, u32)>,
    event: u32,
}
pub(crate) struct Input {
    events: Option<Receiver<Event>>,
    rumble: Option<Sender<Rumble>>,
    pending: VecDeque<Event>,
    keys: [bool; 256],
    scan: [u8; 256],
    pads: [Option<Pad>; 4],
    packets: [u32; 4],
    vibration: [(u16, u16); 4],
    enabled: bool,
    keyboard: bool,
    mouse: bool,
    delta: [i32; 3],
    buttons: [bool; 8],
    hooks: Vec<Hook>,
    next_hook: u32,
    hook_chain: VecDeque<Hook>,
    hook_args: Option<[u32; 3]>,
    di_vtable: u32,
    device_vtable: u32,
    objects: HashMap<u32, Device>,
    settings: HashMap<u32, Vec<u32>>,
}
impl Default for Input {
    fn default() -> Self {
        // Explicit headless desktop policy: accessibility aids and mouse
        // acceleration disabled; these are configurable state, not game data.
        let settings = HashMap::from([
            (3, vec![0, 0, 0]),
            (0x32, vec![24, 0, 0, 0, 0, 0]),
            (0x34, vec![8, 0]),
            (0x3a, vec![8, 0]),
            (0x5e, vec![0]),
            (0x10, vec![0]),
            (0x6e, vec![0]),
        ]);
        Self {
            events: None,
            rumble: None,
            pending: VecDeque::new(),
            keys: [false; 256],
            scan: [0; 256],
            pads: [None; 4],
            packets: [0; 4],
            vibration: [(0, 0); 4],
            enabled: true,
            keyboard: false,
            mouse: false,
            delta: [0; 3],
            buttons: [false; 8],
            hooks: vec![],
            next_hook: 0,
            hook_chain: VecDeque::new(),
            hook_args: None,
            di_vtable: 0,
            device_vtable: 0,
            objects: HashMap::new(),
            settings,
        }
    }
}

fn next_hook(m: &mut dyn Mem, c: &mut Cpu) -> u32 {
    let next = state(|s| s.hook_chain.pop_front().zip(s.hook_args));
    next.map_or(0, |(h, a)| call_guest(m, c, h.callback, &a))
}
/// Drain at a guest service boundary. Low-level keyboard hooks run on the
/// installing thread before its asynchronous key state is updated (MSDN).
pub fn pump(m: &mut dyn Mem, c: &mut Cpu) {
    let poll = crate::runtime::current().input_poll.clone();
    poll.pending.store(false, Ordering::Release);
    state(|s| {
        if let Some(rx) = &s.events {
            s.pending.extend(rx.try_iter());
        }
    });
    loop {
        let event = state(|s| {
            if s.hook_args.is_some() {
                return None;
            }
            let pos = s.pending.iter().position(|e| {
                !matches!(e, Event::Key { .. })
                    || s.hooks.iter().all(|h| h.tid == crate::threads::current_tid())
            });
            pos.and_then(|i| s.pending.remove(i))
        });
        let Some(mut e) = event else { break };
        match &mut e {
            Event::Key { hwnd, .. } | Event::Character { hwnd, .. } if *hwnd == 0 => {
                if let Some((focus, _)) = user32::input_target() {
                    *hwnd = focus;
                }
            }
            Event::Mouse { hwnd, x, y, .. } if *hwnd == 0 => {
                if let Some((focus, (ox, oy))) = user32::input_target() {
                    (*hwnd, *x, *y) = (focus, x.wrapping_add(ox), y.wrapping_add(oy));
                }
            }
            _ => {}
        }
        let changed_kind = match &e {
            Event::Key { .. } => 2,
            Event::Mouse { .. } => 1,
            _ => 0,
        };
        match e {
            Event::Devices { keyboard, mouse } => state(|s| {
                s.keyboard = keyboard;
                s.mouse = mouse;
                for d in s.objects.values_mut() {
                    if (d.kind == 1 && !mouse) || (d.kind == 2 && !keyboard) {
                        d.acquired = false;
                    }
                }
                if !keyboard {
                    s.keys = [false; 256];
                    for key in &poll.keys { key.store(0, Ordering::Release); }
                    s.scan = [0; 256];
                }
                if !mouse {
                    s.delta = [0; 3];
                    s.buttons = [false; 8];
                }
            }),
            Event::Pad { port, state: p } => state(|s| {
                if let Some(old) = s.pads.get_mut(port as usize) {
                    if *old != p {
                        s.packets[port as usize] = s.packets[port as usize].wrapping_add(1);
                        *old = p;
                    }
                }
            }),
            Event::Character { hwnd, utf16, system } => user32::post(Msg {
                hwnd,
                message: if system { 0x106 } else { 0x102 },
                wparam: utf16 as u32,
                lparam: 1,
            }),
            Event::Key {
                hwnd,
                vk,
                scan,
                down,
                extended,
                system,
            } => {
                let old = state(|s| s.keys[vk as usize]);
                let msg = if system { 0x104 } else { 0x100 } + u32::from(!down);
                let saved = c.esp;
                c.esp = c.esp.wrapping_sub(20);
                for (i, v) in [
                    vk as u32,
                    scan as u32,
                    u32::from(extended) | (u32::from(system) << 5) | (u32::from(!down) << 7),
                    crate::services::runtime_clock().as_millis() as u32,
                    0,
                ]
                .into_iter()
                .enumerate()
                {
                    m.set_u32(c.esp + 4 * i as u32, v);
                }
                state(|s| {
                    s.hook_chain = s.hooks.iter().rev().cloned().collect();
                    s.hook_args = Some([0, msg, c.esp]);
                });
                let suppressed = next_hook(m, c) != 0;
                state(|s| {
                    s.hook_chain.clear();
                    s.hook_args = None;
                });
                c.esp = saved;
                if !suppressed {
                    state(|s| {
                        s.keys[vk as usize] = down;
                        poll.key(vk as usize, down, old);
                        s.scan[(scan as usize & 0x7f) | if extended { 0x80 } else { 0 }] =
                            if down { 0x80 } else { 0 };
                    });
                    user32::post(Msg {
                        hwnd,
                        message: msg,
                        wparam: match vk {
                            0xa0 | 0xa1 => 0x10,
                            0xa2 | 0xa3 => 0x11,
                            0xa4 | 0xa5 => 0x12,
                            _ => vk as u32,
                        },
                        lparam: 1
                            | ((scan as u32) << 16)
                            | (u32::from(extended) << 24)
                            | (u32::from(system) << 29)
                            | (u32::from(old) << 30)
                            | (u32::from(!down) << 31),
                    });
                }
            }
            Event::Mouse {
                hwnd,
                x,
                y,
                dx,
                dy,
                wheel,
                buttons,
            } => {
                let old = state(|s| {
                    s.delta[0] = s.delta[0].wrapping_add(dx);
                    s.delta[1] = s.delta[1].wrapping_add(dy);
                    s.delta[2] = s.delta[2].wrapping_add(wheel);
                    let old = s.buttons;
                    s.buttons = buttons;
                    for (i, vk) in [1, 2, 4, 5, 6].into_iter().enumerate() {
                        poll.key(vk, buttons[i], old[i]);
                        s.keys[vk] = buttons[i];
                    }
                    old
                });
                user32::input_cursor(x, y);
                let modifiers = state(|s| {
                    u32::from(s.keys[0x10] || s.keys[0xa0] || s.keys[0xa1]) * 4
                        | u32::from(s.keys[0x11] || s.keys[0xa2] || s.keys[0xa3]) * 8
                });
                let flags = u32::from(buttons[0])
                    | (u32::from(buttons[1]) << 1)
                    | (u32::from(buttons[2]) << 4)
                    | (u32::from(buttons[3]) << 5)
                    | (u32::from(buttons[4]) << 6)
                    | modifiers;
                let (cx, cy) = user32::input_client_point(hwnd, x, y);
                let pos = (cx as u32 & 0xffff) | ((cy as u32 & 0xffff) << 16);
                user32::post(Msg {
                    hwnd,
                    message: 0x200,
                    wparam: flags,
                    lparam: pos,
                });
                for i in 0..3 {
                    if old[i] != buttons[i] {
                        user32::post(Msg {
                            hwnd,
                            message: 0x201 + i as u32 * 3 + u32::from(!buttons[i]),
                            wparam: flags,
                            lparam: pos,
                        });
                    }
                }
                for i in 3..5 {
                    if old[i] != buttons[i] {
                        user32::post(Msg {
                            hwnd,
                            message: 0x20b + u32::from(!buttons[i]),
                            wparam: flags | ((i as u32 - 2) << 16),
                            lparam: pos,
                        });
                    }
                }
                if wheel != 0 {
                    user32::post(Msg {
                        hwnd,
                        message: 0x20a,
                        wparam: flags | ((wheel as u32 & 0xffff) << 16),
                        lparam: (x as u32 & 0xffff) | ((y as u32 & 0xffff) << 16),
                    });
                }
            }
        }
        if changed_kind != 0 {
            let events = state(|s| {
                s.objects
                    .values()
                    .filter(|d| d.kind == changed_kind && d.acquired && d.event != 0)
                    .map(|d| d.event)
                    .collect::<Vec<_>>()
            });
            for event in events {
                let saved = c.clone();
                c.esp -= 8;
                m.set_u32(c.esp, crate::services::CALLBACK_RETURN);
                m.set_u32(c.esp + 4, event);
                crate::threads::set_event(m, c);
                *c = saved;
            }
        }
    }
    if state(|s| !s.pending.is_empty()) {
        // A keyboard hook belongs to another guest thread; preserve the
        // deferred event until that thread pumps it (including hook reentry).
        poll.pending.store(true, Ordering::Release);
    }
}
pub(crate) fn message_removed(msg: Msg) {
    if matches!(msg.message, 0x100 | 0x101 | 0x104 | 0x105) && msg.wparam < 256 {
        {
            let keys = queue_snapshot();
            let down = msg.message & 1 == 0;
            let index = match msg.wparam {
                0x10 => {
                    if (msg.lparam >> 16) & 0xff == 54 {
                        0xa1
                    } else {
                        0xa0
                    }
                }
                0x11 => {
                    if msg.lparam & (1 << 24) != 0 {
                        0xa3
                    } else {
                        0xa2
                    }
                }
                0x12 => {
                    if msg.lparam & (1 << 24) != 0 {
                        0xa5
                    } else {
                        0xa4
                    }
                }
                k => k as usize,
            };
            let mut k = keys[index].load(Ordering::Acquire);
            if down && k & 0x8000 == 0 && matches!(msg.wparam, 0x14 | 0x90 | 0x91) {
                k ^= 1;
            }
            keys[index].store((k & 1) | if down { 0x8000 } else { 0 }, Ordering::Release);
            for (generic, left, right) in [(0x10, 0xa0, 0xa1), (0x11, 0xa2, 0xa3), (0x12, 0xa4, 0xa5)] {
                if index == left || index == right {
                    keys[generic].store((keys[left].load(Ordering::Acquire) | keys[right].load(Ordering::Acquire)) & 0x8000, Ordering::Release);
                }
            }
        }
    }
}
pub fn get_async_key_state(m: &mut dyn Mem, c: &mut Cpu) {
    let poll = crate::runtime::current().input_poll.clone();
    if poll.pending.load(Ordering::Acquire) {
        crate::threads::with_lock(|| pump(m, c));
    }
    let vk = arg(m, c, 0);
    c.eax = poll.async_key(vk);
    ret(m, c, 4);
}
pub fn get_key_state(m: &mut dyn Mem, c: &mut Cpu) {
    let vk = arg(m, c, 0);
    c.eax = queue_snapshot().get(vk as usize).map_or(0, |k| k.load(Ordering::Acquire) as u32);
    ret(m, c, 4);
}
pub fn get_keyboard_layout(m: &mut dyn Mem, c: &mut Cpu) {
    c.eax = 0x04090409;
    ret(m, c, 4);
}
// US scan-set-1 codes, the same public VK/DIK codes defined by winuser.h/dinput.h.
fn scans() -> Vec<(u32, u32)> {
    let mut v = vec![
        (0x1b, 1),
        (0x13, 0xe11d),
        (0x2c, 0x54),
        (0x60, 0x52),
        (0x61, 0x4f),
        (0x62, 0x50),
        (0x63, 0x51),
        (0x64, 0x4b),
        (0x65, 0x4c),
        (0x66, 0x4d),
        (0x67, 0x47),
        (0x68, 0x48),
        (0x69, 0x49),
        (0x6a, 0x37),
        (0x6b, 0x4e),
        (0x6d, 0x4a),
        (0x6e, 0x53),
        (0x6f, 0xe035),
        (8, 14),
        (9, 15),
        (13, 28),
        (0xa2, 29),
        (0xa0, 42),
        (0xa1, 54),
        (0xa4, 56),
        (32, 57),
        (0x14, 58),
        (0x90, 69),
        (0x91, 70),
        (0xa3, 0xe01d),
        (0xa5, 0xe038),
        (0x25, 0xe04b),
        (0x26, 0xe048),
        (0x27, 0xe04d),
        (0x28, 0xe050),
        (0x24, 0xe047),
        (0x23, 0xe04f),
        (0x21, 0xe049),
        (0x22, 0xe051),
        (0x2d, 0xe052),
        (0x2e, 0xe053),
        (0x5b, 0xe05b),
        (0x5c, 0xe05c),
        (0x5d, 0xe05d),
    ];
    for (start, keys) in [
        (2, b"1234567890".as_slice()),
        (16, b"QWERTYUIOP".as_slice()),
        (30, b"ASDFGHJKL".as_slice()),
        (44, b"ZXCVBNM".as_slice()),
    ] {
        v.extend(
            keys.iter()
                .enumerate()
                .map(|(i, k)| (*k as u32, start + i as u32)),
        );
    }
    v.extend((0..10).map(|i| (0x70 + i, 59 + i)));
    v.extend([
        (0x7a, 87),
        (0x7b, 88),
        (0xbd, 12),
        (0xbb, 13),
        (0xdb, 26),
        (0xdd, 27),
        (0xba, 39),
        (0xde, 40),
        (0xc0, 41),
        (0xdc, 43),
        (0xbc, 51),
        (0xbe, 52),
        (0xbf, 53),
    ]);
    v
}
pub fn map_virtual_key(m: &mut dyn Mem, c: &mut Cpu) {
    let (mut code, kind) = (arg(m, c, 0), arg(m, c, 1));
    let mut table = scans();
    table.extend((0x7c..=0x86).map(|vk| (vk, vk - 0x7c + 0x64)));
    table.push((0x87, 0x76));
    c.eax = match kind {
        0 | 4 => {
            code = match code {
                0x10 => 0xa0,
                0x11 => 0xa2,
                0x12 => 0xa4,
                _ => code,
            };
            let sc = table.iter().find(|p| p.0 == code).map_or(0, |p| p.1);
            if kind == 0 {
                sc & 255
            } else {
                sc
            }
        }
        1 | 3 => {
            // VSC_TO_VK names navigation keys even for unprefixed numpad scans.
            // Confirmed with MapVirtualKeyExW(en-US), in addition to public DIK codes.
            let vk = match code {
                0x47 => 0x24,
                0x48 => 0x26,
                0x49 => 0x21,
                0x4b => 0x25,
                0x4c => 0x0c,
                0x4d => 0x27,
                0x4f => 0x23,
                0x50 => 0x28,
                0x51 => 0x22,
                0x52 => 0x2d,
                0x53 => 0x2e,
                0xe01c => 13,
                _ => table.iter().find(|p| p.1 == code).map_or(0, |p| p.0),
            };
            if kind == 1 {
                match vk {
                    0xa0 | 0xa1 => 0x10,
                    0xa2 | 0xa3 => 0x11,
                    0xa4 | 0xa5 => 0x12,
                    _ => vk,
                }
            } else {
                vk
            }
        }
        2 => match code {
            0x30..=0x39 | 0x41..=0x5a | 8 | 9 | 13 | 27 | 32 => code,
            0xba => 59,
            0xbb => 61,
            0xbc => 44,
            0xbd => 45,
            0xbe => 46,
            0xbf => 47,
            0xc0 => 96,
            0xdb => 91,
            0xdc => 92,
            0xdd => 93,
            0xde => 39,
            _ => 0,
        },
        _ => 0,
    };
    ret(m, c, 8);
}
pub fn set_hook(m: &mut dyn Mem, c: &mut Cpu) {
    let (id, callback, thread) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 3));
    c.eax = if id != 13 || callback == 0 || thread != 0 {
        error(87);
        0
    } else {
        state(|s| {
            s.next_hook += 1;
            let handle = 0x7fb00000 + s.next_hook;
            s.hooks.push(Hook {
                handle,
                callback,
                tid: crate::threads::current_tid(),
            });
            handle
        })
    };
    ret(m, c, 16);
}
pub fn unhook(m: &mut dyn Mem, c: &mut Cpu) {
    let h = arg(m, c, 0);
    c.eax = state(|s| {
        let n = s.hooks.len();
        s.hooks.retain(|v| v.handle != h);
        u32::from(n != s.hooks.len())
    });
    if c.eax == 0 {
        error(1404);
    }
    ret(m, c, 4);
}
pub fn call_next_hook(m: &mut dyn Mem, c: &mut Cpu) {
    c.eax = next_hook(m, c);
    ret(m, c, 16);
}
pub fn raw_list(m: &mut dyn Mem, c: &mut Cpu) {
    let (out, count, size) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    if count == 0 || size != 8 {
        error(87);
        c.eax = u32::MAX;
    } else {
        let _ = out;
        m.set_u32(count, 0);
        c.eax = 0;
    }
    ret(m, c, 12);
}
pub fn raw_info(m: &mut dyn Mem, c: &mut Cpu) {
    error(6);
    c.eax = u32::MAX;
    ret(m, c, 16);
}
pub fn imm_virtual_key(m: &mut dyn Mem, c: &mut Cpu) {
    c.eax = 0xe5;
    ret(m, c, 4);
}
pub fn system_parameters(m: &mut dyn Mem, c: &mut Cpu) {
    let (action, n, p) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    let (key, set) = match action {
        4 => (3, true),
        0x33 => (0x32, true),
        0x35 => (0x34, true),
        0x3b => (0x3a, true),
        0x5d => (0x5e, true),
        0x11 => (0x10, true),
        0x6f => (0x6e, true),
        _ => (action, false),
    };
    c.eax = {
        let values = state(|s| s.settings.get(&key).cloned());
        if let Some(mut values) = values {
            let structure = matches!(key, 0x32 | 0x34 | 0x3a);
            if (structure && (p == 0 || n != values.len() as u32 * 4 || m.u32(p) != n))
                || (!set && p == 0)
                || (set && values.len() > 1 && p == 0)
            {
                error(87);
                0
            } else {
                if set {
                    if values.len() == 1 {
                        values[0] = n;
                    } else {
                        for (i, v) in values.iter_mut().enumerate() {
                            *v = m.u32(p + 4 * i as u32);
                        }
                    }
                    state(|s| {
                        s.settings.insert(key, values);
                    });
                } else {
                    for (i, v) in values.into_iter().enumerate() {
                        m.set_u32(p + 4 * i as u32, v);
                    }
                }
                1
            }
        } else {
            error(87);
            0
        }
    };
    ret(m, c, 16);
}
fn put_pad(m: &mut dyn Mem, p: u32, v: Pad) {
    m.set_u16(p, v.buttons);
    m.set_u8(p + 2, v.left_trigger);
    m.set_u8(p + 3, v.right_trigger);
    for (i, a) in [v.left_x, v.left_y, v.right_x, v.right_y].into_iter().enumerate() {
        m.set_u16(p + 4 + i as u32 * 2, a as u16);
    }
}
pub fn xinput_state(m: &mut dyn Mem, c: &mut Cpu) {
    pump(m, c);
    let (port, p) = (arg(m, c, 0), arg(m, c, 1));
    let data = state(|s| {
        s.pads.get(port as usize).copied().flatten().map(|v| {
            (
                if s.enabled { v } else { Pad::default() },
                s.packets[port as usize],
            )
        })
    });
    c.eax = if port >= 4 || p == 0 {
        87
    } else if let Some((v, packet)) = data {
        m.set_u32(p, packet);
        put_pad(m, p + 4, v);
        0
    } else {
        NOT_CONNECTED
    };
    ret(m, c, 8);
}
pub fn xinput_set(m: &mut dyn Mem, c: &mut Cpu) {
    pump(m, c);
    let (port, p) = (arg(m, c, 0), arg(m, c, 1));
    c.eax = if port >= 4 || p == 0 {
        87
    } else {
        state(|s| {
            if s.pads[port as usize].is_none() {
                return NOT_CONNECTED;
            }
            let pair = (m.u16(p), m.u16(p + 2));
            s.vibration[port as usize] = pair;
            if s.enabled {
                if let Some(tx) = &s.rumble {
                    let _ = tx.send(Rumble {
                        port,
                        left: pair.0,
                        right: pair.1,
                    });
                }
            }
            0
        })
    };
    ret(m, c, 8);
}
pub fn xinput_caps(m: &mut dyn Mem, c: &mut Cpu) {
    pump(m, c);
    let (port, flags, p) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    c.eax = if port >= 4 || flags & !1 != 0 || p == 0 {
        87
    } else if state(|s| s.pads[port as usize].is_some()) {
        zero(m, p, 20);
        m.set_u8(p, 1);
        m.set_u8(p + 1, 1);
        put_pad(
            m,
            p + 4,
            Pad {
                buttons: 0xf3ff,
                left_trigger: 255,
                right_trigger: 255,
                left_x: -1,
                left_y: -1,
                right_x: -1,
                right_y: -1,
            },
        );
        m.set_u16(p + 16, 65535);
        m.set_u16(p + 18, 65535);
        0
    } else {
        NOT_CONNECTED
    };
    ret(m, c, 12);
}
pub fn xinput_enable(m: &mut dyn Mem, c: &mut Cpu) {
    let enabled = arg(m, c, 0) != 0;
    state(|s| {
        s.enabled = enabled;
        if let Some(tx) = &s.rumble {
            for i in 0..4 {
                if s.pads[i].is_some() {
                    let (l, r) = if enabled { s.vibration[i] } else { (0, 0) };
                    let _ = tx.send(Rumble {
                        port: i as u32,
                        left: l,
                        right: r,
                    });
                }
            }
        }
    });
    ret(m, c, 4);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("USER32.dll", "GetAsyncKeyState", get_async_key_state),
    ("USER32.dll", "GetKeyState", get_key_state),
    ("USER32.dll", "GetKeyboardLayout", get_keyboard_layout),
    ("USER32.dll", "MapVirtualKeyA", map_virtual_key),
    ("USER32.dll", "SetWindowsHookExA", set_hook),
    ("USER32.dll", "UnhookWindowsHookEx", unhook),
    ("USER32.dll", "CallNextHookEx", call_next_hook),
    ("USER32.dll", "GetRawInputDeviceList", raw_list),
    ("USER32.dll", "GetRawInputDeviceInfoA", raw_info),
    ("USER32.dll", "SystemParametersInfoA", system_parameters),
    ("IMM32.dll", "ImmGetVirtualKey", imm_virtual_key),
    ("XINPUT1_3.dll", "#2", xinput_state),
    ("XINPUT1_3.dll", "#3", xinput_set),
    ("XINPUT1_3.dll", "#4", xinput_caps),
    ("XINPUT1_3.dll", "#5", xinput_enable),
    ("DINPUT8.dll", "DirectInput8Create", direct_input_create),
];

// DirectInput COM implementations follow below. Unimplemented slots stay named.
const INPUT_NAMES: &[&str] = &[
    "QueryInterface",
    "AddRef",
    "Release",
    "CreateDevice",
    "EnumDevices",
    "GetDeviceStatus",
    "RunControlPanel",
    "Initialize",
    "FindDevice",
    "EnumDevicesBySemantics",
    "ConfigureDevices",
];
const DEVICE_NAMES: &[&str] = &[
    "QueryInterface",
    "AddRef",
    "Release",
    "GetCapabilities",
    "EnumObjects",
    "GetProperty",
    "SetProperty",
    "Acquire",
    "Unacquire",
    "GetDeviceState",
    "GetDeviceData",
    "SetDataFormat",
    "SetEventNotification",
    "SetCooperativeLevel",
    "GetObjectInfo",
    "GetDeviceInfo",
    "RunControlPanel",
    "Initialize",
    "CreateEffect",
    "EnumEffects",
    "GetEffectInfo",
    "GetForceFeedbackState",
    "SendForceFeedbackCommand",
    "EnumCreatedEffectObjects",
    "Escape",
    "Poll",
    "SendDeviceData",
    "EnumEffectsInFile",
    "WriteEffectToFile",
    "BuildActionMap",
    "SetActionMap",
    "GetImageInfo",
];
pub fn install(m: &mut dyn Mem, r: &mut Registry, t: &mut Tokens) -> Result<(), Conflict> {
    let di = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "IDirectInput8A",
            slots: 11,
            names: INPUT_NAMES,
            methods: &[
                (0, di_method::<0>),
                (1, di_method::<1>),
                (2, di_method::<2>),
                (3, di_method::<3>),
                (4, di_method::<4>),
                (5, di_method::<5>),
                (6, di_method::<6>),
                (7, di_method::<7>),
                (8, di_method::<8>),
            ],
        },
    )?;
    let device = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "IDirectInputDevice8A",
            slots: 32,
            names: DEVICE_NAMES,
            methods: &[
                (0, device_method::<0>),
                (1, device_method::<1>),
                (2, device_method::<2>),
                (3, device_method::<3>),
                (4, device_method::<4>),
                (5, device_method::<5>),
                (6, device_method::<6>),
                (7, device_method::<7>),
                (8, device_method::<8>),
                (9, device_method::<9>),
                (10, device_method::<10>),
                (11, device_method::<11>),
                (12, device_method::<12>),
                (13, device_method::<13>),
                (14, device_method::<14>),
                (15, device_method::<15>),
                (18, device_method::<18>),
                (19, device_method::<19>),
                (21, device_method::<21>),
                (22, device_method::<22>),
                (25, device_method::<25>),
            ],
        },
    )?;
    state(|s| {
        s.di_vtable = di;
        s.device_vtable = device;
    });
    Ok(())
}
fn new_object(m: &mut dyn Mem, kind: u32) -> u32 {
    let v = state(|s| if kind == 0 { s.di_vtable } else { s.device_vtable });
    assert_ne!(v, 0, "input interfaces not installed");
    let o = interfaces::object(m, v, 4);
    state(|s| {
        s.objects.insert(
            o,
            Device {
                kind,
                refs: 1,
                acquired: false,
                format: 0,
                fields: vec![],
                event: 0,
            },
        );
    });
    o
}
pub fn direct_input_create(m: &mut dyn Mem, c: &mut Cpu) {
    pump(m, c);
    let (version, iid, out, outer) = (arg(m, c, 1), arg(m, c, 2), arg(m, c, 3), arg(m, c, 4));
    c.eax = if out == 0 || iid == 0 {
        INVALID
    } else {
        m.set_u32(out, 0);
        if outer != 0 {
            0x80040110
        } else if version == 0 {
            0x80070015 // DIERR_NOTINITIALIZED
        } else if version < 0x800 {
            0x80070481 // DIERR_BETADIRECTINPUTVERSION
        } else if version > 0x800 {
            0x8007047e // DIERR_OLDDIRECTINPUTVERSION
        } else if guid(m, iid) != IID_INPUT {
            0x80004002
        } else {
            let o = new_object(m, 0);
            m.set_u32(out, o);
            0
        }
    };
    ret(m, c, 20);
}
fn identity(m: &mut dyn Mem, c: &mut Cpu, slot: u32) -> bool {
    if slot > 2 {
        return false;
    }
    let this = arg(m, c, 0);
    if slot == 0 {
        let (iid, out) = (arg(m, c, 1), arg(m, c, 2));
        c.eax = if out == 0 || iid == 0 {
            INVALID
        } else {
            m.set_u32(out, 0);
            let g = guid(m, iid);
            let found = state(|s| {
                s.objects
                    .get(&this)
                    .map(|d| if d.kind == 0 { IID_INPUT } else { IID_DEVICE })
            });
            if found.is_some() && (g == IUNKNOWN || Some(g) == found) {
                state(|s| s.objects.get_mut(&this).unwrap().refs += 1);
                m.set_u32(out, this);
                0
            } else {
                0x80004002
            }
        };
        ret(m, c, 12);
    } else {
        c.eax = state(|s| {
            let d = s.objects.get_mut(&this).expect("released DirectInput object");
            if slot == 1 {
                d.refs += 1;
            } else {
                d.refs -= 1;
            }
            let n = d.refs;
            if n == 0 {
                s.objects.remove(&this);
            }
            n
        });
        ret(m, c, 4);
    }
    true
}
fn available(g: [u32; 4]) -> Option<u32> {
    state(|s| {
        if g == MOUSE_GUID && s.mouse {
            Some(1)
        } else if g == KEYBOARD_GUID && s.keyboard {
            Some(2)
        } else {
            None
        }
    })
}
fn instance(m: &mut dyn Mem, p: u32, kind: u32) {
    zero(m, p, 580);
    m.set_u32(p, 580);
    let g = if kind == 1 { MOUSE_GUID } else { KEYBOARD_GUID };
    write_guid(m, p + 4, g);
    write_guid(m, p + 20, g);
    m.set_u32(p + 36, if kind == 1 { 0x112 } else { 0x413 });
    let name = if kind == 1 {
        b"System Mouse".as_slice()
    } else {
        b"System Keyboard".as_slice()
    };
    for base in [40, 300] {
        for (i, b) in name.iter().enumerate() {
            m.set_u8(p + base + i as u32, *b);
        }
    }
}
fn di_method<const SLOT: u32>(m: &mut dyn Mem, c: &mut Cpu) {
    if identity(m, c, SLOT) {
        return;
    }
    pump(m, c);
    let a = |k| arg(m, c, k);
    let cleanup = match SLOT {
        3 => 16,
        4 => 20,
        5 => 8,
        6 => 12,
        7 => 12,
        8 => 16,
        _ => unreachable!(),
    };
    c.eax = match SLOT {
        3 => {
            let (g, out, outer) = (a(1), a(2), a(3));
            if g == 0 || out == 0 {
                INVALID
            } else {
                m.set_u32(out, 0);
                if outer != 0 {
                    0x80040110
                } else if let Some(kind) = available(guid(m, g)) {
                    let d = new_object(m, kind);
                    m.set_u32(out, d);
                    0
                } else {
                    NOT_REGISTERED
                }
            }
        }
        4 => {
            let (kind, callback, context, flags) = (a(1), a(2), a(3), a(4));
            if callback == 0 || flags & !0x131 != 0 {
                INVALID
            } else {
                let devices = state(|s| [(s.mouse, 1), (s.keyboard, 2)]);
                let saved = c.esp;
                c.esp -= 580;
                for (yes, k) in devices {
                    if yes
                        && matches!(kind, 0 | 1 | 2 | 3)
                        && !(kind == 2 && k != 2)
                        && !(kind == 3 && k != 1)
                    {
                        instance(m, c.esp, k);
                        if call_guest(m, c, callback, &[c.esp, context]) == 0 {
                            break;
                        }
                    } else if yes && kind == if k == 1 { 0x12 } else { 0x13 } {
                        instance(m, c.esp, k);
                        if call_guest(m, c, callback, &[c.esp, context]) == 0 {
                            break;
                        }
                    }
                }
                c.esp = saved;
                0
            }
        }
        5 => {
            if a(1) != 0 && available(guid(m, a(1))).is_some() {
                0
            } else {
                NOT_REGISTERED
            }
        }
        7 => {
            if a(2) == 0x800 {
                0
            } else {
                INVALID
            }
        }
        6 => 0x80004001,
        8 => NOT_REGISTERED,
        _ => unreachable!(),
    };
    ret(m, c, cleanup);
}
// Public DirectInput object identities and DIDEVICEOBJECTINSTANCEA layout.
fn device_objects(kind: u32) -> Vec<(u32, u32)> {
    if kind == 1 {
        (0..3)
            .map(|i| (i * 4, (i << 8) | 1))
            .chain((0..8).map(|i| (12 + i, (i << 8) | 4)))
            .collect()
    } else {
        (0..256).map(|i| (i, (i << 8) | 4)).collect()
    }
}
fn object_info(m: &mut dyn Mem, p: u32, kind: u32, ofs: u32, t: u32) {
    zero(m, p, 316);
    m.set_u32(p, 316);
    let axis = t & 3 != 0;
    let index = (t >> 8) & 0xffff;
    let g = if kind == 2 {
        [0x55728220, 0x11cfd33c, 0x4544c7bf, 0x5453]
    } else {
        [
            if axis { 0xa36d02e0 + index } else { 0xa36d02f0 },
            0x11cfc9f3,
            0x4544c7bf,
            0x5453,
        ]
    };
    write_guid(m, p + 4, g);
    m.set_u32(p + 20, ofs);
    m.set_u32(p + 24, t);
    let name = if axis {
        format!("Axis {index}")
    } else {
        format!("Button {index}")
    };
    for (i, b) in name.bytes().enumerate() {
        m.set_u8(p + 32 + i as u32, b);
    }
}
fn device_method<const SLOT: u32>(m: &mut dyn Mem, c: &mut Cpu) {
    if identity(m, c, SLOT) {
        return;
    }
    pump(m, c);
    let this = arg(m, c, 0);
    let kind = state(|s| s.objects.get(&this).expect("released input device").kind);
    let cleanup = match SLOT {
        3 | 11 | 12 | 15 | 21 | 22 => 8,
        10 | 18 => 20,
        4 | 14 | 19 => 16,
        5 | 6 | 9 | 13 => 12,
        7 | 8 | 25 => 4,
        _ => unreachable!(),
    };
    c.eax = match SLOT {
        3 => {
            let p = arg(m, c, 1);
            if p == 0 || m.u32(p) != 44 {
                INVALID
            } else {
                zero(m, p, 44);
                m.set_u32(p, 44);
                m.set_u32(p + 4, 1);
                m.set_u32(p + 8, if kind == 1 { 0x112 } else { 0x413 });
                m.set_u32(p + 12, if kind == 1 { 3 } else { 0 });
                m.set_u32(p + 16, if kind == 1 { 8 } else { 256 });
                0
            }
        }
        4 => {
            let (cb, context, flags) = (arg(m, c, 1), arg(m, c, 2), arg(m, c, 3));
            if cb == 0 {
                INVALID
            } else {
                let saved = c.esp;
                c.esp -= 316;
                for (ofs, t) in device_objects(kind) {
                    if flags & 0x01000000 != 0 || (flags & 0xff != 0 && flags & t & 0xff == 0) {
                        continue;
                    }
                    object_info(m, c.esp, kind, ofs, t);
                    if call_guest(m, c, cb, &[c.esp, context]) == 0 {
                        break;
                    }
                }
                c.esp = saved;
                0
            }
        }
        5 | 6 => 0x80004001,
        7 => state(|s| {
            if (kind == 1 && !s.mouse) || (kind == 2 && !s.keyboard) {
                return 0x8007001e;
            }
            let d = s.objects.get_mut(&this).unwrap();
            if d.format == 0 {
                return INVALID;
            }
            let old = d.acquired;
            d.acquired = true;
            u32::from(old)
        }),
        8 => state(|s| {
            let d = s.objects.get_mut(&this).unwrap();
            let old = d.acquired;
            d.acquired = false;
            u32::from(!old)
        }),
        9 => {
            let (size, p) = (arg(m, c, 1), arg(m, c, 2));
            let data = state(|s| {
                let d = &s.objects[&this];
                (d.acquired, d.format, d.fields.clone(), s.delta, s.buttons, s.scan)
            });
            if !data.0 {
                NOT_ACQUIRED
            } else if p == 0 || size != data.1 {
                INVALID
            } else {
                zero(m, p, size);
                for (ofs, t) in data.2 {
                    let index = (t >> 8) & 0xffff;
                    if kind == 1 && t & 3 != 0 && index < 3 {
                        m.set_u32(p + ofs, data.3[index as usize] as u32);
                    } else if t & 0xc != 0 {
                        let v = if kind == 1 {
                            u8::from(data.4.get(index as usize).copied().unwrap_or(false)) * 128
                        } else {
                            data.5.get(index as usize).copied().unwrap_or(0)
                        };
                        m.set_u8(p + ofs, v);
                    }
                }
                if kind == 1 {
                    state(|s| s.delta = [0; 3]);
                }
                0
            }
        }
        10 => {
            let (size, count) = (arg(m, c, 1), arg(m, c, 3));
            if size != 20 || count == 0 {
                INVALID
            } else {
                m.set_u32(count, 0);
                0x80040207
            }
        } // DIERR_NOTBUFFERED: no DIPROP_BUFFERSIZE configured
        11 => {
            let p = arg(m, c, 1);
            if p == 0 || m.u32(p) != 24 || m.u32(p + 4) != 16 {
                INVALID
            } else {
                let (size, count, objects) = (m.u32(p + 12), m.u32(p + 16), m.u32(p + 20));
                if count > 256 || size > 4096 || objects == 0 {
                    INVALID
                } else {
                    let mut next_axis = 0;
                    let mut next_button = 0;
                    let fields: Vec<_> = (0..count)
                        .map(|i| {
                            let o = objects + i * 16;
                            let mut t = m.u32(o + 8);
                            if (t >> 8) & 0xffff == 0xffff {
                                let index = if t & 3 != 0 {
                                    // Axis GUIDs are public DirectInput GUID_XAxis/YAxis/ZAxis.
                                    let g = m.u32(o);
                                    let axis = if g != 0 {
                                        match m.u32(g) {
                                            0xa36d02e0 => Some(0),
                                            0xa36d02e1 => Some(1),
                                            0xa36d02e2 => Some(2),
                                            _ => None,
                                        }
                                    } else {
                                        None
                                    };
                                    let v = axis.unwrap_or(next_axis);
                                    next_axis = v + 1;
                                    v
                                } else {
                                    let v = next_button;
                                    next_button += 1;
                                    v
                                };
                                t = (t & 0xff0000ff) | (index << 8);
                            }
                            (m.u32(o + 4), t)
                        })
                        .collect();
                    if fields.iter().any(|(o, t)| {
                        o.checked_add(if t & 3 != 0 { 4 } else { 1 })
                            .is_none_or(|end| end > size)
                    }) {
                        INVALID
                    } else {
                        state(|s| {
                            let d = s.objects.get_mut(&this).unwrap();
                            if d.acquired {
                                return 0x800700aa;
                            }
                            d.format = size;
                            d.fields = fields;
                            0
                        })
                    }
                }
            }
        }
        12 => {
            let event = arg(m, c, 1);
            state(|s| s.objects.get_mut(&this).unwrap().event = event);
            0
        }
        13 => {
            let flags = arg(m, c, 2);
            if flags & 3 == 0 || flags & 3 == 3 || flags & 12 == 0 || flags & 12 == 12 || flags & !31 != 0 {
                INVALID
            } else {
                0
            }
        }
        14 => {
            let (p, obj, how) = (arg(m, c, 1), arg(m, c, 2), arg(m, c, 3));
            if p == 0 || m.u32(p) != 316 || !matches!(how, 1 | 2) {
                INVALID
            } else if let Some((ofs, t)) =
                device_objects(kind)
                    .into_iter()
                    .find(|&(ofs, t)| if how == 1 { ofs == obj } else { t == obj })
            {
                object_info(m, p, kind, ofs, t);
                0
            } else {
                0x80070002
            }
        }
        15 => {
            let p = arg(m, c, 1);
            if p == 0 || m.u32(p) != 580 {
                INVALID
            } else {
                instance(m, p, kind);
                0
            }
        }
        18 => {
            let out = arg(m, c, 3);
            if out != 0 {
                m.set_u32(out, 0);
            }
            0x80004001
        }
        19 => 0,
        21 | 22 => 0x80004001,
        25 => {
            if state(|s| s.objects[&this].acquired) {
                0
            } else {
                NOT_ACQUIRED
            }
        }
        _ => unreachable!(),
    };
    ret(m, c, cleanup);
}
