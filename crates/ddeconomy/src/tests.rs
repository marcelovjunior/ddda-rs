//! Session tests on a synthetic catalog: one and two players.

use super::*;
use dditem::itl::ItemList;
use dditem::lvup::EquipLvUp;
use dditem::shop::{Judgment, Lineup, ShopList, JUDGMENTS};
use inventory::{Slot, BAG_SLOTS, FLAG_IN_USE, STORAGE_SLOTS};

const SWORD: u32 = 301;
const ORE: u32 = 1227;
const IDOL: u32 = 170;
const HERB: u32 = 43;
const RING: u32 = 0xe5; // buy-back eligible
const CURIO: u32 = 1300; // a rarity

/// Item records: (id, buy, sell, kind, category type).
fn items() -> ItemList {
    let spec: &[(u32, u32, u32, u32, u32)] = &[
        (SWORD, 1000, 400, 7, 5),
        (ORE, 30, 12, 6, 2),
        (IDOL, 125_000, 30_000, 3, 4),
        (HERB, 70, 28, 2, 0),
        (RING, 5000, 2000, 1, 3),
        (CURIO, 1000, 500, 6, 2),
        (66, 2000, 200, 0, 1),
    ];
    let n = 1901u32;
    let mut d = b"ITL2".to_vec();
    d.extend(dditem::itl::STAMP.to_le_bytes());
    d.extend(n.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    for id in 0..n {
        let mut r = [0u8; 128];
        r[0x3c..0x40].copy_from_slice(&id.to_le_bytes());
        if let Some(&(_, buy, sell, kind, cat)) = spec.iter().find(|s| s.0 == id) {
            r[0x48..0x4c].copy_from_slice(&buy.to_le_bytes());
            r[0x4c..0x50].copy_from_slice(&sell.to_le_bytes());
            r[0x38..0x3c].copy_from_slice(&(kind | cat << 5).to_le_bytes());
        }
        d.extend(r);
    }
    ItemList::parse(&d).unwrap()
}

fn lvup() -> EquipLvUp {
    let mut d = b"qlv\0".to_vec();
    d.extend(3u32.to_le_bytes());
    d.extend(1u32.to_le_bytes());
    d.extend((SWORD as u16).to_le_bytes());
    d.extend(0u16.to_le_bytes());
    d.extend(70u32.to_le_bytes());
    for (g, m, c) in [(220u32, ORE as u16, 1u16), (690, ORE as u16, 4), (4000, ORE as u16, 3), (5000, ORE as u16, 5)] {
        d.extend(g.to_le_bytes());
        d.extend(m.to_le_bytes());
        d.extend(c.to_le_bytes());
    }
    EquipLvUp::parse(&d).unwrap()
}

fn none() -> [Judgment; JUDGMENTS] {
    [Judgment::default(); JUDGMENTS]
}

fn line(item: u32, num: i32, rearrival: u32) -> Lineup {
    Lineup {
        item,
        num,
        rearrival,
        open_and: none(),
        open_or: none(),
    }
}

fn shop(shop_type: u32, lineup: Vec<Lineup>) -> ShopList {
    ShopList {
        shop_type,
        discount_ok_item: [IDOL as i32, -1, -1, -1, -1, -1, -1, -1],
        discount_rate: [70, 100, 100, 100, 100, 100, 100, 100],
        default_rate: 100,
        discount_gsf: 0xffff_ffff,
        discount_gsf_rate: 100,
        open_and: none(),
        open_or: none(),
        lineup,
    }
}

const SMITH: u32 = 149;
const BLACK_CAT: u32 = 156;
const FOURNIVAL: u32 = 133;

fn catalog() -> Catalog {
    let mut shops = BTreeMap::new();
    let mut s = shop(1, vec![line(SWORD, 2, 3), line(HERB, -1, 0), line(ORE, 10, 1)]);
    // Herb only from story progress 4800 (command 11).
    s.lineup[1].open_and[0] = Judgment {
        command: 11,
        params: [4800, 0, 0, 0],
    };
    // The shop is closed while GSF 23 is set (command 4).
    s.open_and[0] = Judgment {
        command: 4,
        params: [23, 0, 0, 0],
    };
    shops.insert(SMITH, s);
    shops.insert(BLACK_CAT, shop(3, vec![line(HERB, -1, 0)]));
    shops.insert(FOURNIVAL, shop(7, vec![line(66, 3, 2)]));
    Catalog {
        items: items(),
        shops,
        lvup: lvup(),
        suasion: (1.15, 1.0),
    }
}

fn member(owner: Owner, augments: [u32; 6]) -> Member {
    Member {
        owner,
        present: true,
        level: 10,
        augments,
    }
}

fn party(p: PlayerId) -> Party {
    Party {
        members: vec![member(Owner::Arisen(p), [u32::MAX; 6]), member(Owner::Pawn(0), [u32::MAX; 6])],
    }
}

fn world() -> World {
    let mut w = World::default();
    w.sce_no = 5000;
    w
}

#[test]
fn buy_with_rate_stock_and_wallet() {
    let cat = catalog();
    let w = world();
    let pt = party(0);
    let mut e = Economy::new(w.day);
    e.wallet(0).gold = 3000;
    let s = e.open_shop(&cat, &w, &pt, 0, SMITH).unwrap();
    let offers = e.offers(&cat, &w, &pt, &s).unwrap();
    let sword = offers.iter().find(|o| o.item == SWORD).unwrap();
    assert_eq!((sword.unit_price, sword.stock), (1000, 2));
    assert!(offers.iter().any(|o| o.item == HERB), "progress 5000 >= 4800");
    assert_eq!(e.buy(&cat, &w, &pt, &s, SWORD, 2, Owner::Arisen(0)).unwrap(), 2000);
    assert_eq!(e.wallets[&0].gold, 1000);
    assert_eq!(e.inventory.count(&[Owner::Arisen(0)], SWORD as i16), 2);
    assert_eq!(e.buy(&cat, &w, &pt, &s, SWORD, 1, Owner::Arisen(0)), Err(Error::OutOfStock));
    assert_eq!(e.buy(&cat, &w, &pt, &s, ORE, 10, Owner::Arisen(0)), Ok(300));
    assert_eq!(e.buy(&cat, &w, &pt, &s, HERB, 100, Owner::Arisen(0)), Err(Error::NotEnough(Currency::Gold)));
    // A Gold Idol in the main pawn's bag gives 70%.
    e.inventory.add(&cat.items, Owner::Pawn(0), 0, IDOL as i16, 1, 0, &mut BuyBack::default());
    let herb = e.offers(&cat, &w, &pt, &s).unwrap().into_iter().find(|o| o.item == HERB).unwrap();
    assert_eq!(herb.unit_price, 49);
    // Below progress 4800 the herb is not offered.
    let mut early = world();
    early.sce_no = 100;
    assert!(!e.offers(&cat, &early, &pt, &s).unwrap().iter().any(|o| o.item == HERB));
    e.close(s);
    // GSF 23 closes the shop.
    let mut closed = world();
    closed.gsf.set(23, true);
    assert_eq!(e.open_shop(&cat, &closed, &pt, 0, SMITH), Err(Error::Closed));
}

#[test]
fn sell_suasion_level_buyback_and_stock() {
    let cat = catalog();
    let w = world();
    let mut pt = party(0);
    let mut e = Economy::new(w.day);
    let mut bb = BuyBack::default();
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, SWORD as i16, 1, 3, &mut bb);
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, RING as i16, 1, 0, &mut bb);
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, ORE as i16, 5, 0, &mut bb);
    let s = e.open_shop(&cat, &w, &pt, 0, SMITH).unwrap();
    // Sword level 3: 400 * 145% = 580.
    assert_eq!(e.sell(&cat, &pt, &s, Owner::Arisen(0), 0, 1), Ok(580));
    // A pawn with Suasion (0x54): trunc(2000 * 1.15f32) = 2299.
    pt.members[1].augments[2] = 0x54;
    let ring = e.inventory.bag(Owner::Arisen(0)).unwrap().slots.iter().position(|x| x.item == RING as i16).unwrap();
    assert_eq!(e.sell(&cat, &pt, &s, Owner::Arisen(0), ring, 1), Ok(2299));
    assert_eq!(e.buyback.entries, vec![(RING as i16, 1)]);
    // Selling ore the shop lists raises its stock (10 -> 13, capped 999).
    let ore = e.inventory.bag(Owner::Arisen(0)).unwrap().slots.iter().position(|x| x.item == ORE as i16).unwrap();
    assert_eq!(e.sell(&cat, &pt, &s, Owner::Arisen(0), ore, 3), Ok(3 * 13));
    assert_eq!(e.stock.left(SMITH, ORE, false), 13);
    assert_eq!(e.buyback.entries.len(), 1, "ore is not buy-back eligible");
    assert_eq!(e.wallets[&0].gold, 580 + 2299 + 39);
    e.close(s);
    e.wallet(0).gold += 5000;
    // The Black Cat sells the ring back at base price and has no sell mode.
    let s = e.open_shop(&cat, &w, &pt, 0, BLACK_CAT).unwrap();
    let o = e.offers(&cat, &w, &pt, &s).unwrap();
    assert_eq!((o[0].item, o[0].unit_price, o[0].stock, o[0].buyback), (RING, 5000, 1, true));
    assert_eq!(e.buy(&cat, &w, &pt, &s, RING, 1, Owner::Storage(0)), Ok(5000));
    assert!(e.buyback.entries.is_empty());
    assert_eq!(e.sell(&cat, &pt, &s, Owner::Storage(0), 0, 1), Err(Error::NoSelling));
}

