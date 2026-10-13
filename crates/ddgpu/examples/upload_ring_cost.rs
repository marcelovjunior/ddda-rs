//! Synthetic upload CPU cost; no game assets. Build release, run with a 60 s cap.
//! Reports producer cost separately from finish/submit/GPU wait. `constant`
//! compares one legacy aggregate queue write with direct mapped producer fills;
//! `dynamic` compares per-call queue writes with ordered staging copies.
#[path = "../src/upload_ring.rs"]
#[allow(dead_code)]
mod upload_ring;
use std::time::Instant;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake { fn wake(self: std::sync::Arc<Self>) { self.0.unpark(); } }
    let w = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&w);
    let mut f = std::pin::pin!(f);
    loop { match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(x) => return x, std::task::Poll::Pending => std::thread::park(),
    }}
}

fn main() {
    let ring_mode = std::env::args().any(|s| s == "--ring");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let adapter = block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
    for constant in [false, true] {
        for size in [256u64, 4096, 65536, 1048576] {
            let count = (4 * 1048576 / size).min(2048);
            let dst_size = if constant { size * count } else { size };
            let dst = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: dst_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            let data: Vec<u8> = (0..size).map(|x| (x * 37) as u8).collect();
            let mut cpu = vec![0; dst_size as usize];
            let mut ring = upload_ring::UploadRing::new();
            let mut producer = 0.0;
            let mut total = 0.0;
            const ROUNDS: u64 = 12;
            for round in 0..ROUNDS + 2 {
                let begin = Instant::now();
                let mut enc = device.create_command_encoder(&Default::default());
                for i in 0..count {
                    if ring_mode {
                        if constant {
                            ring.prefix_buffer(&device, &dst, i * size, size, 256, |mut out| out.copy_from_slice(&data));
                        } else {
                            ring.buffer(&device, &mut enc, &dst, size, |mut out| out.copy_from_slice(&data));
                        }
                    } else if constant {
                        cpu[(i * size) as usize..((i + 1) * size) as usize].copy_from_slice(&data);
                    } else {
                        queue.write_buffer(&dst, 0, &data);
                    }
                }
                if !ring_mode && constant { queue.write_buffer(&dst, 0, &cpu); }
                let staged = begin.elapsed().as_secs_f64();
                if ring_mode {
                    let (prefix, submitted) = ring.finish(&device);
                    queue.submit(prefix.into_iter().chain([enc]).map(|e| e.finish()));
                    submitted.recycle(&queue);
                } else { queue.submit([enc.finish()]); }
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                if round >= 2 { producer += staged; total += begin.elapsed().as_secs_f64(); }
            }
            let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: dst_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&dst, 0, &read, 0, dst_size);
            queue.submit([enc.finish()]);
            read.map_async(wgpu::MapMode::Read, .., |_| {});
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let view = read.get_mapped_range(..);
            for bytes in view.chunks(size as usize) { assert_eq!(bytes, data); }
            let uploads = (count * ROUNDS) as f64;
            println!("{} {} size={size} ns/upload={:.1} ns/MB={:.1} total_ns/upload={:.1} hash={} {:?}",
                if constant { "constant" } else { "dynamic" }, if ring_mode { "ring" } else { "queue" },
                producer * 1e9 / uploads, producer * 1e9 / (uploads * size as f64 / 1e6),
                total * 1e9 / uploads, blake3::hash(&view), ring.stats);
        }
    }

    for width in [64u32, 257, 512] {
        let height = 64;
        let row = width * 4;
        let size = row as u64 * height as u64;
        let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None, size: extent, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        let data: Vec<u8> = (0..size).map(|x| (x * 37) as u8).collect();
        let mut ring = upload_ring::UploadRing::new();
        let mut producer = 0.0;
        let mut total = 0.0;
        const COUNT: u64 = 32;
        const ROUNDS: u64 = 12;
        for round in 0..ROUNDS + 2 {
            let begin = Instant::now();
            let mut enc = device.create_command_encoder(&Default::default());
            for _ in 0..COUNT {
                if ring_mode {
                    ring.texture(&device, &mut enc, texture.as_image_copy(), extent, row, height, |_, y, mut dst| {
                        let at = (y * row) as usize;
                        dst.copy_from_slice(&data[at..at + row as usize]);
                    });
                } else {
                    queue.write_texture(texture.as_image_copy(), &data,
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) }, extent);
                }
            }
            let staged = begin.elapsed().as_secs_f64();
            if ring_mode {
                let (prefix, submitted) = ring.finish(&device);
                assert!(prefix.is_none());
                queue.submit([enc.finish()]);
                submitted.recycle(&queue);
            } else { queue.submit([enc.finish()]); }
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if round >= 2 { producer += staged; total += begin.elapsed().as_secs_f64(); }
        }
        let pitch = row.next_multiple_of(256);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None, size: (pitch * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(texture.as_image_copy(), wgpu::TexelCopyBufferInfo {
            buffer: &read, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(pitch), rows_per_image: Some(height) },
        }, extent);
        queue.submit([enc.finish()]);
        read.map_async(wgpu::MapMode::Read, .., |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let view = read.get_mapped_range(..);
        for y in 0..height { assert_eq!(&view[(y * pitch) as usize..(y * pitch + row) as usize], &data[(y * row) as usize..((y + 1) * row) as usize]); }
        let uploads = (COUNT * ROUNDS) as f64;
        println!("texture {} size={size} ns/upload={:.1} ns/MB={:.1} total_ns/upload={:.1} hash={} {:?}",
            if ring_mode { "ring" } else { "queue" }, producer * 1e9 / uploads,
            producer * 1e9 / (uploads * size as f64 / 1e6), total * 1e9 / uploads,
            blake3::hash(&data), ring.stats);
    }
}
