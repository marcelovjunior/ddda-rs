//! One character's progression record (an Arisen or a pawn): the parts of
//! `cPlayerInfo` this crate owns.

use crate::tables::{augment_value, Tables};
use ddskill::growth::{self, Discipline, Progress};
use ddskill::vocation::{Vocation, WeaponCategory};

/// Skill ids are below 0x1ae (every skill bitset in the exe is bounded by it).
pub const SKILL_IDS: usize = 0x1ae;
/// Augment ids are below 0x73 (0x006fe050 / 0x006fc6d8).
pub const AUGMENT_IDS: usize = 0x73;

/// A plain bit set (the exe's skill bitsets at `cPlayerInfo`+0x72c are
/// LSB-first words, 0x00780ba0 / 0x00780c90).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bits(Vec<u32>);

impl Bits {
    pub fn new(len: usize) -> Self {
        Bits(vec![0; len.div_ceil(32)])
    }
    pub fn get(&self, i: usize) -> bool {
        self.0.get(i >> 5).is_some_and(|w| w & (1 << (i & 31)) != 0)
    }
    pub fn set(&mut self, i: usize, v: bool) {
        if let Some(w) = self.0.get_mut(i >> 5) {
            if v {
                *w |= 1 << (i & 31);
            } else {
                *w &= !(1 << (i & 31));
            }
        }
    }
}

/// What a character has learned (`cPlayerInfo`+0x72c and +0x7dc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Learned {
    /// +0x730: core skills and weapon skills at level 1.
    pub level1: Bits,
    /// +0x768: weapon skills at level 2.
    pub level2: Bits,
    /// +0x7a0: level 3 granted by worn equipment (rebuilt by 0x00442af0).
    pub level3: Bits,
    /// +0x7dc: augments.
    pub augments: Bits,
}

impl Default for Learned {
    fn default() -> Self {
        Learned {
            level1: Bits::new(SKILL_IDS),
            level2: Bits::new(SKILL_IDS),
            level3: Bits::new(SKILL_IDS),
            augments: Bits::new(AUGMENT_IDS),
        }
    }
}

impl Learned {
    /// Level of a weapon skill, 0 if not learned. Translated from 0x00780ba0
    /// (DDDA.exe 2017-12-13 Steam), type-2 branch: level 3 needs the
    /// equipment bit and level 1 or 2.
    pub fn weapon_level(&self, id: u16) -> u8 {
        let i = id as usize;
        if self.level3.get(i) && (self.level2.get(i) || self.level1.get(i)) {
            3
        } else if self.level2.get(i) {
            2
        } else {
            u8::from(self.level1.get(i))
        }
    }
}

/// Discipline-boosting gear the host reports (item ids per 0x00782d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DisciplineGear {
    /// Item 0x361 (Asura Armor) in equipment slot +0x3e4.
    pub asura_armor: bool,
    /// Item 0x47f (Ring of Perseverance) in slots +0x3f4 and +0x3f8.
    pub perseverance_rings: [bool; 2],
}

/// One character.
#[derive(Debug, Clone, PartialEq)]
pub struct Character {
    /// `mJob` (+0x10).
    pub vocation: Vocation,
    pub progress: Progress,
    pub discipline: Discipline,
    pub learned: Learned,
    /// `mAbility[6]` (+0x258).
    pub augments: [Option<u16>; 6],
    /// `mWeaponSkill[category]` (+0x120 + category * 0x18): six ids per
    /// weapon category.
    pub weapon_sets: [[Option<u16>; 6]; 13],
    /// `mMainSkill[3]` (+0x270) and `mSubSkill[3]` (+0x27c).
    pub active: [Option<u16>; 6],
    /// `OnlinePoint` (+0x34c): Rift Crystals a support pawn earned for its
    /// owner (0x00443470, cap 99,999,999).
    pub rift_crystals: i32,
    /// Vocations already paid for (the GSF flags 540..554 of the original,
    /// kept per character here; see `docs/VOCATIONS.md`, co-op).
    pub paid: [bool; 10],
    /// Number of vocation changes (sGameSys+0xb88b4 for the Arisen,
    /// 0x0044b7a0; the pawn's twin 0x0044b4d0), cap 9999.
    pub changes: u32,
    pub gear: DisciplineGear,
    pub is_pawn: bool,
}

