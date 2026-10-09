//! `--perf`: frame-time statistics every 5 seconds (median, 99th
//! percentile, worst, and frames over 50 ms), for finding stutters.

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct Perf {
    frames: Vec<f32>,
    since: f32,
    unfocused_frames: usize,
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
) {
    let Some(mut p) = perf else { return };
    ON.store(true, std::sync::atomic::Ordering::Relaxed);
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
