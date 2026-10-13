//! Multisampled targets (the AntiAlias option, brief L31): draws into 2, 4
//! and 8 sample targets, their resolves through StretchRect, depth-stencil
//! StretchRect and the multisample render states, compared with the host's
//! D3D9 driver. Skips without D3D9 or an adapter.
use d3d9cmd::{ClearCall, Constants, CopyCall, DrawCall, GraphicsBackend, Indices, Target, Vertices};

const W: u32 = 8;
const H: u32 = 8;

/// vs_3_0: o0 = v0
fn vs() -> Vec<u32> {
    vec![0xFFFE_0300, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0000, 0xE00F_0000, 0x0200_0001, 0xE00F_0000, 0x90E4_0000, 0x0000_FFFF]
}

/// ps_3_0: oC0 = c0
fn ps() -> Vec<u32> {
    vec![0xFFFF_0300, 0x0200_0001, 0x800F_0800, 0xA0E4_0000, 0x0000_FFFF]
}

fn target(surface: u32, format: u32, samples: u32) -> Target {
    Target { surface, format, width: W, height: H, texture: None, level: 0, samples, quality: 0 }
}

/// A triangle with slanted edges (partly covered pixels) at depth `z`.
fn triangle(points: [[f32; 2]; 3], z: f32) -> Vec<u8> {
    points.iter().flat_map(|p| [p[0], p[1], z, 1.0]).flat_map(|f| f.to_le_bytes()).collect()
}

fn states(extra: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = vec![
        (7, 0), (14, 0), (23, 4), (15, 0), (22, 1), (27, 0), (19, 2), (20, 1), (171, 1), (206, 0), (168, 0xF), (174, 0),
        (194, 0), (195, 0), (175, 0), (52, 0), (185, 0), (8, 3), (161, 1), (162, 0xFFFF_FFFF),
    ];
    for &(k, val) in extra {
        v.retain(|s| s.0 != k);
        v.push((k, val));
    }
    v.sort();
    v
}

fn draw(colour: [f32; 4], points: [[f32; 2]; 3], z: f32, rt: &Target, depth: Option<&Target>, extra: &[(u32, u32)]) -> DrawCall {
    let mut c = Constants { f: vec![[0; 4]; 224], ..Default::default() };
    c.f[0] = colour.map(f32::to_bits);
    DrawCall {
        primitive: 4, primitives: 1, start: 0, base_vertex: 0, min_index: 0, vertex_count: 3,
        vertices: Vertices::Inline { bytes: triangle(points, z), stride: 16 },
        indices: Indices::None,
        vertex_shader: Some((vs()).into()), pixel_shader: Some((ps()).into()),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(), fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
        ps_constants: c.into(), textures: vec![].into(),
        render_states: states(extra).into(),
        viewport: [0, 0, W, H, 0f32.to_bits(), 1f32.to_bits()],
        scissor: [0, 0, W as i32, H as i32],
        targets: [Some(rt.clone()), None, None, None],
        depth: depth.cloned(),
    }
}

fn clear(rt: &Target, depth: Option<&Target>, colour: u32) -> ClearCall {
    ClearCall {
        rects: vec![], flags: if depth.is_some() { 7 } else { 1 }, colour, z: 1.0, stencil: 0,
        targets: [Some(rt.clone()), None, None, None], depth: depth.cloned(),
        viewport: [0, 0, W, H, 0, 1f32.to_bits()], scissor: None, srgb: false,
    }
}

fn copy(src: &Target, dst: &Target, src_rect: [i32; 4], dst_rect: [i32; 4]) -> CopyCall {
    CopyCall { src: src.clone(), src_rect, dst: dst.clone(), dst_rect, filter: 0 }
}

/// The same commands on the wgpu backend; the reads (single-sampled).
fn ours(clears: &[ClearCall], draws: &[DrawCall], copies: &[CopyCall], reads: &[Target]) -> Result<Vec<Vec<u8>>, String> {
    let mut gpu = ddgpu::WgpuBackend::headless().ok_or("no adapter")?;
    for c in clears {
        gpu.clear(c);
    }
    for d in draws {
        gpu.draw(d);
    }
    for c in copies {
        gpu.copy(c);
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

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn coord(&mut self) -> f32 {
        (self.next() % 2001) as f32 / 1000.0 - 1.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() % 1001) as f32 / 1000.0
    }
}

