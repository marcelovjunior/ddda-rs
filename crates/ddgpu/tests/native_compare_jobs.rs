//! L127 integration: real D3D9 output plus isolated native shadow jobs.
use d3d9cmd::*;
use ddgpu::{native_compare::*, WgpuBackend};
use std::sync::Arc;

fn target() -> Target {
    Target {
        surface: 71,
        format: 21,
        width: 8,
        height: 8,
        texture: None,
        level: 0,
        samples: 0,
        quality: 0,
    }
}
fn clear(target: &Target) -> ClearCall {
    ClearCall {
        rects: vec![],
        flags: 1,
        colour: 0xff0000ff,
        z: 1.0,
        stencil: 0,
        targets: [Some(target.clone()), None, None, None],
        depth: None,
        viewport: [0, 0, 8, 8, 0, 1f32.to_bits()],
        scissor: None,
        srgb: false,
    }
}
fn fixture(backend: &mut WgpuBackend, target: &Target) -> (NativeJob, DrawCall) {
    backend.upload(Upload::Buffer {
        resource: 88,
        bytes: [
            [-1.0f32, -1.0, 0.5, 1.0],
            [-1.0, 3.0, 0.5, 1.0],
            [3.0, -1.0, 0.5, 1.0],
        ]
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect(),
    });
    let vs: Arc<[u32]> = vec![
        0xfffe0300, 0x0200001f, 0x80000000, 0x900f0000, 0x0200001f, 0x80000000, 0xe00f0000, 0x02000001,
        0xe00f0000, 0x90e40000, 0xffff,
    ]
    .into();
    let ps: Arc<[u32]> = vec![0xffff0300, 0x02000001, 0x800f0800, 0xa0e40000, 0xffff].into();
    let declaration: Arc<[[u8; 8]]> = vec![[0, 0, 0, 0, 3, 0, 0, 0]].into();
    let states: Arc<[(u32, u32)]> = vec![
        (7, 0),
        (8, 3),
        (15, 0),
        (19, 5),
        (20, 6),
        (22, 1),
        (27, 1),
        (168, 15),
        (171, 1),
    ]
    .into();
    let colour = [1.0f32, 0.0, 0.0, 0.5].map(f32::to_bits);
    let draw = NativeDraw {
        state: Arc::new(NativeFixedState {
            identity: Default::default(),
            render_states: states.clone(),
        }),
        vertex_shader: vs.clone(),
        pixel_shader: ps.clone(),
        declaration: declaration.clone(),
        textures: Arc::from([]),
        streams: vec![NativeStream {
            stream: 0,
            resource: 88,
            offset: 0,
            stride: 16,
        }]
        .into(),
        indices: None,
        vs_constants: Default::default(),
        ps_constants: Arc::new(NativeConstants {
            floats: vec![NativeFloatWrite {
                first: 0,
                registers: vec![colour].into(),
            }],
            ..Default::default()
        }),
        viewport: [0, 0, 8, 8, 0, 1f32.to_bits()],
        scissor: None,
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        instances: 1,
    };
    let oracle = DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Streams(vec![(0, 88, 0, 16, 1)].into()),
        indices: Indices::None,
        vertex_shader: Some(vs),
        pixel_shader: Some(ps),
        declaration,
        fvf: 0,
        vs_constants: Default::default(),
        ps_constants: Arc::new(Constants {
            f: vec![colour],
            ..Default::default()
        }),
        textures: Arc::from([]),
        render_states: states.to_vec().into(),
        viewport: draw.viewport,
        scissor: [0, 0, 8, 8],
        targets: [Some(target.clone()), None, None, None],
        depth: None,
    };
    let job = NativeJob {
        passes: vec![NativePass {
            colours: oracle.targets.clone(),
            draws: vec![draw],
            ..Default::default()
        }],
    };
    (job, oracle)
}
fn begin(id: u64, target: &Target) -> NativeCompareBegin {
    NativeCompareBegin {
        id,
        colours: [Some(target.clone()), None, None, None],
        depth: None,
    }
}
fn wait_results(backend: &mut WgpuBackend, expected: usize) -> Vec<CompareResult> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut reports = Vec::new();
    while reports.len() < expected {
        reports.extend(backend.poll_native_comparisons().into_iter().map(Result::unwrap));
        assert!(std::time::Instant::now() < deadline, "comparison result timeout");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    reports
}

