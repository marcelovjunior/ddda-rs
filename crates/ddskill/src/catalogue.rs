//! The complete skill and augment catalogue.
//!
//! Joins three sources (see `docs/VOCATIONS.md`):
//! * `etc\skill\skill` ([`SkillList`]): learning cost and rank;
//! * tables in the player's `DDDA.exe` ([`ExeTables`], read at run time):
//!   the skill records, the name tables, the per-weapon action tables and
//!   the per-skill motion lists;
//! * the game text (`skillName`, `skill_command`, `abilityName`,
//!   `abilityInfo`), which gives every name.

use crate::exe::Exe;
use crate::skill_list::SkillList;
use crate::vocation::{Vocation, WeaponCategory};
use crate::Error;
use std::collections::HashMap;

/// Skill records, 0x20 bytes per id (users 0x006fdb40, 0x005ddcf0, 0x00b60e20).
pub const SKILL_TABLE_VA: u32 = 0x014f_3658;
/// Skill ids 0..429; every skill bitset in the exe is bounded by 0x1ae.
pub const SKILL_COUNT: usize = 430;
/// Skill text records, 12 bytes each, indexed by the skill record's +0x1c
/// (0x005ddcf0).
pub const SKILL_TEXT_VA: u32 = 0x0152_0278;
pub const SKILL_TEXT_COUNT: usize = 149;
/// Augment id -> `abilityName` message (2 bytes per id; used at 0x006ff129).
pub const AUGMENT_TEXT_VA: u32 = 0x0152_14a0;
/// Augment ids 0..114 (0x00783340 handles ids up to 0x72).
pub const AUGMENT_COUNT: usize = 115;
/// Per weapon category, a pointer to the per-skill motion list
/// (`(bank key, path)` pairs indexed by `skill id - first weapon skill`).
/// `uPlayer`+0x4c2c is set to this table (0x00b5876b) and 0x00b5b400 loads
/// one `rMotionList` per active skill slot from it.
pub const MOTION_LISTS_VA: u32 = 0x0163_dfc0;
/// Size of one action record: `MtDTI*` of the action class, then parameters.
pub const ACTION_RECORD_SIZE: u32 = 0x44;

/// The action table registered for each weapon category, `(address, count)`.
/// Primary weapons (categories 0..7) go to action group 1, from the jump
/// table at 0x00b611bc in 0x00b610f0; secondary ones (8..12) to group 2, from
/// 0x00b611e0. Both call 0x007f1fc0, which stores record pointers
/// `address + i * 0x44` for `i < count`. An action id is
/// `group << 24 | weapon << 16 | index`; the index selects the record.
pub const ACTION_TABLES: [(u32, u32); 13] = [
    (0x0186_f2a8, 7),  // 0 unarmed
    (0x0186_e3f8, 50), // 1 sword
    (0x0186_e3f8, 50), // 2 mace
    (0x0186_c498, 59), // 3 longsword
    (0x0186_b1e8, 65), // 4 dagger
    (0x0186_f5f0, 42), // 5 staff
    (0x0186_f5f0, 42), // 6 archistaff
    (0x0186_c498, 59), // 7 warhammer
    (0x0186_dd10, 26), // 8 shield
    (0x0186_d448, 33), // 9 magick shield
    (0x0186_b118, 3),  // 10 bow
    (0x0186_b118, 3),  // 11 longbow
    (0x0186_b118, 3),  // 12 magick bow
];

/// One record of the exe's skill table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillRecord {
    /// +0x00. Unused rows hold copy-paste ids; compare with the index.
    pub stored_id: i32,
    /// +0x04, -1 for none.
    pub action: u32,
    /// +0x08, bit `1 << vocation id`; 0 = any vocation using the weapon.
    pub vocation_mask: u32,
    /// +0x0c, bit `1 << weapon category`.
    pub weapon_mask: u32,
    /// +0x10: 0 action only, 1 core skill, 2 weapon skill.
    pub kind: u32,
    /// +0x14, meaning unresolved.
    pub field_14: i32,
    /// +0x18, meaning unresolved.
    pub flags: u32,
    /// +0x1c, index into the skill text table, -1 for none.
    pub text: i32,
}

