//! `--perf`: frame-time statistics every 5 seconds (median, 99th
//! percentile, worst, and frames over 50 ms), for finding stutters.

use bevy::prelude::*;

/// Normal logging is unchanged. A profile-trace build can opt into a short
/// stage-specific span window while keeping ordinary events throughout the
/// route; this avoids writing a many-gigabyte trace before reaching the field.
pub fn log_plugin() -> bevy::log::LogPlugin {
    #[cfg(feature = "profile-trace")]
    { field_trace::log_plugin() }
    #[cfg(not(feature = "profile-trace"))]
    { default() }
}

#[cfg(feature = "profile-trace")]
mod field_trace {
    use super::*;
    use std::{collections::HashMap, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, thread::ThreadId};
    use bevy::log::{BoxedLayer, LogPlugin, tracing::{self, span}, tracing_subscriber::{Layer, layer::Context, registry::Registry}};

    #[derive(Resource)]
    pub struct TraceWindow {
        pub stage: u32,
        delay: f64,
        duration: f64,
        target_since: Option<f64>,
        active_since: Option<f64>,
        done: bool,
        enabled: Arc<AtomicBool>,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub enum Transition { None, Started, Finished, LeftStage }

    impl TraceWindow {
        fn new(stage: u32, delay: f64, duration: f64) -> Self {
            Self { stage, delay, duration, target_since: None, active_since: None,
                done: false, enabled: Arc::new(AtomicBool::new(false)) }
        }

        pub fn advance(&mut self, stage: u32, now: f64) -> Transition {
            if self.done { return Transition::None; }
            if stage != self.stage {
                self.target_since = None;
                if self.active_since.is_some() {
                    self.enabled.store(false, Ordering::Release);
                    self.done = true;
                    return Transition::LeftStage;
                }
                return Transition::None;
            }
            if let Some(start) = self.active_since {
                if now - start >= self.duration {
                    self.enabled.store(false, Ordering::Release);
                    self.done = true;
                    return Transition::Finished;
                }
            } else if now - *self.target_since.get_or_insert(now) >= self.delay {
                // Start at the actual update, so a loading hitch cannot skip
                // the entire window or reduce its requested sample duration.
                self.active_since = Some(now);
                self.enabled.store(true, Ordering::Release);
                return Transition::Started;
            }
            Transition::None
        }
    }

    /// Bevy's SystemMeta caches a Span during plugin build. Filtering its
    /// creation would permanently disable that span, even after the window
    /// opens. Keep span metadata, gate the Chrome enter callbacks instead,
    /// and pair exits using the decision made at enter (including boundaries).
    struct WindowLayer<L> {
        inner: L,
        enabled: Arc<AtomicBool>,
        entered: Mutex<HashMap<ThreadId, Vec<(span::Id, bool)>>>,
    }

    impl<L> WindowLayer<L> {
        fn new(inner: L, enabled: Arc<AtomicBool>) -> Self {
            Self { inner, enabled, entered: Mutex::new(HashMap::new()) }
        }
    }

