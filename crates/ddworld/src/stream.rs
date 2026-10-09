//! Which stage cells the original keeps around the player: the candidate
//! window per kind of cell data and the additional distance predicate
//! (docs/STREAMING.md). Pure rules; loading is the caller's.
//!
//! Each kind has its own decision: terrain models, collision and layouts
//! (scenery placements) are separate, and a cell is wanted only while it is
//! both inside the kind's window around the player's grid index and passes
//! the kind's distance test. The same test keeps a cell (no unload
//! hysteresis was found in the model and collision paths).

/// The kinds of cell data `uStageSplitCtrl::vf08` (0x00c5ca60) streams with
/// their own windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// Terrain models (`0x00c5f070`).
    Model,
    /// Collision (`0x00c5f680`).
    Collision,
    /// Layouts: placed scenery (`0x00c60510`).
    Layout,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Model, Kind::Collision, Kind::Layout];

    /// Window width in cells: the stage fields +0x7f0 / +0x7f8 / +0x800
    /// (X; Z alike) that the base stage constructor 0x004fdf60 sets to 5, 5
    /// and 3. (Runtime overrides, if any, are not traced.)
    pub fn width(self) -> i32 {
        match self {
            Kind::Model | Kind::Collision => 5,
            Kind::Layout => 3,
        }
    }

    /// The predicate's divisor: 2.5 (0x017f41e8), 3.3 and 2.4, selected by
    /// the hidden EDI argument of 0x005075d0 (0, 1, 2).
    fn divisor(self) -> f32 {
        match self {
            Kind::Model => 2.5,
            Kind::Collision => 3.3,
            Kind::Layout => 2.4,
        }
    }
}

/// A stage's grid descriptor (`aStage100::DTI_newInstance` 0x0051b7f0
/// stores 0x0153023c at stage +0x830): X/Z origins, cell counts, cell sizes
/// and the filename number offsets. Read from the installed exe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub origin: [f32; 2],
    pub count: [i32; 2],
    pub cell: [f32; 2],
    /// Added to the zero-based X / Z index to make the filename's N / M.
    pub name_offset: [i32; 2],
}

/// Stage 100's grid descriptor in the 2017-12-13 Steam exe.
pub const STAGE100_GRID: u32 = 0x0153_023C;

impl Grid {
    /// Reads a descriptor: +0/+4 origins, +8/+12 counts, +16/+20 cell
    /// sizes, +24/+28 filename offsets.
    pub fn from_exe(exe: &ddexe::Exe, va: u32) -> Result<Grid, String> {
        let e = |r: Result<f32, ddexe::Error>| r.map_err(|e| e.0);
        let i = |r: Result<i32, ddexe::Error>| r.map_err(|e| e.0);
        let g = Grid {
            origin: [e(exe.f32(va))?, e(exe.f32(va + 4))?],
            count: [i(exe.i32(va + 8))?, i(exe.i32(va + 12))?],
            cell: [e(exe.f32(va + 16))?, e(exe.f32(va + 20))?],
            name_offset: [i(exe.i32(va + 24))?, i(exe.i32(va + 28))?],
        };
        if g.count[0] <= 0 || g.count[1] <= 0 || g.cell[0] <= 0.0 || g.cell[1] <= 0.0 {
            return Err(format!("grid at {va:#x}: {g:?}"));
        }
        Ok(g)
    }

    /// The zero-based grid index of a stage-frame position (cm).
    pub fn index(&self, pos: [f32; 3]) -> [i32; 2] {
        [
            ((pos[0] - self.origin[0]) / self.cell[0]).floor() as i32,
            ((pos[2] - self.origin[1]) / self.cell[1]).floor() as i32,
        ]
    }

    /// The cell's filename numbers `(M, N)` (as in `st100_<M>m<N>n`) of a
    /// zero-based index.
    pub fn name(&self, index: [i32; 2]) -> (i32, i32) {
        (index[1] + self.name_offset[1], index[0] + self.name_offset[0])
    }

    /// The centre (stage frame, X and Z) of the cell named `(m, n)`.
    pub fn centre(&self, (m, n): (i32, i32)) -> [f32; 2] {
        [
            (n - self.name_offset[0]) as f32 * self.cell[0] + self.origin[0] + self.cell[0] * 0.5,
            (m - self.name_offset[1]) as f32 * self.cell[1] + self.origin[1] + self.cell[1] * 0.5,
        ]
    }

