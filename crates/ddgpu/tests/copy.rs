//! Copy ordering and submission boundaries; the MSAA/targets suites supply
//! the driver oracle for filtering, formats, depth and stencil precision.
use d3d9cmd::{ClearCall, CopyCall, GraphicsBackend, Target, TargetTexture, TexKind, Upload, USAGE_RENDERTARGET};

fn target(id: u32, format: u32, size: u32, samples: u32) -> Target {
    Target { surface: id, format, width: size, height: size, samples, quality: 0, texture: None, level: 0 }
}

fn clear(gpu: &mut ddgpu::WgpuBackend, target: &Target, colour: u32) {
    gpu.clear(&ClearCall {
        rects: vec![], flags: 1, colour, z: 1.0, stencil: 0,
        targets: [Some(target.clone()), None, None, None], depth: None,
        viewport: [0, 0, target.width, target.height, 0, 1f32.to_bits()], scissor: None, srgb: false,
    });
}

fn copy(gpu: &mut ddgpu::WgpuBackend, src: &Target, dst: &Target) {
    gpu.copy(&CopyCall { src: src.clone(), dst: dst.clone(), src_rect: [0; 4], dst_rect: [0; 4], filter: 0 });
}

#[test]
fn copy_chains_record_without_submitting_and_keep_command_order() {
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter");
    gpu.read_frames = false;
    let src = target(0x100, 21, 8, 0);
    let scaled = target(0x101, 21, 4, 0);
    let converted = target(0x102, 113, 4, 0);
    let restored = target(0x103, 21, 4, 0);
    let resolved = target(0x104, 21, 8, 0);
    let msaa = target(0x105, 21, 8, 4);
    let resolved_small = target(0x106, 21, 4, 0);
    let roundtrip = target(0x107, 21, 8, 0);
    let before = gpu.submission_count();
    clear(&mut gpu, &src, 0xFF00FF00);
    copy(&mut gpu, &src, &scaled);
    copy(&mut gpu, &scaled, &converted);
    copy(&mut gpu, &converted, &restored);
    clear(&mut gpu, &src, 0xFFFF0000); // must not change the already copied green
    clear(&mut gpu, &msaa, 0xFF0000FF);
    copy(&mut gpu, &msaa, &resolved);
    copy(&mut gpu, &msaa, &resolved_small); // resolve then blit in the same encoder
    copy(&mut gpu, &src, &msaa); // single-sample source into multisample destination
    copy(&mut gpu, &msaa, &roundtrip);
    assert_eq!(gpu.submission_count(), before, "copies must only record");
    gpu.present(&restored);
    assert_eq!(gpu.submission_count(), before + 1);
    assert_eq!(gpu.read_back(&restored).unwrap(), [0, 255, 0, 255].repeat(16));
    assert_eq!(gpu.read_back(&resolved).unwrap(), [255, 0, 0, 255].repeat(64));
    assert_eq!(gpu.read_back(&resolved_small).unwrap(), [255, 0, 0, 255].repeat(16));
    assert_eq!(gpu.read_back(&roundtrip).unwrap(), [0, 0, 255, 255].repeat(64));
    assert_eq!(gpu.read_back(&src).unwrap(), [0, 0, 255, 255].repeat(64));
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
}

#[test]
fn depth_copy_paths_do_not_submit() {
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter");
    gpu.read_frames = false;
    for (index, (ss, ds)) in [(0, 0), (4, 0), (4, 4)].into_iter().enumerate() {
        let id = 0x200 + index as u32 * 4;
        let src = target(id, 75, 8, ss);
        let dst = target(id + 1, 75, 8, ds);
        let colour = target(id + 2, 21, 8, ss);
        let before = gpu.submission_count();
        gpu.clear(&ClearCall {
            rects: vec![], flags: 7, colour: 0, z: 0.375, stencil: 0xA5,
            targets: [Some(colour.clone()), None, None, None], depth: Some(src.clone()),
            viewport: [0, 0, 8, 8, 0, 1f32.to_bits()], scissor: None, srgb: false,
        });
        copy(&mut gpu, &src, &dst);
        assert_eq!(gpu.submission_count(), before, "{ss} -> {ds}");
        gpu.present(&colour);
        assert_eq!(gpu.submission_count(), before + 1);
        assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    }
}

#[test]
fn upload_after_scaled_copy_preserves_the_copied_source() {
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter");
    let mut src = target(0x300, 21, 8, 0);
    src.texture = Some(TargetTexture { resource: 0x310, kind: TexKind::Flat, width: 8, height: 8, depth: 1, levels: 1, usage: 0 });
    let dst = target(0x301, 21, 4, 0);
    let upload = |pixel: [u8; 4]| Upload::Image {
        resource: 0x310, index: 0, format: 21, width: 8, height: 8, depth: 1,
        pitch: 32, slice: 256, bytes: pixel.repeat(64),
    };
    gpu.upload(upload([0, 255, 0, 255]));
    copy(&mut gpu, &src, &dst);
    let before = gpu.submission_count();
    gpu.upload(upload([0, 0, 255, 255]));
    assert_eq!(gpu.submission_count(), before + 1, "upload must submit prior reads of the resource");
    assert_eq!(gpu.read_back(&dst).unwrap(), [0, 255, 0, 255].repeat(16));
    let mut dst_texture = target(0x302, 21, 4, 0);
    dst_texture.texture = Some(TargetTexture { resource: 0x311, kind: TexKind::Flat, width: 4, height: 4, depth: 1, levels: 1, usage: USAGE_RENDERTARGET });
    copy(&mut gpu, &src, &dst_texture);
    let before = gpu.submission_count();
    gpu.upload(Upload::Image {
        resource: 0x311, index: 0, format: 21, width: 4, height: 4, depth: 1,
        pitch: 16, slice: 64, bytes: [255, 0, 0, 255].repeat(16),
    });
    assert_eq!(gpu.submission_count(), before + 1, "upload must also submit prior writes to its destination");
    assert_eq!(gpu.read_back(&dst_texture).unwrap(), [255, 0, 0, 255].repeat(16));
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
}
