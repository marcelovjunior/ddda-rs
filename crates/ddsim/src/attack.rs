//! The Arisen's basic attacks: the light and heavy buttons, the attack
//! actions of the sword and the staff (`cPlActWpn*`), their chains and
//! their stamina (docs/MOVEMENT.md, "Attacks").
//!
//! How the original does it:
//! - **Input.** The button presses become command bits (key-config 0x17
//!   light, 0x18 heavy; `0x00B52A00`), pressed edges at uPlayer +0x32E0.
//! - **Transition lists.** `uPlayerBase::vf136` (0x00B6D510) calls vf203
//!   (0x00B6EA70) and `0x00ABA690`, which scans the weapon's skill lists,
//!   then the basic-attack list of the primary weapon category (pointer
//!   array 0x014F8DCC), then the default list, and takes the first record
//!   whose current action, command, trigger, ground / air flag and check
//!   match. The record's action goes to request slot uPlayer +0x38B8[slot].
//!   Sword (categories 1, 2) list 0x014F83B8, staff (category 5) list
//!   0x014F8898; [`next`] holds the ground rows.
//! - **Cancel slots** (the combo windows). The cancel object at uPlayer
//!   +0x2588 has 7 slots bound to motion layer 0, event group 0
//!   (`0x00827F50`): slot `i` is in state 1 (buffer) while bit `0x11 + 2i`
//!   is set and in state 2 (execute) while only bit `0x10 + 2i` is
//!   (`0x00828030`, [`slot_state`]). A request is taken only while its
//!   slot is non-zero and stamina is above 0 (`0x00ABAAC0`), is dropped when
//!   the slot falls back to 0, and starts its action once the slot reaches
//!   state 2 (vf136's second loop, 0x00B6DC10). Slot 0 is the stick's
//!   (bit 0x10, `crate::MOVE_CANCEL_BIT`), slot 1 the attacks'.
//! - **Sequence counters** (`0x0082B400`, reset with each new motion by
//!   vf36 0x00B800C0): uPlayer +0x361C counts the motion's frames with
//!   group 0 bit 31, +0x365C those with bit 30. Check 22 of the chain rows
//!   needs +0x361C == 0 (bit 31 closes the chain: a later press starts over
//!   at the first attack); an attack ends when +0x365C >= 1 or its motion
//!   ends (`0x01057100`), e.g. `cPlActWpnSwordAtckLandS1_1::vf06` 0x00B3CC20.
//! - **Stamina.** Each sword or staff strike is charged once in its vf05,
//!   `0x00B81E30(-m_pl000_00[id], 1, 0)`; WandShot is charged per volley.
//!   Mode 1 of the cost modifier `0x00B82060` makes it 0 unless the
//!   equipped weapon's level is well above the Arisen's ([`attack_cost`]).

/// The weapon whose basic attacks a player uses (its category's
/// transition list and motion banks, `ddskill::vocation::WeaponCategory`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Weapon {
    /// No attacks (the transition lists of other categories are not
    /// modelled).
    #[default]
    None,
    /// Sword or mace (categories 1, 2): `m0001_co` / `m0001_at`.
    Sword,
    /// Staff (category 5): `m0007_co` / `m0007_at`.
    Staff,
}

/// A basic attack action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attack {
    /// `cPlActWpnSwordAtckLandS1_1` (0x01010000): light 1.
    SwordLight1,
    /// `cPlActWpnSwordAtckLandS1_2` (0x01010001): light 2.
    SwordLight2,
    /// `cPlActWpnSwordAtckLandS1_3` (0x01010002): light 3.
    SwordLight3,
    /// `cPlActWpnSwordAtckLandL` (0x01010004): the heavy attack.
    SwordHeavy,
    /// `cPlActWpnWandStrike1` (0x0107001E): heavy 1.
    StaffStrike1,
    /// `cPlActWpnWandStrike2` (0x0107001F): heavy 2.
    StaffStrike2,
    /// `cPlActWpnSwordAtckAirS` (0x01010003): light in the air.
    SwordAirLight,
    /// `cPlActWpnSwordAtckAirL` (0x01010005): heavy in the air, the plunge.
    SwordAirHeavy,
    /// The plunge's landing (its vf06 phase 2 plays motion 0x704 on ground).
    SwordAirHeavyLand,
    /// `cPlActWpnWandAirStrike` (0x01070020): the staff's heavy in the air.
    StaffAirStrike,
}

