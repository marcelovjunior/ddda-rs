//! Saved/story pawn base stats and equipment, before runtime modifiers.
//!
//! This is a construction adapter, not a Fighter kit or an AI driver. The
//! saved record retains vocation, learned/equipped skills, appearance and
//! inclinations. Body construction, augments, active statuses, equipment
//! skill overrides and Mage input are not applied by this module.

use dditem::forge::{level_stat, level_table, LevelTable};
use dditem::itl::{ItemList, ItemRecord};
use dditem::lvparam::ItemLevelParam;
use ddpawn::cmc::{Cmc, Equip};

#[derive(Debug, Clone, PartialEq)]
pub struct EquipmentStats {
    /// Main/offhand, each [physical, magick]. No runtime attack multipliers.
    pub weapon: [[f32; 2]; 2],
    pub defence: [f32; 2],
    /// Percent remaining after equipment resistance, starting from 100.
    pub slash_percent: f32,
    pub strike_percent: f32,
    /// None, fire, ice, thunder, holy, dark. Signed percentages are retained.
    pub element_resistance: [f32; 6],
    /// Active-info +0x640/+0x6a0, decoded from equipped item instance data.
    /// Defence/element contributions are included above; other consumers
    /// (HP, attack, stamina and status resistance) remain separate.
    pub accessory: [[f32; 24]; 2],
}

impl Default for EquipmentStats {
    fn default() -> Self {
        Self {
            weapon: [[0.0; 2]; 2],
            defence: [0.0; 2],
            slash_percent: 100.0,
            strike_percent: 100.0,
            element_resistance: [0.0; 6],
            accessory: [[0.0; 24]; 2],
        }
    }
}

/// An owned snapshot for one host-assigned actor. No shared mutable pools.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedPawnBase {
    pub owner: u32,
    pub record: Cmc,
    /// Resolved active equipment slots; last eligible saved entry wins.
    pub equipped: [Option<Equip>; 12],
    pub equipment: EquipmentStats,
}

impl SavedPawnBase {
    /// Learned level observed by 0x00780ba0. Gear-provided level-three
    /// bits (active skill state +0x74) are runtime inputs, not saved flags.
    /// `ignore_gear` is the original function's argument. This does not
    /// decide whether the actor can currently execute the skill.
    pub fn skill_level(
        &self,
        tables: &ddskill::catalogue::ExeTables,
        skill: u32,
        gear_levels: &[u32; 14],
        ignore_gear: bool,
    ) -> Result<u8, String> {
        if skill == u32::MAX {
            return Ok(0);
        }
        let row = tables
            .skills
            .get(skill as usize)
            .filter(|_| skill < 430)
            .ok_or_else(|| format!("skill {skill} outside installed skill table"))?;
        if row.text == -1 {
            return Ok(0);
        }
        let text = usize::try_from(row.text)
            .ok()
            .and_then(|index| tables.texts.get(index))
            .ok_or_else(|| format!("skill {skill}: invalid text metadata {}", row.text))?;
        let has = |bits: &[u32; 14]| bits[skill as usize / 32] & (1 << (skill % 32)) != 0;
        Ok(learned_level(
            text.flags & 0x8000 != 0,
            has(&self.record.param.skill_lv1),
            has(&self.record.param.skill_lv2),
            has(gear_levels),
            ignore_gear,
        ))
    }

    /// Baseline attack inputs, before augments, enchantment, special
    /// equipment effects, critical capability and status multipliers.
    pub fn base_attacker(&self, offhand: bool) -> ddcombat::damage::PlayerAttacker {
        let weapon = self.equipment.weapon[usize::from(offhand)];
        ddcombat::damage::PlayerAttacker::new(
            self.record.param.attack,
            self.record.param.mgc_attack,
            weapon[0],
            weapon[1],
        )
    }

    /// Baseline defence inputs, before the actor's ability/difficulty,
    /// guard and status modifiers. Saved growth is retained verbatim.
    pub fn base_defender(&self) -> ddcombat::damage::PlayerDefender {
        let mut out =
            ddcombat::damage::PlayerDefender::new(self.record.param.defend, self.record.param.mgc_defend);
        out.equip_defence = self.equipment.defence[0];
        out.equip_mgc_defence = self.equipment.defence[1];
        out.slash_pct = self.equipment.slash_percent;
        out.strike_pct = self.equipment.strike_percent;
        out.element_resist_pct = self.equipment.element_resistance;
        out
    }
}

