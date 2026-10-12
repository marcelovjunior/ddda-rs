//! The backend's caches (pipelines, layouts, bind groups, target views,
//! ring buffers, batched submission) never change what a draw draws: one
//! long-lived backend running many differently configured frames produces,
//! frame by frame, the pixels a fresh backend produces for that frame
//! alone. Skips without an adapter.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};

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

/// mov oC0, c0
fn ps_constant() -> Vec<u32> {
    vec![0xFFFF_0300, 0x0200_0001, 0x800F_0800, 0xA0E4_0000, 0x0000_FFFF]
}

/// mov oC0, c0; mov oC1, c0 (two targets)
fn ps_two() -> Vec<u32> {
    vec![0xFFFF_0300, 0x0200_0001, 0x800F_0800, 0xA0E4_0000, 0x0200_0001, 0x800F_0801, 0xA0E4_0000, 0x0000_FFFF]
}

/// texld r0, v0, s0; mul oC0, r0, c0
fn ps_texture() -> Vec<u32> {
    vec![
        0xFFFF_0300,
        0x0200_001F, 0x8000_0005, 0x9003_0000,
        0x0200_001F, 0x9000_0000, 0xA00F_0800,
        0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800,
        0x0300_0005, 0x800F_0800, 0x80E4_0000, 0xA0E4_0000,
        0x0000_FFFF,
    ]
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.next() as usize % v.len()]
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 1000) as f32 / 1000.0
    }
}

const W: u32 = 16;
const H: u32 = 16;

fn back_buffer() -> Target {
    Target { surface: 0x100, format: 21, width: W, height: H, texture: None, level: 0, samples: 0, quality: 0 }
}

fn depth_target() -> Target {
    Target { surface: 0x101, format: 75, width: W, height: H, texture: None, level: 0, samples: 0, quality: 0 }
}

/// The second render target of MRT draws; its format varies by frame.
fn second_target(format: u32) -> Target {
    Target { surface: 0x102, format, width: W, height: H, texture: None, level: 0, samples: 0, quality: 0 }
}

/// A random vertex (position float4, texcoord float2).
fn vertex(rng: &mut Rng) -> [f32; 6] {
    [rng.unit() * 2.4 - 1.2, rng.unit() * 2.4 - 1.2, rng.unit(), 1.0, rng.unit() * 2.0, rng.unit() * 2.0]
}

