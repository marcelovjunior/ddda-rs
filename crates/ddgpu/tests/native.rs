//! Synthetic native/D3D9 backend/host D3D9 pixel equivalence. No game assets.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, Vertices};
use ddgpu::native::*;
fn vs() -> Vec<u32> {
    vec![
        0xfffe0300, 0x0200001f, 0x80000000, 0x900f0000, 0x0200001f, 0x80000000, 0xe00f0000, 0x02000001,
        0xe00f0000, 0x90e40000, 0xffff,
    ]
}
fn ps() -> Vec<u32> {
    vec![0xffff0300, 0x02000001, 0x800f0800, 0xa0e40000, 0xffff]
}
fn target(surface: u32) -> Target {
    Target {
        surface,
        format: 21,
        width: 8,
        height: 8,
        texture: None,
        level: 0,
        samples: 0,
        quality: 0,
    }
}
fn vertices() -> Vec<u8> {
    [
        [-1.0f32, -1.0, 0.5, 1.0],
        [-1.0, 3.0, 0.5, 1.0],
        [3.0, -1.0, 0.5, 1.0],
    ]
    .into_iter()
    .flatten()
    .flat_map(f32::to_le_bytes)
    .collect()
}
fn clear(t: &Target) -> ClearCall {
    ClearCall {
        rects: vec![],
        flags: 1,
        colour: 0xff0000ff,
        z: 1.0,
        stencil: 0,
        targets: [Some(t.clone()), None, None, None],
        depth: None,
        viewport: [0, 0, 8, 8, 0, 0],
        scissor: None,
        srgb: false,
    }
}
fn call(t: &Target, indexed: bool, blended: bool, scissor: bool, colour: [f32; 4]) -> DrawCall {
    let mut states = vec![
        (7, 0),
        (8, 3),
        (15, 0),
        (19, 5),
        (20, 6),
        (22, 1),
        (27, blended as u32),
        (168, 15),
        (171, 1),
        (174, scissor as u32),
    ];
    states.sort();
    DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Inline {
            bytes: vertices(),
            stride: 16,
        },
        indices: if indexed {
            Indices::Inline {
                bytes: vec![0, 0, 1, 0, 2, 0],
                format: 101,
            }
        } else {
            Indices::None
        },
        vertex_shader: Some(vs().into()),
        pixel_shader: Some(ps().into()),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(),
        fvf: 0,
        vs_constants: Constants::default().into(),
        ps_constants: Constants {
            f: vec![colour.map(f32::to_bits)],
            ..Default::default()
        }
        .into(),
        textures: vec![].into(),
        render_states: states.into(),
        viewport: [0, 0, 8, 8, 0, 1f32.to_bits()],
        scissor: [1, 1, 5, 6],
        targets: [Some(t.clone()), None, None, None],
        depth: None,
    }
}
#[test]
fn native_matches_d3d9_backend_and_driver() {
    compare_constant_draws(false, false);
}
#[test]
fn native_compact_matches_d3d9_backend_and_driver() {
    compare_constant_draws(true, false);
}
#[test]
fn native_resident_bundles_match_d3d9_backend_and_driver() {
    compare_constant_draws(false, true);
    compare_constant_draws(true, true);
}
fn compare_constant_draws(compact: bool, resident: bool) {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else {
        eprintln!("SKIP no GPU");
        return;
    };
    let mut oracle = d3d9ref::Oracle::new().expect("host D3D9 driver");
    let native_target = target(0x100);
    let legacy_target = target(0x200);
    let mut device = Device::from_backend(&mut gpu);
    let view = device.import_target(&mut gpu, &native_target, false).unwrap();
    let make_shader = if compact {
        Device::create_compact_shader
    } else {
        Device::create_shader
    };
    let vertex_shader = make_shader(&mut device, &vs(), Stage::Vertex).unwrap();
    let pixel_shader = make_shader(&mut device, &ps(), Stage::Pixel).unwrap();
    let vl = device.shader_constant_layout(vertex_shader);
    let pl = device.shader_constant_layout(pixel_shader);
    let input = device
        .create_input_layout(InputLayoutDesc {
            streams: vec![InputStream {
                stride: 16,
                step: wgpu::VertexStepMode::Vertex,
            }],
            elements: vec![InputElement {
                stream: 0,
                offset: 0,
                format: wgpu::VertexFormat::Float32x4,
                semantic: (0, 0),
                location: 0,
            }],
        })
        .unwrap();
    let depth_stencil = device.create_depth_stencil(Default::default());
    let vb = device.create_buffer(&vertices(), wgpu::BufferUsages::VERTEX);
    let ib = device.create_buffer(&[0, 0, 1, 0, 2, 0], wgpu::BufferUsages::INDEX);
    let mut pipelines = vec![];
    for blended in [false, true] {
        for scissor in [false, true] {
            let mut blend_desc = BlendDesc::default();
            blend_desc.targets[0].enable = blended;
            blend_desc.targets[0].colour = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            };
            blend_desc.targets[0].alpha = blend_desc.targets[0].colour;
            let blend = device.create_blend(blend_desc);
            let rasterizer = device.create_rasterizer(RasterizerDesc {
                scissor_enable: scissor,
                ..Default::default()
            });
            let desc = PipelineDesc {
                vertex_shader,
                pixel_shader,
                input,
                blend,
                rasterizer,
                depth_stencil,
                colours: [Some(wgpu::TextureFormat::Bgra8Unorm), None, None, None],
                depth: None,
                samples: 1,
                sample_mask: !0,
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index: None,
            };
            let p = device.create_pipeline(desc).unwrap();
            assert_eq!(device.create_pipeline(desc).unwrap(), p, "handle tuple cache hit");
            let sets = [
                device.create_texture_set(p, Stage::Pixel, &[]).unwrap(),
                device.create_texture_set(p, Stage::Vertex, &[]).unwrap(),
            ];
            pipelines.push((p, sets));
        }
    }
    assert_eq!(device.pipeline_count(), 4);
    assert_eq!(
        device.texture_set_count(),
        2,
        "empty sets shared across pipeline states"
    );
    let mut recorder = Recorder::with_resident_constants(&device, 128 * 1024, resident);
    recorder.enable_bundles(resident);
    let pc = recorder.create_constant_buffer_for_shader(pixel_shader);
    let vc = recorder.create_constant_buffer_for_shader(vertex_shader);
    let mut vs_bytes = vec![0; vl.byte_len];
    vs_bytes[vl.extra_offset..vl.extra_offset + 4].copy_from_slice(&(1.0f32 / 8.0).to_le_bytes());
    vs_bytes[vl.extra_offset + 4..vl.extra_offset + 8].copy_from_slice(&(-1.0f32 / 8.0).to_le_bytes());
    recorder.set_constants(pc);
    recorder.set_constants(vc);
    recorder.set_vertex_buffer(
        0,
        BufferSlice {
            buffer: vb,
            offset: 0,
            size: 48,
        },
    );
    recorder.set_index_buffer(
        BufferSlice {
            buffer: ib,
            offset: 0,
            size: 6,
        },
        wgpu::IndexFormat::Uint16,
    );
    recorder.set_scissor([1, 1, 4, 5]);
    for (case, (indexed, blended, scissor)) in (0..8).map(|n| (n, (n & 1 != 0, n & 2 != 0, n & 4 != 0))) {
        let colour = [0.8, 0.2, case as f32 / 8.0, 0.5];
        let d = call(&legacy_target, indexed, blended, scissor, colour);
        let reference = oracle
            .render(
                &[],
                Some(&clear(&legacy_target)),
                std::slice::from_ref(&d),
                &legacy_target,
            )
            .unwrap();
        gpu.clear(&clear(&legacy_target));
        gpu.draw(&d);
        gpu.present(&legacy_target);
        assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
        assert_eq!(
            gpu.last_frame.as_ref().unwrap().rgba,
            reference.rgba,
            "D3D9 backend case {case}"
        );
        let (p, sets) = pipelines[blended as usize * 2 + scissor as usize];
        recorder.set_pipeline(p);
        for set in sets {
            recorder.set_texture_set(set);
        }
        recorder.update_constants(vc, &vs_bytes).unwrap();
        let mut ps_bytes = vec![0; pl.byte_len];
        for (dst, value) in ps_bytes[..16].chunks_exact_mut(4).zip(colour) {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        recorder.update_constants(pc, &ps_bytes).unwrap();
        let writes = recorder.stats.uniform_writes;
        recorder.update_constants(pc, &ps_bytes).unwrap();
        assert_eq!(
            recorder.stats.uniform_writes, writes,
            "unchanged constants not copied"
        );
        recorder
            .begin_pass(PassDesc {
                colours: [Some(view), None, None, None],
                depth: None,
                clear_colour: Some(wgpu::Color::BLUE),
                clear_depth: None,
                clear_stencil: None,
            })
            .unwrap();
        if indexed {
            recorder.draw_indexed(0..3, 0, 0..1).unwrap();
        } else {
            recorder.draw(0..3, 0..1).unwrap();
        }
        recorder.submit();
        gpu.present(&native_target);
        assert_eq!(
            gpu.last_frame.as_ref().unwrap().rgba,
            reference.rgba,
            "native case {case}"
        );
    }
    assert_eq!(recorder.stats.draws, 8);
    assert_eq!(recorder.stats.uniform_writes, 16);
    if resident {
        // Distinct snapshots inside one pass, dynamic-state splits, then an
        // exact repeated sequence with new bytes at unchanged offsets.
        let builds = recorder.stats.bundle_builds;
        let hits = recorder.stats.bundle_hits;
        for frame in 0..4 {
            let mut draws = vec![
                call(
                    &legacy_target,
                    true,
                    false,
                    true,
                    [0.2 + frame as f32 * 0.1, 0.0, 0.0, 1.0],
                ),
                call(
                    &legacy_target,
                    false,
                    false,
                    true,
                    [0.0, 0.3 + frame as f32 * 0.1, 0.0, 1.0],
                ),
                call(&legacy_target, true, true, false, [0.0, 0.0, 0.8, 0.5]),
            ];
            draws[0].scissor = [0, 0, 4, 8];
            draws[1].scissor = [4, 0, 8, 8];
            // A changed draw range must invalidate the cached packet.
            if frame == 3 {
                draws[1].primitives = 0;
            }
            let reference = oracle
                .render(&[], Some(&clear(&legacy_target)), &draws, &legacy_target)
                .unwrap();
            recorder.update_constants(vc, &vs_bytes).unwrap();
            recorder
                .begin_pass(PassDesc {
                    colours: [Some(view), None, None, None],
                    depth: None,
                    clear_colour: Some(wgpu::Color::BLUE),
                    clear_depth: None,
                    clear_stencil: None,
                })
                .unwrap();
            for (n, draw) in draws.iter().enumerate() {
                let (pipeline, sets) = pipelines[if n == 2 { 2 } else { 1 }];
                recorder.set_pipeline(pipeline);
                for set in sets {
                    recorder.set_texture_set(set);
                }
                let [x, y, right, bottom] = draw.scissor;
                recorder.set_scissor([x as u32, y as u32, (right - x) as u32, (bottom - y) as u32]);
                let mut block = vec![0; pl.byte_len];
                for (word, value) in block[..16].chunks_exact_mut(4).zip(draw.ps_constants.f[0]) {
                    word.copy_from_slice(&value.to_le_bytes());
                }
                recorder.update_constants(pc, &block).unwrap();
                if n == 1 {
                    recorder.draw(0..draw.primitives * 3, 0..1).unwrap();
                } else {
                    recorder.draw_indexed(0..3, 0, 0..1).unwrap();
                }
            }
            recorder.submit();
            gpu.present(&native_target);
            assert_eq!(
                gpu.last_frame.as_ref().unwrap().rgba,
                reference.rgba,
                "resident multi-snapshot frame {frame}"
            );
        }
        assert_eq!(
            recorder.stats.bundle_builds - builds,
            4,
            "three chunks plus one changed draw"
        );
        assert_eq!(
            recorder.stats.bundle_hits - hits,
            8,
            "unchanged exact sequences reused"
        );
    }
}

