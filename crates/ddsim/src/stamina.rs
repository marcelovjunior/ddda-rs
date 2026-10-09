//! Stamina: the per-frame recovery and the sprint's drain, and the motion
//! speed the walk, run and sprint rows give the player's own time.
//! Evidence: docs/STAMINA.md, docs/PARAMS.md, docs/ACCEPTANCE.md.
//!
//! Per player (host-owned): the current and maximum stamina and the body
//! and load classes that pick table rows. The classes come from the body
//! and carried weights in the original (`0x00B81120`, `0x00B81270`), which
//! the simulation does not model yet; they default to 1, the row whose
//! values the acceptance recordings show (sprint -1.5 and recovery +1.625
//! per frame, motion speed 1.15).

/// One row of an `rPlStamina` table (`cPlStamina`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaminaRow {
    pub stamina: f32,
    pub frame: f32,
    pub motion_speed: f32,
}

const fn row(stamina: f32, frame: f32, motion_speed: f32) -> StaminaRow {
    StaminaRow {
        stamina,
        frame,
        motion_speed,
    }
}

/// The stamina tables the simulation uses, rows indexed by class.
#[derive(Debug, Clone, PartialEq)]
pub struct StaminaTables {
    /// `param\pl\stamina\plstaminawalk`, `plstaminarun`, `plstaminadash`:
    /// rows by load class.
    pub walk: Vec<StaminaRow>,
    pub run: Vec<StaminaRow>,
    pub dash: Vec<StaminaRow>,
    /// `plstaminarecovernormal`: rows 0-4 by body class, 5-9 by load class.
    pub recover_normal: Vec<StaminaRow>,
}

impl StaminaTables {
    /// The tables as shipped (docs/PARAMS.md, rPlStamina).
    pub fn shipped() -> Self {
        let speeds = |s: [f32; 6]| s.iter().map(|&m| row(0.0, 1.0, m)).collect::<Vec<_>>();
        StaminaTables {
            walk: speeds([1.09, 1.06, 1.03, 1.0, 0.97, 0.94]),
            run: speeds([1.2, 1.15, 1.1, 1.0, 0.9, 0.8]),
            dash: [
                (1.25, 1.2),
                (1.5, 1.15),
                (1.75, 1.1),
                (2.0, 1.0),
                (2.5, 0.9),
                (3.0, 0.8),
            ]
            .iter()
            .map(|&(s, m)| row(s, 1.0, m))
            .collect(),
            recover_normal: [
                (1.812, 1.0),
                (1.625, 1.0),
                (1.438, 1.0),
                (1.25, 1.0),
                (1.062, 1.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.0),
                (0.0, 0.2),
                (0.0, 0.4),
            ]
            .iter()
            .map(|&(s, f)| row(s, f, 1.0))
            .collect(),
        }
    }

    fn pick(rows: &[StaminaRow], i: usize) -> Option<StaminaRow> {
        rows.get(i.min(rows.len().saturating_sub(1))).copied()
    }

    /// Normal recovery per frame:
    /// `RecoverNormal[body].mStamina / (RecoverNormal[4 + load].mFrame +
    /// RecoverNormal[body].mFrame)` (the actor factor +0x2B44 is 1 here).
    // Translated from 0x00B81810 (DDDA.exe 2017-12-13 Steam): the row
    // pointers are read at [table + body*4] and [table + load*4 + 0x10].
    pub fn recovery(&self, body: usize, load: usize) -> f32 {
        let (Some(b), Some(l)) = (self.recover_normal.get(body), self.recover_normal.get(4 + load)) else {
            return 0.0;
        };
        let d = l.frame + b.frame;
        if d == 0.0 {
            0.0
        } else {
            b.stamina / d
        }
    }

    /// The sprint's drain per frame: `-(mStamina x 1 / mFrame)` of the dash
    /// row (no ability multiplier).
    // Translated from 0x00B81920 (DDDA.exe 2017-12-13 Steam), actions 5 / 0x7D.
    pub fn dash_drain(&self, load: usize) -> f32 {
        match Self::pick(&self.dash, load) {
            Some(r) if r.frame != 0.0 => -(r.stamina / r.frame),
            _ => 0.0,
        }
    }
}

/// The recovery-suppressing motion event: group 1, bit 29. `0x00B81810`
/// tests the motion layers' event words (uPlayer +0x494 / +0x5D4 / +0x854,
/// group 1 of each layer) against mask 0x20000000. The sprint start (motion
/// 43) and sprint (20) carry it throughout, the sprint stop (21) for frames
/// 0-17: the recorded stop holds stamina until motion frame 18-19.
pub const RECOVERY_SUPPRESS_GROUP: usize = 1;
pub const RECOVERY_SUPPRESS_BIT: u32 = 0x2000_0000;

/// One stamina change: recovery (if not suppressed), then the drain, each
/// times `dt` frames of game time (sMain's delta x sWorkRate rates 0 and 1,
/// both 1 here), each clamped to [0, max] after adding. Returns the new
/// value and whether it newly ran out (previous > 0, new <= 0).
// Translated from 0x00B81450 (DDDA.exe 2017-12-13 Steam), with the adding
// and clamping of 0x00B81E30; status modifiers, recovery debt and the tired
// action (cPlActTired) are not modelled (docs/STAMINA.md).
pub fn step(stamina: f32, max: f32, recovery: f32, drain: f32, dt: f32) -> (f32, bool) {
    let add = |s: f32, d: f32| (s + d).min(max).max(0.0);
    let mut s = stamina;
    let mut out = false;
    for d in [recovery * dt, drain * dt] {
        if d == 0.0 {
            continue;
        }
        let n = add(s, d);
        out |= s > 0.0 && n <= 0.0;
        s = n;
    }
    (s, out)
}

