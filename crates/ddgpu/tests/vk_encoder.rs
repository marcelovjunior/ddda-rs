//! Production L138 path: byte-exact Vulkan/wgpu comparison. Run serially.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, Vertices};

/// vs_3_0: o0 = v0 (position only)
fn vs() -> Vec<u32> {
    vec![
        0xFFFE_0300,
        0x0200_001F,
        0x8000_0000,
        0x900F_0000, // dcl_position v0
        0x0200_001F,
        0x8000_0000,
        0xE00F_0000, // dcl_position o0
        0x0200_0001,
        0xE00F_0000,
        0x90E4_0000, // mov o0, v0
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
    Target {
        surface,
        format,
        width: 4,
        height: 4,
        texture: None,
        level: 0,
        samples: 0,
        quality: 0,
    }
}

fn depth_target() -> Target {
    Target {
        surface: 0x180,
        format: 75,
        width: 4,
        height: 4,
        texture: None,
        level: 0,
        samples: 0,
        quality: 0,
    }
}

/// A triangle over the whole 4x4 target at depth `z` (float4 positions).
fn triangle(z: f32) -> Vec<u8> {
    [[-1.0f32, -1.0, z, 1.0], [-1.0, 3.0, z, 1.0], [3.0, -1.0, z, 1.0]]
        .iter()
        .flatten()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// The render states every draw sets (the D3D9 device keeps state between
/// draws, so each draw names all it depends on), with `extra` on top.
fn states(extra: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = vec![
        (7, 0),
        (14, 0),
        (23, 4),
        (15, 0),
        (22, 1),
        (27, 0),
        (19, 2),
        (20, 1),
        (171, 1),
        (206, 0),
        (207, 2),
        (208, 1),
        (209, 1),
        (193, 0xFFFF_FFFF),
        (168, 0xF),
        (190, 0xF),
        (191, 0xF),
        (192, 0xF),
        (174, 0),
        (194, 0),
        (195, 0),
        (175, 0),
        (52, 0),
        (185, 0),
        (8, 3),
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
    let mut ps_constants = Constants {
        f: vec![[0; 4]; 224],
        ..Default::default()
    };
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
        vertices: Vertices::Inline {
            bytes: triangle(z),
            stride: 16,
        },
        indices: Indices::None,
        vertex_shader: Some((vs()).into()),
        pixel_shader: Some((ps(c.len() as u32)).into()),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(),
        fvf: 0,
        vs_constants: Constants {
            f: vec![[0; 4]; 256],
            ..Default::default()
        }
        .into(),
        ps_constants: ps_constants.into(),
        textures: vec![].into(),
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
    ClearCall {
        rects: vec![],
        flags: 1,
        colour,
        z: 1.0,
        stencil: 0,
        targets: t,
        depth: None,
        viewport: [0, 0, 4, 4, 0, 0],
        scissor: None,
        srgb: false,
    }
}

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn backend(vk: bool) -> ddgpu::WgpuBackend {
    std::env::set_var("DDGPU_VK", if vk { "1" } else { "0" });
    let mut gpu = ddgpu::WgpuBackend::headless().expect("Vulkan adapter required for this gate");
    std::env::remove_var("DDGPU_VK");
    assert_eq!(gpu.adapter.backend, wgpu::Backend::Vulkan);
    gpu.set_async_submit(true);
    gpu
}

fn compare_case(name: &str, clears: &[ClearCall], draws: &[DrawCall], reads: &[Target]) {
    let mut expected = vec![];
    for vk in [false, true] {
        let mut gpu = backend(vk);
        let before = ddgpu::vk::stats();
        let mut result = vec![];
        // First frame deliberately exercises the initialization fallback;
        // following frames must execute the actual raw encoder.
        for _ in 0..3 {
            for clear in clears {
                gpu.clear(clear);
            }
            for d in draws {
                gpu.draw(d);
            }
            result = reads
                .iter()
                .map(|t| gpu.read_back(t).expect("readback"))
                .collect::<Vec<_>>();
            assert!(gpu.errors.is_empty(), "{name}: {:?}", gpu.errors);
        }
        if !vk {
            expected = result;
            continue;
        }
        assert_eq!(result, expected, "{name}");
        let after = ddgpu::vk::stats();
        assert!(
            after.passes > before.passes,
            "{name}: raw encoder never executed: {after:?}"
        );
        let bytes = result.concat();
        eprintln!(
            "{name}: {} bytes hash={} raw_passes={} raw_draws={} fallback={}",
            bytes.len(),
            blake3::hash(&bytes),
            after.passes - before.passes,
            after.draws - before.draws,
            after.fallback_passes - before.fallback_passes
        );
    }
}

fn primitive_case(kind: usize) {
    let _lock = SERIAL.lock().unwrap();
    let t = target(0x100, 21);
    let clear = clear_of(&[t.clone()], 0xff123456);
    {
        let mut d = draw(
            &[[0.8, 0.2, 0.4, 1.0]],
            [Some(t.clone()), None, None, None],
            0.4,
            &[],
        );
        if kind > 0 {
            d.indices = Indices::Inline {
                bytes: if kind % 2 == 1 {
                    [0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes).collect()
                } else {
                    [0u32, 1, 2].into_iter().flat_map(u32::to_le_bytes).collect()
                },
                format: if kind % 2 == 1 { 101 } else { 102 },
            };
        }
        if kind > 2 {
            d.primitive = 5;
        }
        if kind == 4 {
            d.render_states = states(&[(174, 1), (193, 0xff808080)]).into();
            d.scissor = [1, 0, 4, 3];
        }
        compare_case(&format!("primitive-{kind}"), &[clear.clone()], &[d], &[t.clone()]);
    }
}

#[test]
fn plain() {
    primitive_case(0);
}
#[test]
fn indexed_u16() {
    primitive_case(1);
}
#[test]
fn indexed_u32() {
    primitive_case(2);
}
#[test]
fn strip_u16() {
    primitive_case(3);
}
#[test]
fn strip_u32_scissor() {
    primitive_case(4);
}

fn blend_case(index: usize) {
    let _lock = SERIAL.lock().unwrap();
    let a = target(0x100, 21);
    let b = target(0x101, 113);
    let z = depth_target();
    let mut clear = clear_of(&[a.clone(), b.clone()], 0xff204080);
    clear.depth = Some(z.clone());
    clear.flags = 7;
    let colors = [Some(a.clone()), Some(b.clone()), None, None];
    let modes = [
        vec![],
        vec![(27, 1), (19, 5), (20, 6)],
        vec![
            (27, 1),
            (19, 14),
            (20, 15),
            (193, 0x804060a0),
            (206, 1),
            (207, 2),
            (208, 2),
            (209, 1),
        ],
        vec![(27, 1), (171, 3), (19, 2), (20, 2)],
    ];
    let blend = modes[index].clone();
    {
        let mut states = blend;
        states.extend([(7, 1), (14, 1), (23, 4)]);
        let mut near = draw(
            &[[0.8, 0.2, 0.4, 0.5], [0.5, 0.75, 0.25, 0.6]],
            colors.clone(),
            0.3,
            &states,
        );
        near.depth = Some(z.clone());
        let mut far = draw(
            &[[0.0, 1.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]],
            colors.clone(),
            0.8,
            &states,
        );
        far.depth = Some(z.clone());
        compare_case(
            &format!("MRT-depth-blend-{states:?}"),
            &[clear.clone()],
            &[near, far],
            &[a.clone(), b.clone()],
        );
    }
}

#[test]
fn mrt_depth() {
    blend_case(0);
}
#[test]
fn blend_alpha() {
    blend_case(1);
}
#[test]
fn blend_constant_separate_alpha() {
    blend_case(2);
}
#[test]
fn blend_reverse_subtract() {
    blend_case(3);
}

#[test]
fn sampled_render_target_and_many_tiny_passes() {
    use d3d9cmd::{TargetTexture, TexKind, TextureBinding};
    let _lock = SERIAL.lock().unwrap();
    let targets = [0x200, 0x201].map(|resource| {
        let mut t = target(resource + 0x100, 116);
        t.texture = Some(TargetTexture {
            resource,
            kind: TexKind::Flat,
            width: 4,
            height: 4,
            levels: 1,
            depth: 1,
            usage: 1,
        });
        t
    });
    let mut sampler = [0; 14];
    for n in [1, 2, 3] {
        sampler[n] = 3;
    }
    for n in [5, 6] {
        sampler[n] = 1;
    }
    // texld r0,c0,s0; mad oC0,r0,c1,c2. Every float32 result depends
    // on the previous pass; changing ordering, constants or a barrier matters.
    let shader = vec![
        0xffff0300, 0x0200001f, 0x90000000, 0xa00f0800, 0x03000042, 0x800f0000, 0xa0e40000, 0xa0e40800,
        0x04000004, 0x800f0800, 0x80e40000, 0xa0e40001, 0xa0e40002, 0xffff,
    ];
    let draws = (0..900)
        .map(|n| {
            let src = &targets[n % 2];
            let dst = &targets[1 - n % 2];
            let mut d = draw(&[[0.0; 4]], [Some(dst.clone()), None, None, None], 0.0, &[]);
            d.pixel_shader = Some(shader.clone().into());
            d.ps_constants = Constants {
                f: vec![
                    [0.5f32; 4].map(f32::to_bits),
                    [0.5f32, 0.25, 0.75, 0.5].map(f32::to_bits),
                    [(n % 7) as f32 / 16.0, (n % 11) as f32 / 32.0, 0.0625, 0.5].map(f32::to_bits),
                ],
                ..Default::default()
            }
            .into();
            d.textures = vec![TextureBinding {
                sampler: 0,
                resource: src.texture.unwrap().resource,
                kind: TexKind::Flat,
                format: 116,
                width: 4,
                height: 4,
                depth: 1,
                levels: 1,
                usage: 1,
                states: sampler,
            }]
            .into();
            d
        })
        .collect::<Vec<_>>();
    compare_case(
        "900 dependent float32 ping-pong passes",
        &[
            clear_of(&[targets[0].clone()], 0xff204080),
            clear_of(&[targets[1].clone()], 0xff102030),
        ],
        &draws,
        &targets,
    );
}

#[test]
fn resource_release_and_recreation() {
    let _lock = SERIAL.lock().unwrap();
    let t = target(0x100, 21);
    for vk in [false, true] {
        let mut gpu = backend(vk);
        for iteration in 0..8 {
            let color = if iteration % 2 == 0 {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                [0.0, 1.0, 0.0, 1.0]
            };
            let d = draw(&[color], [Some(t.clone()), None, None, None], 0.0, &[]);
            for _ in 0..3 {
                gpu.draw(&d);
                gpu.present(&t);
            }
            let bytes = gpu.read_back(&t).unwrap();
            assert!(bytes.chunks_exact(4).all(|p| p
                == if iteration % 2 == 0 {
                    [0, 0, 255, 255]
                } else {
                    [0, 255, 0, 255]
                }));
            gpu.release(t.surface);
        }
    }
}

#[test]
fn raw_and_query_fallback_with_parallel_chunks() {
    let _lock = SERIAL.lock().unwrap();
    let targets = [target(0x100, 21), target(0x101, 21)];
    let mut reference = None;
    for (vk, workers) in [(false, 1), (true, 1), (true, 4)] {
        let mut gpu = backend(vk);
        gpu.set_encode_threads(workers);
        gpu.draws_per_submit = 16;
        let start = ddgpu::vk::stats();
        let mut outputs = vec![];
        for frame in 0..3 {
            for target in &targets {
                gpu.clear(&clear_of(&[target.clone()], 0));
            }
            for n in 0..65 {
                let t = &targets[n % 2];
                let d = draw(
                    &[[(n % 7) as f32 / 8.0, frame as f32 / 4.0, 0.75, 1.0]],
                    [Some(t.clone()), None, None, None],
                    0.0,
                    &[],
                );
                if n % 13 == 0 {
                    gpu.query_begin(0x900);
                }
                gpu.draw(&d);
                if n % 13 == 0 {
                    gpu.query_end(0x900);
                    assert_eq!(gpu.query_result(0x900), Some(16));
                }
                if n % 17 == 0 {
                    gpu.copy(&d3d9cmd::CopyCall {
                        src: t.clone(),
                        dst: targets[1 - n % 2].clone(),
                        src_rect: [0; 4],
                        dst_rect: [0; 4],
                        filter: 1,
                    });
                }
            }
            for t in &targets {
                outputs.push(gpu.read_back(t).unwrap());
            }
        }
        assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
        if let Some(expected) = &reference {
            assert_eq!(&outputs, expected);
        } else {
            reference = Some(outputs);
        }
        if vk {
            let end = ddgpu::vk::stats();
            assert!(end.passes > start.passes);
            assert!(end.fallback_passes >= start.fallback_passes + 15);
            assert_eq!(end.encode_errors, start.encode_errors);
        }
    }
}

/// Includes DrawCall preparation, ring upload, finisher encoding and queue
/// submission. Readback drains the GPU between samples, outside the timer.
#[test]
#[ignore = "manual bounded production microbenchmark"]
fn production_cost() {
    let _lock = SERIAL.lock().unwrap();
    for (count, passes, textured) in [(5000, 1, false), (5000, 1, true), (900, 900, false)] {
        let mut reference = None;
        for vk in [false, true] {
            let mut gpu = backend(vk);
            gpu.set_encode_threads(1);
            gpu.read_frames = false;
            gpu.draws_per_submit = 8192;
            let targets = [target(0x100, 21), target(0x101, 21)];
            let draws = (0..count)
                .map(|n| {
                    let mut d = draw(
                        &[[(n % 7) as f32 / 8.0, 0.5, 0.75, 1.0]],
                        [
                            Some(targets[if passes == 1 { 0 } else { n % 2 }].clone()),
                            None,
                            None,
                            None,
                        ],
                        0.0,
                        &[],
                    );
                    d.vertices = Vertices::Streams(vec![(0, 0x300, 0, 16, 1)].into());
                    if textured {
                        d.pixel_shader = Some(
                            vec![
                                0xffff0300, 0x0200001f, 0x90000000, 0xa00f0800, 0x03000042, 0x800f0000,
                                0xa0e40000, 0xa0e40800, 0x03000005, 0x800f0800, 0x80e40000, 0xa0e40001,
                                0xffff,
                            ]
                            .into(),
                        );
                        d.ps_constants = Constants {
                            f: vec![
                                [0.5f32; 4].map(f32::to_bits),
                                [(n % 7) as f32 / 8.0, 0.5, 0.75, 0.5].map(f32::to_bits),
                            ],
                            ..Default::default()
                        }
                        .into();
                        let mut extra = vec![(168, (n % 10 + 1) as u32)];
                        match (n % 40) / 10 {
                            0 => {}
                            1 => extra.extend([(27, 1), (19, 5), (20, 6)]),
                            2 => extra.extend([(27, 1), (19, 2), (20, 2)]),
                            _ => extra.extend([(27, 1), (19, 14), (20, 15), (193, 0x80406080)]),
                        }
                        d.render_states = states(&extra).into();
                        let mut sampler = [0; 14];
                        for i in [1, 2, 3] {
                            sampler[i] = 3;
                        }
                        for i in [5, 6] {
                            sampler[i] = 1;
                        }
                        d.textures = vec![d3d9cmd::TextureBinding {
                            sampler: 0,
                            resource: 0x1000 + (n % 200) as u32,
                            kind: d3d9cmd::TexKind::Flat,
                            format: 21,
                            width: 1,
                            height: 1,
                            depth: 1,
                            levels: 1,
                            usage: 0,
                            states: sampler,
                        }]
                        .into();
                    }
                    d
                })
                .collect::<Vec<_>>();
            gpu.upload(d3d9cmd::Upload::Buffer {
                resource: 0x300,
                bytes: triangle(0.0),
            });
            if textured {
                for n in 0..200 {
                    gpu.upload(d3d9cmd::Upload::Image {
                        resource: 0x1000 + n,
                        index: 0,
                        format: 21,
                        width: 1,
                        height: 1,
                        depth: 1,
                        pitch: 4,
                        slice: 4,
                        bytes: vec![(n % 251) as u8, ((n * 3) % 251) as u8, ((n * 7) % 251) as u8, 255],
                    });
                }
            }
            let mut samples = vec![];
            let mut pixels = vec![];
            let before = ddgpu::vk::stats();
            for frame in 0..12 {
                let start = std::time::Instant::now();
                for d in &draws {
                    gpu.draw(d);
                }
                gpu.present(&targets[0]);
                let elapsed = start.elapsed().as_secs_f64() * 1e6 / count as f64;
                if frame >= 3 {
                    samples.push(elapsed);
                }
                pixels = gpu.read_back(&targets[0]).unwrap();
            }
            assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
            if vk {
                assert!(ddgpu::vk::stats().draws > before.draws);
            }
            let pipelines = gpu.pipeline_creation_counts().1;
            assert_eq!(pipelines, if textured { 40 } else { 1 });
            if let Some(expected) = &reference {
                assert_eq!(&pixels, expected);
            } else {
                reference = Some(pixels.clone());
            }
            samples.sort_by(f64::total_cmp);
            eprintln!("L138 production draws={count} passes={passes} pipelines={pipelines} sampled_sets={} vk={vk} median={:.3}us/unit range={:.3}..{:.3} hash={}",if textured {200} else {0},samples[4],samples[0],samples[8],blake3::hash(&pixels));
        }
    }
}
