//! Level, experience, discipline and vocation-rank rules.
//!
//! Inputs are the `ddparam` tables: `rPlLevel` (one per vocation,
//! `param\pl\level\Lv<Vocation>`), `rPlExp` (`param\pl\level\exp`) and
//! `rPlJobLevel` (`param\pl\JobLevel\JobLv<Vocation>`). Field offsets were
//! checked against the record constructors and the property registration of
//! `cPlLevel` (0x00cd6610), `cPlExp` (0x00cd5ed0: +4 `mRate` f32, +8 `mLv` u32)
//! and `cPlJobLevel` (0x00cd60b0).
//!
//! Character state lives in `cPlayerInfo` (one per party member: the Arisen
//! at sGameSys+0xa76d0, the main pawn at +0xa7ec0); the struct fields named
//! here follow its reflected MtDTI names.

use ddparam::{ExpEntry, JobLevelEntry, LevelEntry};

/// Highest character level (0x00b5bd20 clamps to 200).
pub const MAX_LEVEL: u32 = 200;
/// Highest vocation rank (0x00b5c230 clamps to 9).
pub const MAX_RANK: u32 = 9;
/// `mHpMax` cap applied on level-up (0x00b5bd20 stores 9999.0).
pub const HP_CAP: f32 = 9999.0;
/// `cumulative_exp` result for level 200 and above: a constant in 0x00b5c150.
pub const TOTAL_EXP_AT_MAX: u32 = 0x022f_0778;
/// `mExp` ceiling (0x00782d10: values >= 1e9 are set to 1e9).
pub const EXP_CAP: u32 = 1_000_000_000;
/// `mJobPoint` (discipline) ceiling (0x00782d10).
pub const DISCIPLINE_CAP: i32 = 999_999;

/// x87 `FISTP` under a control word with RC = 0b11 (truncate), as used by
/// 0x00b5c080 and 0x00b5c150. The FPU keeps at least double precision here:
/// the running game returns 485 for level 2 (claude-4 oracle, board #722),
/// which f32 arithmetic would round up to 486. The operands (an f32 rate
/// times an integer below 2^8) are exact in f64, so f64 matches.
fn trunc_u32(v: f64) -> u32 {
    v as u32
}

/// Experience needed to go from `level` to `level + 1` (`mNextExp`).
///
/// Translated from 0x00b5c080 (DDDA.exe 2017-12-13 Steam). Row 0 `mLv` is the
/// requirement at level 1; row 1 `mLv` is the base unit; from level 2 each
/// step adds `rate(u) * unit`, where `rate(u)` is the `mRate` of the last row
/// k >= 2 whose `mLv <= u`. Compared live for levels 1-199 (see the test).
pub fn next_exp(level: u32, rows: &[ExpEntry]) -> u32 {
    if level == 1 {
        return rows[0].level;
    }
    if level < 2 || rows.len() < 4 {
        return 0;
    }
    let unit = f64::from(rows[1].level);
    let mut acc = rows[0].level;
    let (mut cur, mut next) = (2usize, 3usize);
    for u in 2..=level {
        if next < rows.len() && rows[next].level <= u {
            cur = next;
            next += 1;
        }
        acc = trunc_u32(f64::from(acc) + f64::from(rows[cur].rate) * unit);
    }
    acc
}

/// Total experience from level 1 to reach `level`.
///
/// Translated from 0x00b5c150 (cdecl): the sum of `next_exp(1..level)`, with
/// level 1 giving 0 and levels above 199 returning the constant
/// `TOTAL_EXP_AT_MAX`. That constant (36,636,536) is 786 more than the sum of
/// the curve (36,635,750); the exe uses it as is.
pub fn cumulative_exp(level: u32, rows: &[ExpEntry]) -> u32 {
    if level > 199 {
        return TOTAL_EXP_AT_MAX;
    }
    (1..level).map(|l| next_exp(l, rows)).sum()
}

