//! Actual texture readback for borrowed uploads, row compaction and pending
//! replay. Each case is small and CPU preparation is tested independently.
use d3d9cmd::{GraphicsBackend, Target, TargetTexture, TexKind, Upload};

#[test]
fn tight_and_padded_image_uploads_keep_pending_and_existing_texture_bytes() {
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter");
    gpu.read_frames = false;
    for (case, (width, height, padding)) in [(1u32, 1u32, 0u32), (3, 2, 0), (7, 5, 12)].into_iter().enumerate() {
        let resource = 0x14700 + case as u32;
        let target = Target {
            surface: resource + 0x100, format: 21, width, height, samples: 0, quality: 0, level: 0,
            texture: Some(TargetTexture { resource, kind: TexKind::Flat, width, height, depth: 1, levels: 1, usage: 0 }),
        };
        // First upload waits for texture creation; second updates it in place.
        for seed in [13u32, 97] {
            let pitch = width * 4 + padding;
            let mut source = vec![0xef; (pitch * (height - 1) + width * 4) as usize];
            let mut expected = Vec::new();
            for y in 0..height {
                for x in 0..width {
                    let pixel = [(seed + x * 3) as u8, (seed + y * 7) as u8, (seed + x + y) as u8, 255];
                    let at = (y * pitch + x * 4) as usize;
                    source[at..at + 4].copy_from_slice(&pixel);
                    expected.extend_from_slice(&pixel);
                }
            }
            gpu.upload(Upload::Image { resource, index: 0, format: 21, width, height, depth: 1,
                                       pitch, slice: pitch * height, bytes: source });
            assert_eq!(gpu.read_back(&target).unwrap(), expected, "case {case}, seed {seed}");
        }
    }
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
}
