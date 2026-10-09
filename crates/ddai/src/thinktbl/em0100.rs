//! uEm0100 (goblin family): the unit side of the think.
//!
//! * [`action_classes`]: the unit's action table (0x018335d0, 0x44-byte
//!   records, class DTI at +0x28), which turns a think result into an
//!   action class (0 `cEm0100ActWait`, 13 `cEm0100ActDashTrace`, 101
//!   `cEm0100ActNAttack1`, ...).
//! * [`check`]: the part of the think-check callback 0x008ea7e0 (ids below
//!   1000) that reads only the unit's current action.
//! * [`map_result`]: 0x008d0fb0, the unit's rewrite of a think result.
//! * [`Driver`]: the think part of the unit's per-frame think 0x008d2360.

use super::mgr::{Host, ThinkMgr};
use super::table::Tables;
use crate::Error;

/// The default table uEm0100 installs at creation (0x008cefa0): built-in
/// table 0 (and 2).
pub const DEFAULT_TABLE: u32 = 0x017e_0858;
/// uEm0100's action records: base, stride, count and the DTI offset.
pub const ACTIONS_VA: u32 = 0x0183_35d0;
pub const ACTION_STRIDE: u32 = 0x44;
pub const ACTION_COUNT: u32 = 237;
const ACTION_DTI: u32 = 0x28;

/// Action id -> class name, read from the exe's action records and the
/// DTI registrations (`ddexe::Exe::class_names`).
pub fn action_classes(exe: &ddexe::Exe<'_>) -> Result<Vec<String>, Error> {
    let names = exe.class_names();
    (0..ACTION_COUNT)
        .map(|i| {
            let dti = exe
                .u32(ACTIONS_VA + i * ACTION_STRIDE + ACTION_DTI)
                .map_err(|e| Error::Data(e.0))?;
            names
                .get(&dti)
                .cloned()
                .ok_or_else(|| Error::Data(format!("goblin action {i}: no class for DTI {dti:#010x}")))
        })
        .collect()
}

/// 0x008ea7e0 for the ids that depend only on the current action (unit
/// +0x2dd4): `None` for every other id (they read the world; the host
/// answers them).
///
/// | id | true when the current action is |
/// |----|---------------------------------|
/// | 0 | `arg` |
/// | 2 | 53 (`Sit`) or 56..=62 (`SitDirection`) |
/// | 3 | 63 (`Lie`) or 66, 67 (`LieDirection`) |
/// | 4 | 5, 6 (`Walk`, `WalkTrace`) |
/// | 5 | 8, 9, 11, 230 (`Run`, `RunTrace`, `RunStumble`, `Run`) |
/// | 6 | 12, 13, 14, 224 (`Dash`, `DashTrace` x3) |
/// | 12 | 43 (`GuardHit`) or 192 (`CSldGrdHit`) |
// Translated from 0x008ea7e0 (DDDA.exe 2017-12-13 Steam).
pub fn check(id: u32, arg: u32, action: i32) -> Option<bool> {
    Some(match id {
        0 => action == arg as i32,
        2 => action == 0x35 || (0x38..0x3f).contains(&action),
        3 => action == 0x3f || (0x42..0x44).contains(&action),
        4 => (action as u32).wrapping_sub(5) < 2,
        5 => matches!(action, 8 | 9 | 0xb | 0xe6),
        6 => (0xc..0xf).contains(&action) || action == 0xe0,
        0xc => action == 0x2b || action == 0xc0,
        _ => return None,
    })
}

/// What 0x008d0fb0 reads from the unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MapState {
    /// Unit vf108 (0x008d1240): 0 when vf117 (unit +0x5bd0) or +0x739c bit
    /// 0 is set, 1 for +0x739c bit 1, 2 for bit 2, else 0.
    pub mode: u8,
    /// Unit +0x609f.
    pub f609f: bool,
    /// Unit +0x6034.
    pub s6034: i32,
    /// Unit +0x615d, think flag 9 (unit +0x607c == 1, 0x008d262c).
    pub f615d: bool,
}

