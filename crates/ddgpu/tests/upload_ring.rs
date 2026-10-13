//! L150 exact legacy/ring upload comparisons. Synthetic data only; run serially.
use d3d9cmd::{ClearCall, Constants, DrawCall, GraphicsBackend, Indices, Target, TexKind, TextureBinding, Upload, Vertices};

const VS: [u32; 17] = [
    // vs_3_0; dcl_position v0; dcl_texcoord v1; dcl_position o0; dcl_texcoord o1
    0xFFFE_0300,
    0x0200_001F, 0x8000_0000, 0x900F_0000,
    0x0200_001F, 0x8000_0005, 0x900F_0001,
    0x0200_001F, 0x8000_0000, 0xE00F_0000,
    0x0200_001F, 0x8000_0005, 0xE00F_0001,
    // mov o0, v0 / mov o1, v1 (below), END
    0x0200_0001, 0xE00F_0000, 0x90E4_0000,
    0x0000_FFFF,
];

fn vs() -> Vec<u32> {
    let mut v = VS[..16].to_vec();
    v.extend_from_slice(&[0x0200_0001, 0xE00F_0001, 0x90E4_0001, 0x0000_FFFF]);
    v
}

/// ps_3_0; mov oC0, c0
fn ps_constant() -> Vec<u32> {
    vec![0xFFFF_0300, 0x0200_0001, 0x800F_0800, 0xA0E4_0000, 0x0000_FFFF]
}

/// ps_3_0; dcl_texcoord v0.xy; dcl_2d s0; texld oC0... via r0
fn ps_texture() -> Vec<u32> {
    vec![
        0xFFFF_0300,
        0x0200_001F, 0x8000_0005, 0x9003_0000, // dcl_texcoord v0.xy
        0x0200_001F, 0x9000_0000, 0xA00F_0800, // dcl_2d s0
        0x0300_0042, 0x800F_0000, 0x90E4_0000, 0xA0E4_0800, // texld r0, v0, s0
        0x0200_0001, 0x800F_0800, 0x80E4_0000, // mov oC0, r0
        0x0000_FFFF,
    ]
}

fn back_buffer() -> Target {
    Target { surface: 0x100, format: 21, width: 4, height: 4, texture: None, level: 0, samples: 0, quality: 0 }
}

/// position float4 at 0, texcoord float2 at 16 (stream 0)
fn declaration() -> Vec<[u8; 8]> {
    vec![[0, 0, 0, 0, 3, 0, 0, 0], [0, 0, 16, 0, 1, 0, 5, 0]]
}

fn render_states() -> Vec<(u32, u32)> {
    // CULLMODE none, ZENABLE off, COLORWRITEENABLE all
    let mut v = vec![(7, 0), (22, 1), (168, 0xF), (190, 0xF), (191, 0xF), (192, 0xF), (27, 0), (15, 0)];
    v.sort();
    v
}

/// A triangle covering the whole target, texcoords spanning [0, 2) (the
/// visible part [0, 1)), 24-byte vertices.
fn triangle() -> Vec<u8> {
    let v: [[f32; 6]; 3] = [[-1.0, -1.0, 0.0, 1.0, 0.0, 1.0], [-1.0, 3.0, 0.0, 1.0, 0.0, -1.0], [3.0, -1.0, 0.0, 1.0, 2.0, 1.0]];
    v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect()
}

fn draw(ps: Vec<u32>, ps_constants: Constants, textures: Vec<TextureBinding>) -> DrawCall {
    DrawCall {
        primitive: 4,
        primitives: 1,
        start: 0,
        base_vertex: 0,
        min_index: 0,
        vertex_count: 3,
        vertices: Vertices::Inline { bytes: triangle(), stride: 24 },
        indices: Indices::None,
        vertex_shader: Some((vs()).into()),
        pixel_shader: Some((ps).into()),
        declaration: declaration().into(),
        fvf: 0,
        vs_constants: Constants { f: vec![[0; 4]; 256], ..Default::default() }.into(),
        ps_constants: ps_constants.into(),
        textures: textures.into(),
        render_states: render_states().into(),
        viewport: [0, 0, 4, 4, 0f32.to_bits(), 1f32.to_bits()],
        scissor: [0, 0, 4, 4],
        targets: [Some(back_buffer()), None, None, None],
        depth: None,
    }
}


static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn backend(ring: bool, raw: bool, threads: usize) -> ddgpu::WgpuBackend {
    std::env::set_var("DDGPU_UPLOAD_RING", if ring { "1" } else { "0" });
    std::env::set_var("DDGPU_VK", if raw { "1" } else { "0" });
    let mut gpu = ddgpu::WgpuBackend::headless().expect("GPU adapter required");
    std::env::remove_var("DDGPU_UPLOAD_RING");
    std::env::remove_var("DDGPU_VK");
    gpu.set_encode_threads(threads);
    gpu.set_async_submit(threads > 1);
    gpu.draws_per_submit = 7;
    gpu.read_frames = false;
    gpu
}

