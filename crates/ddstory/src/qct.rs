//! Quest control table evaluation (cQuestCtrl).
//!
//! Translated from 0x00792360 (row pass), 0x00793520 / 0x0078ece0
//! (judgments) and 0x0078fbb0 (results), DDDA.exe 2017-12-13 Steam, for
//! the judgment and result types the opening uses; any other type is an
//! [`Error::Unimplemented`].

use crate::{Env, Error, Request};
use ddquest::qct::{Outcome, QuestCtrl, Row};
use std::sync::Arc;

/// Runtime of one quest's table (cQuestCtrl, 0x18 bytes).
#[derive(Debug, Clone)]
pub struct Ctrl {
    pub table: Arc<QuestCtrl>,
    /// Disabled-row bitmask (MSB-first by row id; cQuest +8, 0x20 bytes).
    pub disabled: [u32; 8],
    /// Rows that fired this frame (row +0x15, reset by 0x00cd7990).
    fired: Vec<bool>,
    /// Row ids: sequential over all tables (row +0x18, loader 0x00cd7200).
    first_id: Vec<usize>,
}

impl Ctrl {
    pub fn new(table: Arc<QuestCtrl>) -> Ctrl {
        let mut first_id = vec![];
        let mut n = 0;
        for t in &table.tables {
            first_id.push(n);
            n += t.rows.len();
        }
        Ctrl { table, disabled: [0; 8], fired: vec![false; n], first_id }
    }

    fn row_disabled(&self, id: usize) -> bool {
        id >= 256 || self.disabled[id >> 5] & (0x8000_0000 >> (id & 31)) != 0
    }
    fn set_disabled(&mut self, id: usize, v: bool) {
        if id < 256 {
            let m = 0x8000_0000 >> (id & 31);
            if v {
                self.disabled[id >> 5] |= m;
            } else {
                self.disabled[id >> 5] &= !m;
            }
        }
    }

    /// One quest's evaluation for this frame (0x007922f0): with an active
    /// quest, first a pass that favours it, then a normal pass if nothing
    /// fired; then the per-frame fired marks are cleared (0x00cd7990).
    // Translated from 0x007922f0 (DDDA.exe 2017-12-13 Steam).
    pub fn frame(&mut self, env: &mut Env, quest: u32) -> Result<(), Error> {
        if env.world.active_quest == -1 || !self.pass(env, quest, 0, true)? {
            self.pass(env, quest, 0, false)?;
        }
        self.fired.iter_mut().for_each(|f| *f = false);
        Ok(())
    }

