//! The window's input for the game (brief L16's host seam,
//! `ddruntime::input`): keys, characters and the mouse from the window's
//! events, and the host's XInput pads polled on their own thread with the
//! game's rumble sent back. Events carry `hwnd: 0` (the guest's focused
//! window); mouse positions are the window's pixels scaled to the game's
//! back buffer, which is the guest window's client area.
use ddruntime::input::{Event, HostInput, Pad};
use std::sync::mpsc::Sender;
use winit::event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::platform::scancode::PhysicalKeyExtScancode;

pub struct Input {
    events: Sender<Event>,
    /// Held keys as (vk, scan, extended), released when the window loses focus.
    held: Vec<(u8, u8, bool)>,
    alt: bool,
    /// The cursor in guest client coordinates.
    cursor: (i32, i32),
    buttons: [bool; 8],
}

impl Input {
    /// Take over the session's host input: keyboard and mouse connected, the
    /// pads polled from now on.
    pub fn new(host: HostInput) -> Input {
        let _ = host.events.send(Event::Devices { keyboard: true, mouse: true });
        let (events, rumble) = (host.events.clone(), host.rumble);
        std::thread::Builder::new()
            .name("host pads".into())
            .spawn(move || pads(events, rumble))
            .expect("the pad thread");
        Input { events: host.events, held: Vec::new(), alt: false, cursor: (0, 0), buttons: [false; 8] }
    }

    /// `window` is the window's size and `client` the game's back buffer
    /// size (both in pixels).
    pub fn window_event(&mut self, event: &WindowEvent, window: (u32, u32), client: (u32, u32)) {
        match event {
            WindowEvent::ModifiersChanged(m) => self.alt = m.state().alt_key(),
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(code) = event.physical_key.to_scancode() else { return };
                let down = event.state == ElementState::Pressed;
                let (scan, extended) = ((code & 0xff) as u8, code & 0xff00 == 0xe000);
                let vk = virtual_key(code);
                if vk == 0 {
                    return;
                }
                // WM_SYSKEYDOWN while Alt is held, and for F10 (Windows' rule).
                let system = self.alt || vk == 0x79 || matches!(vk, 0x12 | 0xa4 | 0xa5);
                self.send(Event::Key { hwnd: 0, vk, scan, down, extended, system });
                self.held.retain(|&(v, _, _)| v != vk);
                if down {
                    self.held.push((vk, scan, extended));
                    if let Some(text) = &event.text {
                        for unit in text.encode_utf16() {
                            self.send(Event::Character { hwnd: 0, utf16: unit, system: self.alt });
                        }
                    }
                }
            }
            WindowEvent::Focused(false) => {
                for (vk, scan, extended) in std::mem::take(&mut self.held) {
                    self.send(Event::Key { hwnd: 0, vk, scan, down: false, extended, system: false });
                }
                if self.buttons.iter().any(|&b| b) {
                    self.buttons = [false; 8];
                    self.mouse(0, 0, 0);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = |p: f64, w: u32, c: u32| (p * c as f64 / w.max(1) as f64).floor() as i32;
                self.cursor = (scale(position.x, window.0, client.0), scale(position.y, window.1, client.1));
                self.mouse(0, 0, 0);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let i = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(_) => return,
                };
                self.buttons[i] = *state == ElementState::Pressed;
                self.mouse(0, 0, 0);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // WHEEL_DELTA (120) per notch
                let wheel = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 120.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y as i32,
                };
                if wheel != 0 {
                    self.mouse(0, 0, wheel);
                }
            }
            _ => {}
        }
    }

    /// Raw mouse motion (DirectInput's relative axes).
    pub fn device_event(&mut self, event: &DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            self.mouse(*dx as i32, *dy as i32, 0);
        }
    }

    fn mouse(&mut self, dx: i32, dy: i32, wheel: i32) {
        let (x, y) = self.cursor;
        self.send(Event::Mouse { hwnd: 0, x, y, dx, dy, wheel, buttons: self.buttons });
    }

    fn send(&self, e: Event) {
        let _ = self.events.send(e);
    }
}

/// The virtual key of a scan code (0xE0xx for extended keys) in the host's
/// keyboard layout, with left/right modifiers distinguished.
fn virtual_key(code: u32) -> u8 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VSC_TO_VK_EX};
    // Keys whose scan code maps to a different key than Windows reports
    // for them (the numeric keypad's extended twins are separate keys).
    match code {
        0xe01c => return 0x0d, // keypad Enter: VK_RETURN, extended
        0xe035 => return 0x6f, // keypad divide
        0x45 => return 0x90,   // Num Lock
        0xe045 => return 0x90,
        0xe11d | 0x0e11d => return 0x13, // Pause
        _ => {}
    }
    unsafe { MapVirtualKeyW(code, MAPVK_VSC_TO_VK_EX) as u8 }
}

/// Poll the host's four XInput ports, forward changes, apply rumble.
fn pads(events: Sender<Event>, rumble: std::sync::mpsc::Receiver<ddruntime::input::Rumble>) {
    use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XInputSetState, XINPUT_STATE, XINPUT_VIBRATION};
    let mut last: [Option<Pad>; 4] = [None; 4];
    let mut announced = [false; 4];
    loop {
        for r in rumble.try_iter() {
            let v = XINPUT_VIBRATION { wLeftMotorSpeed: r.left, wRightMotorSpeed: r.right };
            unsafe { XInputSetState(r.port, &v) };
        }
        for port in 0..4u32 {
            let mut s: XINPUT_STATE = unsafe { std::mem::zeroed() };
            let pad = (unsafe { XInputGetState(port, &mut s) } == 0).then(|| {
                let g = s.Gamepad;
                Pad {
                    buttons: g.wButtons,
                    left_trigger: g.bLeftTrigger,
                    right_trigger: g.bRightTrigger,
                    left_x: g.sThumbLX,
                    left_y: g.sThumbLY,
                    right_x: g.sThumbRX,
                    right_y: g.sThumbRY,
                }
            });
            let k = port as usize;
            if pad != last[k] || !announced[k] {
                last[k] = pad;
                announced[k] = true;
                if events.send(Event::Pad { port, state: pad }).is_err() {
                    return;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}