fn sequence(gpu: &mut ddgpu::WgpuBackend) -> Vec<u8> {
    let mut images = Vec::new();
    let mut states = [0; 14];
    for k in [1, 2, 3, 5, 6] { states[k] = 1; }
    let binding = TextureBinding { sampler: 0, resource: 0x200, kind: TexKind::Flat,
        format: 21, width: 3, height: 2, depth: 1, levels: 1, usage: 0, states };
    let textured = draw(ps_texture(), Constants::default(), vec![binding]);
    for frame in 0..12u32 {
        gpu.clear(&ClearCall { rects: vec![], flags: 1, colour: 0xff123456, z: 1.0, stencil: 0,
            targets: [Some(back_buffer()), None, None, None], depth: None,
            viewport: [0, 0, 4, 4, 0, 1f32.to_bits()], scissor: None, srgb: false });
        let mut vb = triangle();
        if frame % 2 == 1 { vb[..4].copy_from_slice(&0.0f32.to_le_bytes()); }
        // Same-size update, replacement, then same-size update before first use.
        for bytes in [vec![0; 17], vb.clone(), vb] {
            gpu.upload(Upload::Buffer { resource: 0x300, bytes });
        }
        gpu.upload(Upload::Buffer { resource: 0x301, bytes: [0u16, 1, 2].into_iter().flat_map(u16::to_le_bytes).collect() });
        for step in 0..14u32 {
            let mut constants = Constants { f: vec![[0; 4]; 224], ..Default::default() };
            constants.f[0] = [(frame as f32 / 12.).to_bits(), (step as f32 / 14.).to_bits(), 0.5f32.to_bits(), 1f32.to_bits()];
            let mut d = draw(ps_constant(), constants, vec![]);
            if step % 2 == 0 {
                d.vertices = Vertices::Streams(vec![(0, 0x300, 0, 24, 1)].into());
                d.indices = Indices::Buffer(0x301, 101);
            }
            gpu.draw(&d);
        }
        images.extend(gpu.read_back(&back_buffer()).unwrap());
        for seed in [frame as u8, 197] {
            let mut bytes = vec![0xef; 28]; // final row has no unused padding
            for y in 0..2usize { for x in 0..3usize {
                bytes[y * 16 + x * 4..y * 16 + x * 4 + 4].copy_from_slice(&[seed, x as u8 * 70, y as u8 * 190, 255]);
            }}
            gpu.upload(Upload::Image { resource: 0x200, index: 0, format: 21, width: 3, height: 2,
                depth: 1, pitch: 16, slice: 32, bytes });
            gpu.draw(&textured);
            images.extend(gpu.read_back(&back_buffer()).unwrap());
        }
    }
    gpu.wait_idle();
    assert!(gpu.errors.is_empty(), "{:?}", gpu.errors);
    images
}

fn compare(raw: bool, threads: usize) {
    let _guard = SERIAL.lock().unwrap();
    let expected = sequence(&mut backend(false, raw, threads));
    let mut candidate = backend(true, raw, threads);
    let before = ddgpu::vk::stats();
    let actual = sequence(&mut candidate);
    assert_eq!(expected, actual);
    let stats = candidate.upload_stats();
    assert!(stats.producer_bytes > 0 && stats.texture_copies > 0 && stats.buffer_copies > 0);
    assert!(stats.reused_pages > 0, "staging did not recycle: {stats:?}");
    if raw { assert!(ddgpu::vk::stats().passes > before.passes, "raw path was not exercised"); }
    eprintln!("L150 raw={raw} threads={threads}: {} bytes hash={} {stats:?}", actual.len(), blake3::hash(&actual));
}

#[test] fn synchronous_uploads_match_queue_writes() { compare(false, 1); }
#[test] fn parallel_uploads_match_queue_writes() { compare(false, 2); }
#[test] fn four_encoder_uploads_match_queue_writes() { compare(false, 4); }
#[test] fn raw_uploads_match_queue_writes() { compare(true, 2); }

#[path = "../src/upload_ring.rs"]
#[allow(dead_code)]
mod staging;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake { fn wake(self: std::sync::Arc<Self>) { self.0.unpark(); } }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop { match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => return v,
        std::task::Poll::Pending => std::thread::park(),
    }}
}

