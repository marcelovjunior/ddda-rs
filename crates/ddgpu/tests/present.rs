//! Presenting without read-back (brief L25): a `PresentSink` gets the back
//! buffer on the GPU, and `present::Blitter` shows it with the same pixels
//! the read-back path produces. Skips without an adapter.
use d3d9cmd::{ClearCall, FillCall, GammaRamp, GraphicsBackend, Target};
use ddgpu::present::{fit, Blitter};
use ddgpu::{PresentSink, Presented, WgpuBackend};
use std::sync::{Arc, Mutex};

fn back_buffer() -> Target {
    Target { surface: 0x100, format: 21, width: 4, height: 4, texture: None, level: 0 }
}

/// Read an RGBA8 texture back (rows top to bottom).
fn read(device: &wgpu::Device, queue: &wgpu::Queue, t: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    let row = (w * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = buffer.slice(..).get_mapped_range();
    (0..h).flat_map(|y| data[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect()
}

/// A sink that shows each frame in a `w` x `h` texture (standing in for a
/// window surface) and keeps what it showed.
struct Window {
    blitter: Option<Blitter>,
    w: u32,
    h: u32,
    shown: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl PresentSink for Window {
    fn present(&mut self, f: &Presented) {
        let blitter = self.blitter.get_or_insert_with(|| Blitter::new(f.device));
        let t = f.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: self.w, height: self.h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        // a surface that prefers sRGB is drawn through its plain view
        let format = Blitter::view_format(t.format());
        let view = t.create_view(&wgpu::TextureViewDescriptor { format: Some(format), ..Default::default() });
        blitter.draw(f, &view, format, self.w, self.h);
        self.shown.lock().unwrap().push(read(f.device, f.queue, &t, self.w, self.h));
    }
}

/// A 4x4 back buffer with four differently coloured quadrants, presented
/// once to a window of `w` x `h`: (the read-back frame, what the window showed).
fn present_quadrants(w: u32, h: u32, gamma: Option<&GammaRamp>) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut gpu = WgpuBackend::headless()?;
    let shown = Arc::new(Mutex::new(Vec::new()));
    gpu.present_sink = Some(Box::new(Window { blitter: None, w, h, shown: shown.clone() }));
    gpu.gamma(gamma);
    gpu.clear(&ClearCall {
        rects: vec![], flags: 1, colour: 0xFF10_2030, z: 1.0, stencil: 0,
        targets: [Some(back_buffer()), None, None, None], depth: None, viewport: [0, 0, 4, 4, 0, 0],
    });
    // ColorFill quadrants (D3DCOLOR is ARGB): red, green / blue, a mix
    for (rect, colour) in [([0, 0, 2, 2], 0xFFFF_0000), ([2, 0, 4, 2], 0xFF00_FF00), ([0, 2, 2, 4], 0xFF00_00FF), ([2, 2, 4, 4], 0xFFC8_7828)] {
        gpu.fill(&FillCall { dst: back_buffer(), rect, colour });
    }
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    gpu.present(&back_buffer());
    let frame = gpu.last_frame.clone().expect("read-back frame").rgba;
    let shown = shown.lock().unwrap().pop().expect("the sink was called");
    Some((frame, shown))
}

#[test]
fn the_window_shows_the_read_back_pixels() {
    let Some((frame, shown)) = present_quadrants(4, 4, None) else {
        eprintln!("skipped: no adapter");
        return;
    };
    assert_eq!(&frame[0..4], &[255, 0, 0, 255], "red top left");
    assert_eq!(&frame[60..64], &[200, 120, 40, 255], "the mix bottom right");
    assert_eq!(shown, frame);
}

#[test]
fn the_window_applies_the_gamma_ramp_like_the_read_back() {
    let ramp: GammaRamp = [
        std::array::from_fn(|k| ((255 - k as u16) << 8) | 0x7F),
        std::array::from_fn(|k| ((k as u16 / 2) << 8) | 0xFF),
        std::array::from_fn(|k| k as u16 * 257),
    ];
    let Some((frame, shown)) = present_quadrants(4, 4, Some(&ramp)) else { return };
    assert_eq!(&frame[0..4], &[0, 0, 0, 255], "red 255 maps to 0, green 0 to 0");
    assert_eq!(shown, frame);
}

#[test]
fn a_wider_window_letterboxes_the_exact_pixels() {
    let Some((frame, shown)) = present_quadrants(8, 4, None) else { return };
    for y in 0..4 {
        for x in 0..8 {
            let px = &shown[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4];
            if (2..6).contains(&x) {
                assert_eq!(px, &frame[(y * 4 + x - 2) * 4..(y * 4 + x - 2) * 4 + 4], "({x}, {y})");
            } else {
                assert_eq!(px, &[0, 0, 0, 255], "bar at ({x}, {y})");
            }
        }
    }
}

#[test]
fn a_doubled_window_keeps_the_corner_pixels() {
    let Some((frame, shown)) = present_quadrants(8, 8, None) else { return };
    let src = |x: usize, y: usize| &frame[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
    let dst = |x: usize, y: usize| &shown[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4];
    // bilinear with clamped edges: the outer pixels are the corner texels
    assert_eq!(dst(0, 0), src(0, 0));
    assert_eq!(dst(7, 0), src(3, 0));
    assert_eq!(dst(0, 7), src(0, 3));
    assert_eq!(dst(7, 7), src(3, 3));
}

#[test]
fn fitting_keeps_the_aspect_ratio() {
    assert_eq!(fit(1280, 720, 1280, 720), [0, 0, 1280, 720]);
    assert_eq!(fit(1280, 720, 1920, 1080), [0, 0, 1920, 1080]);
    assert_eq!(fit(1280, 720, 1280, 1024), [0, 152, 1280, 720]);
    assert_eq!(fit(1280, 720, 2560, 1080), [320, 0, 1920, 1080]);
    assert_eq!(fit(4, 4, 8, 4), [2, 0, 4, 4]);
}
