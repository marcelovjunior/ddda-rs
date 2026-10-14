//! Learning and equipping skills and augments (the skill screens'
//! rules, without the screens).

use crate::character::{Character, AUGMENT_IDS, SKILL_IDS};
use crate::tables::Tables;
use ddskill::catalogue::SkillRecord;
use ddskill::vocation::{Vocation, WeaponCategory};

/// What can be learned or equipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Learnable {
    /// A core skill (`rSkillList` first table).
    Core(u16),
    /// A weapon skill at level 1 or 2.
    Weapon { id: u16, level: u8 },
    /// An augment.
    Augment(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Not in `rSkillList`, or listed with cost 0 (never offered).
    NotOffered,
    /// Not offered to the character's current vocation.
    WrongVocation,
    /// Vocation rank too low.
    Rank { need: i32, have: u32 },
    /// Not enough discipline.
    Discipline { need: i32, have: i32 },
    AlreadyKnown,
    /// Level 2 before level 1.
    NeedsLevel1,
    /// Augments 100..114 need their unlock (save bytes sGameSys+0xbe1b4).
    Locked,
    NotLearned,
    /// The skill does not belong to that weapon, or the slot is out of range.
    WrongSlot,
    /// The augment is already in another slot.
    Duplicate,
}

fn record(tables: &Tables, id: u16) -> Option<&SkillRecord> {
    tables.records.get(id as usize).filter(|_| (id as usize) < SKILL_IDS)
}

/// Whether the vocation's skill screen offers a skill with this weapon
/// mask. The screen's tabs (0x01516738, mapped to masks by 0x01516bd8)
/// cover the same weapons as the vocation's skill-set list (0x0163e784)
/// for all nine vocations, so the list is used here.
fn weapon_tab(v: Vocation, weapon_mask: u32) -> bool {
    v.weapon_categories().iter().any(|c| weapon_mask & c.mask() != 0)
}

/// Discipline cost of learning, or why not. Translated from the lineup
/// filters of `uGUISkillLearn`: core 0x006fd540 (weapon tab, cost != 0,
/// rank), weapon skills 0x006fdb40 (type 2, cost != 0, weapon tab,
/// vocation bit, rank, level 2 after level 1), augments 0x006fdc30
/// (`id / 10 == vocation - 1` under 100, unlock bytes from 100, rank), and
/// the confirm check 0x00702b94 (rank met, cost <= discipline, not known).
/// `unlocked(k)` answers sGameSys+0xbe1b4 + k for augments 100 + k.
pub fn check(
    tables: &Tables,
    c: &Character,
    what: Learnable,
    unlocked: impl Fn(u16) -> bool,
) -> Result<i32, Refused> {
    let v = c.vocation;
    let have_rank = c.discipline.rank[v as usize];
    let (cost, rank) = match what {
        Learnable::Core(id) => {
            let s = tables.skills.core.iter().find(|s| s.skill_id == i32::from(id)).ok_or(Refused::NotOffered)?;
            let r = record(tables, id).ok_or(Refused::NotOffered)?;
            if !weapon_tab(v, r.weapon_mask) {
                return Err(Refused::WrongVocation);
            }
            if c.learned.level1.get(id as usize) {
                return Err(Refused::AlreadyKnown);
            }
            (s.cost, s.rank)
        }
        Learnable::Weapon { id, level } => {
            let s = tables
                .skills
                .weapon
                .iter()
                .find(|s| s.skill_id == i32::from(id) && s.level == i32::from(level))
                .ok_or(Refused::NotOffered)?;
            let r = record(tables, id).ok_or(Refused::NotOffered)?;
            if r.kind != 2 {
                return Err(Refused::NotOffered);
            }
            if !weapon_tab(v, r.weapon_mask) || r.vocation_mask & v.mask() == 0 {
                return Err(Refused::WrongVocation);
            }
            let known = c.learned.weapon_level(id);
            if (level == 1 && c.learned.level1.get(id as usize)) || (level == 2 && c.learned.level2.get(id as usize)) {
                return Err(Refused::AlreadyKnown);
            }
            if level == 2 && known < 1 {
                return Err(Refused::NeedsLevel1);
            }
            (s.cost, s.rank)
        }
        Learnable::Augment(id) => {
            let a = tables.skills.augments.iter().find(|a| a.augment_id == i32::from(id)).ok_or(Refused::NotOffered)?;
            if id < 100 {
                if i32::from(id) / 10 != v as i32 - 1 {
                    return Err(Refused::WrongVocation);
                }
            } else if !unlocked(id - 100) {
                return Err(Refused::Locked);
            }
            if c.learned.augments.get(id as usize) {
                return Err(Refused::AlreadyKnown);
            }
            (a.cost, a.rank)
        }
    };
    if cost == 0 {
        return Err(Refused::NotOffered);
    }
    if rank > have_rank as i32 {
        return Err(Refused::Rank { need: rank, have: have_rank });
    }
    if cost > c.discipline.points {
        return Err(Refused::Discipline { need: cost, have: c.discipline.points });
    }
    Ok(cost)
}

