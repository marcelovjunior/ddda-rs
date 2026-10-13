use d3d9cmd::*;
use std::sync::Arc;

fn target(id: u32, format: u32) -> Target {
    Target {
        surface: id,
        format,
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
fn vs() -> Arc<[u32]> {
    vec![
        0xfffe0300, 0x0200001f, 0x80000000, 0x900f0000, 0x0200001f, 0x80000000, 0xe00f0000, 0x02000001,
        0xe00f0000, 0x90e40000, 0xffff,
    ]
    .into()
}
fn ps() -> Arc<[u32]> {
    vec![0xffff0300, 0x02000001, 0x800f0800, 0xa0e40000, 0xffff].into()
}
fn draw(blend: bool, scissor: bool, depth: bool, colour: [f32; 4]) -> NativeDraw {
    let mut states = vec![
        (7, depth as u32),
        (8, 3),
        (14, 1),
        (19, 5),
        (20, 6),
        (22, 1),
        (23, 4),
        (27, blend as u32),
        (52, 0),
        (57, 0),
        (58, 255),
        (59, 255),
        (168, 15),
        (171, 1),
        (174, scissor as u32),
        (190, 15),
        (191, 15),
        (192, 15),
        (193, 0xffffffff),
        (206, 0),
    ];
    states.sort_unstable();
    NativeDraw {
        state: Arc::new(NativeFixedState {
            identity: Default::default(),
            render_states: states.into(),
        }),
        vertex_shader: vs(),
        pixel_shader: ps(),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(),
        textures: Arc::from([]),
        streams: vec![NativeStream {
            stream: 0,
            resource: 10,
            offset: 0,
            stride: 16,
        }]
        .into(),
        indices: Some(NativeIndices {
            resource: 11,
            format: 101,
        }),
        vs_constants: Default::default(),
        ps_constants: Arc::new(NativeConstants {
            floats: vec![NativeFloatWrite {
                first: 0,
                registers: vec![colour.map(f32::to_bits)].into(),
            }],
            ..Default::default()
        }),
        viewport: [0, 0, 8, 8, 0, 1f32.to_bits()],
        scissor: scissor.then_some([1, 2, 6, 7]),
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        instances: 1,
    }
}
fn legacy(d: &NativeDraw, t: &Target, depth: Option<&Target>) -> DrawCall {
    DrawCall {
        primitive: d.primitive,
        primitives: d.primitives,
        start: d.start,
        base_vertex: d.base_vertex,
        min_index: d.min_index,
        vertex_count: d.vertex_count,
        vertices: Vertices::Inline {
            bytes: vertices(),
            stride: 16,
        },
        indices: if d.indices.is_some() {
            Indices::Inline {
                bytes: vec![0, 0, 1, 0, 2, 0],
                format: 101,
            }
        } else {
            Indices::None
        },
        vertex_shader: Some(d.vertex_shader.clone()),
        pixel_shader: Some(d.pixel_shader.clone()),
        declaration: d.declaration.clone(),
        fvf: 0,
        vs_constants: Arc::new(Constants::default()),
        ps_constants: Arc::new(Constants {
            f: d.ps_constants.floats[0].registers.to_vec(),
            ..Default::default()
        }),
        textures: d.textures.clone(),
        render_states: d.state.render_states.to_vec().into(),
        viewport: d.viewport,
        scissor: d.scissor.unwrap_or([0, 0, 8, 8]),
        targets: [Some(t.clone()), None, None, None],
        depth: depth.cloned(),
    }
}
fn clear(t: &Target, depth: Option<&Target>) -> ClearCall {
    ClearCall {
        rects: vec![],
        flags: if depth.is_some() { 7 } else { 1 },
        colour: 0xff0000ff,
        z: 1.0,
        stencil: 0,
        targets: [Some(t.clone()), None, None, None],
        depth: depth.cloned(),
        viewport: [0, 0, 8, 8, 0, 1f32.to_bits()],
        scissor: None,
        srgb: false,
    }
}
fn job(d: NativeDraw, t: &Target, depth: Option<&Target>) -> NativeJob {
    NativeJob {
        passes: vec![NativePass {
            colours: [Some(t.clone()), None, None, None],
            depth: depth.cloned(),
            draws: vec![d],
            ..Default::default()
        }],
    }
}
fn upload(b: &mut dyn GraphicsBackend) {
    b.upload(Upload::Buffer {
        resource: 10,
        bytes: vertices(),
    });
    b.upload(Upload::Buffer {
        resource: 11,
        bytes: vec![0, 0, 1, 0, 2, 0],
    });
}

/// Run this binary serially: backend selection is a process environment switch.
#[test]
fn raw_native_jobs_match_wgpu_with_clears_reuse_and_async_submission() {
    let old = std::env::var_os("DDGPU_VK");
    let mut images = Vec::new();
    let before = ddgpu::vk::stats();
    for raw in [false, true] {
        std::env::set_var("DDGPU_VK", if raw { "1" } else { "0" });
        let mut backend = ddgpu::WgpuBackend::headless().expect("Vulkan adapter");
        backend.set_async_submit(true);
        upload(&mut backend);
        let t = target(41, 21);
        let z = target(42, 75);
        for round in 0..12 {
            // Back-to-back jobs, no completion wait between constant mutations.
            for part in 0..4 {
                let mut d = draw(part & 1 != 0, part & 2 != 0, true,
                    [round as f32 / 16.0, part as f32 / 4.0, 0.25, 0.5]);
                if part & 1 == 0 { d.indices = None; }
                let mut j = job(d, &t, Some(&z));
                if part == 0 {
                    j.passes[0].clear_colour = Some(0xff123456);
                    j.passes[0].clear_depth = Some(1f32.to_bits());
                    j.passes[0].clear_stencil = Some(round);
                }
                assert!(backend.native(j).is_ok());
            }
            images.push(backend.read_back(&t).unwrap());
            if round == 5 {
                backend.release(10);
                upload(&mut backend);
            }
        }
        assert_eq!(backend.native_job_stats().accepted, 48);
        assert_eq!(backend.native_job_stats().submit_batches, 48,
            "dirty uploads and render commands share one ordered queue batch per job");
        assert!(backend.errors.is_empty(), "{:?}", backend.errors);
    }
    match old { Some(value) => std::env::set_var("DDGPU_VK", value), None => std::env::remove_var("DDGPU_VK") }
    assert_eq!(&images[..12], &images[12..]);
    let after = ddgpu::vk::stats();
    assert!(after.passes > before.passes, "native jobs never reached raw Vulkan");
    assert_eq!(after.encode_errors, before.encode_errors);
    eprintln!("L152 native clear/reuse hash {} raw_passes={}", blake3::hash(&images.concat()), after.passes-before.passes);
}

#[test]
#[ignore = "synthetic native backend cost; run optimized, serially, with a 60s cap"]
fn native_job_encoding_cost() {
    let mut backend = ddgpu::WgpuBackend::headless().expect("Vulkan adapter");
    backend.set_async_submit(true);
    upload(&mut backend);
    let t = target(43, 21);
    let draws: usize = std::env::var("L152_JOB_DRAWS").ok().and_then(|s|s.parse().ok()).unwrap_or(5000);
    assert!(draws > 0 && draws <= 5000);
    let jobs = 100000 / draws;
    let passes: usize = std::env::var("L152_PASSES").ok().and_then(|s|s.parse().ok()).unwrap_or(1);
    assert!(passes > 0 && passes <= draws);
    let mut j = NativeJob { passes: vec![] };
    let seed = draw(false,false,false,[0.0;4]);
    for p in 0..passes {
        let mut pass = job(seed.clone(),&t,None).passes.remove(0);
        pass.draws = (p*draws/passes..(p+1)*draws/passes).map(|i| {
            let mut d = seed.clone();
            Arc::make_mut(&mut d.ps_constants).floats[0].registers = vec![[(i%17) as f32/16.0,(i%13) as f32/12.0,0.25,1.0].map(f32::to_bits)].into();
            d
        }).collect();
        j.passes.push(pass);
    }
    // Warm pipelines, descriptors, constant storage and initialization tracking.
    for _ in 0..3 { assert!(backend.native(j.clone()).is_ok()); }
    let expected=backend.read_back(&t).unwrap();
    let churn = std::env::var("L152_CHURN").as_deref() == Ok("1");
    let mut alternate = j.clone();
    for d in alternate.passes.iter_mut().flat_map(|p| &mut p.draws) {
        Arc::make_mut(&mut Arc::make_mut(&mut d.ps_constants).floats[0].registers)[0][2] = 0.75f32.to_bits();
    }
    assert!(backend.native(alternate.clone()).is_ok());
    let alternate_expected = backend.read_back(&t).unwrap();
    assert!(backend.native(j.clone()).is_ok());
    assert_eq!(backend.read_back(&t).unwrap(), expected);
    // Fixture production is outside the backend timer. Consumption, job
    // destruction, preparation, encoding and ordered submission remain inside.
    let queued: Vec<_> = (0..jobs).map(|i| if churn && i%2 == 1 { alternate.clone() } else { j.clone() }).collect();
    let before=backend.native_job_stats();
    let start=std::time::Instant::now();
    for job in queued { assert!(backend.native(job).is_ok()); }
    backend.set_async_submit(false); // Includes ordered finish/submit, no GPU wait.
    let elapsed=start.elapsed();
    let result=backend.read_back(&t).unwrap();
    assert_eq!(result,if churn && jobs%2 == 0 { alternate_expected } else { expected });
    assert!(backend.errors.is_empty(),"{:?}",backend.errors);
    let after=backend.native_job_stats();
    eprintln!("L152 native_cost jobs={jobs} draws_per_job={draws} passes={passes} churn={churn} ns_per_draw={:.1} upload_bytes={} hash={} vk={:?}",
        elapsed.as_nanos() as f64/(jobs*draws) as f64,after.upload_bytes-before.upload_bytes,blake3::hash(&result),ddgpu::vk::stats());
}

#[test]
fn native_worker_effective_states_match_backend_and_host_driver() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let mut oracle = d3d9ref::Oracle::new().unwrap();
    let original = target(1, 21);
    let native = target(2, 21);
    let z = target(3, 75);
    upload(&mut backend);
    for case in 0..16 {
        let depth = (case & 8 != 0).then_some(&z);
        let mut d = draw(
            case & 2 != 0,
            case & 4 != 0,
            depth.is_some(),
            [0.8, case as f32 / 16.0, 0.25, 0.5],
        );
        if case & 1 == 0 {
            d.indices = None;
        }
        let legacy = legacy(&d, &original, depth);
        let expected = oracle
            .render(
                &[],
                Some(&clear(&original, depth)),
                std::slice::from_ref(&legacy),
                &original,
            )
            .unwrap();
        backend.clear(&clear(&original, depth));
        backend.draw(&legacy);
        backend.present(&original);
        assert_eq!(
            backend.last_frame.as_ref().unwrap().rgba,
            expected.rgba,
            "legacy case {case}"
        );
        backend.clear(&clear(&native, depth));
        assert!(
            backend.native(job(d, &native, depth)).is_ok(),
            "native rejected case {case}"
        );
        backend.present(&native);
        assert_eq!(
            backend.last_frame.as_ref().unwrap().rgba,
            expected.rgba,
            "native case {case}"
        );
        assert!(backend.errors.is_empty(), "{:?}", backend.errors);
    }
    assert_eq!(backend.native_job_stats().accepted, 16);
}

/// L140: run with DDGPU_NATIVE_PIPELINE_CACHE=1 to exercise the opt-in;
/// also valid with it unset, comparing the ordinary native path's pixels.
#[test]
fn persisted_native_pipelines_preserve_pixels() {
    let enabled = std::env::var("DDGPU_NATIVE_PIPELINE_CACHE").as_deref() == Ok("1");
    let dir = std::env::temp_dir().join(format!("ddgpu-l140-native-{}", std::process::id()));
    let mut expected = Vec::new();
    for round in 0..2 {
        let Some(mut backend) = ddgpu::WgpuBackend::headless() else { return; };
        if !backend.enable_pipeline_cache(&dir) { return; }
        upload(&mut backend);
        let target = target(1, 21);
        for case in 0..4 {
            let d = draw(case & 1 != 0, case & 2 != 0, false, [0.8, case as f32 / 4.0, 0.25, 0.5]);
            backend.clear(&clear(&target, None));
            assert!(backend.native(job(d, &target, None)).is_ok());
            backend.present(&target);
            let pixels = backend.last_frame.as_ref().unwrap().rgba.clone();
            if round == 0 { expected.push(pixels); }
            else { assert_eq!(pixels, expected[case], "warm native pixels case {case}"); }
            assert!(backend.errors.is_empty(), "{:?}", backend.errors);
        }
        ddgpu::save_pipeline_caches();
        if enabled {
            let files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap()).collect();
            assert!(files.iter().any(|e| e.metadata().unwrap().len() > 0), "native pipeline cache was not saved");
        }
    }
    if dir.exists() { std::fs::remove_dir_all(dir).unwrap(); }
}

