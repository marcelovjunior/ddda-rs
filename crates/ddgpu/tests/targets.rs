//! Render targets of every format the renderer uses (brief L29 census:
//! A8R8G8B8, A16B16G16R16F, A2B10G10R10, R32F, G16R16, A32B32G32R32F),
//! several at once (MRT), and D24S8 depth, compared with the host's D3D9
//! driver as the bits each target holds. Skips without D3D9 or an adapter.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, Upload, Vertices};

/// vs_3_0: o0 = v0 (position only)
fn vs() -> Vec<u32> {
    vec![
        0xFFFE_0300,
        0x0200_001F, 0x8000_0000, 0x900F_0000, // dcl_position v0
        0x0200_001F, 0x8000_0000, 0xE00F_0000, // dcl_position o0
        0x0200_0001, 0xE00F_0000, 0x90E4_0000, // mov o0, v0
        0x0000_FFFF,
    ]
}

/// ps_3_0 writing oC_k = c_k for k < `outputs`
fn ps(outputs: u32) -> Vec<u32> {
    let mut t = vec![0xFFFF_0300];
    for k in 0..outputs {
        t.extend_from_slice(&[0x0200_0001, 0x800F_0800 | k, 0xA0E4_0000 | k]);
    }
    t.push(0x0000_FFFF);
    t
}

