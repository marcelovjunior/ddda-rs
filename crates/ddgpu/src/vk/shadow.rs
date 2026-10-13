//! L159 same-input pass oracle. Only diagnostic mode allocates shadow targets.
//! Copies load state (including depth/stencil) before either renderer; comparison
//! commands snapshot results before later passes can overwrite real attachments.
use super::pass::Pass;
use crate::native_compare::{make_pipeline, CompareAspect};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static SEEN: AtomicU64 = AtomicU64::new(0);
static SELECTED: AtomicU64 = AtomicU64::new(0);
static MATCHED: AtomicU64 = AtomicU64::new(0);
static MISMATCHED: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);
static PENDING: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);

fn interval(value: Option<&str>) -> Option<u64> {
    value.and_then(|v| v.parse().ok()).filter(|n| *n > 0)
}
pub(super) fn selected() -> Option<u64> {
    static EVERY: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    let Some(n) = *EVERY.get_or_init(|| interval(std::env::var("DDGPU_VK_SHADOW").ok().as_deref())) else {
        return None;
    };
    let id = SEEN.fetch_add(1, Relaxed) + 1;
    if (id - 1) % n != 0 {
        return None;
    }
    SELECTED.fetch_add(1, Relaxed);
    Some(id)
}
pub(super) fn skipped(reason: &str) {
    if SKIPPED.fetch_add(1, Relaxed) < 8 {
        eprintln!("ddgpu-vk-shadow skipped: {reason}");
    }
}
/// Counts are cumulative per process; matched/mismatched count target aspects.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShadowStats {
    pub selected: u64,
    pub matched_aspects: u64,
    pub mismatched_aspects: u64,
    pub skipped_passes: u64,
    pub pending_aspects: u64,
    pub errors: u64,
}
pub fn shadow_stats() -> ShadowStats {
    ShadowStats {
        selected: SELECTED.load(Relaxed),
        matched_aspects: MATCHED.load(Relaxed),
        mismatched_aspects: MISMATCHED.load(Relaxed),
        skipped_passes: SKIPPED.load(Relaxed),
        pending_aspects: PENDING.load(Relaxed),
        errors: ERRORS.load(Relaxed),
    }
}
pub(super) fn report() {
    if SELECTED.load(Relaxed) != 0 {
        eprintln!("ddgpu-vk-shadow selected={} matched_aspects={} mismatched_aspects={} skipped_passes={} pending_aspects={} errors={}", SELECTED.load(Relaxed), MATCHED.load(Relaxed), MISMATCHED.load(Relaxed), SKIPPED.load(Relaxed), PENDING.load(Relaxed), ERRORS.load(Relaxed));
    }
}

pub(super) struct Check {
    read: wgpu::Buffer,
    pixels: u32,
    id: u64,
    slot: usize,
    aspect: CompareAspect,
}
impl Drop for Check {
    fn drop(&mut self) {
        PENDING.fetch_sub(1, Relaxed);
    }
}
impl Check {
    pub fn map(self) {
        let read = self.read.clone();
        read.slice(..).map_async(wgpu::MapMode::Read, move |status| {
            if status.is_err() {
                ERRORS.fetch_add(1, Relaxed);
                eprintln!("ddgpu-vk-shadow map_error pass={} slot={} aspect={:?}", self.id, self.slot, self.aspect);
                return;
            }
            let bytes = self.read.slice(..).get_mapped_range();
            let word = |n| u32::from_le_bytes(bytes[n..n+4].try_into().unwrap());
            let different = word(0);
            let pixels = word(12);
            let valid = pixels == self.pixels;
            if !valid { ERRORS.fetch_add(1, Relaxed); }
            else if different == 0 { MATCHED.fetch_add(1, Relaxed); }
            else { MISMATCHED.fetch_add(1, Relaxed); }
            // Per-aspect receipts include hashes of the 16-byte reduction, not
            // frame hashes. Equality always uses differing-pixel counts.
            eprintln!("ddgpu-vk-shadow pass={} slot={} aspect={:?} pixels={}/{} differing_pixels={} max_diff={} nonfinite={} summary_hash={}", self.id, self.slot, self.aspect, pixels, self.pixels, different, f32::from_bits(word(4)), word(8), blake3::hash(&bytes));
            drop(bytes);
            self.read.unmap();
        });
    }
}