#[test]
fn native_job_rejection_is_atomic_and_recovers() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let t = target(1, 21);
    upload(&mut backend);
    backend.clear(&clear(&t, None));
    let before = backend.read_back(&t).unwrap();
    let good = draw(false, false, false, [1.0, 0.0, 0.0, 1.0]);
    let mut bad = good.clone();
    bad.ps_constants = Default::default();
    let mut j = job(good.clone(), &t, None);
    j.passes[0].clear_colour = Some(0xff00ff00);
    j.passes[0].draws.push(bad);
    assert_eq!(backend.native(j.clone()), Err(j));
    assert_eq!(
        backend.read_back(&t).unwrap(),
        before,
        "reject must not clear or partially draw"
    );
    assert!(backend.native(job(good.clone(), &t, None)).is_ok());
    let red = backend.read_back(&t).unwrap();
    assert_ne!(red, before);
    backend.query_begin(99);
    let j = job(good.clone(), &t, None);
    assert_eq!(backend.native(j.clone()), Err(j));
    backend.query_end(99);
    backend.release(10);
    let j = job(good.clone(), &t, None);
    assert_eq!(backend.native(j.clone()), Err(j));
    assert_eq!(backend.read_back(&t).unwrap(), red);
    backend.upload(Upload::Buffer {
        resource: 10,
        bytes: vertices(),
    });
    assert!(backend.native(job(good, &t, None)).is_ok());
    assert_eq!(backend.native_job_stats().rejected, 3);
}