fn target(surface: u32, format: u32) -> Target {
    Target { surface, format, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
}

fn depth_target() -> Target {
    Target { surface: 0x180, format: 75, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
}

/// A triangle over the whole 4x4 target at depth `z` (float4 positions).
fn triangle(z: f32) -> Vec<u8> {
    [[-1.0f32, -1.0, z, 1.0], [-1.0, 3.0, z, 1.0], [3.0, -1.0, z, 1.0]].iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

/// The render states every draw sets (the D3D9 device keeps state between
/// draws, so each draw names all it depends on), with `extra` on top.
fn states(extra: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = vec![
        (7, 0), (14, 0), (23, 4), (15, 0), (22, 1), (27, 0), (19, 2), (20, 1), (171, 1), (206, 0), (207, 2), (208, 1),
        (209, 1), (193, 0xFFFF_FFFF), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (174, 0), (194, 0), (195, 0), (175, 0),
        (52, 0), (185, 0), (8, 3),
    ];
    for &(k, val) in extra {
        v.retain(|s| s.0 != k);
        v.push((k, val));
    }
    v.sort();
    v
}

/// A draw of constants `c` (one per output) into `targets`.
fn draw(c: &[[f32; 4]], targets: [Option<Target>; 4], z: f32, extra: &[(u32, u32)]) -> DrawCall {
    let mut ps_constants = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    for (k, v) in c.iter().enumerate() {
        ps_constants.f[k] = v.map(f32::to_bits);
    }
    DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Inline { bytes: triangle(z), stride: 16 },
        indices: Indices::None,
        vertex_shader: Some((vs()).into()),
        pixel_shader: Some((ps(c.len() as u32)).into()),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(),
        fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
        ps_constants: ps_constants.into(),
        textures: vec![],
        render_states: states(extra).into(),
        viewport: [0, 0, 4, 4, 0f32.to_bits(), 1f32.to_bits()],
        scissor: [0, 0, 4, 4],
        targets,
        depth: None,
    }
}

fn clear_of(targets: &[Target], colour: u32) -> ClearCall {
    let mut t: [Option<Target>; 4] = [None, None, None, None];
    for (k, x) in targets.iter().enumerate() {
        t[k] = Some(x.clone());
    }
    ClearCall { rects: vec![], flags: 1, colour, z: 1.0, stencil: 0, targets: t, depth: None, viewport: [0, 0, 4, 4, 0, 0], scissor: None, srgb: false }
}

/// Draw on the wgpu backend and read every target of `reads` back.
fn ours(uploads: &[Upload], clear: &ClearCall, draws: &[DrawCall], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
    let mut gpu = ddgpu::WgpuBackend::headless().ok_or("no adapter")?;
    for u in uploads {
        gpu.upload(u.clone());
    }
    gpu.clear(clear);
    for d in draws {
        gpu.draw(d);
    }
    let out: Option<Vec<Vec<u8>>> = reads.iter().map(|t| gpu.read_back(t)).collect();
    if !gpu.errors.is_empty() {
        return Err(format!("{:?}", gpu.errors));
    }
    out.ok_or_else(|| "read back".into())
}

fn available() -> Option<d3d9ref::Oracle> {
    let d3d9 = d3d9ref::Oracle::new()?;
    ddgpu::WgpuBackend::headless()?;
    Some(d3d9)
}

/// A pixel's channels as numbers for messages.
fn decode(format: u32, px: &[u8]) -> Vec<f64> {
    let u16le = |k: usize| u16::from_le_bytes([px[2 * k], px[2 * k + 1]]);
    let f32le = |k: usize| f32::from_le_bytes(px[4 * k..4 * k + 4].try_into().unwrap()) as f64;
    let half = |h: u16| {
        let (s, e, m) = ((h >> 15) as i32, ((h >> 10) & 31) as i32, (h & 1023) as f64);
        let v = if e == 0 { m * 2f64.powi(-24) } else if e == 31 { if m == 0.0 { f64::INFINITY } else { f64::NAN } } else { (1.0 + m / 1024.0) * 2f64.powi(e - 15) };
        if s == 1 { -v } else { v }
    };
    match format {
        21 | 22 => vec![px[2] as f64, px[1] as f64, px[0] as f64, px[3] as f64],
        113 => (0..4).map(|k| half(u16le(k))).collect(),
        111 => vec![half(u16le(0))],
        112 => (0..2).map(|k| half(u16le(k))).collect(),
        114 => vec![f32le(0)],
        115 => (0..2).map(f32le).collect(),
        116 => (0..4).map(f32le).collect(),
        34 => (0..2).map(|k| u16le(k) as f64).collect(),
        36 => (0..4).map(|k| u16le(k) as f64).collect(),
        31 => {
            let v = u32::from_le_bytes(px[..4].try_into().unwrap());
            vec![(v & 1023) as f64, ((v >> 10) & 1023) as f64, ((v >> 20) & 1023) as f64, (v >> 30) as f64]
        }
        _ => px.iter().map(|&b| b as f64).collect(),
    }
}

/// Equal, except the one known difference: a blend result beyond the
/// half range in a 16-bit float target is +-inf on the D3D9 driver and
/// +-65504 on the Vulkan driver's blend unit (direct writes agree).
fn same(format: u32, blending: bool, ours: &[u8], theirs: &[u8]) -> bool {
    if ours == theirs {
        return true;
    }
    if !blending || !matches!(format, 111 | 112 | 113) || ours.len() != theirs.len() {
        return false;
    }
    ours.chunks_exact(2).zip(theirs.chunks_exact(2)).all(|(a, b)| {
        let (a, b) = (u16::from_le_bytes([a[0], a[1]]), u16::from_le_bytes([b[0], b[1]]));
        a == b || (b & 0x7FFF == 0x7C00 && a == (b & 0x8000) | 0x7BFF)
    })
}

/// The formats of the census plus the other float targets the backend maps.
const FORMATS: [u32; 10] = [21, 113, 31, 114, 34, 116, 111, 112, 115, 36];

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
    fn value(&mut self) -> f32 {
        self.pick(&[0.0f32, 1.0, 0.5, 0.25, 0.1, 0.3333, 0.75, 1.5, 2.0, 65504.0, 100000.0, -0.5, -1.0, -3.0, 1e-5, 6e-8, 0.999, 0.5001])
    }
}

/// Every target format, cleared then drawn with random values (in and out
/// of [0, 1], negative, beyond half range, denormal), blending (factors,
/// ops, separate alpha), write masks and sRGB writes, compared as bits.
/// TARGETS_FUZZ sets the number of cases.
#[test]
fn target_formats_match_the_d3d9_driver() {
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let cases: usize = std::env::var("TARGETS_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let mut rng = Rng(0x7A26_0029);
    let blends = [1u32, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14, 15];
    let mut failures = Vec::new();
    let mut refused = std::collections::BTreeSet::new();
    let mut compared = 0;
    for case in 0..cases {
        let format = FORMATS[case % FORMATS.len()];
        let t = target(0x100, format);
        let c = [[rng.value(), rng.value(), rng.value(), rng.value()]];
        let blend = rng.next() % 2;
        let extra = vec![
            (27, blend), (19, rng.pick(&blends)), (20, rng.pick(&blends)), (171, rng.pick(&[1u32, 2, 3, 4, 5])),
            (206, rng.next() % 2), (207, rng.pick(&blends)), (208, rng.pick(&blends)), (209, rng.pick(&[1u32, 2, 3, 4, 5])),
            (193, rng.next()), (168, if rng.next() % 4 == 0 { rng.next() % 16 } else { 0xF }), (194, if rng.next() % 4 == 0 { 1 } else { 0 }),
        ];
        let d = draw(&c, [Some(t.clone()), None, None, None], 0.5, &extra);
        let clear = clear_of(&[t.clone()], rng.next());
        let theirs = match d3d9.render_targets(&[], Some(&clear), &[d.clone()], &[t.clone()]) {
            Ok(r) => r,
            Err(e) if e.contains("CreateRenderTarget") || e.contains("CreateOffscreenPlainSurface") => {
                refused.insert(format);
                continue;
            }
            Err(e) => panic!("D3D9: {e}"),
        };
        compared += 1;
        match ours(&[], &clear, &[d], &[t.clone()]) {
            Ok(o) if same(format, blend != 0, &o[0], &theirs[0]) => {}
            Ok(o) => failures.push(format!("case {case} format {format}: ours {:?} D3D9 {:?}; constant {:?} clear {:#010x} states {:?}",
                                           decode(format, &o[0][..16.min(o[0].len())]), decode(format, &theirs[0][..16.min(theirs[0].len())]), c[0], clear.colour, extra)),
            Err(e) => failures.push(format!("case {case} format {format}: backend error {e}; states {extra:?}")),
        }
    }
    eprintln!("{} of {compared} compared cases differ; formats the driver refuses as targets: {refused:?}", failures.len());
    for f in failures.iter().take(30) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty());
}

/// Two to four targets at once (MRT), each written from its own output,
/// with per-target write masks and blending; mixed formats (the device has
/// D3DPMISCCAPS_MRTINDEPENDENTBITDEPTHS).
#[test]
fn mrt_draws_match_the_d3d9_driver() {
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let mut rng = Rng(0x0000_4D52);
    let sets: [&[u32]; 5] = [&[21, 21], &[21, 21, 21, 21], &[113, 113, 21], &[21, 114, 31, 113], &[113, 34, 114]];
    let mut failures = Vec::new();
    for case in 0..40 {
        let formats = sets[case % sets.len()];
        let ts: Vec<Target> = formats.iter().enumerate().map(|(k, &f)| target(0x100 + k as u32, f)).collect();
        let mut targets: [Option<Target>; 4] = [None, None, None, None];
        for (k, t) in ts.iter().enumerate() {
            targets[k] = Some(t.clone());
        }
        let c: Vec<[f32; 4]> = (0..ts.len()).map(|_| [rng.value(), rng.value(), rng.value(), rng.value()]).collect();
        let extra = vec![
            (27, rng.next() % 2), (19, rng.pick(&[2u32, 5, 9])), (20, rng.pick(&[1u32, 2, 6])), (171, rng.pick(&[1u32, 4])),
            (168, rng.next() % 16), (190, rng.next() % 16), (191, rng.next() % 16), (192, rng.next() % 16),
        ];
        let d = draw(&c, targets, 0.5, &extra);
        let clear = clear_of(&ts, rng.next());
        let theirs = d3d9.render_targets(&[], Some(&clear), &[d.clone()], &ts).expect("D3D9");
        match ours(&[], &clear, &[d], &ts) {
            Ok(o) => {
                for (k, (a, b)) in o.iter().zip(&theirs).enumerate() {
                    if a != b {
                        failures.push(format!("case {case} target {k} ({}): ours {:?} D3D9 {:?}; constants {c:?} states {extra:?}",
                                              formats[k], decode(formats[k], &a[..16.min(a.len())]), decode(formats[k], &b[..16.min(b.len())])));
                    }
                }
            }
            Err(e) => failures.push(format!("case {case}: backend error {e}")),
        }
    }
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} MRT targets differ", failures.len());
}

