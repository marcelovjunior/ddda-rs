//! Shop, price, restock and enhancement rules traced from DDDA.exe
//! (2017-12-13 Steam build). Pure functions over plain values: the caller
//! supplies game state (flags, inventory, party) through arguments or the
//! [`JudgmentState`] trait, so these work for any number of players.
//!
//! None of this has been compared against the running game yet.

use crate::shop::{Judgment, ShopList, JUDGMENTS};

/// Percent applied to the sell price at item enhancement levels 0..=6.
/// Table at `0x0158640c`, indexed by `level & 0xf` in `0x006e17c0`.
pub const SELL_LEVEL_PERCENT: [u32; 7] = [100, 110, 120, 145, 270, 320, 370];

/// Upper bound of gold and Rift Crystals (`0x004431e0`, `0x00443310`).
pub const CURRENCY_MAX: i32 = 99_999_999;

/// Distinct item slots per character bag and in storage (`0x00465190`).
pub const BAG_SLOTS: u32 = 256;
pub const STORAGE_SLOTS: u32 = 7000;

/// Buy-price rate of a shop in percent.
///
/// Translated from `0x00cde010` (DDDA.exe 2017-12-13 Steam): the GSF
/// discount wins when its flag is set; otherwise the first discount slot
/// whose item the player owns (count >= 1, `0x00460350`); otherwise
/// `mDefaultRate`.
pub fn shop_rate(shop: &ShopList, gsf_set: impl Fn(u32) -> bool, owns: impl Fn(u32) -> bool) -> u32 {
    if shop.discount_gsf != 0xffff_ffff && shop.discount_gsf < 0x600 && gsf_set(shop.discount_gsf) {
        return shop.discount_gsf_rate as u32;
    }
    for (item, rate) in shop.discount_ok_item.iter().zip(shop.discount_rate) {
        if *item != -1 && owns(*item as u32) {
            return rate as u32;
        }
    }
    shop.default_rate as u32
}

/// Buy price of one unit.
///
/// Translated from `0x006e14b0`: `buy * rate / 100` in unsigned 32-bit
/// arithmetic. When the stock is rebuilt with the "special" flag set
/// (`0x006e86c0`: buy mode and the cart holds an item whose
/// `category_kind` is 0), item 66 (Ferrystone) and item 155 (Wakestone
/// Shard) cost a further 50%. What that flag means for the player is not
/// yet known.
pub fn buy_price(item: u32, base_buy: u32, rate: u32, special: bool) -> u32 {
    let p = base_buy.wrapping_mul(rate) / 100;
    if special && (item == 0x42 || item == 0x9b) {
        p.wrapping_mul(50) / 100
    } else {
        p
    }
}

/// Party sell multiplier: the product over the four party slots of
/// `with` (the slot's character has augment 0x54) or `without`.
///
/// Translated from `0x0044b1f0`. The factors are `mFVal0` / `mFVal1` of
/// record 0x54 of `param\pl\other\plAbilityParam` (1.15 / 1.0 in the
/// shipped data). An absent slot uses `without`. A second per-slot
/// condition (a u16 in the slot's item bag at manager offset
/// `slot * 0x11e8 + 0x12ba` being non-zero) also selects `with`; its
/// meaning is unresolved and folded into `has_augment` here.
pub fn sell_factor(has_augment: [bool; 4], with: f32, without: f32) -> f32 {
    // Starts from the constant 1.0 at 0x015319b8; MULSS, single precision.
    let mut f = 1.0f32;
    for h in has_augment {
        f *= if h { with } else { without };
    }
    f
}

/// `trunc(value * factor)` computed exactly, as the x87 does it in
/// `0x006e1a02..0x006e1a32`: `FILD` of the u32, `FMUL` by the f32 factor
/// in extended precision (a 32-bit integer times a 24-bit mantissa is
/// exact), then `FISTP` with the rounding-control bits forced to 11
/// (truncate toward zero).
fn mul_trunc(value: u32, factor: f32) -> u32 {
    if factor.is_nan() || factor <= 0.0 || value == 0 {
        return 0;
    }
    let bits = factor.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = (bits & 0x7f_ffff) as u128;
    let (mant, e) = if exp == 0 {
        (frac, -149)
    } else {
        (frac | 0x80_0000, exp - 150)
    };
    let prod = value as u128 * mant;
    let r = if e >= 0 { prod << e } else { prod >> (-e) };
    r as u64 as u32
}