/// One record of the skill text table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillText {
    /// `skillName` (DDN) message: the name itself for a core skill, the
    /// level-1 name plus one for a weapon skill (`name - 1 + level`).
    pub name: i16,
    /// `skill_command` message per level, -1 where the level does not exist.
    pub command: [i16; 3],
    /// +8, meaning unresolved.
    pub field_8: i16,
    /// +0xa; 0x8000 = levelled weapon skill (0x005ddcf0, 0x00780ba0).
    pub flags: u16,
}

/// One action record: the action class and its parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRecord {
    /// Address of the class's `MtDTI` object; 0 = empty slot.
    pub dti: u32,
    /// Class name, from [`Exe::dti_names`].
    pub class: Option<String>,
    /// The 16 words after the class pointer. For a weapon skill word 0 is
    /// the skill id (read by the action's vf05, e.g. 0x00b23d50, into
    /// action+0x54); other classes use it differently.
    pub params: [u32; 16],
}

/// A motion list entry: bank key (0x700 primary, 0x900 secondary) and path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionEntry {
    pub bank: u32,
    pub path: String,
}

/// Everything the catalogue reads from DDDA.exe.
#[derive(Debug, Clone)]
pub struct ExeTables {
    pub skills: Vec<SkillRecord>,
    pub texts: Vec<SkillText>,
    /// Per augment id: `abilityName` message index (`None` for 0xff) and
    /// the second byte (icon or group, unresolved).
    pub augments: Vec<(Option<u8>, u8)>,
    /// Per weapon category, the action table (see [`ACTION_TABLES`]).
    pub actions: Vec<Vec<ActionRecord>>,
    /// Per weapon category, the per-skill motion lists (empty for unarmed).
    pub motion_lists: Vec<Vec<MotionEntry>>,
}

impl ExeTables {
    /// Reads the tables from the bytes of the player's `DDDA.exe`.
    pub fn from_exe(bytes: &[u8]) -> Result<Self, Error> {
        let exe = Exe::parse(bytes)?;
        let mut skills = Vec::with_capacity(SKILL_COUNT);
        for id in 0..SKILL_COUNT as u32 {
            let r = SKILL_TABLE_VA + id * 0x20;
            skills.push(SkillRecord {
                stored_id: exe.i32(r)?,
                action: exe.u32(r + 4)?,
                vocation_mask: exe.u32(r + 8)?,
                weapon_mask: exe.u32(r + 0xc)?,
                kind: exe.u32(r + 0x10)?,
                field_14: exe.i32(r + 0x14)?,
                flags: exe.u32(r + 0x18)?,
                text: exe.i32(r + 0x1c)?,
            });
        }
        let mut texts = Vec::with_capacity(SKILL_TEXT_COUNT);
        for i in 0..SKILL_TEXT_COUNT as u32 {
            let r = SKILL_TEXT_VA + i * 12;
            texts.push(SkillText {
                name: exe.i16(r)?,
                command: [exe.i16(r + 2)?, exe.i16(r + 4)?, exe.i16(r + 6)?],
                field_8: exe.i16(r + 8)?,
                flags: exe.u16(r + 10)?,
            });
        }
        let mut augments = Vec::with_capacity(AUGMENT_COUNT);
        for id in 0..AUGMENT_COUNT as u32 {
            let m = exe.u8(AUGMENT_TEXT_VA + 2 * id)?;
            augments.push(((m != 0xff).then_some(m), exe.u8(AUGMENT_TEXT_VA + 2 * id + 1)?));
        }
        let names = exe.dti_names();
        let mut actions = Vec::with_capacity(ACTION_TABLES.len());
        for &(base, count) in &ACTION_TABLES {
            let mut table = Vec::with_capacity(count as usize);
            for i in 0..count {
                let r = base + i * ACTION_RECORD_SIZE;
                let dti = exe.u32(r)?;
                let mut params = [0u32; 16];
                for (k, p) in params.iter_mut().enumerate() {
                    *p = exe.u32(r + 4 + 4 * k as u32)?;
                }
                let class = names.get(&dti).cloned();
                if dti != 0 && class.is_none() {
                    return Err(Error(format!("action record {r:#010x}: unknown class {dti:#010x}")));
                }
                table.push(ActionRecord { dti, class, params });
            }
            actions.push(table);
        }
        let mut motion_lists = Vec::with_capacity(13);
        for cat in WeaponCategory::ALL {
            let list = exe.u32(MOTION_LISTS_VA + 4 * cat as u32)?;
            let mut entries = vec![];
            if list != 0 {
                let (_, count) = cat.weapon_skills();
                for i in 0..count as u32 {
                    let bank = exe.u32(list + 8 * i)?;
                    let path = exe.cstr(exe.u32(list + 8 * i + 4)?)?;
                    entries.push(MotionEntry { bank, path });
                }
            }
            motion_lists.push(entries);
        }
        Ok(Self { skills, texts, augments, actions, motion_lists })
    }

