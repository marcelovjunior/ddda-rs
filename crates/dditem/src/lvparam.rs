//! `rItemLevelParam` (`etc\item\LvParamWepon`, `LvParamArmor`,
//! `LvParamAccessory`): per-row stat changes for enhancement levels 1-6.
//!
//! Loader: `rItemLevelParam::vf10` at `0x00cbd280` (magic `ite\0`, version
//! `0x1400`). A file row is 36 u32 stat kinds, 36 f32 rates and 36 u32
//! booleans (direct value), each grouped by level as 2, 4, 6, 8, 8, 8
//! entries for levels 1..6: 432 bytes. Rows are selected by the item's
//! `mLevelUpType`, the table by its `mKind` (`0x0045b620`, see SHOPS.md).

use crate::bin::Cursor;
use crate::Error;

pub const MAGIC: &[u8; 4] = b"ite\0";
pub const VERSION: u32 = 0x1400;
/// Entries per level, levels 1..=6 (loop counts in `0x00cbd280`).
pub const PER_LEVEL: [usize; 6] = [2, 4, 6, 8, 8, 8];
pub const ENTRIES: usize = 36;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemLevelParam {
    pub rows: Vec<LevelRow>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelEntry {
    /// Stat kind, -1 for an empty entry.
    pub kind: i32,
    pub rate: f32,
    /// Add `rate` instead of multiplying by it.
    pub direct: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LevelRow {
    pub entries: [LevelEntry; ENTRIES],
}

impl ItemLevelParam {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut c = Cursor::new(data);
        let count = c.header(MAGIC, VERSION)? as usize;
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let empty = LevelEntry {
                kind: -1,
                rate: 0.0,
                direct: false,
            };
            let mut entries = [empty; ENTRIES];
            for e in entries.iter_mut() {
                e.kind = c.i32()?;
            }
            for e in entries.iter_mut() {
                e.rate = c.f32()?;
            }
            for e in entries.iter_mut() {
                e.direct = c.u32()? != 0;
            }
            rows.push(LevelRow { entries });
        }
        c.finish()?;
        Ok(Self { rows })
    }
}

impl LevelRow {
    /// The entries of enhancement level `level` (1..=6).
    pub fn level(&self, level: usize) -> &[LevelEntry] {
        assert!((1..=6).contains(&level));
        let start: usize = PER_LEVEL[..level - 1].iter().sum();
        &self.entries[start..start + PER_LEVEL[level - 1]]
    }
}
