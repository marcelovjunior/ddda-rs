//! `rEquipLvUp` (`etc\item\equipLvUp`): what the smith charges to enhance
//! one item, level by level.
//!
//! Loader: `rEquipLvUp::vf10` at `0x00cb8a40` (magic `qlv\0`, version 3).
//! Field names from `rEquipLvUp::cLvupData::vf03` at `0x00cb8440`:
//! `mItemNo`, `Lv1_Gold`, `Lv2_Gold`/`Lv2_0`/`Lv2_0_num`, `Lv3_*`, `Lv5_*`,
//! `Lv6_*`. There is no `LV4` group and level 1 has no material: the
//! loader initialises the level-1 material slot (+0xc in memory) to -1 and
//! never reads it from the file.
//!
//! File record, 40 bytes:
//! `u16 item, u16 (unnamed), u32 lv1_gold,`
//! then for levels 2, 3, 5, 6: `u32 gold, u16 material, u16 count`.

use crate::bin::Cursor;
use crate::Error;

pub const MAGIC: &[u8; 4] = b"qlv\0";
pub const VERSION: u32 = 3;

/// The enhancement levels the table prices, in file order.
pub const LEVELS: [u8; 5] = [1, 2, 3, 5, 6];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquipLvUp {
    pub rows: Vec<LvUpRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LvUpRow {
    pub item: u16,
    /// The u16 after `mItemNo`; no property name, zero in the shipped table
    /// (install test).
    pub unnamed: u16,
    /// Cost of each level in [`LEVELS`] order.
    pub steps: [LvUpStep; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LvUpStep {
    pub gold: u32,
    /// Material item number, or `None` (level 1, and `0xffff` entries).
    pub material: Option<u16>,
    pub material_count: u16,
}

impl EquipLvUp {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut c = Cursor::new(data);
        let count = c.header(MAGIC, VERSION)? as usize;
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let item = c.u16()?;
            let unnamed = c.u16()?;
            let lv1 = LvUpStep {
                gold: c.u32()?,
                material: None,
                material_count: 0,
            };
            let mut steps = [lv1; 5];
            for step in steps.iter_mut().skip(1) {
                let gold = c.u32()?;
                let material = c.u16()?;
                let material_count = c.u16()?;
                *step = LvUpStep {
                    gold,
                    material: (material != 0xffff).then_some(material),
                    material_count,
                };
            }
            rows.push(LvUpRow {
                item,
                unnamed,
                steps,
            });
        }
        c.finish()?;
        Ok(Self { rows })
    }

    pub fn row(&self, item: u16) -> Option<&LvUpRow> {
        self.rows.iter().find(|r| r.item == item)
    }
}

impl LvUpRow {
    /// The cost of reaching `level` (1, 2, 3, 5 or 6).
    pub fn step(&self, level: u8) -> Option<&LvUpStep> {
        LEVELS
            .iter()
            .position(|&l| l == level)
            .map(|i| &self.steps[i])
    }
}