#[test]
fn deferred_native_jobs_preserve_upload_copy_and_resource_reuse_order() {
    let Some(backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let mut backend = DeferredBackend::new(Box::new(backend));
    assert!(backend.native_supported());
    let t = target(1, 21);
    let dst = target(2, 21);
    upload(&mut backend);
    let d = draw(false, false, false, [1.0, 0.0, 0.0, 1.0]);
    backend.clear(&clear(&t, None));
    assert!(backend.native(job(d.clone(), &t, None)).is_ok());
    backend.copy(&CopyCall {
        src: t.clone(),
        dst: dst.clone(),
        src_rect: [0, 0, 8, 8],
        dst_rect: [0, 0, 8, 8],
        filter: 0,
    });
    let first = backend.read_back(&dst).unwrap();
    assert_eq!(first, backend.read_back(&t).unwrap());
    backend.release(10);
    backend.upload(Upload::Buffer {
        resource: 10,
        bytes: vec![0; 48],
    });
    backend.clear(&clear(&t, None));
    assert!(backend.native(job(d.clone(), &t, None)).is_ok());
    let degenerate = backend.read_back(&t).unwrap();
    assert_ne!(
        first, degenerate,
        "released buffer must not be reused from native cache"
    );
    backend.release(10);
    backend.upload(Upload::Buffer {
        resource: 10,
        bytes: vertices(),
    });
    assert!(backend.native(job(d, &t, None)).is_ok());
    assert_eq!(backend.read_back(&t).unwrap(), first);
}

#[test]
fn native_worker_sampled_uploads_and_sampler_identity_match_legacy() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let a = target(1, 21);
    let b = target(2, 21);
    upload(&mut backend);
    let mut d = draw(false, false, false, [0.25, 0.25, 0.0, 0.0]);
    d.pixel_shader = vec![
        0xffff0300, 0x0200001f, 0x90000000, 0xa00f0800, 0x02000001, 0x800f0001, 0xa0e40000, 0x03000042,
        0x800f0800, 0x80e40001, 0xa00f0800, 0xffff,
    ]
    .into();
    let mut states = [0; 14];
    states[1..4].fill(3);
    states[5] = 1;
    states[6] = 1;
    d.textures = vec![TextureBinding {
        sampler: 0,
        resource: 50,
        kind: TexKind::Flat,
        format: 21,
        width: 2,
        height: 2,
        depth: 1,
        levels: 1,
        usage: 0,
        states,
    }]
    .into();
    for colour in [[0u8, 0, 255, 255], [0, 255, 0, 255]] {
        backend.upload(Upload::Image {
            resource: 50,
            index: 0,
            format: 21,
            width: 2,
            height: 2,
            depth: 1,
            pitch: 8,
            slice: 16,
            bytes: colour.repeat(4),
        });
        backend.clear(&clear(&a, None));
        backend.draw(&legacy(&d, &a, None));
        backend.present(&a);
        let expected = backend.last_frame.as_ref().unwrap().rgba.clone();
        backend.clear(&clear(&b, None));
        assert!(backend.native(job(d.clone(), &b, None)).is_ok());
        backend.present(&b);
        assert_eq!(backend.last_frame.as_ref().unwrap().rgba, expected);
    }
    assert_eq!(backend.native_job_stats().accepted, 2);
}

