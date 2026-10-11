//! ddplay: runs the rebuilt game in a window (docs/RUNTIME.md, "a
//! window/presentation host").
//!
//! The game runs on its own guest thread exactly as the runtime runs it
//! headless (`Session::start`: the CRT start-up, then WinMain), with the
//! wgpu backend (`ddgpu`) drawing its Direct3D 9 commands. Every frame the
//! game presents is handed to the window thread, which shows it scaled to
//! the window. The window title shows where the game is (running, or the
//! report of where it stopped).
//!
//!     cargo run -p ddplay -- [path\to\DDDA.exe]
//!
//! Bring-up version: frames are read back and re-uploaded (simple and
//! robust); sharing one device comes when performance matters. Input is
//! not forwarded yet (brief L16). Audio plays on the default output
//! device (`sink`, brief L19).
mod sink;

use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

enum GameEvent {
    Frame(ddgpu::Frame),
    Status(String),
}

const BLIT: &str = "
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
  let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  return V(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}
@fragment fn fs(v: V) -> @location(0) vec4<f32> { return textureSample(frame, smp, v.uv); }
";

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame: Option<(wgpu::Texture, u32, u32)>,
}

struct App {
    exe: String,
    proxy: EventLoopProxy<GameEvent>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    frames: u64,
    started: bool,
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

impl App {
    fn create_gpu(window: Arc<Window>) -> Gpu {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("a surface for the window");
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })).expect("an adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
        let size = window.inner_size();
        let config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("surface configuration");
        surface.configure(&device, &config);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: config.format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Gpu { surface, device, queue, config, pipeline, layout, sampler, frame: None }
    }

    /// Start the game on its guest thread (once).
    fn start_game(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (exe, proxy) = (self.exe.clone(), self.proxy.clone());
        std::thread::Builder::new()
            .name("guest main thread".into())
            .stack_size(256 << 20)
            .spawn(move || {
                let status = |s: String| {
                    let _ = proxy.send_event(GameEvent::Status(s));
                };
                let mut session = match ddruntime::session::Session::new(&exe) {
                    Ok(s) => s,
                    Err(e) => return status(format!("could not load {exe}: {e}")),
                };
                ddruntime::session::install_fault_hook();
                sink::start(ddruntime::audio::output());
                match ddgpu::WgpuBackend::headless() {
                    Some(mut backend) => {
                        backend.read_frames = false;
                        let frames = proxy.clone();
                        backend.on_present = Some(Box::new(move |f| {
                            let _ = frames.send_event(GameEvent::Frame(f.clone()));
                        }));
                        ddruntime::d3d9draw::set_backend(Box::new(backend));
                    }
                    None => status("no GPU adapter for the game's backend; running headless".into()),
                }
                status("running".into());
                let started = session.start();
                let threads: Vec<String> = ddruntime::threads::faults().into_iter()
                    .map(|(tid, f)| format!("guest thread {tid:#x}: {f}")).collect();
                let mut report = format!("{}: {}", started.step, started.outcome);
                for t in threads {
                    report.push_str(&format!("\n{t}"));
                }
                eprintln!("{report}");
                status(format!("stopped in {}", started.step));
            })
            .expect("the guest main thread");
    }

    fn show(&mut self, f: ddgpu::Frame) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let size = wgpu::Extent3d { width: f.width, height: f.height, depth_or_array_layers: 1 };
        if gpu.frame.as_ref().is_none_or(|(_, w, h)| (*w, *h) != (f.width, f.height)) {
            let t = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("game frame"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            gpu.frame = Some((t, f.width, f.height));
        }
        let (tex, _, _) = gpu.frame.as_ref().unwrap();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &f.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * f.width), rows_per_image: Some(f.height) },
            size,
        );
        self.frames += 1;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn redraw(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let out = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
            _ => return,
        };
        let view = out.texture.create_view(&Default::default());
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some((tex, _, _)) = &gpu.frame {
                let tv = tex.create_view(&Default::default());
                let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &gpu.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&tv) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&gpu.sampler) },
                    ],
                });
                pass.set_pipeline(&gpu.pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        gpu.queue.submit([enc.finish()]);
        out.present();
    }
}

impl ApplicationHandler<GameEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Dragon's Dogma: Dark Arisen (rebuilt) - starting")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("a window"));
        self.gpu = Some(App::create_gpu(window.clone()));
        self.window = Some(window);
        self.start_game();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: GameEvent) {
        match event {
            GameEvent::Frame(f) => self.show(f),
            GameEvent::Status(s) => {
                if let Some(w) = &self.window {
                    w.set_title(&format!("Dragon's Dogma: Dark Arisen (rebuilt) - {s} - {} frames", self.frames));
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(&gpu.device, &gpu.config);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

fn main() {
    let exe = std::env::args().nth(1).unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let event_loop = EventLoop::<GameEvent>::with_user_event().build().expect("an event loop");
    let mut app = App { exe, proxy: event_loop.create_proxy(), window: None, gpu: None, frames: 0, started: false };
    event_loop.run_app(&mut app).expect("the event loop");
}