    /// The candidate window of `kind` around the player's index: inclusive,
    /// clipped to the grid, `index - width/2 ..= index + width/2` with signed
    /// integer division (0x00507440).
    pub fn window(&self, kind: Kind, index: [i32; 2]) -> impl Iterator<Item = (i32, i32)> + '_ {
        let h = kind.width() / 2;
        let lo = |i: i32| (i - h).max(0);
        let hi = |i: i32, n: i32| (i + h).min(n - 1);
        let (x0, x1) = (lo(index[0]), hi(index[0], self.count[0]));
        let (z0, z1) = (lo(index[1]), hi(index[1], self.count[1]));
        (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| self.name([x, z])))
    }

    /// The additional distance test of `kind` for the cell `(m, n)` and the
    /// player at `pos` (stage frame): horizontal distance to the cell centre
    /// strictly below `quality x width / divisor x cell` plus
    /// `|sin(2 acos t)| x (sqrt 2 - 1) x cell`, where `t` is the Z component
    /// of the normalized difference of the normalized player and cell-centre
    /// directions from the stage frame's origin.
    // Translated from 0x005075d0 (DDDA.exe 2017-12-13 Steam). The night
    // divisors next to these are unreachable (hour >= 21 and <= 3 at once).
    pub fn near(&self, kind: Kind, cell: (i32, i32), pos: [f32; 3], quality: f32) -> bool {
        let c = self.centre(cell);
        let norm = |v: [f32; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if l >= f32::EPSILON {
                [v[0] / l, v[1] / l, v[2] / l]
            } else {
                v
            }
        };
        let p = norm([pos[0], 0.0, pos[2]]);
        let q = norm([c[0], 0.0, c[1]]);
        let d = norm([p[0] - q[0], p[1] - q[1], p[2] - q[2]]);
        let t = d[2].clamp(-1.0, 1.0);
        let angular = (2.0 * t.acos()).sin().abs() * (self.cell[0] * std::f32::consts::SQRT_2 - self.cell[0]);
        let base = quality * (kind.width() as f32 / kind.divisor()) * self.cell[0];
        let dist = ((pos[2] - c[1]).powi(2) + (pos[0] - c[0]).powi(2)).sqrt();
        dist < angular + base
    }

    /// The cells of `kind` the original wants for the player at `pos`.
    pub fn wanted(&self, kind: Kind, pos: [f32; 3], quality: f32) -> Vec<(i32, i32)> {
        let index = self.index(pos);
        self.window(kind, index)
            .filter(|&c| self.near(kind, c, pos, quality))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stage 100's values as documented (docs/STREAMING.md), for the rule
    /// tests; the runtime reads them from the exe.
    const G: Grid = Grid {
        origin: [-200000.0, -100000.0],
        count: [40, 33],
        cell: [10000.0, 10000.0],
        name_offset: [30, 40],
    };

    #[test]
    fn names_and_centres_use_the_stage_frame_of_cell_50m50n() {
        // cell 58m50n holds z 80000..90000, x 0..10000
        let pos = [215.9, 41054.3, 83921.1];
        assert_eq!(G.name(G.index(pos)), (58, 50));
        assert_eq!(G.centre((58, 50)), [5000.0, 85000.0]);
    }

    #[test]
    fn windows_are_5_5_3_and_clip_at_the_grid_edge() {
        let mid = G.index([5000.0, 0.0, 85000.0]);
        assert_eq!(G.window(Kind::Model, mid).count(), 25);
        assert_eq!(G.window(Kind::Layout, mid).count(), 9);
        // index 0 on both axes: only the upper half of the window
        assert_eq!(G.window(Kind::Collision, [0, 0]).count(), 9);
    }

    #[test]
    fn the_distance_test_trims_the_window_corners() {
        // in the middle of a cell, the 5x5 model window's corners (about
        // 28,000 away) fail the 20,000 + angular test; the layout window's
        // neighbours (10,000) pass 12,500
        let pos = [5000.0, 0.0, 85000.0];
        let models = G.wanted(Kind::Model, pos, 1.0);
        assert!(models.len() < 25 && models.len() > 9, "{}", models.len());
        assert!(models.contains(&(58, 50)) && !models.contains(&(60, 52)));
        let layouts = G.wanted(Kind::Layout, pos, 1.0);
        assert!(layouts.contains(&(58, 51)) && layouts.contains(&(59, 50)));
        // equality rejects: a cell exactly at the base distance fails
        let far = G.wanted(Kind::Collision, pos, 1.0);
        assert!(far.iter().all(|&c| {
            let k = G.centre(c);
            ((k[0] - pos[0]).powi(2) + (k[1] - pos[2]).powi(2)).sqrt() < 15151.6 + 4143.0
        }));
    }
}