/// D24S8 depth precision: a draw at z1 writes depth, a second at
/// z1 + delta tests against it (LESS, EQUAL, LESSEQUAL, GREATER); deltas
/// around the 24-bit step 2^-24 show how depth values are quantised.
#[test]
fn depth_precision_matches_the_d3d9_driver() {
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let rt = target(0x100, 21);
    let mut failures = Vec::new();
    for z1 in [0.5f32, 0.25, 0.9, 0.1, 0.33333334] {
        for delta_steps in [0.0f32, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, -0.5, -1.0] {
            for func in [2u32, 3, 4, 5] {
                let z2 = z1 + delta_steps * 2f32.powi(-24);
                let mut first = draw(&[[1.0, 0.0, 0.0, 1.0]], [Some(rt.clone()), None, None, None], z1, &[(7, 1), (14, 1), (23, 8)]);
                first.depth = Some(depth_target());
                let mut second = draw(&[[0.0, 1.0, 0.0, 1.0]], [Some(rt.clone()), None, None, None], z2, &[(7, 1), (14, 0), (23, func)]);
                second.depth = Some(depth_target());
                let clear = ClearCall { flags: 7, z: 1.0, depth: Some(depth_target()), ..clear_of(&[rt.clone()], 0xFF00_00FF) };
                let theirs = d3d9.render_targets(&[], Some(&clear), &[first.clone(), second.clone()], &[rt.clone()]).expect("D3D9");
                match ours(&[], &clear, &[first, second], &[rt.clone()]) {
                    Ok(o) if o == theirs => {}
                    Ok(o) => failures.push(format!("z1 {z1} delta {delta_steps}x2^-24 func {func}: ours {:?} D3D9 {:?}", &o[0][..4], &theirs[0][..4])),
                    Err(e) => failures.push(format!("backend error {e}")),
                }
            }
        }
    }
    for f in failures.iter().take(30) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} depth cases differ", failures.len());
}

