//! L127: asynchronous native/oracle target comparison on the backend queue.
//!
//! The caller seeds shadow attachments BEFORE the oracle pass, executes the
//! oracle against real attachments and the native job against shadows, then
//! enqueues each pair here. Inputs must be single-sample D2 subresource views
//! with identical sizes/formats and TEXTURE_BINDING usage. Use linear views
//! for sRGB storage so comparisons describe stored channel values.
//! Only 16 bytes per target cross back to the CPU. No GPU wait is performed.
//! Like the backend's query/read_back helpers, readback uses COPY_DST/MAP_READ
//! buffers and map_async; unlike read_back, poll uses Poll, never Wait.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareAspect {
    Color,
    Depth,
    Stencil,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareTag {
    pub pass: u64,
    pub target: u32,
    pub aspect: CompareAspect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompareResult {
    pub tag: CompareTag,
    pub pixels: u32,
    pub differing_pixels: u32,
    /// Absolute channel difference, in normalized units for UNORM targets.
    /// Infinity means a differing non-finite channel or an overflowing finite
    /// difference was encountered; nonfinite_pixels distinguishes those cases.
    pub max_channel_diff: f32,
    pub nonfinite_pixels: u32,
}

impl std::fmt::Display for CompareResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native-compare pass={} target={:#x} aspect={:?} pixels={} differing_pixels={} max_channel_diff={} nonfinite_pixels={}",
            self.tag.pass, self.tag.target, self.tag.aspect, self.pixels,
            self.differing_pixels, self.max_channel_diff, self.nonfinite_pixels)
    }
}

struct Pending {
    tag: CompareTag,
    pixels: u32,
    read: wgpu::Buffer,
    mapped: Arc<AtomicU8>,
}

/// Bounded asynchronous diagnostic queue. Capacity exhaustion is an explicit
/// unverified result, never a successful comparison and never a blocking wait.
pub struct CompareQueue {
    capacity: usize,
    pipelines: HashMap<CompareAspect, (wgpu::ComputePipeline, wgpu::BindGroupLayout)>,
    pending: Vec<Pending>,
}

impl CompareQueue {
    pub fn new(_device: &wgpu::Device, capacity: usize) -> Self {
        Self {
            capacity,
            pipelines: HashMap::new(),
            pending: Vec::new(),
        }
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
    pub fn available(&self) -> usize {
        self.capacity.saturating_sub(self.pending.len())
    }

    /// The caller must submit preceding oracle/native encoders first, including
    /// draining any CPU finisher queue. Queue ordering then snapshots this pass
    /// before subsequent writes, without waiting for GPU completion.
    pub fn enqueue(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tag: CompareTag,
        oracle: &wgpu::TextureView,
        native: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if self.available() == 0 {
            return Err(format!(
                "native-compare unverified {tag:?}: pending capacity exhausted"
            ));
        }
        let limit = device.limits().max_compute_workgroups_per_dimension;
        if width == 0
            || height == 0
            || width.checked_mul(height).is_none()
            || width.div_ceil(8) > limit
            || height.div_ceil(8) > limit
        {
            return Err(format!(
                "native-compare unverified {tag:?}: unsupported extent {width}x{height}"
            ));
        }
        let (pipeline, layout) = self
            .pipelines
            .entry(tag.aspect)
            .or_insert_with(|| make_pipeline(device, tag.aspect));
        let result = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native comparison summary"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native comparison readback"),
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native comparison"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(oracle),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(native),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: result.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("native compare reduction"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(&result, 0, &read, 0, 16);
        queue.submit([encoder.finish()]);
        let mapped = Arc::new(AtomicU8::new(0));
        let done = mapped.clone();
        read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            done.store(if r.is_ok() { 1 } else { 2 }, Ordering::Release)
        });
        self.pending.push(Pending {
            tag,
            pixels: width * height,
            read,
            mapped,
        });
        Ok(())
    }

    /// Retires only completed mappings. Individual map failures are returned
    /// with their pass/target identity; they must not be reported as equality.
    pub fn poll(&mut self, device: &wgpu::Device) -> Result<Vec<Result<CompareResult, String>>, String> {
        device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| format!("native-compare poll: {e}"))?;
        let mut results = Vec::new();
        let mut i = 0;
        while i < self.pending.len() {
            let state = self.pending[i].mapped.load(Ordering::Acquire);
            if state == 0 {
                i += 1;
                continue;
            }
            let p = self.pending.remove(i);
            if state == 2 {
                results.push(Err(format!("native-compare unverified {:?}: map failed", p.tag)));
                continue;
            }
            let result = {
                let bytes = p.read.slice(..).get_mapped_range();
                decode(p.tag, &bytes)
            };
            p.read.unmap();
            if result.pixels != p.pixels {
                results.push(Err(format!(
                    "native-compare unverified {:?}: compared {} of {} expected pixels",
                    p.tag, result.pixels, p.pixels
                )));
            } else {
                results.push(Ok(result));
            }
        }
        Ok(results)
    }
}

