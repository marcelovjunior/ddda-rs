//! Synthetic GPU comparisons only: no installed data or game window.
use ddgpu::native_compare::*;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

fn gpu() -> (wgpu::Device, wgpu::Queue) {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(desc);
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("Vulkan test adapter");
    block_on(adapter.request_device(&wgpu::DeviceDescriptor { ..Default::default() })).unwrap()
}

fn texture(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("comparison fixture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, bytes: &[u8]) {
    queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(texture.width() * texture.format().block_copy_size(None).unwrap()),
            rows_per_image: Some(texture.height()),
        },
        texture.size(),
    );
}

fn finish(compare: &mut CompareQueue, device: &wgpu::Device, expected: usize) -> Vec<CompareResult> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut out = Vec::new();
    while out.len() < expected {
        out.extend(compare.poll(device).unwrap().into_iter().map(Result::unwrap));
        assert!(std::time::Instant::now() < deadline, "async comparison timed out");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(compare.pending(), 0);
    out
}

#[test]
fn colour_summaries_are_exact_and_keep_pass_snapshots() {
    let (device, queue) = gpu();
    let mut compare = CompareQueue::new(&device, 8);
    for (pass, format) in [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureFormat::Rgba32Float,
    ]
    .into_iter()
    .enumerate()
    {
        let a = texture(&device, format, 9, 17);
        let b = texture(&device, format, 9, 17);
        let bpp = format.block_copy_size(None).unwrap() as usize;
        let mut bytes = vec![0u8; 153 * bpp];
        upload(&queue, &a, &bytes);
        upload(&queue, &b, &bytes);
        let av = a.create_view(&Default::default());
        let bv = b.create_view(&Default::default());
        let tag = CompareTag {
            pass: pass as u64 * 2,
            target: 77,
            aspect: CompareAspect::Color,
        };
        compare.enqueue(&device, &queue, tag, &av, &bv, 9, 17).unwrap();
        // Queue ordering must preserve the previous equality before this write.
        for pixel in [0, 74, 152] {
            match format {
                wgpu::TextureFormat::Rgba16Float => {
                    bytes[pixel * bpp..pixel * bpp + 2].copy_from_slice(&0x3c00u16.to_le_bytes())
                }
                wgpu::TextureFormat::Rgba32Float => {
                    bytes[pixel * bpp..pixel * bpp + 4].copy_from_slice(&1.0f32.to_le_bytes())
                }
                _ => bytes[pixel * bpp] = 255,
            }
        }
        upload(&queue, &b, &bytes);
        compare
            .enqueue(
                &device,
                &queue,
                CompareTag {
                    pass: tag.pass + 1,
                    ..tag
                },
                &av,
                &bv,
                9,
                17,
            )
            .unwrap();
    }
    for result in finish(&mut compare, &device, 8) {
        assert_eq!(result.pixels, 153);
        assert_eq!(
            result.differing_pixels,
            if result.tag.pass % 2 == 0 { 0 } else { 3 }
        );
        assert_eq!(
            result.max_channel_diff,
            if result.tag.pass % 2 == 0 { 0.0 } else { 1.0 }
        );
        assert_eq!(result.nonfinite_pixels, 0);
    }
}

