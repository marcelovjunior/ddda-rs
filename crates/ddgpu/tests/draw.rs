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
        textures,
        render_states: render_states().into(),
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
        targets: [Some(back_buffer()), None, None, None], depth: None, viewport: [0, 0, 4, 4, 0, 0], scissor: None, srgb: false,
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
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat, format: 21, width: 2, height: 2, depth: 1, levels: 1, usage: 0, states };
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
    ClearCall { rects: vec![], flags: 1, colour: 0xFF00_00FF, z: 1.0, stencil: 0, targets: [Some(back_buffer()), None, None, None], depth: None, viewport: [0, 0, 4, 4, 0, 0], scissor: None, srgb: false }
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
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat, format: 21, width: 2, height: 2, depth: 1, levels: 1, usage: 0, states };
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

/// xorshift32
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.next() as usize) % v.len()]
    }
}

/// Random texel bytes of a 4x4 texture in `format`, with its pitch.
fn texels(rng: &mut Rng, format: u32) -> (Vec<u8>, u32) {
    let dxt = |c: &[u8; 4]| u32::from_le_bytes(*c);
    let (bytes, pitch) = if format == dxt(b"DXT1") {
        (8, 8)
    } else if format == dxt(b"DXT3") || format == dxt(b"DXT5") {
        (16, 16)
    } else {
        let bpp = match format { 21 | 22 | 32 | 33 => 4, 23 | 24 | 25 | 26 | 51 => 2, _ => 1 };
        (16 * bpp, 4 * bpp)
    };
    ((0..bytes).map(|_| rng.next() as u8).collect(), pitch)
}

