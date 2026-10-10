//! NPC quest-control selection and the two real dialogue phases.
//! Translated from 0x00792130 / 0x00792360 / 0x0078FBB0
//! (DDDA.exe 2017-12-13 Steam). Tables remain installed runtime data.

use crate::Error;
use ddquest::qct::{QuestCtrl, Row};
use ddstory::World;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowKey {
    pub table: usize,
    pub row: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Item { item: i16, count: u32, level: u8 },
    QuestBit { quest: usize, bit: usize, value: bool },
    Gsf { flag: usize, value: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub row: RowKey,
    pub quest: u32,
    pub conversation: i32,
    /// Phase2 effects, not applied when phase1 opens the text.
    pub effects: Vec<Effect>,
    repeatable: bool,
}

pub struct Context<'a> {
    pub world: &'a World,
    pub npc: u32,
    /// sNpcManager+0x8E8+npc*0x110 (judgment38, 0x0078ECE0).
    /// This counter is not World::npc_friend at +0x8B8.
    pub talk_counter: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Control {
    pub table: Arc<QuestCtrl>,
    disabled: Vec<bool>,
    fired: Vec<bool>,
    first_id: Vec<usize>,
    pending: Option<Selection>,
}

impl Control {
    pub fn new(table: Arc<QuestCtrl>) -> Self {
        let mut first_id = Vec::new();
        let mut total = 0;
        for part in &table.tables {
            first_id.push(total);
            total += part.rows.len();
        }
        Self {
            table,
            disabled: vec![false; total],
            fired: vec![false; total],
            first_id,
            pending: None,
        }
    }

    pub fn disabled(&self, row: RowKey) -> bool {
        self.first_id
            .get(row.table)
            .and_then(|&first| self.disabled.get(first + row.row))
            .copied()
            .unwrap_or(true)
    }

    /// Begin phase1. Active-quest pass precedes the ordinary pass at
    /// 0x00792130. An in-flight row cannot be selected again.
    pub fn begin(&mut self, context: &Context<'_>) -> Result<Option<Selection>, Error> {
        if self.pending.is_some() {
            return Err(Error::Data("NPC quest talk already active".into()));
        }
        let mut trail = Vec::new();
        let mut result = None;
        if context.world.active_quest != -1 {
            result = self.pass(context, 0, true, &mut trail)?;
        }
        if result.is_none() {
            result = self.pass(context, 0, false, &mut trail)?;
        }
        self.pending = result.clone();
        Ok(result)
    }

    fn pass(
        &mut self,
        context: &Context<'_>,
        table: usize,
        active: bool,
        trail: &mut Vec<usize>,
    ) -> Result<Option<Selection>, Error> {
        if trail.contains(&table) {
            return Err(Error::Data("cyclic NPC QCT continuation".into()));
        }
        let data = self.table.clone();
        let Some(part) = data.tables.get(table) else {
            return Ok(None);
        };
        trail.push(table);
        let result = (|| {
            for (index, row) in part.rows.iter().enumerate() {
                let id = self.first_id[table] + index;
                if self.disabled[id] || self.fired[id] {
                    continue;
                }
                if !self.judge(context, row, id)? {
                    continue;
                }
                let Some(first) = row.results.first() else {
                    return Ok(None);
                };
                if first.0[0] == 1 {
                    // +0xC is field b, the quest number (0079243B), not c.
                    if active && context.world.active_quest != first.0[2] as i32 {
                        continue;
                    }
                    // Parent continuation phase0 only supports the authored
                    // Continue/Repeatable pair here; no effects are dropped.
                    if row.results.iter().any(|r| !matches!(r.0[0], 1 | 22)) {
                        return Err(Error::Unimplemented("NPC QCT continuation effects".into()));
                    }
                    self.disabled[id] = false; // 0079246D clears parent mask.
                    if let Some(selected) = self.pass(context, first.0[1] as usize, active, trail)? {
                        return Ok(Some(selected));
                    }
                    continue;
                }
                // Special active-quest suppression for NPC105/141 at the
                // root table (007924DD..0079252A), not all NPCs.
                if active
                    && table == 0
                    && matches!(context.npc, 105 | 141)
                    && row
                        .results
                        .iter()
                        .any(|r| matches!(r.0[0], 13 | 26) && r.0[1] as i32 != context.world.active_quest)
                {
                    continue;
                }
                let selected = selection(RowKey { table, row: index }, row)?;
                self.fired[id] = true; // row+0x15; phase1 does not disable.
                return Ok(Some(selected));
            }
            Ok(None)
        })();
        trail.pop();
        result
    }

    // 00793520 requires every MSB-first judgment bit. 0078ECE0 evaluates
    // all judgments, including type7's permanent disable side effect.
    fn judge(&mut self, context: &Context<'_>, row: &Row, id: usize) -> Result<bool, Error> {
        let w = context.world;
        let mut all = true;
        let mut unresolved_counter = false;
        for judgment in &row.judgments {
            let [kind, a, b, ..] = judgment.0;
            let value = match kind {
                0 => true,
                1 => w.quest_bit(a as usize, b as usize),
                2 => !w.quest_bit(a as usize, b as usize),
                3 => w.gsf.get(a as usize),
                4 => !w.gsf.get(a as usize),
                7 => {
                    let clear = !w.quest_bit(a as usize, b as usize);
                    if !clear {
                        self.disabled[id] = true;
                    }
                    clear
                }
                10 => w.sce_no as i32 == a as i32,
                11 => w.sce_no as i32 >= a as i32,
                12 => (w.sce_no as i32) < a as i32,
                32 => w.stage_no == a,
                33 => w.stage_no != a,
                38 => match context.talk_counter {
                    Some(count) => a <= count,
                    None => {
                        unresolved_counter = true;
                        true
                    }
                },
                _ => return Err(Error::Unimplemented(format!("NPC QCT judgment {kind}"))),
            };
            all &= value;
        }
        // A known false judgment already proves the conjunction false.
        // Judgment38 has no side effects; unknown cannot make a row true.
        if all && unresolved_counter {
            return Err(Error::Unimplemented("NPC QCT talk-counter observation".into()));
        }
        Ok(all)
    }

    /// Only the current selected row can reach phase2. Stock uNpc talk
    /// vf06 state3 calls 0x00BA02E0 after real message-completion gates,
    /// which invokes 0x0078FBB0(row,2). No elapsed-time acknowledgement.
    pub fn complete(&mut self, row: RowKey) -> Option<Vec<Effect>> {
        if self.pending.as_ref().is_none_or(|pending| pending.row != row) {
            return None;
        }
        let selected = self.pending.take()?;
        if !selected.repeatable {
            self.disabled[self.first_id[row.table] + row.row] = true;
        }
        self.clear_transient();
        Some(selected.effects)
    }

    /// Actor/text cancellation is not phase2: no reward or quest flag.
    pub fn abort(&mut self) {
        self.pending = None;
        self.clear_transient();
    }

    /// 0x007925C0 -> 0x00CD7990 clears transient row+0x15 marks when talk
    /// exits. Permanent row masks survive that cleanup.
    fn clear_transient(&mut self) {
        self.fired.fill(false);
    }
}

fn selection(key: RowKey, row: &Row) -> Result<Selection, Error> {
    let mut message = None;
    let mut effects = Vec::new();
    let mut repeatable = false;
    for result in &row.results {
        let [kind, a, b, c, ..] = result.0;
        match kind {
            13 => {
                if message.replace((a, b as i32)).is_some() {
                    return Err(Error::Unimplemented("NPC QCT multiple message results".into()));
                }
            }
            11 => {
                let item = i16::try_from(a)
                    .map_err(|_| Error::Data("NPC QCT item outside inventory range".into()))?;
                let level =
                    u8::try_from(c).map_err(|_| Error::Data("NPC QCT item level outside range".into()))?;
                if (b as i32) < 0 {
                    return Err(Error::Data("negative NPC QCT item count".into()));
                }
                effects.push(Effect::Item {
                    item,
                    count: if b == 0 { 1 } else { b },
                    level,
                });
            }
            2 | 3 => effects.push(Effect::QuestBit {
                quest: a as usize,
                bit: b as usize,
                value: kind == 2,
            }),
            4 | 5 => effects.push(Effect::Gsf {
                flag: a as usize,
                value: kind == 4,
            }),
            22 => repeatable = true,
            _ => return Err(Error::Unimplemented(format!("NPC QCT result {kind}"))),
        }
    }
    let (quest, conversation) =
        message.ok_or_else(|| Error::Unimplemented("NPC QCT row without supported dialogue".into()))?;
    Ok(Selection {
        row: key,
        quest,
        conversation,
        effects,
        repeatable,
    })
}
