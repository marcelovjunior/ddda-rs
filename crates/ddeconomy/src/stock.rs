//! Saved shop stock (`sNpcManager.mSaveShop`: 17 `cSAVE_SHOP` records of
//! 250 slots) and the restock tick. World state, host-owned.

use dditem::rules::{adjust_stock, restock_tick, StockSlot};
use dditem::shop::ShopList;

pub const RECORDS: usize = 17;
pub const SLOTS: usize = 250;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockRecord {
    pub npc: u32,
    pub slots: Vec<StockSlot>,
    /// Seen-bits by lineup index (`+0x7d8`, set by `0x00cdcac0`).
    pub seen: [u32; 8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockBook {
    pub records: Vec<Option<StockRecord>>,
}

impl Default for StockBook {
    fn default() -> Self {
        StockBook {
            records: vec![None; RECORDS],
        }
    }
}

/// Iola (NPC 11) sells Berry, Twigbean and Carrot (19, 28, 29) as 99 with a
/// one-day rearrival whatever the list says (`0x004752d0`, `0x00477ad0`).
fn forced(npc: u32, item: u32) -> bool {
    npc == 11 && matches!(item, 0x13 | 0x1c | 0x1d)
}

fn fresh(npc: u32, list: &ShopList) -> StockRecord {
    // Translated from 0x004752d0 (DDDA.exe 2017-12-13 Steam).
    let slots = list
        .lineup
        .iter()
        .take(SLOTS)
        .map(|l| {
            let (now, init, rearrival) = if forced(npc, l.item) {
                // Stock 99 only when the list's rearrival is 0 (it is, for
                // all three, in the shipped n011 list).
                let now = if l.rearrival == 0 { 99 } else { l.num as i16 };
                (now, 99, 1)
            } else {
                (l.num as i16, l.num as i16, l.rearrival as i8)
            };
            StockSlot {
                item: l.item as i16,
                day: 0,
                rearrival,
                now,
                init,
            }
        })
        .collect();
    StockRecord {
        npc,
        slots,
        seen: [0; 8],
    }
}

impl StockBook {
    pub fn record(&self, npc: u32) -> Option<&StockRecord> {
        self.records.iter().flatten().find(|r| r.npc == npc)
    }

    fn record_mut(&mut self, npc: u32) -> Option<&mut StockRecord> {
        self.records.iter_mut().flatten().find(|r| r.npc == npc)
    }

    /// Make sure `npc` has a record matching its list. Translated from
    /// `0x00477ad0`: an existing record is kept while every lineup entry's
    /// item, rearrival and count match the saved ones (Iola's forced
    /// entries always match), else it is rebuilt full; a new shop takes
    /// the first free record; with all 17 taken nothing is saved.
    pub fn ensure(&mut self, npc: u32, list: &ShopList) {
        if let Some(r) = self.record_mut(npc) {
            let matches = list.lineup.iter().take(SLOTS).enumerate().all(|(i, l)| {
                r.slots.get(i).is_some_and(|s| {
                    s.item == l.item as i16
                        && (forced(npc, l.item)
                            || (s.rearrival == l.rearrival as i8 && s.init == l.num as i16))
                })
            });
            if !matches {
                *r = fresh(npc, list);
            }
            return;
        }
        if let Some(free) = self.records.iter_mut().find(|r| r.is_none()) {
            *free = Some(fresh(npc, list));
        }
    }

    /// Stock of `item` at `npc` (`0x00477930`): the first slot holding it;
    /// -1 (unlimited) when the shop or item has no slot. `has_makers_finger`
    /// covers the special case NPC 133 / item 140: 1 if the party has none,
    /// else 0.
    pub fn left(&self, npc: u32, item: u32, has_makers_finger: bool) -> i32 {
        let Some(r) = self.record(npc) else { return -1 };
        let Some(s) = r.slots.iter().find(|s| s.item as u32 == item) else {
            return -1;
        };
        if npc == 0x85 && item == 0x8c {
            return if has_makers_finger { 0 } else { 1 };
        }
        s.now as i32
    }

    /// `0x00477a40`: change the first slot of `item` by `delta`, clamped to
    /// 0..=999; unlimited stock is untouched.
    pub fn adjust(&mut self, npc: u32, item: u32, delta: i32) {
        if let Some(r) = self.record_mut(npc) {
            if let Some(s) = r.slots.iter_mut().find(|s| s.item as u32 == item) {
                adjust_stock(s, delta);
            }
        }
    }

    /// One restock tick over every record (`0x00477cb0`).
    pub fn tick(&mut self) {
        for r in self.records.iter_mut().flatten() {
            for s in &mut r.slots {
                restock_tick(s);
            }
        }
    }

    pub fn mark_seen(&mut self, npc: u32, index: usize) {
        if let Some(r) = self.record_mut(npc) {
            if index < 256 {
                r.seen[index >> 5] |= 0x8000_0000 >> (index & 31);
            }
        }
    }
}
