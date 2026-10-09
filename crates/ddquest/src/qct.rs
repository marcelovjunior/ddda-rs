//! rQuestCtrlTbl ("QCT"): a quest's control table of judgment/result rows.
//!
//! Layout from the loader `rQuestCtrlTbl::vf10` 0x00cd7200 (DDDA.exe
//! 2017-12-13 Steam); every field is a u32 (reader 0x00cfeb40):
//!
//! ```text
//! u32 magic 0x00746371 ("qct\0"), u32 version 0x10
//! u32 allocation size (runtime bytes, not the file length)
//! u32 table count
//! table[]: u32 row count
//!          row[]: u32 judgment count, judgment[]: 5 x u32
//!                 u32 result count,   result[]:  11 x u32
//!          NUL-terminated table name (the loader keeps at most 0x1e chars)
//! u32 trailer (rQuestCtrlTbl +0x60; copied into cQuest +4 when that is 0,
//!     0x0078d0d0; meaning unknown)
//! ```
//!
//! Evaluation (`0x00792360`, per quest per frame from sQuestManager vf06
//! 0x00482dd0 -> cQuest 0x0078c830): rows in list order; a row runs when
//! all its judgments hold (0x0078ece0) and it is not disabled; if its first
//! result has type 1 evaluation continues in table `a` instead; otherwise
//! its results run (0x0078fbb0) and the pass ends (one row per pass). A row
//! that fired is disabled for good unless one of its results has type 22.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub const MAGIC: u32 = 0x0074_6371;
pub const VERSION: u32 = 0x10;

/// One judgment: `[type, a, b, c, d]` (QuestTblJudgmentBinary +4..+0x14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Judgment(pub [u32; 5]);

/// One result: `[type, a, b, c, d, ...]` (QuestTblResultBinary +4..+0x2c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome(pub [u32; 11]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub judgments: Vec<Judgment>,
    pub results: Vec<Outcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestCtrl {
    pub alloc_size: u32,
    pub tables: Vec<Table>,
    pub trailer: u32,
}

struct Cursor<'a> {
    b: &'a [u8],
    o: usize,
}

impl Cursor<'_> {
    fn u32(&mut self) -> Result<u32, Error> {
        let s = self
            .b
            .get(self.o..self.o + 4)
            .ok_or_else(|| Error(format!("truncated at {:#x}", self.o)))?;
        self.o += 4;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn count(&mut self, item: usize) -> Result<usize, Error> {
        let n = self.u32()? as usize;
        // Refuse counts the remaining bytes cannot hold.
        if n.saturating_mul(item) > self.b.len() - self.o {
            return Err(Error(format!("count {n} too large at {:#x}", self.o - 4)));
        }
        Ok(n)
    }
}

impl QuestCtrl {
    pub fn parse(b: &[u8]) -> Result<QuestCtrl, Error> {
        let mut c = Cursor { b, o: 0 };
        let (magic, version) = (c.u32()?, c.u32()?);
        if magic != MAGIC || version != VERSION {
            return Err(Error(format!("magic/version {magic:#x}/{version:#x}")));
        }
        let alloc_size = c.u32()?;
        let ntab = c.count(4)?;
        let mut tables = Vec::with_capacity(ntab);
        for _ in 0..ntab {
            let nrow = c.count(8)?;
            let mut rows = Vec::with_capacity(nrow);
            for _ in 0..nrow {
                let nj = c.count(20)?;
                let mut judgments = Vec::with_capacity(nj);
                for _ in 0..nj {
                    let mut j = [0; 5];
                    for x in &mut j {
                        *x = c.u32()?;
                    }
                    judgments.push(Judgment(j));
                }
                let nr = c.count(44)?;
                let mut results = Vec::with_capacity(nr);
                for _ in 0..nr {
                    let mut r = [0; 11];
                    for x in &mut r {
                        *x = c.u32()?;
                    }
                    results.push(Outcome(r));
                }
                rows.push(Row { judgments, results });
            }
            let rest = &b[c.o..];
            let n = rest
                .iter()
                .position(|&x| x == 0)
                .ok_or_else(|| Error("unterminated table name".into()))?;
            let name = rest[..n].iter().map(|&x| x as char).collect();
            c.o += n + 1;
            tables.push(Table { name, rows });
        }
        let trailer = c.u32()?;
        if c.o != b.len() {
            return Err(Error(format!("{} trailing bytes", b.len() - c.o)));
        }
        Ok(QuestCtrl { alloc_size, tables, trailer })
    }
}

/// Judgment types traced in 0x0078ece0 (jump table 0x0078faac). Others
/// are kept raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgmentKind {
    Always,
    QuestBitSet { quest: u32, bit: u32 },
    QuestBitClear { quest: u32, bit: u32 },
    GsfSet(u32),
    GsfClear(u32),
    /// Quest bit clear; once it is set the row is disabled for good.
    QuestBitClearElseDisable { quest: u32, bit: u32 },
    SceNoEq(u32),
    SceNoAtLeast(u32),
    SceNoBelow(u32),
    StageIs(u32),
    StageIsNot(u32),
    Other(Judgment),
}