// Translated from 0x00780ba0 with bit getters 0x0077c390/0x0077c400
// (DDDA.exe 2017-12-13 Steam). Core skills accept either learned bit;
// level-three gear requires at least one learned bit and a weapon skill.
fn learned_level(weapon: bool, first: bool, second: bool, gear: bool, ignore_gear: bool) -> u8 {
    if !weapon {
        u8::from(first || second)
    } else if gear && !ignore_gear {
        if first || second {
            3
        } else {
            0
        }
    } else if second {
        2
    } else {
        u8::from(first)
    }
}

/// Range-record tier chosen by 0x00542620 (2017-12-13 Steam): missing
/// player-info or levels zero/one select Lv1, two selects Lv2, three LvEX.
/// This only selects a parameter record; it does not grant a skill.
pub fn ai_parameter_tier(observed_skill_level: Option<u8>) -> usize {
    match observed_skill_level {
        Some(3..) => 2,
        Some(2) => 1,
        _ => 0,
    }
}

pub struct EquipmentData {
    pub items: ItemList,
    pub weapon_levels: ItemLevelParam,
    pub armour_levels: ItemLevelParam,
    pub accessory_levels: ItemLevelParam,
    pub exclusions: ItemExclusions,
    defence_slots: Vec<usize>,
}

/// Item-ID exclusion predicate used by the combat-stat getters. The bulk
/// switch maps remain in the player's executable; they are not copied here.
pub struct ItemExclusions {
    excluded: Vec<bool>,
}

impl ItemExclusions {
    /// Translated switch dispatch from 0x004647e0 (2017-12-13 Steam).
    pub fn load(exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let mut excluded = vec![false; 0x76d];
        // First ID, count, byte-index map, two-target jump table. These
        // addresses and bounds are operands of the source's dispatch code.
        for (first, count, map, jumps) in [
            (0x47, 0xe6, 0x00464900, 0x004648f8),
            (0x192, 0xed, 0x004649f0, 0x004649e8),
            (0x29c, 0xde, 0x00464ae8, 0x00464ae0),
            (0x3c1, 0xf3, 0x00464bd0, 0x00464bc8),
            (0x57c, 0xa9, 0x00464ccc, 0x00464cc4),
            (0x6b1, 0x83, 0x00464d80, 0x00464d78),
            (0x736, 0x0b, 0x00464e0c, 0x00464e04),
        ] {
            let targets = exe.u32s(jumps, 2)?;
            for (index, selector) in exe.bytes(map, count)?.iter().enumerate() {
                let target = targets
                    .get(usize::from(*selector))
                    .ok_or_else(|| format!("item exclusion map {map:#x}: invalid selector {selector}"))?;
                excluded[first + index] = match *target {
                    0x004648d1 => true,
                    // The first six ranges are below the final fallback
                    // range0x736..0x740, so its bounds reject them.
                    0x004648d4 if first < 0x736 => false,
                    0x004648f3 => false,
                    _ => return Err(format!("item exclusion map {map:#x}: unknown branch {target:#x}")),
                };
            }
        }
        // Equality branches encoded directly in 0x004647e0.
        for id in [0x17d, 0x298, 0x3a0, 0x53a, 0x6a4] {
            excluded[id] = true;
        }
        Ok(Self { excluded })
    }

    /// The source compares signed item IDs. Negative IDs are not excluded
    /// by this predicate (callers reject them separately); IDs >=0x76d are.
    pub fn contains(&self, item: i32) -> bool {
        if item >= 0x76d {
            true
        } else {
            usize::try_from(item)
                .ok()
                .and_then(|id| self.excluded.get(id))
                .copied()
                .unwrap_or(false)
        }
    }
}

