//! What a Present costs the guest thread (brief L25): the read-back path
//! (`read_frames`, which ddplay used to show frames: copy to a buffer, wait
//! for the GPU, map, swizzle) against a `PresentSink` that draws the back
//! buffer into another texture on the GPU (what a window surface gets).
//!
//!     cargo run --release -p ddgpu --example present_cost [width height frames]
//!
//! Each frame is a clear and a ColorFill of the back buffer, then Present.
//! "pipelined" lets the GPU run behind (a window with frame latency);
//! "waited" waits for the GPU after every frame, as the read-back must.
use d3d9cmd::{ClearCall, FillCall, GraphicsBackend, Target};
use ddgpu::present::Blitter;
use ddgpu::{PresentSink, Presented, WgpuBackend};
use std::time::Instant;

struct Offscreen {
    blitter: Option<Blitter>,
    target: Option<wgpu::Texture>,
    wait: bool,
}

impl PresentSink for Offscreen {
    fn present(&mut self, f: &Presented) {
        let blitter = self.blitter.get_or_insert_with(|| Blitter::new(f.device));
        let t = self.target.get_or_insert_with(|| f.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: f.width, height: f.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }));
        let view = t.create_view(&Default::default());
        blitter.draw(f, &view, wgpu::TextureFormat::Bgra8Unorm, f.width, f.height);
        if self.wait {
            f.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
    }
}

fn run(gpu: &mut WgpuBackend, w: u32, h: u32, frames: u32) -> f64 {
    let bb = Target { surface: 0x100, format: 21, width: w, height: h, texture: None, level: 0 };
    let frame = |gpu: &mut WgpuBackend, k: u32| {
        gpu.clear(&ClearCall {
            rects: vec![], flags: 1, colour: 0xFF00_0000 | (k & 0xFF), z: 1.0, stencil: 0,
            targets: [Some(bb.clone()), None, None, None], depth: None, viewport: [0, 0, w, h, 0, 0],
        });
        let x = (k * 7 % w) as i32;
        gpu.fill(&FillCall { dst: bb.clone(), rect: [x, 0, (x + 64).min(w as i32), h as i32 / 2], colour: 0xFFFF_FFFF });
        gpu.present(&bb);
    };
    for k in 0..10 {
        frame(gpu, k); // warm-up: pipelines, targets
    }
    let t = Instant::now();
    for k in 0..frames {
        frame(gpu, k);
    }
    t.elapsed().as_secs_f64() * 1e3 / frames as f64
}

fn main() {
    let a: Vec<u32> = std::env::args().skip(1).filter_map(|v| v.parse().ok()).collect();
    let (w, h, frames) = (a.first().copied().unwrap_or(1280), a.get(1).copied().unwrap_or(720), a.get(2).copied().unwrap_or(300));
    let backend = || WgpuBackend::headless().expect("a GPU adapter");
    let mut read = backend();
    read.read_frames = true;
    let ms = run(&mut read, w, h, frames);
    println!("{w}x{h}, {frames} frames: read-back present {ms:.3} ms/frame");
    for wait in [true, false] {
        let mut gpu = backend();
        gpu.read_frames = false;
        gpu.present_sink = Some(Box::new(Offscreen { blitter: None, target: None, wait }));
        let ms = run(&mut gpu, w, h, frames);
        println!("{w}x{h}, {frames} frames: GPU present ({}) {ms:.3} ms/frame", if wait { "waited" } else { "pipelined" });
    }
}
