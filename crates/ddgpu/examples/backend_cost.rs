//! L143 backend CPU cost: generated game-sized shaders on a 16x16 target.
//! Prebuilt draws, changing constant blocks, release build, no game assets.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};
use std::time::Instant;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct Allocator;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;


/// vs_3_0 of the game's size: `n` adds of constants.
fn vertex_shader(n: u32) -> Vec<u32> {
    let mut t = vec![0xFFFE_0300, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0005, 0x900F_0001,
                     0x0200_001F, 0x8000_0000, 0xE00F_0000, 0x0200_001F, 0x8000_0005, 0xE00F_0001,
                     0x0200_0001, 0x800F_0000, 0x90E4_0000];
    for k in 0..n {
        t.extend([0x0300_0002, 0x800F_0000, 0x80E4_0000, 0xA0E4_0000 | (k % 256)]);
    }
    // the position unchanged, so the triangles stay on screen
    t.extend([0x0200_0001, 0xE00F_0000, 0x90E4_0000, 0x0200_0001, 0xE00F_0001, 0x90E4_0001, 0x0000_FFFF]);
    t
}

/// ps_3_0 of the game's size sampling six textures.
fn pixel_shader(n: u32) -> Vec<u32> {
    let mut t = vec![0xFFFF_0300, 0x0200_001F, 0x8000_0005, 0x900F_0000];
    for s in 0..6 {
        t.extend([0x0200_001F, 0x9000_0000, 0xA00F_0800 | s]);
    }
    t.extend([0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800]);
    for s in 1..6 {
        t.extend([0x0300_0042, 0x800F_0001, 0x90E4_0000, 0xA0E4_0800 | s, 0x0300_0002, 0x800F_0000, 0x80E4_0000, 0x80E4_0001]);
    }
    for k in 0..n {
        t.extend([0x0300_0005, 0x800F_0000, 0x80E4_0000, 0xA0E4_0000 | (k % 32)]);
    }
    t.extend([0x0200_0001, 0x800F_0800, 0x80E4_0000, 0x0000_FFFF]);
    t
}

