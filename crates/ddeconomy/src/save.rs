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

const VERSION: u8 = 2;

impl crate::PlayerState {
    /// Reuse the validated ECON player-part codec, without copying shared state.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(VERSION).bool(self.wallet.is_some());
        if let Some(wallet) = &self.wallet { w.i32(wallet.gold).i32(wallet.rift_crystals); }
        opt_bag(&mut w, self.bag.as_ref());
        opt_bag(&mut w, self.storage.as_ref());
        w.bool(self.first_choice).len(self.rewards.len());
        for &quest in &self.rewards { w.u32(quest); }
        w.finish()
    }

    pub fn load(shared: &[u8], bytes: &[u8]) -> Result<Self> {
        let mut economy = Economy::load(&ddid::Parts {
            shared: shared.to_vec(), players: [(0, bytes.to_vec())].into(),
        })?;
        Ok(economy.detach_player(0))
    }
}

fn bag(w: &mut Writer, b: &Bag) {
    w.len(b.slots.len());
    for s in &b.slots {
        w.i16(s.item).i16(s.num).u32(s.flags);
    }
}

#[cfg(test)]
mod opening_tests {
    use super::*;
    #[test]
    fn opening_delivery_ids_follow_saved_player_parts_when_seats_swap() {
        let mut e=Economy::default();
        e.opening_phase=Some(crate::new_game::Phase::NewlyArisen);
        e.inventory.bag_mut(Owner::Arisen(0)).slots.push(Slot{item:43,num:5,flags:1});
        e.inventory.bag_mut(Owner::Arisen(1)).slots.push(Slot{item:1058,num:1,flags:1});
        e.opening_grants.first_choices.insert(0);
        e.opening_grants.rewards.insert((0,1));
        let mut parts=e.save();
        let first=parts.players.remove(&0).unwrap();
        let second=parts.players.remove(&1).unwrap();
        parts.players.insert(0,second);parts.players.insert(1,first);
        let restored=Economy::load(&parts).unwrap();
        assert_eq!(restored.inventory.bag(Owner::Arisen(1)).unwrap().count(43),5);
        assert!(restored.opening_grants.first_choices.contains(&1));
        assert!(restored.opening_grants.rewards.contains(&(1,1)));
        assert!(!restored.opening_grants.rewards.contains(&(0,1)));
        assert_eq!(restored.opening_phase,e.opening_phase);
        let mut mixed=parts.clone();
        let old_player=mixed.players.get_mut(&0).unwrap();
        old_player[0]=1;
        old_player.truncate(old_player.len()-5);
        assert!(Economy::load(&mixed).is_err(), "mixed versions must not lose delivery protection");
        let mut invalid=parts;*invalid.shared.last_mut().unwrap()=99;
        assert!(Economy::load(&invalid).is_err());
    }

    #[test]
    fn legacy_econ_v1_loads_bags_without_invented_opening_grants() {
        let mut shared=Writer::new();
        shared.u8(1).u64(0).len(RECORDS);
        for _ in 0..RECORDS {shared.bool(false);}
        shared.len(0).len(0).len(0).i32(0).bool(false).len(0);
        let mut player=Writer::new();
        player.u8(1).bool(false).bool(true).len(1).i16(43).i16(3).u32(1).bool(false);
        let parts=Parts{shared:shared.finish(),players:[(7,player.finish())].into()};
        let restored=Economy::load(&parts).unwrap();
        assert_eq!(restored.inventory.bag(Owner::Arisen(7)).unwrap().count(43),3);
        assert!(restored.opening_phase.is_none());
        assert!(restored.opening_grants.rewards.is_empty());
        assert!(restored.opening_grants.first_choices.is_empty());
    }
}

fn read_bag(r: &mut Reader, maximum: usize) -> Result<Bag> {
    let n = r.len(8)?;
    if n > maximum { return Err("ECON bag slot limit".into()); }
    let mut slots = Vec::with_capacity(n);
    for _ in 0..n {
        let slot = Slot { item: r.i16()?, num: r.i16()?, flags: r.u32()? };
        if slot.item < 0 || slot.num <= 0 { return Err("ECON occupied slot item/count".into()); }
        slots.push(slot);
    }
    Ok(Bag { slots })
}

fn opt_bag(w: &mut Writer, b: Option<&Bag>) {
    w.bool(b.is_some());
    if let Some(b) = b {
        bag(w, b);
    }
}

fn read_opt_bag(r: &mut Reader, maximum: usize) -> Result<Option<Bag>> {
    Ok(if r.bool()? { Some(read_bag(r, maximum)?) } else { None })
}

fn version(r: &mut Reader) -> Result<u8> {
    let v=r.u8()?;
    if !matches!(v,1|VERSION) { return Err(format!("ECON version {v}")); }
    Ok(v)
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
        // Project format v2 persists delivery state beside the native-derived
        // bags. Per-profile delivery sets remap with the corresponding bag.
        w.u8(match self.opening_phase {
            None=>0,Some(crate::new_game::Phase::BeforeCreation)=>1,
            Some(crate::new_game::Phase::DragonChallenge)=>2,
            Some(crate::new_game::Phase::NewlyArisen)=>3,
        });
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
                w.bool(self.opening_grants.first_choices.contains(&p));
                let rewards: Vec<_> = self.opening_grants.rewards.iter()
                    .filter_map(|&(seat,quest)|(seat==p).then_some(quest)).collect();
                w.len(rewards.len());
                for quest in rewards {w.u32(quest);}
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
        let shared_version=version(&mut r)?;
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
            if n > 2 || e.inventory.bags.contains_key(&Owner::Pawn(n)) { return Err("ECON pawn bag identity".into()); }
            e.inventory.bags.insert(Owner::Pawn(n), read_bag(&mut r, crate::inventory::BAG_SLOTS)?);
        }
        if shared_version>=2 {
            e.opening_phase=match r.u8()? {0=>None,1=>Some(crate::new_game::Phase::BeforeCreation),2=>Some(crate::new_game::Phase::DragonChallenge),3=>Some(crate::new_game::Phase::NewlyArisen),_=>return Err("ECON opening phase".into())};
        }
        r.end()?;
        for (&p, b) in &parts.players {
            let mut r = Reader::new(b);
            let player_version=version(&mut r)?;
            if player_version != shared_version { return Err("ECON mixed part versions".into()); }
            if r.bool()? {
                e.wallets.insert(p, Wallet { gold: r.i32()?, rift_crystals: r.i32()? });
            }
            if let Some(b) = read_opt_bag(&mut r, crate::inventory::BAG_SLOTS)? {
                e.inventory.bags.insert(Owner::Arisen(p), b);
            }
            if let Some(b) = read_opt_bag(&mut r, crate::inventory::STORAGE_SLOTS)? {
                e.inventory.bags.insert(Owner::Storage(p), b);
            }
            if player_version>=2 {
                if r.bool()? {e.opening_grants.first_choices.insert(p);}
                for _ in 0..r.len(4)? {
                    let quest=r.u32()?;
                    if !e.opening_grants.rewards.insert((p,quest)) {return Err("ECON repeated reward identity".into());}
                }
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
