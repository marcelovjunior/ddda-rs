//! Kill records and respawn by days.
//!
//! `sSetManager` keeps one 16-byte record per group slot of the stage
//! (`+0x18A90` block, records from `+0x18A98`, 295 slots, saved with the
//! game). The record's first two words, as one 64-bit value:
//! - bit 0: the record is in use (a unit of the group died);
//! - bits 1..7: the stage index (`0x01530348` table), bits 8..16: the
//!   group;
//! - bits 17..19: the respawn type copied from the group (`+0x58` bits 0..2);
//! - bits 20..22 (type 6: 20..27): days left;
//! - bits 23..24 / 25..26: the partial-respawn step and its growth per day
//!   (type 1);
//! - bits 28..59: dead units (one bit per unit of the group).
//!
//! Written at a kill by 0x004A7AD0 / 0x004A7C10 / 0x0049F5D0; aged by
//! `sSetManager::vf06` (0x004A40D0) through 0x004A8150 when the day counter
//! (sGameSys +0xB8760) moves.

use crate::gpl::Group;

/// One group's record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Record {
    pub bits: u64,
}

impl Record {
    pub fn in_use(self) -> bool {
        self.bits & 1 != 0
    }
    pub fn kind(self) -> u32 {
        (self.bits >> 17) as u32 & 7
    }
    pub fn days(self) -> u32 {
        let w = (self.bits >> 20) as u32;
        if self.kind() == 6 {
            w & 0xff
        } else {
            w & 7
        }
    }
    /// Unit `i` (0..32) is dead (0x004A6510).
    // Translated from 0x004A6510 (DDDA.exe 2017-12-13 Steam).
    pub fn dead(self, i: u32) -> bool {
        self.in_use() && i < 32 && (self.bits >> 28) & (1 << i) != 0
    }
}

/// A kill of unit `unit` of `group` (0x004A7AD0): the first kill opens the
/// record and copies the group's respawn condition (0x004A7C10); every
/// kill marks its unit dead (0x0049F5D0) unless the type is 5.
// Translated from 0x004A7AD0, 0x004A7C10 and 0x0049F5D0 (DDDA.exe
// 2017-12-13 Steam).
pub fn kill(rec: &mut Record, group: &Group, stage_index: u32, unit: u32) {
    let mut b = rec.bits;
    if b & 1 == 0 {
        b |= 1;
        b = (b & !0xfe) | (u64::from(stage_index & 0x7f) << 1);
        b = (b & !(0x1ff << 8)) | (u64::from(group.slot & 0x1ff) << 8);
        let r = group.respawn;
        match r & 7 {
            2 => {
                b = (b & !(0x7 << 17)) | (2 << 17);
                b = (b & !(0xff << 20)) | (u64::from(group.layout_flags & 0xff) << 20);
            }
            1 => {
                b = (b & !(0x7 << 17)) | (1 << 17);
                b = (b & !(0x7 << 20)) | (u64::from((r >> 3) & 7) << 20);
                b = (b & !(0x3 << 23)) | (u64::from((r >> 11) & 3) << 23);
                b = (b & !(0x3 << 25)) | (u64::from((r >> 13) & 3) << 25);
            }
            6 => {
                b = (b & !(0x7 << 17)) | (6 << 17);
                b = (b & !(0xff << 20)) | (u64::from((r >> 3) & 0xff) << 20);
            }
            _ => {}
        }
    }
    if (b >> 17) & 7 != 5 && unit < 32 {
        b |= 1 << (28 + unit);
    }
    rec.bits = b;
}

/// The three partial-respawn masks (75 %, 50 %, 25 %: a unit's bit is set
/// when its roll of `rand % 100` is above 24, 49 or 74); index 0 is no
/// respawn.
pub fn chance_masks(mut roll: impl FnMut() -> u32) -> [u64; 4] {
    let mut m = [0u64; 4];
    for i in 0..64 {
        let r = roll() % 100;
        if r > 0x4a {
            m[3] |= 1 << i;
        }
        if r > 0x31 {
            m[2] |= 1 << i;
        }
        if r > 0x18 {
            m[1] |= 1 << i;
        }
    }
    m
}