/// The weight tables (`rPlWeight`, docs/PARAMS.md): body-weight class
/// boundaries and, per body class, the carried-weight boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightTables {
    /// `param\pl\weight\pleditweight`: 5 rows (24, 50, 70, 90, 110).
    pub edit: Vec<f32>,
    /// `pltotalweightss` .. `pltotalweightll`, indexed by body class: 6 rows.
    pub total: Vec<Vec<f32>>,
}

impl WeightTables {
    /// The tables as shipped (docs/PARAMS.md, rPlWeight).
    pub fn shipped() -> Self {
        WeightTables {
            edit: vec![24.0, 50.0, 70.0, 90.0, 110.0],
            total: vec![
                vec![0.0, 34.0, 44.0, 54.0, 59.0, 64.0],
                vec![0.0, 60.0, 80.0, 90.0, 95.0, 100.0],
                vec![0.0, 85.0, 95.0, 115.0, 125.0, 135.0],
                vec![0.0, 110.0, 130.0, 145.0, 155.0, 165.0],
                vec![0.0, 130.0, 155.0, 180.0, 195.0, 210.0],
            ],
        }
    }

    /// Body class 0..4 (SS..LL) from the body weight: a boundary row's
    /// weight belongs to the higher class; M (2) without a table.
    // Translated from 0x00B81120 (DDDA.exe 2017-12-13 Steam)
    pub fn body_class(&self, body_weight: f32) -> usize {
        let e = &self.edit;
        if e.len() < 5 {
            return 2;
        }
        if e[4] <= body_weight {
            4
        } else if e[3] <= body_weight {
            3
        } else if body_weight < e[2] {
            if body_weight < e[1] {
                0
            } else {
                1
            }
        } else {
            2
        }
    }

    /// Load class 1..5 from the total (body plus carried) weight: the body
    /// class's carried-weight rows 1..4, each raised by `capacity` (the
    /// ability adjustment of 0x00B7E710, 0 without abilities) and by the
    /// body's weight above its class's boundary; 3 without a table.
    // Translated from 0x00B81270 (DDDA.exe 2017-12-13 Steam)
    pub fn load_class(&self, body_weight: f32, total_weight: f32, capacity: f32) -> usize {
        let body = self.body_class(body_weight);
        let (Some(t), Some(&edit)) = (self.total.get(body), self.edit.get(body)) else {
            return 3;
        };
        if t.len() < 5 {
            return 3;
        }
        let over = body_weight - edit;
        let at = |k: usize| t[k] + capacity + over;
        if at(4) <= total_weight {
            5
        } else if at(3) <= total_weight {
            4
        } else if at(2) <= total_weight {
            3
        } else if total_weight < at(1) {
            1
        } else {
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_classes_from_the_shipped_tables() {
        let w = WeightTables::shipped();
        // boundaries 50, 70, 90, 110 belong to the higher class
        assert_eq!(w.body_class(49.9), 0);
        assert_eq!(w.body_class(50.0), 1);
        assert_eq!(w.body_class(69.0), 1);
        assert_eq!(w.body_class(70.0), 2);
        assert_eq!(w.body_class(76.0), 2);
        assert_eq!(w.body_class(110.0), 4);
        // body 60 (class S, 10 over its boundary): carried rows 60, 80, 90, 95 + 10
        assert_eq!(w.load_class(60.0, 69.9, 0.0), 1);
        assert_eq!(w.load_class(60.0, 70.0, 0.0), 2);
        assert_eq!(w.load_class(60.0, 90.0, 0.0), 3);
        assert_eq!(w.load_class(60.0, 100.0, 0.0), 4);
        assert_eq!(w.load_class(60.0, 105.0, 0.0), 5);
        // an ability's capacity raises every boundary
        assert_eq!(w.load_class(60.0, 75.0, 10.0), 1);
    }

    #[test]
    fn shipped_rows_give_the_recorded_rates() {
        let t = StaminaTables::shipped();
        // recorded: sprint -45.000/s, idle recovery +48.750/s (30 frames/s)
        assert_eq!(t.dash_drain(1) * 30.0, -45.0);
        assert!((t.recovery(1, 1) * 30.0 - 48.75).abs() < 1e-4);
        assert_eq!(t.run[1].motion_speed, 1.15);
        assert_eq!(t.dash[1].motion_speed, 1.15);
        // a heavier load slows recovery (row 4 + 5: frame 0.4)
        assert!((t.recovery(1, 5) - 1.625 / 1.4).abs() < 1e-6);
    }

    #[test]
    fn step_clamps_and_reports_running_out() {
        assert_eq!(step(100.0, 640.0, 1.625, 0.0, 2.0), (103.25, false));
        assert_eq!(step(639.0, 640.0, 1.625, 0.0, 2.0), (640.0, false));
        assert_eq!(step(1.0, 640.0, 0.0, -1.5, 1.0), (0.0, true));
        // already empty: no new crossing
        assert_eq!(step(0.0, 640.0, 0.0, -1.5, 1.0), (0.0, false));
    }
}