/// The `rPlLevel` row that applies to a character reaching `level`.
///
/// Translated from the search loops in 0x00b5bd20 and 0x00b5cbf0: rows from 1
/// upward; the row `i` with `rows[i].level <= level < rows[i + 1].level`, or
/// the last row once `i` reaches `len - 1`.
pub fn level_row(level: u32, rows: &[LevelEntry]) -> &LevelEntry {
    let mut i = 1;
    loop {
        let (cur, next) = (&rows[i], &rows[i + 1]);
        if u32::from(cur.level) <= level && level < u32::from(next.level) {
            return cur;
        }
        i += 1;
        if i >= rows.len() - 1 {
            return &rows[i];
        }
    }
}

/// The level-dependent stats in `cPlayerInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stats {
    /// `mHpMax` (+0x2a0)
    pub hp_max: f32,
    /// `mStaminaLv` (+0x2b0): stamina gained from levels.
    pub stamina_lv: f32,
    /// `mBasicAttack` (+0x2b4), Strength
    pub attack: f32,
    /// `mBasicDefend` (+0x2b8), Defense
    pub defend: f32,
    /// `mBasicMgcAttack` (+0x2bc), Magick
    pub magic_attack: f32,
    /// `mBasicMgcDefend` (+0x2c0), Magick Defense
    pub magic_defend: f32,
}

impl Stats {
    /// Level-1 stats: row 1 of the vocation's `rPlLevel` (0x00b5ccb0).
    pub fn initial(rows: &[LevelEntry]) -> Self {
        let r = &rows[1];
        Self {
            hp_max: r.hp_max.into(),
            stamina_lv: r.stamina_max.into(),
            attack: r.attack.into(),
            defend: r.defend.into(),
            magic_attack: r.magic_attack.into(),
            magic_defend: r.magic_defend.into(),
        }
    }

    /// Adds one level's growth for the vocation held at that moment
    /// (0x00b5bd20). HP is capped at `HP_CAP`; the other stats are not.
    pub fn grow(&mut self, row: &LevelEntry) {
        self.hp_max += f32::from(row.hp_max);
        if self.hp_max >= HP_CAP {
            self.hp_max = HP_CAP;
        }
        self.attack += f32::from(row.attack);
        self.defend += f32::from(row.defend);
        self.magic_attack += f32::from(row.magic_attack);
        self.magic_defend += f32::from(row.magic_defend);
        self.stamina_lv += f32::from(row.stamina_max);
    }
}

/// Level and experience of one character.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Progress {
    /// `mLevel` (+0x700)
    pub level: u32,
    /// `mExp` (+0x2c4): experience toward the next level.
    pub exp: u32,
    /// `mNextExp` (+0x2c8)
    pub next_exp: u32,
    pub stats: Stats,
}

impl Progress {
    /// A character set to `level` (clamped to 200) with the given vocation's
    /// growth for every level, and zero experience. Translated from 0x00b5ccb0,
    /// which also rebuilds stats when `cPlayerInfo` +0x35a is set (0x00b5cb30):
    /// stats then reflect the current vocation only, unlike normal play.
    pub fn at_level(level: u32, growth: &[LevelEntry], exp_rows: &[ExpEntry]) -> Self {
        let level = level.min(MAX_LEVEL);
        let mut p = Self {
            level: 1,
            exp: 0,
            next_exp: exp_rows[0].level,
            stats: Stats::initial(growth),
        };
        if level > 1 {
            while p.level < level {
                p.level_up(true, growth, exp_rows);
            }
            p.exp = 0;
        }
        p
    }

    /// One level-up step. Translated from 0x00b5bd20 (`uPlayer` update vf90,
    /// 0x00b61a20, calls it once per frame, so a large exp award resolves one
    /// level per frame). `growth` is the `rPlLevel` of the vocation held now.
    /// Returns whether a level was gained. `force` ignores the exp check.
    pub fn level_up(&mut self, force: bool, growth: &[LevelEntry], exp_rows: &[ExpEntry]) -> bool {
        if self.level > 199 {
            return false;
        }
        if self.exp < self.next_exp && !force {
            return false;
        }
        self.exp = self.exp.wrapping_sub(self.next_exp);
        self.level = (self.level + 1).min(MAX_LEVEL);
        self.stats.grow(level_row(self.level, growth));
        self.next_exp = if self.level < 200 { next_exp(self.level, exp_rows) } else { 0 };
        true
    }
}

