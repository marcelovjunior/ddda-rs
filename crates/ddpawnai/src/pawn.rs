//! A pawn's think and goals, headless: inputs -> state -> a virtual pad.
//!
//! The game runs the pawn (`uCmc`) through the player pipeline: its action
//! interface writes a virtual pad (stick angle and buttons) that `uCmc::vf200`
//! (0x00B537D0) copies to the unit before `uPlayerBase::vf200`. This module
//! makes the same pad; the host feeds it to the character simulation like a
//! player's input.
//!
//! Chain (docs/PAWN_AI.md):
//! 1. the priority list ([`crate::think::build`]) is walked from the top;
//!    the first entry whose code handler takes a goal wins (0x00583CF0);
//! 2. code 4 (battle, Wait_Follow-01) follows the Arisen when it is farther
//!    than the follow range (0x00585D00); code 7 (Enemy-04) is the battle
//!    entry (0x00585880): the nearest live enemy (0x005B5020) offers the
//!    meta goal 4, which the pawn's weapon expands to its weapon goal
//!    (0x005A4B70: sword -> 52 `WpnSwordAtk`); code 6 waits;
//! 3. the weapon goal's plan picks one usable action interface by weight
//!    (0x005A4A00 over `vf16`, 50 each for the sword combos);
//! 4. the interface runs each frame (`cCmcComboActTemplate::vf18`
//!    0x0054C8B0): approach into the command's range
//!    (`AIPlActParamSword`), then press the combo's buttons, each within a
//!    timer, while the previous step's action plays.
//!
//! *Not ported* (each named where it matters): codes other than 4, 5, 6 and
//! 7 (they are listed in [`Pawn::unported`] when the walk passes them), the
//! enemy's `rAIEnemyActionParameter` filters, the support goals of meta goal
//! 0x10, the early combo stop (0x00548240), skills, chatter.

use crate::incl::{self, Active};
use crate::plact::{Range, Record, SWORD_BANK_START};
use crate::prio::PriorityThink;
use crate::think::{self, Entry, FollowState, FOLLOW, NO_COMMAND};
use std::collections::BTreeSet;

/// Goal ids (the GOAP table 0x014F3430; 4 is the weapon meta goal).
pub const GOAL_WAIT: u32 = 0;
pub const GOAL_FOLLOW: u32 = 1;
pub const GOAL_WEAPON: u32 = 4;
pub const GOAL_SWORD: u32 = 52;
pub const GOAL_WAND: u32 = 55;

/// 0x005A4B70 for meta goal 4, main weapon: the weapon type (high byte of
/// uCmc +0x3554) -> goal. Jump table 0x005A4D50: 1, 2 -> 52; 3, 7 -> 53;
/// 4 -> 54; 5, 6 -> 55. (The off hand, +0x3558, adds 56 / 57.)
// Translated from 0x005A4B70 (DDDA.exe 2017-12-13 Steam).
pub fn weapon_goal(main_type: u8) -> Option<u32> {
    match main_type {
        1 | 2 => Some(52),
        3 | 7 => Some(53),
        4 => Some(54),
        5 | 6 => Some(55),
        _ => None,
    }
}

/// One step of a combo: the action id it presses for and the early-stop
/// chance (not ported).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub action: u32,
    pub stop: f32,
}

/// A combo of `cCmcComboActTemplate` (combo table 0x0152E3B0, 16-byte
/// entries: command, steps, count).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Combo {
    /// The interface class.
    pub class: &'static str,
    /// `InterfaceID` in `WpnSwordAtk`.
    pub interface: i64,
    /// The command (0xA presses the light button, 0xE the heavy one,
    /// 0x00541D60).
    pub command: u32,
    pub steps: &'static [Step],
    /// +0x284: frames allowed for each press (constructors 0x00539C20,
    /// 0x00539D00, 0x00539DE0).
    pub wait: f32,
    /// `vf16` weight (0x0054CEE0).
    pub weight: u32,
}

/// The sword combos the pawn can pick here. `cCmcSwordComboDly` (type 1,
/// 0x0101000A..C) and the second step of `cCmcSwordComboL` (0x0101000D) are
/// actions the character simulation does not play, so Dly is left out and
/// L stops after its first step (*placeholder*: in the game the three weigh
/// 50 each).
pub const SWORD_COMBOS: [Combo; 2] = [
    Combo {
        class: "cCmcSwordCombo",
        interface: 6,
        command: 0xA,
        steps: &[
            Step {
                action: 0x0101_0000,
                stop: 0.0,
            },
            Step {
                action: 0x0101_0001,
                stop: 0.5,
            },
            Step {
                action: 0x0101_0002,
                stop: 0.5,
            },
        ],
        wait: 60.0,
        weight: 50,
    },
    Combo {
        class: "cCmcSwordComboL",
        interface: 127,
        command: 0xE,
        steps: &[Step {
            action: 0x0101_0004,
            stop: 0.0,
        }],
        wait: 30.0,
        weight: 50,
    },
];

