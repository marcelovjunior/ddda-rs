//! Synthetic regression tests for the readers and the translated rules.

use crate::rules::*;
use crate::shop::{Judgment, ShopList, JUDGMENTS};
use crate::*;

fn shop() -> ShopList {
    ShopList {
        shop_type: 1,
        discount_ok_item: [170, 279, 198, 280, 199, 281, 218, 152],
        discount_rate: [70, 70, 80, 80, 90, 90, 92, 95],
        default_rate: 100,
        discount_gsf: 0xffff_ffff,
        discount_gsf_rate: 100,
        open_and: [Judgment::default(); JUDGMENTS],
        open_or: [Judgment::default(); JUDGMENTS],
        lineup: vec![],
    }
}

#[test]
fn rate_picks_gsf_then_first_owned_item_then_default() {
    let mut s = shop();
    assert_eq!(shop_rate(&s, |_| true, |_| false), 100);
    assert_eq!(shop_rate(&s, |_| false, |i| i == 199 || i == 152), 90);
    assert_eq!(shop_rate(&s, |_| false, |i| i == 280), 80);
    s.discount_gsf = 0x20d;
    s.discount_gsf_rate = 90;
    assert_eq!(shop_rate(&s, |f| f == 0x20d, |i| i == 170), 90);
    assert_eq!(shop_rate(&s, |_| false, |i| i == 170), 70);
}

#[test]
fn buy_and_sell_prices() {
    assert_eq!(buy_price(301, 1000, 70, false), 700);
    assert_eq!(buy_price(301, 999, 95, false), 949);
    assert_eq!(buy_price(66, 2000, 100, true), 1000);
    assert_eq!(buy_price(66, 2000, 100, false), 2000);
    assert_eq!(buy_price(301, 1000, 100, true), 1000);
    // Sell: level percent and party factor, truncating.
    assert_eq!(sell_price(400, 1.0, 0, false), 400);
    assert_eq!(sell_price(400, 1.0, 3, false), 580);
    assert_eq!(sell_price(400, 1.0, 6, false), 1480);
    let one = sell_factor([true, false, false, false], 1.15, 1.0);
    assert_eq!(one, 1.15f32);
    // 1.15f32 is slightly below 1.15: 100 * 1.15f32 truncates to 114.
    assert_eq!(sell_price(100, one, 0, false), 114);
    assert_eq!(sell_price(128, one, 0, false), 147);
    assert_eq!(sell_price(100, 1.0, 0, true), 120);
    let two = sell_factor([true, true, false, false], 1.15, 1.0);
    assert_eq!(two, 1.15f32 * 1.15f32);
}

struct State {
    progress: i32,
    affinity: i32,
    items: Vec<(u32, u32)>,
    gsf: Vec<u32>,
}

impl JudgmentState for State {
    fn quest_flag(&self, quest: u32, flag: u32) -> bool {
        quest == 25 && flag == 0
    }
    fn gsf(&self, flag: u32) -> bool {
        self.gsf.contains(&flag)
    }
    fn progress(&self) -> i32 {
        self.progress
    }
    fn affinity(&self, _npc: i32) -> i32 {
        self.affinity
    }
    fn has_item(&self, item: u32, count: u32) -> bool {
        self.items.iter().any(|&(i, c)| i == item && c >= count)
    }
    fn party_value(&self, _slot: u32) -> u32 {
        0
    }
}

fn j(command: i32, p0: i32, p1: i32) -> Judgment {
    Judgment {
        command,
        params: [p0, p1, 0, 0],
    }
}

#[test]
fn judgment_lists() {
    let st = State {
        progress: 1200,
        affinity: 300,
        items: vec![(66, 2)],
        gsf: vec![7],
    };
    let none = [Judgment::default(); JUDGMENTS];
    assert!(conditions_open(&none, &none, 149, &st));
    let mut and = none;
    and[0] = j(11, 1000, 0);
    and[1] = j(16, 66, 2);
    assert!(conditions_open(&and, &none, 149, &st));
    and[2] = j(16, 66, 3);
    assert!(!conditions_open(&and, &none, 149, &st));
    // The OR list rescues a failed AND list.
    let mut or = none;
    or[3] = j(3, 7, 0);
    assert!(conditions_open(&and, &or, 149, &st));
    or[3] = j(4, 7, 0);
    assert!(!conditions_open(&and, &or, 149, &st));
    assert_eq!(judgment(&j(1, 25, 0), 1, &st), Some(true));
    assert_eq!(judgment(&j(2, 25, 0), 1, &st), Some(false));
    assert_eq!(judgment(&j(14, 0, 300), 1, &st), Some(true));
    assert_eq!(judgment(&j(14, 0, 300), -1, &st), Some(false));
    assert_eq!(judgment(&j(31, 66, 0), 1, &st), Some(false));
    assert_eq!(judgment(&j(99, 0, 0), 1, &st), Some(false));
    assert_eq!(judgment(&j(0, 0, 0), 1, &st), None);
}

