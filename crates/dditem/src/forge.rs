//! Enhancement, dragonforging, item stats per level, Bitterblack curse
//! purification, and the augment name table, traced from DDDA.exe
//! (2017-12-13 Steam). Pure functions; callers supply party and save state.

use crate::lvparam::LevelRow;
use crate::lvup::{LvUpRow, LvUpStep};
use crate::rng::XorShift;
use crate::rules::Currency;
use crate::tables::CurseConversion;

/// Which level-stat table an item kind uses (`0x0045b620` switch on
/// `mKind`): 7-18 weapons, 19-24 and 27 armour, 25 accessories; any other
/// kind has no level stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelTable {
    Weapon,
    Armor,
    Accessory,
}

pub fn level_table(kind: u32) -> Option<LevelTable> {
    match kind {
        7..=18 => Some(LevelTable::Weapon),
        19..=24 | 27 => Some(LevelTable::Armor),
        25 => Some(LevelTable::Accessory),
        _ => None,
    }
}

/// A stat at an enhancement level. Stats are never stored on the item:
/// every getter recomputes them from the base value with this.
///
/// Translated from `0x0045b620`: for levels 1..=`level`, every entry of
/// the row whose kind equals `stat` sets the result to `base * rate`, or
/// `base + rate` when the entry is direct (`MULSS` / `ADDSS`, single
/// precision, `base` kept unchanged in xmm1); the last match wins; no match
/// (or no row, or level 0) leaves `base`. A level above 6 also returns
/// `base`. Entry accessors `0x00cbf8b0` / `0x00cbf910` / `0x00cbf990` use
/// the 2/4/6/8/8/8 grouping of [`LevelRow::level`].
pub fn level_stat(row: Option<&LevelRow>, base: f32, stat: i32, level: u32) -> f32 {
    let Some(row) = row else { return base };
    if level == 0 || level > 6 {
        return base;
    }
    let mut v = base;
    for l in 1..=level as usize {
        for e in row.level(l) {
            if e.kind == stat {
                v = if e.direct { e.rate + base } else { e.rate * base };
            }
        }
    }
    v
}

/// The cost row step for a target level: index `target - 1` for 1..=3,
/// `target - 2` for 5 and 6 (`0x006f3520`, `0x006f3350`).
pub fn enhance_step(row: &LvUpRow, target: u8) -> Option<&LvUpStep> {
    let i = match target {
        1..=4 => target as usize - 1,
        5 | 6 => target as usize - 2,
        _ => return None,
    };
    row.steps.get(i)
}

/// What one enhancement charges: currency and amount, and the material.
///
/// Translated from `0x006f3520`: materials only for targets above 1; gold
/// for targets below 4 (subtracted directly, clamped at 0, no ledger
/// entry), Rift Crystals otherwise (`0x004433f0`).
/// `(currency, amount, Some((material, count)))`.
pub type Charge = (Currency, u32, Option<(u16, u16)>);

pub fn enhance_charge(row: &LvUpRow, target: u8) -> Option<Charge> {
    let s = enhance_step(row, target)?;
    let cur = if target < 4 {
        Currency::Gold
    } else {
        Currency::RiftCrystals
    };
    let mat = if target > 1 {
        s.material.map(|m| (m, s.material_count))
    } else {
        None
    };
    Some((cur, s.gold, mat))
}

/// Where the materials are taken from (`0x006f35f0`). `usable[i]` is the
/// material count of the upgrade screen's owner `i` (party bags in screen
/// order, storage last), already reduced by one for a stack whose slot
/// flag bit 1 or bit 8 is set. Bags are drained from the second-to-last
/// owner down to the first, then storage. Returns `(owner, taken)` pairs.
/// Consumed Wyrmking's Rings (item 229) are also added to the buy-back
/// (Black Cat) list, `0x00461f30`.
pub fn take_materials(usable: &[u32], mut need: u32) -> Vec<(usize, u32)> {
    let mut out = Vec::new();
    if need == 0 || usable.is_empty() {
        return out;
    }
    let storage = usable.len() - 1;
    for owner in (0..storage).rev() {
        let have = usable[owner];
        if have == 0 {
            continue;
        }
        let t = have.min(need);
        out.push((owner, t));
        need -= t;
        if need == 0 {
            return out;
        }
    }
    let t = usable[storage].min(need);
    if t > 0 {
        out.push((storage, t));
    }
    out
}

