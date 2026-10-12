//! The window's side of presenting (brief L25): the game's back buffer
//! goes to the window surface on the GPU, with no read-back.
//!
//! The backend and the surface share one wgpu device. At every Present,
//! the guest thread (which runs the backend) acquires the surface texture,
//! draws the back buffer into it with `ddgpu::present::Blitter` and
//! presents it. With vsync the surface's FIFO queue paces the guest thread
//! at Present, as a D3D9 Present with a presentation interval of one does.
//! The window thread only reports size changes, which the next Present
//! applies, and repaints the last frame itself when the game has stopped
//! presenting (stopped or loading), so a resized window is never left
//! blank.
use ddgpu::present::Blitter;
use ddgpu::{PresentSink, Presented};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Frame-time statistics, read by the window for its title.
#[derive(Default)]
pub struct Stats {
    pub frames: u64,
    pub last_present: Option<Instant>,
    /// The last presented back buffer's size (the game window's client area).
    pub back_buffer: (u32, u32),
    interval_start: Option<Instant>,
    interval_frames: u32,
    longest: Duration,
}

/// What the title shows for one interval.
pub struct Rate {
    pub fps: f64,
    pub mean_ms: f64,
    pub longest_ms: f64,
}

impl Stats {
    fn presented(&mut self, now: Instant) {
        if let Some(prev) = self.last_present {
            self.longest = self.longest.max(now - prev);
        }
        self.frames += 1;
        self.interval_frames += 1;
        self.last_present = Some(now);
        self.interval_start.get_or_insert(now);
    }

    /// The rate since the last call, if a frame was presented since.
    pub fn take(&mut self, now: Instant) -> Option<Rate> {
        let start = self.interval_start?;
        let secs = (now - start).as_secs_f64();
        if self.interval_frames == 0 || secs <= 0.0 {
            return None;
        }
        let r = Rate {
            fps: self.interval_frames as f64 / secs,
            mean_ms: secs * 1e3 / self.interval_frames as f64,
            longest_ms: self.longest.as_secs_f64() * 1e3,
        };
        (self.interval_start, self.interval_frames, self.longest) = (Some(now), 0, Duration::ZERO);
        Some(r)
    }
}

/// The last presented back buffer, for repaints while the game is not
/// presenting.
struct Last {
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    gamma: Option<Box<[[u16; 256]; 3]>>,
}

struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    blitter: Blitter,
    last: Option<Last>,
    /// the present modes the surface offers
    modes: Vec<wgpu::PresentMode>,
}

/// The surface, shared by the guest thread (Present) and the window thread.
pub struct Window {
    surface: Mutex<Surface>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// a size the surface has to take: width << 32 | height, 0 = none
    resize: AtomicU64,
    /// a vsync choice the surface has to take: 1 on, 2 off, 0 none
    vsync: AtomicU8,
    pub stats: Mutex<Stats>,
}

impl Window {
    /// Configure `surface` for the window's size: a format that shows the
    /// bytes as they are (non-sRGB), and FIFO (vsync) or the fastest mode
    /// the surface has without it.
    pub fn new(surface: wgpu::Surface<'static>, adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue,
               width: u32, height: u32, vsync: bool) -> Result<Arc<Window>, String> {
        let caps = surface.get_capabilities(adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied()).ok_or("the surface has no formats")?;
        let view_format = Blitter::view_format(format);
        let present_mode = present_mode(&caps.present_modes, vsync);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format != format { vec![view_format] } else { vec![] },
        };
        surface.configure(&device, &config);
        let blitter = Blitter::new(&device);
        Ok(Arc::new(Window {
            surface: Mutex::new(Surface { surface, config, view_format, blitter, last: None, modes: caps.present_modes.clone() }),
            device,
            queue,
            resize: AtomicU64::new(0),
            vsync: AtomicU8::new(0),
            stats: Mutex::new(Stats::default()),
        }))
    }

    pub fn present_mode(&self) -> wgpu::PresentMode {
        self.surface.lock().unwrap().config.present_mode
    }

    /// The window was resized: the next present (or repaint) applies it.
    pub fn resized(&self, width: u32, height: u32) {
        self.resize.store(((width as u64) << 32) | height as u64 | if width == 0 && height == 0 { 1 << 63 } else { 0 }, Ordering::Release);
    }

    /// Present with vsync (FIFO) or without it from the next present on.
    pub fn set_vsync(&self, on: bool) {
        self.vsync.store(if on { 1 } else { 2 }, Ordering::Release);
    }

    /// Repaint the last frame from the window thread, unless the guest
    /// presented within `idle` (it will paint the next frame itself) or is
    /// presenting right now.
    pub fn repaint_if_idle(&self, idle: Duration) {
        let recent = self.stats.lock().unwrap().last_present.is_some_and(|t| t.elapsed() < idle);
        if recent {
            return;
        }
        let Ok(mut s) = self.surface.try_lock() else { return };
        let Some(last) = s.last.as_ref() else { return };
        let (texture, format, width, height, gamma) = (last.texture.clone(), last.format, last.width, last.height, last.gamma.clone());
        let frame = Presented { device: &self.device, queue: &self.queue, texture: &texture, format, width, height, gamma: gamma.as_deref() };
        self.show(&mut s, &frame);
    }

    fn show(&self, s: &mut Surface, frame: &Presented) {
        let r = self.resize.swap(0, Ordering::AcqRel);
        let v = self.vsync.swap(0, Ordering::AcqRel);
        let mode = if v != 0 { present_mode(&s.modes, v == 1) } else { s.config.present_mode };
        if r != 0 || mode != s.config.present_mode {
            if r != 0 {
                s.config.width = (r >> 32) as u32 & 0x7FFF_FFFF;
                s.config.height = r as u32;
            }
            s.config.present_mode = mode;
            if s.config.width > 0 && s.config.height > 0 {
                s.surface.configure(&self.device, &s.config);
            }
        }
        if s.config.width == 0 || s.config.height == 0 {
            return; // minimised
        }
        let mut out = None;
        for _ in 0..2 {
            match s.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                    out = Some(t);
                    break;
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => s.surface.configure(&self.device, &s.config),
                _ => break, // timeout or occluded: skip this frame
            }
        }
        let Some(out) = out else { return };
        let view = out.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(s.view_format), ..Default::default() });
        let (format, w, h) = (s.view_format, s.config.width, s.config.height);
        s.blitter.draw(frame, &view, format, w, h);
        out.present();
    }
}