/// Experience-to-discipline bands, from the table at 0x014f24a0: each band
/// starts at an amount of experience and keeps `100 - percent` of it.
pub const DISCIPLINE_BANDS: [(i32, f32); 20] = [
    (0, 60.0),
    (50, 75.0),
    (100, 85.0),
    (150, 90.0),
    (200, 92.0),
    (300, 94.0),
    (400, 95.0),
    (500, 96.0),
    (700, 97.0),
    (900, 97.5),
    (1200, 98.0),
    (1500, 98.5),
    (2000, 99.0),
    (3000, 99.2),
    (5000, 99.4),
    (7500, 99.5),
    (10000, 99.6),
    (15000, 99.7),
    (20000, 99.8),
    (30000, 99.9),
];

/// Discipline earned from one experience award.
///
/// Translated from 0x00782f30 (SSE single precision, final `CVTTSS2SI`):
/// the award is split over the bands and each part keeps
/// `part - percent * part * 0.01`.
pub fn discipline_from_exp(exp: i32) -> i32 {
    let mut acc = 0.0f32;
    for (k, &(start, pct)) in DISCIPLINE_BANDS.iter().enumerate() {
        let end = DISCIPLINE_BANDS.get(k + 1).map_or(-1, |b| b.0);
        let (part, last) = if end == -1 || exp < end {
            ((exp - start) as f32, true)
        } else {
            ((end - start) as f32, false)
        };
        acc += part - pct * part * 0.01f32;
        if last {
            break;
        }
    }
    acc as i32
}

/// Discipline and vocation progress of one character.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Discipline {
    /// `mJobPoint` (+0x344): spendable discipline, shared by all vocations.
    pub points: i32,
    /// `mJobLevel[vocation]` (+0x2cc)
    pub rank: [u32; 10],
    /// `mJobExp[vocation]` (+0x2f4)
    pub exp: [u32; 10],
    /// `mJobNextExp[vocation]` (+0x31c)
    pub next: [u32; 10],
}

/// Adds experience to a character. Translated from 0x00782d10.
///
/// `exp_rate` is the Weal augment value (augment 93, 0x00783340: 1.15 when
/// equipped, 1.0 otherwise). `item_rates` are the two equipment factors
/// (item 0x361 in slot +0x3e4; item 0x47f in slots +0x3f4 and +0x3f8, the
/// second multiplying the first) that come from an unresolved parameter
/// object at sGameSys+0xb875c; pass `[1.0, 1.0]` when none is worn.
pub fn award_exp(
    p: &mut Progress,
    d: &mut Discipline,
    vocation: usize,
    exp: u32,
    exp_rate: f32,
    item_rates: [f32; 2],
) {
    // x87: FILD (unsigned fix-up), FMUL by the f32 rate, FISTP truncating;
    // the product is exact in f64.
    let gained = (f64::from(exp) * f64::from(exp_rate)) as i64 as u32;
    p.exp = p.exp.wrapping_add(gained);
    let dp = discipline_from_exp(gained as i32);
    // SSE single: CVTSI2SS, MULSS, MULSS, CVTTSS2SI.
    let dp = (dp as f32 * item_rates[0] * item_rates[1]) as i32;
    d.exp[vocation] = d.exp[vocation].wrapping_add(dp as u32);
    d.points += dp;
    if d.points > DISCIPLINE_CAP {
        d.points = DISCIPLINE_CAP;
    }
    if p.exp >= EXP_CAP {
        p.exp = EXP_CAP;
    }
}

/// `mNextExp` of the `rPlJobLevel` band holding `rank` (0x00b5cc50).
pub fn rank_next_exp(rank: u32, rows: &[JobLevelEntry]) -> u32 {
    let mut i = 1;
    loop {
        if rows[i].level <= rank && rank < rows[i + 1].level {
            return rows[i].next_exp;
        }
        i += 1;
        if i >= rows.len() - 1 {
            return rows[i].next_exp;
        }
    }
}

