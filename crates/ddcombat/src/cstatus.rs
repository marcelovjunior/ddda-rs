//! Timed statuses: `cStatus` (actor+0x2698) and what each status does.
//!
//! A fired build-up slot (`crate::status`) becomes a status *kind* (the
//! slot-to-kind table [`crate::status::SLOT_KIND`]), added with the values
//! of its `rStatusParam` entry (`param\status\enemy` / `param\status\player`,
//! one entry per kind). Every function names the routine of DDDA.exe
//! (2017-12-13 Steam build) it is translated from; `docs/STATUS.md` lists
//! the kinds, their entries and effects.

/// One `cStatusParam` (an `rStatusParam` row). Offsets in memory: +0x04
/// `mCategory`, +0x08 `mTimer`, +0x0C `mParam0`, +0x10 `mParam1`, +0x14
/// `mIsOverwrite`, +0x18 `mOverwriteNum` (read by 0x00837090).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusEntry {
    /// Low byte: group (conflict table); byte 1: subgroup; bit 16: priority.
    pub category: u32,
    pub is_overwrite: bool,
    pub overwrite_num: u32,
    /// Seconds; >= 10000 never runs out.
    pub timer: f32,
    pub param0: f32,
    pub param1: f32,
}

/// An `rStatusParam`: entries indexed by kind (0..0x27).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusTable(pub Vec<Option<StatusEntry>>);
impl StatusTable {
    pub fn get(&self, kind: u32) -> Option<&StatusEntry> {
        self.0.get(kind as usize).and_then(|e| e.as_ref())
    }
}

/// Status kinds (indices of `rStatusParam`); the names of 0..12 and 14..21
/// follow the build-up slots and multipliers that reach them.
pub mod kind {
    pub const POISON: u32 = 0;
    pub const SLOW: u32 = 1;
    pub const BLIND: u32 = 2;
    pub const SILENCE: u32 = 3;
    pub const SLEEP: u32 = 4;
    pub const OIL: u32 = 5;
    pub const DRENCHED: u32 = 6;
    /// `敵化`: turned against its side (possession).
    pub const POSSESSED: u32 = 7;
    pub const SKILL_SEAL: u32 = 8;
    pub const CURSE: u32 = 9;
    pub const BURNING: u32 = 10;
    pub const FROZEN: u32 = 11;
    pub const PETRIFY: u32 = 12;
    pub const ATTACK_DOWN: u32 = 14;
    pub const DEFENCE_DOWN: u32 = 15;
    pub const MGC_ATTACK_DOWN: u32 = 16;
    pub const MGC_DEFENCE_DOWN: u32 = 17;
    pub const ATTACK_UP: u32 = 18;
    pub const DEFENCE_UP: u32 = 19;
    pub const MGC_ATTACK_UP: u32 = 20;
    pub const MGC_DEFENCE_UP: u32 = 21;
    /// Adding it first removes kinds 0..0x11 (0x00837090).
    pub const CLEANSE: u32 = 0x22;
}

/// `0x01509F20`: `CONFLICT[a][b]` with groups a, b (category low byte).
/// An active status of group `old` blocks a new one of group `new` when
/// `CONFLICT[old][new]` (0x00837730); a new status removes the active ones
/// with `CONFLICT[new][old]` (0x00837810).
pub const CONFLICT: [[u8; 9]; 9] = [
    [0; 9],
    [0; 9],
    [0, 1, 0, 1, 1, 0, 1, 0, 0],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0, 1, 1, 1, 1, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 0],
];

fn conflict(a: u32, b: u32) -> bool {
    CONFLICT.get(a as usize & 0xff).and_then(|r| r.get(b as usize & 0xff)).is_some_and(|v| *v != 0)
}

