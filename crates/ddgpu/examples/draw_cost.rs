//! What the wgpu backend costs the guest thread per draw (brief L25,
//! part 2). Until the game issues draws of its own, the stream is
//! synthetic: frames of `draws` textured triangles from a vertex buffer
//! (the draw tests' shaders and states), each with different constants, a
//! few textures and blend settings, as a frame of the game would vary them.
//!
//!     cargo run --release -p ddgpu --example draw_cost [draws frames]
//!
//! Reported: guest-thread time per draw (submitting), and per frame with
//! the GPU waited for at Present.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};
use std::time::Instant;

fn vs() -> Vec<u32> {
    vec![
        0xFFFE_0300,
        0x0200_001F, 0x8000_0000, 0x900F_0000,
        0x0200_001F, 0x8000_0005, 0x900F_0001,
        0x0200_001F, 0x8000_0000, 0xE00F_0000,
        0x0200_001F, 0x8000_0005, 0xE00F_0001,
        0x0200_0001, 0xE00F_0000, 0x90E4_0000,
        0x0200_0001, 0xE00F_0001, 0x90E4_0001,
        0x0000_FFFF,
    ]
}

/// texld r0, v0, s0; mul oC0, r0, c0
fn ps() -> Vec<u32> {
    vec![
        0xFFFF_0300,
        0x0200_001F, 0x8000_0005, 0x9003_0000,
        0x0200_001F, 0x9000_0000, 0xA00F_0800,
        0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800,
        0x0300_0005, 0x800F_0800, 0x80E4_0000, 0xA0E4_0000,
        0x0000_FFFF,
    ]
}

fn main() {
    let a: Vec<u32> = std::env::args().skip(1).filter_map(|v| v.parse().ok()).collect();
    let (draws, frames) = (a.first().copied().unwrap_or(1000), a.get(1).copied().unwrap_or(30));
    let mut gpu = ddgpu::WgpuBackend::headless().expect("a GPU adapter");
    gpu.read_frames = false;
    let (w, h) = (1280u32, 720u32);
    let bb = Target { surface: 0x100, format: 21, width: w, height: h, texture: None, level: 0 };
    // 64 small triangles' worth of vertices: position float4, texcoord float2
    let mut vb = Vec::new();
    for k in 0..64 {
        let (x, y) = ((k % 8) as f32 / 4.0 - 1.0, (k / 8) as f32 / 4.0 - 1.0);
        for v in [[x, y, 0.0, 1.0, 0.0, 1.0], [x, y + 0.25, 0.0, 1.0, 0.0, 0.0], [x + 0.25, y, 0.0, 1.0, 1.0, 1.0]] {
            vb.extend(v.iter().flat_map(|f: &f32| f.to_le_bytes()));
        }
    }
    gpu.upload(Upload::Buffer { resource: 0x300, bytes: vb });
    for t in 0..4u32 {
        let texels: Vec<u8> = (0..64 * 64).flat_map(|k| [(k * 3 + t * 50) as u8, (k * 5) as u8, (k * 7) as u8, 255]).collect();
        gpu.upload(Upload::Image { resource: 0x200 + t, index: 0, format: 21, width: 64, height: 64, depth: 1, pitch: 256, slice: 0x4000, bytes: texels });
    }
    let mut sampler = [0u32; 14];
    for (k, v) in [(1, 1), (2, 1), (3, 1), (5, 2), (6, 2)] {
        sampler[k] = v;
    }
    let draw = |k: u32| {
        let mut ps_c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
        ps_c.f[0] = [((k % 7) as f32 / 7.0).to_bits(), 0.5f32.to_bits(), 1f32.to_bits(), 1f32.to_bits()];
        let mut states = vec![(7, 0), (22, 1), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (15, 0)];
        // half the draws alpha-blended
        states.extend(if k.is_multiple_of(2) { vec![(27, 0)] } else { vec![(27, 1), (19, 5), (20, 6), (171, 1)] });
        states.sort();
        DrawCall {
            primitive: 4, primitives: 1, start: 3 * (k % 64), base_vertex: 0, min_index: 0, vertex_count: 3,
            vertices: Vertices::Streams(vec![(0, 0x300, 0, 24, 1)]),
            indices: Indices::None,
            vertex_shader: Some(vs()),
            pixel_shader: Some(ps()),
            declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]],
            fvf: 0,
            vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() },
            ps_constants: ps_c,
            textures: vec![TextureBinding { sampler: 0, resource: 0x200 + k % 4, kind: TexKind::Flat, format: 21, width: 64, height: 64, depth: 1, levels: 1, states: sampler }],
            render_states: states,
            viewport: [0, 0, w, h, 0f32.to_bits(), 1f32.to_bits()],
            scissor: [0, 0, w as i32, h as i32],
            targets: [Some(bb.clone()), None, None, None],
            depth: None,
        }
    };
    // the commands are built before timing: the backend's cost only
    let stream: Vec<DrawCall> = (0..draws).map(draw).collect();
    let clear = ClearCall { rects: vec![], flags: 1, colour: 0xFF20_2020, z: 1.0, stencil: 0, targets: [Some(bb.clone()), None, None, None], depth: None, viewport: [0, 0, w, h, 0, 0] };
    let frame = |gpu: &mut ddgpu::WgpuBackend| {
        let t = Instant::now();
        gpu.clear(&clear);
        for d in &stream {
            gpu.draw(d);
        }
        let submit = t.elapsed();
        gpu.present(&bb);
        let _ = gpu.read_back(&bb); // waits for the GPU
        (submit, t.elapsed())
    };
    frame(&mut gpu); // warm-up: shaders, pipelines
    assert!(gpu.errors.is_empty(), "{:?}", &gpu.errors[..gpu.errors.len().min(3)]);
    let (mut submit, mut total) = (0.0, 0.0);
    for _ in 0..frames {
        let (s, t) = frame(&mut gpu);
        submit += s.as_secs_f64();
        total += t.as_secs_f64();
    }
    println!("{draws} draws x {frames} frames: {:.2} us per draw submitted, {:.2} ms per frame with the GPU waited for",
             submit * 1e6 / (draws * frames) as f64, total * 1e3 / frames as f64);
}