/// The cancel slot the basic attacks request in (the rows' `slot`).
pub const ATTACK_SLOT: usize = 1;
/// Event group 0 bit 31: counted into uPlayer +0x361C; closes the chain.
pub const CHAIN_CLOSED_BIT: u32 = 1 << 31;
/// Event group 0 bit 30: counted into uPlayer +0x365C; ends the action.
pub const ACTION_END_BIT: u32 = 1 << 30;

impl Attack {
    /// The action id (`group << 24 | weapon << 16 | index`; the index is
    /// the record of the category's action table, `ddskill`'s catalogue).
    pub fn action_id(self) -> u32 {
        match self {
            Attack::SwordLight1 => 0x0101_0000,
            Attack::SwordLight2 => 0x0101_0001,
            Attack::SwordLight3 => 0x0101_0002,
            Attack::SwordHeavy => 0x0101_0004,
            Attack::StaffStrike1 => 0x0107_001E,
            Attack::StaffStrike2 => 0x0107_001F,
            Attack::SwordAirLight => 0x0101_0003,
            Attack::SwordAirHeavy | Attack::SwordAirHeavyLand => 0x0101_0005,
            Attack::StaffAirStrike => 0x0107_0020,
        }
    }

    /// Started in the air (the rows' 0x302 flag); played under air physics.
    pub fn air(self) -> bool {
        matches!(self, Attack::SwordAirLight | Attack::SwordAirHeavy | Attack::StaffAirStrike)
    }

    /// The `pljumpparam` row its vf06 loads with `0x00937CF0` (speed,
    /// gravity, damping for the air): AirS 0x36 (0x00B3D3C0), AirL 0x37
    /// (0x00B3DA8D), WandAirStrike 0x47 (0x00B515F0).
    pub fn jump_row(self) -> Option<usize> {
        match self {
            Attack::SwordAirLight => Some(0x36),
            Attack::SwordAirHeavy => Some(0x37),
            Attack::StaffAirStrike => Some(0x47),
            _ => None,
        }
    }

    /// The motion its vf05 plays (bank 7 = the weapon's `_at` list; force
    /// 1, interpolation -1 = the bank's default, from frame 0 at rate 1):
    /// S1_1 0x00B3C570, S1_2 0x00B3C7B0, S1_3 0x00B3CA00, LandL 0x00B3D410,
    /// Strike1 0x00B51070, Strike2 0x00B51270. The sword's alternative
    /// motions (0x7C8.. while uPlayer +0x206D bit 3 or +0x206E & 0x18) are
    /// not modelled.
    pub fn motion(self) -> u16 {
        match self {
            Attack::SwordLight1 => 0x700,
            Attack::SwordLight2 => 0x701,
            Attack::SwordLight3 => 0x702,
            Attack::SwordHeavy => 0x713,
            Attack::StaffStrike1 => 0x723,
            Attack::StaffStrike2 => 0x724,
            // cPlActWpnSwordAtckAirS::vf06 0x00B3D360, AirL::vf06 0x00B3DA10,
            // cPlActWpnWandAirStrike::vf06 0x00B51590
            Attack::SwordAirLight => 0x714,
            Attack::SwordAirHeavy => 0x703,
            Attack::SwordAirHeavyLand => 0x704,
            Attack::StaffAirStrike => 0x725,
        }
    }

    /// The `m_pl000_00` id of its stamina cost (the vf05s above).
    pub fn cost_id(self) -> usize {
        match self {
            Attack::StaffStrike1 | Attack::StaffStrike2 | Attack::StaffAirStrike => ADJUST_STAFF_COST,
            _ => ADJUST_SWORD_COST,
        }
    }
}