/// `days` days passed (0x004A8150): type 1 counts down and, while counting,
/// keeps each dead unit dead only where the current step's mask says so
/// (the step grows by its growth times the days, up to 3); types 5 and 6
/// count down; at 0 the record is cleared and the whole group returns.
/// Other types never age (0 spawns once; 2 waits for its layout flag).
// Translated from 0x004A8150 (DDDA.exe 2017-12-13 Steam).
pub fn age(rec: &mut Record, days: u32, masks: &[u64; 4]) {
    let b = rec.bits;
    if b & 1 == 0 {
        return;
    }
    let kind = (b >> 17) & 7;
    let mask = match kind {
        1 | 5 => 7u64,
        6 => 0xff,
        _ => return,
    };
    let left = (((b >> 20) & mask) as i64 - i64::from(days)).max(0) as u64;
    let mut nb = (b & !(mask << 20)) | (left << 20);
    if left == 0 {
        // LAB_004A8402: not in use, stage bits 0x7f, the dead units cleared
        rec.bits = ((nb & !1) | 0xfe) & !(0xffff_ffffu64 << 28);
        return;
    }
    if kind == 1 {
        let step = (nb >> 23) & 3;
        if step != 0 {
            let dead = (nb >> 28) & 0xffff_ffff;
            let keep = masks[step as usize] & 0xffff_ffff;
            nb = (nb & !(0xffff_ffffu64 << 28)) | ((dead & keep) << 28);
        }
        let grow = (nb >> 25) & 3;
        let s = (grow * u64::from(days) + ((nb >> 23) & 3)).min(3);
        nb = (nb & !(3 << 23)) | (s << 23);
    }
    rec.bits = nb;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpl::{Group, Scalars};

    fn group(respawn: u32) -> Group {
        Group {
            slot: 250,
            head: (0, 0),
            units: vec![],
            cells: vec![],
            s: Scalars([0; 41]),
            areas1: vec![],
            life: vec![],
            areas2: vec![],
            tail: (0, 0, 0, 0),
            scene_window: None,
            layout_flag: None,
            layout_flag2: None,
            layout_flags: 0,
            hours: None,
            disabled: false,
            respawn,
            despawn: 0,
            other_conditions: vec![],
        }
    }

    const NONE: [u64; 4] = [0; 4];

    #[test]
    fn type1_one_day() {
        // type 1, 1 day, no partial respawn
        let g = group(1 | 1 << 3);
        let mut r = Record::default();
        kill(&mut r, &g, 13, 0);
        kill(&mut r, &g, 13, 2);
        assert!(r.in_use() && r.dead(0) && !r.dead(1) && r.dead(2));
        assert_eq!((r.kind(), r.days()), (1, 1));
        age(&mut r, 0, &NONE);
        assert!(r.dead(0));
        age(&mut r, 1, &NONE);
        assert!(!r.in_use() && !r.dead(0) && !r.dead(2));
    }

    #[test]
    fn type0_never_returns() {
        let g = group(0);
        let mut r = Record::default();
        kill(&mut r, &g, 13, 1);
        age(&mut r, 30, &NONE);
        assert!(r.dead(1));
    }

    #[test]
    fn type1_partial_respawn_by_chance() {
        // 3 days, step 1, growth 1: each day keeps dead only where the mask says
        let g = group(1 | 3 << 3 | 1 << 11 | 1 << 13);
        let mut r = Record::default();
        for u in 0..4 {
            kill(&mut r, &g, 13, u);
        }
        let masks = [0, 0b0101, 0b0001, 0];
        age(&mut r, 1, &masks);
        assert!(r.dead(0) && !r.dead(1) && r.dead(2) && !r.dead(3));
        assert_eq!(r.days(), 2);
        age(&mut r, 1, &masks); // step 2 now: mask 0b0001
        assert!(r.dead(0) && !r.dead(2));
        age(&mut r, 1, &masks);
        assert!(!r.in_use());
    }

    #[test]
    fn masks_by_roll() {
        let mut i = 0u32;
        let m = chance_masks(|| {
            i += 1;
            [10, 30, 60, 90][(i as usize - 1) % 4]
        });
        // unit 0 rolled 10 (none), 1 rolled 30 (75 %), 2 rolled 60, 3 rolled 90
        assert_eq!((m[1] & 0xf, m[2] & 0xf, m[3] & 0xf), (0b1110, 0b1100, 0b1000));
    }
}