#[test]
fn rarities_shop() {
    let cat = catalog();
    let w = world();
    let pt = party(0);
    let mut e = Economy::new(w.day);
    e.wallet(0).gold = 10_000;
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, CURIO as i16, 1, 0, &mut BuyBack::default());
    let s = e.open_shop(&cat, &w, &pt, 0, FOURNIVAL).unwrap();
    let o = e.offers(&cat, &w, &pt, &s).unwrap();
    assert_eq!(o[0].unit_price, 1000, "Ferrystone 2000 at half price");
    assert_eq!(e.sell(&cat, &pt, &s, Owner::Arisen(0), 0, 1), Ok(600), "rarity +20%");
}

#[test]
fn daily_restock_from_the_world_clock() {
    let cat = catalog();
    let mut w = world();
    let pt = party(0);
    let mut e = Economy::new(w.day);
    e.wallet(0).gold = 100_000;
    let s = e.open_shop(&cat, &w, &pt, 0, SMITH).unwrap();
    e.buy(&cat, &w, &pt, &s, SWORD, 2, Owner::Arisen(0)).unwrap();
    assert_eq!(e.stock.left(SMITH, SWORD, false), 0);
    w.set_clock(10, 0); // same day, later: no day change
    e.update(w.day, false);
    let mut sword_after = vec![];
    for _ in 0..6 {
        w.set_clock(10, 0); // same time next day: the day counter advances
        e.update(w.day, false);
        sword_after.push(e.stock.left(SMITH, SWORD, false));
    }
    // Rearrival 3: one sword every third day, up to the full 2.
    assert_eq!(sword_after, vec![0, 0, 1, 1, 1, 2]);
    // Several days at once still give one tick.
    e.buy(&cat, &w, &pt, &s, SWORD, 2, Owner::Arisen(0)).unwrap();
    w.day += 10;
    e.update(w.day, false);
    assert_eq!(e.stock.left(SMITH, SWORD, false), 0);
}