/// One vocation-rank step. Translated from 0x00b5c230 (called after the
/// level-up check every frame): below rank 9, when the vocation's discipline
/// experience reaches its threshold, subtract it and rank up.
pub fn rank_up(d: &mut Discipline, vocation: usize, rows: &[JobLevelEntry]) -> bool {
    let rank = d.rank[vocation];
    if rank >= MAX_RANK || d.exp[vocation] < d.next[vocation] {
        return false;
    }
    d.exp[vocation] -= d.next[vocation];
    d.rank[vocation] = (rank + 1).min(MAX_RANK);
    d.next[vocation] = rank_next_exp(d.rank[vocation], rows);
    true
}

/// Share multiplier for party member `i` when experience is distributed.
///
/// Translated from 0x004437d0 (SSE single precision): the product, over the
/// other three party slots, of `clamp(1 + (level_i - level_j) * 0.01, 0.75,
/// 1.25)`; an empty slot counts as level 0. `levels[j] = None` for an empty
/// slot. Callers (0x00443470) then compute `trunc(trunc(m * exp + 0.5) * rate)`.
pub fn party_share(levels: [Option<u32>; 4], i: usize) -> f32 {
    let Some(own) = levels[i] else { return 1.0 };
    let mut m = 1.0f32;
    for (j, other) in levels.iter().enumerate() {
        if j == i {
            continue;
        }
        let other = other.unwrap_or(0) as i32;
        let f = ((own as i32 - other) as f32 * 0.01f32 + 1.0).clamp(0.75, 1.25);
        m *= f;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp_rows() -> Vec<ExpEntry> {
        // param\pl\level\exp, first copy (docs/PARAMS.md).
        [(200, 1.0), (220, 1.0), (2, 1.3), (6, 3.0), (14, 5.0), (18, 8.0), (25, 9.0),
         (36, 10.0), (42, 11.0), (51, 4.0), (101, 15.0), (151, 20.0), (200, 0.0)]
            .iter()
            .map(|&(level, rate)| ExpEntry { level, rate })
            .collect()
    }

    fn fighter() -> Vec<LevelEntry> {
        // param\pl\level\lvfighter
        [[0, 0, 0, 0, 0, 0, 0], [1, 450, 0, 80, 80, 60, 60], [2, 30, 20, 4, 3, 2, 2],
         [11, 37, 15, 4, 4, 2, 1], [101, 15, 5, 1, 3, 0, 0], [201, 0, 0, 0, 0, 0, 0]]
            .iter()
            .map(|r| LevelEntry {
                level: r[0], hp_max: r[1], stamina_max: r[2], attack: r[3],
                defend: r[4], magic_attack: r[5], magic_defend: r[6], next_exp: 0,
            })
            .collect()
    }

    /// Values returned by the running game's 0x00b5c080 and 0x00b5c150
    /// (claude-4 oracle, board #722, 2026-10-09).
    #[test]
    fn exp_curve_matches_live_game() {
        let rows = exp_rows();
        let live = [
            (1, 200), (2, 485), (3, 770), (5, 1340), (6, 2000), (10, 4640), (14, 7720),
            (50, 80_100), (100, 124_100), (150, 289_100), (199, 504_700),
        ];
        for (level, want) in live {
            assert_eq!(next_exp(level, &rows), want, "next_exp({level})");
        }
        let live = [
            (2, 200), (3, 685), (10, 15_810), (100, 6_543_250), (199, 36_131_050),
            (200, 36_636_536),
        ];
        for (level, want) in live {
            assert_eq!(cumulative_exp(level, &rows), want, "cumulative_exp({level})");
        }
        // The level-200 constant is not the sum of the curve.
        let total: u32 = (1..200).map(|l| next_exp(l, &rows)).sum();
        assert_eq!(total, 36_635_750);
        assert_eq!(cumulative_exp(1, &rows), 0);
    }

    #[test]
    fn level_rows() {
        let g = fighter();
        assert_eq!(level_row(2, &g).level, 2);
        assert_eq!(level_row(10, &g).level, 2);
        assert_eq!(level_row(11, &g).level, 11);
        assert_eq!(level_row(100, &g).level, 11);
        assert_eq!(level_row(101, &g).level, 101);
        assert_eq!(level_row(200, &g).level, 101);
    }

    #[test]
    fn fighter_growth() {
        let (g, e) = (fighter(), exp_rows());
        let p = Progress::at_level(1, &g, &e);
        assert_eq!(p.stats.hp_max, 450.0);
        assert_eq!(p.next_exp, 200);
        let p = Progress::at_level(11, &g, &e);
        // 9 steps of row "2" (levels 2..10) and one of row "11".
        assert_eq!(p.stats.hp_max, 450.0 + 9.0 * 30.0 + 37.0);
        assert_eq!(p.stats.attack, 80.0 + 9.0 * 4.0 + 4.0);
        assert_eq!(p.stats.stamina_lv, 9.0 * 20.0 + 15.0);
        assert_eq!(p.next_exp, next_exp(11, &e));
        let p = Progress::at_level(250, &g, &e);
        assert_eq!(p.level, 200);
        assert_eq!(p.next_exp, 0);
        assert_eq!(p.stats.hp_max, 450.0 + 9.0 * 30.0 + 90.0 * 37.0 + 100.0 * 15.0);
    }

    #[test]
    fn level_up_needs_exp() {
        let (g, e) = (fighter(), exp_rows());
        let mut p = Progress::at_level(1, &g, &e);
        p.exp = 199;
        assert!(!p.level_up(false, &g, &e));
        p.exp = 700;
        assert!(p.level_up(false, &g, &e));
        assert_eq!((p.level, p.exp, p.next_exp), (2, 500, 485));
        assert!(p.level_up(false, &g, &e));
        assert_eq!((p.level, p.exp), (3, 15));
        assert!(!p.level_up(false, &g, &e));
    }

    /// Values returned by the running game's 0x00782f30 (board #722).
    #[test]
    fn discipline_matches_live_game() {
        assert_eq!(discipline_from_exp(0), 0);
        // 100 = 50 * 0.4 + 50 * 0.25; 1000 = 20 + 12.5 + 7.5 + 5 + 8 + 6 + 5 + 8 + 6 + 2.5
        let live = [(10, 4), (49, 19), (50, 20), (100, 32), (1000, 80), (5000, 125), (100_000, 277)];
        for (exp, want) in live {
            assert_eq!(discipline_from_exp(exp), want, "discipline_from_exp({exp})");
        }
    }

    #[test]
    fn rank_up_steps() {
        let rows: Vec<JobLevelEntry> = [0, 500, 1000, 2000, 3000, 4000, 5000, 6000, 7500, 0, 0]
            .iter()
            .enumerate()
            .map(|(i, &n)| JobLevelEntry { level: i as u32, next_exp: n })
            .collect();
        assert_eq!(rank_next_exp(1, &rows), 500);
        assert_eq!(rank_next_exp(9, &rows), 0);
        let mut d = Discipline::default();
        d.rank[1] = 1;
        d.next[1] = 500;
        d.exp[1] = 1600;
        assert!(rank_up(&mut d, 1, &rows));
        assert_eq!((d.rank[1], d.exp[1], d.next[1]), (2, 1100, 1000));
        assert!(rank_up(&mut d, 1, &rows));
        assert!(!rank_up(&mut d, 1, &rows));
        assert_eq!((d.rank[1], d.exp[1]), (3, 100));
    }

    #[test]
    fn award_and_caps() {
        let mut p = Progress::default();
        let mut d = Discipline::default();
        award_exp(&mut p, &mut d, 1, 100, 1.15, [1.0, 1.0]);
        // 100 * 1.15f = 114.9999976 truncates to 114 at x87 precision.
        assert_eq!(p.exp, 114);
        assert_eq!(d.points, discipline_from_exp(114));
        assert_eq!(d.exp[1], d.points as u32);
        d.points = DISCIPLINE_CAP;
        award_exp(&mut p, &mut d, 1, 10, 1.0, [1.0, 1.0]);
        assert_eq!(d.points, DISCIPLINE_CAP);
    }

    #[test]
    fn party_shares() {
        let l = [Some(20), Some(10), None, None];
        // vs pawn: 1.1; vs two empty slots: 1.2 each.
        assert!((party_share(l, 0) - 1.1 * 1.2 * 1.2).abs() < 1e-5);
        assert!((party_share(l, 1) - 0.9 * 1.1 * 1.1).abs() < 1e-5);
        assert_eq!(party_share([Some(100), Some(1), Some(1), Some(1)], 0), 1.25f32.powi(3));
    }
}
