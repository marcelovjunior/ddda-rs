//! The Arisen's side of a fight: its body (the male base skeleton `m000`,
//! its motions and collision), its attack and defence values from the
//! level and weapon tables, and its weapon's motions for ddsim.
//!
//! The Arisen moves and attacks in `ddsim` (`ddsim::attack`); the fight
//! poses it from ddsim's state: the motion of `State::motion_id`, bank 0
//! `m0000_co` or, for the attacks, bank 7, the weapon's `_at` list.

use crate::body::{load, Body, MotionId};
use ddcombat::damage::{PlayerAttacker, PlayerDefender};
use ddskill::vocation::WeaponCategory;
use std::collections::HashMap;

/// Bank of the common motions (`motion\pl\m\m00\m0000_co\m0000_co`), the
/// one ddsim's motion ids address.
pub const COMMON_BANK: u8 = 0;
/// The weapon banks: a primary weapon's lists are bound at bank keys 0x600
/// (`m000N_co`) and 0x700 (`m000N_at`) (table 0x0163DF8C,
/// `ddskill::vocation::WeaponCategory::motion_banks`), so motion 0x700 is
/// slot 0 of the weapon's attack list.
pub const WEAPON_CO_BANK: u8 = 6;
pub const WEAPON_AT_BANK: u8 = 7;
/// The sword's attack list `motion\pl\m\m00\m0001_at\m0001_at`.
pub const SWORD_BANK: u8 = WEAPON_AT_BANK;
/// The light sword attack (S1_1): motion 0 of `m0001_at`. Its collision
/// events switch on `collision\pl\m0001` sequences 1 and 2 (groups 1 and
/// 2, attacks 1 and 2 of `m0001_00_lv1`, relation 1) on frames 6..10;
/// attack 1 is COMBAT.md's worked example.
pub const SWORD_ATTACK_1: MotionId = MotionId::new(SWORD_BANK, 0);

/// `param\pl\level\lvfighter` row of a level-1 Fighter (COMBAT.md 8).
pub const FIGHTER_LEVEL_ROW: usize = 1;
/// The Iron Sword, item 301 (COMBAT.md 8; the Fighter actually starts with
/// the Rusted Sword, 0x141).
pub const IRON_SWORD: u32 = 301;
/// `param\pl\level\lvmagicuser` row of a level-1 Mage.
pub const MAGE_LEVEL_ROW: usize = 1;
/// The Mage's starting staff, item 0x1AA (first-choice equipment,
/// 0x00673740; `ddprogress::change::starting_items`).
pub const MAGE_STAFF: u32 = 0x1aa;
/// `param\pl\level\lvstrider` row of a level-1 Strider.
pub const STRIDER_LEVEL_ROW: usize = 1;
/// The Strider's starting dagger, item 0x195 (first-choice equipment,
/// 0x00673740; `ddprogress::change::starting_items`; its shortbow 0x1F8 is
/// the secondary weapon, not modelled here).
pub const STRIDER_DAGGER: u32 = 0x195;

/// The Arisen's data for the fight.
#[derive(Debug, Clone)]
pub struct ArisenData {
    pub body: Body,
    pub attacker: PlayerAttacker,
    pub defender: PlayerDefender,
    pub hp_max: f32,
    /// The weapon's category (`ddskill`): its motion banks and its
    /// transition list in ddsim.
    pub category: WeaponCategory,
    /// The level row (the Arisen's level) and the weapon's level (item
    /// +0x66, the value `0x00B82360` compares with cPlayerInfo +0x700).
    pub level: u32,
    pub weapon_level: u32,
}

impl ArisenData {
    /// A level-1 Fighter with an Iron Sword, from the install.
    pub fn fighter(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::load(
            vfs,
            r"param\pl\level\lvfighter",
            FIGHTER_LEVEL_ROW,
            IRON_SWORD,
            WeaponCategory::Sword,
            // the sword: groups in m0001, attacks in m0001_00_lv1 (the `lv`
            // selector is *unknown*; lv1 as in COMBAT.md 8)
            (r"collision\pl\m0001", r"collision\pl\m0001_00_lv1"),
        )
    }

    /// A level-1 Strider with the starting dagger, from the install. The
    /// attack collision is the dagger's `m0005_00_lv1` (the `lv` selector is
    /// *unknown*; lv1 as for the sword).
    pub fn strider(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::load(
            vfs,
            r"param\pl\level\lvstrider",
            STRIDER_LEVEL_ROW,
            STRIDER_DAGGER,
            WeaponCategory::Dagger,
            (r"collision\pl\m0005", r"collision\pl\m0005_00_lv1"),
        )
    }