#[test]
fn two_players_lock_wallets_and_bags() {
    let cat = catalog();
    let w = world();
    let p0 = party(0);
    let mut p1 = party(1);
    p1.members.push(member(Owner::Arisen(0), [u32::MAX; 6]));
    let mut e = Economy::new(w.day);
    e.wallet(0).gold = 5000;
    e.wallet(1).gold = 1500;
    let s0 = e.open_shop(&cat, &w, &p0, 0, SMITH).unwrap();
    assert_eq!(e.open_shop(&cat, &w, &p1, 1, SMITH), Err(Error::NpcBusy(0)));
    assert_eq!(e.open_upgrade(1, SMITH, false), Err(Error::NpcBusy(0)));
    // Player 1 trades with another NPC meanwhile.
    let other = e.open_shop(&cat, &w, &p1, 1, FOURNIVAL).unwrap();
    e.close(other);
    e.buy(&cat, &w, &p0, &s0, SWORD, 1, Owner::Arisen(0)).unwrap();
    e.close(s0);
    let s1 = e.open_shop(&cat, &w, &p1, 1, SMITH).unwrap();
    // A closed session is refused.
    assert_eq!(e.offers(&cat, &w, &p0, &s0), Err(Error::WrongSession));
    // Stock is shared world state: one sword left.
    let o = e.offers(&cat, &w, &p1, &s1).unwrap();
    assert_eq!(o.iter().find(|o| o.item == SWORD).unwrap().stock, 1);
    // Player 1 cannot deliver to or sell from player 0's bags.
    assert_eq!(e.buy(&cat, &w, &p1, &s1, SWORD, 1, Owner::Arisen(0)), Err(Error::NotYours));
    assert_eq!(e.sell(&cat, &p1, &s1, Owner::Arisen(0), 0, 1), Err(Error::NotYours));
    assert_eq!(e.buy(&cat, &w, &p1, &s1, SWORD, 1, Owner::Pawn(0)), Ok(1000));
    assert_eq!((e.wallets[&0].gold, e.wallets[&1].gold), (4000, 500));
    assert_eq!(e.inventory.count(&[Owner::Pawn(0)], SWORD as i16), 1);
}

