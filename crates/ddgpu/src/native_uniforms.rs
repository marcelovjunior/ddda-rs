//! Host-side constant snapshot storage. Scatter is an upload operation, not
//! a translation of game logic. Every recorded snapshot has its own offset.
use super::*;

pub(super) struct Uniforms {
    pub buffer: wgpu::Buffer,
    cpu: Vec<u8>,
    at: usize,
    pub generation: u64,
    resident: Option<Resident>,
}
struct Resident {
    previous: Vec<u8>,
    updates: Vec<u8>,
    source: wgpu::Buffer,
    group: wgpu::BindGroup,
    pipeline: wgpu::ComputePipeline,
}
impl Uniforms {
    pub fn new(device: &wgpu::Device, size: u64, resident: bool) -> Self {
        let usage = wgpu::BufferUsages::UNIFORM
            | wgpu::BufferUsages::COPY_DST
            | if resident {
                wgpu::BufferUsages::STORAGE
            } else {
                wgpu::BufferUsages::empty()
            };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native constant snapshots"),
            size,
            usage,
            mapped_at_creation: false,
        });
        let resident = resident.then(|| {
            assert!(
                size * 5 / 4 + 16 <= device.limits().max_storage_buffer_binding_size as u64,
                "resident constant arena exceeds storage binding limit"
            );
            let source = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native constant deltas"),
                size: size * 5 / 4 + 16,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("native constant scatter"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                    r#"
@group(0) @binding(0) var<storage, read> updates: array<u32>;
@group(0) @binding(1) var<storage, read_write> snapshots: array<vec4<u32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x + id.y * updates[1];
    if index >= updates[0] { return; }
    let p = 4u + index * 5u;
    snapshots[updates[p]] = vec4<u32>(updates[p+1u], updates[p+2u], updates[p+3u], updates[p+4u]);
}
"#,
                )),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("native constant scatter"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native constant scatter"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: source.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buffer.as_entire_binding(),
                    },
                ],
            });
            Resident {
                previous: vec![0; size as usize],
                updates: Vec::new(),
                source,
                group,
                pipeline,
            }
        });
        Self {
            buffer,
            cpu: vec![0; size as usize],
            at: 0,
            generation: 0,
            resident,
        }
    }
    pub fn capacity(&self) -> u64 {
        self.cpu.len() as u64
    }
    pub fn fits(&self, len: u64, align: u64) -> bool {
        (self.at as u64).next_multiple_of(align) + len <= self.cpu.len() as u64
    }
    pub fn alloc(&mut self, len: u64, align: u64) -> Option<(u64, &mut [u8])> {
        if !self.fits(len, align) {
            return None;
        }
        let off = (self.at as u64).next_multiple_of(align) as usize;
        self.at = off + len as usize;
        Some((off as u64, &mut self.cpu[off..self.at]))
    }
    /// One transfer plus one scatter dispatch, irrespective of dirty span count.
    /// Queue order protects the previous submission even without a CPU wait.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        stats: &mut RecorderStats,
    ) -> Option<wgpu::CommandBuffer> {
        if self.at == 0 {
            return None;
        }
        let Some(r) = &mut self.resident else {
            queue.write_buffer(&self.buffer, 0, &self.cpu[..self.at]);
            stats.upload_bytes += self.at as u64;
            return None;
        };
        pack_deltas(&self.cpu[..self.at], &mut r.previous[..self.at], &mut r.updates);
        let count = (r.updates.len() - 16) / 20;
        stats.dirty_registers += count as u64;
        if count == 0 {
            return None;
        }
        stats.upload_bytes += r.updates.len() as u64;
        let groups = (count as u32).div_ceil(64);
        let width = groups.min(device.limits().max_compute_workgroups_per_dimension);
        r.updates[4..8].copy_from_slice(&(width * 64).to_le_bytes());
        queue.write_buffer(&r.source, 0, &r.updates);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&r.pipeline);
            pass.set_bind_group(0, &r.group, &[]);
            pass.dispatch_workgroups(width, groups.div_ceil(width), 1);
        }
        Some(encoder.finish())
    }
    pub fn restart(&mut self) {
        self.at = 0;
        self.generation += 1;
    }
}
fn pack_deltas(current: &[u8], previous: &mut [u8], updates: &mut Vec<u8>) {
    assert_eq!(current.len() % 16, 0);
    updates.clear();
    updates.resize(16, 0);
    // Repeated jobs can preserve the entire snapshot sequence. One bulk
    // equality check avoids scanning each register in that common exact case.
    // Otherwise retain the scalar path, including ordered dirty records.
    if previous.starts_with(current) { return; }
    for (index, (now, old)) in current.chunks_exact(16).zip(previous.chunks_exact_mut(16)).enumerate() {
        if now != old {
            updates.extend_from_slice(&(index as u32).to_le_bytes());
            updates.extend_from_slice(now);
            old.copy_from_slice(now);
        }
    }
    let count = (updates.len() - 16) / 20;
    updates[..4].copy_from_slice(&(count as u32).to_le_bytes());
}
#[cfg(test)]
mod tests {
    use super::pack_deltas;
    #[test]
    fn resident_deltas_preserve_bits_and_skip_unchanged_registers() {
        let mut old = vec![0; 64];
        let mut now = old.clone();
        now[16..20].copy_from_slice(&0x7fc01234u32.to_le_bytes());
        now[63] = 128;
        let mut updates = vec![];
        pack_deltas(&now, &mut old, &mut updates);
        assert_eq!(updates.len(), 56);
        assert_eq!(&updates[16..20], &1u32.to_le_bytes());
        assert_eq!(&updates[36..40], &3u32.to_le_bytes());
        assert_eq!(old, now);
        pack_deltas(&now, &mut old, &mut updates);
        assert_eq!(updates, [0; 16]);
        // A shortened submission must preserve unreferenced resident slots.
        now[0] = 7;
        pack_deltas(&now[..16], &mut old[..16], &mut updates);
        assert_eq!(old, now);
    }