    /// 0x00792360 for quest tables (no talking NPC). Returns whether a row
    /// fired.
    // Translated from 0x00792360 (DDDA.exe 2017-12-13 Steam).
    fn pass(&mut self, env: &mut Env, quest: u32, table: usize, active_pass: bool) -> Result<bool, Error> {
        let t = self.table.clone();
        let Some(tab) = t.tables.get(table) else { return Ok(false) };
        for (i, row) in tab.rows.iter().enumerate() {
            let id = self.first_id[table] + i;
            if self.row_disabled(id) || self.fired[id] || !judge(env, row)? {
                continue;
            }
            let Some(first) = row.results.first() else { return Ok(false) };
            if first.0[0] == 1 {
                // Go to table a: only in the active pass when the active
                // quest is the result's c.
                let aq = env.world.active_quest;
                if !active_pass || aq == -1 || aq == first.0[3] as i32 {
                    self.results(env, quest, id, row)?;
                    self.set_disabled(id, false);
                    if self.pass(env, quest, first.0[1] as usize, active_pass)? {
                        return Ok(true);
                    }
                }
                continue;
            }
            self.fired[id] = true;
            self.results(env, quest, id, row)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// 0x0078fbb0: run every result; then disable the row unless one of
    /// its results is 22 ("repeatable").
    // Translated from 0x0078fbb0 (DDDA.exe 2017-12-13 Steam), types below.
    fn results(&mut self, env: &mut Env, quest: u32, id: usize, row: &Row) -> Result<(), Error> {
        let mut keep = false;
        for r in &row.results {
            if outcome(env, quest, r)? {
                keep = true;
            }
        }
        if !keep {
            self.set_disabled(id, true);
        }
        Ok(())
    }
}

/// All judgments must hold (0x00793520 over the mask from 0x0078ece0).
fn judge(env: &Env, row: &Row) -> Result<bool, Error> {
    for j in &row.judgments {
        let [t, a, b, ..] = j.0;
        let w = &*env.world;
        let ok = match t {
            0 => true,
            1 => w.quest_bit(a as usize, b as usize),
            2 => !w.quest_bit(a as usize, b as usize),
            // GSF test 0x00435690 (false past 0x600).
            3 => w.gsf.get(a as usize),
            4 => !w.gsf.get(a as usize),
            10 => w.sce_no as i32 == a as i32,
            11 => w.sce_no as i32 >= a as i32,
            12 => (w.sce_no as i32) < a as i32,
            // 0x00460350: at least max(b, 1) of item a.
            16 => env.host.has_item(a, b.max(1)),
            // Translated from 0x0078ece0 cases 18/19 (DDDA.exe
            // 2017-12-13 Steam): b selects the apparent-death bank
            // (0x00435710) or permanent-death bank (0x00435740).
            18 | 19 => {
                if a >= 900 {
                    return Err(Error::Data(format!("QCT NPC death index {a} outside 0..900")));
                }
                let dead = if b == 0 { w.npc_app_die.get(a as usize) } else { w.npc_die.get(a as usize) };
                dead == (t == 18)
            }
            // Player record +0xc (0x00442ac0 n = 0).
            23 if a == 0 => w.player_sex == 0,
            24 if a == 0 => w.player_sex == 1,
            32 => w.stage_no == a,
            33 => w.stage_no != a,
            other => {
                return Err(Error::Unimplemented(format!("QCT judgment type {other} {:?}", j.0)));
            }
        };
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

/// One result; returns true for type 22 (keep the row enabled).
fn outcome(env: &mut Env, quest: u32, r: &Outcome) -> Result<bool, Error> {
    let [t, a, b, _c, d, ..] = r.0;
    let w = &mut *env.world;
    match t {
        2 => w.set_quest_bit(a as usize, b as usize, true),
        3 => w.set_quest_bit(a as usize, b as usize, false),
        4 => w.gsf.set(a as usize, true),
        5 => w.gsf.set(a as usize, false),
        6 => w.set_lot(a, b as usize, true),
        7 => w.set_lot(a, b as usize, false),
        // 0x00476a50: friendship +/- b for NPC a.
        8 | 9 => {
            if let Some(f) = w.npc_friend.get_mut(a as usize) {
                let delta = if t == 8 { b as i32 } else { -(b as i32) };
                *f = (*f as i32 + delta) as i16;
            }
        }
        // 23 start: quest flags |= 2 (0x0078d6d0), banner 1, log entry d,
        // active quest if none (0x0044a400).
        23 => {
            env.cmds.push(crate::story::Command::QuestFlags { quest: a, set: 2, clear: 0 });
            env.out.push(Request::QuestBanner { kind: 1, quest: a, log: d });
            env.cmds.push(crate::story::Command::Log { quest: a, entry: d });
            if env.world.active_quest == -1 {
                env.cmds.push(crate::story::Command::Activate { quest: a });
            }
        }
        // 17 objective: banner 2, log entry d.
        17 => {
            env.out.push(Request::QuestBanner { kind: 2, quest: a, log: d });
            env.cmds.push(crate::story::Command::Log { quest: a, entry: d });
        }
        // 16 clear: flags & ~0x18 | 0x24 (0x0078d730), banner 6, log entry
        // d, rewards (0x00790539).
        16 => {
            env.cmds.push(crate::story::Command::QuestFlags { quest: a, set: 0x24, clear: 0x18 });
            env.out.push(Request::QuestBanner { kind: 6, quest: a, log: d });
            env.cmds.push(crate::story::Command::Log { quest: a, entry: d });
            env.cmds.push(crate::story::Command::Reward { quest: a });
        }
        // 30 / 19 / 60 set a marker, 20 / 61 clear one (0x00485f90 / 0x00486050).
        19 | 30 | 60 => env.out.push(Request::Marker { quest: a, set: true, raw: r.0 }),
        20 | 61 => env.out.push(Request::Marker { quest: a, set: false, raw: r.0 }),
        // Handled by the pass (go to table).
        1 => {}
        // 31 boot quest a branch b (0x00484000) and mark it started
        // (0x0078d6d0).
        31 => env.cmds.push(crate::story::Command::BootQuest { quest: a, branch: b, start: true }),
        22 => return Ok(true),
        other => {
            let _ = quest;
            return Err(Error::Unimplemented(format!("QCT result type {other} {:?}", r.0)));
        }
    }
    Ok(false)
}