/// What D3D9's Clear does with the scissor test, the colour write mask and
/// sRGB writes on (run with --ignored --nocapture; prints).
#[test]
#[ignore]
fn explore_clear_states() {
    let Some(mut d3d9) = available() else { return };
    let colour = 0xFF80_4020;
    for (name, states, scissor) in [
        ("plain", vec![], [0, 0, 4, 4]),
        ("scissor 1..3", vec![(174u32, 1u32)], [1, 1, 3, 3]),
        ("scissor rect set, test off", vec![(174, 0)], [1, 1, 3, 3]),
        ("write mask red only", vec![(168, 1)], [0, 0, 4, 4]),
        ("write mask 0", vec![(168, 0)], [0, 0, 4, 4]),
        ("srgb write", vec![(194, 1)], [0, 0, 4, 4]),
    ] {
        let raw = d3d9.clear_with_states(colour, &states, scissor).expect("D3D9");
        let px: Vec<[u8; 4]> = raw.chunks_exact(4).map(|p| [p[2], p[1], p[0], p[3]]).collect();
        eprintln!("{name}: pixel 0 {:?}, pixel 5 {:?}, pixel 15 {:?}", px[0], px[5], px[15]);
    }
    let one = 1f32.to_bits();
    for (name, viewport, rects, states, scissor) in [
        ("viewport 1,1 2x2", [1u32, 1, 2, 2, 0, one], vec![], vec![], [0, 0, 4, 4]),
        ("rect 0,0..2,2", [0, 0, 4, 4, 0, one], vec![[0, 0, 2, 2]], vec![], [0, 0, 4, 4]),
        ("rect 0,0..2,2 viewport 1,1 2x2", [1, 1, 2, 2, 0, one], vec![[0, 0, 2, 2]], vec![], [0, 0, 4, 4]),
        ("viewport 1,1 2x2 scissor 0,0..2,2", [1, 1, 2, 2, 0, one], vec![], vec![(174u32, 1u32)], [0, 0, 2, 2]),
    ] {
        let raw = d3d9.clear_probe(colour, &states, scissor, viewport, &rects).expect("D3D9");
        let lit: Vec<usize> = raw.chunks_exact(4).enumerate().filter(|(_, p)| p[3] != 0).map(|(k, _)| k).collect();
        eprintln!("{name}: cleared pixels {lit:?}");
    }
}

