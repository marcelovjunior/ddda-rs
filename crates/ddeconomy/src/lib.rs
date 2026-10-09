//! Headless economy runtime for Dragon's Dogma: Dark Arisen: wallets,
//! bags and storage, shop sessions (open conditions, stock, buy, sell,
//! buy-back), daily restock and enhancement. Rules are the traced ones in
//! `dditem` (see docs/SHOPS.md); this crate keeps the state and applies
//! them for any number of players.
//!
//! The host owns one [`Economy`] for the session. Shop and upgrade screens
//! are [`Session`]s holding an NPC interaction lock: one player per NPC
//! (docs/COOP_DESIGN.md). Story state (flags, progress, affinity, the day
//! counter) comes from `ddstory::World`.
//!
//! Players are seats (`ddid::PlayerId`, u8, 0 = the host). [`Economy::save`]
//! / [`Economy::load`] give the host's `ECON` chunks: a shared part and one
//! part per seat (`save` module; docs/adr/0002-game-host.md).

pub mod inventory;
pub mod item_use;
pub mod save;
pub mod stock;
pub mod wallet;

use dditem::forge;
use dditem::itl::ItemList;
use dditem::lvup::EquipLvUp;
use dditem::rules::{self, Currency, JudgmentState};
use dditem::shop::ShopList;
use ddstory::World;
use inventory::{level_flags, BuyBack, Inventory, Owner, PlayerId, FLAG_VALID};
use std::collections::{BTreeMap, BTreeSet};
use stock::StockBook;
use wallet::Wallet;

/// Game data the economy reads.
#[derive(Debug, Clone)]
pub struct Catalog {
    pub items: ItemList,
    /// Shop lists by NPC number, as `uNpc::vf32` loads them (the `dl1`
    /// copies for NPCs 784 and 156).
    pub shops: BTreeMap<u32, ShopList>,
    pub lvup: EquipLvUp,
    /// `plAbilityParam[0x54]` `mFVal0` / `mFVal1` (1.15 / 1.0 shipped).
    pub suasion: (f32, f32),
}

/// One party slot as the host reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub owner: Owner,
    /// In the party and active (record `+4` set, `+0x354` clear).
    pub present: bool,
    /// Record `+0x700` (judgment commands 20-22).
    pub level: u32,
    /// Equipped augment ids (record `+0x258`).
    pub augments: [u32; 6],
}

/// The party as seen by one player: their own Arisen first, then the other
/// members. The original has four slots (Arisen, main pawn, two hired);
/// in co-op the second Arisen takes a hired slot (COOP_DESIGN.md).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Party {
    pub members: Vec<Member>,
}

impl Party {
    fn bags(&self) -> Vec<Owner> {
        self.members.iter().filter(|m| m.present).map(|m| m.owner).collect()
    }
}

/// Lineup gates outside the shop lists (`0x00cdde40`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gates {
    /// Items of the platform-unlock table (`0x018227f8`) in state 2.
    pub platform_hidden: BTreeSet<u32>,
    /// One-time cosmetics 1613-1633 already used (flags `+0x20e72d..`).
    pub cosmetics_used: BTreeSet<u32>,
    /// Counter `+0x20e728`; -1 hides items 1634/1635.
    pub metamorphosis: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Another player holds this NPC.
    NpcBusy(PlayerId),
    NoShop,
    Closed,
    WrongSession,
    NotOffered,
    OutOfStock,
    NotEnough(Currency),
    /// The Junk screen has no sell mode (`0x006e45d0`).
    NoSelling,
    NotSellable,
    NoSuchSlot,
    /// The player may not use this bag.
    NotYours,
    NotEnhanceable,
    MissingMaterials,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Shop,
    /// Upgrade screen; `extended` is mode 3 (levels 5-6).
    Upgrade { extended: bool },
}

/// An open screen: holds the NPC lock until [`Economy::close`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub player: PlayerId,
    pub npc: u32,
    pub kind: SessionKind,
}

/// One buyable line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    pub item: u32,
    pub unit_price: u32,
    /// -1 = unlimited.
    pub stock: i32,
    pub currency: Currency,
    /// From the buy-back list (Junk screen).
    pub buyback: bool,
}

