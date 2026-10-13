use super::pass::Initialized;
use crate::cache::{FxMap, ModuleKey, PipelineKey, SamplerKinds, PS_BLOCK, VS_BLOCK};
use std::{
    borrow::Cow,
    num::NonZeroU64,
    ops::Deref,
    sync::{Arc, Mutex},
};
use wgpu::hal::{self, api::Vulkan, vulkan as v, Device};

// These objects are never destroyed until the last recorded/submitted pass
// releases its Arc. The wgpu Device keeps the underlying ash device alive.
pub(crate) trait Destroy: Sized {
    unsafe fn destroy(device: &v::Device, value: Self);
}
pub(crate) struct Object<T: Destroy> {
    device: wgpu::Device,
    value: Option<T>,
}
impl<T: Destroy> Object<T> {
    fn new(device: &wgpu::Device, value: T) -> Self {
        Self {
            device: device.clone(),
            value: Some(value),
        }
    }
}
impl<T: Destroy> Deref for Object<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}
impl<T: Destroy> Drop for Object<T> {
    fn drop(&mut self) {
        // SAFETY: last Arc, including GPU completion keepalives, has gone.
        unsafe {
            if let Some(device) = self.device.as_hal::<Vulkan>() {
                T::destroy(&device, self.value.take().unwrap());
            }
        }
    }
}
macro_rules! destroy {
    ($ty:ident, $method:ident) => {
        impl Destroy for v::$ty {
            unsafe fn destroy(device: &v::Device, value: Self) {
                unsafe { device.$method(value) }
            }
        }
    };
}
destroy!(ShaderModule, destroy_shader_module);
destroy!(RenderPipeline, destroy_render_pipeline);
destroy!(PipelineLayout, destroy_pipeline_layout);
destroy!(BindGroupLayout, destroy_bind_group_layout);
destroy!(BindGroup, destroy_bind_group);
destroy!(Sampler, destroy_sampler);
destroy!(TextureView, destroy_texture_view);

pub(crate) struct View {
    pub raw: Object<v::TextureView>,
    _source: wgpu::TextureView,
}

pub(crate) struct Layout {
    pub raw: Object<v::PipelineLayout>,
    pub groups: [Arc<Object<v::BindGroupLayout>>; 3],
}
pub(crate) struct Pipeline {
    pub raw: Object<v::RenderPipeline>,
    pub layout: Arc<Layout>,
    pub uniforms: Arc<Group>,
    pub vertex_layouts: Vec<(u64, wgpu::VertexStepMode, u64)>,
    pub samples: u32,
}
pub(crate) struct Group {
    pub raw: Object<v::BindGroup>,
    pub textures: Vec<wgpu::TextureView>,
    pub buffers: Vec<wgpu::Buffer>,
    _samplers: Vec<Arc<Object<v::Sampler>>>,
    _layout: Arc<Object<v::BindGroupLayout>>,
    _views: Vec<Arc<View>>,
}

pub(crate) struct Context {
    device: wgpu::Device,
    modules: FxMap<ModuleKey, Arc<Object<v::ShaderModule>>>,
    pipelines: FxMap<PipelineKey, Arc<Pipeline>>,
    layouts: FxMap<(SamplerKinds, SamplerKinds), Arc<Layout>>,
    samplers: FxMap<wgpu::Sampler, Arc<Object<v::Sampler>>>,
    pub groups: FxMap<wgpu::BindGroup, Arc<Group>>,
    pub views: FxMap<wgpu::TextureView, Arc<View>>,
    uniform_layout: Arc<Object<v::BindGroupLayout>>,
    uniforms: Arc<Group>,
    pub initialized: Arc<Mutex<Initialized>>,
}

