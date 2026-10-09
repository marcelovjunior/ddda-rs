//! The economy's save parts (the host's `ECON` chunks, ADR 0002 section 6).
//!
//! - **Shared:** restock day, shop stock, buy-back list, lineup gates,
//!   the main-pawn flag and the pawns' bags.
//! - **Per seat:** that Arisen's wallet, bag and storage.
//!
//! Our own format, not the original's `cSAVE_*` records. Open sessions
//! (NPC locks) are not saved: the host saves only while no interaction is
//! open, as the original cannot save inside a shop.

use crate::inventory::{Bag, BuyBack, Owner, Slot};
use crate::stock::{StockBook, StockRecord, RECORDS};
use crate::wallet::Wallet;
use crate::{Economy, Gates};
use ddid::{Parts, PlayerId, Reader, Result, Writer};
use dditem::rules::StockSlot;
use std::collections::BTreeSet;

const VERSION: u8 = 1;

fn bag(w: &mut Writer, b: &Bag) {
    w.len(b.slots.len());
    for s in &b.slots {
        w.i16(s.item).i16(s.num).u32(s.flags);
    }
}

fn read_bag(r: &mut Reader) -> Result<Bag> {
    let n = r.len(8)?;
    let mut slots = Vec::with_capacity(n);
    for _ in 0..n {
        slots.push(Slot { item: r.i16()?, num: r.i16()?, flags: r.u32()? });
    }
    Ok(Bag { slots })
}

fn opt_bag(w: &mut Writer, b: Option<&Bag>) {
    w.bool(b.is_some());
    if let Some(b) = b {
        bag(w, b);
    }
}

fn read_opt_bag(r: &mut Reader) -> Result<Option<Bag>> {
    Ok(if r.bool()? { Some(read_bag(r)?) } else { None })
}

fn set(w: &mut Writer, s: &BTreeSet<u32>) {
    w.len(s.len());
    for &v in s {
        w.u32(v);
    }
}

fn read_set(r: &mut Reader) -> Result<BTreeSet<u32>> {
    let n = r.len(4)?;
    (0..n).map(|_| r.u32()).collect()
}

impl Economy {
    /// The economy's parts: shared state, and per seat the wallet, bag and
    /// storage of every seat that has any of them.
    pub fn save(&self) -> Parts {
        let mut w = Writer::new();
        w.u8(VERSION).u64(self.last_day);
        w.len(self.stock.records.len());
        for rec in &self.stock.records {
            w.bool(rec.is_some());
            if let Some(rec) = rec {
                w.u32(rec.npc).len(rec.slots.len());
                for s in &rec.slots {
                    w.i16(s.item).i8(s.day).i8(s.rearrival).i16(s.now).i16(s.init);
                }
                for &x in &rec.seen {
                    w.u32(x);
                }
            }
        }
        w.len(self.buyback.entries.len());
        for &(item, n) in &self.buyback.entries {
            w.i16(item).u16(n);
        }
        set(&mut w, &self.gates.platform_hidden);
        set(&mut w, &self.gates.cosmetics_used);
        w.i32(self.gates.metamorphosis);
        w.bool(self.inventory.main_pawn_present);
        let pawns: Vec<(u8, &Bag)> = self
            .inventory
            .bags
            .iter()
            .filter_map(|(o, b)| match o {
                Owner::Pawn(n) => Some((*n, b)),
                _ => None,
            })
            .collect();
        w.len(pawns.len());
        for (n, b) in pawns {
            w.u8(n);
            bag(&mut w, b);
        }
        let mut seats: BTreeSet<PlayerId> = self.wallets.keys().copied().collect();
        for o in self.inventory.bags.keys() {
            if let Owner::Arisen(p) | Owner::Storage(p) = o {
                seats.insert(*p);
            }
        }
        let players = seats
            .into_iter()
            .map(|p| {
                let mut w = Writer::new();
                w.u8(VERSION);
                match self.wallets.get(&p) {
                    Some(x) => w.bool(true).i32(x.gold).i32(x.rift_crystals),
                    None => w.bool(false),
                };
                opt_bag(&mut w, self.inventory.bags.get(&Owner::Arisen(p)));
                opt_bag(&mut w, self.inventory.bags.get(&Owner::Storage(p)));
                (p, w.finish())
            })
            .collect();
        Parts { shared: w.finish(), players }
    }

    /// Rebuilds an economy from [`Economy::save`]'s parts. `parts.players`
    /// is keyed by the seat each Arisen has now (the host remaps).
    pub fn load(parts: &Parts) -> Result<Economy> {
        let mut e = Economy::default();
        let mut r = Reader::new(&parts.shared);
        r.version("ECON", VERSION)?;
        e.last_day = r.u64()?;
        let n = r.len(1)?;
        if n != RECORDS {
            return Err(format!("ECON: {n} stock records, expected {RECORDS}"));
        }
        let mut records = Vec::with_capacity(n);
        for _ in 0..n {
            records.push(if r.bool()? {
                let npc = r.u32()?;
                let k = r.len(8)?;
                let mut slots = Vec::with_capacity(k);
                for _ in 0..k {
                    slots.push(StockSlot { item: r.i16()?, day: r.i8()?, rearrival: r.i8()?, now: r.i16()?, init: r.i16()? });
                }
                let mut seen = [0u32; 8];
                for x in &mut seen {
                    *x = r.u32()?;
                }
                Some(StockRecord { npc, slots, seen })
            } else {
                None
            });
        }
        e.stock = StockBook { records };
        let n = r.len(4)?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            entries.push((r.i16()?, r.u16()?));
        }
        e.buyback = BuyBack { entries };
        e.gates = Gates { platform_hidden: read_set(&mut r)?, cosmetics_used: read_set(&mut r)?, metamorphosis: r.i32()? };
        e.inventory.main_pawn_present = r.bool()?;
        for _ in 0..r.len(5)? {
            let n = r.u8()?;
            e.inventory.bags.insert(Owner::Pawn(n), read_bag(&mut r)?);
        }
        r.end()?;
        for (&p, b) in &parts.players {
            let mut r = Reader::new(b);
            r.version("ECON player", VERSION)?;
            if r.bool()? {
                e.wallets.insert(p, Wallet { gold: r.i32()?, rift_crystals: r.i32()? });
            }
            if let Some(b) = read_opt_bag(&mut r)? {
                e.inventory.bags.insert(Owner::Arisen(p), b);
            }
            if let Some(b) = read_opt_bag(&mut r)? {
                e.inventory.bags.insert(Owner::Storage(p), b);
            }
            r.end()?;
        }
        Ok(e)
    }

    /// Whether any shop or upgrade session holds an NPC.
    pub fn sessions_open(&self) -> bool {
        !self.locks.is_empty()
    }
}