fn bytes(v: &[[f32; 6]]) -> Vec<u8> {
    v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

/// One frame's commands: uploads (with the textures' formats), clears (a
/// full one, sometimes a partial one: viewport, rectangle, scissor, sRGB)
/// and a few draws with random states, some into two targets (the second
/// of a random format).
fn frame(rng: &mut Rng) -> (Vec<Upload>, Vec<ClearCall>, Vec<DrawCall>, Target) {
    let mut uploads = Vec::new();
    let second = second_target(rng.pick(&[21u32, 113, 114]));
    // three textures; their formats vary between frames
    let mut formats = [0u32; 3];
    for (k, f) in formats.iter_mut().enumerate() {
        *f = rng.pick(&[21u32, 22, 23, 50]);
        let bpp = if matches!(*f, 21 | 22) { 4 } else if *f == 23 { 2 } else { 1 };
        let texels: Vec<u8> = (0..4 * 4 * bpp).map(|_| rng.next() as u8).collect();
        uploads.push(Upload::Image { resource: 0x200 + k as u32, index: 0, format: *f, width: 4, height: 4, depth: 1, pitch: 4 * bpp, slice: 16 * bpp, bytes: texels });
    }
    // a vertex buffer of 12 vertices; its size varies (in-place or new)
    let n = rng.pick(&[12usize, 12, 15]);
    let vb: Vec<[f32; 6]> = (0..n).map(|_| vertex(rng)).collect();
    uploads.push(Upload::Buffer { resource: 0x300, bytes: bytes(&vb) });
    uploads.push(Upload::Buffer { resource: 0x301, bytes: [0u16, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11].iter().flat_map(|i| i.to_le_bytes()).collect() });
    let clear = ClearCall {
        rects: vec![], flags: 7, colour: rng.next(), z: rng.pick(&[0.4f32, 0.7, 1.0]), stencil: rng.next() % 256,
        targets: [Some(back_buffer()), Some(second.clone()), None, None], depth: Some(depth_target()), viewport: [0, 0, W, H, 0, 0], scissor: None, srgb: false,
    };
    let mut clears = vec![clear.clone()];
    if rng.next() % 2 == 0 {
        let r = |rng: &mut Rng| (rng.next() % 8) as i32;
        clears.push(ClearCall {
            flags: rng.pick(&[1u32, 3, 7, 6]), colour: rng.next(), z: rng.unit(), stencil: rng.next() % 256,
            viewport: [rng.next() % 6, rng.next() % 6, 4 + rng.next() % 8, 4 + rng.next() % 8, 0, 0],
            rects: if rng.next() % 2 == 0 { vec![] } else { vec![[r(rng), r(rng), 8 + r(rng), 8 + r(rng)]] },
            scissor: (rng.next() % 2 == 0).then(|| [r(rng), r(rng), 8 + r(rng), 8 + r(rng)]),
            srgb: rng.next() % 2 == 0,
            ..clear
        });
    }
    let mut draws = Vec::new();
    for _ in 0..4 {
        let mrt = rng.next() % 3 == 0;
        let textured = !mrt && rng.next().is_multiple_of(2);
        let tex = rng.next() % 3;
        let mut st = [0u32, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0];
        st[1] = rng.pick(&[1u32, 2, 3]);
        st[2] = rng.pick(&[1u32, 2, 3]);
        st[5] = rng.pick(&[1u32, 2]);
        st[6] = rng.pick(&[1u32, 2]);
        st[11] = rng.next() % 2;
        let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
        c.f[0] = [rng.unit().to_bits(), rng.unit().to_bits(), rng.unit().to_bits(), rng.unit().to_bits()];
        let mut states = vec![
            (7, rng.next() % 2), (14, rng.next() % 2), (23, rng.pick(&[2u32, 4, 5, 8])),
            (15, rng.next() % 2), (25, rng.pick(&[2u32, 5, 7])), (24, rng.next() % 256),
            (27, rng.next() % 2), (19, rng.pick(&[2u32, 5, 6, 9])), (20, rng.pick(&[1u32, 2, 6])), (171, rng.pick(&[1u32, 2, 4])),
            (168, rng.pick(&[0xFu32, 0x7, 0xA])), (22, rng.pick(&[1u32, 2, 3])), (174, rng.next() % 2), (194, rng.next() % 2),
            (52, rng.next() % 2), (56, rng.pick(&[2u32, 4, 8])), (55, rng.pick(&[1u32, 3, 4])), (57, rng.next() % 256),
            (58, 0xFF), (59, 0xFF), (193, rng.next()),
        ];
        states.sort();
        let (primitive, vertices, indices, primitives) = match rng.next() % 4 {
            0 => (4, Vertices::Inline { bytes: bytes(&(0..6).map(|_| vertex(rng)).collect::<Vec<_>>()), stride: 24 }, Indices::None, 2),
            1 => (5, Vertices::Streams(vec![(0, 0x300, 0, 24, 1)]), Indices::None, 3),
            2 => (6, Vertices::Streams(vec![(0, 0x300, 24, 24, 1)]), Indices::None, 3),
            _ => (4, Vertices::Streams(vec![(0, 0x300, 0, 24, 1)]), Indices::Buffer(0x301, 101), 2),
        };
        let vp = [rng.next() % 4, rng.next() % 4, W - 4 + rng.next() % 4, H - 4 + rng.next() % 4];
        // DrawPrimitiveUP data starts at its first vertex
        let start = if matches!(vertices, Vertices::Inline { .. }) { 0 } else { rng.next() % 3 };
        draws.push(DrawCall {
            primitive, primitives, start, base_vertex: 0, min_index: 0, vertex_count: 12,
            vertices, indices,
            vertex_shader: Some(vs()),
            pixel_shader: Some(if mrt { ps_two() } else if textured { ps_texture() } else { ps_constant() }),
            declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]],
            fvf: 0,
            vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() },
            ps_constants: c,
            textures: if textured {
                vec![TextureBinding { sampler: 0, resource: 0x200 + tex, kind: TexKind::Flat, format: formats[tex as usize], width: 4, height: 4, depth: 1, levels: 1, usage: 0, states: st }]
            } else {
                vec![]
            },
            render_states: states,
            viewport: [vp[0], vp[1], vp[2].min(W - vp[0]), vp[3].min(H - vp[1]), 0f32.to_bits(), 1f32.to_bits()],
            scissor: [(rng.next() % 8) as i32, (rng.next() % 8) as i32, 8 + (rng.next() % 8) as i32, 8 + (rng.next() % 8) as i32],
            targets: [Some(back_buffer()), mrt.then(|| second.clone()), None, None],
            depth: Some(depth_target()),
        });
    }
    (uploads, clears, draws, second)
}

fn run(gpu: &mut ddgpu::WgpuBackend, uploads: &[Upload], clears: &[ClearCall], draws: &[DrawCall], second: &Target) -> (Vec<u8>, Vec<u8>) {
    for u in uploads {
        gpu.upload(u.clone());
    }
    for c in clears {
        gpu.clear(c);
    }
    for d in draws {
        gpu.draw(d);
    }
    gpu.present(&back_buffer());
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    (gpu.last_frame.clone().expect("frame").rgba, gpu.read_back(second).expect("second target"))
}

#[test]
fn a_long_lived_backend_draws_every_frame_like_a_fresh_one() {
    let Some(mut shared) = ddgpu::WgpuBackend::headless() else {
        eprintln!("skipped: no adapter");
        return;
    };
    let mut rng = Rng(0xCAC4_E001);
    let mut differ = Vec::new();
    for k in 0..120 {
        let (uploads, clears, draws, second) = frame(&mut rng);
        // the textures and the second target change format: the guest
        // releases them first
        for r in [0x200, 0x201, 0x202, 0x102] {
            shared.release(r);
        }
        let ours = run(&mut shared, &uploads, &clears, &draws, &second);
        let mut fresh = ddgpu::WgpuBackend::headless().unwrap();
        let theirs = run(&mut fresh, &uploads, &clears, &draws, &second);
        if ours != theirs {
            differ.push(k);
        }
    }
    assert!(differ.is_empty(), "frames that differ from a fresh backend: {differ:?}");
}
