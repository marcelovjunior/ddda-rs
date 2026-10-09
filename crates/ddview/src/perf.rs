//! `--perf`: frame-time statistics every 5 seconds (median, 99th
//! percentile, worst, and frames over 50 ms), for finding stutters.

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct Perf {
    frames: Vec<f32>,
    since: f32,
}

pub fn update(perf: Option<ResMut<Perf>>, time: Res<Time<Real>>) {
    let Some(mut p) = perf else { return };
    let dt = time.delta_secs();
    p.frames.push(dt * 1000.0);
    p.since += dt;
    if p.since < 5.0 {
        return;
    }
    let mut f = std::mem::take(&mut p.frames);
    p.since = 0.0;
    f.sort_by(f32::total_cmp);
    let at = |q: f32| f[((f.len() - 1) as f32 * q) as usize];
    let long = f.iter().filter(|&&ms| ms > 50.0).count();
    info!(
        "perf: {} frames, median {:.1} ms, p99 {:.1} ms, worst {:.1} ms, {} over 50 ms",
        f.len(),
        at(0.5),
        at(0.99),
        f.last().copied().unwrap_or(0.0),
        long
    );
}
