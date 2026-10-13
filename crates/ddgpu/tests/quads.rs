//! Full-target quad draws as compute dispatches (DDGPU_NATIVE_SIM, L113)
//! give the draw path's results bit for bit. Skips without an adapter.
//! (Its own test binary: it sets DDGPU_NATIVE_SIM between two backends.)
use d3d9cmd::{Constants, DrawCall, GraphicsBackend, Indices, Target, TargetTexture, TexKind, TextureBinding, Upload, Vertices, USAGE_RENDERTARGET};

const F32X4: u32 = 116;
const W: u32 = 8;
const H: u32 = 4;

fn dst(kind: u32, index: u32, mask: u32) -> u32 {
    0x8000_0000 | ((kind & 7) << 28) | ((kind >> 3) << 11) | index | (mask << 16)
}
fn src(kind: u32, index: u32) -> u32 {
    0x8000_0000 | ((kind & 7) << 28) | ((kind >> 3) << 11) | index | (0xE4 << 16)
}
fn ins(op: u32, operands: &[u32]) -> Vec<u32> {
    let mut v = vec![op | ((operands.len() as u32) << 24)];
    v.extend_from_slice(operands);
    v
}
const TEMP: u32 = 0;
const INPUT: u32 = 1;
const CONST: u32 = 2;
const OUTPUT: u32 = 6;
const COLOROUT: u32 = 8;
const SAMPLER: u32 = 10;

/// vs_3_0: o0 = v0, o1 = v1 (position, texcoord)
fn vs() -> Vec<u32> {
    let mut v = vec![0xFFFE_0300];
    v.extend(ins(31, &[0x8000_0000, dst(INPUT, 0, 0xF)]));
    v.extend(ins(31, &[0x8000_0005, dst(INPUT, 1, 0xF)]));
    v.extend(ins(31, &[0x8000_0000, dst(OUTPUT, 0, 0xF)]));
    v.extend(ins(31, &[0x8000_0005, dst(OUTPUT, 1, 0xF)]));
    v.extend(ins(1, &[dst(OUTPUT, 0, 0xF), src(INPUT, 0)]));
    v.extend(ins(1, &[dst(OUTPUT, 1, 0xF), src(INPUT, 1)]));
    v.push(0x0000_FFFF);
    v
}

/// ps_3_0 in the soft-body shape: r1 = v0 * c0 (c0 = 1, 1, 0, 0);
/// r0 = texldl(r1, s0); r2 = texldl(r0, s0) (a dependent fetch);
/// oC0 = r2 * c1 + r0; oC1 = r0 * c2
fn ps() -> Vec<u32> {
    let mut v = vec![0xFFFF_0300];
    v.extend(ins(31, &[0x8000_0005, dst(INPUT, 0, 0x3)]));
    v.extend(ins(31, &[0x9000_0000, dst(SAMPLER, 0, 0xF)]));
    v.extend(ins(5, &[dst(TEMP, 1, 0xF), src(CONST, 0), src(INPUT, 0)]));
    v.extend(ins(95, &[dst(TEMP, 0, 0xF), src(TEMP, 1), src(SAMPLER, 0)]));
    v.extend(ins(95, &[dst(TEMP, 2, 0xF), src(TEMP, 0), src(SAMPLER, 0)]));
    v.extend(ins(4, &[dst(COLOROUT, 0, 0xF), src(TEMP, 2), src(CONST, 1), src(TEMP, 0)]));
    v.extend(ins(5, &[dst(COLOROUT, 1, 0xF), src(TEMP, 0), src(CONST, 2)]));
    v.push(0x0000_FFFF);
    v
}

fn target(resource: u32) -> Target {
    let texture = TargetTexture { resource, kind: TexKind::Flat, width: W, height: H, depth: 1, levels: 1, usage: USAGE_RENDERTARGET };
    Target { surface: resource + 1, format: F32X4, width: W, height: H, texture: Some(texture), level: 0, samples: 0, quality: 0 }
}