    /// A level-1 Mage with the starting staff, from the install.
    pub fn mage(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::load(
            vfs,
            r"param\pl\level\lvmagicuser",
            MAGE_LEVEL_ROW,
            MAGE_STAFF,
            WeaponCategory::Staff,
            (r"collision\pl\m0007", r"collision\pl\m0007_00"),
        )
    }

    fn load(
        vfs: &ddvfs::Vfs,
        level_path: &str,
        row: usize,
        weapon: u32,
        category: WeaponCategory,
        weapon_collision: (&str, &str),
    ) -> Result<Self, String> {
        let skeleton = load::skeleton(vfs, r"model\pl\m\m_base\m000\m000")?;
        let mut banks = vec![None; usize::from(WEAPON_AT_BANK) + 1];
        banks[usize::from(COMMON_BANK)] = Some(load::bank(vfs, r"motion\pl\m\m00\m0000_co\m0000_co")?);
        // the category's lists bound at 0x600 and 0x700 (table 0x0163DF8C)
        for (bank, name) in [WEAPON_CO_BANK, WEAPON_AT_BANK]
            .into_iter()
            .zip(category.motion_banks())
        {
            banks[usize::from(bank)] = Some(load::bank(vfs, &format!(r"motion\pl\m\m00\{name}\{name}"))?);
        }
        let collision = vec![
            // the body (hurt group 1: head and torso) keeps its own attacks
            load::collision(vfs, r"collision\pl\m0000", None)?,
            load::collision(vfs, weapon_collision.0, Some(weapon_collision.1))?,
        ];
        let level = ddparam::Level::from_bytes(&load::read(vfs, level_path, "rPlLevel")?)
            .map_err(|e| format!("{level_path}: {e:?}"))?;
        let lv = level
            .entries
            .get(row)
            .ok_or_else(|| format!("{level_path}: no row {row}"))?;
        let items = dditem::itl::ItemList::parse(&load::read(vfs, r"etc\item\itemList", "rItemList")?)
            .map_err(|e| format!("itemList: {e:?}"))?;
        let w = items
            .get(weapon)
            .ok_or_else(|| format!("itemList: no item {weapon:#x}"))?;
        let attacker = PlayerAttacker::new(
            f32::from(lv.attack),
            f32::from(lv.magic_attack),
            w.attack() as f32,
            w.magic_attack() as f32,
        );
        let defender = PlayerDefender::new(f32::from(lv.defend), f32::from(lv.magic_defend));
        Ok(ArisenData {
            body: Body {
                skeleton,
                banks,
                collision,
                scale: 1.0,
            },
            attacker,
            defender,
            hp_max: f32::from(lv.hp_max),
            category,
            level: row as u32,
            weapon_level: u32::from(u16::from_le_bytes([w.raw[0x66], w.raw[0x67]])),
        })
    }

    /// The weapon as ddsim knows it.
    pub fn weapon(&self) -> ddsim::Weapon {
        match self.category {
            WeaponCategory::Sword | WeaponCategory::Mace => ddsim::Weapon::Sword,
            WeaponCategory::Staff => ddsim::Weapon::Staff,
            WeaponCategory::Dagger => ddsim::Weapon::Dagger,
            _ => ddsim::Weapon::None,
        }
    }

    /// How far the weapon's level is above the Arisen's (`0x00B82360`).
    pub fn weapon_level_gap(&self) -> i32 {
        self.weapon_level as i32 - self.level as i32
    }

    /// The weapon banks' motions for ddsim (`bank << 8 | slot`), with their
    /// root tracks, curves and events.
    pub fn sim_motions(&self) -> HashMap<u16, ddsim::RootMotion> {
        let mut out = HashMap::new();
        for bank in [WEAPON_CO_BANK, WEAPON_AT_BANK] {
            let Some(b) = self.body.banks.get(usize::from(bank)).and_then(Option::as_ref) else {
                continue;
            };
            for (slot, m) in b.list.motions.iter().enumerate() {
                let Some(m) = m else { continue };
                let root = ddsim::RootMotion::from_motion(m)
                    .with_curves(b.list.curves[slot].clone())
                    .with_events(b.list.events[slot].clone());
                out.insert(u16::from(bank) << 8 | slot as u16, root);
            }
        }
        out
    }

    /// Puts the weapon into `sim` for player `id`: its motions (once per
    /// weapon), the player's weapon and its level gap.
    pub fn equip(&self, sim: &mut ddsim::Sim, id: ddsim::PlayerId) {
        let w = self.weapon();
        sim.weapon_motions.entry(w).or_insert_with(|| self.sim_motions());
        if let Some(p) = sim.player_mut(id) {
            p.weapon = w;
            p.weapon_level_gap = self.weapon_level_gap();
        }
    }
}