#[test]
fn enhancement_sessions() {
    let cat = catalog();
    let w = world();
    let pt = party(0);
    let mut e = Economy::new(w.day);
    let mut bb = BuyBack::default();
    e.wallet(0).gold = 1000;
    e.wallet(0).rift_crystals = 4500;
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, SWORD as i16, 2, 0, &mut bb);
    e.inventory.add(&cat.items, Owner::Pawn(0), 0, ORE as i16, 2, 0, &mut bb);
    e.inventory.add(&cat.items, Owner::Storage(0), 0, ORE as i16, 5, 0, &mut bb);
    let s = e.open_upgrade(0, SMITH, false).unwrap();
    let offers = e.enhance_offers(&cat, &pt, &s, Owner::Arisen(0), 0).unwrap();
    let targets: Vec<_> = offers.iter().map(|o| (o.0, o.2)).collect();
    assert_eq!(targets, vec![(1, true), (2, true), (3, true)]);
    // Level 1: 70 gold, no material; one copy of the two splits off.
    e.enhance(&cat, &pt, &s, Owner::Arisen(0), 0, 1).unwrap();
    let bag = &e.inventory.bags[&Owner::Arisen(0)];
    assert_eq!((bag.slots[0].num, bag.slots[0].level()), (1, 0));
    assert_eq!((bag.slots[1].num, bag.slots[1].level()), (1, 1));
    // Level 2 on the enhanced copy: 220 gold + 1 ore, taken from the pawn
    // (the last bag owner) before storage.
    e.enhance(&cat, &pt, &s, Owner::Arisen(0), 1, 2).unwrap();
    assert_eq!(e.inventory.count(&[Owner::Pawn(0)], ORE as i16), 1);
    assert_eq!(e.inventory.count(&[Owner::Storage(0)], ORE as i16), 5);
    assert_eq!(e.wallets[&0].gold, 1000 - 70 - 220);
    e.wallet(0).gold = 600;
    assert_eq!(e.enhance(&cat, &pt, &s, Owner::Arisen(0), 1, 3), Err(Error::NotEnough(Currency::Gold)));
    e.wallet(0).gold += 1000;
    // Level 3: 4 ore = 1 from the pawn, 3 from storage.
    e.enhance(&cat, &pt, &s, Owner::Arisen(0), 1, 3).unwrap();
    assert_eq!(e.inventory.count(&[Owner::Pawn(0), Owner::Storage(0)], ORE as i16), 2);
    assert_eq!(e.inventory.bags[&Owner::Arisen(0)].slots[1].level(), 3);
    e.close(s);
    // Level 5 needs dragonforged (4) and the extended screen, paid in RC.
    let s = e.open_upgrade(0, 784, true).unwrap();
    assert!(e.enhance_offers(&cat, &pt, &s, Owner::Arisen(0), 1).unwrap().is_empty());
    let slot = &mut e.inventory.bag_mut(Owner::Arisen(0)).slots[1];
    slot.flags = (slot.flags & !0x20) | 0x40 | FLAG_IN_USE;
    e.inventory.add(&cat.items, Owner::Storage(0), 0, ORE as i16, 1, 0, &mut bb);
    let o = e.enhance_offers(&cat, &pt, &s, Owner::Arisen(0), 1).unwrap();
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].0, 5);
    assert_eq!(o[0].1, (Currency::RiftCrystals, 4000, Some((ORE as u16, 3))));
    e.enhance(&cat, &pt, &s, Owner::Arisen(0), 1, 5).unwrap();
    let slot = e.inventory.bags[&Owner::Arisen(0)].slots[1];
    assert_eq!(slot.level(), 5);
    assert!(slot.flags & FLAG_IN_USE != 0, "an equipped single copy stays equipped");
    assert_eq!(e.wallets[&0].rift_crystals, 500);
}