#[test]
fn restock() {
    let mut s = StockSlot {
        item: 301,
        day: 0,
        rearrival: 2,
        now: 0,
        init: 2,
    };
    restock_tick(&mut s);
    assert_eq!((s.day, s.now), (1, 0));
    restock_tick(&mut s);
    assert_eq!((s.day, s.now), (0, 1));
    restock_tick(&mut s);
    restock_tick(&mut s);
    assert_eq!((s.day, s.now), (0, 2));
    restock_tick(&mut s);
    assert_eq!((s.day, s.now), (0, 2));
    adjust_stock(&mut s, -5);
    assert_eq!(s.now, 0);
    adjust_stock(&mut s, 2000);
    assert_eq!(s.now, 999);
    let mut unlimited = StockSlot {
        now: -1,
        init: -1,
        ..s
    };
    adjust_stock(&mut unlimited, -1);
    restock_tick(&mut unlimited);
    assert_eq!(unlimited.now, -1);
}

#[test]
fn enhancement_offers() {
    assert_eq!(instance_level(0), 0);
    assert_eq!(instance_level(0x8), 1);
    assert_eq!(instance_level(0x10), 2);
    assert_eq!(instance_level(0x40 | 0x8), 4);
    assert_eq!(instance_level(0x400), 6);
    assert_eq!(upgrade_offer(0, 1, false), Some(Currency::Gold));
    assert_eq!(upgrade_offer(0, 3, false), Some(Currency::Gold));
    assert_eq!(upgrade_offer(3, 4, true), None);
    assert_eq!(upgrade_offer(4, 5, false), None);
    assert_eq!(upgrade_offer(4, 5, true), Some(Currency::RiftCrystals));
    assert_eq!(upgrade_offer(4, 6, true), None);
    assert_eq!(upgrade_offer(5, 6, true), Some(Currency::RiftCrystals));
    assert_eq!(upgrade_offer(6, 7, true), None);
}

#[test]
fn inn() {
    assert_eq!(inn_price(6), 50);
    assert_eq!(inn_price(147), 500);
    assert_eq!(inn_price(478), 0);
}

fn header(magic: &[u8; 4], version: u32, count: u32) -> Vec<u8> {
    let mut v = magic.to_vec();
    v.extend(version.to_le_bytes());
    v.extend(count.to_le_bytes());
    v
}

#[test]
fn qlv_reader() {
    let mut d = header(b"qlv\0", 3, 1);
    d.extend(301u16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    d.extend(70u32.to_le_bytes());
    for (g, m, c) in [(220u32, 1416u16, 1u16), (690, 1227, 4), (4000, 1730, 3), (5000, 0xffff, 0)] {
        d.extend(g.to_le_bytes());
        d.extend(m.to_le_bytes());
        d.extend(c.to_le_bytes());
    }
    assert_eq!(d.len(), 12 + 40);
    let t = lvup::EquipLvUp::parse(&d).unwrap();
    let r = t.row(301).unwrap();
    assert_eq!(r.step(1).unwrap().gold, 70);
    assert_eq!(r.step(3).unwrap().material, Some(1227));
    assert_eq!(r.step(6).unwrap().material, None);
    assert!(lvup::EquipLvUp::parse(&d[..d.len() - 1]).is_err());
    let mut extra = d.clone();
    extra.push(0);
    assert!(lvup::EquipLvUp::parse(&extra).is_err());
    let mut bad = d;
    bad[4] = 2;
    assert!(lvup::EquipLvUp::parse(&bad).is_err());
}

#[test]
fn curse_reader_follows_loader() {
    let mut d = header(b"cit\0", 0, 1);
    d.extend(3u16.to_le_bytes());
    let mut items = [-1i16; 20];
    items[0] = 10;
    items[1] = 0x76d; // at or above 1901: the loader reads no rate for it
    items[2] = 11;
    for i in items {
        d.extend(i.to_le_bytes());
    }
    // Rates: slot 0, (slot 1 skipped), slot 2, then 17 empty slots.
    for r in [30i16, 70].into_iter().chain(std::iter::repeat_n(5, 17)) {
        d.extend(r.to_le_bytes());
    }
    let t = tables::parse_item_curse(&d).unwrap();
    assert_eq!(t[0].rates[0], 30);
    assert_eq!(t[0].rates[1], 0);
    assert_eq!(t[0].rates[2], 70);
    assert_eq!(t[0].rates[3], 0, "empty slot forced to 0");
    assert_eq!(t[0].total(), 100);
    assert_eq!(t[0].cumulative()[2], 100);
}

#[test]
fn itl_reader() {
    let mut d = b"ITL2".to_vec();
    d.extend(itl::STAMP.to_le_bytes());
    d.extend(2u32.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    for id in 0..2u32 {
        let mut r = [0u8; 128];
        r[0x3c..0x40].copy_from_slice(&id.to_le_bytes());
        r[0x48..0x4c].copy_from_slice(&(1000 + id).to_le_bytes());
        r[0x4c..0x50].copy_from_slice(&(400 + id).to_le_bytes());
        r[0x38..0x3c].copy_from_slice(&(7u32 | 5 << 5).to_le_bytes());
        d.extend(r);
    }
    let t = itl::ItemList::parse(&d).unwrap();
    assert_eq!(t.get(1).unwrap().buy_price(), 1001);
    assert_eq!(t.get(1).unwrap().sell_price(), 401);
    assert_eq!(t.get(0).unwrap().kind(), 7);
    assert_eq!(t.get(0).unwrap().category_type(), 5);
    assert!(itl::ItemList::parse(&d[..d.len() - 3]).is_err());
}
