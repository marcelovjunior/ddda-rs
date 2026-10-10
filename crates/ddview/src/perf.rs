//! `--perf`: frame-time statistics every 5 seconds (median, 99th
//! percentile, worst, and frames over 50 ms), for finding stutters.

use bevy::prelude::*;

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