/// The press margin of 0x0054B0A0 (50.0), on both the horizontal range and
/// the height range of 0x00542580.
pub const PRESS_MARGIN: f32 = 50.0;

/// What a pawn reads from its data.
#[derive(Debug, Clone)]
pub struct PawnData {
    pub prio: PriorityThink,
    /// Arisen order repeat cooldown, rAdjustParam m_pl000_00[0x135].
    /// Native sender reads it through the active adjust controller;
    /// equipment/skill overrides of that value are not modeled.
    pub order_cooldown: f32,
    /// `AIPlActParamSword` records (command - 0xA).
    pub sword: Vec<Record>,
}

impl PawnData {
    /// The range of `command` at tier 0 (Lv1). *Assumed* tier: 0x00780BA0
    /// picks it from the pawn's skill state, not modelled.
    pub fn range(&self, command: u32) -> Option<Range> {
        let i = command.checked_sub(SWORD_BANK_START)? as usize;
        self.sword.get(i).map(|r| r.tiers[0])
    }
}

/// An enemy the pawn senses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enemy {
    pub id: u32,
    pub pos: [f32; 3],
    pub alive: bool,
    /// Target radius (target +0x40) added to the press range.
    pub radius: f32,
}

/// What the pawn reads this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Senses<'a> {
    pub pos: [f32; 3],
    /// Facing, radians (0 faces +Z).
    pub yaw: f32,
    /// The action playing (uCmc +0x2DD4), when it is an attack.
    pub action: Option<u32>,
    /// The attack cancel slot's state (uCmc +0x2588 + (slot + 1) * 0x1C:
    /// 0 closed, 1 buffer, 2 execute).
    pub slot: u8,
    /// The Arisen the pawn serves.
    pub owner: Option<[f32; 3]>,
    pub enemies: &'a [Enemy],
    /// The battle flag (sGameSys +0xBE2CC); its writer is not traced, the
    /// host decides.
    pub battle: bool,
    /// Frames this step (uCmc +0x14).
    pub dt: f32,
}

/// The virtual pad.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pad {
    /// Where to go (world yaw), or `None` to stand.
    pub heading: Option<f32>,
    pub light: bool,
    pub heavy: bool,
}

/// A running weapon interface (`cCmcComboActTemplate`).
#[derive(Debug, Clone, PartialEq)]
pub struct ComboRun {
    pub combo: usize,
    pub target: u32,
    pub range: Range,
    /// +0xC: 1 approach, 5 pressing, 8 finishing.
    pub state: u8,
    /// +0xFC.
    pub timer: f32,
    /// +0x294.
    pub step: usize,
    /// The last step's action has been seen playing (state 8).
    pub seen_last: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Goal {
    Wait,
    /// Follow the Arisen until within `min`.
    Follow {
        min: f32,
    },
    Attack(ComboRun),
}

/// Why a goal ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Failed, and why.
    Failed(&'static str),
}

#[derive(Debug, Clone)]
pub struct Pawn {
    pub inclinations: [f32; 9],
    pub active: Vec<Active>,
    /// Party command (8 none).
    pub command: i64,
    /// uCmc order mailbox and cAICheckSituationCmc clocks.
    pub orders: crate::commands::Clock,
    /// Main weapon type (uCmc +0x3554 high byte; 1 a sword).
    pub weapon: u8,
    pub list: Vec<Entry>,
    pub goal: Option<Goal>,
    /// Goals taken and how they ended, for hosts and tests.
    pub history: Vec<(u32, Option<Outcome>)>,
    /// Codes the walk passed without a port.
    pub unported: BTreeSet<i64>,
    /// *Placeholder* random source (the game's 0x00D00690 is global).
    pub rng: u32,
}

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn yaw_to(from: [f32; 3], to: [f32; 3]) -> f32 {
    (to[0] - from[0]).atan2(to[2] - from[2])
}

impl Pawn {
    pub fn new(data: &PawnData, inclinations: [f32; 9], weapon: u8, seed: u32) -> Self {
        let active = incl::active(&inclinations);
        let list = think::build(&data.prio, &active, NO_COMMAND);
        Pawn {
            inclinations,
            active,
            command: NO_COMMAND,
            orders: crate::commands::Clock::default(),
            weapon,
            list,
            goal: None,
            history: vec![],
            unported: BTreeSet::new(),
            rng: seed.max(1),
        }
    }