/// Clears of part of a target, as D3D9 does them: a full clear, then a
/// clear with a random viewport, rectangles, scissor rectangle (test on or
/// off), sRGB write and flags, then a probe draw whose depth and stencil
/// tests show the depth-stencil contents; colour targets A8R8G8B8,
/// A16B16G16R16F and R32F over D24S8.
#[test]
fn clears_match_the_d3d9_driver() {
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let mut rng = Rng(0x00C1_EA25);
    let one = 1f32.to_bits();
    let mut failures = Vec::new();
    for case in 0..60 {
        let format = [21u32, 113, 114][case % 3];
        let rt = target(0x100, format);
        let full = ClearCall { flags: 7, z: 1.0, stencil: 0, depth: Some(depth_target()), viewport: [0, 0, 4, 4, 0, one], ..clear_of(&[rt.clone()], rng.next()) };
        let (x, y) = (rng.next() % 3, rng.next() % 3);
        let (w, h) = (1 + rng.next() % (4 - x), 1 + rng.next() % (4 - y));
        let rect = |rng: &mut Rng| {
            let (l, t) = ((rng.next() % 4) as i32, (rng.next() % 4) as i32);
            [l, t, l + 1 + (rng.next() % 3) as i32, t + 1 + (rng.next() % 3) as i32]
        };
        let rects: Vec<[i32; 4]> = (0..rng.next() % 3).map(|_| rect(&mut rng)).collect();
        let partial = ClearCall {
            flags: rng.pick(&[1u32, 2, 4, 3, 5, 6, 7]), z: rng.pick(&[0.25f32, 0.75]), stencil: rng.next() % 256,
            rects, viewport: [x, y, w, h, 0, one], scissor: (rng.next() % 2 == 1).then(|| rect(&mut rng)),
            srgb: format == 21 && rng.next() % 2 == 1, depth: Some(depth_target()), ..clear_of(&[rt.clone()], rng.next())
        };
        let mut probe = draw(&[[0.25, 0.5, 0.75, 1.0]], [Some(rt.clone()), None, None, None], 0.5,
                             &[(7, 1), (14, 0), (23, rng.pick(&[2u32, 5])), (52, 1), (56, 3), (57, partial.stencil), (58, 0xFF), (59, 0)]);
        probe.depth = Some(depth_target());
        let clears = [full, partial.clone()];
        let theirs = d3d9.render_cleared(&[], &clears, &[probe.clone()], &[rt.clone()]).expect("D3D9").remove(0);
        let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
        for c in &clears {
            gpu.clear(c);
        }
        gpu.draw(&probe);
        let o = gpu.read_back(&rt).unwrap();
        if !gpu.errors.is_empty() {
            failures.push(format!("case {case}: backend error {:?}", gpu.errors));
        } else if o != theirs {
            let pix = |raw: &[u8]| raw.chunks_exact(raw.len() / 16).map(|p| decode(format, p)[0] as f32).collect::<Vec<_>>();
            failures.push(format!("case {case} format {format}: ours {:?} D3D9 {:?}; clear {:?}", pix(&o), pix(&theirs),
                                  (partial.flags, partial.viewport, &partial.rects, partial.scissor, partial.srgb)));
        }
    }
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} clears differ", failures.len());
}