    /// The action record an action id selects when a weapon of `category`
    /// is equipped in the slot its group belongs to (group 1 primary,
    /// categories 0..7; group 2 secondary, 8..12).
    pub fn action(&self, category: WeaponCategory, action: u32) -> Option<&ActionRecord> {
        let group = action >> 24;
        let primary = (category as u8) < 8;
        if action == u32::MAX || (group == 1) != primary || !(1..=2).contains(&group) {
            return None;
        }
        self.actions[category as usize].get((action & 0xffff) as usize)
    }
}

/// The text files the catalogue names things with (one language).
pub struct Text<'a> {
    /// `id\DDN\message\skill\skillName_<lang>`
    pub skill_name: &'a [&'a str],
    /// `id\message\skill\skill_command_<lang>`
    pub skill_command: &'a [&'a str],
    /// `id\message\ability\abilityName_<lang>`
    pub ability_name: &'a [&'a str],
    /// `id\message\ability\abilityInfo_<lang>`
    pub ability_info: &'a [&'a str],
}

impl Text<'_> {
    pub const SKILL_NAME: &'static str = "id\\DDN\\message\\skill\\skillName";
    pub const SKILL_COMMAND: &'static str = "id\\message\\skill\\skill_command";
    pub const ABILITY_NAME: &'static str = "id\\message\\ability\\abilityName";
    pub const ABILITY_INFO: &'static str = "id\\message\\ability\\abilityInfo";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillKind {
    /// Learned with discipline, not placed in a skill slot (type 1).
    Core,
    /// Placed in a weapon skill slot, with levels (type 2).
    Weapon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLevel {
    pub level: u8,
    pub name: String,
    /// Input help from `skill_command`.
    pub command: String,
    /// Discipline cost and rank from `rSkillList`; `None` for level 3, which
    /// equipment grants (`docs/VOCATIONS.md`).
    pub cost: Option<i32>,
    pub rank: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub id: u16,
    pub kind: SkillKind,
    pub levels: Vec<SkillLevel>,
    /// Empty for core skills: any vocation using the weapon.
    pub vocations: Vec<Vocation>,
    pub weapons: Vec<WeaponCategory>,
    /// Action id, `None` for skills that are flags on other actions.
    pub action: Option<u32>,
    /// The action's class and first parameter (see [`ActionRecord`]).
    pub action_class: Option<String>,
    pub action_param: Option<u32>,
    /// Per-skill motion list for weapon skills.
    pub motion_list: Option<MotionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Augment {
    pub id: u16,
    pub name: Option<String>,
    pub description: Option<String>,
    /// The vocation that can learn it (`id / 10 + 1` under 100); `None` for
    /// the save-unlocked augments 100..114.
    pub vocation: Option<Vocation>,
    pub cost: i32,
    pub rank: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    pub skills: Vec<Skill>,
    pub augments: Vec<Augment>,
}

fn msg(file: &[&str], index: i32, what: &str) -> Result<String, Error> {
    usize::try_from(index)
        .ok()
        .and_then(|i| file.get(i))
        .map(|s| s.to_string())
        .ok_or_else(|| Error(format!("{what}: no message {index}")))
}

impl Catalogue {
    /// Joins `rSkillList`, the exe tables and the text. Every learnable skill
    /// and augment of `list` appears once, in `list` order.
    pub fn build(exe: &ExeTables, list: &SkillList, text: &Text) -> Result<Self, Error> {
        let mut costs: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
        let mut order: Vec<(i32, SkillKind)> = vec![];
        for s in &list.core {
            costs.insert((s.skill_id, 1), (s.cost, s.rank));
            order.push((s.skill_id, SkillKind::Core));
        }
        for s in &list.weapon {
            if costs.insert((s.skill_id, s.level), (s.cost, s.rank)).is_none() && s.level == 1 {
                order.push((s.skill_id, SkillKind::Weapon));
            }
        }
        let mut skills = Vec::with_capacity(order.len());
        for (id, kind) in order {
            let rec = exe
                .skills
                .get(id as usize)
                .ok_or_else(|| Error(format!("skill {id} outside the exe table")))?;
            let expected = match kind {
                SkillKind::Core => 1,
                SkillKind::Weapon => 2,
            };
            if rec.kind != expected {
                return Err(Error(format!("skill {id}: exe type {} for a {kind:?} skill", rec.kind)));
            }
            let t = exe
                .texts
                .get(rec.text as usize)
                .ok_or_else(|| Error(format!("skill {id}: text index {}", rec.text)))?;
            let mut levels = vec![];
            for level in 1..=3u8 {
                let command = t.command[level as usize - 1];
                if command < 0 || (kind == SkillKind::Core && level > 1) {
                    continue;
                }
                // Translated from 0x005ddcf0 (DDDA.exe 2017-12-13 Steam).
                let name_index = if t.flags & 0x8000 != 0 {
                    i32::from(t.name) - 1 + i32::from(level)
                } else {
                    i32::from(t.name)
                };
                let cost = costs.get(&(id, i32::from(level)));
                levels.push(SkillLevel {
                    level,
                    name: msg(text.skill_name, name_index, "skillName")?,
                    command: msg(text.skill_command, command.into(), "skill_command")?,
                    cost: cost.map(|c| c.0),
                    rank: cost.map(|c| c.1),
                });
            }
            let vocations = Vocation::ALL
                .into_iter()
                .filter(|v| rec.vocation_mask & v.mask() != 0)
                .collect();
            let weapons: Vec<_> = WeaponCategory::ALL
                .into_iter()
                .filter(|c| rec.weapon_mask & c.mask() != 0)
                .collect();
            let action = (rec.action != u32::MAX).then_some(rec.action);
            let record = action.and_then(|a| weapons.iter().find_map(|&c| exe.action(c, a)));
            let motion_list = match kind {
                SkillKind::Weapon => weapons.first().and_then(|&c| {
                    let (first, _) = c.weapon_skills();
                    exe.motion_lists[c as usize].get((id - first) as usize).cloned()
                }),
                SkillKind::Core => None,
            };
            skills.push(Skill {
                id: id as u16,
                kind,
                levels,
                vocations,
                weapons,
                action,
                action_class: record.and_then(|r| r.class.clone()),
                action_param: record.map(|r| r.params[0]),
                motion_list,
            });
        }
        let mut augments = Vec::with_capacity(list.augments.len());
        for a in &list.augments {
            let (message, _) = exe
                .augments
                .get(a.augment_id as usize)
                .copied()
                .ok_or_else(|| Error(format!("augment {} outside the exe table", a.augment_id)))?;
            let message = message.map(i32::from);
            augments.push(Augment {
                id: a.augment_id as u16,
                name: message.map(|m| msg(text.ability_name, m, "abilityName")).transpose()?,
                description: message.map(|m| msg(text.ability_info, m, "abilityInfo")).transpose()?,
                vocation: SkillList::augment_vocation(a.augment_id).and_then(|v| Vocation::from_id(v.into())),
                cost: a.cost,
                rank: a.rank,
            });
        }
        Ok(Self { skills, augments })
    }
}