impl Character {
    /// A new level-1 character. Stats from row 1 of the vocation's
    /// `rPlLevel` (0x00b5ccb0); every vocation at rank 1 with its threshold
    /// (0x00b5cb30; the live save shows rank 1 for unused vocations, board
    /// #722); the skills and augments with cost 0 and rank 0 in
    /// `rSkillList` already known. The vocation counts as paid.
    pub fn new(tables: &Tables, vocation: Vocation, is_pawn: bool) -> Self {
        let progress = Progress::at_level(1, tables.growth(vocation), &tables.exp);
        let mut discipline = Discipline::default();
        for v in Vocation::ALL {
            let j = v as usize;
            discipline.rank[j] = 1;
            discipline.next[j] = growth::rank_next_exp(1, tables.ranks(v));
        }
        discipline.rank[0] = 1;
        discipline.next[0] = growth::rank_next_exp(1, &tables.ranks[0]);
        let mut learned = Learned::default();
        for s in tables.skills.core.iter().filter(|s| s.cost == 0 && s.rank == 0) {
            learned.level1.set(s.skill_id as usize, true);
        }
        for s in tables.skills.weapon.iter().filter(|s| s.cost == 0 && s.rank == 0) {
            let bits = if s.level == 2 { &mut learned.level2 } else { &mut learned.level1 };
            bits.set(s.skill_id as usize, true);
        }
        let mut paid = [false; 10];
        paid[vocation as usize] = true;
        Character {
            vocation,
            progress,
            discipline,
            learned,
            augments: [None; 6],
            weapon_sets: [[None; 6]; 13],
            active: [None; 6],
            rift_crystals: 0,
            paid,
            changes: 0,
            gear: DisciplineGear::default(),
            is_pawn,
        }
    }

    /// Value of an augment for this character (0x00783340).
    pub fn augment(&self, tables: &Tables, id: u16) -> Option<f32> {
        augment_value(tables, &self.augments, id)
    }

    /// Total experience: cumulative experience of the level plus `mExp`, as
    /// 0x00443470 adds them in the x87 unit and stores an f32.
    pub fn total_exp(&self, tables: &Tables) -> f32 {
        (f64::from(growth::cumulative_exp(self.progress.level, &tables.exp)) + f64::from(self.progress.exp)) as f32
    }

    /// Experience award to this character: 0x00782d10 through
    /// `ddskill::growth::award_exp`, with Weal (augment 0x5d) and the
    /// discipline gear.
    pub fn award(&mut self, tables: &Tables, exp: u32) {
        let weal = self.augment(tables, 0x5d).unwrap_or(0.0);
        let mut gear = [1.0f32, 1.0];
        if self.gear.asura_armor {
            gear[0] = tables.asura_armor_rate();
        }
        for &ring in &self.gear.perseverance_rings {
            if ring {
                gear[1] *= tables.perseverance_rate();
            }
        }
        let vocation = self.vocation as usize;
        growth::award_exp(&mut self.progress, &mut self.discipline, vocation, exp, weal, gear);
    }

    /// One simulation step of the `uPlayer` update (vf90 0x00b61a20): at
    /// most one level-up (0x00b5bd20, with the current vocation's growth)
    /// and then at most one rank-up (0x00b5c230). Returns (levelled, ranked).
    pub fn step(&mut self, tables: &Tables) -> (bool, bool) {
        let up = self.progress.level_up(false, tables.growth(self.vocation), &tables.exp);
        let v = self.vocation;
        let rank = growth::rank_up(&mut self.discipline, v as usize, tables.ranks(v));
        (up, rank)
    }

    /// Steps until nothing changes (a host may run this after a large award
    /// instead of one step per frame; the end state is the same).
    pub fn settle(&mut self, tables: &Tables) {
        while self.step(tables) != (false, false) {}
    }

    /// Weapon skill slots of a category the vocation may fill: 6 for staves
    /// in the hands of a Mage or Sorcerer, otherwise 3 (0x00780840,
    /// 0x00780590).
    pub fn set_size(&self, category: WeaponCategory) -> usize {
        match category {
            WeaponCategory::Staff | WeaponCategory::Archistaff => self.vocation.staff_slots(),
            _ => 3,
        }
    }
}