/// The staff's light attack, `cPlActWpnWandShot` (0x01070021, variant 0 of
/// the table at 0x014F70E8, the ground one): a start motion, a loop that
/// fires a volley of shells at its frame 2 (`0x00B4F610`), refires on a
/// light press before 5 volleys, and an end motion.
pub mod shot {
    /// The action id on the ground.
    pub const ACTION: u32 = 0x0107_0021;
    /// Variant 0, +0x00 / +0x0C / +0x18: the layer-0 motions.
    pub const START: u16 = 0x70A;
    pub const LOOP: u16 = 0x70B;
    pub const END: u16 = 0x70C;
    /// +0x34: the loop frame the volley fires on.
    pub const FIRE_FRAME: f32 = 2.0;
    /// `0x00B4F610`: a requested refire restarts the loop from this frame.
    pub const REFIRE_FRAME: f32 = 7.0;
    /// +0x3C: refires are taken while fewer volleys than this were fired.
    pub const VOLLEYS: u32 = 5;
    /// +0x5C: shells per volley.
    pub const SHELLS: u32 = 1;
    /// The push rank its vf05 writes (0x00B4E200: 13).
    pub const PUSH_RANK: u32 = 13;
    /// The air shot (0x01070022): variant 1 of the table (0x014F7148),
    /// motions 0x716 / 0x717 / 0x718, one volley (+0x3C = 1), no turn
    /// motions. Its phases call no physics, so the caster hangs in the air.
    pub const AIR_ACTION: u32 = 0x0107_0022;
    pub const AIR_START: u16 = 0x716;
    pub const AIR_LOOP: u16 = 0x717;
    pub const AIR_END: u16 = 0x718;
    pub const AIR_VOLLEYS: u32 = 1;
}

/// The phases of `cPlActWpnWandShot::vf06` (0x00B4F190) modelled here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShotPhase {
    /// Phase 0 (`0x00B4F490`): the start motion until it ends.
    Start,
    /// Phase 1 (`0x00B4F610`): the loop motion, firing.
    Loop,
    /// Phase 3 (`0x00B50590`): the end motion. (Phase 2, the turn motions
    /// 0x713 / 0x714 for an aim beyond 90 degrees, is not modelled.)
    End,
    /// The same phases of the air variant.
    AirStart,
    AirLoop,
    AirEnd,
}

impl ShotPhase {
    pub fn motion(self) -> u16 {
        match self {
            ShotPhase::Start => shot::START,
            ShotPhase::Loop => shot::LOOP,
            ShotPhase::End => shot::END,
            ShotPhase::AirStart => shot::AIR_START,
            ShotPhase::AirLoop => shot::AIR_LOOP,
            ShotPhase::AirEnd => shot::AIR_END,
        }
    }

    pub fn air(self) -> bool {
        matches!(self, ShotPhase::AirStart | ShotPhase::AirLoop | ShotPhase::AirEnd)
    }
}

/// A WandShot action's own state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ShotState {
    /// The loop motion fired its volley (action +0xB0).
    pub fired: bool,
    /// A light press asked for another volley (+0xB3).
    pub refire: bool,
    /// Volleys fired by this action (+0x84).
    pub volleys: u32,
}

/// What a press asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Request {
    Attack(Attack),
    Shot,
    AirShot,
}

/// The ground rows of the transition lists for a press, first match wins.
/// `current` is the attack playing (None for any other action) and
/// `chain_open` check 22 (no bit 31 yet in its motion).
///
/// Sword list 0x014F83B8: light after S1_1 -> S1_2 and after S1_2 -> S1_3
/// (check 22), otherwise light -> S1_1; heavy -> LandL. The S1_2_1 branch
/// (check 24, an ability bit, *not identified*) and LandL's follow-up
/// (0x0101000D, needs a learned ability) are left out. Staff list
/// 0x014F8898: light -> WandShot; heavy after Strike1 -> Strike2 (check
/// 22), otherwise heavy -> Strike1. In the air (rows flagged 0x302): sword
/// light -> AirS (slot 4), heavy -> AirL; staff light -> the air WandShot,
/// heavy -> WandAirStrike. The staff's charged rows (command 0x0C, checks
/// 8, 15, 16) are not modelled.
// Translated from the lists 0x014F83B8 and 0x014F8898 scanned by 0x00ABA690
// (DDDA.exe 2017-12-13 Steam).
pub fn next(
    weapon: Weapon,
    current: Option<Attack>,
    light: bool,
    heavy: bool,
    chain_open: bool,
    air: bool,
) -> Option<Request> {
    use Attack::*;
    if air {
        return match (weapon, light, heavy) {
            (Weapon::Sword, true, _) => Some(Request::Attack(SwordAirLight)),
            (Weapon::Sword, false, true) => Some(Request::Attack(SwordAirHeavy)),
            (Weapon::Staff, true, _) => Some(Request::AirShot),
            (Weapon::Staff, false, true) => Some(Request::Attack(StaffAirStrike)),
            _ => None,
        };
    }
    match weapon {
        Weapon::None => None,
        Weapon::Sword => {
            if light {
                Some(Request::Attack(match current {
                    Some(SwordLight1) if chain_open => SwordLight2,
                    Some(SwordLight2) if chain_open => SwordLight3,
                    _ => SwordLight1,
                }))
            } else if heavy {
                Some(Request::Attack(SwordHeavy))
            } else {
                None
            }
        }
        Weapon::Staff => {
            if light {
                Some(Request::Shot)
            } else if heavy {
                Some(Request::Attack(match current {
                    Some(StaffStrike1) if chain_open => StaffStrike2,
                    _ => StaffStrike1,
                }))
            } else {
                None
            }
        }
    }
}