/// vs_3_0: o0 = v0, o1 (texcoord) = v1
fn vs_textured() -> Vec<u32> {
    vec![
        0xFFFE_0300,
        0x0200_001F, 0x8000_0000, 0x900F_0000, // dcl_position v0
        0x0200_001F, 0x8000_0005, 0x900F_0001, // dcl_texcoord v1
        0x0200_001F, 0x8000_0000, 0xE00F_0000, // dcl_position o0
        0x0200_001F, 0x8000_0005, 0xE00F_0001, // dcl_texcoord o1
        0x0200_0001, 0xE00F_0000, 0x90E4_0000, // mov o0, v0
        0x0200_0001, 0xE00F_0001, 0x90E4_0001, // mov o1, v1
        0x0000_FFFF,
    ]
}

/// ps_3_0: oC0 = texld(v0.xy, s0)
fn ps_sample() -> Vec<u32> {
    vec![
        0xFFFF_0300,
        0x0200_001F, 0x8000_0005, 0x9003_0000, // dcl_texcoord v0.xy
        0x0200_001F, 0x9000_0000, 0xA00F_0800, // dcl_2d s0
        0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800, // texld r0, v0, s0
        0x0200_0001, 0x800F_0800, 0x80E4_0000, // mov oC0, r0
        0x0000_FFFF,
    ]
}

/// A draw over the whole 4x4 `out` sampling `binding` (texcoords spanning
/// [0, 1) over the target).
fn sample_draw(binding: d3d9cmd::TextureBinding, out: Target, extra: &[(u32, u32)]) -> DrawCall {
    let v: [[f32; 6]; 3] = [[-1.0, -1.0, 0.5, 1.0, 0.0, 1.0], [-1.0, 3.0, 0.5, 1.0, 0.0, -1.0], [3.0, -1.0, 0.5, 1.0, 2.0, 1.0]];
    let mut d = draw(&[], [Some(out), None, None, None], 0.5, extra);
    d.vertices = Vertices::Inline { bytes: v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect(), stride: 24 };
    d.vertex_shader = Some((vs_textured()).into());
    d.pixel_shader = Some((ps_sample()).into());
    d.declaration = vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]].into();
    d.textures = vec![binding];
    d
}