/// Dragonforge base chance in percent for an enemy kind (`uEnemy` byte
/// `+0x2d`, set by each `uEmNNNN` constructor) and the item's current
/// level 0..=3. Translated from `0x0045ce50`; tables at `0x01539d34..`.
///
/// | kind | enemy (class, game text) | chance by level 0/1/2/3 |
/// |---|---|---|
/// | 0x5a | The Dragon (`uEm5800`) | 100 (scenario only, see SHOPS.md) |
/// | 0x5b | The Ur-Dragon (`uEm5801`) | 100, or 0/2/10/30 when `ur_dragon_limited` |
/// | 0x5c-0x5e | Drakes, Wyrms, Wyverns (`uEm5900-5902`) | 0/2/6/18 |
/// | 0xb1-0xb3 | Dire Drakes, Dire Wyrms (`uEm5903/5904`) | 5/15/25/40 |
/// | 0xb4 | Cursed Dragons (`uEm5906`) | 40/55/65/75 |
///
/// `ur_dragon_limited` is `[0x018fa504]+0x15a4c` or `sGameSys+0xb8824`
/// being set (neither is named yet).
pub fn dragonforge_chance(kind: u8, level: u8, ur_dragon_limited: bool) -> Option<i32> {
    const D34: [i32; 4] = [100; 4];
    const D44: [i32; 4] = [100; 4];
    const D54: [i32; 4] = [0, 2, 10, 30];
    const D64: [i32; 4] = [0, 2, 6, 18];
    const D74: [i32; 4] = [5, 15, 25, 40];
    const D84: [i32; 4] = [40, 55, 65, 75];
    let t = match kind {
        0x5a => &D34,
        0x5b if ur_dragon_limited => &D54,
        0x5b => &D44,
        0x5c..=0x5e => &D64,
        0xb1..=0xb3 => &D74,
        0xb4 => &D84,
        _ => return None,
    };
    t.get(level as usize).copied()
}

/// One equipped item's dragonforge roll after an enemy of `kind` dies
/// (`uEnemy::vf72` `0x00aab3b0` → `0x0045cb30`; kind 0x5a is skipped there
/// and handled by the scenario). Only the Arisen's 9 equipment slots
/// (`sItemManager+0x3d0`) roll, and only items below level 4. `bonus` is the
/// signed byte at +0x15 of the item's bag slot; `roll` is the next value of
/// the shared generator at `0x018d017c`. Success sets level 4 through
/// `0x004612c0` (one copy of a stack).
///
/// Translated from `0x0045cb77..0x0045cc2e`: success when
/// `chance + bonus > roll % 100` (signed compare).
pub fn dragonforge_roll(kind: u8, level: u8, ur_dragon_limited: bool, bonus: i8, roll: u32) -> bool {
    if level >= 4 {
        return false;
    }
    match dragonforge_chance(kind, level, ur_dragon_limited) {
        Some(c) if c >= 0 => c + bonus as i32 > (roll % 100) as i32,
        _ => false,
    }
}

/// First and last cursed item ("Bitterblack Novelty Lv.1" .. "Bitterblack
/// Armor Lv.3").
pub const CURSED_FIRST: i16 = 0x6a5;
pub const CURSED_LAST: i16 = 0x6b0;

/// Vocation group offset for the curse table: Fighter/Warrior rows +0,
/// Strider/Ranger +9, Mage/Sorcerer +18; hybrids pick one parent with a
/// coin from a generator seeded with `seed + 0x28` (`0x00464670`).
fn vocation_offset(job: u32, seed: u32) -> i32 {
    let coin = || XorShift::new(seed.wrapping_add(0x28)).next_u32() & 1 != 0;
    match job {
        1 | 7 => 0,
        2 | 8 => 9,
        3 | 9 => 18,
        4 => {
            if coin() {
                0
            } else {
                18
            }
        }
        5 => {
            if coin() {
                0
            } else {
                9
            }
        }
        6 => {
            if coin() {
                9
            } else {
                18
            }
        }
        _ => 0,
    }
}

