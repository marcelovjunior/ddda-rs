//! Field streaming for `--play` on stage 100: the cells around the followed
//! player load and unload as the original decides (`ddworld::stream`):
//! terrain models and collision in their 5x5 windows, layouts (placed
//! scenery and its collision) in 3x3, each trimmed by the distance test.
//!
//! *Viewer choices*, not the original's:
//! - The distant ring is the viewer's (each cell's terrain and `fmfore`
//!   model, `--far-cells` around the player outside the model window). The
//!   original's regional coarse models (0x00c5d7a0) are not read.
//! - Models load within a time budget per frame instead of the original's
//!   asynchronous resource states (no fade in or out).
//! - Collision is rebuilt on a worker thread and swapped in whole.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use ddworld::stream::{Grid, Kind};

use crate::play::PlayState;
use crate::stage::{CellPart, Loader, StreamCell, UNIT};

/// Time the cell loading may take per frame.
const BUDGET: Duration = Duration::from_millis(8);

#[derive(Resource)]
pub struct Streamer {
    pub loader: Loader<'static>,
    vfs: &'static ddvfs::Vfs,
    grid: Grid,
    stage: u32,
    /// The viewer origin's cell `(m, n)` (viewer offset zero).
    origin: (i32, i32),
    /// The predicate's quality multiplier (*assumed*: the config's
    /// ViewRange stands for `[0x018d1d20]+0x38`).
    quality: f32,
    /// The viewer's distant ring, in cells.
    far: i32,
    present: HashSet<StreamCell>,
    /// The collision and layout cells the current terrain was built from.
    built: (Vec<(i32, i32)>, Vec<(i32, i32)>),
    terrain: Option<Task<(Vec<(i32, i32)>, Vec<(i32, i32)>, ddworld::Terrain)>>,
    scenery: Option<Arc<ddworld::SceneryIndex>>,
}

impl Streamer {
    /// `present` holds what setup already spawned (tagged).
    pub fn new(
        loader: Loader<'static>,
        vfs: &'static ddvfs::Vfs,
        grid: Grid,
        stage: u32,
        origin: (i32, i32),
        far: i32,
        present: HashSet<StreamCell>,
    ) -> Self {
        let quality = loader.view_range.clamp(1, 3) as f32;
        Streamer {
            loader,
            vfs,
            grid,
            stage,
            origin,
            quality,
            far,
            present,
            built: (vec![], vec![]),
            terrain: None,
            scenery: None,
        }
    }

    /// The viewer offset of cell `(m, n)`.
    fn offset(&self, (m, n): (i32, i32)) -> Vec3 {
        Vec3::new((n - self.origin.1) as f32 * 100.0, 0.0, (m - self.origin.0) as f32 * 100.0)
    }

    /// The stage-frame position of the viewer origin.
    fn origin_stage(&self) -> Vec3 {
        Vec3::new((self.origin.1 - 50) as f32 * 10000.0, 0.0, (self.origin.0 - 50) as f32 * 10000.0)
    }

    /// Everything that should be spawned for a player at `pos`.
    fn desired(&self, pos: [f32; 3]) -> HashSet<StreamCell> {
        let cell = |(m, n), part| StreamCell { m, n, part };
        let models: HashSet<(i32, i32)> = self.grid.wanted(Kind::Model, pos, self.quality).into_iter().collect();
        let mut out: HashSet<StreamCell> = models.iter().map(|&c| cell(c, CellPart::Model)).collect();
        out.extend(self.grid.wanted(Kind::Layout, pos, self.quality).into_iter().map(|c| cell(c, CellPart::Layout)));
        let (m0, n0) = self.grid.name(self.grid.index(pos));
        for m in m0 - self.far..=m0 + self.far {
            for n in n0 - self.far..=n0 + self.far {
                if !models.contains(&(m, n)) {
                    out.insert(cell((m, n), CellPart::Far));
                }
            }
        }
        out
    }
}

/// Loads and unloads cells around the followed player and swaps in the
/// collision of the cells the original would have.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    streamer: Option<ResMut<Streamer>>,
    st: Option<ResMut<PlayState>>,
    cells: Query<(Entity, &StreamCell)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(mut s), Some(mut st)) = (streamer, st) else { return };
    let Some(p) = st.sim.player(st.follow) else { return };
    let pos = p.position;
    let s = &mut *s;

    // Collision: a finished rebuild replaces the terrain.
    if let Some(task) = s.terrain.as_mut() {
        if let Some((collision, layouts, terrain)) = block_on(future::poll_once(task)) {
            info!("stream: collision of {} cells + {} layouts swapped in ({} triangles)", collision.len(), layouts.len(), terrain.triangles());
            st.terrain = terrain;
            s.built = (collision, layouts);
            s.terrain = None;
        }
    }
    let mut collision = s.grid.wanted(Kind::Collision, pos, s.quality);
    let mut layouts = s.grid.wanted(Kind::Layout, pos, s.quality);
    collision.sort();
    layouts.sort();
    if s.terrain.is_none() && (&collision, &layouts) != (&s.built.0, &s.built.1) {
        let (vfs, stage) = (s.vfs, s.stage);
        let index = s.scenery.get_or_insert_with(|| Arc::new(ddworld::Terrain::scenery_index(vfs))).clone();
        s.terrain = Some(AsyncComputeTaskPool::get().spawn(async move {
            let mut t = ddworld::Terrain::load_collision_cells(vfs, stage, collision.iter().copied());
            t.add_scenery_cells(vfs, &index, stage, layouts.iter().copied());
            (collision, layouts, t)
        }));
    }

    // Models, layouts and the distant ring.
    let desired = s.desired(pos);
    let mut dropped = 0;
    for (e, tag) in &cells {
        if !desired.contains(tag) {
            commands.entity(e).despawn();
            dropped += 1;
        }
    }
    s.present.retain(|t| desired.contains(t));
    let mut missing: Vec<StreamCell> = desired.iter().filter(|t| !s.present.contains(t)).copied().collect();
    if missing.is_empty() {
        if dropped > 0 {
            debug!("stream: {dropped} entities dropped");
        }
        return;
    }
    // nearest first; models and layouts before the distant ring
    let (m0, n0) = s.grid.name(s.grid.index(pos));
    missing.sort_by_key(|t| (t.part == CellPart::Far, (t.m - m0).abs().max((t.n - n0).abs())));
    s.loader.lod_eye = Some((Vec3::from(pos) - s.origin_stage()) * UNIT);
    let start = Instant::now();
    for t in missing {
        if start.elapsed() > BUDGET {
            break;
        }
        let offset = s.offset((t.m, t.n));
        s.loader.tag = Some(t);
        let (stage, l) = (s.stage, &mut s.loader);
        match t.part {
            CellPart::Model => {
                l.spawn_cell_model(&mut commands, &mut meshes, &mut materials, &mut images, stage, t.m, t.n, offset);
            }
            CellPart::Layout => {
                l.spawn_cell_layout(&mut commands, &mut meshes, &mut materials, &mut images, stage, t.m, t.n, offset);
            }
            CellPart::Far => {
                l.spawn_far_cell(&mut commands, &mut meshes, &mut materials, &mut images, stage, t.m, t.n, offset);
            }
        }
        s.loader.tag = None;
        // a cell without data counts as present, so it is not retried
        s.present.insert(t);
    }
}