#[test]
fn bag_full_eviction_and_overflow() {
    let cat = catalog();
    let mut inv = Inventory::default();
    let mut bb = BuyBack::default();
    let a = Owner::Arisen(0);
    // Fill the bag with 256 distinct stacks: one herb (category 0) and
    // 255 fake items of category 4 (ids >= 1500 have category 0 here, so
    // use in-use flags to keep them from eviction).
    inv.bag_mut(a).slots.push(Slot { item: HERB as i16, num: 3, flags: 1 });
    for i in 0..(BAG_SLOTS as i16 - 1) {
        inv.bag_mut(a).slots.push(Slot { item: 1500 + i, num: 1, flags: 1 | FLAG_IN_USE });
    }
    inv.add(&cat.items, a, 0, SWORD as i16, 1, 0, &mut bb);
    // Category 2 has nothing free, category 0 (the herb) is evicted.
    assert_eq!(inv.count(&[Owner::Storage(0)], HERB as i16), 3);
    assert_eq!(inv.count(&[a], SWORD as i16), 1);
    // With storage full too, the Arisen's overflow goes to the main pawn.
    for i in 0..STORAGE_SLOTS as i16 {
        inv.bag_mut(Owner::Storage(0)).slots.push(Slot { item: 300 + i % 1500, num: 1, flags: 1 });
    }
    inv.main_pawn_present = true;
    inv.add(&cat.items, a, 0, ORE as i16, 2, 0, &mut bb);
    assert_eq!(inv.count(&[Owner::Pawn(0)], ORE as i16), 2);
    // A full storage sends eligible items to the buy-back list.
    inv.add(&cat.items, Owner::Storage(0), 0, RING as i16, 1, 0, &mut bb);
    assert_eq!(bb.entries, vec![(RING as i16, 1)]);
}