/// A render-target texture of every target format, cleared and drawn into
/// (the whole level, then its left half through a viewport) before it is
/// ever bound, then sampled with point or linear filtering into an
/// A32B32G32R32F target; 8-bit levels also written and read through sRGB.
/// The level and the sampled values compared with the driver as bits
/// (D3D9 samples the channels a format lacks as 1).
#[test]
fn render_to_texture_matches_the_d3d9_driver() {
    use d3d9cmd::{TargetTexture, TexKind, TextureBinding, USAGE_RENDERTARGET};
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let formats = [21u32, 22, 113, 31, 114, 34, 116, 111, 112, 115, 36];
    let mut rng = Rng(0x7A26_0041);
    let out = target(0x100, 116);
    let mut failures = Vec::new();
    let mut refused = std::collections::BTreeSet::new();
    let mut compared = 0;
    for case in 0..4 * formats.len() {
        let format = formats[case % formats.len()];
        let variant = case / formats.len();
        let tex = TargetTexture { resource: 0x300, kind: TexKind::Flat, width: 4, height: 4, depth: 1, levels: 1, usage: USAGE_RENDERTARGET };
        let level = Target { surface: 0x301, format, width: 4, height: 4, texture: Some(tex), level: 0, samples: 0, quality: 0 };
        let eight_bit = matches!(format, 21 | 22);
        let (srgb_write, srgb_read) = (eight_bit && variant & 2 != 0, eight_bit && variant & 1 != 0);
        let mut value = || [rng.value(), rng.value(), rng.value(), rng.value()];
        let (c1, c2) = (value(), value());
        let level_clear = clear_of(&[level.clone()], rng.next());
        let fill = draw(&[c1], [Some(level.clone()), None, None, None], 0.5, &[(194, srgb_write as u32)]);
        let mut half = draw(&[c2], [Some(level.clone()), None, None, None], 0.5, &[(194, srgb_write as u32)]);
        half.viewport = [0, 0, 2, 4, 0f32.to_bits(), 1f32.to_bits()];
        // ADDRESSU/V/W clamp, MAG/MINFILTER point or linear, SRGBTEXTURE
        let filter = 1 + (variant as u32 & 1);
        let st = [0u32, 3, 3, 3, 0, filter, filter, 0, 0, 0, 1, srgb_read as u32, 0, 0];
        let binding = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 1, usage: USAGE_RENDERTARGET, states: st };
        let sample = sample_draw(binding, out.clone(), &[]);
        let clears = [clear_of(&[out.clone()], 0), level_clear];
        let draws = [fill, half, sample];
        let reads = [out.clone(), level.clone()];
        let theirs = match d3d9.render_cleared(&[], &clears, &draws, &reads) {
            Ok(r) => r,
            Err(e) if e.contains("render target") || e.contains("CreateOffscreenPlainSurface") => {
                refused.insert(format);
                continue;
            }
            Err(e) => panic!("D3D9: {e}"),
        };
        compared += 1;
        let mut gpu = ddgpu::WgpuBackend::headless().expect("adapter");
        for c in &clears {
            gpu.clear(c);
        }
        for d in &draws {
            gpu.draw(d);
        }
        let mine: Vec<Option<Vec<u8>>> = reads.iter().map(|t| gpu.read_back(t)).collect();
        if !gpu.errors.is_empty() {
            failures.push(format!("case {case} format {format}: backend errors {:?}", gpu.errors));
            continue;
        }
        let (Some(o_out), Some(o_level)) = (&mine[0], &mine[1]) else {
            failures.push(format!("case {case} format {format}: no read-back"));
            continue;
        };
        // the X byte of X8R8G8B8 is unused: the driver reads it back as 0
        // whatever was drawn (and samples it as 1, compared below)
        let x_unused = |v: &[u8]| -> Vec<u8> { if format == 22 { v.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect() } else { v.to_vec() } };
        if !same(format, false, &x_unused(o_level), &x_unused(&theirs[1])) {
            failures.push(format!("case {case} format {format} srgb write {srgb_write}: level ours {:?} D3D9 {:?}",
                                  decode(format, &o_level[..16.min(o_level.len())]), decode(format, &theirs[1][..16.min(theirs[1].len())])));
        } else if o_out != &theirs[0] {
            let k = o_out.chunks_exact(16).zip(theirs[0].chunks_exact(16)).position(|(a, b)| a != b).unwrap_or(0);
            failures.push(format!("case {case} format {format} filter {filter} srgb read {srgb_read}: pixel {k} sampled ours {:?} D3D9 {:?}",
                                  decode(116, &o_out[16 * k..16 * k + 16]), decode(116, &theirs[0][16 * k..16 * k + 16])));
        }
    }
    eprintln!("{} of {compared} compared cases differ; formats the driver refuses as render-target textures: {refused:?}", failures.len());
    for f in failures.iter().take(40) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty());
}

/// ps_3_0: oC0 = texld(c0.xyz, s0) on a cube sampler
fn ps_sample_cube() -> Vec<u32> {
    vec![
        0xFFFF_0300,
        0x0200_001F, 0x9800_0000, 0xA00F_0800, // dcl_cube s0
        0x0300_0042, 0x800F_0000, 0xA0E4_0000, 0xA0E4_0800, // texld r0, c0, s0
        0x0200_0001, 0x800F_0800, 0x80E4_0000, // mov oC0, r0
        0x0000_FFFF,
    ]
}

