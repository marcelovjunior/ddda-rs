//! L110 backend floor: precreated objects, 5,000 indexed draws, 40 pipelines,
//! 200 descriptor groups, a distinct dynamic uniform offset on every draw.
//! Run optimized with WGPU_BACKEND=vulkan. CPU timing includes pass end,
//! encoder finish, and queue submit; GPU completion waits are outside timing.
//! This is a tiny-triangle backend floor, NOT a gameplay/FPS prediction.
//! `native_draw_cost [frames=20] [draws=5000] [mode=all|core|bundles|ash] [passes=1]`
use std::{future::Future, sync::Arc, task::{Context, Poll, Wake, Waker}, time::Instant};
use wgpu::util::DeviceExt;
const PIPELINES: usize = 40;
const GROUPS: usize = 200;
const STRIDE: u64 = 256;
const SIZE: u32 = 16;
fn block_on<F: Future>(f: F) -> F::Output {
    struct Thread(std::thread::Thread);
    impl Wake for Thread { fn wake(self: Arc<Self>) { self.0.unpark(); } }
    let waker = Waker::from(Arc::new(Thread(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop { match f.as_mut().poll(&mut cx) { Poll::Ready(v) => return v, Poll::Pending => std::thread::park() } }
}
fn shader(n: usize) -> String {
    format!(r#"
struct C {{ colour: vec4<f32> }};
@group(0) @binding(0) var<uniform> c: C;
@vertex fn vs_main(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> {{ return vec4<f32>(p, 0.0, 1.0); }}
@fragment fn fs_main() -> @location(0) vec4<f32> {{ return c.colour * {}; }}
"#, 1.0 + n as f32 * 0.00001)
}
fn vertices() -> Vec<u8> {
    [[-1.0f32, -1.0], [-1.0, -0.875], [-0.875, -1.0]].into_iter().flatten().flat_map(f32::to_le_bytes).collect()
}
fn constants(draws: usize) -> Vec<u8> {
    let mut b = vec![0; (draws + GROUPS) * STRIDE as usize];
    for (i, chunk) in b.chunks_exact_mut(STRIDE as usize).enumerate() {
        for (dst, f) in chunk[..16].chunks_exact_mut(4).zip([i as f32 / (draws + GROUPS) as f32, 0.2, 0.5, 1.0]) { dst.copy_from_slice(&f.to_le_bytes()); }
    }
    b
}
fn report(name: &str, samples: &mut [[f64; 3]], draws: usize, passes: usize) {
    samples.sort_by(|a, b| a.iter().sum::<f64>().total_cmp(&b.iter().sum::<f64>()));
    let m = samples[samples.len() / 2];
    let scale = 1e6 / draws as f64;
    println!("{name}: {:.3} us/draw total (record {:.3}, finish {:.3}, submit {:.3}); {} frames, {} draws/frame, {} pipelines, {} groups",
        m.iter().sum::<f64>() * scale, m[0] * scale, m[1] * scale, m[2] * scale, samples.len(), draws, PIPELINES, GROUPS);
    println!("  {passes} passes/frame, {:.3} us/pass total CPU (includes draws)", m.iter().sum::<f64>() * 1e6 / passes as f64);
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let frames: usize = args.first().and_then(|v| v.parse().ok()).unwrap_or(20);
    let draws: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(5000);
    let mode = args.get(2).map(String::as_str).unwrap_or("all");
    let passes: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1);
    assert!(frames > 0 && draws > 0 && passes > 0 && passes <= draws);
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = block_on(instance.request_adapter(&Default::default())).expect("Vulkan adapter");
    println!("adapter {:?}; build {}; tiny triangle 16x16; uniform data prefilled, update copies excluded", adapter.get_info(), if cfg!(debug_assertions) { "debug (NOT acceptance)" } else { "optimized" });
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    if mode == "all" || mode == "ash" { vulkan::run(&device, frames, draws, passes); }
    if mode == "ash" { return; }
    let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &constants(draws), usage: wgpu::BufferUsages::UNIFORM });
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &vertices(), usage: wgpu::BufferUsages::VERTEX });
    let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: &[0, 0, 1, 0, 2, 0], usage: wgpu::BufferUsages::INDEX });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &[wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: std::num::NonZeroU64::new(16) }, count: None }] });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
    let groups: Vec<_> = (0..GROUPS).map(|i| device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &bgl, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &ub, offset: i as u64 * STRIDE, size: std::num::NonZeroU64::new(16) }) }] })).collect();
    let pipelines: Vec<_> = (0..PIPELINES).map(|i| {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(shader(i).into()) });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: None, layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[wgpu::VertexBufferLayout { array_stride: 8, step_mode: wgpu::VertexStepMode::Vertex, attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 }] }] },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview_mask: None, cache: None })
    }).collect();
    let target = device.create_texture(&wgpu::TextureDescriptor { label: None, size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let view = target.create_view(&Default::default());
    let started = Instant::now();
    let mut bundles_by_pass = vec![];
    for part in 0..passes {
        let mut bundle = device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor { label: None, color_formats: &[Some(wgpu::TextureFormat::Rgba8Unorm)], depth_stencil: None, sample_count: 1, multiview: None });
        for k in part * draws / passes..(part + 1) * draws / passes {
        bundle.set_pipeline(&pipelines[k % PIPELINES]);
        bundle.set_bind_group(0, &groups[k % GROUPS], &[(k as u64 * STRIDE) as u32]);
        bundle.set_vertex_buffer(0, vb.slice(..)); bundle.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
        bundle.draw_indexed(0..3, 0, 0..1);
        }
        bundles_by_pass.push(bundle.finish(&Default::default()));
    }
    println!("bundle construction {:.3} us/draw (one time)", started.elapsed().as_secs_f64() * 1e6 / draws as f64);
    for bundles in [false, true] {
        if (mode == "core" && bundles) || (mode == "bundles" && !bundles) { continue; }
        let mut samples = vec![];
        for frame in 0..frames + 3 {
            let start = Instant::now();
            let mut encoder = device.create_command_encoder(&Default::default());
            for part in 0..passes {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
                if bundles { pass.execute_bundles([&bundles_by_pass[part]]); } else {
                    for k in part * draws / passes..(part + 1) * draws / passes {
                        pass.set_pipeline(&pipelines[k % PIPELINES]);
                        pass.set_bind_group(0, &groups[k % GROUPS], &[(k as u64 * STRIDE) as u32]);
                        pass.set_vertex_buffer(0, vb.slice(..)); pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
                        pass.draw_indexed(0..3, 0, 0..1);
                    }
                }
            }
            let record = start.elapsed().as_secs_f64();
            let start = Instant::now(); let cmd = encoder.finish(); let finish = start.elapsed().as_secs_f64();
            let start = Instant::now(); let index = queue.submit([cmd]); let submit = start.elapsed().as_secs_f64();
            device.poll(wgpu::PollType::Wait { submission_index: Some(index), timeout: Some(std::time::Duration::from_secs(10)) }).unwrap();
            if frame >= 3 { samples.push([record, finish, submit]); }
        }
        report(if bundles { "wgpu reused bundles" } else { "wgpu direct" }, &mut samples, draws, passes);
    }
}