/// Row of `rItemCurseCnv` used for a cursed item (`0x004644d0`): the item's
/// offset from 0x6a5; for items 0x6a8-0x6b0 (Gear, Weapon, Armor) plus the
/// vocation offset of the player (`sGameSys+0xa76d0+0x10`) or the main
/// pawn (`+0xa7ec0+0x10`), chosen by bit 0 of the first value of a
/// generator seeded with `seed + 0x15` (set: pawn). Rows past the table
/// read row 0 (`rItemCurseCnv::vf15`, `0x0040e310`).
pub fn curse_row(item: i16, seed: u32, player_job: u32, pawn_job: u32) -> u32 {
    let off = (item as i32 - CURSED_FIRST as i32) as u32;
    if !(3..=0xb).contains(&off) {
        return off;
    }
    let pawn = XorShift::new(seed.wrapping_add(0x15)).next_u32() & 1 != 0;
    let job = if pawn { pawn_job } else { player_job };
    (off as i32 + vocation_offset(job, seed)) as u32
}

/// The item a cursed item turns into. Translated from `0x004644d0`:
/// `d = next(seed) % total`; the first slot (from 0) with an item and a
/// running rate sum `>= d` (unsigned) wins; no row or no hit gives item
/// 0x6a5.
///
/// The seed is `sItemManager` word `+0x4070 + item * 4` (for item 0x6a5
/// that is `mItemCurseRandSeedTbl`, +0x5b04); the caller adds 1 to it after
/// every unit rolled (`0x004f3390`), so results are fixed per save.
pub fn curse_roll(table: &[CurseConversion], item: i16, seed: u32, player_job: u32, pawn_job: u32) -> i16 {
    if table.is_empty() {
        return CURSED_FIRST;
    }
    let mut row = curse_row(item, seed, player_job, pawn_job) as usize;
    if row >= table.len() {
        row = 0;
    }
    let r = &table[row];
    let total = r.total() as u32;
    if total == 0 {
        return CURSED_FIRST;
    }
    let d = XorShift::new(seed).next_u32() % total;
    let cum = r.cumulative();
    for (&item, &c) in r.items.iter().zip(cum.iter()) {
        if item != -1 && c as i32 as u32 >= d {
            return item;
        }
    }
    CURSED_FIRST
}

/// Message index in `id\message\ability\abilityName_<lang>` for augment ids
/// 0..=114 (also the record index of `plAbilityParam`), 255 = no name.
/// Table at `0x015214a0` (2 bytes per id, byte 0), read by the skill
/// screens before the name lookup (`0x006f641e`, `0x006ff129`). Some names
/// are shared by two ids (Sinew 1/95, Radiance 75/90, Suasion 84/91).
pub const AUGMENT_NAME_INDEX: [u8; 115] = [
    0, 68, 2, 3, 4, 5, 6, 255, 255, 255, 7, 8, 9, 10, 51, 12, 13, 255, 255, 255, 14, 15, 16, 17, 18,
    19, 20, 255, 255, 255, 21, 22, 23, 24, 25, 26, 27, 255, 255, 255, 28, 29, 30, 31, 32, 33, 34,
    255, 255, 255, 50, 36, 37, 38, 39, 41, 60, 255, 255, 255, 42, 43, 44, 45, 46, 47, 48, 255, 255,
    255, 49, 35, 11, 52, 53, 63, 55, 255, 255, 255, 56, 57, 58, 59, 64, 61, 62, 255, 255, 255, 63,
    64, 65, 66, 67, 68, 255, 255, 255, 255, 69, 70, 71, 72, 73, 74, 255, 75, 76, 77, 78, 79, 80, 81,
    82,
];

/// Byte 1 of the same table; 13 on every id without a name. Meaning not
/// traced (values 0-12).
pub const AUGMENT_GROUP: [u8; 115] = [
    3, 3, 9, 9, 0, 0, 2, 13, 13, 13, 4, 3, 9, 9, 4, 0, 3, 13, 13, 13, 1, 5, 5, 5, 6, 5, 5, 13, 13, 13,
    3, 1, 1, 10, 10, 10, 1, 13, 13, 13, 8, 0, 12, 12, 3, 2, 11, 13, 13, 13, 1, 6, 8, 2, 7, 3, 11, 13,
    13, 13, 1, 6, 3, 0, 0, 1, 0, 13, 13, 13, 9, 11, 9, 6, 7, 8, 2, 13, 13, 13, 6, 9, 3, 5, 7, 5, 5,
    13, 13, 13, 8, 7, 7, 7, 7, 3, 13, 13, 13, 13, 9, 7, 7, 4, 4, 8, 13, 9, 3, 9, 9, 9, 4, 9, 9,
];

/// Augment id of Suasion (sell factor, `0x0044b1f0`).
pub const AUGMENT_SUASION: u32 = 0x54;