    impl<L: Layer<Registry>> Layer<Registry> for WindowLayer<L> {
        fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, Registry>) {
            self.inner.on_new_span(attrs, id, ctx);
        }
        fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, Registry>) {
            self.inner.on_record(id, values, ctx);
        }
        fn on_enter(&self, id: &span::Id, ctx: Context<'_, Registry>) {
            let record = self.enabled.load(Ordering::Acquire);
            self.entered.lock().unwrap().entry(std::thread::current().id())
                .or_default().push((id.clone(), record));
            if record { self.inner.on_enter(id, ctx); }
        }
        fn on_exit(&self, id: &span::Id, ctx: Context<'_, Registry>) {
            let (entered_id, record) = self.entered.lock().unwrap()
                .get_mut(&std::thread::current().id()).and_then(Vec::pop)
                .expect("trace span exit must pair with its thread's enter");
            debug_assert_eq!(&entered_id, id);
            if record { self.inner.on_exit(id, ctx); }
        }
        fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, Registry>) {
            self.inner.on_event(event, ctx);
        }
        fn on_close(&self, id: span::Id, ctx: Context<'_, Registry>) {
            self.inner.on_close(id, ctx);
        }
    }

    #[derive(Resource)]
    struct TraceFlush { _guard: Mutex<tracing_chrome::FlushGuard> }

    fn chrome_name(item: &tracing_chrome::EventOrSpan<'_, '_, Registry>) -> String {
        match item {
            tracing_chrome::EventOrSpan::Event(event) => event.metadata().name().into(),
            tracing_chrome::EventOrSpan::Span(span) => {
                use bevy::log::tracing_subscriber::fmt::{FormattedFields, format::DefaultFields};
                if let Some(fields) = span.extensions().get::<FormattedFields<DefaultFields>>() {
                    format!("{}: {}", span.metadata().name(), fields.fields.as_str())
                } else { span.metadata().name().into() }
            }
        }
    }

    fn trace_layer(app: &mut App) -> Option<BoxedLayer> {
        let stage = std::env::var("DDVIEW_TRACE_STAGE").ok().and_then(|value| {
            value.parse::<u32>().map_err(|_| {
                eprintln!("DDVIEW_TRACE_STAGE must be a stage number; full tracing retained");
            }).ok()
        });
        let read_seconds = |name: &str, fallback: f64, minimum: f64, maximum: f64| {
            std::env::var(name).ok().and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value >= minimum && *value <= maximum)
                .unwrap_or(fallback)
        };
        let enabled = if let Some(stage) = stage {
            let window = TraceWindow::new(stage,
                read_seconds("DDVIEW_TRACE_DELAY", 5.0, 0.0, 60.0),
                read_seconds("DDVIEW_TRACE_SECONDS", 10.0, 0.001, 30.0));
            eprintln!("ddview: trace window stage {stage}, delay {} wall seconds, duration {} wall seconds; normal exit afterwards", window.delay, window.duration);
            let enabled = window.enabled.clone();
            app.insert_resource(window);
            enabled
        } else { Arc::new(AtomicBool::new(true)) };
        let path = std::env::var("TRACE_CHROME").unwrap_or_else(|_| "chrome-trace.json".into());
        let (chrome, guard) = tracing_chrome::ChromeLayerBuilder::<Registry>::new()
            .file(path)
            // Match Bevy's names, including each cached system's recorded name.
            .name_fn(Box::new(chrome_name))
            .build();
        app.insert_resource(TraceFlush { _guard: Mutex::new(guard) });
        Some(Box::new(WindowLayer::new(chrome, enabled)))
    }

    pub fn log_plugin() -> LogPlugin {
        LogPlugin { custom_layer: trace_layer, ..default() }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bevy::log::{tracing, tracing_subscriber::{Layer, layer::Context, prelude::*}};
        use std::sync::atomic::AtomicUsize;

        #[test]
        fn field_window_uses_actual_stage_and_wall_duration_even_after_loading_hitch() {
            let mut window = TraceWindow::new(100, 5.0, 10.0);
            assert_eq!(window.advance(200, 0.0), Transition::None);
            assert_eq!(window.advance(100, 30.0), Transition::None);
            assert_eq!(window.advance(100, 34.9), Transition::None);
            assert_eq!(window.advance(100, 50.0), Transition::Started);
            assert!(window.enabled.load(Ordering::Acquire));
            assert_eq!(window.advance(100, 59.9), Transition::None);
            assert_eq!(window.advance(100, 60.0), Transition::Finished);
            assert!(!window.enabled.load(Ordering::Acquire));
            assert_eq!(window.advance(100, 100.0), Transition::None);
        }

        #[test]
        fn leaving_the_target_resets_waiting_and_ends_an_active_capture() {
            let mut window = TraceWindow::new(100, 5.0, 10.0);
            window.advance(100, 0.0);
            window.advance(200, 4.0);
            window.advance(100, 10.0);
            assert_eq!(window.advance(100, 14.0), Transition::None);
            assert_eq!(window.advance(100, 15.0), Transition::Started);
            assert_eq!(window.advance(200, 16.0), Transition::LeftStage);
            assert!(!window.enabled.load(Ordering::Acquire));
            assert_eq!(window.advance(100, 30.0), Transition::None);
        }

        #[test]
        fn cached_system_span_records_only_window_entries_and_keeps_events() {
            #[derive(Clone)]
            struct Count(Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>);
            impl<S: tracing::Subscriber> Layer<S> for Count {
                fn on_enter(&self, _: &span::Id, _: Context<'_, S>) {
                    self.0.fetch_add(1, Ordering::Relaxed);
                }
                fn on_exit(&self, _: &span::Id, _: Context<'_, S>) {
                    self.1.fetch_add(1, Ordering::Relaxed);
                }
                fn on_event(&self, _: &tracing::Event<'_>, _: Context<'_, S>) {
                    self.2.fetch_add(1, Ordering::Relaxed);
                }
            }
            let enabled = Arc::new(AtomicBool::new(false));
            let count = Count(Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
            let subscriber = Registry::default().with(WindowLayer::new(count.clone(), enabled.clone()));
            tracing::subscriber::with_default(subscriber, || {
                // This is the actual Bevy contract: construct once BEFORE the
                // window, then repeatedly enter the same cached Span.
                let cached = tracing::info_span!("system", name = "cached_bevy_system");
                assert!(!cached.is_disabled());
                cached.in_scope(|| tracing::info!("before window"));
                enabled.store(true, Ordering::Release);
                let entered = cached.enter();
                tracing::info!("inside window");
                // A recorded entry must still get its exit after window close.
                enabled.store(false, Ordering::Release);
                drop(entered);
                cached.in_scope(|| tracing::info!("after window"));
                // Also test the inverse boundary and reentry of the SAME ID:
                // the outer entry was suppressed, the inner one is recorded.
                let outer = cached.enter();
                enabled.store(true, Ordering::Release);
                cached.in_scope(|| {});
                drop(outer);
            });
            assert_eq!(count.0.load(Ordering::Relaxed), 2);
            assert_eq!(count.1.load(Ordering::Relaxed), 2);
            assert_eq!(count.2.load(Ordering::Relaxed), 3);
        }

        #[test]
        fn bevy_system_created_before_window_is_captured_when_reused() {
            #[derive(Clone)]
            struct SystemEntries(Arc<AtomicUsize>);
            impl Layer<Registry> for SystemEntries {
                fn on_enter(&self, id: &span::Id, ctx: Context<'_, Registry>) {
                    if ctx.span(id).unwrap().metadata().name() == "system" {
                        self.0.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let enabled = Arc::new(AtomicBool::new(false));
            let entries = Arc::new(AtomicUsize::new(0));
            let runs = Arc::new(AtomicUsize::new(0));
            let probe_runs = runs.clone();
            let subscriber = Registry::default().with(WindowLayer::new(SystemEntries(entries.clone()), enabled.clone()));
            tracing::subscriber::with_default(subscriber, || {
                fn cached_named_probe() {}
                use bevy::ecs::system::{IntoSystem, System};
                let named_system = IntoSystem::into_system(cached_named_probe);
                assert!(named_system.name().as_string().contains("cached_named_probe"),
                    "release profiling must retain function-derived names; enable Bevy debug");
                let mut app = App::new();
                // IntoSystem constructs and caches SystemMeta's actual span
                // while recording is off. No rename/rebuild is done later.
                app.add_systems(Update, move || { probe_runs.fetch_add(1, Ordering::Relaxed); });
                app.add_systems(Update, named_system);
                app.update();
                assert_eq!(entries.load(Ordering::Relaxed), 0);
                enabled.store(true, Ordering::Release);
                app.update();
                let recorded = entries.load(Ordering::Relaxed);
                assert!(recorded > 0, "the reused Bevy system span must enter Chrome");
                enabled.store(false, Ordering::Release);
                app.update();
                assert_eq!(entries.load(Ordering::Relaxed), recorded);
            });
            assert_eq!(runs.load(Ordering::Relaxed), 3);
        }

        #[test]
        fn chrome_output_retains_cached_system_name_with_log_plugin_layer_order() {
            #[derive(Clone)]
            struct Output(Arc<Mutex<Vec<u8>>>);
            impl std::io::Write for Output {
                fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                    self.0.lock().unwrap().extend_from_slice(bytes);
                    Ok(bytes.len())
                }
                fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
            }
            let output = Output(Arc::new(Mutex::new(Vec::new())));
            let enabled = Arc::new(AtomicBool::new(false));
            let (chrome, guard) = tracing_chrome::ChromeLayerBuilder::<Registry>::new()
                .writer(output.clone()).name_fn(Box::new(chrome_name)).build();
            // LogPlugin's custom_layer is inside its formatting layer. The
            // latter adds FormattedFields after on_new_span; on_enter must
            // still find them when it resolves Chrome's cached-system name.
            let subscriber = Registry::default()
                .with(WindowLayer::new(chrome, enabled.clone()))
                .with(bevy::log::tracing_subscriber::fmt::Layer::default()
                    .with_writer(std::io::sink).with_ansi(false));
            tracing::subscriber::with_default(subscriber, || {
                fn chrome_cached_system_probe() {}
                let mut app = App::new();
                // A real function-derived cached system name, rather than a
                // manually named span that bypasses release DebugName erasure.
                app.add_systems(Update, chrome_cached_system_probe);
                app.update();
                enabled.store(true, Ordering::Release);
                app.update();
                enabled.store(false, Ordering::Release);
                app.update();
            });
            drop(guard); // Actual writer thread exit/flush before inspection.
            let trace = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
            let entries: Vec<_> = trace.lines().filter(|line|
                line.contains("system: name=") && line.contains("chrome_cached_system_probe")).collect();
            assert_eq!(entries.len(), 2, "one cached entry/exit, none outside window: {trace}");
            assert!(entries[0].contains("system: name="), "system name must include recorded fields");
            assert!(entries[0].contains("\"ph\":\"B\""));
            assert!(entries[1].contains("\"ph\":\"E\""));
            assert!(trace.trim_end().ends_with(']'), "trace must close normally");
        }
    }
}

/// Opt-in renderer experiment; preserve Bevy's default until identical-scene
/// captures demonstrate a benefit. This changes instance-buffer preparation,
/// not simulation, geometry, materials or the original's LOD selection.
/// Bevy 0.19.1: PbrPlugin forwards this flag to MeshRenderPlugin and
/// GpuMeshPreprocessPlugin; render/mesh.rs selects its CPU extraction/batching
/// path, and render/gpu_preprocess.rs skips GPU preprocessing systems.
pub fn pbr_plugin() -> bevy::pbr::PbrPlugin {
    bevy::pbr::PbrPlugin { use_gpu_instance_buffer_builder: uses_gpu_mesh_builder(), ..default() }
}

fn uses_gpu_mesh_builder() -> bool {
    static USE_GPU: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *USE_GPU.get_or_init(|| {
        let mode = std::env::var("DDVIEW_MESH_BUILDER").ok();
        let use_gpu = match mode.as_deref() {
            None | Some("gpu") => true,
            Some("cpu") => false,
            Some(value) => {
                eprintln!("DDVIEW_MESH_BUILDER={value:?}: expected cpu or gpu; using GPU default");
                true
            }
        };
        if mode.is_some() {
            eprintln!("ddview: mesh instance builder {} (renderer experiment)", if use_gpu { "gpu" } else { "cpu" });
        }
        use_gpu
    })
}

/// CPU instance buffers and render-phase classification must agree. Setting
/// PbrPlugin's builder flag alone leaves the device capability at Culling;
/// Bevy then queues multidraw meshes that its CPU batching path never fills.
/// Install after DefaultPlugins, before their finish hooks choose render paths.
pub struct MeshBuilderPlugin;

impl Plugin for MeshBuilderPlugin {
    fn build(&self, app: &mut App) {
        configure_mesh_builder(app, uses_gpu_mesh_builder());
        // Both sides of the S41 opt-in A/B use this diagnostic. Report the
        // actual staging upload extent, not an estimate from visible meshes:
        // Bevy retains high-water staging storage after freeing a skin.
        if std::env::var("DDVIEW_SKIN_DIAGNOSTICS").is_ok_and(|value| value == "1") {
            if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
                render.add_systems(bevy::render::Render, report_skin_upload
                    .in_set(bevy::render::RenderSystems::PrepareResources)
                    .after(bevy::pbr::prepare_skins));
            }
        }
    }
}

fn report_skin_upload(
    skins: Res<bevy::pbr::SkinUniforms>,
    mut previous: Local<Option<(usize, usize)>>,
) {
    let state = (skins.all_skins().count(), skins.current_staging_buffer.len());
    if *previous != Some(state) {
        info!("SKIN_UPLOAD palettes={} staging_matrices={} upload_bytes={} current_buffer_bytes={} previous_buffer_bytes={}",
            state.0, state.1, state.1 * std::mem::size_of::<Mat4>(),
            skins.current_buffer.size(), skins.prev_buffer.size());
        *previous = Some(state);
    }
}

fn configure_mesh_builder(app: &mut App, use_gpu: bool) {
    use bevy::render::{RenderApp, RenderStartup, init_gpu_resource, batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport}};
    if !use_gpu {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(GpuPreprocessingSupport { max_supported_mode: GpuPreprocessingMode::None });
            // Bevy 0.19.1 GpuPreprocessingPlugin::finish registers an
            // unconditional init_gpu_resource in RenderStartup (also run on
            // device recovery). A build-time override alone gets overwritten.
            render.add_systems(RenderStartup, force_cpu_mesh_phases
                .after(init_gpu_resource::<GpuPreprocessingSupport>));
        }
    }
}