/// Learns: sets the bit (0x00780c90 at `cPlayerInfo`+0x72c for skills,
/// +0x7dc for augments) and subtracts the cost from `mJobPoint`
/// (0x006fc758). Returns the cost paid.
pub fn learn(
    tables: &Tables,
    c: &mut Character,
    what: Learnable,
    unlocked: impl Fn(u16) -> bool,
) -> Result<i32, Refused> {
    let cost = check(tables, c, what, unlocked)?;
    match what {
        Learnable::Core(id) | Learnable::Weapon { id, level: 1 } => c.learned.level1.set(id as usize, true),
        Learnable::Weapon { id, .. } => c.learned.level2.set(id as usize, true),
        Learnable::Augment(id) => c.learned.augments.set(id as usize, true),
    }
    c.discipline.points -= cost;
    Ok(cost)
}

/// Puts a learned augment in slot `slot` (0..6) of `mAbility`, or empties
/// it. Augments learned in one vocation work in all (the vocation-change
/// path 0x00675da0 never touches `mAbility`). Refusing a second copy of the
/// same augment is **guessed** (the equip screen was not traced).
pub fn equip_augment(c: &mut Character, slot: usize, id: Option<u16>) -> Result<(), Refused> {
    if slot >= 6 {
        return Err(Refused::WrongSlot);
    }
    if let Some(id) = id {
        if id as usize >= AUGMENT_IDS || !c.learned.augments.get(id as usize) {
            return Err(Refused::NotLearned);
        }
        if c.augments.iter().enumerate().any(|(i, a)| i != slot && *a == Some(id)) {
            return Err(Refused::Duplicate);
        }
    }
    c.augments[slot] = id;
    Ok(())
}

/// Puts a learned weapon skill in the skill set of a weapon category, or
/// empties the slot. The skill must be one of that weapon's weapon skills
/// (0x014f6c80 / 0x014f6cb4 ranges, as 0x00780590 checks) and usable by the
/// current vocation (vocation bit, 0x00780840); staves give Mage and
/// Sorcerer six slots, everything else three.
pub fn equip_skill(
    tables: &Tables,
    c: &mut Character,
    category: WeaponCategory,
    slot: usize,
    id: Option<u16>,
) -> Result<(), Refused> {
    if slot >= c.set_size(category) {
        return Err(Refused::WrongSlot);
    }
    if let Some(id) = id {
        let (first, count) = category.weapon_skills();
        if !(first..first + count).contains(&i32::from(id)) {
            return Err(Refused::WrongSlot);
        }
        let r = record(tables, id).ok_or(Refused::WrongSlot)?;
        if r.vocation_mask & c.vocation.mask() == 0 {
            return Err(Refused::WrongVocation);
        }
        if c.learned.weapon_level(id) == 0 {
            return Err(Refused::NotLearned);
        }
    }
    c.weapon_sets[category as usize][slot] = id;
    Ok(())
}

/// Translated from 0x00780350 (DDDA.exe 2017-12-13 Steam): copy all six
/// primary slots, then overwrite slots 3..5 only for nonempty secondary
/// entries. Unarmed categories do not contribute. The separate
/// 0x00780590 range filter is outside this copy operation.
pub fn active_skills(c: &Character, primary: WeaponCategory, secondary: Option<WeaponCategory>) -> [Option<u16>; 6] {
    let mut out = [None; 6];
    if primary != WeaponCategory::Unarmed {
        out = c.weapon_sets[primary as usize];
    }
    if let Some(secondary) = secondary.filter(|category| *category != WeaponCategory::Unarmed) {
        for (slot, value) in c.weapon_sets[secondary as usize][..3].iter().enumerate() {
            if value.is_some() { out[3 + slot] = *value; }
        }
    }
    out
}
