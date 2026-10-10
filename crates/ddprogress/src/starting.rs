//! Free skills learned and placed by the vocation-change screen.
//! The installed executable supplies the records and their order.

use crate::Character;
use ddskill::vocation::{Vocation, WeaponCategory};

/// Row of 0x015827e8, consumed by 0x006749a0 (12 rows, 12 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Starter {
    pub category: WeaponCategory,
    pub skill: u16,
    /// 0x00674a7f: a value of one selects category slot 3.
    pub secondary_slot: bool,
}

#[derive(Debug, Clone)]
pub struct StartingSkills {
    pub records: Vec<Starter>,
    /// Zero-terminated category lists through 0x018757c4, not the separate
    /// weapon-set filter at 0x0163e784 (which includes the mace alias).
    pub categories: [Vec<WeaponCategory>; 10],
}

impl StartingSkills {
    pub fn load(exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let read = || -> Result<Self, ddexe::Error> {
            let mut records = Vec::with_capacity(12);
            for row in 0..12 {
                let base = 0x0158_27e8 + row * 12;
                let category = exe.u32(base)? as usize;
                let skill = exe.u32(base + 4)?;
                let flag = exe.u8(base + 8)?;
                if category == 0 || category >= WeaponCategory::ALL.len() || skill >= 430 || flag > 1 {
                    return Err(ddexe::Error("invalid starting skill record".into()));
                }
                records.push(Starter {
                    category: WeaponCategory::ALL[category],
                    skill: skill as u16,
                    secondary_slot: flag == 1,
                });
            }
            let mut categories = std::array::from_fn(|_| Vec::new());
            for vocation in Vocation::ALL {
                let base = exe.u32(0x0187_57c4 + vocation.id() * 4)?;
                let mut terminated = false;
                for row in 0..13 {
                    let value = exe.u32(base + row * 4)? as usize;
                    if value == 0 {
                        terminated = true;
                        break;
                    }
                    if value >= WeaponCategory::ALL.len() {
                        return Err(ddexe::Error("invalid starting vocation weapon category".into()));
                    }
                    categories[vocation as usize].push(WeaponCategory::ALL[value]);
                }
                if !terminated {
                    return Err(ddexe::Error(
                        "unterminated starting vocation category list".into(),
                    ));
                }
            }
            Ok(Self { records, categories })
        };
        read().map_err(|e| e.to_string())
    }

    /// Translated from 0x006749a0/0x00674a60 (DDDA.exe 2017-12-13 Steam).
    /// Only an unknown skill is learned and placed. Switching back must
    /// preserve the character's customized slots, including empty slots.
    pub fn grant(&self, character: &mut Character) {
        let vocation = character.vocation;
        for &category in &self.categories[vocation as usize] {
            for row in self.records.iter().filter(|row| row.category == category) {
                // The native check excludes Anodyne for the two hybrid
                // staff vocations before checking the learned bits.
                if matches!(vocation, Vocation::MysticKnight | Vocation::MagickArcher) && row.skill == 220 {
                    continue;
                }
                if character.learned.weapon_level(row.skill) != 0 {
                    continue;
                }
                character.learned.level1.set(row.skill as usize, true);
                let slot = if row.secondary_slot { 3 } else { 0 };
                if !row.secondary_slot && row.skill == 210 {
                    character.weapon_sets[WeaponCategory::Staff as usize][0] = Some(row.skill);
                    character.weapon_sets[WeaponCategory::Archistaff as usize][0] = Some(row.skill);
                } else {
                    character.weapon_sets[category as usize][slot] = Some(row.skill);
                }
                let alias = match category {
                    WeaponCategory::Sword => Some(WeaponCategory::Mace),
                    WeaponCategory::Longsword => Some(WeaponCategory::Warhammer),
                    _ => None,
                };
                if let Some(alias) = alias {
                    character.weapon_sets[alias as usize][0] = Some(row.skill);
                }
            }
        }
    }
}