/// A cancel slot's state from an event group 0 word: 1 (buffer) while bit
/// `0x11 + 2 slot` is set, else 2 (execute) while bit `0x10 + 2 slot` is,
/// else 0.
// Translated from 0x00828030 with the bits bound by 0x00827F50 (DDDA.exe
// 2017-12-13 Steam).
pub fn slot_state(word: u32, slot: usize) -> u8 {
    let buffer = 0x11 + 2 * slot as u32;
    if word & (1 << buffer) != 0 {
        1
    } else if word & (1 << (buffer - 1)) != 0 {
        2
    } else {
        0
    }
}

/// `m_pl000_00` ids the attacks read.
pub const ADJUST_TURN: usize = 0xAE;
pub const ADJUST_SWORD_COST: usize = 0x1B8;
pub const ADJUST_STAFF_COST: usize = 0x1BB;
/// Level-gap thresholds of tiers 1, 2, 3 (read 0x1B0, 0x1AF, 0x1AE, from
/// tier 3 down).
pub const ADJUST_TIER_GAP: usize = 0x1AE;
/// Mode-1 cost factors of tiers 1, 2, 3 (`0x1B3 + tier`).
pub const ADJUST_TIER_FACTOR: usize = 0x1B4;

/// The attacks' values from `param\pl\other\m_pl000_00`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackParam {
    /// Turn toward the attack's heading, degrees per frame (0xAE).
    pub turn_deg: f32,
    /// Stamina per sword attack (0x1B8) and per staff strike or volley (0x1BB).
    pub sword_cost: f32,
    pub staff_cost: f32,
    /// Level gaps of tiers 1-3 (0x1AE..0x1B0).
    pub tier_gap: [f32; 3],
    /// Cost factors of tiers 1-3 (0x1B4..0x1B6).
    pub tier_factor: [f32; 3],
}

/// `m_pl000_00` as shipped.
pub const ATTACK_SHIPPED: AttackParam = AttackParam {
    turn_deg: 45.0,
    sword_cost: 80.0,
    staff_cost: 30.0,
    tier_gap: [15.0, 20.0, 25.0],
    tier_factor: [1.0, 1.5, 2.0],
};

impl AttackParam {
    pub fn from_adjust(t: &[f32]) -> Option<Self> {
        let g = |i: usize| t.get(i).copied();
        Some(AttackParam {
            turn_deg: g(ADJUST_TURN)?,
            sword_cost: g(ADJUST_SWORD_COST)?,
            staff_cost: g(ADJUST_STAFF_COST)?,
            tier_gap: [
                g(ADJUST_TIER_GAP)?,
                g(ADJUST_TIER_GAP + 1)?,
                g(ADJUST_TIER_GAP + 2)?,
            ],
            tier_factor: [
                g(ADJUST_TIER_FACTOR)?,
                g(ADJUST_TIER_FACTOR + 1)?,
                g(ADJUST_TIER_FACTOR + 2)?,
            ],
        })
    }

    pub fn cost_of(&self, id: usize) -> f32 {
        if id == ADJUST_STAFF_COST {
            self.staff_cost
        } else {
            self.sword_cost
        }
    }
}

/// The weapon-level tier: the highest of 3, 2, 1 whose gap the equipped
/// weapons' level (the greater of the two weapon slots' item +0x66) above
/// the Arisen's level (cPlayerInfo +0x700) reaches, else 0. The gaps are
/// compared truncated to integers.
// Translated from 0x00B82360 (DDDA.exe 2017-12-13 Steam).
pub fn tier(p: &AttackParam, level_gap: i32) -> usize {
    if level_gap <= 0 {
        return 0;
    }
    for t in (1..=3).rev() {
        if p.tier_gap[t - 1] as i32 <= level_gap {
            return t;
        }
    }
    0
}

