//! The world around the party: field streaming on stage 100 and the
//! handoff between stages.
//!
//! **Field streaming.** On stage 100 the cells around the focus (the
//! followed player) load and unload as the original decides
//! (`ddworld::stream`): terrain models and collision in their 5x5 windows,
//! layouts (placed scenery and its collision) in 3x3, each trimmed by the
//! distance test.
//!
//! **Stage handoff** ([`Streamer::enter`]). A door scenario or a quest's
//! `JumpArea` names a stage and a start position (`ddstory::Request::JumpArea`).
//! `enter` drops the current stage's entities and returns the new stage's
//! collision around the arrival point: stage 100 streams, and any other stage
//! loads whole (`event::spawn_stage_models`,
//! `ddhost::install::stage_collision`).
//!
//! Who drives it:
//! - `--play` ([`Driver::Play`], [`play_hookup`]): the focus and the
//!   collision go through `PlayState`.
//! - the opening ([`Driver::External`]): its host sets [`Streamer::focus`]
//!   each frame and takes [`Streamer::take_terrain`] into its ground.
//!
//! *Viewer choices*, not the original's:
//! - The distant ring is the viewer's (each cell's terrain and `fmfore`
//!   model, `--far-cells` around the focus outside the model window). The
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
use crate::stage::{CellPart, Loader, StageEntity, StreamCell, UNIT};

/// Time the cell loading may take per frame.
const BUDGET: Duration = Duration::from_millis(8);

/// The stage that streams by cells.
pub const FIELD: u32 = 100;

/// Who feeds the focus and takes the collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// `--play`: `PlayState`'s followed player and terrain ([`play_hookup`]).
    Play,
    /// Another mode sets [`Streamer::focus`] and takes [`Streamer::take_terrain`].
    External,
}

type Built = (Vec<(i32, i32)>, Vec<(i32, i32)>);

#[derive(Resource)]
pub struct Streamer {
    pub loader: Loader<'static>,
    vfs: &'static ddvfs::Vfs,
    grid: Grid,
    /// The stage the party is on.
    pub stage: u32,
    pub driver: Driver,
    /// The followed player's stage-frame position; nothing streams without it.
    pub focus: Option<[f32; 3]>,
    /// Authoritative simulation interests, independent of this view's focus.
    /// Co-op collision/layout residency is the union for all current actors.
    pub actors: Vec<[f32; 3]>,
    /// The viewer origin's cell `(m, n)` (viewer offset zero).
    origin: (i32, i32),
    /// The predicate's quality multiplier (*assumed*: the config's
    /// ViewRange stands for `[0x018d1d20]+0x38`).
    quality: f32,
    /// The viewer's distant ring, in cells.
    far: i32,
    present: HashSet<StreamCell>,
    /// The collision and layout cells the current terrain was built from.
    built: Built,
    completed_collision_cells: Vec<(i32, i32)>,
    terrain: Option<Task<(Built, ddworld::Terrain)>>,
    /// A rebuilt terrain not yet taken by the driver.
    ready: Option<ddworld::Terrain>,
    scenery: Option<Arc<ddworld::SceneryIndex>>,
}