#[test]
fn native_resident_jobs_follow_async_finisher_and_skip_unchanged_uploads() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    backend.set_async_submit(true);
    let t = target(1, 21);
    upload(&mut backend);
    let red = draw(false, false, false, [1.0, 0.0, 0.0, 1.0]);
    let mut first = job(red.clone(), &t, None);
    first.passes[0].clear_colour = Some(0xff0000ff);
    assert!(backend.native(first.clone()).is_ok());
    let uploaded = backend.native_job_stats().upload_bytes;
    assert!(uploaded > 0);
    // No readback/Wait between these jobs: prior render finish must precede
    // constant storage reuse, while recording may overlap the finisher.
    assert!(backend.native(first).is_ok());
    assert_eq!(backend.native_job_stats().upload_bytes, uploaded);
    let expected = backend.read_back(&t).unwrap();
    let mut green = red.clone();
    Arc::make_mut(&mut green.ps_constants).floats[0].registers = vec![[0.0f32, 1.0, 0.0, 1.0].map(f32::to_bits)].into();
    assert!(backend.native(job(green, &t, None)).is_ok());
    assert!(backend.native_job_stats().upload_bytes > uploaded);
    let changed = backend.read_back(&t).unwrap();
    assert_ne!(changed, expected);
    backend.set_async_submit(false);
    assert!(backend.native(job(red, &t, None)).is_ok());
    assert_eq!(backend.read_back(&t).unwrap(), expected);
}