/// Two triangles drawn into a multisampled target (2, 4, 8 samples;
/// A8R8G8B8 and A16B16G16R16F) with a multisampled D24S8 (the second
/// crossing the first in depth), resolved by StretchRect (D3DTEXF_NONE)
/// into a single-sampled target of the same format, which is compared.
#[test]
fn multisampled_draws_resolve_like_the_d3d9_driver() {
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let mut rng = Rng(0x00A5_A031);
    let mut failures = Vec::new();
    for case in 0..48 {
        let samples = [2u32, 4, 8][case % 3];
        let format = [21u32, 113][(case / 3) % 2];
        let (ms, ds, resolved) = (target(0x100, format, samples), target(0x180, 75, samples), target(0x101, format, 0));
        let p = |rng: &mut Rng| [[rng.coord(), rng.coord()], [rng.coord(), rng.coord()], [rng.coord(), rng.coord()]];
        let blend = rng.next() % 2;
        let draws = vec![
            draw([rng.unit(), rng.unit(), rng.unit(), rng.unit()], p(&mut rng), 0.3, &ms, Some(&ds), &[(7, 1), (14, 1), (23, 4), (22, 1)]),
            draw([rng.unit(), rng.unit(), rng.unit(), rng.unit()], p(&mut rng), 0.5, &ms, Some(&ds), &[(7, 1), (14, 1), (23, 4), (22, 1), (27, blend), (19, 5), (20, 6)]),
        ];
        let clears = [clear(&ms, Some(&ds), rng.next())];
        let copies = [copy(&ms, &resolved, [0; 4], [0; 4])];
        let theirs = match d3d9.render_copied(&[], &clears, &draws, &copies, &[resolved.clone()]) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("case {case} ({samples}x, format {format}): D3D9 refused: {e}"));
                continue;
            }
        };
        match ours(&clears, &draws, &copies, &[resolved.clone()]) {
            Ok(o) if o == theirs => {}
            Ok(o) => {
                let max = o[0].iter().zip(&theirs[0]).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap();
                let bpp = o[0].len() / 64;
                let first = |b: &[u8]| b.chunks_exact(bpp).map(|p| if bpp == 4 { p[2] as u32 } else { u16::from_le_bytes([p[0], p[1]]) as u32 }).collect::<Vec<_>>();
                failures.push(format!("case {case} ({samples}x, format {format}): bytes differ (max byte diff {max})
  ours {:?}
  D3D9 {:?}", first(&o[0]), first(&theirs[0])));
            }
            Err(e) => failures.push(format!("case {case}: backend error {e}")),
        }
    }
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} of 48 cases differ", failures.len());
}

/// What the driver does with multisampled targets (prints; run with
/// --ignored --nocapture): CSAA qualities and lockable multisampled
/// targets at creation, resolves with rectangles and stretching, depth
/// StretchRect single- and multisampled, MULTISAMPLEANTIALIAS off and
/// MULTISAMPLEMASK.
#[test]
#[ignore]
fn explore_multisampling() {
    let Some(mut d3d9) = available() else { return };
    let tri = [[-0.9f32, -0.7], [0.3, 0.95], [0.85, -0.2]];
    let red = [1.0f32, 0.0, 0.0, 1.0];
    // creation: quality levels beyond the profile's count
    for (samples, quality) in [(4u32, 0u32), (4, 1), (4, 2), (8, 0), (8, 2), (16, 0), (1, 0), (1, 3), (1, 4)] {
        let ms = Target { quality, ..target(0x100, 21, samples) };
        let r = d3d9.render_targets(&[], Some(&clear(&ms, None, 0)), &[], &[]);
        eprintln!("CreateRenderTarget A8R8G8B8 {samples} samples quality {quality}: {:?}", r.map(|_| "ok"));
    }
    // resolves with rectangles
    let ms = target(0x100, 21, 4);
    let plain = target(0x101, 21, 0);
    for (name, sr, dr) in [("whole", [0; 4], [0; 4]), ("same size rects", [0, 0, 4, 4], [2, 2, 6, 6]), ("stretch", [0, 0, 4, 4], [0, 0, 8, 8])] {
        let r = d3d9.render_copied(&[], &[clear(&ms, None, 0xFF00_00FF), clear(&plain, None, 0xFF00_FF00)], &[draw(red, tri, 0.5, &ms, None, &[])],
                                   &[copy(&ms, &plain, sr, dr)], &[plain.clone()]);
        eprintln!("resolve {name}: {}", match r { Ok(b) => format!("ok, first row {:?}", &b[0][..32]), Err(e) => e });
    }
    // depth-stencil StretchRect
    for (name, ss, ds) in [("D24S8 -> D24S8", 0u32, 0u32), ("D24S8 4x -> D24S8", 4, 0), ("D24S8 4x -> D24S8 4x", 4, 4)] {
        let rt = target(0x100, 21, ss);
        let (src, dst) = (target(0x180, 75, ss), target(0x181, 75, ds));
        let r = d3d9.render_copied(&[], &[clear(&rt, Some(&src), 0)], &[draw(red, tri, 0.25, &rt, Some(&src), &[(7, 1), (14, 1), (23, 8)])],
                                   &[copy(&src, &dst, [0; 4], [0; 4])], &[]);
        eprintln!("depth StretchRect {name}: {:?}", r.map(|_| "ok"));
    }
    // MULTISAMPLEANTIALIAS off, MULTISAMPLEMASK
    for (name, extra) in [("antialias on", vec![]), ("antialias off", vec![(161u32, 0u32)]), ("mask 0x1", vec![(162, 1)]), ("mask 0x5", vec![(162, 5)])] {
        let r = d3d9.render_copied(&[], &[clear(&ms, None, 0xFF00_0000)], &[draw(red, tri, 0.5, &ms, None, &extra)],
                                   &[copy(&ms, &plain, [0; 4], [0; 4])], &[plain.clone()]);
        let reds = |b: &[u8]| b.chunks_exact(4).map(|p| p[2]).collect::<Vec<_>>();
        match r {
            Ok(b) => {
                let o = ours(&[clear(&ms, None, 0xFF00_0000)], &[draw(red, tri, 0.5, &ms, None, &extra)], &[copy(&ms, &plain, [0; 4], [0; 4])], &[plain.clone()]);
                eprintln!("{name}: D3D9 red {:?}\n    ours {:?}", reds(&b[0]), o.map(|o| reds(&o[0])));
            }
            Err(e) => eprintln!("{name}: {e}"),
        }
    }
}

