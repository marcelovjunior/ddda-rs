//! L137: generated command sequences, no game assets or game execution.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};

const VS: [u32; 17] = [
    // vs_3_0; dcl_position v0; dcl_texcoord v1; dcl_position o0; dcl_texcoord o1
    0xFFFE_0300,
    0x0200_001F, 0x8000_0000, 0x900F_0000,
    0x0200_001F, 0x8000_0005, 0x900F_0001,
    0x0200_001F, 0x8000_0000, 0xE00F_0000,
    0x0200_001F, 0x8000_0005, 0xE00F_0001,
    // mov o0, v0 / mov o1, v1 (below), END
    0x0200_0001, 0xE00F_0000, 0x90E4_0000,
    0x0000_FFFF,
];

fn vs() -> Vec<u32> {
    let mut v = VS[..16].to_vec();
    v.extend_from_slice(&[0x0200_0001, 0xE00F_0001, 0x90E4_0001, 0x0000_FFFF]);
    v
}

/// ps_3_0; dcl_texcoord v0.xy; dcl_2d s0; texld oC0... via r0
fn ps_texture() -> Vec<u32> {
    vec![
        0xFFFF_0300,
        0x0200_001F, 0x8000_0005, 0x9003_0000, // dcl_texcoord v0.xy
        0x0200_001F, 0x9000_0000, 0xA00F_0800, // dcl_2d s0
        0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800, // texld r0, v0, s0
        0x0200_0001, 0x800F_0800, 0x80E4_0000, // mov oC0, r0
        0x0000_FFFF,
    ]
}

fn back_buffer() -> Target {
    Target { surface: 0x100, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
}

/// position float4 at 0, texcoord float2 at 16 (stream 0)
fn declaration() -> Vec<[u8; 8]> {
    vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]]
}

fn render_states() -> Vec<(u32, u32)> {
    // CULLMODE none, ZENABLE off, COLORWRITEENABLE all
    let mut v = vec![(7, 0), (22, 1), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (27, 0), (15, 0)];
    v.sort();
    v
}

/// A triangle covering the whole target, texcoords spanning [0, 2) (the
/// visible part [0, 1)), 24-byte vertices.
fn triangle() -> Vec<u8> {
    let v: [[f32; 6]; 3] = [[-1.0, -1.0, 0.0, 1.0, 0.0, 1.0], [-1.0, 3.0, 0.0, 1.0, 0.0, -1.0], [3.0, -1.0, 0.0, 1.0, 2.0, 1.0]];
    v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

fn draw(ps: Vec<u32>, ps_constants: Constants, textures: Vec<TextureBinding>) -> DrawCall {
    DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Inline { bytes: triangle(), stride: 24 },
        indices: Indices::None,
        vertex_shader: Some((vs()).into()),
        pixel_shader: Some((ps).into()),
        declaration: declaration().into(),
        fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
        ps_constants: ps_constants.into(),
        textures: textures.into(),
        render_states: render_states().into(),
        viewport: [0, 0, 4, 4, 0f32.to_bits(), 1f32.to_bits()],
        scissor: [0, 0, 4, 4],
        targets: [Some(back_buffer()), None, None, None],
        depth: None,
    }
}


fn target(id: u32) -> Target {
    Target { surface: id + 0x1000, format: 116, texture: Some(d3d9cmd::TargetTexture {
        resource: id, kind: TexKind::Flat, width: 4, height: 4, depth: 1,
        levels: 1, usage: d3d9cmd::USAGE_RENDERTARGET,
    }), ..back_buffer() }
}

fn clear(gpu: &mut ddgpu::WgpuBackend, t: &Target, colour: u32) {
    gpu.clear(&ClearCall {
        rects: vec![], flags: 1, colour, z: 1.0, stencil: 0,
        targets: [Some(t.clone()), None, None, None], depth: None,
        viewport: [0, 0, 4, 4, 0, 0], scissor: None, srgb: false,
    });
}

fn binding(t: &Target) -> TextureBinding {
    let mut states = [0; 14];
    for i in [1, 2, 3, 5, 6] { states[i] = 1; }
    TextureBinding { sampler: 0, resource: t.texture.unwrap().resource, kind: TexKind::Flat,
        format: t.format, width: 4, height: 4, depth: 1, levels: 1,
        usage: d3d9cmd::USAGE_RENDERTARGET, states }
}

// Sample previous target, then MAD with distinct constants. Pass order and
// uniform offsets matter: every pixel depends on all preceding passes.
fn feedback_draw(src: &Target, dst: &Target, n: u32) -> DrawCall {
    let mut ps = ps_texture();
    ps.truncate(ps.len() - 4); // replace final mov oC0,r0 and END
    ps.extend([0x0400_0004, 0x800F_0800, 0x80E4_0000, 0xA0E4_0000, 0xA0E4_0001, 0x0000_FFFF]);
    let mut d = draw(ps, Constants { f: vec![
        [0.5f32, 0.25, 0.75, 0.5].map(f32::to_bits),
        [(n % 7) as f32 / 16.0, (n % 11) as f32 / 32.0, 0.0625, 0.5].map(f32::to_bits),
    ], ..Default::default() }, vec![binding(src)]);
    d.targets[0] = Some(dst.clone());
    d
}

