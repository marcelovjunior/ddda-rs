//! The saved story state ("world"): what sGameSys keeps for the story.
//!
//! One `World` per session, owned by the host (docs/COOP_DESIGN.md).
//! Offsets are sGameSys (`*0x018fa4bc`) offsets in DDDA.exe 2017-12-13.

use ddquest::story::MsbBits;

/// Stage numbers in index order (u16 table at 0x01530348, 65 entries).
/// Per-stage save data (layout flags, AreaHitFlag) is addressed by this
/// index; stages not in it have none.
pub const STAGE_INDEX: [u16; 65] = [
    100, 200, 210, 220, 230, 240, 250, 300, 310, 320, 330, 370, 380, 500, 501, 502, 600, 601, 602, 603, 604, 605,
    606, 607, 608, 609, 610, 700, 701, 702, 703, 704, 705, 706, 611, 615, 800, 801, 802, 803, 400, 401, 402, 405,
    406, 410, 411, 413, 420, 421, 423, 424, 425, 430, 431, 435, 436, 440, 443, 444, 445, 446, 447, 450, 804,
];

pub fn stage_index(stage: u32) -> Option<usize> {
    STAGE_INDEX.iter().position(|&s| s as u32 == stage)
}

pub const GSF_COUNT: usize = 0x600;
pub const QUEST_COUNT: usize = 128;
pub const QUEST_BITS: usize = 96;
/// Layout ("lot") flags per stage: 128 in cSTAGE_FLAG Flag000..096; stage
/// 100 has 128 more at +0xb3330 (QUESTS.md, 0x00443ca0).
pub const LOT_FLAGS: usize = 128;
pub const LOT_FLAGS_ST100: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub struct World {
    /// Scenario number, +0x30.
    pub sce_no: u32,
    /// Band derived from SceNo by 0x0043d750, +0x3c.
    pub sce_band: u32,
    /// Stage to load / loaded (`mStageNo`, +0x34).
    pub stage_no: u32,
    /// Start position number in that stage (+0x44).
    pub start_pos: u32,
    pub gsf: MsbBits,
    quests: Vec<MsbBits>,
    lot: Vec<MsbBits>,
    area_hit: Vec<MsbBits>,
    /// AreaStayFlag word, cleared every frame by sScenario (+0xb884c).
    pub area_stay: u32,
    /// Clock: day counter (+0xb8760, u64), ms of day (+0xb8768), hour
    /// (+0xb876c) and minute (+0xb8770).
    pub day: u64,
    pub ms: u32,
    pub hour: u32,
    pub minute: u32,
    /// Active quest (+0xbe30c), -1 = none.
    pub active_quest: i32,
    /// Player record +0xc (QCT judgments 23/24; gendered text).
    pub player_sex: u8,
    /// NPC friendship (sNpcManager +0x8b8 + npc*0x110, default 500).
    pub npc_friend: Vec<i16>,
    /// NPC death bits, 900 each, MSB-first: `mNpcAppDieFlag`
    /// (+0xacfec, getter 0x00435710) and +0xad060 (`mNpcDieFlag`,
    /// inferred name; getter 0x00435740).
    pub npc_app_die: MsbBits,
    pub npc_die: MsbBits,
    /// cSTAGE_FLAG mOmFlag, 0x200 per stage index (IsOmFlag 0x004f7060).
    om: Vec<MsbBits>,
    /// sGameSys +0xb3360 (CaughtNpcId 0x005bb5a0).
    pub caught_npc: i32,
    /// sGameSys +0xa76e0 (JobPl 0x005bb010).
    pub player_job: u32,
}

impl Default for World {
    fn default() -> Self {
        World {
            sce_no: 0,
            sce_band: 0,
            stage_no: 0,
            start_pos: 0,
            gsf: MsbBits::new(GSF_COUNT),
            quests: (0..QUEST_COUNT).map(|_| MsbBits::new(QUEST_BITS)).collect(),
            lot: STAGE_INDEX
                .iter()
                .map(|&s| MsbBits::new(if s == 100 { LOT_FLAGS_ST100 } else { LOT_FLAGS }))
                .collect(),
            area_hit: STAGE_INDEX.iter().map(|_| MsbBits::new(32)).collect(),
            area_stay: 0,
            day: 0,
            ms: 0,
            hour: 0,
            minute: 0,
            active_quest: -1,
            player_sex: 0,
            npc_friend: vec![500; 900],
            npc_app_die: MsbBits::new(900),
            npc_die: MsbBits::new(900),
            om: STAGE_INDEX.iter().map(|_| MsbBits::new(0x200)).collect(),
            caught_npc: -1,
            player_job: 0,
        }
    }
}