fn binding(resource: u32, usage: u32) -> TextureBinding {
    // point filters, clamped, one level (as the game's soft-body passes)
    TextureBinding { sampler: 0, resource, kind: TexKind::Flat, format: F32X4, width: W, height: H, depth: 1, levels: 1, usage, states: [0, 3, 3, 3, 0, 1, 1, 1, 0, 0, 16, 0, 0, 0] }
}

/// The quad as the game draws it: the half-pixel offset in the positions,
/// texcoords 0..1 (position float4, texcoord float2; 24-byte vertices).
fn quad() -> Vec<u8> {
    let (hx, hy) = (1.0 / W as f32, 1.0 / H as f32);
    let v: [[f32; 6]; 4] = [
        [-1.0 - hx, 1.0 + hy, 0.0, 1.0, 0.0, 0.0],
        [1.0 - hx, 1.0 + hy, 0.0, 1.0, 1.0, 0.0],
        [-1.0 - hx, -1.0 + hy, 0.0, 1.0, 0.0, 1.0],
        [1.0 - hx, -1.0 + hy, 0.0, 1.0, 1.0, 1.0],
    ];
    v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

fn draw(input: TextureBinding, out: [u32; 2], blend: bool) -> DrawCall {
    let c = |v: [f32; 4]| v.map(f32::to_bits);
    let mut ps_constants = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    ps_constants.f[0] = c([1.0, 1.0, 0.0, 0.0]);
    ps_constants.f[1] = c([0.5, -2.0, 3.25, 1.0]);
    ps_constants.f[2] = c([1.5, 0.25, -1.0, 2.0]);
    let mut render_states = vec![(7, 0), (15, 0), (22, 1), (27, blend as u32), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF)];
    render_states.sort();
    DrawCall {
        primitive: 5,
        primitives: 2,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 4,
        vertices: Vertices::Inline { bytes: quad(), stride: 24 },
        indices: Indices::None,
        vertex_shader: Some(vs().into()),
        pixel_shader: Some(ps().into()),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]].into(),
        fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
        ps_constants: ps_constants.into(),
        textures: vec![input].into(),
        render_states: render_states.into(),
        viewport: [0, 0, W, H, 0f32.to_bits(), 1f32.to_bits()],
        scissor: [0, 0, W as i32, H as i32],
        targets: [Some(target(out[0])), Some(target(out[1])), None, None],
        depth: None,
    }
}

#[test]
fn quad_dispatches_match_the_draw_path() {
    std::env::remove_var("DDGPU_NATIVE_SIM");
    let Some(mut a) = ddgpu::WgpuBackend::headless() else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    std::env::set_var("DDGPU_NATIVE_SIM", "1");
    let mut b = ddgpu::WgpuBackend::headless().expect("a second backend");
    std::env::remove_var("DDGPU_NATIVE_SIM");
    // the input: coordinates in [0, 1) and values that exercise the arithmetic
    let mut x = 0x1234_5678u32;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    let mut bytes = Vec::new();
    for k in 0..W * H * 4 {
        let v = if k % 4 < 2 { (next() % 1000) as f32 / 1000.0 } else { (next() as i32 as f32) / 1.0e6 };
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let upload = Upload::Image { resource: 0x400, index: 0, format: F32X4, width: W, height: H, depth: 1, pitch: W * 16, slice: W * H * 16, bytes };
    let draws = [
        draw(binding(0x400, 0), [0x200, 0x300], false),
        // the next pass reads the first one's target (ping-pong)
        draw(binding(0x200, USAGE_RENDERTARGET), [0x500, 0x600], false),
        // blending on: not a dispatch
        draw(binding(0x500, USAGE_RENDERTARGET), [0x700, 0x800], true),
    ];
    for gpu in [&mut a, &mut b] {
        gpu.upload(upload.clone());
        for d in &draws {
            gpu.draw(d);
        }
        assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    }
    assert_eq!(a.quad_dispatches(), 0);
    assert_eq!(b.quad_dispatches(), 2, "the two eligible draws, not the blended one");
    for r in [0x200, 0x300, 0x500, 0x600, 0x700, 0x800] {
        let (x, y) = (a.read_back(&target(r)).expect("read back"), b.read_back(&target(r)).expect("read back"));
        assert_eq!(x, y, "target {r:#x}");
    }
}
