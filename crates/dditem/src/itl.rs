//! `rItemList` (`etc\item\itemList`): one 128-byte record per item.
//!
//! Loader: `rItemList::vf10` at `0x00cbfb30` checks magic `ITL2` and stamp
//! `0x01330611`, reads the count, skips one u32 and copies `count * 0x80`
//! bytes verbatim. The game indexes the table directly by item number
//! (`id << 7` + table, e.g. `0x004621f6`, `0x006e15xx`), so record N is
//! item N. Field accessors below name only what an exe read confirms; the
//! rest stays in [`ItemRecord::raw`].

use crate::bin::Cursor;
use crate::Error;

pub const MAGIC: &[u8; 4] = b"ITL2";
pub const STAMP: u32 = 0x0133_0611;
pub const RECORD_SIZE: usize = 0x80;

#[derive(Debug, Clone)]
pub struct ItemList {
    pub records: Vec<ItemRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemRecord {
    pub raw: [u8; RECORD_SIZE],
}

impl ItemList {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut c = Cursor::new(data);
        let count = c.header(MAGIC, STAMP)? as usize;
        let _reserved = c.u32()?;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(ItemRecord {
                raw: c.bytes(RECORD_SIZE)?.try_into().unwrap(),
            });
        }
        c.finish()?;
        Ok(Self { records })
    }

    pub fn get(&self, item: u32) -> Option<&ItemRecord> {
        self.records.get(item as usize)
    }
}

impl ItemRecord {
    fn u32_at(&self, off: usize) -> u32 {
        u32::from_le_bytes(self.raw[off..off + 4].try_into().unwrap())
    }

    /// Low 13 bits of +0x3c: equals the record index on every record of the
    /// shipped table (checked by the install test).
    pub fn id(&self) -> u32 {
        self.u32_at(0x3c) & 0x1fff
    }

    /// `mKind` (+0x38 bits 0-4): selects the enhancement stat table in
    /// `0x0045b620`; copied as the article kind by the shop stock builders
    /// (`0x006e14b0`, `0x006e17c0`).
    pub fn kind(&self) -> u32 {
        self.u32_at(0x38) & 0x1f
    }

    /// +0x38 bits 5-9, copied by the stock builders into the article's
    /// sort group (`0x006e14b0`: `>> 5 & 0x1f`).
    pub fn category_type(&self) -> u32 {
        (self.u32_at(0x38) >> 5) & 0x1f
    }

    /// +0x38 bits 10-11 (`0x006e14b0`: `>> 10 & 3`).
    pub fn category_kind(&self) -> u32 {
        (self.u32_at(0x38) >> 10) & 3
    }

    /// `mLevelUpType` (u16 at +0x34): row of the kind's level-stat table
    /// (`0x0045b65b`).
    pub fn level_up_type(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x34], self.raw[0x35]])
    }

    /// +0x40: equipment model number.
    pub fn equip_model(&self) -> u32 {
        self.u32_at(0x40)
    }

    /// Weight (f32 at +0x44; the shop keeps it as `weight * 1000`).
    pub fn weight(&self) -> f32 {
        f32::from_bits(self.u32_at(0x44))
    }

    /// Base buy price (u32 at +0x48), read by the shop buy list
    /// `0x006e14b0` and the buy-back list `0x006e1b40`.
    pub fn buy_price(&self) -> u32 {
        self.u32_at(0x48)
    }

    /// Base sell price (u32 at +0x4c), read by the shop sell list
    /// `0x006e17c0`. For rift-point items (flag bit 26) the item-use handler
    /// `0x0045f250` grants this value as Rift Crystals.
    pub fn sell_price(&self) -> u32 {
        self.u32_at(0x4c)
    }

    /// Flag word at +0x58. Bits confirmed in the exe: 25 (`0x0045f250`, use
    /// adds gold), 26 (use adds `sell_price` rift points), 29-31 (the
    /// instance-data packing in `0x00462840`).
    pub fn flags(&self) -> u32 {
        self.u32_at(0x58)
    }
}