fn force_cpu_mesh_phases(mut support: ResMut<bevy::render::batching::gpu_preprocessing::GpuPreprocessingSupport>) {
    support.max_supported_mode = bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode::None;
    info!("CPU mesh phases: preprocessing None after RenderStartup device initialization");
}

#[cfg(test)]
mod mesh_builder_tests {
    use super::*;
    use bevy::app::SubApp;
    use bevy::core_pipeline::core_3d::{Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey};
    use bevy::render::{RenderApp, batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport},
        render_phase::{BinnedRenderPhaseType, DrawFunctionId, InputUniformIndex, ViewBinnedRenderPhases},
        render_resource::CachedRenderPipelineId, view::RetainedViewEntity};

    fn queue_one_opaque_mesh(support: &GpuPreprocessingSupport) -> (usize, usize) {
        let entity = Entity::from_raw_u32(1).unwrap();
        let view = RetainedViewEntity::new(entity.into(), None, 0);
        let mut phases = ViewBinnedRenderPhases::<Opaque3d>::default();
        phases.prepare_for_new_frame(view, support.min(GpuPreprocessingMode::Culling));
        let phase = phases.get_mut(&view).unwrap();
        phase.add(
            Opaque3dBatchSetKey { pipeline: CachedRenderPipelineId::INVALID,
                draw_function: DrawFunctionId(0), material_bind_group_index: None,
                slabs: default(), lightmap_slab: None },
            Opaque3dBinKey { asset_id: AssetId::<Mesh>::invalid().untyped() },
            (entity, entity.into()), InputUniformIndex::default(), BinnedRenderPhaseType::mesh(true, support),
        );
        (phase.batchable_meshes.len(), phase.multidrawable_meshes.len())
    }

    #[test]
    fn cpu_builder_queues_cpu_phase_buffers_even_on_a_gpu_capable_device() {
        let mut app = App::new();
        let mut render = SubApp::new();
        render.insert_resource(GpuPreprocessingSupport { max_supported_mode: GpuPreprocessingMode::Culling });
        app.insert_sub_app(RenderApp, render);
        configure_mesh_builder(&mut app, false);
        let support = app.sub_app(RenderApp).world().resource::<GpuPreprocessingSupport>();
        assert!(support.max_supported_mode == GpuPreprocessingMode::None);
        assert!(matches!(BinnedRenderPhaseType::mesh(true, support), BinnedRenderPhaseType::BatchableMesh));
        // CPU batching visits batchable_meshes, never multidrawable_meshes.
        assert_eq!(queue_one_opaque_mesh(support), (1, 0));
    }

    #[test]
    fn default_gpu_builder_retains_device_capability_and_indirect_phases() {
        let mut app = App::new();
        let mut render = SubApp::new();
        render.insert_resource(GpuPreprocessingSupport { max_supported_mode: GpuPreprocessingMode::Culling });
        app.insert_sub_app(RenderApp, render);
        configure_mesh_builder(&mut app, true);
        let support = app.sub_app(RenderApp).world().resource::<GpuPreprocessingSupport>();
        assert!(support.max_supported_mode == GpuPreprocessingMode::Culling);
        assert_eq!(queue_one_opaque_mesh(support), (0, 1));
    }

    #[test]
    fn cpu_phase_contract_survives_device_startup_and_recovery() {
        use bevy::render::RenderStartup;
        // Reproduce init_gpu_resource's unconditional resource replacement
        // without requiring a real GPU/RenderDevice in this headless test.
        fn reset_device_capability(world: &mut World) {
            world.insert_resource(GpuPreprocessingSupport { max_supported_mode: GpuPreprocessingMode::Culling });
        }
        let mut app = App::new();
        app.insert_sub_app(RenderApp, SubApp::new());
        configure_mesh_builder(&mut app, false);
        let render = app.sub_app_mut(RenderApp);
        render.add_systems(RenderStartup, reset_device_capability.before(force_cpu_mesh_phases));
        for _ in 0..2 {
            render.world_mut().run_schedule(RenderStartup);
            let support = render.world().resource::<GpuPreprocessingSupport>();
            assert!(support.max_supported_mode == GpuPreprocessingMode::None);
            assert_eq!(queue_one_opaque_mesh(support), (1, 0));
        }
    }
}

