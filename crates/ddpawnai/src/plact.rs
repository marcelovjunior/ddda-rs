//! `rAIPlayerActionParameter` (`AI\AIPlayerActionParameter\AIPlActParam*`):
//! the ranges an AI-driven player character (a pawn) attacks from.
//!
//! 0x0047ED00 picks the bank by command id (0..9 Unarm, 0xA..0x45 the
//! sword bank, 0x46..0x77, 0x78..0xB3, ...; banks at sPlayerManager
//! +0x8D0.., array at +0x70) and the record `command - bank start`, then
//! tier `0..=2` (Lv1, Lv2, LvEX) as 0x24 bytes from record +4; 0x00542620
//! copies it to the unit (+0x258..+0x27C).

use crate::{float, int, list};

/// One tier of a record (0x24 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    /// `RangeMinXZ` / `RangeMaxXZ`: horizontal distance to the target, cm.
    pub min_xz: f32,
    pub max_xz: f32,
    /// `RangeMinY` / `RangeMaxY`.
    pub min_y: f32,
    pub max_y: f32,
    pub enable_min_xz: f32,
    pub enable_max_xz: f32,
    pub element: u32,
    pub atk: u32,
    pub use_attr: u32,
}

/// `cAIPlayerActionParameter`: the three tiers of one command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Record {
    pub tiers: [Range; 3],
}

/// The sword bank's first command (0x0047ED00: commands 0xA..=0x45).
pub const SWORD_BANK_START: u32 = 0xA;
/// The sword bank's resource. *Guessed*: the bank at +0x8D4 is the second
/// of the `AIPlActParam*` names in string order (Unarm, Sword, ...).
pub const SWORD_BANK: &str = r"AI\AIPlayerActionParameter\AIPlActParamSword";

pub fn parse(bytes: &[u8]) -> Result<Vec<Record>, String> {
    let x = mtxfs::parse(bytes).map_err(|e| format!("{e:?}"))?;
    let r = &x.root;
    if crate::class(&x, r) != "rAIPlayerActionParameter" {
        return Err(format!("root is {}", crate::class(&x, r)));
    }
    list(&x, r, "mArray")?
        .into_iter()
        .map(|o| {
            let tier = |t: &str| -> Result<Range, String> {
                Ok(Range {
                    min_xz: float(&x, o, &format!("RangeMinXZ_{t}"))?,
                    max_xz: float(&x, o, &format!("RangeMaxXZ_{t}"))?,
                    min_y: float(&x, o, &format!("RangeMinY_{t}"))?,
                    max_y: float(&x, o, &format!("RangeMaxY_{t}"))?,
                    enable_min_xz: float(&x, o, &format!("EnableMinXZ_{t}"))?,
                    enable_max_xz: float(&x, o, &format!("EnableMaxXZ_{t}"))?,
                    element: int(&x, o, &format!("ElementAttr_{t}"))? as u32,
                    atk: int(&x, o, &format!("AtkAttr_{t}"))? as u32,
                    use_attr: int(&x, o, &format!("UseAtr_{t}"))? as u32,
                })
            };
            Ok(Record {
                tiers: [tier("Lv1")?, tier("Lv2")?, tier("LvEX")?],
            })
        })
        .collect()
}