/// The stamina a basic attack takes (a positive amount): mode 1 of
/// `0x00B82060` makes it 0 at tier 0 and scales it by `0x1B3 + tier`
/// otherwise. The augment and status factors of that function (actor
/// +0x3EF0, +0x3EF8) and the exemptions of `0x00B81E30` (stage and game
/// flags, actor +0x26A4, +0x4CF0, +0x31EA) are not modelled.
// Translated from 0x00B82060 (mode 1) (DDDA.exe 2017-12-13 Steam).
pub fn attack_cost(p: &AttackParam, base: f32, level_gap: i32) -> f32 {
    match tier(p, level_gap) {
        0 => 0.0,
        t => base * p.tier_factor[t - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_states_follow_the_bits() {
        // S1_1 frames 8..13 buffer, 13.. execute (m0001_at motion 0)
        assert_eq!(slot_state(0x2288_0000, 1), 1);
        assert_eq!(slot_state(0x1144_0000, 1), 2);
        assert_eq!(slot_state(0x1545_0000, 0), 2);
        assert_eq!(slot_state(0x1540_0000, 1), 0);
        // buffer wins over execute
        assert_eq!(slot_state(0x000C_0000, 1), 1);
    }

    #[test]
    fn chains_follow_the_lists() {
        use Attack::*;
        let s = |c, l, h, o| next(Weapon::Sword, c, l, h, o, false);
        assert_eq!(s(None, true, false, true), Some(Request::Attack(SwordLight1)));
        assert_eq!(
            s(Some(SwordLight1), true, false, true),
            Some(Request::Attack(SwordLight2))
        );
        assert_eq!(
            s(Some(SwordLight2), true, false, true),
            Some(Request::Attack(SwordLight3))
        );
        assert_eq!(
            s(Some(SwordLight3), true, false, true),
            Some(Request::Attack(SwordLight1))
        );
        assert_eq!(
            s(Some(SwordLight1), true, false, false),
            Some(Request::Attack(SwordLight1))
        );
        assert_eq!(
            s(Some(SwordLight2), false, true, true),
            Some(Request::Attack(SwordHeavy))
        );
        let w = |c, l, h, o| next(Weapon::Staff, c, l, h, o, false);
        assert_eq!(w(None, true, false, true), Some(Request::Shot));
        assert_eq!(w(None, false, true, true), Some(Request::Attack(StaffStrike1)));
        assert_eq!(
            w(Some(StaffStrike1), false, true, true),
            Some(Request::Attack(StaffStrike2))
        );
        assert_eq!(
            w(Some(StaffStrike2), false, true, true),
            Some(Request::Attack(StaffStrike1))
        );
        assert_eq!(next(Weapon::None, None, true, true, true, false), None);
        // the air rows (0x302) of both lists
        assert_eq!(next(Weapon::Sword, None, true, false, true, true), Some(Request::Attack(Attack::SwordAirLight)));
        assert_eq!(next(Weapon::Sword, None, false, true, true, true), Some(Request::Attack(Attack::SwordAirHeavy)));
        assert_eq!(next(Weapon::Staff, None, true, false, true, true), Some(Request::AirShot));
        assert_eq!(next(Weapon::Staff, None, false, true, true, true), Some(Request::Attack(Attack::StaffAirStrike)));
        assert_eq!(next(Weapon::None, None, true, true, true, true), None);
    }

    #[test]
    fn basic_attacks_cost_stamina_only_with_a_weapon_far_above_the_level() {
        let p = ATTACK_SHIPPED;
        assert_eq!(attack_cost(&p, 80.0, 0), 0.0);
        assert_eq!(attack_cost(&p, 80.0, 14), 0.0);
        assert_eq!(attack_cost(&p, 80.0, 15), 80.0);
        assert_eq!(attack_cost(&p, 80.0, 20), 120.0);
        assert_eq!(attack_cost(&p, 30.0, 40), 60.0);
    }
}

#[cfg(test)]
mod sim_tests {
    use super::*;
    use crate::{Ground, Input, RootMotion, Sim, State};
    use std::collections::HashMap;

    struct Flat;
    impl Ground for Flat {
        fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
            (0.0 <= below_y).then_some(0.0)
        }
    }

    /// A 44-frame attack with S1_1's group 0 runs (`m0001_at` motion 0):
    /// slot 1 buffers on 8..13 and executes from 13, bit 31 on frame 30,
    /// slot 0 (the stick) executes from 30, bit 30 on frame 34.
    fn s1() -> RootMotion {
        let run = |bits, frames| mtlmt::EventRun { bits, frames };
        let mut m = RootMotion::none(44);
        m.events = vec![vec![
            run(0, 8),
            run(0x2288_0000, 5),
            run(0x1144_0000, 17),
            run(0x9545_0000, 1),
            run(0x1545_0000, 3),
            run(0x5545_0000, 1),
            run(0x1545_0000, 9),
        ]];
        m
    }

    fn sim(weapon: Weapon) -> Sim {
        let mut s = Sim::new(HashMap::new());
        let mut wm = HashMap::new();
        for id in [0x700, 0x701, 0x702, 0x713, 0x723, 0x724] {
            wm.insert(id, s1());
        }
        // WandShot: start 19 frames, loop 17 (bit 31 on 0 and 13..16), end 26
        let run = |bits, frames| mtlmt::EventRun { bits, frames };
        let mut lp = RootMotion::none(17);
        lp.events = vec![vec![
            run(0x9540_0000, 1),
            run(0x1540_0000, 12),
            run(0x9540_0000, 4),
        ]];
        let mut start = RootMotion::none(19);
        start.events = vec![vec![run(0x1540_0000, 19)]];
        let mut end = RootMotion::none(26);
        end.events = vec![vec![
            run(0x2080_0000, 5),
            run(0x1248_0000, 5),
            run(0x1546_0000, 16),
        ]];
        wm.insert(shot::START, start);
        wm.insert(shot::LOOP, lp);
        wm.insert(shot::END, end);
        s.weapon_motions.insert(weapon, wm);
        s.add_player(0, [0.0; 3], 0.0);
        s.player_mut(0).unwrap().weapon = weapon;
        s
    }

    /// Steps one engine frame with `input`; returns the state after it.
    fn step(s: &mut Sim, input: Input) -> State {
        s.step(1.0 / 30.0, &HashMap::from([(0, input)]), &Flat);
        s.player(0).unwrap().state
    }

    fn light() -> Input {
        Input {
            light: true,
            ..Default::default()
        }
    }

    fn heavy() -> Input {
        Input {
            heavy: true,
            ..Default::default()
        }
    }

    fn attack(a: Attack) -> State {
        State::Attack { attack: a }
    }

    /// The state and frame after each of `n` engine frames of `input`.
    fn log(s: &mut Sim, n: usize, input: Input) -> Vec<(State, f32)> {
        (0..n)
            .map(|_| {
                step(s, input);
                let p = s.player(0).unwrap();
                (p.state, p.frame)
            })
            .collect()
    }

    // An action started in a step plays on for the rest of it: the press's
    // step ends at frame 0.75 (three of its four ticks).

    #[test]
    fn the_sword_chain_buffers_executes_and_closes() {
        use Attack::*;
        let mut s = sim(Weapon::Sword);
        assert_eq!(step(&mut s, light()), attack(SwordLight1));
        log(&mut s, 2, Input::default());
        // pressed on frame 2.75: slot 1 closed (no bits before 8), lost
        step(&mut s, light());
        assert_eq!(s.player(0).unwrap().request, None);
        log(&mut s, 6, Input::default());
        // pressed on frame 9.75 (slot 1 buffering): held in the slot
        assert_eq!(step(&mut s, light()), attack(SwordLight1));
        assert_eq!(s.player(0).unwrap().request, Some(Request::Attack(SwordLight2)));
        // and started where slot 1 executes, frame 13
        let l = log(&mut s, 3, Input::default());
        assert_eq!(l[1], (attack(SwordLight1), 12.75), "{l:?}");
        assert_eq!(l[2], (attack(SwordLight2), 0.5), "{l:?}");
        // S1_2 ends with bit 30 on frame 34, then the wait
        let l = log(&mut s, 34, Input::default());
        assert_eq!(l[32].0, attack(SwordLight2), "{l:?}");
        assert_eq!(l[33].0, State::Wait, "{l:?}");
        assert_eq!(s.player(0).unwrap().attacks_started, 2);
    }

    #[test]
    fn after_bit_31_light_starts_over_and_heavy_cancels_into_the_heavy_attack() {
        use Attack::*;
        let mut s = sim(Weapon::Sword);
        step(&mut s, light());
        log(&mut s, 30, Input::default());
        // frame 30.75: bit 31 was seen on frame 30, so S1_1 again
        assert_eq!(step(&mut s, light()), attack(SwordLight1));
        assert_eq!(s.player(0).unwrap().frame, 0.75);
        assert_eq!(s.player(0).unwrap().attacks_started, 2);
        log(&mut s, 20, Input::default());
        assert_eq!(step(&mut s, heavy()), attack(SwordHeavy));
    }

    #[test]
    fn the_stick_cancels_from_slot_0() {
        let mut s = sim(Weapon::Sword);
        step(&mut s, light());
        let fwd = Input {
            stick: [0.0, 1.0],
            ..Default::default()
        };
        for _ in 0..29 {
            assert_eq!(step(&mut s, fwd), attack(Attack::SwordLight1));
        }
        assert!(matches!(step(&mut s, fwd), State::Run { .. }));
    }

    #[test]
    fn strikes_cost_stamina_by_the_weapon_level_tier() {
        let mut s = sim(Weapon::Staff);
        let max = s.player(0).unwrap().stamina;
        step(&mut s, heavy());
        assert_eq!(s.player(0).unwrap().state, attack(Attack::StaffStrike1));
        assert_eq!(s.player(0).unwrap().stamina, max);
        let mut s = sim(Weapon::Staff);
        s.player_mut(0).unwrap().weapon_level_gap = 20;
        step(&mut s, heavy());
        // 30 x 1.5 (tier 2), then a frame of recovery (1.625: no group 1 data)
        let p = s.player(0).unwrap();
        assert!((p.stamina - (max - 45.0 + 1.625)).abs() < 1e-3, "{}", p.stamina);
        for _ in 0..14 {
            step(&mut s, Input::default());
        }
        assert_eq!(step(&mut s, heavy()), attack(Attack::StaffStrike2));
        // out of stamina, a press is not taken
        let mut s = sim(Weapon::Staff);
        s.player_mut(0).unwrap().stamina = 0.0;
        assert_eq!(step(&mut s, heavy()), State::Wait);
    }

    #[test]
    fn wand_shot_fires_volleys_and_refires_on_light() {
        let mut s = sim(Weapon::Staff);
        s.player_mut(0).unwrap().weapon_level_gap = 15;
        let max = s.player(0).unwrap().stamina;
        assert_eq!(
            step(&mut s, light()),
            State::Shot {
                phase: ShotPhase::Start
            }
        );
        let loop_ = State::Shot {
            phase: ShotPhase::Loop,
        };
        // the start plays to its last frame (18), then the loop fires on
        // its frame 2
        let mut l = vec![];
        while s.player(0).unwrap().volleys_fired == 0 {
            l.extend(log(&mut s, 1, Input::default()));
        }
        assert_eq!(l.len(), 20, "{l:?}");
        assert_eq!(
            (l[16].0, l[17].0),
            (
                State::Shot {
                    phase: ShotPhase::Start
                },
                loop_
            ),
            "{l:?}"
        );
        assert_eq!(*l.last().unwrap(), (loop_, 2.75));
        // tier 1: 30 stamina per volley
        assert!(s.player(0).unwrap().stamina < max - 25.0);
        // light on loop frame 3.75: another volley once the loop reaches
        // frame 7, from its start
        step(&mut s, light());
        let l = log(&mut s, 6, Input::default());
        assert_eq!(l[2], (loop_, 6.75), "{l:?}");
        assert_eq!(l[3], (loop_, 0.75), "{l:?}");
        assert_eq!(s.player(0).unwrap().volleys_fired, 2);
        assert_eq!(s.player(0).unwrap().shot.volleys, 2);
        // without another press the loop ends, then the end motion
        let l = log(&mut s, 60, Input::default());
        let end = l
            .iter()
            .position(|x| {
                x.0 == State::Shot {
                    phase: ShotPhase::End,
                }
            })
            .unwrap();
        assert_eq!(l[end - 1].0, loop_);
        assert_eq!(l.last().unwrap().0, State::Wait);
        assert_eq!(s.player(0).unwrap().volleys_fired, 2);
    }
}
