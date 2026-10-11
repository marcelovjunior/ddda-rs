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
        let binding = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 1, states };
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
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat, format: 21, width: 2, height: 2, depth: 1, levels: 1, states };
    gpu.draw(&draw(ps_texture(), Constants { f: vec![[0; 4]; 224], ..Default::default() }, vec![binding]));
    let other = Target { surface: 0x101, format: 21, width: 4, height: 4, texture: None, level: 0 };
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
    Target { surface: 0x180, format: 75, width: 4, height: 4, texture: None, level: 0 }
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
    let [before, building, after, count] = d3d9.occlusion_states().expect("D3D9 queries");
    eprintln!("GetData before Issue {before:#x}, between BEGIN and END {building:#x}, after END {after:#x} (count {count})");
    // the runtime's answers (crates/ddruntime/src/d3d9copy.rs) follow these
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
        d.vertex_shader = Some(vs_offset());
        d.declaration = vec![[0, 0, 0, 0, 3, 0, 0, 0], [1, 0, 0, 0, 3, 0, 5, 1]];
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