impl Streamer {
    /// `present` holds what setup already spawned (tagged); `origin` is the
    /// cell at viewer offset zero.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        loader: Loader<'static>,
        vfs: &'static ddvfs::Vfs,
        grid: Grid,
        stage: u32,
        driver: Driver,
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
            driver,
            focus: None,
            actors: Vec::new(),
            origin,
            quality,
            far,
            present,
            built: (vec![], vec![]),
            completed_collision_cells: Vec::new(),
            terrain: None,
            ready: None,
            scenery: None,
        }
    }

    /// Stage 100's grid from the installed exe, next to `native_pc`.
    pub fn field_grid(native_pc: &std::path::Path) -> Result<Grid, String> {
        let file = ddexe::ExeFile::next_to(native_pc).map_err(|e| e.0)?;
        let exe = file.image().map_err(|e| e.0)?;
        Grid::from_exe(&exe, ddworld::stream::STAGE100_GRID)
    }

    /// The newest rebuilt collision, once.
    pub fn take_terrain(&mut self) -> Option<ddworld::Terrain> {
        self.ready.take()
    }

    /// Actual completed collision rebuild, not desired/pending cells.
    /// cAreaHit's field exception queries the collision list (+0x7c/+0x88),
    /// as confirmed by 0x00c5f630 / 0x00c5f680; layout cells differ.
    pub fn scenario_field_cells(&self) -> Option<ddstory::scenario::LoadedFieldCells> {
        (self.stage == FIELD).then(|| ddstory::scenario::LoadedFieldCells {
            origin: self.grid.origin,
            cell: self.grid.cell,
            name_offset: self.grid.name_offset,
            loaded: self.completed_collision_cells.clone(),
        })
    }

    /// Layout cells requested by the completed terrain/scenery rebuild.
    /// Separate from collision admission; this does not prove individual
    /// layout resources parsed or NPC materialization succeeded.
    pub fn loaded_layout_cells(&self) -> &[(i32, i32)] {
        &self.built.1
    }

    /// Moves the party's world to `stage`, arriving at `at` (stage frame):
    /// drops every streamed cell and whole-stage model (`old`), then loads the
    /// new stage. Returns its collision around `at`, built now so the party
    /// stands on it at once; on stage 100 the streamer then follows the focus.
    /// The caller places the party (the stage's start position,
    /// `ddhost::install::start_positions`) and reports `StageLoaded`.
    #[allow(clippy::too_many_arguments)]
    pub fn enter(
        &mut self,
        commands: &mut Commands,
        old: impl IntoIterator<Item = Entity>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        stage: u32,
        at: [f32; 3],
    ) -> Result<ddworld::Terrain, String> {
        for e in old {
            commands.entity(e).despawn();
        }
        self.present.clear();
        self.built = (vec![], vec![]);
        self.completed_collision_cells.clear();
        self.terrain = None;
        self.ready = None;
        self.stage = stage;
        self.focus = Some(at);
        self.actors.clear();
        if stage == FIELD {
            // the stage frame's own origin cell, so viewer and stage frames
            // differ only by UNIT
            self.origin = (50, 50);
            let (collision, layouts) = self.collision_cells(at);
            let index = self.scenery_index();
            let mut t = ddworld::Terrain::load_collision_cells(self.vfs, stage, collision.iter().copied());
            self.completed_collision_cells = t.parsed_collision_cells().to_vec();
            t.add_scenery_cells(self.vfs, &index, stage, layouts.iter().copied());
            self.built = (collision, layouts);
            info!("stream: entered stage {stage} at {at:?} ({} triangles)", t.triangles());
            Ok(t)
        } else {
            let n = crate::event::spawn_stage_models(commands, &mut self.loader, meshes, materials, images, stage);
            info!("stream: entered stage {stage} at {at:?} ({n} placed objects)");
            ddhost::install::stage_collision(self.vfs, stage)
        }
    }

    fn scenery_index(&mut self) -> Arc<ddworld::SceneryIndex> {
        let vfs = self.vfs;
        self.scenery.get_or_insert_with(|| Arc::new(ddworld::Terrain::scenery_index(vfs))).clone()
    }

    fn collision_cells(&self, pos: [f32; 3]) -> Built {
        // The handoff has only its authored arrival pose until the driver
        // supplies the newly placed party on its next frame.
        let positions = if self.actors.is_empty() { std::slice::from_ref(&pos) } else { &self.actors };
        let collision = self.grid.wanted_for_players(Kind::Collision, positions, self.quality);
        let layouts = self.grid.wanted_for_players(Kind::Layout, positions, self.quality);
        (collision, layouts)
    }

    /// The viewer offset of cell `(m, n)`.
    fn offset(&self, (m, n): (i32, i32)) -> Vec3 {
        Vec3::new((n - self.origin.1) as f32 * 100.0, 0.0, (m - self.origin.0) as f32 * 100.0)
    }

    /// The stage-frame position of the viewer origin.
    fn origin_stage(&self) -> Vec3 {
        Vec3::new((self.origin.1 - 50) as f32 * 10000.0, 0.0, (self.origin.0 - 50) as f32 * 10000.0)
    }

    /// Everything that should be spawned for a focus at `pos`.
    fn desired(&self, pos: [f32; 3]) -> HashSet<StreamCell> {
        let cell = |(m, n), part| StreamCell { m, n, part };
        let models: HashSet<(i32, i32)> = self.grid.wanted(Kind::Model, pos, self.quality).into_iter().collect();
        let mut out: HashSet<StreamCell> = models.iter().map(|&c| cell(c, CellPart::Model)).collect();
        out.extend(self.grid.wanted(Kind::Layout, pos, self.quality).into_iter().map(|c| cell(c, CellPart::Layout)));
        let (m0, n0) = self.grid.name(self.grid.index(pos));
        // the low models for the focus's cell (0x00c5d7a0)
        for (slot, high) in self.loader.low_models_for((m0, n0)) {
            out.insert(cell((slot, i32::from(high)), CellPart::Low));
        }
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

/// `--play`: the followed player is the focus, and rebuilt collision goes
/// into `PlayState`.
pub fn play_hookup(streamer: Option<ResMut<Streamer>>, st: Option<ResMut<PlayState>>) {
    let (Some(mut s), Some(mut st)) = (streamer, st) else { return };
    if s.driver != Driver::Play {
        return;
    }
    s.focus = st.sim.player(st.follow).map(|p| p.position);
    s.actors = st.sim.players.iter().map(|p| p.position).collect();
    if let Some(t) = s.take_terrain() {
        st.terrain = t;
    }
}

/// Loads and unloads cells around the focus on stage 100 and rebuilds the
/// collision of the cells the original would have.
pub fn update(
    mut commands: Commands,
    streamer: Option<ResMut<Streamer>>,
    cells: Query<(Entity, &StreamCell)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let _timing = crate::perf::Section::start("stream::update");
    let Some(mut s) = streamer else { return };
    let s = &mut *s;
    if s.stage != FIELD {
        return;
    }
    let Some(pos) = s.focus else { return };

    // Collision: a finished rebuild waits for the driver.
    if let Some(task) = s.terrain.as_mut() {
        if let Some((built, terrain)) = block_on(future::poll_once(task)) {
            info!("stream: collision of {} cells + {} layouts ready ({} triangles)", built.0.len(), built.1.len(), terrain.triangles());
            s.completed_collision_cells = terrain.parsed_collision_cells().to_vec();
            s.ready = Some(terrain);
            s.built = built;
            s.terrain = None;
        }
    }
    let cells_now = s.collision_cells(pos);
    if s.terrain.is_none() && cells_now != s.built {
        let (vfs, stage) = (s.vfs, s.stage);
        let index = s.scenery_index();
        s.terrain = Some(AsyncComputeTaskPool::get().spawn(async move {
            let (collision, layouts) = &cells_now;
            let mut t = ddworld::Terrain::load_collision_cells(vfs, stage, collision.iter().copied());
            t.add_scenery_cells(vfs, &index, stage, layouts.iter().copied());
            (cells_now, t)
        }));
    }

    // Models, layouts and the distant ring.
    let desired = s.desired(pos);
    for (e, tag) in &cells {
        if !desired.contains(tag) {
            commands.entity(e).despawn();
        }
    }
    s.present.retain(|t| desired.contains(t));
    let mut missing: Vec<StreamCell> = desired.iter().filter(|t| !s.present.contains(t)).copied().collect();
    if missing.is_empty() {
        return;
    }
    // nearest first; models and layouts, then the low models, then the
    // distant ring
    let (m0, n0) = s.grid.name(s.grid.index(pos));
    let rank = |p: CellPart| match p {
        CellPart::Model | CellPart::Layout => 0,
        CellPart::Low => 1,
        CellPart::Far => 2,
    };
    missing.sort_by_key(|t| (rank(t.part), (t.m - m0).abs().max((t.n - n0).abs())));
    s.loader.lod_eye = Some((Vec3::from(pos) - s.origin_stage()) * UNIT);
    let start = Instant::now();
    for t in missing {
        if start.elapsed() > BUDGET {
            break;
        }
        let offset = s.offset((t.m, t.n));
        let origin = s.origin;
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
            CellPart::Low => {
                l.spawn_low_model(&mut commands, &mut meshes, &mut materials, &mut images, origin, t.m, t.n == 1);
            }
        }
        s.loader.tag = None;
        // a cell without data counts as present, so it is not retried
        s.present.insert(t);
    }
}

