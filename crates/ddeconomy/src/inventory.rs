//! Bags, storage and the buy-back list.
//!
//! The original keeps five bags in `sItemManager.mItemBagTbl` (0x11e8 bytes
//! each): the Arisen, three pawns and storage. Here every Arisen has a bag
//! and a storage of its own and pawns have bags (COOP_DESIGN.md: two full
//! players, one shared main pawn, one hired pawn).

use dditem::itl::ItemList;
use std::collections::BTreeMap;

pub type PlayerId = usize;

/// Who holds a bag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Owner {
    Arisen(PlayerId),
    /// Pawn party slot: 0 is the main pawn.
    Pawn(u8),
    Storage(PlayerId),
}

/// Distinct slots per character bag and in storage (`0x00465190`).
pub const BAG_SLOTS: usize = dditem::rules::BAG_SLOTS as usize;
pub const STORAGE_SLOTS: usize = dditem::rules::STORAGE_SLOTS as usize;
/// Count cap of a new slot (`0x0045e680`: `if (999 < count) count = 999`).
pub const STACK_MAX: i16 = 999;

/// Slot flag bits (`+0x20` of a bag slot): bit 0 set on every new slot;
/// bits 1 and 8 mark a stack in use (equipped; excluded from selling,
/// materials and eviction, `0x006e17c0`, `0x006f3230`, `0x0045dc30`); the
/// level bits are those of [`dditem::rules::instance_level`].
pub const FLAG_VALID: u32 = 0x1;
pub const FLAG_IN_USE: u32 = 0x2;
pub const FLAG_IN_USE_2: u32 = 0x4;
pub const FLAG_LOCKED: u32 = 0x100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub item: i16,
    pub num: i16,
    pub flags: u32,
}

impl Slot {
    pub fn level(&self) -> u8 {
        dditem::rules::instance_level(self.flags)
    }

    /// Copies that may be sold (`0x006e17c0`): one fewer with bit 1, two
    /// fewer with bits 1 and 2.
    pub fn sellable(&self) -> i32 {
        let mut n = self.num as i32;
        if self.flags & FLAG_IN_USE != 0 {
            n -= 1;
            if self.flags & FLAG_IN_USE_2 != 0 {
                n -= 1;
            }
        }
        n.max(0)
    }

    /// Copies usable as material (`0x006f3230`): one fewer with bit 1 or 8.
    pub fn usable(&self) -> u32 {
        let mut n = self.num as i32;
        if n != 0 && self.flags & (FLAG_IN_USE | FLAG_LOCKED) != 0 {
            n -= 1;
        }
        n.max(0) as u32
    }
}

/// Flag bit of an enhancement level (inverse of `instance_level`).
pub fn level_flags(level: u8) -> u32 {
    match level {
        1 => 0x8,
        2 => 0x10,
        3 => 0x20,
        4 => 0x40,
        5 => 0x200,
        6 => 0x400,
        _ => 0,
    }
}

/// Item kinds that keep an enhancement level (`0x0045e680` clears the
/// level of kinds below 7 and above 25 except 27).
pub fn keeps_level(kind: u32) -> bool {
    (7..=25).contains(&kind) || kind == 27
}

/// Eviction order of item category types when a bag is full
/// (`0x0045dcc0`, each step `0x0045dc30`).
pub const EVICT_ORDER: [u32; 12] = [2, 0, 1, 5, 5, 6, 7, 8, 9, 10, 11, 3];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bag {
    pub slots: Vec<Slot>,
}

impl Bag {
    pub fn count(&self, item: i16) -> u32 {
        self.slots.iter().filter(|s| s.item == item).map(|s| s.num.max(0) as u32).sum()
    }
}

/// Buy-back list of sold unique items (`sItemManager` junk list, count
/// `+0xe8`, entries `cJunkData {mNum, mItemNo}`), sold again by the Junk
/// screen (The Black Cat). World state: one list for the session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuyBack {
    pub entries: Vec<(i16, u16)>,
}

impl BuyBack {
    /// Translated from `0x00461f30`: only items accepted by
    /// [`dditem::rules::buyback_eligible`]; item 0xa2 is stored as 0x116;
    /// counts add up while below 0xffff.
    pub fn add(&mut self, item: i16, count: u32) {
        if !dditem::rules::buyback_eligible(item as u32) {
            return;
        }
        let item = if item == 0xa2 { 0x116 } else { item };
        if let Some(e) = self.entries.iter_mut().find(|e| e.0 == item) {
            if count + (e.1 as u32) < 0xffff {
                e.1 += count as u16;
            }
            return;
        }
        self.entries.push((item, count.min(0xffff) as u16));
    }

