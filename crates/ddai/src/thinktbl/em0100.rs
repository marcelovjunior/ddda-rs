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

/// uEm0100 vf108 (0x008d1240): 0 when vf117 (unit +0x5bd0) is set or
/// think state bit 0 (+0x124c, unit +0x739c) is set, 1 for state bit 1, 2
/// for state bit 2, else 0. Rows set these bits (combat table 1 row 12 sets
/// bit 2 and clears bits 0 and 1), so the mode follows the tables.
// Translated from 0x008d1240 (DDDA.exe 2017-12-13 Steam).
pub fn mode(state: u32, vf117: bool) -> u8 {
    if vf117 || state & 1 != 0 {
        0
    } else if state & 2 != 0 {
        1
    } else {
        ((state & 0xff) >> 1 & 2) as u8
    }
}

/// 0x008ea7e0 ids 10 and 11: chances on the game's rand (`roll` is the
/// rand value; the code takes it modulo 100). In mode 2 the odds depend on
/// unit +0x6034 (id 10: 40 / 50 / 30 percent for 0 / 1 / 2, id 11: 50 / 60
/// / 40), otherwise both are 10 percent.
// Translated from 0x008ea7e0 cases 10 and 11 (DDDA.exe 2017-12-13 Steam).
pub fn chance(id: u32, mode: u8, s6034: i32, roll: u32) -> Option<bool> {
    let r = roll % 100;
    if id != 10 && id != 11 {
        return None;
    }
    if mode != 2 {
        return Some(r < 10);
    }
    Some(match (id, s6034) {
        (10, 0) | (11, 2) => r <= 0x27,
        (10, 1) | (11, 0) => r <= 0x31,
        (10, 2) => r <= 0x1d,
        (11, 1) => r <= 0x3b,
        _ => false,
    })
}

/// What 0x008d0fb0 reads from the unit besides the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MapState {
    /// Unit vf117 (+0x5bd0), which forces mode 0.
    pub vf117: bool,
    /// Unit +0x609f.
    pub f609f: bool,
    /// Unit +0x6034.
    pub s6034: i32,
    /// Unit +0x615d, think flag 9 (unit +0x607c == 1, 0x008d262c).
    pub f615d: bool,
}

/// 0x008d0fb0: the unit's rewrite of a think result. With [`mode`] 2 the
/// turns and waits move to their alternate ids (18..21 -> 26..29, 0 -> 1,
/// 30 -> 31); with +0x615d set they move to the 166.. range
/// (`cEm0100ActTorch*`, `Walk` 173, ...).
// Translated from 0x008d0fb0 (DDDA.exe 2017-12-13 Steam).
pub fn map_result(r: i32, mode: u8, s: MapState) -> i32 {
    let mut r = r;
    if s.f609f && mode == 2 && r == 0 {
        r = 0xbb;
    }
    if s.s6034 == 1 {
        if r == 0 {
            r = 0x4f;
        }
    } else if s.s6034 == 2 && r == 0 {
        r = 3;
    }
    if mode == 1 {
        if r == 0 {
            r = 0xe8;
        }
    } else if mode == 2 {
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
        let r = map_result(r, mode(self.mgr.state, self.map.vf117), self.map);
        if r != -1 && (r != self.action || self.mgr.ea5) {
            self.action = r;
            return Ok(Some(r));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_from_state_bits() {
        assert_eq!(mode(0, false), 0);
        assert_eq!(mode(4, false), 2);
        assert_eq!(mode(2 | 4, false), 1);
        assert_eq!(mode(1 | 4, false), 0);
        assert_eq!(mode(4, true), 0);
    }

    #[test]
    fn result_rewrite() {
        let plain = MapState::default();
        assert_eq!(map_result(18, 0, plain), 18);
        assert_eq!(map_result(18, 2, plain), 26);
        assert_eq!(map_result(0, 2, plain), 1);
        assert_eq!(map_result(0, 1, plain), 0xe8);
        assert_eq!(map_result(-1, 2, plain), -1);
        assert_eq!(map_result(0, 0, MapState { s6034: 1, ..plain }), 0x4f);
        assert_eq!(map_result(0, 2, MapState { f609f: true, ..plain }), 0xbb);
        let torch = MapState { f615d: true, ..plain };
        assert_eq!(map_result(0, 0, torch), 0xa6);
        assert_eq!(map_result(18, 2, torch), 0xb7);
        assert_eq!(map_result(101, 2, torch), 101);
    }

    #[test]
    fn chance_odds() {
        let hits = |id, mode, s| (0..100).filter(|&r| chance(id, mode, s, r).unwrap()).count();
        assert_eq!((hits(10, 0, 0), hits(11, 1, 2)), (10, 10));
        assert_eq!(
            (hits(10, 2, 0), hits(10, 2, 1), hits(10, 2, 2), hits(10, 2, 3)),
            (40, 50, 30, 0)
        );
        assert_eq!((hits(11, 2, 0), hits(11, 2, 1), hits(11, 2, 2)), (50, 60, 40));
        assert_eq!(chance(9, 2, 0, 0), None);
    }

    #[test]
    fn action_checks() {
        assert_eq!(check(4, 0, 6), Some(true));
        assert_eq!(check(4, 0, 7), Some(false));
        assert_eq!(check(5, 0, 230), Some(true));
        assert_eq!(check(6, 0, 224), Some(true));
        assert_eq!(check(0, 13, 13), Some(true));
        assert_eq!(check(2, 0, 62), Some(true));
        assert_eq!(check(3, 0, 64), Some(false));
        assert_eq!(check(9, 0, 0), None);
    }
}