fn present_mode(modes: &[wgpu::PresentMode], vsync: bool) -> wgpu::PresentMode {
    use wgpu::PresentMode::*;
    let order: &[wgpu::PresentMode] = if vsync { &[Fifo] } else { &[Immediate, Mailbox, Fifo] };
    order.iter().copied().find(|m| modes.contains(m)).unwrap_or(Fifo)
}

/// The backend's sink: shows each Present in the window, and tells the
/// window when the game's presentation (its back buffer size, windowed or
/// full screen, refresh rate, vsync) changes (brief L34).
pub struct Sink {
    pub window: Arc<Window>,
    /// Called on the guest thread with each new presentation.
    follow: Option<Box<dyn FnMut(ddruntime::d3d9dev::Presentation) + Send>>,
    last: Option<ddruntime::d3d9dev::Presentation>,
}

impl Sink {
    pub fn new(window: Arc<Window>) -> Sink {
        Sink { window, follow: None, last: None }
    }

    /// A sink that also reports the game's presentation to `follow`.
    pub fn following(window: Arc<Window>, follow: impl FnMut(ddruntime::d3d9dev::Presentation) + Send + 'static) -> Sink {
        Sink { window, follow: Some(Box::new(follow)), last: None }
    }
}

impl PresentSink for Sink {
    fn present(&mut self, frame: &Presented) {
        if let Some(follow) = self.follow.as_mut() {
            // the session's state is this (guest) thread's
            let p = ddruntime::d3d9dev::presentation();
            if p != self.last {
                self.last = p;
                if let Some(p) = p {
                    follow(p);
                }
            }
        }
        let w = &self.window;
        {
            let mut s = w.surface.lock().unwrap();
            w.show(&mut s, frame);
            s.last = Some(Last {
                texture: frame.texture.clone(),
                format: frame.format,
                width: frame.width,
                height: frame.height,
                gamma: frame.gamma.map(|g| Box::new(*g)),
            });
        }
        let mut stats = w.stats.lock().unwrap();
        stats.presented(Instant::now());
        stats.back_buffer = (frame.width, frame.height);
    }
}

/// A diagnostic, not the game: draws a moving pattern through the game's
/// backend (clears, ColorFill, Present on a 1280x720 back buffer) so the
/// window's presentation, pacing and resizing can be checked before the
/// game presents frames of its own.
pub fn test_pattern(device: wgpu::Device, queue: wgpu::Queue, adapter: wgpu::AdapterInfo, window: Arc<Window>) {
    use ddruntime::d3d9draw::{ClearCall, FillCall, GraphicsBackend, Target};
    let mut gpu = ddgpu::WgpuBackend::new(device, queue, adapter);
    gpu.read_frames = false;
    gpu.present_sink = Some(Box::new(Sink::new(window)));
    let (w, h) = (1280u32, 720u32);
    let bb = Target { surface: 0x100, format: 21, width: w, height: h, texture: None, level: 0, samples: 0, quality: 0 };
    for k in 0u32.. {
        gpu.clear(&ClearCall {
            rects: vec![], flags: 1, colour: 0xFF10_1828, z: 1.0, stencil: 0,
            targets: [Some(bb.clone()), None, None, None], depth: None, viewport: [0, 0, w, h, 0, 0],
            scissor: None, srgb: false,
        });
        let x = (k * 8 % (w - 64)) as i32;
        gpu.fill(&FillCall { dst: bb.clone(), rect: [x, 0, x + 64, h as i32], colour: 0xFFE0_E0E0 });
        gpu.fill(&FillCall { dst: bb.clone(), rect: [0, 0, 8, 8], colour: 0xFFFF_0000 });
        gpu.fill(&FillCall { dst: bb.clone(), rect: [w as i32 - 8, h as i32 - 8, w as i32, h as i32], colour: 0xFF00_FF00 });
        gpu.present(&bb);
    }
}