#[test]
fn deferred_native_refusals_keep_all_original_draws_and_query_order() {
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    gpu.set_async_submit(true);
    let mut backend = DeferredBackend::new(Box::new(gpu));
    let mut oracle = d3d9ref::Oracle::new().unwrap();
    let t = target(1, 21);
    upload(&mut backend);
    for format in [101, 102] {
        let bytes: Vec<u8> = if format == 101 {
            [0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes).collect()
        } else {
            [0u32, 1, 2].into_iter().flat_map(u32::to_le_bytes).collect()
        };
        backend.upload(Upload::Buffer {
            resource: 11,
            bytes: bytes.clone(),
        });
        for primitive in [5] {
            for query in [false, true] {
                // The first draw could run natively; the second refuses. Alpha
                // blending makes both dropped and duplicated first draws visible.
                let mut first = draw(true, false, false, [1.0, 0.0, 0.0, 0.5]);
                first.indices.as_mut().unwrap().format = format;
                let mut strip = draw(true, false, false, [0.0, 1.0, 0.0, 0.5]);
                strip.indices.as_mut().unwrap().format = format;
                strip.primitive = primitive;
                // Alpha-test ALWAYS has the same output, but is deliberately
                // outside the native worker's supported state. Keep exercising
                // atomic fallback now that bounded strips themselves work.
                let state = Arc::make_mut(&mut strip.state);
                let mut states = state.render_states.to_vec();
                states.extend([(15, 1), (24, 0), (25, 8)]);
                states.sort_unstable();
                state.render_states = states.into();
                let calls: Vec<_> = [&first, &strip]
                    .into_iter()
                    .map(|d| {
                        let mut call = legacy(d, &t, None);
                        call.indices = Indices::Inline {
                            bytes: bytes.clone(),
                            format,
                        };
                        call
                    })
                    .collect();
                let expected = oracle.render(&[], Some(&clear(&t, None)), &calls, &t).unwrap();
                backend.clear(&clear(&t, None));
                if query {
                    backend.query_begin(99);
                }
                let mut native = job(first, &t, None);
                native.passes.push(NativePass {
                    colours: [Some(t.clone()), None, None, None],
                    draws: vec![strip],
                    ..Default::default()
                });
                backend.native_or_draw(native, calls);
                if query {
                    backend.query_end(99);
                }
                let mut actual = backend.read_back(&t).unwrap();
                // D3DFMT_A8R8G8B8 read_back returns BGRA, host oracle RGBA.
                for pixel in actual.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                assert_eq!(
                    actual, expected.rgba,
                    "format={format} primitive={primitive} query={query}"
                );
            }
        }
    }
}