impl EquipmentData {
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>) -> Result<Self, String> {
        let read = |path, class| vfs.read(path, mtarc::type_hash(class));
        let levels = |path| ItemLevelParam::parse(&read(path, "rItemLevelParam")?).map_err(|e| e.to_string());
        // 0x00b87450 walks nine words at 0x015ea2a8. Read the selector
        // from the installed executable rather than embedding its data.
        let defence_slots = exe
            .u32s(0x015e_a2a8, 9)?
            .into_iter()
            .map(|slot| usize::try_from(slot).map_err(|_| "equipment slot overflow".to_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        if defence_slots.iter().any(|&slot| slot >= 12) {
            return Err("installed defence slot outside 12 equipment slots".into());
        }
        Ok(Self {
            items: ItemList::parse(&read(r"etc\item\itemList", "rItemList")?).map_err(|e| e.to_string())?,
            weapon_levels: levels(r"etc\item\LvParamWepon")?,
            armour_levels: levels(r"etc\item\LvParamArmor")?,
            accessory_levels: levels(r"etc\item\LvParamAccessory")?,
            exclusions: ItemExclusions::load(exe)?,
            defence_slots,
        })
    }

    fn stat(&self, item: &ItemRecord, equip: &Equip, base: f32, stat: i32) -> f32 {
        let table = match level_table(item.kind()) {
            Some(LevelTable::Weapon) => Some(&self.weapon_levels),
            Some(LevelTable::Armor) => Some(&self.armour_levels),
            Some(LevelTable::Accessory) => Some(&self.accessory_levels),
            None => None,
        };
        let row = table.and_then(|t| t.rows.get(usize::from(item.level_up_type())));
        level_stat(
            row,
            base,
            stat,
            u32::from(dditem::rules::instance_level(equip.flag)),
        )
    }

    /// Copy saved base values without reconstructing growth from a level row
    /// (0x00446a30). Equipment setup follows 0x00446860/0x00459340.
    ///
    /// `unavailable` supplies an explicit exclusion observation. Use
    /// `convert_installed` for the sourced installed predicate0x004647e0.
    /// No body, combat Fighter, bag replacement or AI fallback is created.
    pub fn convert(
        &self,
        owner: u32,
        record: &Cmc,
        mut unavailable: impl FnMut(u32) -> bool,
    ) -> Result<SavedPawnBase, String> {
        let mut equipped = [None; 12];
        for equip in &record.param.equip {
            if equip.flag & 1 == 0 || equip.item_no < 0 {
                continue;
            }
            let Some(item) = self.items.get(equip.item_no as u32) else {
                continue;
            };
            // Translated from 0x00459340; 0x00446860 overrides flag 0x800.
            let slot = if equip.flag & 0x800 != 0 {
                10
            } else {
                equipment_slot(item.kind())
            };
            equipped[slot] = Some(*equip);
        }
        let mut equipment = EquipmentStats::default();
        for (slot, equip) in equipped.iter().enumerate() {
            let Some(equip) = equip else {
                continue;
            };
            let item = self.items.get(equip.item_no as u32).expect("validated item");
            let excluded = unavailable(equip.item_no as u32);
            if !excluded && slot < 2 {
                // Item getters 0x004599a0/0x00459a60 (2017-12-13 Steam).
                equipment.weapon[slot] = [
                    self.stat(item, equip, item.attack() as f32, 0),
                    self.stat(item, equip, item.magic_attack() as f32, 1),
                ];
            }
            if !excluded && self.defence_slots.contains(&slot) {
                // 0x0045a1b0/0x0045a270, then 0x0045a330..0x0045a6f0.
                let packed = u32::from_le_bytes(item.raw[4..8].try_into().unwrap());
                equipment.defence[0] += self.stat(item, equip, (packed & 0x3ff) as f32, 13);
                equipment.defence[1] += self.stat(item, equip, ((packed >> 10) & 0x3ff) as f32, 14);
                equipment.slash_percent -= self.stat(item, equip, item.raw[0x16] as i8 as f32, 15);
                equipment.strike_percent -= self.stat(item, equip, item.raw[0x17] as i8 as f32, 16);
                for element in 1..=5 {
                    equipment.element_resistance[element] += self.stat(
                        item,
                        equip,
                        item.raw[0x17 + element] as i8 as f32,
                        16 + element as i32,
                    );
                }
            }
            // 0x00442d50: bit31 instance bonuses in slots9/10. These
            // assignments are separate from the unavailable-item skill check.
            if item.flags() & 0x8000_0000 != 0 && matches!(slot, 9 | 10) {
                let indices = equip.extra[0].to_le_bytes();
                for (index, value) in indices.into_iter().zip([equip.extra[1], equip.extra[2]]) {
                    let field = equipment.accessory[slot - 9]
                        .get_mut(usize::from(index))
                        .ok_or_else(|| {
                            format!("item {}: accessory field {index} outside 24", equip.item_no)
                        })?;
                    *field = f32::from(value);
                }
            }
        }
        // 0x00b87940 adds both accessory blocks, then 0x00b87450 caps
        // element resistance above at100. It does not clamp negatives.
        for block in &equipment.accessory {
            equipment.defence[0] += block[4];
            equipment.defence[1] += block[5];
            for element in 1..=5 {
                equipment.element_resistance[element] += block[7 + element];
            }
        }
        for value in &mut equipment.element_resistance {
            if *value > 100.0 {
                *value = 100.0;
            }
        }
        for value in &mut equipment.defence {
            if *value < 0.0 {
                *value = 0.0;
            }
        }
        Ok(SavedPawnBase {
            owner,
            record: record.clone(),
            equipped,
            equipment,
        })
    }

    /// Build baseline stats using the installed exclusion switch tables.
    pub fn convert_installed(&self, owner: u32, record: &Cmc) -> Result<SavedPawnBase, String> {
        self.convert(owner, record, |item| self.exclusions.contains(item as i32))
    }
}

// Translated from 0x00459340 (DDDA.exe 2017-12-13 Steam).
fn equipment_slot(kind: u32) -> usize {
    match kind {
        7 | 8 | 11 | 12 | 13 | 17 | 18 => 0,
        9 | 10 | 14 | 15 | 16 => 1,
        19 => 2,
        20 => 3,
        21 => 4,
        22 | 27 => 5,
        23 => 6,
        24 => 7,
        25 => 8,
        _ => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dditem::lvparam::{LevelEntry, LevelRow, ENTRIES};

    #[test]
    fn learned_levels_require_learning_even_with_gear_and_select_ai_tiers() {
        for first in [false, true] {
            for second in [false, true] {
                for gear in [false, true] {
                    for ignore in [false, true] {
                        assert_eq!(
                            learned_level(false, first, second, gear, ignore),
                            u8::from(first || second)
                        );
                    }
                }
            }
        }
        assert_eq!(learned_level(true, false, false, true, false), 0);
        assert_eq!(learned_level(true, true, false, true, false), 3);
        assert_eq!(learned_level(true, false, true, true, false), 3);
        assert_eq!(learned_level(true, true, false, true, true), 1);
        assert_eq!(learned_level(true, false, true, true, true), 2);
        assert_eq!(learned_level(true, true, true, false, false), 2);
        assert_eq!(
            [None, Some(0), Some(1), Some(2), Some(3)].map(ai_parameter_tier),
            [0, 0, 0, 1, 2]
        );
    }

    fn fixture() -> EquipmentData {
        let mut items = ItemList {
            records: vec![ItemRecord { raw: [0; 128] }; 4],
        };
        items.records[0].raw[0..4].copy_from_slice(&(50u32 | (7 << 11)).to_le_bytes());
        items.records[0].raw[0x38] = 7; // main weapon
        items.records[1].raw[0x38] = 19; // head armour, slot2
        items.records[1].raw[4..8].copy_from_slice(&(25u32 | (30 << 10)).to_le_bytes());
        items.records[1].raw[0x16] = 10;
        items.records[1].raw[0x17] = (-15i8) as u8;
        items.records[1].raw[0x18] = 120;
        items.records[1].raw[0x19] = (-20i8) as u8;
        items.records[2].raw[0x38] = 26; // accessory slot9
        items.records[2].raw[0x58..0x5c].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        items.records[3].raw[0x38] = 9; // offhand
        items.records[3].raw[0..4].copy_from_slice(&11u32.to_le_bytes());
        let empty = ItemLevelParam { rows: vec![] };
        let mut entries = [LevelEntry {
            kind: -1,
            rate: 0.0,
            direct: false,
        }; ENTRIES];
        entries[0] = LevelEntry {
            kind: 0,
            rate: 2.0,
            direct: false,
        };
        entries[2] = LevelEntry {
            kind: 0,
            rate: 3.0,
            direct: true,
        };
        EquipmentData {
            items,
            weapon_levels: ItemLevelParam {
                rows: vec![LevelRow { entries }],
            },
            armour_levels: empty.clone(),
            accessory_levels: empty,
            exclusions: ItemExclusions {
                excluded: vec![false; 0x76d],
            },
            defence_slots: (2..=10).collect(),
        }
    }

    #[test]
    fn saved_growth_equipment_slots_and_enhancement_are_independent() {
        let data = fixture();
        let mut record = Cmc::initial();
        record.param.job = 3;
        record.param.level = 47;
        record.param.attack = 123.5;
        record.param.mgc_attack = 321.25;
        record.param.defend = 87.5;
        record.param.mgc_defend = 76.25;
        record.param.hp = 19.0;
        record.param.hp_max_white = 40.0;
        record.param.hp_max = 80.0;
        // Saved entry order differs from resolved equipment slots.
        record.param.equip[5] = Equip {
            num: 1,
            item_no: 0,
            flag: 0x13,
            extra: [0; 4],
        };
        record.param.equip[6] = Equip {
            num: 1,
            item_no: 1,
            flag: 3,
            extra: [0; 4],
        };
        record.param.equip[7] = Equip {
            num: 1,
            item_no: 3,
            flag: 3,
            extra: [0; 4],
        };
        let first = data.convert(4, &record, |_| false).unwrap();
        assert_eq!(first.record, record);
        assert_eq!(first.equipment.weapon, [[53.0, 7.0], [11.0, 0.0]]);
        assert_eq!(first.equipment.defence, [25.0, 30.0]);
        assert_eq!(
            (first.equipment.slash_percent, first.equipment.strike_percent),
            (90.0, 115.0)
        );
        assert_eq!(&first.equipment.element_resistance[1..3], &[100.0, -20.0]);
        assert_eq!(first.base_attacker(false).base_attack, 123.5);
        assert_eq!(first.base_attacker(false).base_mgc_attack, 321.25);
        assert_eq!(first.base_defender().base_defence, 87.5);
        assert_eq!(first.base_defender().base_mgc_defence, 76.25);
        let mut second = data.convert(37, &record, |_| false).unwrap();
        second.record.param.hp = 0.0;
        assert_eq!(first.record.param.hp, 19.0);
        assert_eq!(first.owner, 4);
        assert_eq!(second.owner, 37);
        let excluded = data.convert(4, &record, |_| true).unwrap();
        assert_eq!(excluded.equipment, EquipmentStats::default());
        assert_eq!(excluded.record, record);
    }

    #[test]
    fn accessory_assignment_and_later_slot_replacement_follow_saved_order() {
        let data = fixture();
        let mut record = Cmc::initial();
        record.param.equip[0] = Equip {
            num: 1,
            item_no: 2,
            flag: 3,
            extra: [0x0504, 12, 8, 0],
        };
        record.param.equip[1] = Equip {
            num: 1,
            item_no: 2,
            flag: 0x803,
            extra: [0x0804, 7, 30, 0],
        };
        // Last eligible entry replaces ring1, not adds to its old instance.
        record.param.equip[2] = Equip {
            num: 1,
            item_no: 2,
            flag: 3,
            extra: [0x0404, 999, 3, 0],
        };
        record.param.equip[3] = Equip {
            num: 1,
            item_no: 1,
            flag: 0,
            extra: [0; 4],
        };
        record.param.equip[4] = Equip {
            num: 1,
            item_no: i16::MAX,
            flag: 1,
            extra: [0; 4],
        };
        let value = data.convert(1, &record, |_| false).unwrap();
        assert_eq!(value.equipment.defence, [10.0, 0.0]);
        assert_eq!(value.equipment.element_resistance[1], 30.0);
        assert_eq!(value.equipment.accessory[0][4], 3.0);
        record.param.equip[2].extra[0] = 0x0418;
        assert!(data
            .convert(1, &record, |_| false)
            .unwrap_err()
            .contains("outside 24"));
    }
}