#[test]
fn native_shadow_loads_pre_oracle_pixels_and_never_changes_visible_output() {
    let mut backend = WgpuBackend::headless().expect("test GPU");
    backend.read_frames = false;
    let target = target();
    let (job, oracle) = fixture(&mut backend, &target);
    let mut mismatched = job.clone();
    Arc::make_mut(&mut mismatched.passes[0].draws[0].ps_constants).floats[0].registers =
        vec![[0.0f32, 1.0, 0.0, 0.5].map(f32::to_bits)].into();
    for (id, job) in [(100, job), (101, mismatched)] {
        backend.clear(&clear(&target));
        assert!(backend.native_compare_begin(begin(id, &target)));
        backend.draw(&oracle);
        let before = backend.read_surface(target.surface).unwrap();
        backend.native_compare_end(id, job).unwrap();
        let after = backend.read_surface(target.surface).unwrap();
        assert_eq!(before.rgba, after.rgba, "comparison changed oracle output");
        let reports = wait_results(&mut backend, 1);
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(
            report.tag,
            CompareTag {
                pass: id,
                target: target.surface,
                aspect: CompareAspect::Color
            }
        );
        assert_eq!(report.pixels, 64);
        if id == 100 {
            assert_eq!(
                report.differing_pixels, 0,
                "shadow must start before alpha-blended oracle draw: {report}"
            );
            assert_eq!(report.max_channel_diff, 0.0);
        } else {
            assert_eq!(report.differing_pixels, 64);
            assert!(report.max_channel_diff > 0.49 && report.max_channel_diff < 0.51);
        }
    }
    assert!(backend.errors.is_empty(), "{:?}", backend.errors);
}

#[test]
fn failed_comparison_cannot_write_oracle_targets_and_next_pass_recovers() {
    let mut backend = WgpuBackend::headless().expect("test GPU");
    backend.read_frames = false;
    let target = target();
    let (job, oracle) = fixture(&mut backend, &target);
    backend.clear(&clear(&target));
    assert!(backend.native_compare_begin(begin(200, &target)));
    assert!(!backend.native_compare_begin(begin(201, &target)));
    assert!(backend.native_compare_end(201, job.clone()).is_err());
    backend.draw(&oracle);
    let expected = backend.read_surface(target.surface).unwrap();
    let mut invalid = job.clone();
    invalid.passes[0].draws[0].instances = 2;
    assert!(backend.native_compare_end(200, invalid).is_err());
    assert_eq!(backend.read_surface(target.surface).unwrap().rgba, expected.rgba);
    assert!(backend.native_compare_end(999, job.clone()).is_err());
    // Reject unsupported MSAA at begin before allocating/remapping targets.
    let mut msaa = target.clone();
    msaa.samples = 4;
    assert!(!backend.native_compare_begin(begin(201, &msaa)));
    let rejected = backend.poll_native_comparisons();
    assert_eq!(rejected.len(), 5);
    assert!(rejected.iter().all(Result::is_err));
    backend.clear(&clear(&target));
    assert!(backend.native_compare_begin(begin(202, &target)));
    backend.draw(&oracle);
    backend.native_compare_end(202, job).unwrap();
    assert_eq!(wait_results(&mut backend, 1)[0].differing_pixels, 0);
}

