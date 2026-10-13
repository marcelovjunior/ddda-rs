//! Synthetic eight-copy frames, not a gameplay/FPS measurement.
//! cargo run --release -p ddgpu --example copy_cost -- [frames]
use d3d9cmd::{ClearCall, CopyCall, GraphicsBackend, Target};
use std::time::{Duration, Instant};

fn target(surface: u32, format: u32, width: u32, height: u32, samples: u32) -> Target {
    Target { surface, format, width, height, samples, quality: 0, texture: None, level: 0 }
}

fn main() {
    let frames: u32 = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(10);
    assert!(frames > 0);
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter");
    gpu.read_frames = false;
    for (index, mode) in ["same", "g16r16", "scaled", "conversion", "depth", "resolve"].into_iter().enumerate() {
        let id = 0x100 + index as u32 * 10;
        let depth = mode == "depth";
        let (w, h) = if depth || mode == "g16r16" { (192, 128) } else { (1920, 1080) };
        let format = if depth { 75 } else if mode == "g16r16" { 34 } else { 21 };
        let src = target(id, format, w, h, if mode == "resolve" { 4 } else { 0 });
        let dst = target(id + 1, if mode == "conversion" { 113 } else { format },
            if mode == "scaled" { w / 2 } else { w }, if mode == "scaled" { h / 2 } else { h }, 0);
        let bb = target(id + 2, 21, w, h, src.samples);
        let clear = ClearCall {
            rects: vec![], flags: if depth { 7 } else { 1 }, colour: 0xFF4080C0,
            z: 0.375, stencil: 0xA5,
            targets: [Some(if depth { bb.clone() } else { src.clone() }), None, None, None],
            depth: depth.then(|| src.clone()), viewport: [0, 0, w, h, 0, 1f32.to_bits()],
            scissor: None, srgb: false,
        };
        let copy = CopyCall { src: src.clone(), dst: dst.clone(), src_rect: [0; 4], dst_rect: [0; 4], filter: 0 };
        let mut cost = Duration::ZERO;
        let mut total = Duration::ZERO;
        let mut submits = 0;
        for frame in 0..=frames {
            let begin = Instant::now();
            let before = gpu.submission_count();
            gpu.clear(&clear);
            let start = Instant::now();
            for _ in 0..8 { gpu.copy(&copy); }
            let elapsed = start.elapsed();
            gpu.present(if depth { &bb } else { &dst });
            let count = gpu.submission_count() - before;
            // A separate readback waits for completion; excluded from submit count.
            assert!(gpu.read_back(if depth { &bb } else { &dst }).is_some());
            assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
            if frame > 0 {
                cost += elapsed;
                total += begin.elapsed();
                submits += count;
            }
        }
        println!("{mode}: {frames} frames x 8 copies; {:.3} us/copy; {:.1} submits/frame; {:.3} ms/frame waited",
            cost.as_secs_f64() * 1e6 / f64::from(frames * 8),
            submits as f64 / f64::from(frames), total.as_secs_f64() * 1e3 / f64::from(frames));
    }
}