fn decode(tag: CompareTag, bytes: &[u8]) -> CompareResult {
    let word = |n| u32::from_le_bytes(bytes[n..n + 4].try_into().unwrap());
    CompareResult {
        tag,
        differing_pixels: word(0),
        max_channel_diff: f32::from_bits(word(4)),
        nonfinite_pixels: word(8),
        pixels: word(12),
    }
}

fn make_pipeline(
    device: &wgpu::Device,
    aspect: CompareAspect,
) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    let (ty, sample_type, load) = match aspect {
        CompareAspect::Color => (
            "texture_2d<f32>",
            wgpu::TextureSampleType::Float { filterable: false },
            "textureLoad(t, p, 0)",
        ),
        CompareAspect::Depth => (
            "texture_depth_2d",
            wgpu::TextureSampleType::Depth,
            "vec4<f32>(textureLoad(t, p, 0), 0.0, 0.0, 0.0)",
        ),
        CompareAspect::Stencil => (
            "texture_2d<u32>",
            wgpu::TextureSampleType::Uint,
            "vec4<f32>(textureLoad(t, p, 0))",
        ),
    };
    let source = format!("@group(0) @binding(0) var oracle: {ty};\n@group(0) @binding(1) var native: {ty};\nfn load(t: {ty}, p: vec2<i32>) -> vec4<f32> {{ return {load}; }}\n{REDUCE}");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("native comparison"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        count: None,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("native comparison"),
        entries: &[
            texture_entry(0),
            texture_entry(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                count: None,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(16),
                },
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("native comparison"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("native comparison"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("compare"),
        compilation_options: Default::default(),
        cache: None,
    });
    (pipeline, layout)
}

// Compare the bits delivered by textureLoad (including signed zero). Float
// NaN payload preservation is not guaranteed by GPU texture loads; this is
// image-value comparison, not a raw floating-storage byte oracle.
const REDUCE: &str = r#"
struct Summary { different: atomic<u32>, maximum: atomic<u32>, nonfinite: atomic<u32>, pixels: atomic<u32> }
@group(0) @binding(2) var<storage, read_write> summary: Summary;
var<workgroup> counts: array<vec4<u32>, 64>;
@compute @workgroup_size(8, 8)
fn compare(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    var value = vec4<u32>(0u);
    if all(id.xy < textureDimensions(oracle)) {
        let a = load(oracle, vec2<i32>(id.xy));
        let b = load(native, vec2<i32>(id.xy));
        let ab = bitcast<vec4<u32>>(a);
        let bb = bitcast<vec4<u32>>(b);
        var maximum = 0.0;
        var nonfinite = 0u;
        for (var c = 0u; c < 4u; c++) {
            if ab[c] != bb[c] {
                if (ab[c] & 0x7f800000u) == 0x7f800000u || (bb[c] & 0x7f800000u) == 0x7f800000u {
                    maximum = bitcast<f32>(0x7f800000u);
                    nonfinite = 1u;
                } else { maximum = max(maximum, abs(a[c] - b[c])); }
            }
        }
        value = vec4<u32>(select(0u, 1u, any(ab != bb)), bitcast<u32>(maximum), nonfinite, 1u);
    }
    counts[lane] = value;
    workgroupBarrier();
    for (var stride = 32u; stride > 0u; stride = stride / 2u) {
        if lane < stride {
            let right = counts[lane + stride];
            counts[lane] = vec4<u32>(counts[lane].x + right.x, max(counts[lane].y, right.y), counts[lane].z + right.z, counts[lane].w + right.w);
        }
        workgroupBarrier();
    }
    if lane == 0u {
        atomicAdd(&summary.different, counts[0].x);
        atomicMax(&summary.maximum, counts[0].y);
        atomicAdd(&summary.nonfinite, counts[0].z);
        atomicAdd(&summary.pixels, counts[0].w);
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_preserves_identity_and_metrics() {
        let tag = CompareTag {
            pass: u64::MAX,
            target: 0xf0000001,
            aspect: CompareAspect::Color,
        };
        let bytes: Vec<u8> = [3, 0.25f32.to_bits(), 1, 153]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let r = decode(tag, &bytes);
        assert_eq!(
            r,
            CompareResult {
                tag,
                pixels: 153,
                differing_pixels: 3,
                max_channel_diff: 0.25,
                nonfinite_pixels: 1
            }
        );
        assert!(r.to_string().contains("differing_pixels=3 max_channel_diff=0.25"));
    }
}