    /// A new party command rebuilds the list (0x005AC240 runs on a command
    /// change).
    pub fn set_command(&mut self, data: &PawnData, command: i64) {
        self.command = command;
        self.list = think::build(&data.prio, &self.active, command);
    }

    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// The follow range in battle (0x00598390 -> table 0x0152DAB8).
    pub fn follow_range(&self) -> think::FollowRange {
        FOLLOW[think::follow_mode(
            &self.active,
            FollowState {
                command: self.command,
                ..Default::default()
            },
        )]
    }

    /// 0x00583CF0: walk the list; the first handler that takes a goal wins.
    fn think(&mut self, data: &PawnData, s: &Senses) -> Option<(u32, Goal)> {
        let list = std::mem::take(&mut self.list);
        let mut out = None;
        for e in &list {
            match e.param.code {
                // 0x00585D00: in battle, follow when farther than the range.
                4 => {
                    if !s.battle {
                        continue;
                    }
                    let r = self.follow_range();
                    if s.owner.is_some_and(|o| dist_xz(s.pos, o) > r.max) {
                        out = Some((GOAL_FOLLOW, Goal::Follow { min: r.min }));
                        break;
                    }
                }
                // 0x00585FA0: out of battle, follow. *Placeholder*: its
                // range (0x00598670 / 0x00597FF0, another table set) is
                // taken as the battle table's row 2.
                5 => {
                    if s.battle {
                        continue;
                    }
                    let r = FOLLOW[2];
                    if s.owner.is_some_and(|o| dist_xz(s.pos, o) > r.max) {
                        out = Some((GOAL_FOLLOW, Goal::Follow { min: r.min }));
                        break;
                    }
                }
                // 0x00586200: wait (goal 0).
                6 => {
                    out = Some((GOAL_WAIT, Goal::Wait));
                    break;
                }
                // 0x00585880 -> 0x005907F0 -> 0x0058FDE0: the battle goals.
                7 => {
                    if !s.battle {
                        continue;
                    }
                    if let Some(run) = self.plan_attack(data, s) {
                        out = Some((weapon_goal(self.weapon).unwrap_or(GOAL_WEAPON), Goal::Attack(run)));
                        break;
                    }
                }
                code => {
                    self.unported.insert(code);
                }
            }
        }
        self.list = list;
        out
    }

    /// Meta goal 4 on the nearest live enemy, expanded by the weapon, then
    /// a weighted pick among the combos (0x005A4A00: rand % sum).
    fn plan_attack(&mut self, data: &PawnData, s: &Senses) -> Option<ComboRun> {
        if weapon_goal(self.weapon) != Some(GOAL_SWORD) {
            return None;
        }
        let target = s
            .enemies
            .iter()
            .filter(|e| e.alive)
            .min_by(|a, b| dist_xz(s.pos, a.pos).total_cmp(&dist_xz(s.pos, b.pos)))?;
        let sum: u32 = SWORD_COMBOS.iter().map(|c| c.weight).sum();
        let mut roll = self.rand() % sum;
        let mut pick = 0;
        for (i, c) in SWORD_COMBOS.iter().enumerate() {
            if roll < c.weight {
                pick = i;
                break;
            }
            roll -= c.weight;
        }
        let range = data.range(SWORD_COMBOS[pick].command)?;
        Some(ComboRun {
            combo: pick,
            target: target.id,
            range,
            state: 1,
            timer: 0.0,
            step: 0,
            seen_last: false,
        })
    }