impl Judgment {
    pub fn kind(&self) -> JudgmentKind {
        let [t, a, b, ..] = self.0;
        match t {
            0 => JudgmentKind::Always,
            1 => JudgmentKind::QuestBitSet { quest: a, bit: b },
            2 => JudgmentKind::QuestBitClear { quest: a, bit: b },
            3 => JudgmentKind::GsfSet(a),
            4 => JudgmentKind::GsfClear(a),
            7 => JudgmentKind::QuestBitClearElseDisable { quest: a, bit: b },
            10 => JudgmentKind::SceNoEq(a),
            11 => JudgmentKind::SceNoAtLeast(a),
            12 => JudgmentKind::SceNoBelow(a),
            32 => JudgmentKind::StageIs(a),
            33 => JudgmentKind::StageIsNot(a),
            _ => JudgmentKind::Other(*self),
        }
    }
}

/// Result types traced in 0x0078fbb0 (jump table 0x00791f28, index
/// type-2). Others are kept raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    /// 1: continue evaluation in table `a` (only as a row's first result).
    GotoTable(u32),
    SetQuestBit { quest: u32, bit: u32 },
    ClearQuestBit { quest: u32, bit: u32 },
    SetGsf(u32),
    ClearGsf(u32),
    /// 6/7: stage layout ("lot") flag on/off (0x00443ca0).
    LayoutOn { stage: u32, flag: u32 },
    LayoutOff { stage: u32, flag: u32 },
    /// 8/9: NPC friendship +/- (0x00476a50).
    FriendUp { npc: u32, amount: u32 },
    FriendDown { npc: u32, amount: u32 },
    Gold(u32),
    /// 13: show quest `quest`'s message `message` (sIdMessage).
    ShowMessage { quest: u32, message: u32 },
    /// 14/31: boot quest `quest` branch `branch` (0x00484000).
    BootQuest { quest: u32, branch: u32 },
    /// 23: the quest starts (banner kind 1); log entry `log`.
    Start { log: u32 },
    /// 17: objective update (banner 2); log entry `log`.
    Objective { log: u32 },
    /// 15: report-back objective (banner 3); log entry `log`.
    Report { log: u32 },
    /// 16: cleared (banner 6), rewards from rQuestReward; log entry `log`.
    Clear { log: u32 },
    /// 18: failed (banner 4); log entry `log`.
    Fail { log: u32 },
    SetMarker(Outcome),
    ClearMarker(Outcome),
    /// 22: this row stays enabled after firing.
    Repeatable,
    Other(Outcome),
}

impl Outcome {
    pub fn kind(&self) -> OutcomeKind {
        let [t, a, b, _, d, ..] = self.0;
        match t {
            1 => OutcomeKind::GotoTable(a),
            2 => OutcomeKind::SetQuestBit { quest: a, bit: b },
            3 => OutcomeKind::ClearQuestBit { quest: a, bit: b },
            4 => OutcomeKind::SetGsf(a),
            5 => OutcomeKind::ClearGsf(a),
            6 => OutcomeKind::LayoutOn { stage: a, flag: b },
            7 => OutcomeKind::LayoutOff { stage: a, flag: b },
            8 => OutcomeKind::FriendUp { npc: a, amount: b },
            9 => OutcomeKind::FriendDown { npc: a, amount: b },
            10 => OutcomeKind::Gold(a),
            13 => OutcomeKind::ShowMessage { quest: a, message: b },
            14 | 31 => OutcomeKind::BootQuest { quest: a, branch: b },
            23 => OutcomeKind::Start { log: d },
            17 => OutcomeKind::Objective { log: d },
            15 => OutcomeKind::Report { log: d },
            16 => OutcomeKind::Clear { log: d },
            18 => OutcomeKind::Fail { log: d },
            19 | 30 | 60 => OutcomeKind::SetMarker(*self),
            20 | 61 => OutcomeKind::ClearMarker(*self),
            22 => OutcomeKind::Repeatable,
            _ => OutcomeKind::Other(*self),
        }
    }
}

/// Message index in `id/questlog_data/qNNNN_log_<lang>` of quest-log entry
/// `entry`: the HUD shows message 0 (title) and message `5 * entry + 10`
/// (`lea edi,[eax+eax*4+0xa]` at 0x0066f8ec).
pub fn log_message_index(entry: u32) -> u32 {
    5 * entry + 10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[u32]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn sample() -> Vec<u8> {
        let mut b = words(&[MAGIC, VERSION, 0x100, 1, 1, 1, 1, 2, 0, 0, 0, 1]);
        b.extend(words(&[23, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        b.extend_from_slice(b"QuestTbl\0");
        b.extend(words(&[7]));
        b
    }

    #[test]
    fn parses_rows() {
        let q = QuestCtrl::parse(&sample()).unwrap();
        assert_eq!(q.tables.len(), 1);
        assert_eq!(q.tables[0].name, "QuestTbl");
        let row = &q.tables[0].rows[0];
        assert_eq!(row.judgments[0].kind(), JudgmentKind::QuestBitSet { quest: 2, bit: 0 });
        assert_eq!(row.results[0].kind(), OutcomeKind::Start { log: 0 });
        assert_eq!(q.trailer, 7);
    }

    #[test]
    fn rejects_truncation_and_trailing_bytes() {
        let b = sample();
        for n in 0..b.len() {
            assert!(QuestCtrl::parse(&b[..n]).is_err(), "prefix {n}");
        }
        let mut long = b.clone();
        long.push(0);
        assert!(QuestCtrl::parse(&long).is_err());
    }

    #[test]
    fn log_entries_are_five_messages_apart() {
        assert_eq!(log_message_index(0), 10);
        assert_eq!(log_message_index(3), 25);
    }
}