/// One active status (`cStatus::cStatWork`, 0x20 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Work {
    pub kind: u32,
    pub category: u32,
    /// +0x0C: frames left (`mTimer` x 30).
    pub timer: f32,
    /// +0x10 / +0x14: speed factors of the countdown (1; drenched sets
    /// +0x10 to 2 while the owner's +0x2697 is set, 0x00852B10).
    pub speed: [f32; 2],
    /// +0x18 / +0x1C: `mParam0`, `mParam1`.
    pub param0: f32,
    pub param1: f32,
}

/// A character's active statuses (`cStatus`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Statuses {
    pub works: Vec<Work>,
    /// The owner refuses new statuses (player-side owners with +0x4CF0 set).
    pub immune: bool,
}

impl Statuses {
    pub fn has(&self, kind: u32) -> bool {
        self.works.iter().any(|w| w.kind == kind)
    }

    /// Active kinds as bits (`cStatus`+0x08, actor+0x26A0 for kinds < 32).
    pub fn mask(&self) -> u64 {
        self.works.iter().fold(0, |m, w| m | 1u64 << w.kind.min(63))
    }

    /// Translated from 0x00837730: may a status of `category` be added.
    pub fn can_add(&self, kind: u32, category: u32) -> bool {
        for w in &self.works {
            if w.kind == kind::CLEANSE && 0x11 < kind && kind < 0x16 {
                return true;
            }
            let (n, o) = (category, w.category);
            if conflict(o, n) {
                return false;
            }
            if n & 0xff == o & 0xff && (o & 0xff00 == 0x1a00 || (n & 0xff00 == o & 0xff00 && n & 0x1_0000 != 0 && o & 0x1_0000 == 0)) {
                return false;
            }
        }
        true
    }

    /// Translated from 0x00837810: remove the active statuses a new one of
    /// `category` displaces.
    pub fn remove_displaced(&mut self, category: u32) {
        let n = category;
        let nb = n & 0xff00;
        self.works.retain(|w| {
            let o = w.category;
            let displaced = conflict(n, o)
                || (n & 0xff == o & 0xff
                    && nb != 0
                    && (nb == 0x1a00 || (nb == o & 0xff00 && !(n & 0x1_0000 != 0 && o & 0x1_0000 == 0))));
            !displaced
        });
    }

    /// Translated from 0x00837540: remove every status of this kind.
    pub fn remove(&mut self, kind: u32) -> usize {
        let before = self.works.len();
        self.works.retain(|w| w.kind != kind);
        before - self.works.len()
    }

    /// Translated from 0x00837090 (DDDA.exe 2017-12-13 Steam): add or
    /// refresh a status. A kind already active is refreshed (timer back to
    /// `mTimer` x 30 frames and its parameters) unless `mIsOverwrite` allows
    /// another stack (fewer than `mOverwriteNum` active), which is added if
    /// [`Statuses::can_add`] agrees, after [`Statuses::remove_displaced`].
    /// Adding [`kind::CLEANSE`] first removes kinds 0..0x11.
    pub fn add(&mut self, kind: u32, table: &StatusTable) -> bool {
        if kind > 0x27 || self.immune {
            return false;
        }
        if kind == kind::CLEANSE {
            for k in 0..0x12 {
                self.remove(k);
            }
        }
        let Some(e) = table.get(kind).copied() else { return false };
        let count = self.works.iter().filter(|w| w.kind == kind).count() as u32;
        let existing = self.works.iter().position(|w| w.kind == kind);
        let mut slot = existing;
        if (existing.is_none() || (e.is_overwrite && count < e.overwrite_num)) && self.can_add(kind, e.category) {
            if kind != kind::CLEANSE {
                self.remove_displaced(e.category);
            }
            self.works.push(Work { kind, category: e.category, timer: 0.0, speed: [1.0, 1.0], param0: 0.0, param1: 0.0 });
            slot = Some(self.works.len() - 1);
        }
        let Some(i) = slot else { return false };
        let w = &mut self.works[i];
        w.timer = e.timer * 30.0;
        w.param0 = e.param0;
        w.param1 = e.param1;
        true
    }