    #[test]
    fn bulk_equal_deltas_match_scalar_records_across_snapshot_lengths() {
        for registers in [0, 1, 15, 16, 17, 63, 257] {
            let mut old = vec![0u8; registers * 16];
            let mut now = old.clone();
            for generation in 0..5 {
                for (i, byte) in now.iter_mut().enumerate() {
                    if (i + generation * 13) % 37 == 0 { *byte = (i + generation) as u8; }
                }
                let mut expected = vec![0; 16];
                for (i,(a,b)) in now.chunks_exact(16).zip(old.chunks_exact(16)).enumerate() {
                    if a != b { expected.extend_from_slice(&(i as u32).to_le_bytes()); expected.extend_from_slice(a); }
                }
                let count = (expected.len()-16)/20;
                expected[..4].copy_from_slice(&(count as u32).to_le_bytes());
                let mut actual = vec![];
                pack_deltas(&now,&mut old,&mut actual);
                assert_eq!(actual,expected);
                assert_eq!(old,now);
            }
        }
    }

    #[test]
    #[ignore = "CPU dirty-scan timing; run optimized on a quiet machine"]
    fn resident_dirty_scan_cost() {
        fn scalar(current: &[u8], previous: &mut [u8], updates: &mut Vec<u8>) {
            updates.clear(); updates.resize(16,0);
            for (index,(now,old)) in current.chunks_exact(16).zip(previous.chunks_exact_mut(16)).enumerate() {
                if now != old {
                    updates.extend_from_slice(&(index as u32).to_le_bytes());
                    updates.extend_from_slice(now); old.copy_from_slice(now);
                }
            }
            let count=(updates.len()-16)/20;
            updates[..4].copy_from_slice(&(count as u32).to_le_bytes());
        }
        let size=256*1024;
        for stride in [0,4096,16] {
            let a=vec![0;size]; let mut b=a.clone();
            if stride != 0 { for i in (0..size).step_by(stride) { b[i]=1; } }
            let mut expected=vec![]; let mut actual=vec![];
            scalar(&b,&mut a.clone(),&mut expected);
            pack_deltas(&b,&mut a.clone(),&mut actual);
            assert_eq!(actual,expected);
            for round in 0..3 {
                for fast in if round%2 == 0 { [false,true] } else { [true,false] } {
                    let mut old=a.clone(); let mut updates=Vec::with_capacity(size*5/4+16);
                    let start=std::time::Instant::now();
                    for i in 0..2000 {
                        let current=std::hint::black_box(if i%2 == 0 { &b } else { &a });
                        if fast { pack_deltas(current,&mut old,&mut updates); }
                        else { scalar(current,&mut old,&mut updates); }
                        std::hint::black_box(&updates);
                    }
                    eprintln!("L152 dirty_scan stride={stride} round={round} bulk_equal={fast} ns_per_scan={:.1}",start.elapsed().as_nanos() as f64/2000.0);
                }
            }
        }
    }
}