/// Sell price of one unit.
///
/// Translated from `0x006e17c0`: `trunc(sell * factor)` (see
/// [`mul_trunc`]), times the level percent, integer-divided by 100; with
/// the "special" flag and the item tagged 4 in the per-item list at
/// `0x017f1ae0`, a further 120%.
pub fn sell_price(base_sell: u32, factor: f32, level: u8, special_tag4: bool) -> u32 {
    let scaled = mul_trunc(base_sell, factor);
    let pct = SELL_LEVEL_PERCENT[(level & 0xf).min(6) as usize];
    let p = pct.wrapping_mul(scaled) / 100;
    if special_tag4 {
        p.wrapping_mul(0x78) / 100
    } else {
        p
    }
}

/// Game state a shop judgment reads.
pub trait JudgmentState {
    /// Quest flag `flag` of quest `quest` (`0x004356c0`).
    fn quest_flag(&self, quest: u32, flag: u32) -> bool;
    /// Game-state flag (`0x00435690`, bitfield at save+0xac6e4).
    fn gsf(&self, flag: u32) -> bool;
    /// Story progress value at save+0x30 (commands 10-12).
    fn progress(&self) -> i32;
    /// Affinity of the shop's NPC (commands 13-15); 0 below index 0, 500
    /// from index 900 up (`0x00cde1b0`).
    fn affinity(&self, npc: i32) -> i32;
    /// Player owns at least `count` of `item` (`0x00460350`).
    fn has_item(&self, item: u32, count: u32) -> bool;
    /// Field +0x700 of party slot `slot`'s record (0 player, 1-3 pawns);
    /// commands 20-22. The level, per claude-4's live reading.
    fn party_value(&self, slot: u32) -> u32;
}

/// Truth of one judgment, or `None` for an empty slot (command 0) or an
/// unknown command (which the exe treats as false but still counts as
/// non-empty).
///
/// Translated from the switch in `0x00cde1b0`.
pub fn judgment(j: &Judgment, npc: i32, s: &impl JudgmentState) -> Option<bool> {
    let p0 = j.params[0];
    let p1 = j.params[1];
    let count = if p1 as u32 == 0 { 1 } else { p1 as u32 };
    Some(match j.command {
        0 => return None,
        1 => s.quest_flag(p0 as u32, p1 as u32),
        2 => !s.quest_flag(p0 as u32, p1 as u32),
        3 => s.gsf(p0 as u32),
        4 => !s.gsf(p0 as u32),
        10 => s.progress() == p0,
        11 => p0 <= s.progress(),
        12 => s.progress() < p0,
        13 => npc != -1 && s.affinity(npc) == p1,
        14 => npc != -1 && p1 <= s.affinity(npc),
        15 => npc != -1 && s.affinity(npc) < p1,
        16 => s.has_item(p0 as u32, count),
        20 => s.party_value(p0 as u32) == p1 as u32,
        21 => p1 as u32 <= s.party_value(p0 as u32),
        22 => s.party_value(p0 as u32) < p1 as u32,
        31 => !s.has_item(p0 as u32, count),
        _ => false,
    })
}

/// Open test for an AND list and an OR list (shop open: `0x00cddf60`;
/// lineup entry: `0x00cdde40`). With no non-empty judgment in either list
/// the result is open. Otherwise it is open when the AND list has entries
/// and all of them hold, or the OR list has entries and one of them holds.
pub fn conditions_open(
    and: &[Judgment; JUDGMENTS],
    or: &[Judgment; JUDGMENTS],
    npc: i32,
    s: &impl JudgmentState,
) -> bool {
    let a: Vec<bool> = and.iter().filter_map(|j| judgment(j, npc, s)).collect();
    let o: Vec<bool> = or.iter().filter_map(|j| judgment(j, npc, s)).collect();
    if a.is_empty() && o.is_empty() {
        return true;
    }
    (!a.is_empty() && a.iter().all(|&b| b)) || (!o.is_empty() && o.iter().any(|&b| b))
}