mod vulkan {
    use super::*;
    use ash::vk;
    fn spirv(n: usize, stage: naga::ShaderStage, entry: &str) -> Vec<u32> {
        let module = naga::front::wgsl::parse_str(&shader(n)).unwrap();
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap();
        naga::back::spv::write_vec(&module, &info, &Default::default(), Some(&naga::back::spv::PipelineOptions { shader_stage: stage, entry_point: entry.into() })).unwrap()
    }
    pub fn run(device: &wgpu::Device, frames: usize, draws: usize, passes: usize) {
        // SAFETY: all raw resources are exclusively owned by this benchmark;
        // no wgpu work is in flight or submitted concurrently on this queue.
        // Every frame waits its fence before resetting its pool or destroying
        // resources. The hal guard and wgpu Device outlive every raw handle.
        unsafe { run_raw(device, frames, draws, passes); }
    }
    unsafe fn run_raw(device: &wgpu::Device, frames: usize, draws: usize, passes: usize) {
        let hal = device.as_hal::<wgpu::hal::api::Vulkan>().expect("Vulkan device");
        let raw = hal.raw_device();
        let memory = hal.shared_instance().raw_instance().get_physical_device_memory_properties(hal.raw_physical_device());
        let memory_type = |bits: u32, flags: vk::MemoryPropertyFlags| (0..memory.memory_type_count).find(|&i| bits & (1 << i) != 0 && memory.memory_types[i as usize].property_flags.contains(flags)).expect("memory type");
        let buffer = |bytes: &[u8], usage: vk::BufferUsageFlags| {
            let b = raw.create_buffer(&vk::BufferCreateInfo::default().size(bytes.len() as u64).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE), None).unwrap();
            let req = raw.get_buffer_memory_requirements(b);
            let mem = raw.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)), None).unwrap();
            raw.bind_buffer_memory(b, mem, 0).unwrap();
            let ptr = raw.map_memory(mem, 0, bytes.len() as u64, vk::MemoryMapFlags::empty()).unwrap();
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            raw.unmap_memory(mem);
            (b, mem)
        };
        let (ub, um) = buffer(&constants(draws), vk::BufferUsageFlags::UNIFORM_BUFFER);
        let (vb, vm) = buffer(&vertices(), vk::BufferUsageFlags::VERTEX_BUFFER);
        let (ib, im) = buffer(&[0, 0, 1, 0, 2, 0], vk::BufferUsageFlags::INDEX_BUFFER);
        let image = raw.create_image(&vk::ImageCreateInfo::default().image_type(vk::ImageType::TYPE_2D).format(vk::Format::R8G8B8A8_UNORM).extent(vk::Extent3D { width: SIZE, height: SIZE, depth: 1 }).mip_levels(1).array_layers(1).samples(vk::SampleCountFlags::TYPE_1).tiling(vk::ImageTiling::OPTIMAL).usage(vk::ImageUsageFlags::COLOR_ATTACHMENT).sharing_mode(vk::SharingMode::EXCLUSIVE), None).unwrap();
        let req = raw.get_image_memory_requirements(image);
        let image_mem = raw.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)), None).unwrap();
        raw.bind_image_memory(image, image_mem, 0).unwrap();
        let view = raw.create_image_view(&vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D).format(vk::Format::R8G8B8A8_UNORM).subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 }), None).unwrap();
        let attachments = [vk::AttachmentDescription::default().format(vk::Format::R8G8B8A8_UNORM).samples(vk::SampleCountFlags::TYPE_1).load_op(vk::AttachmentLoadOp::CLEAR).store_op(vk::AttachmentStoreOp::STORE).initial_layout(vk::ImageLayout::UNDEFINED).final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let colour_ref = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
        let subpasses = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&colour_ref)];
        let dependencies = [vk::SubpassDependency::default().src_subpass(vk::SUBPASS_EXTERNAL).dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT).dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE).dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dependency_flags(vk::DependencyFlags::BY_REGION)];
        let render_pass = raw.create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&attachments).subpasses(&subpasses).dependencies(&dependencies), None).unwrap();
        let views = [view];
        let framebuffer = raw.create_framebuffer(&vk::FramebufferCreateInfo::default().render_pass(render_pass).attachments(&views).width(SIZE).height(SIZE).layers(1), None).unwrap();
        let bindings = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let set_layout = raw.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None).unwrap();
        let set_layouts = [set_layout];
        let layout = raw.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts), None).unwrap();
        let sizes = [vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: GROUPS as u32 }];
        let descriptors = raw.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(GROUPS as u32).pool_sizes(&sizes), None).unwrap();
        let group_layouts = vec![set_layout; GROUPS];
        let groups = raw.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(descriptors).set_layouts(&group_layouts)).unwrap();
        for (n, &set) in groups.iter().enumerate() {
            let info = [vk::DescriptorBufferInfo { buffer: ub, offset: n as u64 * STRIDE, range: 16 }];
            raw.update_descriptor_sets(&[vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC).buffer_info(&info)], &[]);
        }
        let mut pipelines = vec![];
        for n in 0..PIPELINES {
            let vs = raw.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv(n, naga::ShaderStage::Vertex, "vs_main")), None).unwrap();
            let ps = raw.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv(n, naga::ShaderStage::Fragment, "fs_main")), None).unwrap();
            let stages = [vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vs).name(c"vs_main"), vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(ps).name(c"fs_main")];
            let binding = [vk::VertexInputBindingDescription { binding: 0, stride: 8, input_rate: vk::VertexInputRate::VERTEX }];
            let attribute = [vk::VertexInputAttributeDescription { location: 0, binding: 0, format: vk::Format::R32G32_SFLOAT, offset: 0 }];
            let input = vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&binding).vertex_attribute_descriptions(&attribute);
            let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
            let viewport = [vk::Viewport { x: 0.0, y: 0.0, width: SIZE as f32, height: SIZE as f32, min_depth: 0.0, max_depth: 1.0 }];
            let scissor = [vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: SIZE, height: SIZE } }];
            let viewport_state = vk::PipelineViewportStateCreateInfo::default().viewports(&viewport).scissors(&scissor);
            let raster = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).front_face(vk::FrontFace::COUNTER_CLOCKWISE).line_width(1.0);
            let multisample = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let blend_attachment = [vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::RGBA)];
            let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
            let info = vk::GraphicsPipelineCreateInfo::default().stages(&stages).vertex_input_state(&input).input_assembly_state(&assembly).viewport_state(&viewport_state).rasterization_state(&raster).multisample_state(&multisample).color_blend_state(&blend).layout(layout).render_pass(render_pass).subpass(0);
            pipelines.push(raw.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None).unwrap()[0]);
            raw.destroy_shader_module(vs, None); raw.destroy_shader_module(ps, None);
        }
        let pool = raw.create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(hal.queue_family_index()).flags(vk::CommandPoolCreateFlags::TRANSIENT), None).unwrap();
        let cmd = raw.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1)).unwrap()[0];
        let fence = raw.create_fence(&vk::FenceCreateInfo::default(), None).unwrap();
        let mut samples = vec![];
        for frame in 0..frames + 3 {
            let start = Instant::now();
            raw.reset_command_pool(pool, vk::CommandPoolResetFlags::empty()).unwrap();
            raw.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)).unwrap();
            let clear = [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } }];
            for part in 0..passes {
                raw.cmd_begin_render_pass(cmd, &vk::RenderPassBeginInfo::default().render_pass(render_pass).framebuffer(framebuffer).render_area(vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: SIZE, height: SIZE } }).clear_values(&clear), vk::SubpassContents::INLINE);
                for k in part * draws / passes..(part + 1) * draws / passes {
                raw.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipelines[k % PIPELINES]);
                raw.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, layout, 0, &[groups[k % GROUPS]], &[(k as u64 * STRIDE) as u32]);
                raw.cmd_bind_vertex_buffers(cmd, 0, &[vb], &[0]); raw.cmd_bind_index_buffer(cmd, ib, 0, vk::IndexType::UINT16);
                raw.cmd_draw_indexed(cmd, 3, 1, 0, 0, 0);
                }
                raw.cmd_end_render_pass(cmd);
            }
            let record = start.elapsed().as_secs_f64();
            let start = Instant::now(); raw.end_command_buffer(cmd).unwrap(); let finish = start.elapsed().as_secs_f64();
            let start = Instant::now();
            raw.reset_fences(&[fence]).unwrap();
            raw.queue_submit(hal.raw_queue(), &[vk::SubmitInfo::default().command_buffers(&[cmd])], fence).unwrap();
            let submit = start.elapsed().as_secs_f64();
            raw.wait_for_fences(&[fence], true, 10_000_000_000).unwrap();
            if frame >= 3 { samples.push([record, finish, submit]); }
        }
        report("ash Vulkan direct", &mut samples, draws, passes);
        raw.destroy_fence(fence, None); raw.destroy_command_pool(pool, None);
        for p in pipelines { raw.destroy_pipeline(p, None); }
        raw.destroy_pipeline_layout(layout, None); raw.destroy_descriptor_pool(descriptors, None); raw.destroy_descriptor_set_layout(set_layout, None);
        raw.destroy_framebuffer(framebuffer, None); raw.destroy_render_pass(render_pass, None); raw.destroy_image_view(view, None); raw.destroy_image(image, None); raw.free_memory(image_mem, None);
        for (b, m) in [(ub, um), (vb, vm), (ib, im)] { raw.destroy_buffer(b, None); raw.free_memory(m, None); }
    }
}