#[test]
fn random_states_and_formats_match_the_d3d9_driver() {
    let (Some(mut d3d9), Some(_)) = (d3d9ref::Oracle::new(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let cases: usize = std::env::var("DDGPU_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let formats = [21u32, 22, 23, 24, 25, 26, 50, 51, 28, 32, 33, u32::from_le_bytes(*b"DXT1"), u32::from_le_bytes(*b"DXT3"), u32::from_le_bytes(*b"DXT5")];
    let mut rng = Rng(0xC0FF_EE11);
    let mut failures = Vec::new();
    let mut unsupported = std::collections::BTreeSet::new();
    for case in 0..cases {
        let format = rng.pick(&formats);
        let (bytes, pitch) = texels(&mut rng, format);
        let dxt = pitch >= 8 && (format > 255);
        let upload = Upload::Image { resource: 0x300, index: 0, format, width: 4, height: 4, depth: 1, pitch, slice: if dxt { pitch } else { pitch * 4 }, bytes };
        let mut states = [0u32; 14];
        let address = rng.pick(&[1u32, 2, 3]);
        let filter = rng.pick(&[1u32, 2]);
        for (k, v) in [(1, address), (2, address), (3, address), (5, filter), (6, filter)] {
            states[k] = v;
        }
        let binding = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 1, usage: 0, states };
        let mut d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![binding]);
        // random fixed-function state
        let blend = rng.next() % 2;
        let src = rng.pick(&[1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let dst = rng.pick(&[1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let op = rng.pick(&[1u32, 2, 3, 4, 5]);
        let alpha_test = rng.next() % 2;
        let alpha_func = rng.pick(&[1u32, 2, 3, 4, 5, 6, 7, 8]);
        let alpha_ref = rng.next() % 256;
        let mask = rng.next() % 16;
        let cull = rng.pick(&[1u32, 2, 3]);
        d.render_states.retain(|s| ![22, 27, 168].contains(&s.0));
        d.render_states.extend([(22, cull), (27, blend), (19, src), (20, dst), (171, op), (15, alpha_test), (25, alpha_func), (24, alpha_ref), (168, mask)]);
        d.render_states.sort();
        // random winding: swap two vertices half the time (CW vs CCW)
        if rng.next() % 2 == 1 {
            if let Vertices::Inline { bytes, .. } = &mut d.vertices {
                let (a, b) = bytes.split_at_mut(24);
                a.swap_with_slice(&mut b[..24]);
            }
        }
        let clear = ClearCall { colour: rng.next(), ..blue_clear() };
        let theirs = match d3d9.render(&[upload.clone()], Some(&clear), &[d.clone()], &back_buffer()) {
            Ok(f) => f.rgba,
            Err(e) if e.contains("CreateTexture") => {
                // the driver does not offer this format as a texture
                unsupported.insert(format);
                continue;
            }
            Err(e) => panic!("D3D9: {e}"),
        };
        let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
        gpu.upload(upload);
        gpu.clear(&clear);
        gpu.draw(&d);
        gpu.present(&back_buffer());
        if !gpu.errors.is_empty() {
            failures.push(format!("case {case}: backend error {:?}", gpu.errors));
            continue;
        }
        let ours = gpu.last_frame.unwrap().rgba;
        let max = ours.iter().zip(&theirs).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        if max > 1 {
            failures.push(format!("case {case}: format {format:#x} address {address} filter {filter} blend {blend} ({src},{dst},op {op}) alpha test {alpha_test} (func {alpha_func}, ref {alpha_ref}) mask {mask:#x} cull {cull}: max diff {max}, ours {:?} / D3D9 {:?}", &ours[..8], &theirs[..8]));
        }
    }
    eprintln!("{} of {cases} cases differ; formats the driver refuses as textures: {unsupported:x?}", failures.len());
    for f in failures.iter().take(12) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn copies_fills_and_read_back() {
    use d3d9cmd::{CopyCall, FillCall};
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else { return };
    // source: the textured draw's target; destination: another 4x4 target
    let texels: Vec<u8> = [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255], [255, 255, 255, 255]].concat();
    gpu.upload(Upload::Image { resource: 0x200, index: 0, format: 21, width: 2, height: 2, depth: 1, pitch: 8, slice: 16, bytes: texels });
    let mut states = [0u32; 14];
    for (k, v) in [(1, 1), (2, 1), (3, 1), (5, 1), (6, 1)] {
        states[k] = v;
    }
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat, format: 21, width: 2, height: 2, depth: 1, levels: 1, usage: 0, states };
    gpu.draw(&draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![binding]));
    let other = Target { surface: 0x101, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 };
    // a same-size copy is exact
    gpu.copy(&CopyCall { src: back_buffer(), src_rect: [0; 4], dst: other.clone(), dst_rect: [0; 4], filter: 1 });
    let a = gpu.read_back(&back_buffer()).unwrap();
    let b = gpu.read_back(&other).unwrap();
    assert_eq!(a, b);
    assert_eq!(&a[0..4], &[0, 0, 255, 255], "D3D9 A8R8G8B8 byte order (B, G, R, A): red");
    // a fill of the top-left 2x2 rectangle
    gpu.fill(&FillCall { dst: other.clone(), rect: [0, 0, 2, 2], colour: 0xFF20_4060 });
    let b = gpu.read_back(&other).unwrap();
    assert_eq!(&b[0..4], &[0x60, 0x40, 0x20, 0xFF]);
    assert_eq!(&b[(2 * 4 + 2) * 4..(2 * 4 + 2) * 4 + 4], &a[(2 * 4 + 2) * 4..(2 * 4 + 2) * 4 + 4], "outside the rectangle unchanged");
    // a scaled copy (point): the 4x4 source's top-left 2x2 into the whole 4x4 target
    gpu.copy(&CopyCall { src: back_buffer(), src_rect: [0, 0, 2, 2], dst: other.clone(), dst_rect: [0; 4], filter: 1 });
    let b = gpu.read_back(&other).unwrap();
    assert!(b.chunks_exact(4).all(|px| px == [0, 0, 255, 255]), "the top-left 2x2 of the source is all red");
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
}

fn depth_target() -> Target {
    Target { surface: 0x180, format: 75, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
}

/// The full-screen triangle at depth `z`.
fn triangle_at(z: f32) -> Vec<u8> {
    let v: [[f32; 6]; 3] = [[-1.0, -1.0, z, 1.0, 0.0, 1.0], [-1.0, 3.0, z, 1.0, 0.0, -1.0], [3.0, -1.0, z, 1.0, 2.0, 1.0]];
    v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

/// A constant-colour draw at depth `z` against the depth target (ZFUNC
/// LESS, depth writes on), with `states` on top.
fn depth_draw(z: f32, alpha: f32, states: &[(u32, u32)]) -> DrawCall {
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = [1f32.to_bits(), 0.5f32.to_bits(), 0, alpha.to_bits()];
    let mut d = draw(ps_constant(), c, vec![]);
    d.vertices = Vertices::Inline { bytes: triangle_at(z), stride: 24 };
    d.depth = Some(depth_target());
    let mut all: Vec<(u32, u32)> = vec![(7, 1), (14, 1), (23, 2)];
    all.extend_from_slice(states);
    for (k, v) in all {
        d.render_states.retain(|s| s.0 != k);
        d.render_states.push((k, v));
    }
    d.render_states.sort();
    d
}

#[test]
fn occlusion_query_states_on_the_d3d9_driver() {
    let Some(mut d3d9) = d3d9ref::Oracle::new() else {
        eprintln!("skipped: no D3D9 device");
        return;
    };
    let [before, left, building, after, count] = d3d9.occlusion_states().expect("D3D9 queries");
    eprintln!("GetData before Issue {before:#x} (buffer {left:#x}), between BEGIN and END {building:#x}, after END {after:#x} (count {count})");
    // the runtime's answers (crates/ddruntime/src/d3d9copy.rs) follow these
    assert_eq!(building, 1, "S_FALSE while counting");
    assert_eq!((after, count), (0, 0), "an ended query with no draws counts 0");
}

/// Issue occlusion queries around `draws` like the oracle does: query k
/// begins before draw spans[k].start and ends after draw spans[k].end - 1.
fn counted(gpu: &mut ddgpu::WgpuBackend, draws: &[DrawCall], spans: &[std::ops::Range<usize>]) -> Vec<u32> {
    let ids: Vec<u32> = (0..spans.len() as u32).map(|k| 0x900 + k).collect();
    let issue = |gpu: &mut ddgpu::WgpuBackend, at: usize, end: bool| {
        for (k, s) in spans.iter().enumerate() {
            if end && s.end == at && s.start != at {
                gpu.query_end(ids[k]);
            } else if !end && s.start == at {
                gpu.query_begin(ids[k]);
                if s.end == at {
                    gpu.query_end(ids[k]);
                }
            }
        }
    };
    for (k, d) in draws.iter().enumerate() {
        issue(gpu, k, false);
        gpu.draw(d);
        issue(gpu, k + 1, true);
    }
    issue(gpu, draws.len(), false);
    ids.iter().map(|&q| gpu.query_result(q).expect("count") as u32).collect()
}

#[test]
fn occlusion_counts_match_the_d3d9_driver() {
    let (Some(mut d3d9), Some(mut gpu)) = (d3d9ref::Oracle::new(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let clear = ClearCall { flags: 1 | 2 | 4, z: 0.5, depth: Some(depth_target()), ..blue_clear() };
    let mut draws = vec![
        depth_draw(0.25, 1.0, &[]),                             // in front of the cleared depth
        depth_draw(0.75, 1.0, &[]),                             // behind
        depth_draw(0.75, 1.0, &[(7, 0)]),                       // depth test off
        depth_draw(0.1, 0.25, &[(15, 1), (25, 7), (24, 128)]),  // alpha test GREATEREQUAL 128 kills it
        depth_draw(0.1, 1.0, &[(22, 2)]),                       // culled (CULL_CW)
        depth_draw(0.2, 1.0, &[(14, 0), (174, 1)]),             // scissor (below)
    ];
    draws[5].scissor = [0, 0, 2, 4];
    // one query at a time, as the game issues them (FUN_011481B0 /
    // FUN_011481F0); overlapping queries are not counted independently by
    // the driver (a query spanning all six draws while the others ran
    // counted only the last draw), so they are not compared
    let spans = vec![0..1, 1..2, 2..3, 3..4, 4..5, 5..6, 6..6];
    let (_, theirs) = d3d9.render_counted(&[], Some(&clear), &draws, &back_buffer(), &spans).expect("D3D9 render");
    gpu.clear(&clear);
    let ours = counted(&mut gpu, &draws, &spans);
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    eprintln!("occlusion counts: D3D9 {theirs:?}, wgpu {ours:?}");
    assert_eq!(ours, theirs, "occlusion counts (wgpu / D3D9)");
}

/// vs_3_0: o0 = v0 + v1 (v1: texcoord1, the per-instance offset; one
/// input register per instruction, so through r0)
fn vs_offset() -> Vec<u32> {
    vec![
        0xFFFE_0300,
        0x0200_001F, 0x8000_0000, 0x900F_0000, // dcl_position v0
        0x0200_001F, 0x8001_0005, 0x900F_0001, // dcl_texcoord1 v1
        0x0200_001F, 0x8000_0000, 0xE00F_0000, // dcl_position o0
        0x0200_0001, 0x800F_0000, 0x90E4_0001, // mov r0, v1
        0x0300_0002, 0xE00F_0000, 0x90E4_0000, 0x80E4_0000, // add o0, v0, r0
        0x0000_FFFF,
    ]
}

fn floats(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Instanced draws (SetStreamSourceFreq, as the game's draw commands 2 and
/// 3 make them): three instances of a one-pixel triangle offset per
/// instance, indexed with a base vertex, and non-indexed (which D3D9 draws
/// once) from vertex 3 and from vertex 0.
#[test]
fn instanced_draws_match_the_d3d9_driver() {
    let (Some(mut d3d9), Some(_)) = (d3d9ref::Oracle::new(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    // a triangle covering pixel (0, 0); buffer 0x400 has three unused
    // vertices before it
    let triangle = floats(&[-1.0, 1.0, 0.0, 1.0, -0.4, 1.0, 0.0, 1.0, -1.0, 0.4, 0.0, 1.0]);
    let mut geometry = floats(&[9.0, 9.0, 0.0, 1.0].repeat(3));
    geometry.extend_from_slice(&triangle);
    // per instance: clip-space offsets of 2 pixels right, 0, 3 right and 1 down
    let offsets = floats(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, -0.5, 0.0, 0.0]);
    let indices: Vec<u8> = [0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()).collect();
    let uploads = vec![
        Upload::Buffer { resource: 0x400, bytes: geometry },
        Upload::Buffer { resource: 0x401, bytes: offsets },
        Upload::Buffer { resource: 0x402, bytes: indices },
        Upload::Buffer { resource: 0x403, bytes: triangle },
    ];
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = [1f32.to_bits(), 1f32.to_bits(), 0, 1f32.to_bits()];
    for (indexed, start) in [(false, 0), (true, 3), (false, 3)] {
        let mut d = draw(ps_constant(), c.clone(), vec![]);
        d.vertex_shader = Some((vs_offset()).into());
        d.declaration = vec![[0, 0, 0, 0, 3, 0, 0, 0], [1, 0, 0, 0, 3, 0, 5, 1]].into();
        let geometry = if start == 0 { 0x403 } else { 0x400 };
        d.vertices = Vertices::Streams(vec![(0, geometry, 0, 16, 0x4000_0003), (1, 0x401, 0, 16, 0x8000_0001)]);
        if indexed {
            d.indices = Indices::Buffer(0x402, 101);
            d.base_vertex = start as i32;
        } else {
            d.start = start;
        }
        let theirs = d3d9.render(&uploads, Some(&blue_clear()), std::slice::from_ref(&d), &back_buffer()).expect("D3D9 render").rgba;
        let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
        for u in &uploads {
            gpu.upload(u.clone());
        }
        gpu.clear(&blue_clear());
        gpu.draw(&d);
        gpu.present(&back_buffer());
        assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
        let ours = gpu.last_frame.unwrap().rgba;
        let lit = |f: &[u8]| f.chunks_exact(4).enumerate().filter(|(_, p)| p[2] == 0).map(|(k, _)| k).collect::<Vec<_>>();
        eprintln!("indexed {indexed} from {start}: D3D9 lit pixels {:?}, ours {:?}", lit(&theirs), lit(&ours));
        assert_eq!(ours, theirs, "instanced draw, indexed {indexed}, from {start}");
    }
}

#[test]
fn a_gamma_ramp_maps_presented_frames() {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else { return };
    gpu.clear(&ClearCall { colour: 0xFF10_8040, ..blue_clear() });
    let mut ramp = [[0u16; 256]; 3];
    for channel in ramp.iter_mut() {
        for (v, e) in channel.iter_mut().enumerate() {
            *e = ((255 - v) as u16) << 8 | 0xFF;
        }
    }
    gpu.gamma(Some(&ramp));
    gpu.present(&back_buffer());
    assert_eq!(&gpu.last_frame.as_ref().unwrap().rgba[0..4], &[0xEF, 0x7F, 0xBF, 0xFF], "inverted");
    gpu.gamma(None);
    gpu.present(&back_buffer());
    assert_eq!(&gpu.last_frame.as_ref().unwrap().rgba[0..4], &[0x10, 0x80, 0x40, 0xFF], "identity");
}

#[test]
fn a_released_surface_address_can_be_reused() {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else { return };
    gpu.clear(&blue_clear());
    gpu.release(0x100);
    let bigger = Target { width: 8, height: 2, ..back_buffer() };
    gpu.clear(&ClearCall { targets: [Some(bigger.clone()), None, None, None], ..blue_clear() });
    gpu.present(&bigger);
    let f = gpu.last_frame.unwrap();
    assert_eq!((f.width, f.height), (8, 2));
}

/// An IEEE half from an f32 in [0, 1] (round to nearest).
fn half(v: f32) -> u16 {
    if v == 0.0 {
        return 0;
    }
    let b = v.to_bits();
    let e = ((b >> 23) & 0xFF) as i32 - 127 + 15;
    if e <= 0 {
        return ((v * 16_777_216.0).round() as u32) as u16; // subnormal: v / 2^-24
    }
    let m = b & 0x7F_FFFF;
    let r = ((e as u32) << 10) + (m >> 13) + ((m >> 12) & 1);
    r as u16
}

/// Random texels of one `w` x `h` level of `format` (float formats in
/// [0, 1]), with the level's pitch and the bytes of one block row.
fn level_texels(rng: &mut Rng, format: u32, w: u32, h: u32) -> (Vec<u8>, u32) {
    let dxt1 = u32::from_le_bytes(*b"DXT1");
    if format > 255 {
        let block = if format == dxt1 { 8 } else { 16 };
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        return ((0..block * bw * bh).map(|_| rng.next() as u8).collect(), block * bw);
    }
    let unit = |rng: &mut Rng| (rng.next() % 1001) as f32 / 1000.0;
    let mut bytes = Vec::new();
    for _ in 0..w * h {
        match format {
            114 => bytes.extend(unit(rng).to_le_bytes()),
            115 => (0..2).for_each(|_| bytes.extend(unit(rng).to_le_bytes())),
            116 => (0..4).for_each(|_| bytes.extend(unit(rng).to_le_bytes())),
            111 => bytes.extend(half(unit(rng)).to_le_bytes()),
            112 => (0..2).for_each(|_| bytes.extend(half(unit(rng)).to_le_bytes())),
            113 => (0..4).for_each(|_| bytes.extend(half(unit(rng)).to_le_bytes())),
            _ => {
                let bpp = match format {
                    21 | 22 | 32 | 33 | 31 | 34 | 63 | 64 => 4,
                    23 | 24 | 25 | 26 | 51 | 81 => 2,
                    36 | 110 => 8,
                    _ => 1,
                };
                bytes.extend((0..bpp).map(|_| rng.next() as u8));
            }
        }
    }
    let pitch = bytes.len() as u32 / h;
    (bytes, pitch)
}

/// Every render state, sampler state and texture format of the census
/// (brief L20 Results 1) at random, compared with the D3D9 driver: a
/// textured draw with random blend, write-mask, alpha-test, cull,
/// scissor, sRGB, depth (function, writes, bias) and stencil (one- and
/// two-sided) states over a D24S8 target, then a probe draw whose depth
/// and stencil tests show what the first draw left in that target.
/// DDGPU_FUZZ sets the number of cases.
#[test]
fn render_states_match_the_d3d9_driver() {
    let (Some(mut d3d9), Some(probe_gpu)) = (d3d9ref::Oracle::new(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    drop(probe_gpu);
    let cases: usize = std::env::var("DDGPU_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    let fourcc = |s: &[u8; 4]| u32::from_le_bytes(*s);
    let formats = [21u32, 22, 23, 24, 25, 26, 28, 31, 32, 33, 34, 36, 50, 51, 63, 64, 81, 110, 111, 112, 113, 114, 115, 116,
                   fourcc(b"DXT1"), fourcc(b"DXT3"), fourcc(b"DXT5")];
    let mut rng = Rng(0x5EED_1020);
    let mut failures = Vec::new();
    let mut unsupported = std::collections::BTreeSet::new();
    let mut compared = 0;
    for case in 0..cases {
        // one group of states at random per case, the others neutral, so
        // that a difference points at its group
        let group = case % 6;
        let names = ["texture and sampler", "blend", "alpha test, cull, scissor", "depth", "stencil", "sRGB write"];
        // texture: 4x4 with three levels
        let format = if group == 0 { rng.pick(&formats) } else { 21 };
        let mut uploads = Vec::new();
        for (level, size) in [4u32, 2, 1].into_iter().enumerate() {
            let (bytes, pitch) = level_texels(&mut rng, format, size, size);
            let rows = if format > 255 { size.div_ceil(4) } else { size };
            uploads.push(Upload::Image { resource: 0x300, index: level as u32, format, width: size, height: size, depth: 1, pitch, slice: pitch * rows, bytes });
        }
        // neutral sampler: wrap, point, no mips, no bias
        let mut st = [0u32, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0];
        if group == 0 {
            st[1] = rng.pick(&[1u32, 2, 3, 4]);
            st[2] = rng.pick(&[1u32, 2, 3, 4]);
            st[4] = rng.pick(&[0u32, 0xFF00_0000, 0xFFFF_FFFF]);
            st[5] = rng.pick(&[1u32, 2]);
            st[6] = rng.pick(&[1u32, 2, 3]);
            st[7] = rng.pick(&[0u32, 1, 2]);
            // integer biases: at a fractional level of detail the NVIDIA
            // D3D9 and Vulkan drivers blend mip levels with different
            // weights (trilinear optimisation; explore_d3d9_semantics)
            st[8] = rng.pick(&[0f32, 1.0, -1.0, 2.0]).to_bits();
            st[9] = rng.pick(&[0u32, 1, 2]);
            st[10] = rng.pick(&[1u32, 2, 4, 16]);
            st[11] = rng.next() % 2;
            // formats with missing channels: D3D9 borders only the channels
            // the format has (the others read 1); wgpu's border colours
            // cannot express (0, 1, 1, 1), so these get the white border
            if [22u32, 23, 24, 28, 33, 34, 50, 51, 64, 81, 111, 112, 114, 115].contains(&format) {
                st[4] = 0xFFFF_FFFF;
            }
            // anisotropic filtering takes implementation-specific samples:
            // MINFILTER ANISOTROPIC is compared at MAXANISOTROPY 1 (linear)
            if st[6] == 3 {
                st[10] = 1;
            }
        }
        let binding = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 3, usage: 0, states: st };
        // the draw
        let mut d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![binding]);
        let z = if group == 3 { rng.pick(&[0.2f32, 0.5, 0.8]) } else { 0.5 };
        d.vertices = Vertices::Inline { bytes: triangle_at(z), stride: 24 };
        if matches!(group, 2 | 4) && rng.next() % 2 == 1 {
            if let Vertices::Inline { bytes, .. } = &mut d.vertices {
                let (a, b) = bytes.split_at_mut(24);
                a.swap_with_slice(&mut b[..24]);
            }
        }
        d.depth = Some(depth_target());
        let blends = [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14, 15];
        let stencil_ops = [1u32, 2, 3, 4, 5, 6, 7, 8];
        let funcs = [1u32, 2, 3, 4, 5, 6, 7, 8];
        let masks = [0xFFu32, 0x0F, 0xF0, rng.next() % 256];
        // neutral: no depth, stencil, blend, alpha test, culling, scissor, sRGB
        let mut states: Vec<(u32, u32)> = vec![
            (7, 0), (14, 0), (23, 4), (15, 0), (25, 8), (24, 0), (27, 0), (19, 2), (20, 1), (171, 1), (206, 0), (207, 2),
            (208, 1), (209, 1), (193, 0xFFFF_FFFF), (168, 0xF), (22, 1), (174, 0), (194, 0), (195, 0), (175, 0), (52, 0),
            (56, 8), (53, 1), (54, 1), (55, 1), (57, 0), (58, 0xFF), (59, 0xFF), (185, 0), (189, 8), (186, 1), (187, 1), (188, 1),
        ];
        let mut set = |k: u32, v: u32| {
            states.retain(|s| s.0 != k);
            states.push((k, v));
        };
        match group {
            1 | 5 => {
                set(27, rng.next() % 2);
                set(19, rng.pick(&blends));
                set(20, rng.pick(&blends));
                set(171, rng.pick(&[1u32, 2, 3, 4, 5]));
                set(206, rng.next() % 2);
                set(207, rng.pick(&blends));
                set(208, rng.pick(&blends));
                set(209, rng.pick(&[1u32, 2, 3, 4, 5]));
                set(193, rng.next());
                set(168, rng.next() % 16);
                if group == 5 {
                    set(194, 1);
                }
            }
            2 => {
                set(15, rng.next() % 2);
                set(25, rng.pick(&funcs));
                set(24, rng.next() % 256);
                set(22, rng.pick(&[1u32, 2, 3]));
                set(174, rng.next() % 2);
            }
            3 => {
                set(7, rng.next() % 2);
                set(14, rng.next() % 2);
                set(23, rng.pick(&funcs));
                set(195, rng.pick(&[0f32, 0.25, -0.25]).to_bits());
                set(175, rng.pick(&[0f32, 1.0]).to_bits());
            }
            4 => {
                set(52, 1);
                set(56, rng.pick(&funcs));
                set(53, rng.pick(&stencil_ops));
                set(54, rng.pick(&stencil_ops));
                set(55, rng.pick(&stencil_ops));
                set(57, rng.next() % 256);
                set(58, rng.pick(&masks));
                set(59, rng.pick(&masks));
                set(185, rng.next() % 2);
                set(189, rng.pick(&funcs));
                set(186, rng.pick(&stencil_ops));
                set(187, rng.pick(&stencil_ops));
                set(188, rng.pick(&stencil_ops));
                // half the time the depth test fails, for the ZFAIL ops
                set(7, rng.next() % 2);
                set(23, 2);
            }
            _ => {}
        }
        for &(k, v) in &states {
            d.render_states.retain(|s| s.0 != k);
            d.render_states.push((k, v));
        }
        d.render_states.sort();
        let x0 = (rng.next() % 3) as i32;
        let y0 = (rng.next() % 3) as i32;
        d.scissor = [x0, y0, x0 + 1 + (rng.next() % (4 - x0 as u32)) as i32, y0 + 1 + (rng.next() % (4 - y0 as u32)) as i32];
        // the probe: depth LESS at a random depth, stencil LESS (ref < value)
        let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
        c.f[0] = [0.1f32.to_bits(), 0.9f32.to_bits(), 0.4f32.to_bits(), 1f32.to_bits()];
        let mut probe = draw(ps_constant(), c, vec![]);
        probe.vertices = Vertices::Inline { bytes: triangle_at(rng.pick(&[0.3f32, 0.6, 0.9])), stride: 24 };
        probe.depth = Some(depth_target());
        // every state the draw set (the D3D9 device keeps them), at the
        // probe's values
        probe.render_states = d.render_states.clone();
        for (k, v) in [(7, 1), (14, 0), (23, 2), (52, 1), (56, 2), (57, rng.next() % 256), (58, 0xFF), (59, 0xFF), (53, 1), (54, 1), (55, 1),
                       (185, 0), (15, 0), (27, 0), (168, 0xF), (22, 1), (174, 0), (194, 0), (195, 0), (175, 0)] {
            probe.render_states.retain(|s| s.0 != k);
            probe.render_states.push((k, v));
        }
        probe.render_states.sort();
        let clear = ClearCall {
            flags: 7, colour: rng.next(), z: rng.pick(&[0.35f32, 0.65]), stencil: rng.next() % 256,
            depth: Some(depth_target()), ..blue_clear()
        };
        let draws = [d.clone(), probe];
        let theirs = match d3d9.render(&uploads, Some(&clear), &draws, &back_buffer()) {
            Ok(f) => f.rgba,
            Err(e) if e.contains("CreateTexture") => {
                unsupported.insert(format);
                continue;
            }
            Err(e) => panic!("D3D9: {e}"),
        };
        compared += 1;
        let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
        for u in &uploads {
            gpu.upload(u.clone());
        }
        gpu.clear(&clear);
        for dr in &draws {
            gpu.draw(dr);
        }
        gpu.present(&back_buffer());
        let changed: Vec<(u32, u32)> = states.iter().copied().filter(|s| ![(7, 0), (14, 0), (23, 4), (15, 0), (25, 8), (24, 0), (27, 0), (19, 2), (20, 1), (171, 1), (206, 0), (207, 2),
            (208, 1), (209, 1), (193, 0xFFFF_FFFF), (168, 0xF), (22, 1), (174, 0), (194, 0), (195, 0), (175, 0), (52, 0),
            (56, 8), (53, 1), (54, 1), (55, 1), (57, 0), (58, 0xFF), (59, 0xFF), (185, 0), (189, 8), (186, 1), (187, 1), (188, 1)].contains(s)).collect();
        let describe = || format!("[{}] format {format:#x} sampler {:?} states {:?} scissor {:?} z {z} clear z {} stencil {}",
                                  names[group], &st[1..12], changed, d.scissor, clear.z, clear.stencil);
        if !gpu.errors.is_empty() {
            failures.push(format!("case {case}: backend error {:?}; {}", gpu.errors, describe()));
            continue;
        }
        let ours = gpu.last_frame.unwrap().rgba;
        let max = ours.iter().zip(&theirs).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
        // sRGB reads of 16-bit packed formats: D3D9 decodes the 4/5/6-bit
        // value, the backend the value widened to 8 bits (off by up to 2)
        let tolerance = if st[11] != 0 && [23u32, 24, 25, 26].contains(&format) { 2 } else { 1 };
        if max > tolerance {
            failures.push(format!("case {case}: max diff {max}, ours {:?} / D3D9 {:?}; {}", &ours[..16], &theirs[..16], describe()));
        }
    }
    eprintln!("{} of {compared} compared cases differ; formats the driver refuses as textures: {unsupported:x?}", failures.len());
    for f in failures.iter().take(40) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty());
}

/// One set of draws on both, the top-left pixel of each (D3D9, wgpu,
/// backend errors).
fn both(d3d9: &mut d3d9ref::Oracle, uploads: &[Upload], clear: &ClearCall, draws: &[DrawCall]) -> Result<([u8; 4], [u8; 4], Vec<String>), String> {
    let theirs = d3d9.render(uploads, Some(clear), draws, &back_buffer())?.rgba;
    let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
    for u in uploads {
        gpu.upload(u.clone());
    }
    gpu.clear(clear);
    for d in draws {
        gpu.draw(d);
    }
    gpu.present(&back_buffer());
    let ours = gpu.last_frame.map(|f| f.rgba).unwrap_or_default();
    Ok((theirs[..4].try_into().unwrap(), ours.get(..4).map(|p| p.try_into().unwrap()).unwrap_or([0; 4]), gpu.errors))
}

/// The alpha test compares the shader's alpha rounded to 8 bits; point and
/// solid fill modes rasterise like the driver (wireframe does not: D3D9's
/// line rules differ from the Vulkan driver's, see the brief's Results).
#[test]
fn alpha_rounding_and_fill_modes_match_the_d3d9_driver() {
    let (Some(mut d3d9), Some(_)) = (d3d9ref::Oracle::new(), ddgpu::WgpuBackend::headless()) else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let k = 48f32 / 255.0;
    for alpha in [k, f32::from_bits(k.to_bits() - 1), f32::from_bits(k.to_bits() + 1), 47.6 / 255.0, 47.4 / 255.0, 48.4 / 255.0, 48.6 / 255.0] {
        for func in [2u32, 3, 4, 5, 6, 7] {
            let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
            c.f[0] = [1f32.to_bits(), 0, 0, alpha.to_bits()];
            let mut d = draw(ps_constant(), c, vec![]);
            d.render_states.retain(|s| ![15, 25, 24].contains(&s.0));
            d.render_states.extend([(15, 1), (25, func), (24, 48)]);
            d.render_states.sort();
            let (theirs, ours, errors) = both(&mut d3d9, &[], &blue_clear(), &[d]).unwrap();
            assert!(errors.is_empty(), "{errors:?}");
            assert_eq!(ours, theirs, "alpha {} func {func}", alpha * 255.0);
        }
    }
    for fill in [1u32, 3] {
        let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
        c.f[0] = [1f32.to_bits(), 1f32.to_bits(), 0, 1f32.to_bits()];
        let mut d = draw(ps_constant(), c, vec![]);
        let v: [[f32; 6]; 3] = [[-0.9, 0.9, 0.0, 1.0, 0.0, 0.0], [0.6, 0.9, 0.0, 1.0, 0.0, 0.0], [-0.9, -0.6, 0.0, 1.0, 0.0, 0.0]];
        d.vertices = Vertices::Inline { bytes: v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect(), stride: 24 };
        d.render_states.retain(|s| s.0 != 8);
        d.render_states.push((8, fill));
        d.render_states.sort();
        let theirs = d3d9.render(&[], Some(&blue_clear()), &[d.clone()], &back_buffer()).unwrap().rgba;
        let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
        gpu.clear(&blue_clear());
        gpu.draw(&d);
        gpu.present(&back_buffer());
        if gpu.errors.iter().any(|e| e.contains("cannot rasterise")) {
            continue;
        }
        assert_eq!(gpu.last_frame.unwrap().rgba, theirs, "fill mode {fill}");
    }
}

/// Exploration of D3D9 behaviour (run with --ignored --nocapture).
#[test]
#[ignore]
fn explore_d3d9_semantics() {
    let Some(mut d3d9) = d3d9ref::Oracle::new() else { return };
    eprintln!("adapter {:?}", ddgpu::WgpuBackend::headless().map(|g| g.adapter));
    // depth bias: clear 0.65, draw at 0.2 with ZFUNC GREATER
    for bias in [0f32, 0.05, 0.1, 0.15, 0.2, 0.25, 0.44, 0.46, 0.5, 1.0] {
        let clear = ClearCall { flags: 7, z: 0.65, depth: Some(depth_target()), ..blue_clear() };
        let d = depth_draw(0.2, 1.0, &[(23, 5), (14, 0), (195, bias.to_bits())]);
        eprintln!("depth bias {bias}: {:?}", both(&mut d3d9, &[], &clear, &[d]));
    }
    // mip selection: levels red, green, blue (4x4, 2x2, 1x1); the
    // triangle maps 2 texels per pixel (level 1 by derivatives)
    let level = |size: u32, bgra: [u8; 4]| bgra.repeat((size * size) as usize);
    let uploads: Vec<Upload> = [(4u32, [0u8, 0, 255, 255]), (2, [0, 255, 0, 255]), (1, [255, 0, 0, 255])].iter().enumerate()
        .map(|(k, &(size, c))| Upload::Image { resource: 0x300, index: k as u32, format: 21, width: size, height: size, depth: 1, pitch: 4 * size, slice: 4 * size * size, bytes: level(size, c) })
        .collect();
    for mip in [0u32, 1, 2] {
        for maxmip in [0u32, 1, 2] {
            for bias in [0f32, -1.0, 1.0] {
                let st = [0u32, 1, 1, 1, 0, 1, 1, mip, bias.to_bits(), maxmip, 1, 0, 0, 0];
                let b = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format: 21, width: 4, height: 4, depth: 1, levels: 3, usage: 0, states: st };
                let d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![b]);
                eprintln!("mipfilter {mip} maxmiplevel {maxmip} bias {bias}: {:?}", both(&mut d3d9, &uploads, &blue_clear(), &[d]));
            }
        }
    }
    // alpha test precision: alpha near 48/255 against ref 48
    let k = 48f32 / 255.0;
    for alpha in [k, f32::from_bits(k.to_bits() - 1), f32::from_bits(k.to_bits() + 1), 47.6 / 255.0, 47.4 / 255.0, 48.4 / 255.0, 48.6 / 255.0] {
        for func in [3u32, 5, 7] {
            let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
            c.f[0] = [1f32.to_bits(), 0, 0, alpha.to_bits()];
            let mut d = draw(ps_constant(), c, vec![]);
            d.render_states.retain(|s| ![15, 25, 24].contains(&s.0));
            d.render_states.extend([(15, 1), (25, func), (24, 48)]);
            d.render_states.sort();
            let r = both(&mut d3d9, &[], &blue_clear(), &[d]).unwrap();
            eprintln!("alpha {} ({:#x}) func {func} ref 48: D3D9 {:?} wgpu {:?}", alpha * 255.0, alpha.to_bits(), r.0, r.1);
        }
    }
    // a texel of alpha 48 (A8R8G8B8) against ref 48
    for func in [3u32, 6] {
        let up = Upload::Image { resource: 0x300, index: 0, format: 21, width: 1, height: 1, depth: 1, pitch: 4, slice: 4, bytes: vec![0, 0, 255, 48] };
        let st = [0u32, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0];
        let b = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format: 21, width: 1, height: 1, depth: 1, levels: 1, usage: 0, states: st };
        let mut d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![b]);
        d.render_states.retain(|s| ![15, 25, 24].contains(&s.0));
        d.render_states.extend([(15, 1), (25, func), (24, 48)]);
        d.render_states.sort();
        let r = both(&mut d3d9, &[up], &blue_clear(), &[d]).unwrap();
        eprintln!("texel alpha 48 func {func} ref 48: D3D9 {:?} wgpu {:?}", r.0, r.1);
    }
    // A2B10G10R10 with MINFILTER ANISOTROPIC, MAXANISOTROPY 1, mips
    for (min, maxaniso, mip) in [(3u32, 1u32, 2u32), (2, 1, 2), (3, 1, 0), (3, 2, 2), (2, 1, 1)] {
        let mut rng = Rng(77);
        let uploads: Vec<Upload> = [4u32, 2, 1].iter().enumerate().map(|(k, &size)| {
            let (bytes, pitch) = level_texels(&mut rng, 31, size, size);
            Upload::Image { resource: 0x300, index: k as u32, format: 31, width: size, height: size, depth: 1, pitch, slice: pitch * size, bytes }
        }).collect();
        let st = [0u32, 1, 1, 1, 0, 1, min, mip, 0.5f32.to_bits(), 0, maxaniso, 0, 0, 0];
        let b = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format: 31, width: 4, height: 4, depth: 1, levels: 3, usage: 0, states: st };
        let d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![b]);
        eprintln!("A2B10G10R10 min {min} maxaniso {maxaniso} mip {mip}: {:?}", both(&mut d3d9, &uploads, &blue_clear(), &[d]));
    }
    // fill modes: a triangle inside the 4x4 target
    for fill in [1u32, 2, 3] {
        let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
        c.f[0] = [1f32.to_bits(), 1f32.to_bits(), 0, 1f32.to_bits()];
        let mut d = draw(ps_constant(), c, vec![]);
        let v: [[f32; 6]; 3] = [[-0.9, 0.9, 0.0, 1.0, 0.0, 0.0], [0.6, 0.9, 0.0, 1.0, 0.0, 0.0], [-0.9, -0.6, 0.0, 1.0, 0.0, 0.0]];
        d.vertices = Vertices::Inline { bytes: v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect(), stride: 24 };
        d.render_states.retain(|s| s.0 != 8);
        d.render_states.push((8, fill));
        d.render_states.sort();
        let theirs = d3d9.render(&[], Some(&blue_clear()), &[d.clone()], &back_buffer()).unwrap().rgba;
        let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
        gpu.clear(&blue_clear());
        gpu.draw(&d);
        gpu.present(&back_buffer());
        let lit = |f: &[u8]| f.chunks_exact(4).enumerate().filter(|(_, p)| p[2] == 0).map(|(k, _)| k).collect::<Vec<_>>();
        eprintln!("fill {fill}: D3D9 lit {:?} wgpu lit {:?} {:?}", lit(&theirs), gpu.last_frame.as_ref().map(|f| lit(&f.rgba)), gpu.errors);
    }
    // mip LINEAR blends per format and bias: levels 0x00, 0x80, 0xFF grey
    for format in [21u32, 31] {
        for bias in [0f32, 0.25, 0.5, 0.75, 1.0] {
            let uploads: Vec<Upload> = [(4u32, 0u32), (2, 0x80), (1, 0xFF)].iter().enumerate().map(|(k, &(size, v))| {
                let texel = if format == 21 { u32::from_le_bytes([v as u8, v as u8, v as u8, 255]) } else { let t = v * 1023 / 255; t | t << 10 | t << 20 | 3 << 30 };
                Upload::Image { resource: 0x300, index: k as u32, format, width: size, height: size, depth: 1, pitch: 4 * size, slice: 4 * size * size, bytes: texel.to_le_bytes().repeat((size * size) as usize) }
            }).collect();
            let st = [0u32, 1, 1, 1, 0, 2, 2, 2, bias.to_bits(), 0, 1, 0, 0, 0];
            let b = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 3, usage: 0, states: st };
            let d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![b]);
            let r = both(&mut d3d9, &uploads, &blue_clear(), &[d]).unwrap();
            eprintln!("mip linear format {format} bias {bias}: D3D9 {:?} wgpu {:?}", r.0, r.1);
        }
    }
    // border colours per format: a 4x4 level of 0.25, linear, border white
    for format in [21u32, 114, 111, 116, 113] {
        let texel: Vec<u8> = match format {
            21 => vec![0x40, 0x40, 0x40, 0x40],
            114 => 0.25f32.to_le_bytes().to_vec(),
            116 => [0.25f32; 4].iter().flat_map(|f| f.to_le_bytes()).collect(),
            111 => half(0.25).to_le_bytes().to_vec(),
            _ => [half(0.25); 4].iter().flat_map(|f| f.to_le_bytes()).collect(),
        };
        for (mag, min, maxmip, mip) in [(1u32, 2u32, 0u32, 0u32), (2, 2, 0, 0), (1, 2, 2, 0), (1, 1, 0, 0)] {
            let up = Upload::Image { resource: 0x300, index: 0, format, width: 4, height: 4, depth: 1, pitch: 4 * texel.len() as u32, slice: 16 * texel.len() as u32, bytes: texel.repeat(16) };
            let st = [0u32, 4, 4, 1, 0xFFFF_FFFF, mag, min, mip, 0, maxmip, 1, 0, 0, 0];
            let b = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 1, usage: 0, states: st };
            let d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![b]);
            let theirs = d3d9.render(std::slice::from_ref(&up), Some(&blue_clear()), std::slice::from_ref(&d), &back_buffer()).unwrap().rgba;
            let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
            gpu.upload(up);
            gpu.clear(&blue_clear());
            gpu.draw(&d);
            gpu.present(&back_buffer());
            let ours = gpu.last_frame.unwrap().rgba;
            let red = |f: &[u8]| f.chunks_exact(4).map(|p| p[0]).collect::<Vec<_>>();
            eprintln!("border format {format} mag {mag} min {min} maxmip {maxmip}: D3D9 {:?} wgpu {:?}", red(&theirs), red(&ours));
        }
    }
    // sRGB reads per format: one texel of a mid value
    for (format, texel) in [(21u32, vec![0x80u8, 0x40, 0x20, 0xFF]), (22, vec![0x80, 0x40, 0x20, 0xFF]), (23, vec![0x10, 0x84]), (24, vec![0x10, 0x42]),
                            (25, vec![0x10, 0xC2]), (26, vec![0x84, 0xF2]), (50, vec![0x80]), (51, vec![0x80, 0xFF]), (28, vec![0x80]),
                            (31, vec![0x00, 0x02, 0x08, 0xE0])] {
        for srgb in [0u32, 1] {
            let up = Upload::Image { resource: 0x300, index: 0, format, width: 1, height: 1, depth: 1, pitch: texel.len() as u32, slice: texel.len() as u32, bytes: texel.clone() };
            let st = [0u32, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, srgb, 0, 0];
            let b = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 1, height: 1, depth: 1, levels: 1, usage: 0, states: st };
            let d = draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![b]);
            eprintln!("format {format} srgb {srgb}: {:?}", both(&mut d3d9, &[up], &blue_clear(), &[d]));
        }
    }
}