#[test]
fn submitted_pages_preserve_every_byte_before_recycling() {
    let _guard = SERIAL.lock().unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let adapter = block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    const SIZE: u64 = 65536;
    const COUNT: u64 = 48;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: SIZE,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: SIZE * COUNT,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let mut ring = staging::UploadRing::new();
    for cycle in 0..3 {
        let mut expected = vec![];
        // 48 copies in four submissions, with no CPU/GPU wait between them.
        // Each copy reads a different fill even though the destination repeats.
        for batch in 0..4 {
            let mut enc = device.create_command_encoder(&Default::default());
            for k in 0..12 {
                let index = batch * 12 + k;
                let bytes: Vec<u8> = (0..SIZE).map(|i| (i * 37 + index * 13 + cycle * 91) as u8).collect();
                ring.buffer(&device, &mut enc, &buffer, SIZE, |mut dst| dst.copy_from_slice(&bytes));
                enc.copy_buffer_to_buffer(&buffer, 0, &read, index * SIZE, SIZE);
                expected.extend_from_slice(&bytes);
            }
            let (prefix, submitted) = ring.finish(&device);
            assert!(prefix.is_none());
            queue.submit([enc.finish()]);
            submitted.recycle(&queue);
        }
        let (send, recv) = std::sync::mpsc::channel();
        read.map_async(wgpu::MapMode::Read, .., move |r| send.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        recv.recv().unwrap().unwrap();
        assert_eq!(&*read.get_mapped_range(..), expected);
        eprintln!("staging cycle {cycle} hash={}", blake3::hash(&expected));
        read.unmap();
        // Work-done schedules mapping, whose callback needs another poll.
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    }
    assert!(ring.stats.reused_pages >= 4, "{:?}", ring.stats);
    eprintln!("staging lifecycle {:?}", ring.stats);
}

#[test]
fn texture_block_rows_mips_layers_and_volume_bytes() {
    let _guard = SERIAL.lock().unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TEXTURE_COMPRESSION_BC, ..Default::default()
    })).unwrap();
    let mut ring = staging::UploadRing::new();
    let mut all = vec![];
    for (format, block, bpp, volume) in [
        (wgpu::TextureFormat::Rgba8Unorm, 1, 4, false),
        (wgpu::TextureFormat::Rgba16Float, 1, 8, false),
        (wgpu::TextureFormat::Bc1RgbaUnorm, 4, 8, false),
        (wgpu::TextureFormat::Bc3RgbaUnorm, 4, 16, false),
        (wgpu::TextureFormat::Rgba8Unorm, 1, 4, true),
    ] {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None, size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: if volume { 4 } else { 6 } },
            mip_level_count: 4, sample_count: 1,
            dimension: if volume { wgpu::TextureDimension::D3 } else { wgpu::TextureDimension::D2 },
            format, usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        for mip in 0..4 {
            let width = (8u32 >> mip).max(block);
            let depth = if volume { (4u32 >> mip).max(1) } else { 1 };
            let extent = wgpu::Extent3d { width, height: width, depth_or_array_layers: depth };
            let rows = width / block;
            let row = rows * bpp;
            let pitch = row.next_multiple_of(256);
            let expected: Vec<u8> = (0..row * rows * depth).map(|x| (x * 37 + mip * 19) as u8).collect();
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None, size: (pitch * rows * depth) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
            });
            let target = wgpu::TexelCopyTextureInfo {
                texture: &texture, mip_level: mip,
                origin: wgpu::Origin3d { x: 0, y: 0, z: if volume { 0 } else { 3 } },
                aspect: wgpu::TextureAspect::All,
            };
            for staged in [false, true] {
                let mut enc = device.create_command_encoder(&Default::default());
                if staged {
                    // Erase the oracle result so a missing staging copy cannot
                    // pass by reading the preceding identical queue upload.
                    queue.write_texture(target, &vec![0; expected.len()], wgpu::TexelCopyBufferLayout {
                        offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows),
                    }, extent);
                    ring.texture(&device, &mut enc, target, extent, row, rows, |z, y, mut dst| {
                        let at = ((z * rows + y) * row) as usize;
                        dst.copy_from_slice(&expected[at..at + row as usize]);
                    });
                } else {
                    queue.write_texture(target, &expected, wgpu::TexelCopyBufferLayout {
                        offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows),
                    }, extent);
                }
                enc.copy_texture_to_buffer(target, wgpu::TexelCopyBufferInfo {
                    buffer: &read, layout: wgpu::TexelCopyBufferLayout {
                        offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(rows),
                    },
                }, extent);
                let (_, submitted) = ring.finish(&device);
                queue.submit([enc.finish()]);
                submitted.recycle(&queue);
                read.map_async(wgpu::MapMode::Read, .., |_| {});
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                {
                    let view = read.get_mapped_range(..);
                    let actual: Vec<u8> = view.chunks(pitch as usize).flat_map(|r| r[..row as usize].iter().copied()).collect();
                    assert_eq!(actual, expected, "{format:?} mip={mip} staged={staged}");
                    if staged { all.extend(actual); }
                }
                read.unmap();
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            }
        }
    }
    assert!(ring.stats.reused_pages > 0);
    eprintln!("texture layout hash={} bytes={} {:?}", blake3::hash(&all), all.len(), ring.stats);
}