fn main() {
    let a: Vec<u32> = std::env::args().skip(1).filter_map(|v| v.parse().ok()).collect();
    let (draws, frames) = (a.first().copied().unwrap_or(1000), a.get(1).copied().unwrap_or(30));
    let mut gpu = ddgpu::WgpuBackend::headless().expect("a GPU adapter");
    gpu.read_frames = false;
    let (w, h) = (16u32, 16u32);
    let bb = Target { surface: 0x100, format: 21, width: w, height: h, texture: None, level: 0, samples: 0, quality: 0 };
    let depth = Target { surface: 0x101, format: 75, width: w, height: h, texture: None, level: 0, samples: 0, quality: 0 };
    let mut vb = Vec::new();
    for k in 0..64 {
        let (x, y) = ((k % 8) as f32 / 4.0 - 1.0, (k / 8) as f32 / 4.0 - 1.0);
        for v in [[x, y, 0.5, 1.0, 0.0, 1.0], [x, y + 0.25, 0.5, 1.0, 0.0, 0.0], [x + 0.25, y, 0.5, 1.0, 1.0, 1.0]] {
            vb.extend(v.iter().flat_map(|f: &f32| f.to_le_bytes()));
        }
    }
    gpu.upload(Upload::Buffer { resource: 0x300, bytes: vb });
    let ib: Vec<u8> = (0..192u16).flat_map(|i| i.to_le_bytes()).collect();
    gpu.upload(Upload::Buffer { resource: 0x301, bytes: ib });
    // 96 64x64 textures with 7 levels
    for t in 0..96u32 {
        for level in 0..7u32 {
            let size = 64 >> level;
            let texels: Vec<u8> = (0..size * size).flat_map(|k| [(k * 3 + t * 20) as u8, (k * 5) as u8, (k * 7) as u8, 255]).collect();
            gpu.upload(Upload::Image { resource: 0x200 + t, index: level, format: 21, width: size, height: size, depth: 1, pitch: 4 * size, slice: 4 * size * size, bytes: texels });
        }
    }
    let mut sampler = [0u32; 14];
    for (k, v) in [(1, 1), (2, 1), (3, 1), (5, 2), (6, 2), (7, 2), (10, 1)] {
        sampler[k] = v;
    }
    // the device's render states (~200; most at values the backend ignores)
    let mut states: Vec<(u32, u32)> = (1..210).map(|k| (k, 0)).collect();
    for (k, v) in [(7, 1), (14, 1), (23, 4), (22, 3), (8, 3), (27, 0), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (174, 0), (162, !0), (58, 0xFF), (59, 0xFF)] {
        states.iter_mut().find(|s| s.0 == k).unwrap().1 = v;
    }
    let shaders: Vec<(std::sync::Arc<[u32]>, std::sync::Arc<[u32]>)> = (0..8).map(|v| (vertex_shader(140 + v).into(), pixel_shader(90 + v).into())).collect();
    let draw = |k: u32| {
        let (vs, ps) = &shaders[(k % 8) as usize];
        let mut states = states.clone();
        if k % 4 == 0 {
            for (s, v) in [(27, 1), (19, 5), (20, 6)] {
                states.iter_mut().find(|e| e.0 == s).unwrap().1 = v;
            }
        }
        let mut vs_c = Constants { f: vec![[0; 4]; 256], ..Default::default() };
        vs_c.f[(k % 200) as usize] = [1f32.to_bits(); 4];
        let mut ps_c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
        ps_c.f[0] = [((k % 7) as f32 / 7.0).to_bits(), 0.5f32.to_bits(), 1f32.to_bits(), 1f32.to_bits()];
        let set = (k * 7) % 40 * 2;
        DrawCall {
            primitive: 4, primitives: 1, start: 3 * (k % 64), base_vertex: 0, min_index: 0, vertex_count: 192,
            vertices: Vertices::Streams(vec![(0, 0x300, 0, 24, 1)].into()),
            indices: Indices::Buffer(0x301, 101),
            vertex_shader: Some(vs.clone()),
            pixel_shader: Some(ps.clone()),
            declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]].into(),
            fvf: 0,
            vs_constants: vs_c.into(),
            ps_constants: ps_c.into(),
            textures: (0..6).map(|s| TextureBinding { sampler: s, resource: 0x200 + set + s, kind: TexKind::Flat, format: 21, width: 64, height: 64, depth: 1, levels: 7, usage: 0, states: sampler }).collect(),
            render_states: states.into(),
            viewport: [0, 0, w, h, 0f32.to_bits(), 1f32.to_bits()],
            scissor: [0, 0, w as i32, h as i32],
            targets: [Some(bb.clone()), None, None, None],
            depth: Some(depth.clone()),
        }
    };
    let stream: Vec<DrawCall> = (0..draws).map(draw).collect();
    let clear = ClearCall { rects: vec![], flags: 7, colour: 0xFF20_2020, z: 1.0, stencil: 0, targets: [Some(bb.clone()), None, None, None], depth: Some(depth.clone()), viewport: [0, 0, w, h, 0, 0], scissor: None, srgb: false };
    let frame = |gpu: &mut dyn GraphicsBackend| {
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
    let allocations = ALLOCS.load(Ordering::Relaxed);
    let (mut submit, mut total) = (0.0, 0.0);
    for _ in 0..frames {
        let (s, t) = frame(&mut gpu);
        submit += s.as_secs_f64();
        total += t.as_secs_f64();
    }
    println!("{draws} game-like draws x {frames} frames: {:.2} us per draw submitted, {:.2} ms per frame with the GPU waited for",
             submit * 1e6 / (draws * frames) as f64, total * 1e3 / frames as f64);
    println!("direct allocations/draw={:.3}", (ALLOCS.load(Ordering::Relaxed) - allocations) as f64 / (draws * frames) as f64);
    let (recorded, passes) = gpu.pass_counts();
    println!("{recorded} draws in {passes} render passes ({:.1} draws per pass)", recorded as f64 / passes.max(1) as f64);
    // Reuse the same warmed device, shaders and pipelines. The submission
    // interval now includes owned snapshot copies and bounded-queue handoff;
    // total time still waits for actual GPU completion through read_back.
    let mut deferred = d3d9cmd::DeferredBackend::new(Box::new(gpu));
    frame(&mut deferred);
    let allocations = ALLOCS.load(Ordering::Relaxed);
    let (mut queued_submit, mut queued_total) = (0.0, 0.0);
    for _ in 0..frames {
        let (s, t) = frame(&mut deferred);
        queued_submit += s.as_secs_f64();
        queued_total += t.as_secs_f64();
    }
    println!("deferred allocations/draw={:.3}", (ALLOCS.load(Ordering::Relaxed) - allocations) as f64 / (draws * frames) as f64);
    println!("deferred: {:.2} us per draw recorded, {:.2} ms per frame with the GPU waited for; producer speedup {:.2}x",
        queued_submit * 1e6 / (draws * frames) as f64,
        queued_total * 1e3 / frames as f64, submit / queued_submit);
}