/// A cube render-target texture: each face drawn with its own colour
/// before the texture is bound (level surfaces face * levels + level),
/// then sampled towards each face; compared with the driver as bits.
#[test]
fn cube_render_targets_match_the_d3d9_driver() {
    use d3d9cmd::{TargetTexture, TexKind, TextureBinding, USAGE_RENDERTARGET};
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    // +X, -X, +Y, -Y, +Z, -Z, off the face centres (point sampling)
    let directions = [[1.0f32, 0.3, 0.2, 0.0], [-1.0, 0.3, 0.2, 0.0], [0.3, 1.0, 0.2, 0.0], [0.3, -1.0, 0.2, 0.0], [0.3, 0.2, 1.0, 0.0], [0.3, 0.2, -1.0, 0.0]];
    let out = target(0x100, 116);
    let mut failures = Vec::new();
    for format in [21u32, 113] {
        let tex = TargetTexture { resource: 0x300, kind: TexKind::Cube, width: 4, height: 4, depth: 1, levels: 2, usage: USAGE_RENDERTARGET };
        let face = |f: u32| Target { surface: 0x310 + f, format, width: 4, height: 4, texture: Some(tex), level: 2 * f, samples: 0, quality: 0 };
        let mut draws: Vec<DrawCall> = (0..6u32).map(|f| {
            let v = (f + 1) as f32 / 8.0;
            draw(&[[v, 1.0 - v, 0.5 * v, 1.0]], [Some(face(f)), None, None, None], 0.5, &[])
        }).collect();
        let st = [0u32, 3, 3, 3, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0];
        let binding = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Cube, format, width: 4, height: 4, depth: 1, levels: 2, usage: USAGE_RENDERTARGET, states: st };
        let mut seen = Vec::new();
        for (k, dir) in directions.iter().enumerate() {
            let mut s = sample_draw(binding.clone(), out.clone(), &[]);
            s.pixel_shader = Some((ps_sample_cube()).into());
            std::sync::Arc::make_mut(&mut s.ps_constants).f[0] = dir.map(f32::to_bits);
            draws.truncate(6);
            draws.push(s);
            let clear = clear_of(&[out.clone()], 0);
            let theirs = d3d9.render_targets(&[], Some(&clear), &draws, &[out.clone()]).unwrap_or_else(|e| panic!("D3D9: {e}"));
            let ours = ours(&[], &clear, &draws, &[out.clone()]).unwrap_or_else(|e| panic!("wgpu: {e}"));
            if ours[0] != theirs[0] {
                failures.push(format!("format {format} towards face {k}: ours {:?} D3D9 {:?}", decode(116, &ours[0][..16]), decode(116, &theirs[0][..16])));
            }
            seen.push(theirs[0][..16].to_vec());
        }
        // each face holds its own colour (the comparison is not of blanks)
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 6, "format {format}: the six faces sample distinct colours");
    }
    for f in &failures {
        eprintln!("{f}");
    }
    assert!(failures.is_empty());
}

#[test]
fn a_draw_sampling_its_own_target_matches_the_d3d9_driver() {
    // L59: the game's menu draws into a texture it also samples. wgpu
    // forbids that in one pass; ddgpu samples a copy made before the draw.
    use d3d9cmd::{TargetTexture, TexKind, TextureBinding, USAGE_RENDERTARGET};
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let format = 21u32; // A8R8G8B8
    let tex = TargetTexture { resource: 0x300, kind: TexKind::Flat, width: 4, height: 4, depth: 1, levels: 1, usage: USAGE_RENDERTARGET };
    let level = Target { surface: 0x301, format, width: 4, height: 4, texture: Some(tex), level: 0, samples: 0, quality: 0 };
    let fill = draw(&[[0.25, 0.5, 0.75, 1.0]], [Some(level.clone()), None, None, None], 0.5, &[]);
    // point filtering, clamp: each pixel samples its own texel
    let st = [0u32, 3, 3, 3, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0];
    let binding = TextureBinding { sampler: 0, resource: 0x300, kind: TexKind::Flat, format, width: 4, height: 4, depth: 1, levels: 1, usage: USAGE_RENDERTARGET, states: st };
    let feedback = sample_draw(binding, level.clone(), &[]);
    let clears = [clear_of(&[level.clone()], 0xFF10_2030)];
    let draws = [fill, feedback];
    let reads = [level.clone()];
    let theirs = d3d9.render_cleared(&[], &clears, &draws, &reads).expect("D3D9");
    let mut gpu = ddgpu::WgpuBackend::headless().expect("adapter");
    for c in &clears {
        gpu.clear(c);
    }
    for d in &draws {
        gpu.draw(d);
    }
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    let mine = gpu.read_back(&level).expect("read back");
    assert_eq!(mine, theirs[0]);
}