impl Context {
    pub fn new(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<Self, String> {
        unsafe {
            let hal = device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let entries = [(0, wgpu::ShaderStages::FRAGMENT), (2, wgpu::ShaderStages::VERTEX)].map(
                |(binding, visibility)| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                    count: None,
                },
            );
            let layout = Arc::new(Object::new(
                device,
                hal.create_bind_group_layout(&hal::BindGroupLayoutDescriptor {
                    label: None,
                    flags: hal::BindGroupLayoutFlags::empty(),
                    entries: &entries,
                })
                .map_err(|e| e.to_string())?,
            ));
            let raw_buffer = buffer.as_hal::<Vulkan>().ok_or("uniform buffer destroyed")?;
            let buffers = [PS_BLOCK, VS_BLOCK]
                .map(|size| hal::BufferBinding::new_unchecked(&*raw_buffer, 0, NonZeroU64::new(size)));
            let entries = [0, 2]
                .into_iter()
                .enumerate()
                .map(|(i, binding)| hal::BindGroupEntry {
                    binding,
                    resource_index: i as u32,
                    count: 1,
                })
                .collect::<Vec<_>>();
            let raw = hal
                .create_bind_group(&hal::BindGroupDescriptor {
                    label: None,
                    layout: &**layout,
                    buffers: &buffers,
                    samplers: &[],
                    textures: &[],
                    entries: &entries,
                    acceleration_structures: &[],
                    external_textures: &[],
                })
                .map_err(|e| e.to_string())?;
            let uniforms = Arc::new(Group {
                raw: Object::new(device, raw),
                textures: vec![],
                buffers: vec![buffer.clone()],
                _samplers: vec![],
                _layout: layout.clone(),
                _views: vec![],
            });
            Ok(Self {
                device: device.clone(),
                modules: Default::default(),
                pipelines: Default::default(),
                layouts: Default::default(),
                samplers: Default::default(),
                groups: Default::default(),
                views: Default::default(),
                uniform_layout: layout,
                uniforms,
                initialized: Default::default(),
            })
        }
    }