    /// Bought back (`0x00462070`): lower the count, drop empty entries.
    pub fn take(&mut self, item: i16, count: u32) {
        if let Some(i) = self.entries.iter().position(|e| e.0 == item) {
            let e = &mut self.entries[i];
            e.1 = e.1.saturating_sub(count.min(0xffff) as u16);
            if e.1 == 0 {
                self.entries.remove(i);
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    pub bags: BTreeMap<Owner, Bag>,
    /// The main pawn is in the party (sGameSys `+0xa7ec4`); used when the
    /// Arisen's bag and storage are both full.
    pub main_pawn_present: bool,
}

impl Inventory {
    pub fn bag(&self, o: Owner) -> Option<&Bag> {
        self.bags.get(&o)
    }

    pub fn bag_mut(&mut self, o: Owner) -> &mut Bag {
        self.bags.entry(o).or_default()
    }

    fn storage_of(o: Owner, player: PlayerId) -> Owner {
        match o {
            Owner::Arisen(p) | Owner::Storage(p) => Owner::Storage(p),
            Owner::Pawn(_) => Owner::Storage(player),
        }
    }

    /// Add items to `owner`'s bag. `player` names the storage a pawn's
    /// overflow goes to.
    ///
    /// Translated from `0x0045e680` (stacking, capacity, redirect) and
    /// `0x0045dcc0` (eviction). A matching stack (same item and level)
    /// grows; the cap of 999 is traced for new slots and applied to grown
    /// stacks as well (**guessed**: `0x00458600` not read). A new slot in a
    /// full bag first evicts a stack to storage; with storage full the item
    /// goes to the main pawn (from the Arisen, when present) or to storage;
    /// items for a full storage go to the buy-back list instead (lost
    /// unless eligible). Per-instance item data (flag bits 29-31 items) is
    /// not modelled.
    #[allow(clippy::too_many_arguments)]
    pub fn add(&mut self, items: &ItemList, owner: Owner, player: PlayerId, item: i16, count: u32, level: u8, buyback: &mut BuyBack) {
        if count == 0 || item < 0 {
            return;
        }
        let kind = items.get(item as u32).map(|r| r.kind()).unwrap_or(0);
        let level = if keeps_level(kind) { level } else { 0 };
        let lf = level_flags(level);
        let bag = self.bag_mut(owner);
        if let Some(s) = bag.slots.iter_mut().find(|s| s.item == item && s.level() == level) {
            s.num = (s.num as u32 + count).min(STACK_MAX as u32) as i16;
            return;
        }
        let storage = Self::storage_of(owner, player);
        if let Owner::Storage(_) = owner {
            if self.bag_mut(owner).slots.len() >= STORAGE_SLOTS {
                buyback.add(item, count);
                return;
            }
        } else if self.bag_mut(owner).slots.len() >= BAG_SLOTS {
            if self.bag_mut(storage).slots.len() >= STORAGE_SLOTS {
                let to = match owner {
                    Owner::Arisen(_) if self.main_pawn_present => Owner::Pawn(0),
                    _ => storage,
                };
                if to != owner {
                    self.add(items, to, player, item, count, level, buyback);
                }
                return;
            }
            self.evict(items, owner, storage, buyback);
        }
        self.bag_mut(owner).slots.push(Slot {
            item,
            num: count.min(STACK_MAX as u32) as i16,
            flags: FLAG_VALID | lf,
        });
    }

    /// Move the first stack not in use of the first category type in
    /// [`EVICT_ORDER`] that has one to storage (`0x0045dcc0`).
    fn evict(&mut self, items: &ItemList, owner: Owner, storage: Owner, buyback: &mut BuyBack) {
        for cat in EVICT_ORDER {
            let bag = self.bag_mut(owner);
            let pos = bag.slots.iter().position(|s| {
                s.flags & (FLAG_IN_USE | FLAG_LOCKED) == 0
                    && items.get(s.item as u32).map(|r| r.category_type()) == Some(cat)
            });
            if let Some(i) = pos {
                let s = bag.slots.remove(i);
                let player = match storage {
                    Owner::Storage(p) => p,
                    _ => 0,
                };
                self.add(items, storage, player, s.item, s.num as u32, s.level(), buyback);
                return;
            }
        }
    }

    /// Remove `count` copies from slot `index` of `owner`; empty slots go.
    pub fn remove(&mut self, owner: Owner, index: usize, count: u32) {
        let bag = self.bag_mut(owner);
        if let Some(s) = bag.slots.get_mut(index) {
            let n = (s.num as i32 - count as i32).max(0);
            s.num = n as i16;
            if n == 0 {
                bag.slots.remove(index);
            }
        }
    }

    /// Remove `count` copies of `item` from `owner`, slot by slot.
    pub fn remove_item(&mut self, owner: Owner, item: i16, mut count: u32) {
        while count > 0 {
            let Some(i) = self.bag(owner).and_then(|b| b.slots.iter().position(|s| s.item == item)) else {
                return;
            };
            let have = self.bags[&owner].slots[i].usable().min(count);
            if have == 0 {
                return;
            }
            self.remove(owner, i, have);
            count -= have;
        }
    }

    /// Total copies of `item` held by `owners` (`0x00460350` sums the
    /// counts of the asked owners via `0x0045fce0`).
    pub fn count(&self, owners: &[Owner], item: i16) -> u32 {
        owners.iter().filter_map(|o| self.bag(*o)).map(|b| b.count(item)).sum()
    }
}