/// Screen kinds by `mShopType` (`0x00510d70`).
pub fn shop_type_rim(t: u32) -> bool {
    t == 6
}
pub fn shop_type_junk(t: u32) -> bool {
    t == 3
}
pub fn shop_type_rarities(t: u32) -> bool {
    t == 7
}

struct Judge<'a> {
    world: &'a World,
    party: &'a Party,
    inv: &'a Inventory,
    storage: Owner,
}

impl Judge<'_> {
    /// `0x00460350`: copies held by `owners`, plus the paired item for ids
    /// 1-42, compared with `count`.
    fn holds(&self, owners: &[Owner], item: u32, count: u32) -> bool {
        let mut n = self.inv.count(owners, item as i16);
        if n >= count {
            return true;
        }
        if let Some(p) = rules::paired_item(item) {
            n += self.inv.count(owners, p as i16);
        }
        n >= count
    }
}

impl JudgmentState for Judge<'_> {
    fn quest_flag(&self, quest: u32, flag: u32) -> bool {
        quest < 0x80 && flag < 0x60 && self.world.quest_bit(quest as usize, flag as usize)
    }
    fn gsf(&self, flag: u32) -> bool {
        (flag as usize) < ddstory::world::GSF_COUNT && self.world.gsf.get(flag as usize)
    }
    fn progress(&self) -> i32 {
        self.world.sce_no as i32
    }
    fn affinity(&self, npc: i32) -> i32 {
        if npc < 0 {
            0
        } else if npc >= 900 {
            500
        } else {
            self.world.npc_friend.get(npc as usize).copied().unwrap_or(500) as i32
        }
    }
    /// Commands 16/31 ask with mode 1: the party's bags and storage.
    fn has_item(&self, item: u32, count: u32) -> bool {
        let mut owners = self.party.bags();
        owners.push(self.storage);
        self.holds(&owners, item, count)
    }
    fn party_value(&self, slot: u32) -> u32 {
        self.party.members.get(slot as usize).map(|m| m.level).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Economy {
    pub wallets: BTreeMap<PlayerId, Wallet>,
    pub inventory: Inventory,
    pub buyback: BuyBack,
    pub stock: StockBook,
    pub gates: Gates,
    /// Day counter at the last restock (`sNpcManager +0x48038`).
    pub last_day: u64,
    locks: BTreeMap<u32, PlayerId>,
}

impl Economy {
    pub fn new(day: u64) -> Self {
        Economy {
            last_day: day,
            ..Default::default()
        }
    }

    pub fn wallet(&mut self, p: PlayerId) -> &mut Wallet {
        self.wallets.entry(p).or_default()
    }

    /// Run once per host update with `World::day` (`sNpcManager::vf06`,
    /// `0x00475ad0`): one restock tick when the day has advanced, however
    /// many days passed. `blocked` is sGameSys `+0xb8844` bit 0x80000.
    pub fn update(&mut self, day: u64, blocked: bool) {
        if day > self.last_day {
            if !blocked {
                self.stock.tick();
            }
            self.last_day = day;
        }
    }

    fn lock(&mut self, player: PlayerId, npc: u32) -> Result<(), Error> {
        match self.locks.get(&npc) {
            Some(&p) if p != player => Err(Error::NpcBusy(p)),
            _ => {
                self.locks.insert(npc, player);
                Ok(())
            }
        }
    }

    pub fn locked_by(&self, npc: u32) -> Option<PlayerId> {
        self.locks.get(&npc).copied()
    }

    pub fn close(&mut self, s: Session) {
        if self.locks.get(&s.npc) == Some(&s.player) {
            self.locks.remove(&s.npc);
        }
    }

    /// Ends every session `player` holds (the player left the session).
    pub fn close_all(&mut self, player: PlayerId) {
        self.locks.retain(|_, p| *p != player);
    }

    fn may_use(player: PlayerId, party: &Party, o: Owner) -> bool {
        match o {
            Owner::Arisen(p) | Owner::Storage(p) => p == player,
            Owner::Pawn(_) => party.members.iter().any(|m| m.owner == o && m.present),
        }
    }

    /// Open a shop. Translated from `0x00cddf60` (shop open: Olra, NPC 783,
    /// is closed on stage 200; then the open AND/OR lists) and `0x00477ad0`
    /// (stock record).
    pub fn open_shop(&mut self, cat: &Catalog, world: &World, party: &Party, player: PlayerId, npc: u32) -> Result<Session, Error> {
        let list = cat.shops.get(&npc).ok_or(Error::NoShop)?;
        if let Some(&p) = self.locks.get(&npc) {
            if p != player {
                return Err(Error::NpcBusy(p));
            }
        }
        if npc == 0x30f && world.stage_no == 200 {
            return Err(Error::Closed);
        }
        let judge = Judge {
            world,
            party,
            inv: &self.inventory,
            storage: Owner::Storage(player),
        };
        if !rules::conditions_open(&list.open_and, &list.open_or, npc as i32, &judge) {
            return Err(Error::Closed);
        }
        self.lock(player, npc)?;
        self.stock.ensure(npc, list);
        Ok(Session {
            player,
            npc,
            kind: SessionKind::Shop,
        })
    }

    /// Open an upgrade screen at `npc` (talk handlers 7 and 17).
    pub fn open_upgrade(&mut self, player: PlayerId, npc: u32, extended: bool) -> Result<Session, Error> {
        self.lock(player, npc)?;
        Ok(Session {
            player,
            npc,
            kind: SessionKind::Upgrade { extended },
        })
    }

    fn check(&self, s: &Session, kind: SessionKind) -> Result<(), Error> {
        if self.locks.get(&s.npc) != Some(&s.player) || s.kind != kind {
            return Err(Error::WrongSession);
        }
        Ok(())
    }

    /// The buy list. Translated from `0x006e1b40` (buy-back first, at base
    /// price) and `0x006e14b0` (lineup: gates and conditions `0x00cdde40`,
    /// price `buy * rate / 100`, Rarities halving, duplicate items merge
    /// their stock). Marks newly opened entries as seen.
    pub fn offers(&mut self, cat: &Catalog, world: &World, party: &Party, s: &Session) -> Result<Vec<Offer>, Error> {
        self.check(s, SessionKind::Shop)?;
        let list = cat.shops.get(&s.npc).ok_or(Error::NoShop)?;
        let currency = if shop_type_rim(list.shop_type) {
            Currency::RiftCrystals
        } else {
            Currency::Gold
        };
        let mut out: Vec<Offer> = Vec::new();
        if shop_type_junk(list.shop_type) {
            for &(item, num) in &self.buyback.entries {
                let base = cat.items.get(item as u32).map(|r| r.buy_price()).unwrap_or(0);
                out.push(Offer {
                    item: item as u32,
                    unit_price: base,
                    stock: num as i32,
                    currency,
                    buyback: true,
                });
            }
        }
        let judge = Judge {
            world,
            party,
            inv: &self.inventory,
            storage: Owner::Storage(s.player),
        };
        let bags = party.bags();
        let rate = rules::shop_rate(list, |f| judge.gsf(f), |item| judge.holds(&bags, item, 1));
        // 0x00477930 asks the party bags (0x00460350) and then a second
        // lookup (0x00458860); counting storage too is the closest model.
        let has_finger = judge.has_item(0x8c, 1);
        let mut seen = Vec::new();
        for (i, l) in list.lineup.iter().enumerate() {
            if self.gates.platform_hidden.contains(&l.item)
                || ((1613..=1633).contains(&l.item) && self.gates.cosmetics_used.contains(&l.item))
                || (self.gates.metamorphosis == -1 && matches!(l.item, 0x662 | 0x663))
            {
                continue;
            }
            if !rules::conditions_open(&l.open_and, &l.open_or, s.npc as i32, &judge) {
                continue;
            }
            seen.push(i);
            let left = self.stock.left(s.npc, l.item, has_finger);
            if let Some(o) = out.iter_mut().find(|o| o.item == l.item && !o.buyback) {
                if o.stock != -1 {
                    o.stock = if left == -1 { -1 } else { o.stock + left };
                }
                continue;
            }
            let base = cat.items.get(l.item).map(|r| r.buy_price()).unwrap_or(0);
            out.push(Offer {
                item: l.item,
                unit_price: rules::buy_price(l.item, base, rate, shop_type_rarities(list.shop_type)),
                stock: left,
                currency,
                buyback: false,
            });
        }
        for i in seen {
            self.stock.mark_seen(s.npc, i);
        }
        Ok(out)
    }

    /// Buy `count` of `item` into `to`. The total is the cart sum
    /// `unit * count` (`0x005ea940`), paid on confirmation (CheckItem,
    /// `0x006e3345`); then items are added (`0x0045e680`), bought buy-back
    /// entries removed (`0x00462070`) and stock lowered (`0x00477a40`;
    /// all in `0x006e6640`). Returns the price paid.
    #[allow(clippy::too_many_arguments)]
    pub fn buy(&mut self, cat: &Catalog, world: &World, party: &Party, s: &Session, item: u32, count: u32, to: Owner) -> Result<u32, Error> {
        if !Self::may_use(s.player, party, to) {
            return Err(Error::NotYours);
        }
        let offers = self.offers(cat, world, party, s)?;
        let o = offers.iter().find(|o| o.item == item).copied().ok_or(Error::NotOffered)?;
        if count == 0 || (o.stock != -1 && (o.stock as u32) < count) {
            return Err(Error::OutOfStock);
        }
        let total = o.unit_price.checked_mul(count).ok_or(Error::NotEnough(o.currency))?;
        let w = self.wallet(s.player);
        if (w.balance(o.currency) as i64) < total as i64 {
            return Err(Error::NotEnough(o.currency));
        }
        w.spend(o.currency, total);
        self.inventory
            .add(&cat.items, to, s.player, item as i16, count, 0, &mut self.buyback);
        if o.buyback {
            self.buyback.take(item as i16, count);
        } else {
            self.stock.adjust(s.npc, item, -(count as i32));
        }
        Ok(total)
    }

    /// Sell `count` from slot `index` of `from`. Unit price `0x006e17c0`
    /// (party Suasion factor `0x0044b1f0`, level percent, Rarities +20%
    /// for rarities); paid in the shop's currency; the items leave
    /// (`0x004588c0`), unique ones go to the buy-back list (`0x00461f30`),
    /// and the shop's stock of that item rises by `count` (`0x00477a40`).
    #[allow(clippy::too_many_arguments)]
    pub fn sell(&mut self, cat: &Catalog, party: &Party, s: &Session, from: Owner, index: usize, count: u32) -> Result<u32, Error> {
        self.check(s, SessionKind::Shop)?;
        if !Self::may_use(s.player, party, from) {
            return Err(Error::NotYours);
        }
        let list = cat.shops.get(&s.npc).ok_or(Error::NoShop)?;
        if shop_type_junk(list.shop_type) {
            return Err(Error::NoSelling);
        }
        let slot = *self
            .inventory
            .bag(from)
            .and_then(|b| b.slots.get(index))
            .ok_or(Error::NoSuchSlot)?;
        if count == 0 || (slot.sellable() as u32) < count {
            return Err(Error::NotSellable);
        }
        let mut with = [false; 4];
        for (i, m) in party.members.iter().take(4).enumerate() {
            with[i] = m.present && m.augments.contains(&forge::AUGMENT_SUASION);
        }
        let f = rules::sell_factor(with, cat.suasion.0, cat.suasion.1);
        let base = cat.items.get(slot.item as u32).map(|r| r.sell_price()).unwrap_or(0);
        let rare = shop_type_rarities(list.shop_type) && rules::is_rarity(slot.item as u32);
        let unit = rules::sell_price(base, f, slot.level(), rare);
        let total = unit.saturating_mul(count);
        let currency = if shop_type_rim(list.shop_type) {
            Currency::RiftCrystals
        } else {
            Currency::Gold
        };
        self.wallet(s.player).add(currency, total);
        self.inventory.remove(from, index, count);
        self.buyback.add(slot.item, count);
        self.stock.adjust(s.npc, slot.item as u32, count as i32);
        Ok(total)
    }

    /// Material owners of a player's upgrade screen: the party bags, then
    /// the player's storage last.
    fn material_owners(party: &Party, player: PlayerId) -> Vec<Owner> {
        let mut v = party.bags();
        v.push(Owner::Storage(player));
        v
    }

    /// What the upgrade screen offers for one item (`0x006f3350`), with
    /// the charge and whether it is affordable now.
    pub fn enhance_offers(&self, cat: &Catalog, party: &Party, s: &Session, owner: Owner, index: usize) -> Result<Vec<(u8, forge::Charge, bool)>, Error> {
        let SessionKind::Upgrade { extended } = s.kind else {
            return Err(Error::WrongSession);
        };
        self.check(s, s.kind)?;
        let slot = *self.inventory.bag(owner).and_then(|b| b.slots.get(index)).ok_or(Error::NoSuchSlot)?;
        let row = cat.lvup.row(slot.item as u16).ok_or(Error::NotEnhanceable)?;
        let cur = slot.level();
        let owners = Self::material_owners(party, s.player);
        let wallet = self.wallets.get(&s.player).copied().unwrap_or_default();
        let mut out = Vec::new();
        for target in cur + 1..=6 {
            if rules::upgrade_offer(cur, target, extended).is_none() {
                continue;
            }
            let Some(charge) = forge::enhance_charge(row, target) else { continue };
            let money = wallet.balance(charge.0) as i64 >= charge.1 as i64;
            let mats = charge.2.is_none_or(|(m, n)| self.usable(&owners, m as i16) >= n as u32);
            out.push((target, charge, money && mats));
        }
        Ok(out)
    }

    fn usable(&self, owners: &[Owner], item: i16) -> u32 {
        owners
            .iter()
            .filter_map(|o| self.inventory.bag(*o))
            .flat_map(|b| b.slots.iter().filter(|s| s.item == item))
            .map(|s| s.usable())
            .sum()
    }

    /// Enhance one copy of slot `index` of `owner` to `target`.
    /// Translated from `0x006f29f0` (commit), `0x006f3520` (pay),
    /// `0x006f35f0` (materials, via `forge::take_materials`) and
    /// `0x004612c0` (one copy re-added with the new level; a single
    /// equipped copy keeps its in-use flags).
    #[allow(clippy::too_many_arguments)]
    pub fn enhance(&mut self, cat: &Catalog, party: &Party, s: &Session, owner: Owner, index: usize, target: u8) -> Result<forge::Charge, Error> {
        if !Self::may_use(s.player, party, owner) {
            return Err(Error::NotYours);
        }
        let offers = self.enhance_offers(cat, party, s, owner, index)?;
        let &(_, charge, ok) = offers.iter().find(|o| o.0 == target).ok_or(Error::NotEnhanceable)?;
        if !ok {
            let w = self.wallets.get(&s.player).copied().unwrap_or_default();
            return Err(if (w.balance(charge.0) as i64) < charge.1 as i64 {
                Error::NotEnough(charge.0)
            } else {
                Error::MissingMaterials
            });
        }
        if let Some((m, n)) = charge.2 {
            let owners = Self::material_owners(party, s.player);
            let usable: Vec<u32> = owners
                .iter()
                .map(|o| {
                    self.inventory
                        .bag(*o)
                        .map(|b| b.slots.iter().filter(|x| x.item == m as i16).map(|x| x.usable()).sum())
                        .unwrap_or(0)
                })
                .collect();
            for (i, take) in forge::take_materials(&usable, n as u32) {
                self.inventory.remove_item(owners[i], m as i16, take);
            }
            if m == 0xe5 {
                self.buyback.add(m as i16, n as u32);
            }
        }
        self.wallet(s.player).spend(charge.0, charge.1);
        let bag = self.inventory.bag_mut(owner);
        let slot = bag.slots.get_mut(index).ok_or(Error::NoSuchSlot)?;
        let old = slot.level();
        if slot.num == 1 {
            slot.flags = (slot.flags & !level_flags(old)) | level_flags(target);
        } else {
            let item = slot.item;
            slot.num -= 1;
            bag.slots.push(inventory::Slot {
                item,
                num: 1,
                flags: FLAG_VALID | level_flags(target),
            });
        }
        Ok(charge)
    }
}

#[cfg(test)]
mod tests;
