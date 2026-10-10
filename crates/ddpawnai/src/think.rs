//! The priority think's order and the follow distance.
//!
//! `cAIPriorityThink` (pawn AI object +0x70) sorts the entries of
//! `AI\PrioThink\cmc` into (group, level) lists once at start and whenever
//! the party command changes or expires (0x005AC240). The think walks those
//! lists from the highest priority down and runs each entry's code handler
//! (dispatcher 0x005851D0, loop 0x00583CF0); the first handler that
//! requests a goal wins.

use crate::incl::{self, Active};
use crate::prio::{PrioParam, PriorityThink};

/// The six groups in priority order (property order of the resource; the
/// build walks `g = 0..6`).
pub const GROUPS: [&str; 6] = [
    "QUEST",
    "PL_Party",
    "Situ_Personal",
    "Enemy",
    "Wait_Follow",
    "Etc",
];

/// No party command (`situation +0x52C == 8`).
pub const NO_COMMAND: i64 = 8;

/// One entry placed by the build.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Group index into [`GROUPS`].
    pub group: u8,
    /// The level it was authored at (`<group> - NN`).
    pub authored: u8,
    /// The level the build placed it at.
    pub level: u8,
    /// Position in its authored list.
    pub index: u32,
    pub param: PrioParam,
}

/// 0x005AC240 for a type-1 table: each entry's level is its authored level
/// plus the `加算値S32` of every inclination value whose checks all pass
/// (stopping after the first applied one with `適用したら抜ける`); with a
/// party command (not [`NO_COMMAND`]) a matching `命令` sets the level
/// outright. A level outside 0..=7 falls back to the authored one. The
/// result is sorted by (level, group, placement order): level 0 first,
/// then QUEST before PL_Party ... before Etc.
// Translated from 0x005AC240 (DDDA.exe 2017-12-13 Steam).
pub fn build(think: &PriorityThink, active: &[Active], command: i64) -> Vec<Entry> {
    // lists[(g, l)] in placement order
    let mut lists: Vec<Vec<Entry>> = vec![Vec::new(); 6 * 8];
    for (g, name) in GROUPS.iter().enumerate() {
        for l in 0..8u32 {
            let Some(group) = think.group(name, l) else {
                continue;
            };
            for (index, p) in group.params.iter().enumerate() {
                let mut add = 0i64;
                for v in &p.personal {
                    if v.checks
                        .iter()
                        .all(|c| incl::passes(active, c.personality, c.state))
                    {
                        add += v.add_s32;
                        if v.stop_after_apply {
                            break;
                        }
                    }
                }
                let mut lvl = add + i64::from(l);
                if command != NO_COMMAND {
                    for o in &p.orders {
                        if o.order_type == command {
                            lvl = o.value;
                        }
                    }
                }
                let level = if (0..8).contains(&lvl) { lvl as u32 } else { l };
                lists[g * 8 + level as usize].push(Entry {
                    group: g as u8,
                    authored: l as u8,
                    level: level as u8,
                    index: index as u32,
                    param: p.clone(),
                });
            }
        }
    }
    let mut out = Vec::new();
    for level in 0..8 {
        for g in 0..6 {
            out.extend(lists[g * 8 + level].iter().cloned());
        }
    }
    out
}

/// One row of the follow table (pointer 0x017f4170 -> 0x0152dab8, 7 rows
/// of 0x14 bytes): the pawn follows when farther than `max` (cm, XZ) and
/// the move ends inside `min`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowRange {
    pub max: f32,
    pub min: f32,
}

/// The follow table's seven rows (`.rdata` 0x0152dab8; +0x00 max, +0x10
/// min).
pub const FOLLOW: [FollowRange; 7] = [
    FollowRange { max: 500.0, min: 0.0 },
    FollowRange {
        max: 750.0,
        min: 500.0,
    },
    FollowRange {
        max: 1500.0,
        min: 750.0,
    },
    FollowRange {
        max: 2500.0,
        min: 1500.0,
    },
    FollowRange {
        max: 3500.0,
        min: 2500.0,
    },
    FollowRange {
        max: 7000.0,
        min: 3500.0,
    },
    FollowRange {
        max: 5000.0,
        min: 3500.0,
    },
];

/// What 0x00598390 reads besides the inclinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FollowState {
    /// The party command (`situation +0x52C`; 8 none, 9 / 10 / 11 the
    /// commands; 9 Go, 10 Come, 11 Help, from sender 0x00B768E0).
    pub command: i64,
    /// 0x005985A0: the battle target is one of the large enemies (unit
    /// byte +0x2D in 'P', 'R', 'Z', '[', 0xAE, 0xB8, 0xB9) or sGameSys
    /// +0xAC6EC bit 26.
    pub large_enemy: bool,
    /// sGameSys +0x34 == 0x262.
    pub area_262: bool,
}

/// 0x00598390: the follow mode (an index into [`FOLLOW`]).
/// - Pioneer (7) at status 2: command 10 -> 2, else 3, or 4 with command 9;
/// - Guardian (5) or Medicant (1) at status 2: 2, or 3 with command 9;
/// - otherwise command 10 -> 1, else 2, or 4 with command 9;
/// - outside the command-10 cases a large enemy makes it 5;
/// - area 0x262 makes it 6.
// Translated from 0x00598390 (DDDA.exe 2017-12-13 Steam).
pub fn follow_mode(active: &[Active], s: FollowState) -> usize {
    let nine = usize::from(s.command == 9);
    let mode = if incl::has(active, incl::PIONEER, 2) {
        if s.command == 10 {
            return if s.area_262 { 6 } else { 2 };
        }
        3 + nine
    } else if incl::has(active, incl::GUARDIAN, 2) || incl::has(active, incl::MEDICANT, 2) {
        2 + nine
    } else {
        if s.command == 10 {
            return if s.area_262 { 6 } else { 1 };
        }
        2 + 2 * nine
    };
    if s.area_262 {
        6
    } else if s.large_enemy {
        5
    } else {
        mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_modes() {
        let none: Vec<Active> = vec![];
        let st = |command| FollowState {
            command,
            ..FollowState::default()
        };
        assert_eq!(follow_mode(&none, st(8)), 2);
        assert_eq!(follow_mode(&none, st(10)), 1);
        assert_eq!(follow_mode(&none, st(9)), 4);
        let guard = [Active {
            inclination: incl::GUARDIAN,
            status: 2,
        }];
        assert_eq!(follow_mode(&guard, st(8)), 2);
        assert_eq!(follow_mode(&guard, st(9)), 3);
        let pioneer = [Active {
            inclination: incl::PIONEER,
            status: 2,
        }];
        assert_eq!(
            (
                follow_mode(&pioneer, st(8)),
                follow_mode(&pioneer, st(9)),
                follow_mode(&pioneer, st(10))
            ),
            (3, 4, 2)
        );
        let big = FollowState {
            command: 8,
            large_enemy: true,
            area_262: false,
        };
        assert_eq!(follow_mode(&none, big), 5);
        assert_eq!(follow_mode(&none, FollowState { command: 10, ..big }), 1);
        assert_eq!(
            follow_mode(
                &none,
                FollowState {
                    area_262: true,
                    ..big
                }
            ),
            6
        );
    }
}