#[test]
fn bounded_indexed_strips_match_host_driver_with_start_base_and_culling() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let mut oracle = d3d9ref::Oracle::new().unwrap();
    let original = target(1, 21);
    let native = target(2, 21);
    let bytes: Vec<u8> = [
        [-1.0f32, -1.0, 0.5, 1.0],
        [-1.0, 1.0, 0.5, 1.0],
        [1.0, -1.0, 0.5, 1.0],
        [1.0, 1.0, 0.5, 1.0],
    ]
    .into_iter()
    .flatten()
    .flat_map(f32::to_le_bytes)
    .collect();
    backend.upload(Upload::Buffer {
        resource: 10,
        bytes: bytes.clone(),
    });
    for format in [101, 102] {
        let index_bytes: Vec<u8> = if format == 101 {
            [0u16, 1, 2, 3, 4]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            [0u32, 1, 2, 3, 4]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect()
        };
        backend.upload(Upload::Buffer {
            resource: 11,
            bytes: index_bytes.clone(),
        });
        for primitive in [5] {
            for cull in [1, 2, 3] {
                let mut d = draw(false, false, false, [0.8, 0.2, 0.5, 1.0]);
                d.indices.as_mut().unwrap().format = format;
                d.primitive = primitive;
                d.primitives = if primitive == 3 { 3 } else { 2 };
                d.start = 1;
                d.base_vertex = -1;
                d.min_index = 1;
                d.vertex_count = 4;
                let state = Arc::make_mut(&mut d.state);
                Arc::make_mut(&mut state.render_states)
                    .iter_mut()
                    .find(|x| x.0 == 22)
                    .unwrap()
                    .1 = cull;
                let mut call = legacy(&d, &original, None);
                // Inline indices encode UP draws and start at zero in the
                // backend. Buffer-backed controls exercise start=1 correctly.
                call.vertices = Vertices::Streams(vec![(0, 10, 0, 16, 1)].into());
                call.indices = Indices::Buffer(11, format);
                let expected = oracle
                    .render(
                        &[
                            Upload::Buffer {
                                resource: 10,
                                bytes: bytes.clone(),
                            },
                            Upload::Buffer {
                                resource: 11,
                                bytes: index_bytes.clone(),
                            },
                        ],
                        Some(&clear(&original, None)),
                        &[call.clone()],
                        &original,
                    )
                    .unwrap();
                backend.clear(&clear(&original, None));
                backend.draw(&call);
                backend.present(&original);
                assert_eq!(
                    backend.last_frame.as_ref().unwrap().rgba,
                    expected.rgba,
                    "legacy format={format} primitive={primitive} cull={cull}"
                );
                backend.clear(&clear(&native, None));
                assert!(backend.native(job(d, &native, None)).is_ok());
                backend.present(&native);
                assert_eq!(
                    backend.last_frame.as_ref().unwrap().rgba,
                    expected.rgba,
                    "native format={format} primitive={primitive} cull={cull}"
                );
            }
        }
    }
    assert_eq!(backend.native_job_stats().accepted, 6);
    assert_eq!(backend.native_job_stats().rejected, 0);
    // L141: a declared vertex range reaching the u16 sentinel draws as the
    // D3D9 path draws it (DrawIndexedPrimitive's NumVertices is a hint the
    // D3D9 path does not use for strips; both give the pipeline the index
    // format, so an index 0xFFFF would restart both: their shared limit)
    let mut wide = draw(false, false, false, [1.0, 0.5, 0.25, 1.0]);
    wide.primitive = 5;
    wide.primitives = 1;
    wide.vertex_count = 65536;
    let mut call = legacy(&wide, &original, None);
    call.vertices = Vertices::Streams(vec![(0, 10, 0, 16, 1)].into());
    call.indices = Indices::Buffer(11, 102);
    wide.indices.as_mut().unwrap().format = 102;
    backend.clear(&clear(&original, None));
    backend.draw(&call);
    backend.present(&original);
    let expected = backend.last_frame.as_ref().unwrap().rgba.clone();
    backend.clear(&clear(&native, None));
    assert!(backend.native(job(wide, &native, None)).is_ok());
    backend.present(&native);
    assert_eq!(backend.last_frame.as_ref().unwrap().rgba, expected, "a wide declared range");
    assert_eq!(backend.native_job_stats().rejected, 0);
}

/// L141: a guest release keeps the native worker and its pipelines. A
/// resource the worker does not hold changes nothing; one it holds resets
/// only its resource tables, and the next job still draws the same image.
#[test]
fn releases_keep_the_native_worker_and_its_pipelines() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let t = target(1, 21);
    upload(&mut backend);
    backend.upload(Upload::Buffer { resource: 99, bytes: vec![0; 64] });
    let d = draw(false, false, false, [0.0, 1.0, 0.0, 1.0]);
    backend.clear(&clear(&t, None));
    assert!(backend.native(job(d.clone(), &t, None)).is_ok());
    let first = backend.read_back(&t).unwrap();
    let pipeline_creations = ddgpu::pipeline_stats().pipelines;
    // not held by the worker
    backend.release(99);
    assert!(backend.native(job(d.clone(), &t, None)).is_ok());
    let s = backend.native_job_stats();
    assert_eq!((s.worker_instances, s.resource_resets), (1, 0));
    // held: the vertex buffer, uploaded again with the same bytes
    backend.release(10);
    backend.upload(Upload::Buffer { resource: 10, bytes: vertices() });
    backend.clear(&clear(&t, None));
    assert!(backend.native(job(d, &t, None)).is_ok());
    assert_eq!(backend.read_back(&t).unwrap(), first);
    assert_eq!(ddgpu::pipeline_stats().pipelines, pipeline_creations,
        "resource release must retain both wgpu and raw pipeline objects");
    let s = backend.native_job_stats();
    assert_eq!((s.worker_instances, s.resource_resets, s.accepted), (1, 1, 3));
}