#[test]
fn indexed_strips_match_the_d3d9_driver() {
    // DrawIndexedPrimitive(TRIANGLESTRIP) with 16-bit indices: a full-screen
    // quad as two strip triangles (wgpu needs the strip index format)
    if d3d9ref::Oracle::new().is_none() || ddgpu::WgpuBackend::headless().is_none() {
        eprintln!("skipped: no D3D9 or no adapter");
        return;
    }
    let quad: [[f32; 6]; 4] = [[-1.0, -1.0, 0.0, 1.0, 0.0, 1.0], [-1.0, 1.0, 0.0, 1.0, 0.0, 0.0],
                               [1.0, -1.0, 0.0, 1.0, 1.0, 1.0], [1.0, 1.0, 0.0, 1.0, 1.0, 0.0]];
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = [0.25f32.to_bits(), 0.75f32.to_bits(), 0.5f32.to_bits(), 1f32.to_bits()];
    let mut d = draw(ps_constant(), c, vec![]);
    d.primitive = 5;
    d.primitives = 2;
    d.vertex_count = 4;
    d.vertices = Vertices::Inline { bytes: quad.iter().flatten().flat_map(|f| f.to_le_bytes()).collect(), stride: 24 };
    d.indices = Indices::Inline { bytes: [3u16, 2, 1, 0].iter().flat_map(|i| i.to_le_bytes()).collect(), format: 101 };
    let (ours, driver) = compare(&[], &blue_clear(), &[d]);
    assert_eq!(ours, driver);
}

