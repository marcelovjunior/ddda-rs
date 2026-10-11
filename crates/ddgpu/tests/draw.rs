//! The backend draws what D3D9 commands describe. Skips without an adapter.
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

/// ps_3_0; mov oC0, c0
fn ps_constant() -> Vec<u32> {
    vec![0xFFFF_0300, 0x0200_0001, 0x800F_0800, 0xA0E4_0000, 0x0000_FFFF]
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
    Target { surface: 0x100, format: 21, width: 4, height: 4, texture: None, level: 0 }
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
        vertex_shader: Some(vs()),
        pixel_shader: Some(ps),
        declaration: declaration(),
        fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() },
        ps_constants,
        textures,
        render_states: render_states(),
        viewport: [0, 0, 4, 4, 0f32.to_bits(), 1f32.to_bits()],
        scissor: [0, 0, 4, 4],
        targets: [Some(back_buffer()), None, None, None],
        depth: None,
    }
}

#[test]
fn clear_then_a_constant_colour_triangle() {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else {
        eprintln!("skipped: no adapter");
        return;
    };
    gpu.clear(&ClearCall {
        rects: vec![], flags: 1, colour: 0xFF00_00FF, z: 1.0, stencil: 0,
        targets: [Some(back_buffer()), None, None, None], depth: None, viewport: [0, 0, 4, 4, 0, 0],
    });
    gpu.present(&back_buffer());
    let f = gpu.last_frame.clone().expect("frame");
    assert_eq!(&f.rgba[0..4], &[0, 0, 255, 255], "cleared to blue");
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = [1f32.to_bits(), 0.2f32.to_bits(), 0, 1f32.to_bits()];
    gpu.draw(&draw(ps_constant(), c, vec![]));
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    gpu.present(&back_buffer());
    let f = gpu.last_frame.clone().expect("frame");
    assert_eq!((f.width, f.height), (4, 4));
    for px in f.rgba.chunks_exact(4) {
        assert_eq!(px, &[255, 51, 0, 255]);
    }
}

#[test]
fn a_textured_triangle_samples_the_uploaded_level() {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else { return };
    // 2x2 A8R8G8B8 (BGRA bytes): red, green / blue, white
    let texels: Vec<u8> = [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255], [255, 255, 255, 255]].concat();
    gpu.upload(Upload::Image { resource: 0x200, index: 0, format: 21, width: 2, height: 2, depth: 1, pitch: 8, slice: 16, bytes: texels });
    let mut states = [0u32; 14];
    states[1] = 1; // wrap
    states[2] = 1;
    states[3] = 1;
    states[5] = 1; // point
    states[6] = 1;
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat, format: 21, width: 2, height: 2, depth: 1, levels: 1, states };
    gpu.draw(&draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![binding]));
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    gpu.present(&back_buffer());
    let f = gpu.last_frame.clone().unwrap();
    // 4x4 target over texcoords [0,1): each texel covers 2x2 pixels
    let px = |x: usize, y: usize| &f.rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
    assert_eq!(px(0, 0), &[255, 0, 0, 255]);
    assert_eq!(px(3, 0), &[0, 255, 0, 255]);
    assert_eq!(px(0, 3), &[0, 0, 255, 255]);
    assert_eq!(px(3, 3), &[255, 255, 255, 255]);
}

/// The same commands on the host's D3D9 driver and on the wgpu backend
/// produce the same pixels.
fn compare(uploads: &[Upload], clear: &ClearCall, draws: &[DrawCall]) -> (Vec<u8>, Vec<u8>) {
    let mut d3d9 = d3d9ref::Oracle::new().expect("D3D9");
    let reference = d3d9.render(uploads, Some(clear), draws, &back_buffer()).expect("D3D9 render");
    let mut gpu = ddgpu::WgpuBackend::headless().expect("adapter");
    for u in uploads {
        gpu.upload(u.clone());
    }
    gpu.clear(clear);
    for d in draws {
        gpu.draw(d);
    }
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    gpu.present(&back_buffer());
    (gpu.last_frame.unwrap().rgba, reference.rgba)
}

fn blue_clear() -> ClearCall {
    ClearCall { rects: vec![], flags: 1, colour: 0xFF00_00FF, z: 1.0, stencil: 0, targets: [Some(back_buffer()), None, None, None], depth: None, viewport: [0, 0, 4, 4, 0, 0] }
}

#[test]
fn frames_match_the_d3d9_driver() {
    if d3d9ref::Oracle::new().is_none() || ddgpu::WgpuBackend::headless().is_none() {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    }
    // constant colour
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = [0.25f32.to_bits(), 0.75f32.to_bits(), 0.2f32.to_bits(), 1f32.to_bits()];
    let (ours, theirs) = compare(&[], &blue_clear(), &[draw(ps_constant(), c.clone(), vec![])]);
    assert_eq!(ours, theirs, "constant colour");
    // textured
    let texels: Vec<u8> = [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255], [255, 255, 255, 255]].concat();
    let upload = Upload::Image { resource: 0x200, index: 0, format: 21, width: 2, height: 2, depth: 1, pitch: 8, slice: 16, bytes: texels };
    let mut states = [0u32; 14];
    for (k, v) in [(1, 1), (2, 1), (3, 1), (5, 1), (6, 1)] {
        states[k] = v;
    }
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat, format: 21, width: 2, height: 2, depth: 1, levels: 1, states };
    let (ours, theirs) = compare(&[upload], &blue_clear(), &[draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![binding])]);
    assert_eq!(ours, theirs, "textured");
    // alpha blending over the clear colour: SRCALPHA / INVSRCALPHA
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = [1f32.to_bits(), 0.0f32.to_bits(), 0.0f32.to_bits(), 0.25f32.to_bits()];
    let mut d = draw(ps_constant(), c, vec![]);
    d.render_states.extend([(27, 1), (19, 5), (20, 6), (171, 1)]);
    d.render_states.sort();
    d.render_states.dedup_by_key(|s| s.0);
    let (ours, theirs) = compare(&[], &blue_clear(), &[d]);
    let max = ours.iter().zip(&theirs).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
    assert!(max <= 1, "blended: ours {:?} / D3D9 {:?}", &ours[..4], &theirs[..4]);
}
