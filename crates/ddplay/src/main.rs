//! ddplay: runs the rebuilt game in a window (docs/RUNTIME.md, "a
//! window/presentation host").
//!
//! The game runs on its own guest thread exactly as the runtime runs it
//! headless (`Session::start`: the CRT start-up, then WinMain), with the
//! wgpu backend (`ddgpu`) drawing its Direct3D 9 commands. The backend and
//! the window surface share one wgpu device: every Present draws the back
//! buffer straight into the surface (src/present.rs), with no read-back.
//! The window title shows where the game is (running, or the report of
//! where it stopped) and the frame rate and frame times.
//!
//!     cargo run --release -p ddplay -- [path\to\DDDA.exe] [--no-vsync]
//!
//! The window follows the game's own display choice (docs/OPTIONS.md,
//! brief L34): windowed at its back buffer's size, or full screen in the
//! monitor's video mode of that size and refresh rate, with vsync as its
//! VSYNC option asks. `--no-vsync` presents immediately (or mailbox,
//! whatever the surface has) whatever the game asks. F11 or Alt+Enter
//! toggles borderless fullscreen. `--test-pattern` runs a moving pattern
//! through the backend instead of the game (a presentation diagnostic).
//! Keyboard, mouse and the host's XInput pads reach the game through the
//! runtime's input seam (`input.rs`). Audio plays on the default output
//! device (`sink.rs`, brief L19).
mod input;
mod present;
mod sink;

use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

enum GameEvent {
    /// The session's host input, connected on the guest thread.
    Input(ddruntime::input::HostInput),
    Status(String),
    /// The game's presentation changed (read on the guest thread at Present).
    Mode(ddruntime::d3d9dev::Presentation),
}

/// D3DPRESENT_INTERVAL_IMMEDIATE: the game's VSYNC=OFF (0x00DB1480, the
/// table at 0x01429878); DEFAULT (0) and ONE (1) wait for the refresh.
const INTERVAL_IMMEDIATE: u32 = 0x8000_0000;

/// How often the title's frame statistics update.
const TITLE_INTERVAL: Duration = Duration::from_millis(500);
/// The game not presenting for this long counts as idle: the window
/// repaints the last frame itself (resizes, exposes).
const IDLE: Duration = Duration::from_millis(250);

/// The device the game's backend draws with, shared with the surface.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::AdapterInfo,
    window: Arc<present::Window>,
}

struct App {
    exe: String,
    vsync: bool,
    test_pattern: bool,
    proxy: EventLoopProxy<GameEvent>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    status: String,
    rate: String,
    next_title: Instant,
    modifiers: ModifiersState,
    started: bool,
    input: Option<input::Input>,
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

impl App {
    /// One device for the window surface and the game's backend: an adapter
    /// that can present to the window and has the backend's features.
    fn create_gpu(window: Arc<Window>, vsync: bool) -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).map_err(|e| format!("a surface for the window: {e}"))?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })).map_err(|e| format!("an adapter: {e}"))?;
        let needed = ddgpu::WgpuBackend::required_features();
        if !adapter.features().contains(needed) {
            return Err(format!("the adapter lacks {:?}", needed - adapter.features()));
        }
        // the optional features WgpuBackend::headless() asks for where the
        // adapter has them (draws needing a missing one report an error);
        // the same set as headless(), until ddgpu exports it (L29:
        // WgpuBackend::optional_features())
        let optional = wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER | wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::POLYGON_MODE_LINE | wgpu::Features::POLYGON_MODE_POINT;
        let features = needed | (adapter.features() & optional);
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, ..Default::default() }))
            .map_err(|e| format!("a device: {e}"))?;
        let size = window.inner_size();
        let shown = present::Window::new(surface, &adapter, device.clone(), queue.clone(), size.width, size.height, vsync)?;
        Ok(Gpu { device, queue, adapter: adapter.get_info(), window: shown })
    }

    /// Start the game on its guest thread (once).
    fn start_game(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (exe, proxy, vsync) = (self.exe.clone(), self.proxy.clone(), self.vsync);
        let gpu = self.gpu.as_ref().map(|g| (g.device.clone(), g.queue.clone(), g.adapter.clone(), g.window.clone()));
        if self.test_pattern {
            if let Some((device, queue, adapter, window)) = gpu {
                self.status = "test pattern (not the game)".into();
                std::thread::Builder::new().name("test pattern".into())
                    .spawn(move || present::test_pattern(device, queue, adapter, window)).expect("a thread");
            }
            return;
        }
        std::thread::Builder::new()
            .name("guest main thread".into())
            .stack_size(256 << 20)
            .spawn(move || {
                let status = |s: String| {
                    let _ = proxy.send_event(GameEvent::Status(s));
                };
                let mut session = match ddruntime::session::Session::new(&exe) {
                    Ok(s) => s,
                    Err(e) => return status(format!("could not load {exe}: {e}")),
                };
                ddruntime::session::install_fault_hook();
                let _ = proxy.send_event(GameEvent::Input(ddruntime::input::connect()));
                sink::start(ddruntime::audio::output());
                match gpu {
                    Some((device, queue, adapter, window)) => {
                        let mut backend = ddgpu::WgpuBackend::new(device, queue, adapter);
                        backend.read_frames = false;
                        // the window follows the game's presentation; vsync too,
                        // unless --no-vsync overrides it
                        let (shown, modes) = (window.clone(), proxy.clone());
                        backend.present_sink = Some(Box::new(present::Sink::following(window, move |p| {
                            if vsync {
                                shown.set_vsync(p.interval != INTERVAL_IMMEDIATE);
                            }
                            let _ = modes.send_event(GameEvent::Mode(p));
                        })));
                        ddruntime::d3d9draw::set_backend(Box::new(backend));
                    }
                    None => status("no GPU device for the game's backend; running headless".into()),
                }
                status("running".into());
                let started = session.start();
                let threads: Vec<String> = ddruntime::threads::faults().into_iter()
                    .map(|(tid, f)| format!("guest thread {tid:#x}: {f}")).collect();
                let mut report = format!("{}: {}", started.step, started.outcome);
                for t in threads {
                    report.push_str(&format!("\n{t}"));
                }
                eprintln!("{report}");
                if let Some(stats) = ddruntime::threads::lock_stats() {
                    eprint!("{stats}");
                }
                status(format!("stopped in {}", started.step));
            })
            .expect("the guest main thread");
    }

    fn update_title(&mut self) {
        let Some(w) = &self.window else { return };
        let (frames, mode) = match &self.gpu {
            Some(g) => {
                let mut stats = g.window.stats.lock().unwrap();
                if let Some(r) = stats.take(Instant::now()) {
                    self.rate = format!("{:.1} FPS, {:.2} ms/frame (longest {:.1} ms)", r.fps, r.mean_ms, r.longest_ms);
                } else if stats.last_present.is_none_or(|t| t.elapsed() > TITLE_INTERVAL) {
                    self.rate = "not presenting".into();
                }
                (stats.frames, match g.window.present_mode() {
                    wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed => "vsync",
                    wgpu::PresentMode::Mailbox => "no vsync (mailbox)",
                    _ => "no vsync",
                })
            }
            None => (0, "no GPU"),
        };
        w.set_title(&format!("Dragon's Dogma: Dark Arisen (rebuilt) - {} - {} - {mode} - {frames} frames", self.status, self.rate));
    }

    /// Take the game's presentation: windowed at its back buffer's size, or
    /// full screen in the monitor's video mode with that size and refresh
    /// rate (the highest rate when the game asks for the default, 0);
    /// borderless when the monitor has no such mode.
    fn follow(&self, p: ddruntime::d3d9dev::Presentation) {
        let Some(w) = &self.window else { return };
        let size = winit::dpi::PhysicalSize::new(p.width, p.height);
        if p.windowed {
            w.set_fullscreen(None);
            let _ = w.request_inner_size(size);
            return;
        }
        let mode = w.current_monitor().and_then(|m| {
            m.video_modes()
                .filter(|v| v.size() == size && v.bit_depth() == 32)
                .filter(|v| p.refresh == 0 || (v.refresh_rate_millihertz() + 500) / 1000 == p.refresh)
                .max_by_key(|v| v.refresh_rate_millihertz())
        });
        w.set_fullscreen(Some(match mode {
            Some(v) => Fullscreen::Exclusive(v),
            None => Fullscreen::Borderless(None),
        }));
    }

    fn toggle_fullscreen(&self) {
        if let Some(w) = &self.window {
            w.set_fullscreen(if w.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(None)) });
        }
    }
}