/// The queue must preserve mutable uploads/constants, copies, query boundaries,
/// release/reuse and the actual presentation sink, not just a clear's pixels.
#[test]
fn deferred_frame_matches_synchronous_image() {
    use std::sync::{Arc, Mutex};
    struct Sink(Arc<Mutex<Vec<std::thread::ThreadId>>>);
    impl ddgpu::PresentSink for Sink {
        fn present(&mut self, _: &ddgpu::Presented) {
            self.0.lock().unwrap().push(std::thread::current().id());
        }
    }
    fn render(deferred: bool) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<std::thread::ThreadId>) {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let threads = Arc::new(Mutex::new(Vec::new()));
        let mut gpu = ddgpu::WgpuBackend::headless().expect("adapter");
        let output = frames.clone();
        gpu.on_present = Some(Box::new(move |frame| output.lock().unwrap().push(frame.rgba.clone())));
        gpu.present_sink = Some(Box::new(Sink(threads.clone())));
        let mut backend: Box<dyn GraphicsBackend> = if deferred {
            Box::new(d3d9cmd::DeferredBackend::with_capacity(Box::new(gpu), 8))
        } else { Box::new(gpu) };
        let bb = back_buffer();
        let copy = Target { surface: 0x400, ..bb.clone() };
        let mut reads = Vec::new();
        for colour in [[1.0f32, 0.2, 0.0, 1.0], [0.0, 1.0, 0.2, 1.0]] {
            backend.clear(&ClearCall { rects: vec![], flags: 1, colour: 0xFF0000FF, z: 1.0, stencil: 0,
                targets: [Some(bb.clone()), None, None, None], depth: None, viewport: [0, 0, 4, 4, 0, 0], scissor: None, srgb: false });
            backend.upload(Upload::Buffer { resource: 0x300, bytes: triangle() });
            backend.upload(Upload::Buffer { resource: 0x301, bytes: [0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes).collect() });
            let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
            c.f[0] = colour.map(f32::to_bits);
            let mut d = draw(ps_constant(), c, vec![]);
            d.vertices = Vertices::Streams(vec![(0, 0x300, 0, 24, 1)]);
            d.indices = Indices::Buffer(0x301, 101);
            backend.query_begin(0x500);
            backend.draw(&d);
            d.ps_constants = Constants::default().into();
            // Neither later writes nor releasing/reusing an ID may affect the draw.
            backend.upload(Upload::Buffer { resource: 0x300, bytes: vec![0; 72] });
            backend.query_end(0x500);
            backend.copy(&d3d9cmd::CopyCall { src: bb.clone(), dst: copy.clone(), src_rect: [0, 0, 4, 4], dst_rect: [0, 0, 4, 4], filter: 1 });
            backend.fill(&d3d9cmd::FillCall { dst: bb.clone(), rect: [0, 0, 1, 1], colour: 0xFFFFFFFF });
            backend.present(&bb);
            reads.push(backend.read_back(&copy).expect("copied image"));
            // a deferred backend answers None until its worker has the count
            // (L102: GetData never waits); a game polls the same way
            let count = loop {
                if let Some(n) = backend.query_result(0x500) {
                    break n;
                }
                backend.fence();
            };
            assert_eq!(count, 16);
            backend.release(0x300);
            backend.release(0x301);
        }
        drop(backend);
        let frames = frames.lock().unwrap().clone();
        let threads = threads.lock().unwrap().clone();
        (reads, frames, threads)
    }
    let (sync_reads, sync_frames, sync_threads) = render(false);
    let (async_reads, async_frames, async_threads) = render(true);
    assert_eq!(sync_reads, async_reads);
    assert_eq!(sync_frames, async_frames);
    assert_ne!(sync_frames[0], sync_frames[1]);
    assert_eq!(&sync_frames[0][0..4], &[255; 4]);
    assert_eq!(&sync_frames[0][4..8], &[255, 51, 0, 255]);
    assert!(sync_threads.iter().all(|id| *id == std::thread::current().id()));
    assert!(async_threads.iter().all(|id| *id != std::thread::current().id()));
    assert_eq!(async_threads[0], async_threads[1]);
}