pub(super) fn prepare(
    device: &wgpu::Device,
    pass: &Pass,
    id: u64,
) -> Result<(Vec<wgpu::CommandBuffer>, wgpu::CommandBuffer, Vec<Check>), String> {
    // Each encoding worker can add at most six aspects after this check.
    if PENDING.load(Relaxed) >= 48 {
        return Err("readback capacity".into());
    }
    let mut seed = device.create_command_encoder(&Default::default());
    let mut compare = device.create_command_encoder(&Default::default());
    let mut colors = vec![None; pass.colors.len()];
    let mut depth = None;
    let mut checks = vec![];
    let attachments = pass
        .colors
        .iter()
        .zip(&pass.native_colors)
        .enumerate()
        .filter_map(|(slot, (v, n))| Some((slot, v.as_ref()?, n.as_ref()?)))
        .chain(
            pass.depth
                .iter()
                .zip(&pass.native_depth)
                .map(|((v, _), n)| (pass.colors.len(), v, n)),
        );
    for (slot, source, native) in attachments {
        let tx = source.texture();
        let desc = &native.desc;
        let required = wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING;
        if !tx.usage().contains(required)
            || tx.dimension() != wgpu::TextureDimension::D2
            || desc
                .dimension
                .is_some_and(|d| d != wgpu::TextureViewDimension::D2)
            || desc.aspect != wgpu::TextureAspect::All
            || desc
                .mip_level_count
                .unwrap_or(tx.mip_level_count() - desc.base_mip_level)
                != 1
            || desc
                .array_layer_count
                .unwrap_or(tx.depth_or_array_layers() - desc.base_array_layer)
                != 1
        {
            return Err("attachment usage/subresource unsupported".into());
        }
        let width = (tx.width() >> desc.base_mip_level).max(1);
        let height = (tx.height() >> desc.base_mip_level).max(1);
        let limit = device.limits().max_compute_workgroups_per_dimension;
        if width.checked_mul(height).is_none() || width.div_ceil(8) > limit || height.div_ceil(8) > limit {
            return Err("comparison extent unsupported".into());
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let format = tx.format();
        let view_format = desc.format.unwrap_or(format);
        let shadow = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Vulkan oracle attachment"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[view_format],
        });
        seed.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tx,
                mip_level: desc.base_mip_level,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: desc.base_array_layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            shadow.as_image_copy(),
            size,
        );
        let view = shadow.create_view(&wgpu::TextureViewDescriptor {
            format: Some(view_format),
            ..Default::default()
        });
        if slot < colors.len() {
            colors[slot] = Some(view);
        } else {
            depth = Some((view, view_format));
        }
        let aspects = if format.is_depth_stencil_format() {
            vec![
                (
                    CompareAspect::Depth,
                    wgpu::TextureAspect::DepthOnly,
                    format.has_depth_aspect(),
                ),
                (
                    CompareAspect::Stencil,
                    wgpu::TextureAspect::StencilOnly,
                    format.has_stencil_aspect(),
                ),
            ]
        } else {
            vec![(CompareAspect::Color, wgpu::TextureAspect::All, true)]
        };
        for (aspect, texture_aspect, enabled) in aspects {
            if !enabled {
                continue;
            }
            let (pipeline, layout) = make_pipeline(device, aspect);
            let real = tx.create_view(&wgpu::TextureViewDescriptor {
                format: Some(
                    format
                        .aspect_specific_format(texture_aspect)
                        .unwrap_or(format)
                        .remove_srgb_suffix(),
                ),
                dimension: Some(wgpu::TextureViewDimension::D2),
                aspect: texture_aspect,
                base_mip_level: desc.base_mip_level,
                mip_level_count: Some(1),
                base_array_layer: desc.base_array_layer,
                array_layer_count: Some(1),
                ..Default::default()
            });
            let oracle = shadow.create_view(&wgpu::TextureViewDescriptor {
                format: Some(
                    format
                        .aspect_specific_format(texture_aspect)
                        .unwrap_or(format)
                        .remove_srgb_suffix(),
                ),
                aspect: texture_aspect,
                ..Default::default()
            });
            let result = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&oracle),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&real),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: result.as_entire_binding(),
                    },
                ],
            });
            {
                let mut p = compare.begin_compute_pass(&Default::default());
                p.set_pipeline(&pipeline);
                p.set_bind_group(0, &group, &[]);
                p.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
            }
            compare.copy_buffer_to_buffer(&result, 0, &read, 0, 16);
            PENDING.fetch_add(1, Relaxed);
            checks.push(Check {
                read,
                pixels: width * height,
                id,
                slot,
                aspect,
            });
        }
    }
    if checks.is_empty() {
        return Err("no comparable attachment".into());
    }
    Ok((
        vec![seed.finish(), pass.wgpu_targets(device, &colors, depth.as_ref())],
        compare.finish(),
        checks,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "GPU: run isolated with a 60 second timeout"]
    fn readback_rejects_mismatch_and_incomplete_counts() {
        let gpu = crate::WgpuBackend::headless().expect("Vulkan device");
        let before = super::shadow_stats();
        for words in [[1u32, 0.5f32.to_bits(), 0, 16], [0, 0, 0, 15], [0, 0, 0, 16]] {
            let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue.write_buffer(&read, 0, &words.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>());
            gpu.queue.submit([]);
            super::PENDING.fetch_add(1, super::Relaxed);
            super::Check {
                read,
                pixels: 16,
                id: 1,
                slot: 0,
                aspect: super::CompareAspect::Color,
            }
            .map();
        }
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let after = super::shadow_stats();
        assert_eq!(after.mismatched_aspects, before.mismatched_aspects + 1);
        assert_eq!(after.errors, before.errors + 1);
        assert_eq!(after.matched_aspects, before.matched_aspects + 1);
        assert_eq!(after.pending_aspects, before.pending_aspects);
    }

    #[test]
    fn sampling_is_explicit_positive_interval() {
        for v in [None, Some(""), Some("0"), Some("-1"), Some("bad")] {
            assert_eq!(super::interval(v), None);
        }
        assert_eq!(super::interval(Some("1")), Some(1));
        assert_eq!(super::interval(Some("997")), Some(997));
    }
}