    pub fn shader(&mut self, key: ModuleKey, wgsl: &str) -> Result<(), String> {
        if self.modules.contains_key(&key) {
            return Ok(());
        }
        let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| e.emit_to_string(wgsl))?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .map_err(|e| e.to_string())?;
        // Source is the exact translated module already accepted by wgpu on
        // this device. hal applies its same SPIR-V binding/coordinate options.
        unsafe {
            let device = self.device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let module = device
                .create_shader_module(
                    &hal::ShaderModuleDescriptor {
                        label: None,
                        runtime_checks: wgpu::ShaderRuntimeChecks::checked(),
                    },
                    hal::ShaderInput::Naga(hal::NagaShader {
                        module: Cow::Owned(module),
                        info,
                        debug_source: None,
                    }),
                )
                .map_err(|e| e.to_string())?;
            self.modules
                .insert(key, Arc::new(Object::new(&self.device, module)));
        }
        Ok(())
    }

    fn layout(&mut self, ps: &SamplerKinds, vs: &SamplerKinds) -> Result<Arc<Layout>, String> {
        let key = (ps.clone(), vs.clone());
        if let Some(layout) = self.layouts.get(&key) {
            return Ok(layout.clone());
        }
        unsafe {
            let device = self.device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let mut groups = vec![self.uniform_layout.clone()];
            for (kinds, visibility) in [
                (ps, wgpu::ShaderStages::FRAGMENT),
                (vs, wgpu::ShaderStages::VERTEX),
            ] {
                let mut entries = vec![];
                for &(n, kind) in kinds {
                    entries.push(wgpu::BindGroupLayoutEntry {
                        binding: 2 * n,
                        visibility,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: match kind {
                                0 => wgpu::TextureViewDimension::D2,
                                1 => wgpu::TextureViewDimension::Cube,
                                _ => wgpu::TextureViewDimension::D3,
                            },
                            multisampled: false,
                        },
                        count: None,
                    });
                    entries.push(wgpu::BindGroupLayoutEntry {
                        binding: 2 * n + 1,
                        visibility,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    });
                }
                entries.sort_by_key(|e| e.binding);
                groups.push(Arc::new(Object::new(
                    &self.device,
                    device
                        .create_bind_group_layout(&hal::BindGroupLayoutDescriptor {
                            label: None,
                            flags: hal::BindGroupLayoutFlags::empty(),
                            entries: &entries,
                        })
                        .map_err(|e| e.to_string())?,
                )));
            }
            let raw = device
                .create_pipeline_layout(&hal::PipelineLayoutDescriptor {
                    label: None,
                    flags: hal::PipelineLayoutFlags::FIRST_VERTEX_INSTANCE
                        | hal::PipelineLayoutFlags::NUM_WORK_GROUPS,
                    bind_group_layouts: &groups.iter().map(|g| Some(&***g)).collect::<Vec<_>>(),
                    immediate_size: 0,
                })
                .map_err(|e| e.to_string())?;
            let layout = Arc::new(Layout {
                raw: Object::new(&self.device, raw),
                groups: groups.try_into().ok().unwrap(),
            });
            self.layouts.insert(key, layout.clone());
            Ok(layout)
        }
    }

    pub fn pipeline(
        &mut self,
        key: &PipelineKey,
        ps: &SamplerKinds,
        vs: &SamplerKinds,
    ) -> Result<Arc<Pipeline>, String> {
        if let Some(p) = self.pipelines.get(key) {
            return Ok(p.clone());
        }
        let layout = self.layout(ps, vs)?;
        let vertex = self.modules.get(&key.vs).ok_or("missing vertex module")?;
        let fragment = self.modules.get(&key.ps).ok_or("missing pixel module")?;
        let attrs = key
            .buffers
            .iter()
            .map(|b| {
                b.2.iter()
                    .map(|&(format, offset, shader_location)| wgpu::VertexAttribute {
                        format,
                        offset,
                        shader_location,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let buffers = key
            .buffers
            .iter()
            .zip(&attrs)
            .map(|(b, a)| hal::VertexBufferLayout {
                array_stride: b.0,
                step_mode: b.1,
                attributes: a,
            })
            .collect::<Vec<_>>();
        let constants = Default::default();
        let stage = |module| hal::ProgrammableStage {
            module,
            entry_point: "main",
            constants: &constants,
            zero_initialize_workgroup_memory: true,
        };
        unsafe {
            let device = self.device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let raw = device
                .create_render_pipeline(&hal::RenderPipelineDescriptor {
                    label: None,
                    layout: &*layout.raw,
                    vertex_processor: hal::VertexProcessor::Standard {
                        vertex_buffers: &buffers,
                        vertex_stage: stage(&***vertex),
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: key.topology,
                        strip_index_format: key.strip_index,
                        front_face: wgpu::FrontFace::Cw,
                        cull_mode: key.cull,
                        polygon_mode: key.polygon_mode,
                        ..Default::default()
                    },
                    depth_stencil: key.depth.clone(),
                    multisample: wgpu::MultisampleState {
                        count: key.samples,
                        mask: key.sample_mask as u64,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment_stage: Some(stage(&***fragment)),
                    color_targets: &key.targets,
                    multiview_mask: None,
                    cache: None,
                })
                .map_err(|e| e.to_string())?;
            let vertex_layouts = key
                .buffers
                .iter()
                .map(|b| {
                    (
                        b.0,
                        b.1,
                        b.2.iter().map(|(f, o, _)| o + f.size()).max().unwrap_or(0),
                    )
                })
                .collect();
            let pipeline = Arc::new(Pipeline {
                raw: Object::new(&self.device, raw),
                layout,
                uniforms: self.uniforms.clone(),
                vertex_layouts,
                samples: key.samples,
            });
            self.pipelines.insert(key.clone(), pipeline.clone());
            Ok(pipeline)
        }
    }

    pub fn sampler(&mut self, key: &wgpu::Sampler, desc: &wgpu::SamplerDescriptor<'_>) -> Result<(), String> {
        unsafe {
            let device = self.device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let raw = device
                .create_sampler(&hal::SamplerDescriptor {
                    label: None,
                    address_modes: [desc.address_mode_u, desc.address_mode_v, desc.address_mode_w],
                    mag_filter: desc.mag_filter,
                    min_filter: desc.min_filter,
                    mipmap_filter: desc.mipmap_filter,
                    lod_clamp: desc.lod_min_clamp..desc.lod_max_clamp,
                    compare: desc.compare,
                    anisotropy_clamp: desc.anisotropy_clamp,
                    border_color: desc.border_color,
                })
                .map_err(|e| e.to_string())?;
            self.samplers
                .insert(key.clone(), Arc::new(Object::new(&self.device, raw)));
        }
        Ok(())
    }

    pub fn group(
        &mut self,
        key: &wgpu::BindGroup,
        pipeline: &Pipeline,
        stage: usize,
        views: &[(u32, wgpu::TextureView, wgpu::Sampler)],
    ) -> Result<(), String> {
        let samplers = views
            .iter()
            .map(|(_, _, s)| self.samplers.get(s).cloned().ok_or("missing sampler"))
            .collect::<Result<Vec<_>, _>>()?;
        let owned = views
            .iter()
            .map(|(_, v, _)| self.views.get(v).cloned().ok_or("missing view"))
            .collect::<Result<Vec<_>, _>>()?;
        unsafe {
            let device = self.device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let textures = owned
                .iter()
                .map(|v| hal::TextureBinding {
                    view: &*v.raw,
                    usage: wgpu::TextureUses::RESOURCE,
                })
                .collect::<Vec<_>>();
            let entries = views
                .iter()
                .enumerate()
                .flat_map(|(i, (n, _, _))| {
                    [
                        hal::BindGroupEntry {
                            binding: 2 * n,
                            resource_index: i as u32,
                            count: 1,
                        },
                        hal::BindGroupEntry {
                            binding: 2 * n + 1,
                            resource_index: i as u32,
                            count: 1,
                        },
                    ]
                })
                .collect::<Vec<_>>();
            let layout = pipeline.layout.groups[stage + 1].clone();
            let raw = device
                .create_bind_group(&hal::BindGroupDescriptor {
                    label: None,
                    layout: &**layout,
                    buffers: &[],
                    samplers: &samplers.iter().map(|s| &***s).collect::<Vec<_>>(),
                    textures: &textures,
                    entries: &entries,
                    acceleration_structures: &[],
                    external_textures: &[],
                })
                .map_err(|e| e.to_string())?;
            self.groups.insert(
                key.clone(),
                Arc::new(Group {
                    raw: Object::new(&self.device, raw),
                    textures: views.iter().map(|(_, v, _)| v.clone()).collect(),
                    buffers: vec![],
                    _samplers: samplers,
                    _layout: layout,
                    _views: owned,
                }),
            );
        }
        for (_, view, _) in views {
            self.views.remove(view);
        }
        Ok(())
    }

    pub fn view(
        &mut self,
        view: &wgpu::TextureView,
        desc: &wgpu::TextureViewDescriptor<'_>,
        usage: wgpu::TextureUses,
    ) -> Result<(), String> {
        let tx = view.texture();
        unsafe {
            let device = self.device.as_hal::<Vulkan>().ok_or("not Vulkan")?;
            let texture = tx.as_hal::<Vulkan>().ok_or("texture destroyed")?;
            let raw = device
                .create_texture_view(
                    &texture,
                    &hal::TextureViewDescriptor {
                        label: None,
                        format: desc.format.unwrap_or(tx.format()),
                        dimension: desc.dimension.unwrap_or(wgpu::TextureViewDimension::D2),
                        usage,
                        range: wgpu::ImageSubresourceRange {
                            aspect: desc.aspect,
                            base_mip_level: desc.base_mip_level,
                            mip_level_count: Some(
                                desc.mip_level_count
                                    .unwrap_or(tx.mip_level_count() - desc.base_mip_level),
                            ),
                            base_array_layer: desc.base_array_layer,
                            array_layer_count: Some(desc.array_layer_count.unwrap_or(
                                if tx.dimension() == wgpu::TextureDimension::D3 {
                                    1
                                } else {
                                    tx.depth_or_array_layers() - desc.base_array_layer
                                },
                            )),
                        },
                    },
                )
                .map_err(|e| e.to_string())?;
            self.views.insert(
                view.clone(),
                Arc::new(View {
                    raw: Object::new(&self.device, raw),
                    _source: view.clone(),
                }),
            );
        }
        Ok(())
    }

    pub fn release_resources(&mut self) {
        self.groups.clear();
        self.views.clear();
        self.initialized = Default::default();
    }
}