/// The backend's own multisample paths, without the driver: a 4x draw
/// resolves with antialiased edges, and depth-stencil StretchRect copies
/// depth and stencil exactly (a probe on the copy lights the pixels a
/// probe on the source lights), single-sampled and from 4x.
#[test]
fn the_backend_resolves_and_copies_multisampled_targets() {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else {
        eprintln!("skipped: no adapter");
        return;
    };
    let tri = [[-0.9f32, -0.7], [0.3, 0.95], [0.85, -0.2]];
    let (ms, plain) = (target(0x100, 21, 4), target(0x101, 21, 0));
    gpu.clear(&clear(&ms, None, 0xFF00_0000));
    gpu.draw(&draw([1.0, 0.0, 0.0, 1.0], tri, 0.5, &ms, None, &[]));
    gpu.copy(&copy(&ms, &plain, [0; 4], [0; 4]));
    let reds: Vec<u8> = gpu.read_back(&plain).expect("resolved").chunks_exact(4).map(|p| p[2]).collect();
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    assert!(gpu.read_back(&ms).is_none(), "a multisampled target is not read back");
    assert!(reds.contains(&255) && reds.contains(&0) && reds.iter().any(|&r| r > 0 && r < 255), "antialiased edges: {reds:?}");
    for samples in [0u32, 4] {
        let rt = target(0x110, 21, samples);
        let (src, dst) = (target(0x180, 75, samples), target(0x181, 75, 0));
        let probe_rt = target(0x111, 21, 0);
        let mut g = ddgpu::WgpuBackend::headless().unwrap();
        g.clear(&clear(&rt, Some(&src), 0));
        // depth 0.25 and stencil 0x5A where the triangle covers
        g.draw(&draw([1.0; 4], tri, 0.25, &rt, Some(&src), &[(7, 1), (14, 1), (23, 8), (52, 1), (56, 8), (55, 3), (57, 0x5A), (58, 0xFF), (59, 0xFF)]));
        g.copy(&copy(&src, &dst, [0; 4], [0; 4]));
        let probe = |g: &mut ddgpu::WgpuBackend, ds: &Target, func: u32| {
            g.clear(&clear(&probe_rt, None, 0xFF00_0000));
            g.draw(&draw([0.0, 1.0, 0.0, 1.0], [[-1.0, -1.0], [-1.0, 3.0], [3.0, -1.0]], 0.5, &probe_rt, Some(ds),
                         &[(7, 1), (14, 0), (23, func), (52, 1), (56, 3), (57, 0x5A), (58, 0xFF), (59, 0)]));
            g.read_back(&probe_rt).expect("probe")
        };
        // a single-sampled source is probed directly; a 4x source through
        // the copy only (the probe target is single-sampled)
        let on_copy = probe(&mut g, &dst, 5);
        assert!(g.errors.is_empty(), "{samples}x: {:?}", g.errors);
        let lit: Vec<usize> = on_copy.chunks_exact(4).enumerate().filter(|(_, p)| p[1] == 255).map(|(k, _)| k).collect();
        assert!(!lit.is_empty() && lit.len() < 64, "{samples}x: the copy holds the triangle's depth and stencil: {lit:?}");
        if samples == 0 {
            let on_source = probe(&mut g, &src, 5);
            assert_eq!(on_copy, on_source, "the copy equals the source");
        }
    }
}