#[test]
fn native_imported_texture_and_constant_generations() {
    compare_texture_constants(false, false);
}
#[test]
fn native_compact_texture_and_constant_generations() {
    compare_texture_constants(true, false);
}
#[test]
fn native_resident_bundle_texture_generations() {
    compare_texture_constants(false, true);
    compare_texture_constants(true, true);
}
fn compare_texture_constants(compact: bool, resident: bool) {
    use d3d9cmd::{TexKind, TextureBinding as LegacyTexture, Upload};
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else {
        eprintln!("SKIP no GPU");
        return;
    };
    let t = target(0x300);
    let ps_tex = vec![
        0xffff0300, 0x0200001f, 0x90000000, 0xa00f0800, 0x02000001, 0x800f0001, 0xa0e40000, 0x03000042,
        0x800f0000, 0x80e40001, 0xa00f0800, 0x02000001, 0x800f0800, 0x80e40000, 0xffff,
    ];
    let upload = Upload::Image {
        resource: 0x400,
        index: 0,
        format: 21,
        width: 2,
        height: 2,
        depth: 1,
        pitch: 8,
        slice: 16,
        bytes: vec![0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255],
    };
    let mut states = [0; 14];
    for i in [1, 2, 3] {
        states[i] = 3;
    }
    for i in [5, 6] {
        states[i] = 1;
    }
    let texture = LegacyTexture {
        sampler: 0,
        resource: 0x400,
        kind: TexKind::Flat,
        format: 21,
        width: 2,
        height: 2,
        depth: 1,
        levels: 1,
        usage: 0,
        states,
    };
    let mut d = call(&t, false, false, false, [0.25, 0.25, 0.0, 0.0]);
    d.pixel_shader = Some(ps_tex.clone().into());
    d.textures = vec![texture.clone()].into();
    let mut oracle = d3d9ref::Oracle::new().expect("D3D9");
    let expected = oracle
        .render(
            std::slice::from_ref(&upload),
            Some(&clear(&t)),
            std::slice::from_ref(&d),
            &t,
        )
        .unwrap();
    gpu.upload(upload);
    gpu.clear(&clear(&t));
    gpu.draw(&d);
    gpu.present(&t);
    assert_eq!(gpu.last_frame.as_ref().unwrap().rgba, expected.rgba);
    let mut device = Device::from_backend(&mut gpu);
    let target = device.import_target(&mut gpu, &t, false).unwrap();
    let texture = device.import_texture(&mut gpu, &texture).unwrap();
    let view = device.create_view(texture, &Default::default());
    let sampler = device.create_sampler(&Default::default()).unwrap();
    assert!(
        device
            .create_sampler(&SamplerDesc {
                lod_bias: 1.0,
                ..Default::default()
            })
            .is_ok()
    );
    let make_shader = if compact {
        Device::create_compact_shader
    } else {
        Device::create_shader
    };
    let vertex_shader = make_shader(&mut device, &vs(), Stage::Vertex).unwrap();
    let pixel_shader = make_shader(&mut device, &ps_tex, Stage::Pixel).unwrap();
    let vl = device.shader_constant_layout(vertex_shader);
    let pl = device.shader_constant_layout(pixel_shader);
    let input = device
        .create_input_layout(InputLayoutDesc {
            streams: vec![InputStream {
                stride: 16,
                step: wgpu::VertexStepMode::Vertex,
            }],
            elements: vec![InputElement {
                stream: 0,
                offset: 0,
                format: wgpu::VertexFormat::Float32x4,
                semantic: (0, 0),
                location: 0,
            }],
        })
        .unwrap();
    let blend = device.create_blend(Default::default());
    let rasterizer = device.create_rasterizer(Default::default());
    let depth_stencil = device.create_depth_stencil(Default::default());
    let p = device
        .create_pipeline(PipelineDesc {
            vertex_shader,
            pixel_shader,
            input,
            blend,
            rasterizer,
            depth_stencil,
            colours: [Some(wgpu::TextureFormat::Bgra8Unorm), None, None, None],
            depth: None,
            samples: 1,
            sample_mask: !0,
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index: None,
        })
        .unwrap();
    let bindings = [ddgpu::native::TextureBinding {
        slot: 0,
        view,
        sampler,
    }];
    let tex = device.create_texture_set(p, Stage::Pixel, &bindings).unwrap();
    assert_eq!(
        device.create_texture_set(p, Stage::Pixel, &bindings).unwrap(),
        tex
    );
    let empty = device.create_texture_set(p, Stage::Vertex, &[]).unwrap();
    let vb = device.create_buffer(&vertices(), wgpu::BufferUsages::VERTEX);
    let mut r = Recorder::with_resident_constants(&device, 16384, resident);
    r.enable_bundles(resident);
    r.set_pipeline(p);
    r.set_texture_set(tex);
    r.set_texture_set(empty);
    r.set_vertex_buffer(
        0,
        BufferSlice {
            buffer: vb,
            offset: 0,
            size: 48,
        },
    );
    let pc = r.create_constant_buffer_for_shader(pixel_shader);
    let vc = r.create_constant_buffer_for_shader(vertex_shader);
    r.set_constants(pc);
    r.set_constants(vc);
    let mut bytes = vec![0; pl.byte_len];
    bytes[..4].copy_from_slice(&0.25f32.to_le_bytes());
    bytes[4..8].copy_from_slice(&0.25f32.to_le_bytes());
    r.update_constants(pc, &bytes).unwrap();
    let mut vs_bytes = vec![0; vl.byte_len];
    vs_bytes[vl.extra_offset..vl.extra_offset + 4].copy_from_slice(&0.125f32.to_le_bytes());
    vs_bytes[vl.extra_offset + 4..vl.extra_offset + 8].copy_from_slice(&(-0.125f32).to_le_bytes());
    r.update_constants(vc, &vs_bytes).unwrap();
    assert!(r.update_constants(pc, &[0; 16]).is_err());
    for frame in 0..2 {
        r.begin_pass(PassDesc {
            colours: [Some(target), None, None, None],
            depth: None,
            clear_colour: Some(wgpu::Color::BLUE),
            clear_depth: None,
            clear_stencil: None,
        })
        .unwrap();
        for _ in 0..3 {
            r.draw(0..3, 0..1).unwrap();
        }
        assert_eq!(
            r.stats.pipeline_sets,
            if resident { 0 } else { frame + 1 },
            "unchanged draw pipeline filtered"
        );
        assert_eq!(
            r.stats.uniform_writes,
            (frame + 1) * 2,
            "same constants restored once after submit"
        );
        r.submit();
        gpu.present(&t);
        assert_eq!(gpu.last_frame.as_ref().unwrap().rgba, expected.rgba);
    }
    if resident {
        assert_eq!(r.stats.bundle_builds, 1);
        assert_eq!(r.stats.bundle_hits, 1);
        assert!(
            r.stats.upload_bytes < 1024,
            "only initially nonzero registers uploaded"
        );
    }
    let mut last = 0.25f32;
    let mut exhausted = false;
    for i in 1..128 {
        let x = i as f32 / 128.0;
        bytes[..4].copy_from_slice(&x.to_le_bytes());
        if r.update_constants(pc, &bytes).is_err() {
            exhausted = true;
            break;
        }
        last = x;
    }
    assert!(exhausted, "bounded ring reports exhaustion");
    let writes = r.stats.uniform_writes;
    bytes[..4].copy_from_slice(&last.to_le_bytes());
    r.update_constants(pc, &bytes).unwrap();
    assert_eq!(
        r.stats.uniform_writes, writes,
        "failed update retained previous snapshot"
    );
    assert!(r.submit().is_none(), "no draw commands need submission");
    r.update_constants(pc, &bytes).unwrap();
    assert_eq!(
        r.stats.uniform_writes,
        writes + 1,
        "empty submission permits retry"
    );
    let writes = r.stats.uniform_writes;
    r.update_constant_range(pc, 0, &0.25f32.to_le_bytes()).unwrap();
    r.update_constant_range(pc, 8, &1.0f32.to_le_bytes()).unwrap();
    assert!(r.update_constant_range(pc, usize::MAX, &[1]).is_err());
    assert_eq!(r.stats.uniform_writes, writes, "range deltas wait for a draw");
    r.begin_pass(PassDesc {
        colours: [Some(target), None, None, None],
        depth: None,
        clear_colour: Some(wgpu::Color::BLUE),
        clear_depth: None,
        clear_stencil: None,
    })
    .unwrap();
    r.draw(0..3, 0..1).unwrap();
    assert_eq!(
        r.stats.uniform_writes,
        writes + 2,
        "two deltas coalesce; VS restored after submit"
    );
    r.submit();
    gpu.present(&t);
    assert_eq!(gpu.last_frame.as_ref().unwrap().rgba, expected.rgba);
}