#[test]
fn float_nonfinite_signed_zero_and_bounded_queue() {
    let (device, queue) = gpu();
    let mut compare = CompareQueue::new(&device, 1);
    let a = texture(&device, wgpu::TextureFormat::Rgba32Float, 3, 1);
    let b = texture(&device, wgpu::TextureFormat::Rgba32Float, 3, 1);
    let baseline = [0.0f32, 0., 0., 0., 1., 0., 0., 0., f32::INFINITY, 0., 0., 0.];
    let changed = [
        -0.0f32,
        0.,
        0.,
        0.,
        f32::INFINITY,
        0.,
        0.,
        0.,
        f32::INFINITY,
        0.,
        0.,
        0.,
    ];
    upload(
        &queue,
        &a,
        &baseline
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    upload(
        &queue,
        &b,
        &changed.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>(),
    );
    let av = a.create_view(&Default::default());
    let bv = b.create_view(&Default::default());
    let tag = CompareTag {
        pass: 99,
        target: 3,
        aspect: CompareAspect::Color,
    };
    assert!(compare
        .enqueue(&device, &queue, tag, &av, &bv, 0, 1)
        .unwrap_err()
        .contains("unsupported extent"));
    compare.enqueue(&device, &queue, tag, &av, &bv, 3, 1).unwrap();
    assert_eq!(compare.available(), 0);
    assert!(compare
        .enqueue(&device, &queue, tag, &av, &bv, 3, 1)
        .unwrap_err()
        .contains("capacity exhausted"));
    let result = finish(&mut compare, &device, 1).remove(0);
    assert_eq!(result.pixels, 3);
    assert_eq!(result.differing_pixels, 2);
    assert_eq!(result.max_channel_diff, f32::INFINITY);
    assert_eq!(result.nonfinite_pixels, 1);
    assert_eq!(compare.available(), 1);
}

#[test]
fn depth_and_stencil_are_compared_independently() {
    let (device, queue) = gpu();
    let a = texture(&device, wgpu::TextureFormat::Depth24PlusStencil8, 9, 17);
    let b = texture(&device, wgpu::TextureFormat::Depth24PlusStencil8, 9, 17);
    let mut encoder = device.create_command_encoder(&Default::default());
    for (texture, depth, stencil) in [(&a, 0.25, 7), (&b, 0.75, 9)] {
        let view = texture.create_view(&Default::default());
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(depth),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(stencil),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            ..Default::default()
        });
    }
    queue.submit([encoder.finish()]);
    let mut compare = CompareQueue::new(&device, 2);
    for (aspect, gpu_aspect) in [
        (CompareAspect::Depth, wgpu::TextureAspect::DepthOnly),
        (CompareAspect::Stencil, wgpu::TextureAspect::StencilOnly),
    ] {
        let desc = wgpu::TextureViewDescriptor {
            aspect: gpu_aspect,
            ..Default::default()
        };
        let tag = CompareTag {
            pass: 44,
            target: 55,
            aspect,
        };
        compare
            .enqueue(
                &device,
                &queue,
                tag,
                &a.create_view(&desc),
                &b.create_view(&desc),
                9,
                17,
            )
            .unwrap();
    }
    for result in finish(&mut compare, &device, 2) {
        assert_eq!(result.pixels, 153);
        assert_eq!(result.differing_pixels, 153);
        let expected = if result.tag.aspect == CompareAspect::Depth {
            0.5
        } else {
            2.0
        };
        assert!((result.max_channel_diff - expected).abs() < 0.000001);
    }
}

#[test]
fn full_scene_extent_and_many_pending_passes() {
    let (device, queue) = gpu();
    let mut compare = CompareQueue::new(&device, 128);
    let mut expected = std::collections::HashMap::new();
    let started = std::time::Instant::now();
    for (width, height, passes) in [(1920, 1080, 8), (64, 8, 120)] {
        let a = texture(&device, wgpu::TextureFormat::Rgba8Unorm, width, height);
        let b = texture(&device, wgpu::TextureFormat::Rgba8Unorm, width, height);
        let mut bytes = vec![0; (width * height * 4) as usize];
        upload(&queue, &a, &bytes);
        // Check the final pixel so incomplete edge tiles cannot silently pass.
        let last = bytes.len() - 1;
        bytes[last] = 255;
        upload(&queue, &b, &bytes);
        let av = a.create_view(&Default::default());
        let bv = b.create_view(&Default::default());
        for _ in 0..passes {
            let tag = CompareTag {
                pass: expected.len() as u64,
                target: width,
                aspect: CompareAspect::Color,
            };
            expected.insert(tag.pass, width * height);
            compare
                .enqueue(&device, &queue, tag, &av, &bv, width, height)
                .unwrap();
        }
    }
    assert_eq!(compare.pending(), 128);
    for result in finish(&mut compare, &device, 128) {
        assert_eq!(result.pixels, expected.remove(&result.tag.pass).unwrap());
        assert_eq!(result.differing_pixels, 1);
        assert_eq!(result.max_channel_diff, 1.0);
    }
    assert!(expected.is_empty());
    eprintln!(
        "128 target summaries (8 full-HD + 120 tiny), 2048 readback bytes, {:?}",
        started.elapsed()
    );
}