fn sequence(gpu: &mut ddgpu::WgpuBackend) -> (Vec<Vec<u8>>, u64) {
    let targets = [target(0x200), target(0x201)];
    let copy = target(0x202);
    gpu.draws_per_submit = 32;
    clear(gpu, &targets[0], 0xFF204080);
    clear(gpu, &targets[1], 0xFF102030);
    let start = gpu.submission_count();
    let mut reads = Vec::new();
    for frame in 0..3 {
        gpu.query_begin(0x900);
        for n in 0..129 {
            let src = &targets[n % 2];
            let dst = &targets[1 - n % 2];
            gpu.draw(&feedback_draw(src, dst, n as u32 + frame * 3));
            if n % 29 == 0 {
                // Copies stay in the same ordered stream across encoder cuts.
                gpu.copy(&d3d9cmd::CopyCall { src: dst.clone(), dst: copy.clone(),
                    src_rect: [0; 4], dst_rect: [0; 4], filter: 1 });
                gpu.copy(&d3d9cmd::CopyCall { src: copy.clone(), dst: dst.clone(),
                    src_rect: [0; 4], dst_rect: [0; 4], filter: 1 });
            }
        }
        gpu.query_end(0x900);
        reads.push(gpu.read_back(&targets[1]).expect("feedback readback"));
        assert_eq!(gpu.query_result(0x900), Some(129 * 16));
        gpu.present(&targets[1]);
    }
    // A previously recorded chunk samples old texels before an in-place write.
    let mut tex = binding(&targets[0]);
    tex.resource = 0x300;
    tex.format = 21;
    tex.usage = 0;
    let upload = |value| Upload::Image { resource: 0x300, index: 0, format: 21,
        width: 4, height: 4, depth: 1, pitch: 16, slice: 64, bytes: vec![value; 64] };
    gpu.upload(upload(64));
    let mut d = draw(ps_texture(), Constants::default(), vec![tex]);
    d.targets[0] = Some(targets[0].clone());
    for n in 0..24 {
        d.targets[0] = Some(targets[n % 2].clone());
        gpu.draw(&d);
    }
    gpu.upload(upload(192));
    d.targets[0] = Some(copy.clone());
    gpu.draw(&d);
    for t in targets.iter().chain([&copy]) { reads.push(gpu.read_back(t).unwrap()); }
    assert_ne!(reads[3], reads[5], "old and new upload must stay distinct");
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    (reads, gpu.submission_count() - start)
}

#[test]
fn recorded_multi_pass_sequence_is_byte_exact_with_one_two_and_four_workers() {
    let mut reference = None;
    for workers in [1, 2, 4] {
        let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter required for encoding oracle");
        gpu.set_encode_threads(workers);
        gpu.set_async_submit(true);
        let result = sequence(&mut gpu);
        if let Some(expected) = &reference {
            assert_eq!(&result, expected, "images/query results/submission count with {workers} workers");
        } else { reference = Some(result); }
        // Drain and replace pools with pending passes too.
        gpu.draw(&feedback_draw(&target(0x200), &target(0x201), 3));
        gpu.set_encode_threads(1);
        gpu.set_async_submit(false);
        assert!(gpu.read_back(&target(0x201)).is_some());
    }
}

/// Synthetic API+finish benchmark; timings with PROFILE=1 include instrumentation.
/// Run separately with DDGPU_ENCODE_THREADS=1/2/4 and --ignored --nocapture.
#[test]
#[ignore = "manual bounded GPU benchmark"]
fn encode_stage_benchmark() {
    let mut gpu = ddgpu::WgpuBackend::headless().expect("adapter");
    gpu.set_async_submit(true);
    gpu.draws_per_submit = 512;
    gpu.read_frames = false;
    let targets = [target(0x200), target(0x201)];
    let draws: Vec<_> = (0..950).map(|n|
        feedback_draw(&targets[n % 2], &targets[1 - n % 2], n as u32)).collect();
    clear(&mut gpu, &targets[0], 0xFF204080);
    clear(&mut gpu, &targets[1], 0xFF102030);
    let mut samples = Vec::new();
    for frame in 0..12 {
        let start = std::time::Instant::now();
        for (n, d) in draws.iter().enumerate() {
            // 864 tiny passes plus 86 larger passes: 5,294 total draws.
            let count = if n < 864 { 1 } else if n == 949 { 95 } else { 51 };
            for _ in 0..count { gpu.draw(d); }
        }
        gpu.present(&targets[0]);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if frame >= 3 { samples.push(ms); }
    }
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    samples.sort_by(f64::total_cmp);
    eprintln!("L137 synthetic 950 target runs / 5294 draws: median={:.3}ms min={:.3}ms max={:.3}ms", samples[4], samples[0], samples[8]);
}
