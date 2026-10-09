//! The small fixed-record tables under `etc\item`: recipes (`rItemMix`),
//! item sets and drops (`rItemSetTbl`), curse conversion (`rItemCurseCnv`)
//! and `rItemRandParam`.

use crate::bin::Cursor;
use crate::Error;

/// `rItemMix` (`etc\item\itemMix`), loader `0x00cbff80`: magic `imx\0`,
/// version 4, records of four u32 named by `cItemMixData::vf03`
/// (`0x00cbfdb0`): `mBaseItemNo0`, `mBaseItemNo1`, `mNewItemNo`,
/// `mNewItemNum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    pub base0: u32,
    pub base1: u32,
    pub result: u32,
    pub count: u32,
}

pub fn parse_item_mix(data: &[u8]) -> Result<Vec<Recipe>, Error> {
    let mut c = Cursor::new(data);
    let n = c.header(b"imx\0", 4)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        out.push(Recipe {
            base0: c.u32()?,
            base1: c.u32()?,
            result: c.u32()?,
            count: c.u32()?,
        });
    }
    c.finish()?;
    Ok(out)
}

/// `rItemSetTbl` (`etc\item\itemSetTbl`, `ItemEmListSetTbl`), loader
/// `0x00cc04f0`: magic `ist\0`, version 0, records of 20 u16. Names from
/// `cParam::vf03` (`0x00cc0310`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemSet {
    pub set_id: u16,
    pub set_type: u16,
    pub get_count: u16,
    pub drop_count: u16,
    /// `mItemNo[8]`, `0xffff` = empty.
    pub items: [u16; 8],
    /// `mItemGetProbability[8]`.
    pub probability: [u16; 8],
}

pub fn parse_item_set(data: &[u8]) -> Result<Vec<ItemSet>, Error> {
    let mut c = Cursor::new(data);
    let n = c.header(b"ist\0", 0)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let set_id = c.u16()?;
        let set_type = c.u16()?;
        let get_count = c.u16()?;
        let drop_count = c.u16()?;
        let mut items = [0; 8];
        for v in &mut items {
            *v = c.u16()?;
        }
        let mut probability = [0; 8];
        for v in &mut probability {
            *v = c.u16()?;
        }
        out.push(ItemSet {
            set_id,
            set_type,
            get_count,
            drop_count,
            items,
            probability,
        });
    }
    c.finish()?;
    Ok(out)
}

/// `rItemCurseCnv` (`etc\item\itemCurse`), loader `0x0040e560`: magic
/// `cit\0`, version 0. A record is `mLevel`, 20 `mGetItemNo` and 20
/// `mGetItemRate` (names from `cParam::vf03`, `0x0040e430`). The loader
/// reads a rate only when its item number is below 1901 (`0x76d`), forces
/// the rate of an empty (`-1`) slot to 0, and turns the rates into running
/// sums in memory; [`CurseConversion::cumulative`] reproduces that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurseConversion {
    pub level: u16,
    pub items: [i16; 20],
    pub rates: [i16; 20],
}

pub fn parse_item_curse(data: &[u8]) -> Result<Vec<CurseConversion>, Error> {
    let mut c = Cursor::new(data);
    let n = c.header(b"cit\0", 0)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let level = c.u16()?;
        let mut items = [0i16; 20];
        for v in &mut items {
            *v = c.i16()?;
        }
        let mut rates = [0i16; 20];
        for (i, v) in rates.iter_mut().enumerate() {
            // Translated from 0x0040e560: `if (item < 0x76d) read rate`.
            if items[i] < 0x76d {
                *v = c.i16()?;
                if items[i] == -1 {
                    *v = 0;
                }
            }
        }
        out.push(CurseConversion {
            level,
            items,
            rates,
        });
    }
    c.finish()?;
    Ok(out)
}

impl CurseConversion {
    /// Sum of all rates (the loader stores it at +0x58).
    pub fn total(&self) -> i32 {
        self.rates.iter().map(|&r| r as i32).sum()
    }

    /// Running sums, as the loader leaves the rate array in memory.
    pub fn cumulative(&self) -> [i16; 20] {
        let mut out = self.rates;
        for i in 1..20 {
            out[i] = out[i].wrapping_add(out[i - 1]);
        }
        out
    }
}

/// `rItemRandParam` (`etc\item\itemRandParam`), loader `0x0040e980`: magic
/// `irp\0`, version 0, records of seven u32. `cParam` registers no
/// property names, so the fields are unnamed.
pub fn parse_item_rand(data: &[u8]) -> Result<Vec<[u32; 7]>, Error> {
    let mut c = Cursor::new(data);
    let n = c.header(b"irp\0", 0)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let mut r = [0u32; 7];
        for v in &mut r {
            *v = c.u32()?;
        }
        out.push(r);
    }
    c.finish()?;
    Ok(out)
}
