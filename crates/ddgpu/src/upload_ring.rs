//! Mapped upload pages owned by a submission until GPU completion.
//! Mapping is legal only on COPY_SRC staging buffers, never while the GPU
//! reads them. A page is unmapped at handoff and remapped after work-done.
use std::sync::{Arc, Mutex};

pub(crate) fn enabled() -> bool {
    std::env::var("DDGPU_UPLOAD_RING").as_deref() == Ok("1")
}

struct Page { buffer: wgpu::Buffer, view: wgpu::BufferViewMut, used: u64 }
struct Copy { src: wgpu::Buffer, offset: u64, dst: wgpu::Buffer, dest: u64, size: u64 }

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub allocated_pages: u64,
    pub reused_pages: u64,
    pub producer_bytes: u64,
    pub buffer_copies: u64,
    pub texture_copies: u64,
}

pub(crate) struct UploadRing {
    active: Vec<Page>,
    free: Arc<Mutex<Vec<wgpu::Buffer>>>,
    prefix: Vec<Copy>,
    page_size: u64,
    pub stats: Stats,
}

#[derive(Default)]
pub(crate) struct Submitted {
    pages: Vec<wgpu::Buffer>,
    free: Option<Arc<Mutex<Vec<wgpu::Buffer>>>>,
}

impl Submitted {
    /// Called on the submitting thread, AFTER queue.submit, including L124.
    pub fn recycle(self, queue: &wgpu::Queue) {
        if self.pages.is_empty() { return; }
        queue.on_submitted_work_done(move || {
            for page in self.pages {
                let free = self.free.as_ref().unwrap().clone();
                let mapped = page.clone();
                page.map_async(wgpu::MapMode::Write, .., move |result| {
                    if result.is_ok() { free.lock().unwrap().push(mapped); }
                });
            }
        });
    }
}

impl UploadRing {
    pub fn new() -> Self { Self::with_page_size(1 << 20) }
    fn with_page_size(page_size: u64) -> Self {
        Self { active: vec![], free: Arc::default(), prefix: vec![], page_size, stats: Stats::default() }
    }

    fn fill(&mut self, device: &wgpu::Device, size: u64, align: u64,
            fill: impl FnOnce(wgpu::WriteOnly<'_, [u8]>)) -> (wgpu::Buffer, u64) {
        assert!(size > 0 && size.is_multiple_of(4));
        self.stats.producer_bytes += size;
        let align = align.max(wgpu::MAP_ALIGNMENT);
        let index = self.active.iter().position(|p| p.used.next_multiple_of(align) + size <= p.buffer.size());
        let index = index.unwrap_or_else(|| {
            let mut free = self.free.lock().unwrap();
            let buffer = if let Some(i) = free.iter().position(|b| b.size() >= size) {
                self.stats.reused_pages += 1;
                free.swap_remove(i)
            } else {
                self.stats.allocated_pages += 1;
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("upload ring page"), size: self.page_size.max(size),
                    usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: true,
                })
            };
            let view = buffer.get_mapped_range_mut(..);
            self.active.push(Page { buffer, view, used: 0 });
            self.active.len() - 1
        });
        let page = &mut self.active[index];
        let offset = page.used.next_multiple_of(align);
        page.used = offset + size;
        fill(page.view.slice(offset as usize..(offset + size) as usize));
        (page.buffer.clone(), offset)
    }

    /// Uniform/inline ring regions are fresh destination offsets. Their
    /// copies can precede the complete batch, including raw Vulkan chunks.
    pub fn prefix_buffer(&mut self, device: &wgpu::Device, dst: &wgpu::Buffer,
                         dest: u64, size: u64, align: u64, fill: impl FnOnce(wgpu::WriteOnly<'_, [u8]>)) {
        let (src, offset) = self.fill(device, size, align, fill);
        if let Some(last) = self.prefix.last_mut().filter(|c|
            c.src == src && c.dst == *dst && offset == c.offset + c.size
            && dest == c.dest + c.size) {
            last.size = dest - last.dest + size;
        } else {
            self.prefix.push(Copy { src, offset, dst: dst.clone(), dest, size });
        }
    }

    pub fn buffer(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder,
                  dst: &wgpu::Buffer, size: u64, fill: impl FnOnce(wgpu::WriteOnly<'_, [u8]>)) {
        let (src, offset) = self.fill(device, size, 8, fill);
        self.stats.buffer_copies += 1;
        enc.copy_buffer_to_buffer(&src, offset, dst, 0, size);
    }

    pub fn texture(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder,
                   dst: wgpu::TexelCopyTextureInfo<'_>, extent: wgpu::Extent3d,
                   row: u32, rows: u32, fill_row: impl Fn(u32, u32, wgpu::WriteOnly<'_, [u8]>)) {
        let pitch = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size = pitch as u64 * rows as u64 * extent.depth_or_array_layers as u64;
        let (src, offset) = self.fill(device, size, 256, |mut data| {
            for z in 0..extent.depth_or_array_layers {
                for y in 0..rows {
                    let at = ((z * rows + y) * pitch) as usize;
                    fill_row(z, y, data.slice(at..at + row as usize));
                }
            }
        });
        enc.copy_buffer_to_texture(wgpu::TexelCopyBufferInfo {
            buffer: &src, layout: wgpu::TexelCopyBufferLayout {
                offset, bytes_per_row: Some(pitch), rows_per_image: Some(rows),
            },
        }, dst, extent);
        self.stats.texture_copies += 1;
    }

    pub fn finish(&mut self, device: &wgpu::Device) -> (Option<wgpu::CommandEncoder>, Submitted) {
        let prefix = if self.prefix.is_empty() { None } else {
            let mut enc = device.create_command_encoder(&Default::default());
            for c in self.prefix.drain(..) {
                enc.copy_buffer_to_buffer(&c.src, c.offset, &c.dst, c.dest, c.size);
                self.stats.buffer_copies += 1;
            }
            Some(enc)
        };
        let pages = self.active.drain(..).map(|p| { drop(p.view); p.buffer.unmap(); p.buffer }).collect();
        (prefix, Submitted { pages, free: Some(self.free.clone()) })
    }
}
