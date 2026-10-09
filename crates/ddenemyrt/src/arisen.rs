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
/// This crate's bank number for the sword attack list
/// `motion\pl\m\m00\m0001_at\m0001_at` (the game's own bank number for it
/// is *not traced*).
pub const SWORD_BANK: u8 = 0x20;
/// The light sword attack: motion 0 of `m0001_at`. Its collision events
/// switch on `collision\pl\m0001` sequences 1 and 2 (groups 1 and 2,
/// attacks 1 and 2 of `m0001_00_lv1`, relation 1) on frames 6..10; attack
/// 1 is COMBAT.md's worked example. Found by scanning every player motion
/// list's events for those sequences: only this motion and m0001_at
/// motions 200, 203 and 205 use them.
pub const SWORD_ATTACK_1: MotionId = MotionId::new(SWORD_BANK, 0);

/// `param\pl\level\lvfighter` row of a level-1 Fighter (COMBAT.md 8).
pub const FIGHTER_LEVEL_ROW: usize = 1;
/// The Iron Sword, item 301 (COMBAT.md 8; the Fighter actually starts with
/// the Rusted Sword, 0x141).
pub const IRON_SWORD: u32 = 301;

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
        let skeleton = load::skeleton(vfs, r"model\pl\m\m_base\m000\m000")?;
        let mut banks = vec![None; usize::from(SWORD_BANK) + 1];
        banks[usize::from(COMMON_BANK)] = Some(load::bank(vfs, r"motion\pl\m\m00\m0000_co\m0000_co")?);
        banks[usize::from(SWORD_BANK)] = Some(load::bank(vfs, r"motion\pl\m\m00\m0001_at\m0001_at")?);
        let collision = vec![
            // the body (hurt group 1: head and torso) keeps its own attacks
            load::collision(vfs, r"collision\pl\m0000", None)?,
            // the sword: groups in m0001, attacks in m0001_00_lv1 (the
            // `lv` selector is *unknown*; lv1 as in COMBAT.md 8)
            load::collision(vfs, r"collision\pl\m0001", Some(r"collision\pl\m0001_00_lv1"))?,
        ];
        let level = ddparam::Level::from_bytes(&load::read(vfs, r"param\pl\level\lvfighter", "rPlLevel")?)
            .map_err(|e| format!("lvfighter: {e:?}"))?;
        let lv = level
            .entries
            .get(FIGHTER_LEVEL_ROW)
            .ok_or("lvfighter: no row 1")?;
        let items = dditem::itl::ItemList::parse(&load::read(vfs, r"etc\item\itemList", "rItemList")?)
            .map_err(|e| format!("itemList: {e:?}"))?;
        let sword = items.get(IRON_SWORD).ok_or("itemList: no Iron Sword")?;
        let attacker = PlayerAttacker::new(
            f32::from(lv.attack),
            f32::from(lv.magic_attack),
            sword.attack() as f32,
            sword.magic_attack() as f32,
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