/// Everything [`Streamer::enter`] drops: streamed cells and whole-stage
/// models.
pub type WorldEntities<'w, 's> = Query<'w, 's, Entity, Or<(With<StreamCell>, With<StageEntity>)>>;

#[cfg(test)]
mod tests {
    /// Cassardis's gate door (st200_door record 0: stage 100, start position
    /// 2) lands on field ground: the collision `Streamer::enter` builds there
    /// holds the start position.
    #[test]
    fn the_cassardis_gate_arrives_on_field_ground() {
        let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !native.exists() {
            return;
        }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let grid = super::Streamer::field_grid(native).unwrap();
        let start = ddhost::install::start_positions(&vfs, super::FIELD).unwrap().entries[2].position;
        let at = [start[0], start[1], start[2]];
        let mut collision = grid.wanted(ddworld::stream::Kind::Collision, at, 1.0);
        collision.sort();
        assert!(collision.contains(&grid.name(grid.index(at))), "{collision:?}");
        let t = ddworld::Terrain::load_collision_cells(&vfs, super::FIELD, collision);
        let ground = ddsim::Ground::height(&t, at[0], at[2], at[1] + 200.0);
        let g = ground.unwrap_or_else(|| panic!("no ground at {at:?}"));
        assert!((g - at[1]).abs() < 100.0, "ground {g} vs start {at:?}");
        eprintln!("start 2 {at:?} ground {g}");
    }
}