#[test]
fn repeated_jobs_reuse_shadows_without_losing_pass_identity() {
    let mut backend = WgpuBackend::headless().expect("test GPU");
    backend.set_async_submit(true);
    backend.read_frames = false;
    let target = target();
    let (job, oracle) = fixture(&mut backend, &target);
    for id in 300..332 {
        backend.clear(&clear(&target));
        assert!(backend.native_compare_begin(begin(id, &target)));
        backend.draw(&oracle);
        backend.native_compare_end(id, job.clone()).unwrap();
    }
    let mut ids = std::collections::BTreeSet::new();
    for report in wait_results(&mut backend, 32) {
        assert_eq!(report.pixels, 64);
        assert_eq!(report.differing_pixels, 0, "{report}");
        assert!(ids.insert(report.tag.pass));
    }
    assert_eq!(ids, (300..332).collect());
    assert!(backend.errors.is_empty(), "{:?}", backend.errors);
}

#[test]
fn depth_stencil_load_state_is_copied_and_compared_without_changing_oracle() {
    let mut backend = WgpuBackend::headless().expect("test GPU");
    backend.read_frames = false;
    let colour = target();
    let depth = Target {
        surface: 72,
        format: 75,
        ..colour.clone()
    };
    let mut initial = clear(&colour);
    initial.depth = Some(depth.clone());
    initial.flags = 7;
    initial.z = 0.25;
    initial.stencil = 7;
    backend.clear(&initial);
    for id in 400..403 {
        let mut marker = begin(id, &colour);
        marker.depth = Some(depth.clone());
        assert!(backend.native_compare_begin(marker));
        let mut pass = NativePass {
            colours: initial.targets.clone(),
            depth: Some(depth.clone()),
            ..Default::default()
        };
        if id == 401 {
            pass.clear_depth = Some(0.75f32.to_bits());
            pass.clear_stencil = Some(9);
        } else if id == 402 {
            pass.clear_depth = Some(0.25f32.to_bits());
            pass.clear_stencil = Some(7);
        }
        backend
            .native_compare_end(id, NativeJob { passes: vec![pass] })
            .unwrap();
        // The third shadow writes the known original .25/7. It only matches
        // if the previous native-only clear left real depth/stencil untouched.
        let reports = wait_results(&mut backend, 3);
        assert_eq!(reports.len(), 3);
        for report in reports {
            assert_eq!(report.pixels, 64);
            let mismatched = id == 401 && report.tag.aspect != CompareAspect::Color;
            assert_eq!(
                report.differing_pixels,
                if mismatched { 64 } else { 0 },
                "{report}"
            );
            if mismatched {
                let expected = if report.tag.aspect == CompareAspect::Depth {
                    0.5
                } else {
                    2.0
                };
                assert!((report.max_channel_diff - expected).abs() < 0.000001);
            }
        }
    }
    assert_eq!(
        backend.read_surface(colour.surface).unwrap().rgba,
        [0, 0, 255, 255].repeat(64)
    );
}

#[test]
fn texture_mip_attachments_compare_the_selected_subresource() {
    let mut backend = WgpuBackend::headless().expect("test GPU");
    backend.read_frames = false;
    let mut target = target();
    target.level = 1;
    target.texture = Some(TargetTexture {
        resource: 90,
        kind: TexKind::Flat,
        width: 16,
        height: 16,
        depth: 1,
        levels: 2,
        usage: USAGE_RENDERTARGET,
    });
    let (job, oracle) = fixture(&mut backend, &target);
    backend.clear(&clear(&target));
    assert!(backend.native_compare_begin(begin(500, &target)));
    backend.draw(&oracle);
    let before = backend.read_back(&target).unwrap();
    backend.native_compare_end(500, job).unwrap();
    assert_eq!(backend.read_back(&target).unwrap(), before);
    let reports = wait_results(&mut backend, 1);
    assert_eq!(reports[0].tag.pass, 500);
    assert_eq!(reports[0].pixels, 64);
    assert_eq!(reports[0].differing_pixels, 0);
}