/// SceNo band (sGameSys +0x3c) as 0x0043d750 computes it. The 4600..4799
/// test for band 5 comes after the 4600..6399 test for band 6, so 5 is
/// never produced.
// Translated from 0x0043d7d8..0x0043d930 (DDDA.exe 2017-12-13 Steam).
pub fn sce_band(v: u32) -> u32 {
    let v = v as i32;
    let r = |lo: i32, hi: i32| (lo..=hi).contains(&v);
    if r(9050, 10000) {
        14
    } else if r(8000, 9049) {
        12
    } else if r(7800, 7999) {
        11
    } else if r(7200, 7799) {
        10
    } else if r(6800, 7199) {
        9
    } else if r(6550, 6799) {
        8
    } else if r(6400, 6549) {
        7
    } else if r(4600, 6399) {
        6
    } else if r(3600, 4599) {
        4
    } else if r(2850, 3599) {
        3
    } else if r(1801, 2849) {
        2
    } else if r(1000, 1800) {
        1
    } else if r(0, 999) {
        13
    } else {
        0
    }
}

impl World {
    /// The SceNo setter 0x0043d750(value, raise_only). With `raise_only`
    /// a value not above the current one is ignored. Sets GSF 530 once
    /// SceNo >= 6520 (0x0043d78c), updates the band. Returns whether it
    /// changed. Not modelled: its calls into other managers (0x004861e0,
    /// the active-quest marker reset, the 800/3600 special cases).
    // Translated from 0x0043d750 (DDDA.exe 2017-12-13 Steam).
    pub fn set_sce_no(&mut self, value: u32, raise_only: bool) -> bool {
        if raise_only && (value as i32) <= self.sce_no as i32 {
            return false;
        }
        self.sce_no = value;
        if value as i32 > 0x1977 {
            self.gsf.set(530, true);
        }
        self.sce_band = sce_band(value);
        true
    }

    /// 0x0043f070(hour, minute, advance = 1): set the clock; a time not
    /// later than the current one starts the next day. Not modelled: the
    /// weather and party notifications and the elapsed-time accumulator.
    // Translated from 0x0043f070 (DDDA.exe 2017-12-13 Steam), clock part.
    pub fn set_clock(&mut self, hour: u32, minute: u32) {
        let ms = (minute + 60 * hour) * 60_000;
        if ms <= self.ms {
            self.day += 1;
        }
        self.ms = ms;
        self.hour = hour;
        self.minute = minute;
    }

    pub fn quest_bit(&self, quest: usize, bit: usize) -> bool {
        self.quests.get(quest).is_some_and(|q| q.get(bit))
    }
    /// 0x00437520 (set) / 0x00437590 (clear); quests >= 128 ignored.
    pub fn set_quest_bit(&mut self, quest: usize, bit: usize, v: bool) {
        if let Some(q) = self.quests.get_mut(quest) {
            q.set(bit, v);
        }
    }
    pub fn quest_bits(&self, quest: usize) -> Option<&MsbBits> {
        self.quests.get(quest)
    }

    /// Layout flag test 0x00437610; unknown stages read false.
    pub fn lot(&self, stage: u32, flag: usize) -> bool {
        stage_index(stage).is_some_and(|i| self.lot[i].get(flag))
    }
    /// Layout flag set 0x00443ca0 / clear 0x00443f20; unknown stages ignored.
    pub fn set_lot(&mut self, stage: u32, flag: usize, v: bool) {
        if let Some(i) = stage_index(stage) {
            self.lot[i].set(flag, v);
        }
    }

    /// AreaHitFlag (getter 0x007abc70 -> 0x00523110: cSTAGE_FLAG +0x18 of
    /// the current stage, MSB-first).
    pub fn area_hit(&self, stage: u32, n: usize) -> bool {
        n < 32 && stage_index(stage).is_some_and(|i| self.area_hit[i].get(n))
    }
    pub fn set_area_hit(&mut self, stage: u32, n: usize, v: bool) {
        if let Some(i) = stage_index(stage) {
            self.area_hit[i].set(n, v);
        }
    }
    pub fn om_flag(&self, stage: u32, n: usize) -> bool {
        n < 0x200 && stage_index(stage).is_some_and(|i| self.om[i].get(n))
    }

    /// Revive NPC (0x00446540): clear both death bits.
    pub fn revive_npc(&mut self, npc: usize) {
        self.npc_app_die.set(npc, false);
        self.npc_die.set(npc, false);
    }
    /// Kill NPC (0x00446460): only if not already dead; sets both bits.
    /// Not modelled: mNpcDieDate and the intel/manager calls.
    pub fn kill_npc(&mut self, npc: usize) {
        if npc < 900 && !self.npc_die.get(npc) {
            self.npc_app_die.set(npc, true);
            self.npc_die.set(npc, true);
        }
    }

    /// AreaStayFlag getter 0x005bae00: MSB-first in one word.
    pub fn area_stay(&self, n: usize) -> bool {
        n < 32 && self.area_stay & (0x8000_0000 >> n) != 0
    }

    // ---- save / load -------------------------------------------------
    //
    // A plain versioned little-endian dump of this struct. It is this
    // project's own format (the original's save is XML, see claude-5's
    // save notes); fields keep the original's bit order.

