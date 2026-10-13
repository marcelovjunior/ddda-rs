//! Scratch lists must not leak state across errors, release or inline draws.
use d3d9cmd::*;

fn target() -> Target {
    Target { surface: 0x1ffff, format: 21, width: 4, height: 4,
        texture: None, level: 0, samples: 0, quality: 0 }
}

fn vertices() -> Vec<u8> {
    [[-1.0f32, -1.0, 0.0, 1.0], [-1.0, 3.0, 0.0, 1.0], [3.0, -1.0, 0.0, 1.0]]
        .into_iter().flatten().flat_map(f32::to_le_bytes).collect()
}

fn draw() -> DrawCall {
    DrawCall {
        primitive: 4, primitives: 1, start: 0, base_vertex: 0, min_index: 0, vertex_count: 3,
        vertices: Vertices::Streams(vec![(0, 0xffff_ffff, 0, 16, 1)].into()),
        indices: Indices::None,
        vertex_shader: Some(vec![0xfffe0300, 0x0200001f, 0x80000000, 0x900f0000,
            0x0200001f, 0x80000000, 0xe00f0000, 0x02000001, 0xe00f0000, 0x90e40000, 0xffff].into()),
        pixel_shader: Some(vec![0xffff0300, 0x02000001, 0x800f0800, 0xa0e40000, 0xffff].into()),
        declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(), fvf: 0,
        vs_constants: Constants::default().into(),
        ps_constants: Constants { f: vec![[1f32.to_bits(), 0, 0, 1f32.to_bits()]], ..Default::default() }.into(),
        textures: vec![].into(), render_states: vec![(7, 0), (22, 1), (168, 15)].into(),
        viewport: [0, 0, 4, 4, 0, 1f32.to_bits()], scissor: [0, 0, 4, 4],
        targets: [Some(target()), None, None, None], depth: None,
    }
}

#[test]
fn rejected_draw_then_resource_reuse_and_inline_match_clean_backend() {
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU acceptance requires an adapter");
    let mut clean = ddgpu::WgpuBackend::headless().expect("GPU acceptance requires an adapter");
    let good = draw();
    gpu.upload(Upload::Buffer { resource: 0xffff_ffff, bytes: vertices() });
    gpu.draw(&good);
    // Rejected after vertex buffers have been collected: the scratch handles
    // must still be released and the scratch length reset.
    let mut rejected = good.clone();
    rejected.primitive = 6;
    rejected.indices = Indices::Buffer(0x10000, 101);
    gpu.draw(&rejected);
    assert_eq!(gpu.errors.len(), 1);
    assert!(gpu.errors[0].contains("indexed triangle fans"), "{:?}", gpu.errors);
    gpu.release(0xffff_ffff);
    gpu.upload(Upload::Buffer { resource: 0xffff_ffff, bytes: vertices() });
    gpu.draw(&good);
    let mut inline = good.clone();
    inline.vertices = Vertices::Inline { bytes: vertices(), stride: 16 };
    inline.ps_constants = Constants { f: vec![[0, 1f32.to_bits(), 0, 1f32.to_bits()]], ..Default::default() }.into();
    gpu.draw(&inline);
    clean.draw(&inline);
    let actual = gpu.read_back(&target()).unwrap();
    let expected = clean.read_back(&target()).unwrap();
    assert_eq!(actual, expected);
    assert!(actual.chunks_exact(4).all(|p| p == [0, 255, 0, 255]));
    assert_eq!(gpu.errors.len(), 1, "only the deliberate rejection");
    assert!(clean.errors.is_empty(), "{:?}", clean.errors);
}