/// 0x008d0fb0: the unit's rewrite of a think result. With `mode` 2 the
/// turns and waits move to their alternate ids (18..21 -> 26..29, 0 -> 1,
/// 30 -> 31); with +0x615d set they move to the 166.. range
/// (`cEm0100ActTorch*`, `Walk` 173, ...).
// Translated from 0x008d0fb0 (DDDA.exe 2017-12-13 Steam).
pub fn map_result(r: i32, s: MapState) -> i32 {
    let mut r = r;
    if s.f609f && s.mode == 2 && r == 0 {
        r = 0xbb;
    }
    if s.s6034 == 1 {
        if r == 0 {
            r = 0x4f;
        }
    } else if s.s6034 == 2 && r == 0 {
        r = 3;
    }
    if s.mode == 1 {
        if r == 0 {
            r = 0xe8;
        }
    } else if s.mode == 2 {
        r = match r {
            0 => 1,
            0x12 => 0x1a,
            0x13 => 0x1b,
            0x14 => 0x1c,
            0x15 => 0x1d,
            0x1e => 0x1f,
            r => r,
        };
    }
    if s.f615d {
        return match r {
            0 => 0xa6,
            1 => 0xa8,
            5 => 0xad,
            6 => 0xae,
            7 => 0xaf,
            8 => 0xb0,
            9 => 0xb1,
            10 => 0xb2,
            0x12 => 0xb3,
            0x13 => 0xb4,
            0x14 => 0xb5,
            0x15 => 0xb6,
            0x1a => 0xb7,
            0x1b => 0xb8,
            0x1c => 0xb9,
            0x1d => 0xba,
            0x48 => 0xa9,
            r => r,
        };
    }
    r
}

/// The think part of uEm0100's think (0x008d2360): what runs when no
/// forced action is pending and the current action allows a think (its
/// record +0x50, or unit +0x7360).
#[derive(Debug, Clone)]
pub struct Driver {
    pub mgr: ThinkMgr,
    /// Unit +0x2dd4: the current action.
    pub action: i32,
    /// What the result rewrite reads. (Unit +0x6ff5, think +0xea5, makes
    /// the driver request even an unchanged action.)
    pub map: MapState,
}

impl Driver {
    /// A fresh unit: think reset (0x008b73f0), default and base table
    /// 0x017e0858 (0x008cefa0..0x008cefd3), range scale `scale` (unit
    /// +0x5ae4 into think +0x44, applied when above 0 and not 1, 0x008cef2e).
    // Translated from 0x008cef2e..0x008cefd3 (DDDA.exe 2017-12-13 Steam).
    pub fn new(scale: f32) -> Self {
        let mut mgr = ThinkMgr::new();
        mgr.scale_on = scale > 0.0 && scale != 1.0;
        mgr.scale = if mgr.scale_on { scale } else { 1.0 };
        mgr.table = DEFAULT_TABLE;
        mgr.root = DEFAULT_TABLE;
        mgr.default_table = DEFAULT_TABLE;
        mgr.base_table = DEFAULT_TABLE;
        Driver {
            mgr,
            action: 0,
            map: MapState::default(),
        }
    }

    /// One think: reload the default table (0x008b80a0), run the table
    /// (0x008b81a0), rewrite the result (0x008d0fb0) and return the action
    /// to request (uEm0100 vf132 0x008d11e0), if any.
    // Translated from 0x008d2360 (think part) (DDDA.exe 2017-12-13 Steam).
    pub fn think(&mut self, tables: &Tables, host: &mut dyn Host) -> Result<Option<i32>, Error> {
        self.mgr.reload_default();
        let r = self.mgr.think(tables, host)?;
        let r = map_result(r, self.map);
        if r != -1 && (r != self.action || self.mgr.ea5) {
            self.action = r;
            return Ok(Some(r));
        }
        Ok(None)
    }
}
