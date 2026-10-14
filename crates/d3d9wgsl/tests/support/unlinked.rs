//! Hand-authored SM3 input regressions; no game bytecode fixtures.
fn input_shader(register: u32, declared: bool) -> Vec<u32> {
    let mut tokens = vec![0xffff_0300];
    if declared {
        tokens.extend([0x0200_001f, 0x8000_0005, 0x900f_0000 | register]);
    }
    tokens.extend([0x0200_0001, 0x800f_0800, 0x90e4_0000 | register, 0x0000_ffff]);
    tokens
}

#[test]
fn input_registers_validate_without_a_vertex_output() {
    for register in [0, 3, 9] {
        let shader = d3d9shader::parse(&input_shader(register, true)).unwrap();
        let translation = d3d9wgsl::translate_with(
            &shader,
            d3d9wgsl::Options {
                ps_input_locations: Some(vec![None]),
                ..Default::default()
            },
        )
        .unwrap();
        let module = naga::front::wgsl::parse_str(&translation.wgsl)
            .unwrap_or_else(|e| panic!("v{register}: {}", e.emit_to_string(&translation.wgsl)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap();
        assert_eq!(translation.interface.inputs, vec![(5, 0, register)]);
    }
}

#[test]
fn undeclared_input_is_rejected_by_d3d9() {
    let Some(mut oracle) = d3d9ref::Oracle::new() else {
        return;
    };
    for register in [0, 3, 9] {
        let result = oracle.run_pixel(&input_shader(register, false), &d3d9ref::PixelRun::default());
        eprintln!("undeclared v{register}: {result:?}");
        assert!(result
            .unwrap_err()
            .contains("CreatePixelShader failed: 0x8876086c"));
    }
}

#[test]
fn unlinked_inputs_match_the_driver() {
    // Microsoft calls the missing-semantic pairing invalid, rather than
    // specifying a default: https://learn.microsoft.com/en-us/windows/win32/direct3dhlsl/shader-model-3
    // The semantic-dependent fallback is measured driver behavior, not a
    // portable D3D9 guarantee.
    use d3d9cmd::{ClearCall, Constants, DrawCall, Indices, Target, Vertices};
    let (Some(mut oracle), Some(gpu)) = (d3d9ref::Oracle::new(), super::Gpu::new()) else {
        return;
    };
    let target = Target {
        surface: 0x100,
        format: 116, // A32B32G32R32F: compare exact floats, without UNORM clamping.
        width: 4,
        height: 4,
        texture: None,
        level: 0,
        samples: 0,
        quality: 0,
    };
    // Position-only VS: deliberately no TEXCOORD output to match the PS.
    let vs = vec![
        0xfffe_0300,
        0x0200_001f,
        0x8000_0000,
        0x900f_0000,
        0x0200_001f,
        0x8000_0000,
        0xe00f_0000,
        0x0200_0001,
        0xe00f_0000,
        0x90e4_0000,
        0x0000_ffff,
    ];
    let vertices = [
        [-1.0f32, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
    ];
    let draw = DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Inline {
            bytes: vertices.iter().flatten().flat_map(|v| v.to_le_bytes()).collect(),
            stride: 16,
        },
        indices: Indices::None,
        vertex_shader: Some(vs),
        pixel_shader: Some(input_shader(0, true)),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]],
        fvf: 0,
        vs_constants: Constants::default(),
        ps_constants: Constants::default(),
        textures: vec![],
        render_states: vec![(7, 0), (15, 0), (22, 1), (27, 0), (168, 15)],
        viewport: [0, 0, 4, 4, 0, 1f32.to_bits()],
        scissor: [0, 0, 4, 4],
        targets: [Some(target.clone()), None, None, None],
        depth: None,
    };
    let clear = ClearCall {
        rects: vec![],
        flags: 1,
        colour: 0xffff00ff,
        z: 1.0,
        stencil: 0,
        targets: [Some(target.clone()), None, None, None],
        depth: None,
        viewport: [0, 0, 4, 4, 0, 0],
        scissor: None,
        srgb: false,
    };
    let mut cases = Vec::new();
    for (usage, index) in [(5, 0), (5, 7), (10, 0), (10, 1)] {
        for register in [0, 3, 9] {
            let mut tokens = input_shader(register, true);
            tokens[2] = 0x8000_0000 | usage | (index << 16);
            let expected = if usage == 10 && index == 0 { 1.0f32 } else { 0.0 };
            cases.push((
                format!("usage {usage}:{index} v{register}"),
                tokens,
                [expected; 4],
            ));
        }
    }
    // Read the real regression from the player's install; never embed game
    // bytecode. This root shader divides COLOR0.x by COLOR0.y. With the
    // driver's missing-COLOR0 white input it returns (1,1,0,1), not (0,0,0,1).
    let installed = super::pixel_shaders("root");
    if !installed.is_empty() {
        let tokens = installed
            .into_iter()
            .find(|t| super::tag(t) == "2681fe2cf6b72f46")
            .expect("root COLOR0 regression shader");
        cases.push(("root:2681fe2cf6b72f46".into(), tokens, [1.0, 1.0, 0.0, 1.0]));
    }
    for (label, tokens, expected) in cases {
        let mut draw = draw.clone();
        draw.pixel_shader = Some(tokens.clone());
        draw.ps_constants.f = vec![[0; 4]; 224];
        let frames = oracle
            .render_targets(&[], Some(&clear), &[draw], &[target.clone()])
            .unwrap();
        assert_eq!(frames[0].len(), 4 * 4 * 16);
        for pixel in frames[0].chunks_exact(16) {
            for (component, expected) in pixel.chunks_exact(4).zip(expected) {
                assert_eq!(
                    u32::from_le_bytes(component.try_into().unwrap()),
                    expected.to_bits(),
                    "D3D9 {label}"
                );
            }
        }
        let shader = d3d9shader::parse(&tokens).unwrap();
        let t = d3d9wgsl::translate_with(
            &shader,
            d3d9wgsl::Options {
                ps_input_locations: Some(vec![None]),
                ..Default::default()
            },
        )
        .unwrap();
        // Omit the harness VS outputs as well as the fragment input fields.
        let mut harness_interface = t.interface.clone();
        harness_interface.inputs.clear();
        let ours = gpu
            .run(&t.wgsl, &harness_interface, &d3d9ref::PixelRun::default())
            .unwrap();
        assert_eq!(ours, vec![expected], "WGSL {label}");
    }
}