#[derive(Resource, Default)]
pub struct Perf {
    frames: Vec<f32>,
    since: f32,
    unfocused_frames: usize,
    start: Option<std::time::Instant>,
    stop_after: Option<f64>,
    stage: Option<u32>,
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    perf: Option<ResMut<Perf>>,
    time: Res<Time<Real>>,
    all: Query<Entity>,
    audio: Query<(), With<AudioPlayer>>,
    meshes: Query<(&Mesh3d, &ViewVisibility, Option<&MeshMaterial3d<crate::stage::DdMaterial>>)>,
    nodes: Query<(), With<Node>>,
    dd_materials: Res<Assets<crate::stage::DdMaterial>>,
    windows: Query<&Window>,
    args: Res<crate::Args>,
    stream: Option<Res<crate::stream::Streamer>>,
    mut exit: MessageWriter<AppExit>,
    #[cfg(feature = "profile-trace")] mut trace_window: Option<ResMut<field_trace::TraceWindow>>,
) {
    let Some(mut p) = perf else { return };
    ON.store(true, std::sync::atomic::Ordering::Relaxed);
    if p.start.is_none() {
        p.start = Some(std::time::Instant::now());
        if let Some(value) = std::env::var_os("DDVIEW_PERF_SECONDS") {
            p.stop_after = value.to_str().and_then(|v| v.parse::<f64>().ok())
                .filter(|s| s.is_finite() && *s > 0.0);
            if p.stop_after.is_none() {
                warn!("perf: DDVIEW_PERF_SECONDS must be finite and positive; timed exit disabled");
            }
        }
    }
    let elapsed = p.start.unwrap().elapsed().as_secs_f64();
    if p.stop_after.is_some_and(|seconds| elapsed >= seconds) {
        info!("perf: timed benchmark completed after {elapsed:.3} seconds; normal app exit");
        exit.write(AppExit::Success);
        p.stop_after = None;
    }
    let stage = stream.as_ref().map_or(args.stage, |stream| stream.stage);
    #[cfg(feature = "profile-trace")]
    if let Some(window) = trace_window.as_mut() {
        match window.advance(stage, elapsed) {
            field_trace::Transition::Started => {
                info!(name: "s41_trace_window_start", stage, elapsed, "stage-specific CPU trace window started");
            }
            field_trace::Transition::Finished | field_trace::Transition::LeftStage => {
                info!(name: "s41_trace_window_end", stage, elapsed, "stage-specific CPU trace window ended; normal app exit");
                exit.write(AppExit::Success);
            }
            field_trace::Transition::None => {}
        }
    }
    if p.stage != Some(stage) {
        // A window crossing a stage load cannot describe either steady scene.
        p.frames.clear();
        p.since = 0.0;
        p.unfocused_frames = 0;
        p.stage = Some(stage);
        info!("perf: stage {stage} starts at elapsed {elapsed:.3} seconds");
        report_sections();
    }
    let dt = time.delta_secs();
    p.frames.push(dt * 1000.0);
    if !windows.iter().any(|window| window.focused) {
        p.unfocused_frames += 1;
    }
    p.since += dt;
    if p.since < 5.0 {
        return;
    }
    let mut f = std::mem::take(&mut p.frames);
    p.since = 0.0;
    f.sort_by(f32::total_cmp);
    let at = |q: f32| f[((f.len() - 1) as f32 * q) as usize];
    let long = f.iter().filter(|&&ms| ms > 50.0).count();
    let over_budget = f.iter().filter(|&&ms| ms > 1000.0 / 60.0).count();
    let over_33 = f.iter().filter(|&&ms| ms > 33.0).count();
    let unfocused_frames = std::mem::take(&mut p.unfocused_frames);
    info!(
        "perf: {} frames, median {:.1} ms, p99 {:.1} ms, worst {:.1} ms, {} over 50 ms; entities {} (meshes {}, audio {}, ui {})",
        f.len(),
        at(0.5),
        at(0.99),
        f.last().copied().unwrap_or(0.0),
        long,
        all.iter().count(),
        meshes.iter().count(),
        audio.iter().count(),
        nodes.iter().count()
    );
    info!("perf: stage {stage}, elapsed {elapsed:.3} seconds");
    let visible: Vec<_> = meshes.iter().filter(|(_, visibility, _)| visibility.get()).collect();
    let unique_meshes: std::collections::HashSet<_> = visible.iter().map(|(mesh, _, _)| mesh.0.id()).collect();
    let unique_materials: std::collections::HashSet<_> = visible.iter().filter_map(|(_, _, mat)| mat.map(|mat| mat.0.id())).collect();
    info!("perf: p95 {:.1} ms, {over_budget} over 16.67 ms, {over_33} over 33 ms; visible meshes {} ({} unique), visible DD materials {} ({} allocated). Visibility is across views, not draw calls.", at(0.95), visible.len(), unique_meshes.len(), unique_materials.len(), dd_materials.len());
    report_sections();
    info!("perf: {unfocused_frames}/{} frames without window focus", f.len());
}

/// Time spent in named sections since the last report: (total ms, count,
/// worst ms). Only recorded while `--perf` is on.
static SECTIONS: std::sync::Mutex<Vec<(&'static str, f64, u32, f64)>> = std::sync::Mutex::new(Vec::new());
static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Times its scope as section `name` (drop records it).
pub struct Section(&'static str, std::time::Instant);

impl Section {
    pub fn start(name: &'static str) -> Section {
        Section(name, std::time::Instant::now())
    }
}

impl Drop for Section {
    fn drop(&mut self) {
        if !ON.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let ms = self.1.elapsed().as_secs_f64() * 1000.0;
        let mut s = SECTIONS.lock().unwrap();
        match s.iter_mut().find(|e| e.0 == self.0) {
            Some(e) => {
                e.1 += ms;
                e.2 += 1;
                e.3 = e.3.max(ms);
            }
            None => s.push((self.0, ms, 1, ms)),
        }
    }
}

fn report_sections() {
    let mut s = SECTIONS.lock().unwrap();
    for (name, total, n, worst) in s.drain(..) {
        info!("perf: {name}: {n} runs, {:.2} ms average, {worst:.1} ms worst", total / f64::from(n));
    }
}