impl ApplicationHandler<GameEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Dragon's Dogma: Dark Arisen (rebuilt) - starting")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("a window"));
        match App::create_gpu(window.clone(), self.vsync) {
            Ok(g) => self.gpu = Some(g),
            Err(e) => {
                eprintln!("no GPU for the window: {e}");
                self.status = format!("no GPU: {e}");
            }
        }
        self.window = Some(window);
        self.start_game();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: GameEvent) {
        match event {
            GameEvent::Input(host) => self.input = Some(input::Input::new(host)),
            GameEvent::Status(s) => {
                self.status = s;
                self.update_title();
            }
            GameEvent::Mode(p) => self.follow(p),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let (Some(input), Some(w)) = (self.input.as_mut(), &self.window) {
            let size = w.inner_size();
            // the game's back buffer is its window's client area
            let client = self.gpu.as_ref().map(|g| g.window.stats.lock().unwrap().back_buffer).filter(|&(w, h)| w > 0 && h > 0)
                .unwrap_or((size.width.max(1), size.height.max(1)));
            input.window_event(&event, (size.width, size.height), client);
        }
        match event {
            WindowEvent::CloseRequested => {
                // DDRUNTIME_LOCK_STATS=1: the guest lock per service (L64)
                if let Some(stats) = ddruntime::threads::lock_stats() {
                    eprint!("{stats}");
                }
                event_loop.exit()
            }
            WindowEvent::Resized(size) => {
                if let Some(g) = &self.gpu {
                    g.window.resized(size.width, size.height);
                    g.window.repaint_if_idle(IDLE);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &self.gpu {
                    g.window.repaint_if_idle(IDLE);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && !event.repeat => {
                let alt_enter = self.modifiers.alt_key() && event.logical_key == Key::Named(NamedKey::Enter);
                if alt_enter || event.logical_key == Key::Named(NamedKey::F11) {
                    self.toggle_fullscreen();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        if let Some(input) = self.input.as_mut() {
            input.device_event(&event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_title {
            self.update_title();
            self.next_title = now + TITLE_INTERVAL;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_title));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let vsync = !args.iter().any(|a| a == "--no-vsync");
    let test_pattern = args.iter().any(|a| a == "--test-pattern");
    let exe = args.iter().find(|a| !a.starts_with("--")).cloned()
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let event_loop = EventLoop::<GameEvent>::with_user_event().build().expect("an event loop");
    let mut app = App {
        exe,
        vsync,
        test_pattern,
        proxy: event_loop.create_proxy(),
        window: None,
        gpu: None,
        status: "starting".into(),
        rate: String::new(),
        next_title: Instant::now(),
        modifiers: ModifiersState::empty(),
        started: false,
        input: None,
    };
    event_loop.run_app(&mut app).expect("the event loop");
}