/// L141: a render-target texture lacking channels (X8R8G8B8: alpha) is
/// sampled with them as 1 by the native path too, as the D3D9 path does.
#[test]
fn native_render_target_samples_fill_missing_channels_as_the_d3d9_path() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let a = target(1, 21);
    let b = target(2, 21);
    upload(&mut backend);
    let mut d = draw(false, false, false, [0.25, 0.25, 0.0, 0.0]);
    // texld oC0, c0, s0: the texture's colour, alpha included
    d.pixel_shader = vec![
        0xffff0300, 0x0200001f, 0x90000000, 0xa00f0800, 0x02000001, 0x800f0001, 0xa0e40000, 0x03000042,
        0x800f0800, 0x80e40001, 0xa00f0800, 0xffff,
    ]
    .into();
    let mut states = [0; 14];
    states[1..4].fill(3);
    states[5] = 1;
    states[6] = 1;
    d.textures = vec![TextureBinding {
        sampler: 0,
        resource: 50,
        kind: TexKind::Flat,
        format: 22,
        width: 2,
        height: 2,
        depth: 1,
        levels: 1,
        usage: USAGE_RENDERTARGET,
        states,
    }]
    .into();
    // the render-target texture's contents: a clear through its surface,
    // alpha 0 (X8R8G8B8 keeps none)
    let rt = Target {
        surface: 60,
        format: 22,
        width: 2,
        height: 2,
        texture: Some(TargetTexture { resource: 50, kind: TexKind::Flat, width: 2, height: 2, depth: 1, levels: 1, usage: USAGE_RENDERTARGET }),
        level: 0,
        samples: 0,
        quality: 0,
    };
    backend.clear(&ClearCall { colour: 0x00ff_8000, viewport: [0, 0, 2, 2, 0, 1f32.to_bits()], ..clear(&rt, None) });
    backend.clear(&clear(&a, None));
    backend.draw(&legacy(&d, &a, None));
    backend.present(&a);
    let expected = backend.last_frame.as_ref().unwrap().rgba.clone();
    assert!(expected.chunks(4).all(|p| p[3] == 255), "the D3D9 path samples alpha as 1");
    backend.clear(&clear(&b, None));
    assert!(backend.native(job(d, &b, None)).is_ok(), "{:?}", backend.native_job_stats().rejection_reasons);
    backend.present(&b);
    assert_eq!(backend.last_frame.as_ref().unwrap().rgba, expected);
    assert!(backend.errors.is_empty(), "{:?}", backend.errors);
}

/// L141: DEPTHBIAS draws natively, in the pixel shader as the D3D9 path
/// does: the same image, and the bias moves the depth test's outcome.
#[test]
fn native_depth_bias_matches_the_d3d9_path() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let a = target(1, 21);
    let b = target(2, 21);
    let z = target(3, 75);
    upload(&mut backend);
    let clear_at = |t: &Target| ClearCall { z: 0.6, ..clear(t, Some(&z)) };
    let mut images = vec![];
    for bias in [0.0f32, 0.25] {
        let mut d = draw(false, false, true, [0.5, 0.25, 1.0, 1.0]);
        let mut states = d.state.render_states.to_vec();
        states.push((195, bias.to_bits()));
        states.sort_unstable();
        d.state = Arc::new(NativeFixedState { identity: Default::default(), render_states: states.into() });
        backend.clear(&clear_at(&a));
        backend.draw(&legacy(&d, &a, Some(&z)));
        backend.present(&a);
        let expected = backend.last_frame.as_ref().unwrap().rgba.clone();
        backend.clear(&clear_at(&b));
        assert!(backend.native(job(d, &b, Some(&z))).is_ok(), "{:?}", backend.native_job_stats().rejection_reasons);
        backend.present(&b);
        assert_eq!(backend.last_frame.as_ref().unwrap().rgba, expected, "bias {bias}");
        images.push(expected);
    }
    assert_ne!(images[0], images[1], "the bias changes what passes the depth test");
    assert!(backend.errors.is_empty(), "{:?}", backend.errors);
}