/// One lineup slot of a shop's saved stock (`cSAVE_SHOP`, 0x7f8 bytes, 17
/// shops at save+0x3c4b0, 250 slots each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StockSlot {
    pub item: i16,
    /// Day counter (`+0x1fc` array).
    pub day: i8,
    /// Days per restocked unit, copied from `mItemRearrival` (`+0x2f6`).
    pub rearrival: i8,
    /// Units in stock, -1 = unlimited (`mNowNum`, `+0x3f0`).
    pub now: i16,
    /// Full stock, copied from `mItemNum` (`mInitNum`, `+0x5e4`).
    pub init: i16,
}

/// One restock tick, run by `sNpcManager::vf06` (`0x00475ad0`) when the
/// in-game day counter (save+0xb8760, advanced past midnight by
/// `0x0043f070`) is greater than the day it last saw. Several days passing
/// between two updates still give a single tick.
///
/// Translated from `0x00477cb0`.
pub fn restock_tick(slot: &mut StockSlot) {
    if slot.item == -1 || slot.init == -1 || slot.rearrival == 0 {
        return;
    }
    if slot.now < slot.init {
        slot.day = slot.day.wrapping_add(1);
        if slot.rearrival <= slot.day {
            slot.day = 0;
            slot.now += 1;
            if slot.init <= slot.now {
                slot.now = slot.init;
            }
        }
    } else {
        slot.day = 0;
    }
}

/// Change a slot's stock by `delta` (negative when buying), clamped to
/// 0..=999; unlimited (-1) stock is untouched. Translated from
/// `0x00477a40`.
pub fn adjust_stock(slot: &mut StockSlot, delta: i32) {
    if slot.now == -1 {
        return;
    }
    slot.now = (slot.now as i32 + delta).clamp(0, 999) as i16;
}

/// Enhancement level of an item instance from its flag word (bag slot
/// +0x20, `cItemParam` +0x18): bits 0x400, 0x200, 0x40, 0x20, 0x10, 0x8
/// give 6, 5, 4, 3, 2, 1 (`0x00462840`, `0x006f3350`).
pub fn instance_level(flags: u32) -> u8 {
    if flags & 0x400 != 0 {
        6
    } else if flags & 0x200 != 0 {
        5
    } else if flags & 0x40 != 0 {
        4
    } else if flags & 0x20 != 0 {
        3
    } else if flags & 0x10 != 0 {
        2
    } else {
        ((flags & 0xff) >> 3 & 1) as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Currency {
    Gold,
    RiftCrystals,
}

/// Whether the upgrade screen offers `target` for an item now at
/// `current`, and what it costs in. Translated from `0x006f3350`:
/// - nothing above 6;
/// - targets above 3 only on the second upgrade screen
///   (`uGUIShopUpgrade::ctor(1)`, shop mode 3);
/// - an item below level 4 can only go up to 3 (level 4, dragonforged, is
///   never bought);
/// - an item at level 4 or above only to `current + 1`;
/// - targets below 4 cost gold, 4 and above Rift Crystals.
///
/// The cost row is `rEquipLvUp` for the item (no row: not enhanceable,
/// `0x00cb8d60`); see [`crate::lvup::LvUpRow::step`].
pub fn upgrade_offer(current: u8, target: u8, extended_screen: bool) -> Option<Currency> {
    if target > 6 || target <= current {
        return None;
    }
    if target > 3 && !extended_screen {
        return None;
    }
    if current < 4 && target >= 4 {
        return None;
    }
    if current >= 4 && target != current + 1 {
        return None;
    }
    Some(if target < 4 {
        Currency::Gold
    } else {
        Currency::RiftCrystals
    })
}

/// Inn rest price by innkeeper NPC number: table at `0x0153a694`, looked up
/// by `0x004573d0`; any other NPC gives 0. The rest handler (`0x004565d0`)
/// refuses when gold < price, else subtracts it.
pub fn inn_price(npc: u32) -> u32 {
    match npc {
        6 => 50,
        404 => 300,
        147 => 500,
        475..=477 => 100,
        _ => 0,
    }
}