    /// One frame: run the goal (or think first) and return the pad.
    pub fn step(&mut self, data: &PawnData, s: &Senses) -> Pad {
        if self.orders.step(s.dt, true) {
            self.set_command(data, self.orders.active as i64);
        }
        if self.goal.is_none() {
            if let Some((id, g)) = self.think(data, s) {
                self.history.push((id, None));
                self.goal = Some(g);
            }
        }
        let Some(goal) = self.goal.clone() else {
            return Pad::default();
        };
        let (pad, end) = match goal {
            Goal::Wait => (Pad::default(), Some(Outcome::Done)),
            Goal::Follow { min } => match s.owner {
                Some(o) if dist_xz(s.pos, o) > min => (
                    Pad {
                        heading: Some(yaw_to(s.pos, o)),
                        ..Pad::default()
                    },
                    None,
                ),
                _ => (Pad::default(), Some(Outcome::Done)),
            },
            Goal::Attack(mut run) => {
                let r = combo_step(&mut run, s);
                self.goal = Some(Goal::Attack(run));
                r
            }
        };
        if let Some(o) = end {
            self.goal = None;
            if let Some(h) = self.history.last_mut() {
                h.1 = Some(o);
            }
        }
        pad
    }
}

/// `cCmcComboActTemplate::vf18` (0x0054C8B0) for the sword combos:
/// - 1: approach until the horizontal distance is within the command's
///   range (0x00544300; the minimum only counts when above ~0);
/// - 5: count the press timer down (fail at 0); press when the previous
///   step's action plays (uCmc +0x2DD4), after the range check of
///   0x0054B0A0 (min - 50 .. max + target radius + 50; the `Enable` pair
///   when its maximum is larger) and its height check 0x00542580
///   (`RangeMinY` / `RangeMaxY` widened by 50 and by half an unidentified
///   height, taken as 0 here), else fail; the press is taken while the
///   attack cancel slot executes (0x00536A90), each press restarting the
///   timer;
/// - 8: done when the attack has ended (0x0054B660, *assumed*: no attack
///   action playing).
///
/// A dead or missing target fails the run (the game drops it when the
/// goal list is rebuilt, 0x0058FDE0).
// Translated from 0x0054C8B0 (DDDA.exe 2017-12-13 Steam).
fn combo_step(run: &mut ComboRun, s: &Senses) -> (Pad, Option<Outcome>) {
    let combo = &SWORD_COMBOS[run.combo];
    let fail = |why| (Pad::default(), Some(Outcome::Failed(why)));
    let target = match s.enemies.iter().find(|e| e.id == run.target) {
        Some(t) if t.alive => *t,
        _ if run.state == 8 => return finish(run, s),
        _ => return fail("target gone"),
    };
    let d = dist_xz(s.pos, target.pos);
    let toward = yaw_to(s.pos, target.pos);
    match run.state {
        1 => {
            let near = run.range.min_xz > 1.192_092_9e-7 && d < run.range.min_xz;
            if near || d > run.range.max_xz {
                return (
                    Pad {
                        heading: Some(toward),
                        ..Pad::default()
                    },
                    None,
                );
            }
            run.state = 5;
            run.timer = combo.wait;
            (Pad::default(), None)
        }
        5 => {
            run.timer -= s.dt;
            if run.timer <= 0.0 {
                return fail("press timer");
            }
            let ready = run.step == 0 || s.action == Some(combo.steps[run.step - 1].action);
            if !ready {
                return (Pad::default(), None);
            }
            let (min, max) = if run.range.max_xz < run.range.enable_max_xz {
                (run.range.enable_min_xz, run.range.enable_max_xz)
            } else {
                (run.range.min_xz, run.range.max_xz)
            };
            let lo = min - PRESS_MARGIN;
            let hi = max + target.radius + PRESS_MARGIN;
            if (lo > 1.192_092_9e-7 && d < lo) || d > hi {
                return fail("out of range");
            }
            let dy = target.pos[1] - s.pos[1];
            if dy < run.range.min_y - PRESS_MARGIN || dy > run.range.max_y + PRESS_MARGIN {
                return fail("height");
            }
            // 0x00541A30 -> 0x00541D60 -> 0x00536A90: the press is taken
            // only while the slot executes (2; the buffer state 1 needs flag
            // bit 5, which the sword combos lack); otherwise try again next
            // frame.
            if s.slot != 2 {
                return (Pad::default(), None);
            }
            let pad = Pad {
                heading: None,
                light: combo.command == 0xA,
                heavy: combo.command == 0xE,
            };
            run.timer = combo.wait;
            run.step += 1;
            if run.step >= combo.steps.len() {
                run.state = 8;
            }
            (pad, None)
        }
        _ => finish(run, s),
    }
}

/// State 8: done once the last step's action has played and ended; the
/// press timer still runs (*assumed*: a press the character never took
/// would otherwise hang the goal).
fn finish(run: &mut ComboRun, s: &Senses) -> (Pad, Option<Outcome>) {
    let last = SWORD_COMBOS[run.combo].steps.last().map(|st| st.action);
    if s.action.is_some() && s.action == last {
        run.seen_last = true;
    }
    if run.seen_last && s.action.is_none() {
        return (Pad::default(), Some(Outcome::Done));
    }
    run.timer -= s.dt;
    if !run.seen_last && run.timer <= 0.0 {
        return (Pad::default(), Some(Outcome::Failed("last step never played")));
    }
    (Pad::default(), None)
}
