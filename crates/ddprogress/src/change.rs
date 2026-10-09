//! Vocation change (`uGUIJobChange` rules without the screen).

use crate::character::Character;
use crate::tables::Tables;
use ddskill::vocation::{self, Vocation};

/// Starting equipment given on the free first choice (0x00673740, item ids
/// for the item system): per vocation, then two for everyone.
pub fn starting_items(v: Vocation) -> Vec<u16> {
    let mut items: Vec<u16> = match v {
        Vocation::Fighter => vec![0x141, 0x1db],
        Vocation::Strider => vec![0x195, 0x1f8],
        Vocation::Mage => vec![0x1aa],
        _ => vec![],
    };
    items.extend([0x227, 0x228]);
    items
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Not in the list the screen offers (0x00675780: level gate, pawn list).
    NotOffered,
    /// Already the current vocation (0x00675e8a).
    Current,
    /// Not enough discipline for the first-time cost (0x00675e8a).
    Discipline { need: i32, have: i32 },
}

/// What a change did, for the host to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub from: Vocation,
    pub to: Vocation,
    /// Discipline paid (0 when already paid or on the first choice).
    pub cost: i32,
    /// Items to give (first choice only).
    pub items: Vec<u16>,
}

/// Cost of changing `c` to `to`, or why it is refused. Translated from
/// 0x00675780 (offered list), 0x00671b70 (cost table: 0 on the first choice
/// or when the vocation's flag is set, else 0x01517a7c) and 0x00675e8a
/// (selectable when not current and cost <= discipline).
pub fn check(c: &Character, to: Vocation, first_choice: bool) -> Result<i32, Refused> {
    if !vocation::offered(c.is_pawn, c.progress.level, first_choice).contains(&to) {
        return Err(Refused::NotOffered);
    }
    if to == c.vocation && !first_choice {
        return Err(Refused::Current);
    }
    let cost = vocation::change_cost(to, c.is_pawn, first_choice, |_| c.paid[to as usize]);
    if cost > c.discipline.points {
        return Err(Refused::Discipline { need: cost, have: c.discipline.points });
    }
    Ok(cost)
}

/// Changes vocation. Translated from 0x00672c10 (pay, set the flag) and
/// 0x00675da0 (apply: `mJob`, 0x00673740 empties the six active skills,
/// 0x00780840 empties skill-set entries the new vocation cannot use and
/// staff slots 3..5 outside Mage and Sorcerer). Level, experience, stats,
/// discipline, every vocation's rank, learned skills and augments carry
/// over. `b8844_40000` is the sGameSys+0xb8844 bit 0x40000, which skips
/// the change counter (0x0044b7a0) and, in 0x00780840, the clearing of
/// active slots (cleared here anyway by 0x00673740); its meaning is
/// unresolved and it is normally false.
pub fn change(
    tables: &Tables,
    c: &mut Character,
    to: Vocation,
    first_choice: bool,
    b8844_40000: bool,
) -> Result<Changed, Refused> {
    let cost = check(c, to, first_choice)?;
    let from = c.vocation;
    if !first_choice {
        c.discipline.points -= cost;
        // 0x0044b7a0 / 0x0044b4d0, skipped while sGameSys+0xb8844 & 0x40000.
        if !b8844_40000 {
            c.changes = (c.changes + 1).min(9999);
        }
    }
    if to.unlock_flag(c.is_pawn).is_some() {
        c.paid[to as usize] = true;
    }
    c.vocation = to;
    c.active = [None; 6];
    // Translated from 0x00780840 (DDDA.exe 2017-12-13 Steam), skill-set part.
    for cat in to.weapon_categories() {
        for (slot, entry) in c.weapon_sets[*cat as usize].iter_mut().enumerate() {
            if let Some(id) = *entry {
                let mask = tables.records.get(id as usize).map_or(0, |r| r.vocation_mask);
                if mask != 0 && mask & to.mask() == 0 {
                    *entry = None;
                }
            }
            let staff = matches!(cat, vocation::WeaponCategory::Staff | vocation::WeaponCategory::Archistaff);
            if staff && slot > 2 && !matches!(to, Vocation::Mage | Vocation::Sorcerer) {
                *entry = None;
            }
        }
    }
    let items = if first_choice { starting_items(to) } else { vec![] };
    Ok(Changed { from, to, cost, items })
}