/// L141: a draw sampling the texture it renders into samples what the pass
/// drew before it (the D3D9 path copies it first; L59), natively too.
#[test]
fn native_feedback_draws_sample_what_the_pass_drew_before_them() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    let rt = |surface: u32| Target {
        surface,
        format: 21,
        width: 8,
        height: 8,
        texture: Some(TargetTexture { resource: 70, kind: TexKind::Flat, width: 8, height: 8, depth: 1, levels: 1, usage: USAGE_RENDERTARGET }),
        level: 0,
        samples: 0,
        quality: 0,
    };
    let t = rt(71);
    upload(&mut backend);
    // first: a flat colour; second: samples the target at c0 = (0.25, 0.25)
    let first = draw(false, false, false, [0.0, 0.5, 1.0, 1.0]);
    let mut second = draw(false, false, false, [0.25, 0.25, 0.0, 0.0]);
    // dcl_2d s0; mov r1, c0; texld r0, r1, s0; mov oC0, r0
    second.pixel_shader = vec![
        0xffff0300, 0x0200001f, 0x90000000, 0xa00f0800, 0x02000001, 0x800f0001, 0xa0e40000, 0x03000042,
        0x800f0000, 0x80e40001, 0xa0e40800, 0x02000001, 0x800f0800, 0x80e40000, 0xffff,
    ]
    .into();
    let mut states = [0; 14];
    states[1..4].fill(3);
    states[5] = 1;
    states[6] = 1;
    second.textures = vec![TextureBinding {
        sampler: 0, resource: 70, kind: TexKind::Flat, format: 21, width: 8, height: 8, depth: 1, levels: 1,
        usage: USAGE_RENDERTARGET, states,
    }]
    .into();
    backend.clear(&clear(&t, None));
    backend.draw(&legacy(&first, &t, None));
    backend.draw(&legacy(&second, &t, None));
    let expected = backend.read_back(&t).unwrap();
    backend.clear(&clear(&t, None));
    let job = NativeJob {
        passes: vec![NativePass { colours: [Some(t.clone()), None, None, None], draws: vec![first, second], ..Default::default() }],
    };
    assert!(backend.native(job).is_ok(), "{:?}", backend.native_job_stats().rejection_reasons);
    assert_eq!(backend.read_back(&t).unwrap(), expected);
    // the second draw sampled the first's colour, not the clear's
    let p = &expected[..4];
    assert!(p[0] == 255 && p[1] > 100 && p[2] == 0 && p[3] == 255, "BGRA of (0, 0.5, 1, 1): {p:?}");
    assert!(backend.errors.is_empty(), "{:?}", backend.errors);
}

/// L141: native jobs no longer wait for the finisher to submit all earlier
/// work; each job's constant deltas go to a scatter source no pending batch
/// reads. Many jobs in a row (more than the sources), each a different
/// colour into its own target, with the finisher asynchronous and no read
/// in between: every target holds its own job's colour.
#[test]
fn consecutive_native_jobs_keep_their_own_constants_without_waiting() {
    let Some(mut backend) = ddgpu::WgpuBackend::headless() else {
        return;
    };
    backend.set_async_submit(true);
    upload(&mut backend);
    let n = 12;
    let targets: Vec<Target> = (0..n).map(|k| target(100 + k, 21)).collect();
    for (k, t) in targets.iter().enumerate() {
        let v = (k as f32 + 1.0) / 16.0;
        let mut j = job(draw(false, false, false, [v, 1.0 - v, 0.5, 1.0]), t, None);
        j.passes[0].clear_colour = Some(0xff00_0000);
        assert!(backend.native(j).is_ok());
    }
    let images: Vec<Vec<u8>> = targets.iter().map(|t| backend.read_back(t).unwrap()).collect();
    for (k, image) in images.iter().enumerate() {
        let v = (k as f32 + 1.0) / 16.0;
        // A8R8G8B8 read back as BGRA
        let want = [(0.5f32 * 255.0).round() as i32, ((1.0 - v) * 255.0).round() as i32, (v * 255.0).round() as i32, 255];
        let p = &image[..4];
        assert!(p.iter().zip(want).all(|(&g, w)| (g as i32 - w).abs() <= 1), "job {k}: {p:?} vs {want:?}");
    }
    assert!(backend.errors.is_empty(), "{:?}", backend.errors);
}