    pub fn save(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"DDST");
        let mut w = |v: u32| b.extend_from_slice(&v.to_le_bytes());
        w(1);
        for v in [self.sce_no, self.sce_band, self.stage_no, self.start_pos, self.area_stay, self.hour, self.minute] {
            w(v);
        }
        w(self.day as u32);
        w((self.day >> 32) as u32);
        w(self.ms);
        w(self.active_quest as u32);
        w(self.player_sex as u32);
        let banks: Vec<&MsbBits> = std::iter::once(&self.gsf)
            .chain(self.quests.iter())
            .chain(self.lot.iter())
            .chain(self.area_hit.iter())
            .collect();
        for bank in banks {
            for &x in bank.words() {
                w(x);
            }
        }
        for &f in &self.npc_friend {
            w(f as u16 as u32);
        }
        for bank in [&self.npc_app_die, &self.npc_die].into_iter().chain(self.om.iter()) {
            for &x in bank.words() {
                w(x);
            }
        }
        w(self.caught_npc as u32);
        w(self.player_job);
        b
    }

    pub fn load(b: &[u8]) -> Result<World, String> {
        if b.get(..4) != Some(b"DDST") {
            return Err("not a ddstory save".into());
        }
        struct R<'a>(std::slice::ChunksExact<'a, u8>);
        impl R<'_> {
            fn u(&mut self) -> Result<u32, String> {
                self.0
                    .next()
                    .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                    .ok_or_else(|| "truncated save".to_string())
            }
            fn bank(&mut self, len: usize) -> Result<MsbBits, String> {
                let v: Vec<u32> = (0..len.div_ceil(32)).map(|_| self.u()).collect::<Result<_, _>>()?;
                Ok(MsbBits::from_words(len, &v))
            }
        }
        let mut r = R(b[4..].chunks_exact(4));
        if r.u()? != 1 {
            return Err("unsupported save version".into());
        }
        let mut w = World {
            sce_no: r.u()?,
            sce_band: r.u()?,
            stage_no: r.u()?,
            start_pos: r.u()?,
            area_stay: r.u()?,
            hour: r.u()?,
            minute: r.u()?,
            day: r.u()? as u64 | (r.u()? as u64) << 32,
            ms: r.u()?,
            active_quest: r.u()? as i32,
            player_sex: r.u()? as u8,
            ..World::default()
        };
        w.gsf = r.bank(GSF_COUNT)?;
        for q in 0..QUEST_COUNT {
            w.quests[q] = r.bank(QUEST_BITS)?;
        }
        for (i, &s) in STAGE_INDEX.iter().enumerate() {
            w.lot[i] = r.bank(if s == 100 { LOT_FLAGS_ST100 } else { LOT_FLAGS })?;
        }
        for i in 0..STAGE_INDEX.len() {
            w.area_hit[i] = r.bank(32)?;
        }
        for i in 0..w.npc_friend.len() {
            w.npc_friend[i] = r.u()? as u16 as i16;
        }
        w.npc_app_die = r.bank(900)?;
        w.npc_die = r.bank(900)?;
        for i in 0..STAGE_INDEX.len() {
            w.om[i] = r.bank(0x200)?;
        }
        w.caught_npc = r.u()? as i32;
        w.player_job = r.u()?;
        if !r.0.remainder().is_empty() || r.0.next().is_some() {
            return Err("trailing bytes in save".into());
        }
        Ok(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_table_matches_0x0043d750() {
        assert_eq!(sce_band(100), 13);
        assert_eq!(sce_band(1000), 1);
        assert_eq!(sce_band(1800), 1);
        assert_eq!(sce_band(1801), 2);
        // Live: SceNo 3400 -> band 3 (oracle, 2026-10-09).
        assert_eq!(sce_band(3400), 3);
        assert_eq!(sce_band(4700), 6);
        assert_eq!(sce_band(9100), 14);
        assert_eq!(sce_band(10001), 0);
    }

    #[test]
    fn raise_only_and_gsf_530() {
        let mut w = World::default();
        assert!(w.set_sce_no(1200, true));
        assert!(!w.set_sce_no(1100, true));
        assert_eq!(w.sce_no, 1200);
        assert!(w.set_sce_no(1000, false));
        assert!(!w.gsf.get(530));
        w.set_sce_no(6520, true);
        assert!(w.gsf.get(530));
    }

    #[test]
    fn save_round_trip() {
        let mut w = World::default();
        w.set_sce_no(1200, false);
        w.gsf.set(372, true);
        w.set_quest_bit(1, 94, true);
        w.set_lot(200, 30, true);
        w.set_lot(100, 200, true);
        w.npc_friend[304] = 600;
        let b = w.save();
        assert_eq!(World::load(&b).unwrap(), w);
        assert!(World::load(&b[..b.len() - 1]).is_err());
    }
}