    /// Translated from 0x00836C70: count every status down by `rate` (the
    /// scaled frame step, 1 per 1/30 s) times its two speed factors; a
    /// timer of 10000 frames or more never runs out. Returns the kinds that
    /// wore off.
    pub fn tick(&mut self, rate: f32) -> Vec<u32> {
        let mut ended = Vec::new();
        self.works.retain_mut(|w| {
            if w.timer < 10000.0 {
                w.timer -= rate * w.speed[0] * w.speed[1];
                if w.timer <= 0.0 {
                    ended.push(w.kind);
                    return false;
                }
            }
            true
        });
        ended
    }

    /// Translated from 0x00837480: the product of `mParam0` of every active
    /// status of kind `a` or `b` (1 if none).
    pub fn product(&self, a: u32, b: u32) -> f32 {
        self.works.iter().filter(|w| w.kind == a || w.kind == b).fold(1.0, |p, w| p * w.param0)
    }

    /// 0x00837650 / 0x008376C0: `mParam0` / `mParam1` of the first active
    /// status of this kind.
    pub fn param0(&self, kind: u32) -> Option<f32> {
        self.works.iter().find(|w| w.kind == kind).map(|w| w.param0)
    }
    pub fn param1(&self, kind: u32) -> Option<f32> {
        self.works.iter().find(|w| w.kind == kind).map(|w| w.param1)
    }

    /// The attacker's multipliers (0x0088B530 / 0x0088AA20): physical
    /// `Π(attack down, attack up)`, magick `Π(magick attack down, up)`,
    /// each times curse `mParam0` while cursed.
    pub fn attack_muls(&self) -> (f32, f32) {
        let curse = if self.has(kind::CURSE) { self.param0(kind::CURSE).unwrap_or(1.0) } else { 1.0 };
        (
            self.product(kind::ATTACK_DOWN, kind::ATTACK_UP) * curse,
            self.product(kind::MGC_ATTACK_DOWN, kind::MGC_ATTACK_UP) * curse,
        )
    }

    /// The defender's multipliers (0x00AAA490 / 0x00B7AD90): damage taken
    /// `Π(defence down, defence up)` and `Π(magick defence down, up)`, each
    /// times curse `mParam1` while cursed.
    pub fn defence_muls(&self) -> (f32, f32) {
        let curse = if self.has(kind::CURSE) { self.param1(kind::CURSE).unwrap_or(1.0) } else { 1.0 };
        (
            self.product(kind::DEFENCE_DOWN, kind::DEFENCE_UP) * curse,
            self.product(kind::MGC_DEFENCE_DOWN, kind::MGC_DEFENCE_UP) * curse,
        )
    }

    /// The victim-side scaling of `uCharacterBase::vf68` 0x0084D8B0 and
    /// `uEnemy::vf68` 0x00AAA490 for a hit of `element`: returns
    /// (physical, magick, stagger and knockdown) multipliers.
    /// - frozen: physical x `mParam0` (2);
    /// - oil: fire hits x `mParam0` (2), both parts;
    /// - drenched: fire x `mParam0` (0.5), ice and thunder x `mParam1` (2);
    /// - sleep: damage x `mParam1` (2), stagger and knockdown x `mParam0`
    ///   (1.5); on enemies also damage x `param\game`[0x12] and stagger x
    ///   [0x13] (`game_sleep` = (3, 10) in this install).
    pub fn victim_scale(&self, element: u32, enemy_sleep: Option<(f32, f32)>) -> (f32, f32, f32) {
        let (mut p, mut m, mut b) = (1.0f32, 1.0f32, 1.0f32);
        if let (Some((d, s)), true) = (enemy_sleep, self.has(kind::SLEEP)) {
            p *= d;
            m *= d;
            b *= s;
        }
        if self.has(kind::FROZEN) {
            p *= self.param0(kind::FROZEN).unwrap_or(1.0);
        }
        if self.has(kind::OIL) && element == 1 {
            let f = self.param0(kind::OIL).unwrap_or(1.0);
            p *= f;
            m *= f;
        }
        if self.has(kind::DRENCHED) {
            let f = match element {
                1 => self.param0(kind::DRENCHED),
                2 | 3 => self.param1(kind::DRENCHED),
                _ => None,
            }
            .unwrap_or(1.0);
            p *= f;
            m *= f;
        }
        if self.has(kind::SLEEP) {
            let d = self.param1(kind::SLEEP).unwrap_or(1.0);
            p *= d;
            m *= d;
            b *= self.param0(kind::SLEEP).unwrap_or(1.0);
        }
        (p, m, b)
    }