/// Depth-stencil StretchRect (D24S8, whole surfaces, outside the scene),
/// as the game's draw-command case 5 does it: single-sampled, and from 2x,
/// 4x and 8x to 1x, and 4x to 4x. A triangle writes depth 0.25 and stencil
/// 0x5A into the source; after the copy a probe draw over the whole target
/// (depth and stencil tests against the copy) shows what arrived.
#[test]
fn depth_stencil_copies_match_the_d3d9_driver() {
    let Some(mut d3d9) = available() else {
        eprintln!("skipped: no D3D9 device or adapter");
        return;
    };
    let tri = [[-0.9f32, -0.7], [0.3, 0.95], [0.85, -0.2]];
    let full = [[-1.0f32, -1.0], [-1.0, 3.0], [3.0, -1.0]];
    let mut failures = Vec::new();
    for (ss, ds) in [(0u32, 0u32), (2, 0), (4, 0), (8, 0), (4, 4)] {
        for (zfunc, sfunc) in [(2u32, 8u32), (5, 8), (8, 3), (8, 6), (2, 3)] {
            let (rt, src, dst) = (target(0x100, 21, ss), target(0x180, 75, ss), target(0x181, 75, ds));
            let (probe_rt, resolved) = (target(0x102, 21, ds), target(0x103, 21, 0));
            let write = draw([1.0; 4], tri, 0.25, &rt, Some(&src), &[(7, 1), (14, 1), (23, 8), (52, 1), (56, 8), (55, 3), (57, 0x5A), (58, 0xFF), (59, 0xFF)]);
            let probe = draw([0.0, 1.0, 0.0, 1.0], full, 0.5, &probe_rt, Some(&dst),
                             &[(7, 1), (14, 0), (23, zfunc), (52, 1), (56, sfunc), (57, 0x5A), (58, 0xFF), (59, 0), (53, 1), (54, 1), (55, 1)]);
            let clears = [clear(&probe_rt, Some(&dst), 0xFF00_0000), clear(&rt, Some(&src), 0xFF00_0000)];
            let mut copies = vec![(1usize, copy(&src, &dst, [0; 4], [0; 4]))];
            let read = if ds != 0 {
                copies.push((2, copy(&probe_rt, &resolved, [0; 4], [0; 4])));
                resolved.clone()
            } else {
                probe_rt.clone()
            };
            let theirs = match d3d9.render_placed(&[], &clears, &[write.clone(), probe.clone()], &copies, &[read.clone()]) {
                Ok(mut r) => r.remove(0),
                Err(e) => {
                    failures.push(format!("{ss}x -> {ds}x: D3D9: {e}"));
                    continue;
                }
            };
            let mut gpu = ddgpu::WgpuBackend::headless().unwrap();
            for c in &clears {
                gpu.clear(c);
            }
            gpu.draw(&write);
            gpu.copy(&copies[0].1);
            gpu.draw(&probe);
            if ds != 0 {
                gpu.copy(&copies[1].1);
            }
            let ours = gpu.read_back(&read);
            let lit = |b: &[u8]| b.chunks_exact(4).enumerate().filter(|(_, p)| p[1] > 0).map(|(k, _)| k).collect::<Vec<_>>();
            if !gpu.errors.is_empty() {
                failures.push(format!("{ss}x -> {ds}x: backend {:?}", gpu.errors));
            } else if ours.as_ref() != Some(&theirs) {
                failures.push(format!("{ss}x -> {ds}x zfunc {zfunc} sfunc {sfunc}: ours {:?} / D3D9 {:?}", ours.map(|o| lit(&o)), lit(&theirs)));
            }
        }
    }
    for f in &failures {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} depth copies differ", failures.len());
}
