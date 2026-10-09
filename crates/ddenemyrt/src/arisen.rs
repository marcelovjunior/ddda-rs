//! The Arisen's side of a fight: its body (the male base skeleton `m000`,
//! its motions and collision), its attack and defence values from the
//! level and weapon tables, and a scripted light attack.
//!
//! The Arisen moves in `ddsim`; the fight poses it from ddsim's state (the
//! motion of `State::motion_id` in bank `m0000_co`) or, while a scripted
//! attack plays, from the attack motion. Attack input and the attack
//! actions (`cPlAct*` for weapons) are not modelled yet: the attack is
//! started by the host (a script), plays its motion once and moves the
//! Arisen by the motion's root travel.

use crate::body::{load, Body, MotionId};
use ddcombat::damage::{PlayerAttacker, PlayerDefender};

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
/// The light sword attack: motion 0 of `m0001_at`. Its collision events
/// switch on `collision\pl\m0001` sequences 1 and 2 (groups 1 and 2,
/// attacks 1 and 2 of `m0001_00_lv1`, relation 1) on frames 6..10; attack
/// 1 is COMBAT.md's worked example. Found by scanning every player motion
/// list's events for those sequences: only this motion and m0001_at
/// motions 200, 203 and 205 use them.
pub const SWORD_ATTACK_1: MotionId = MotionId::new(SWORD_BANK, 0);

/// The push rank the sword's ground attacks write (actor +0x1DF0 = 14,
/// `cPlActWpnSwordAtckLandS1_1` 0x00B3C795 and the rest of the chain).
pub const SWORD_PUSH_RANK: u32 = 14;

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

/// The Arisen's data for the fight.
#[derive(Debug, Clone)]
pub struct ArisenData {
    pub body: Body,
    pub attacker: PlayerAttacker,
    pub defender: PlayerDefender,
    pub hp_max: f32,
}

impl ArisenData {
    /// A level-1 Fighter with an Iron Sword, from the install.
    pub fn fighter(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::load(
            vfs,
            r"param\pl\level\lvfighter",
            FIGHTER_LEVEL_ROW,
            IRON_SWORD,
            ["m0001_co", "m0001_at"],
            // the sword: groups in m0001, attacks in m0001_00_lv1 (the `lv`
            // selector is *unknown*; lv1 as in COMBAT.md 8)
            (r"collision\pl\m0001", r"collision\pl\m0001_00_lv1"),
        )
    }

    /// A level-1 Mage with the starting staff, from the install.
    pub fn mage(vfs: &ddvfs::Vfs) -> Result<Self, String> {
        Self::load(
            vfs,
            r"param\pl\level\lvmagicuser",
            MAGE_LEVEL_ROW,
            MAGE_STAFF,
            ["m0007_co", "m0007_at"],
            (r"collision\pl\m0007", r"collision\pl\m0007_00"),
        )
    }

    fn load(
        vfs: &ddvfs::Vfs,
        level_path: &str,
        row: usize,
        weapon: u32,
        lists: [&str; 2],
        weapon_collision: (&str, &str),
    ) -> Result<Self, String> {
        let skeleton = load::skeleton(vfs, r"model\pl\m\m_base\m000\m000")?;
        let mut banks = vec![None; usize::from(WEAPON_AT_BANK) + 1];
        banks[usize::from(COMMON_BANK)] = Some(load::bank(vfs, r"motion\pl\m\m00\m0000_co\m0000_co")?);
        for (bank, name) in [WEAPON_CO_BANK, WEAPON_AT_BANK].into_iter().zip(lists) {
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
        })
    }
}

/// A scripted attack in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swing {
    pub motion: MotionId,
    pub frame: f32,
}

/// Plays a scripted attack: advances it by `step` motion frames and
/// returns the root travel over the step in model space (x, y, z), or
/// None when the motion ended (frame past its last).
pub fn advance(body: &Body, swing: &mut Swing, step: f32) -> Option<[f32; 3]> {
    let frames = body.frames(swing.motion) as f32;
    if swing.frame >= frames - 1.0 {
        return None;
    }
    let d = body
        .root_motion(swing.motion)
        .map_or([0.0; 3], |r| r.delta_once(swing.frame, step));
    swing.frame += step;
    Some(d)
}