    /// Slow (`uCharacterBase::vf138` 0x00852670): the owner's step
    /// (`cUnit.mDeltaTime`, +0x14) is multiplied by `mParam0` every frame.
    pub fn step_scale(&self) -> f32 {
        if self.has(kind::SLOW) {
            self.param0(kind::SLOW).unwrap_or(1.0)
        } else {
            1.0
        }
    }
}

/// Per-character state of the damage-over-time handlers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DotState {
    /// actor+0x27B4: 0 not started.
    pub poison_stage: u32,
    /// actor+0x2854: the poison interval counter.
    pub poison_timer: f32,
}

/// Translated from `uEnemy::vf137` 0x00AAFE10: enemy poison. The first
/// frame deals `mParam0` at once, then every `mParam1` seconds (counted
/// down by the owner's step `dt`). Returns the HP change (negative).
pub fn poison_enemy(st: &mut DotState, s: &Statuses, dt: f32) -> f32 {
    let Some(w) = s.works.iter().find(|w| w.kind == kind::POISON) else {
        *st = DotState::default();
        return 0.0;
    };
    if st.poison_stage == 0 {
        st.poison_stage = 1;
        st.poison_timer = 0.0;
    }
    st.poison_timer -= dt;
    if st.poison_timer < 0.0 {
        st.poison_timer = w.param1 * 30.0;
        return -w.param0;
    }
    0.0
}

/// Translated from `uPlayerBase::vf137` 0x00B75070: player poison. The
/// interval counter counts up by the scaled step; it starts at
/// `mParam1` x 30 - 3, a cue plays at `mParam1` x 30 - 3 and the damage
/// `mParam0` lands at `mParam1` x 30, after which it restarts at 0. So the
/// first damage comes 3 frames after onset, then every `mParam1` seconds.
pub fn poison_player(st: &mut DotState, s: &Statuses, rate: f32) -> f32 {
    let Some(w) = s.works.iter().find(|w| w.kind == kind::POISON) else {
        *st = DotState::default();
        return 0.0;
    };
    let period = w.param1 * 30.0;
    if st.poison_stage == 0 {
        st.poison_stage = 1;
        st.poison_timer = period - 3.0;
    }
    st.poison_timer += rate;
    match st.poison_stage {
        1 => {
            if period - 3.0 <= st.poison_timer {
                st.poison_stage = 2;
            }
            0.0
        }
        _ => {
            if period <= st.poison_timer {
                st.poison_stage = 1;
                st.poison_timer = 0.0;
                -w.param0
            } else {
                0.0
            }
        }
    }
}

/// Translated from `uCharacterBase::vf147` 0x00852F40: burning takes
/// `mParam0` HP per second, `mParam0 * dt / 30` per frame. (The burning
/// owner also carries a fire attack volume, entry +0x2750, that can set
/// others alight; not modelled.)
pub fn burning(s: &Statuses, dt: f32) -> f32 {
    match s.param0(kind::BURNING) {
        Some(p0) if s.has(kind::BURNING) => -(p0 * dt * 0.033_333_335),
        _ => 0.0,
    }
}
