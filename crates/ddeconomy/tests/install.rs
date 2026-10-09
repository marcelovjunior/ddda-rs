//! Sessions against the shipped data (skips without an install; set
//! DDDA_NATIVE_PC for another one).
use ddeconomy::inventory::{BuyBack, Owner};
use ddeconomy::*;
use dditem::rules::Currency;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn catalog() -> Option<Catalog> {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !root.is_dir() {
        assert!(explicit.is_none(), "DDDA_NATIVE_PC does not exist");
        eprintln!("skipped: no install");
        return None;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let read = |c: &str, p: &str| vfs.read(p, mtarc::type_hash(c)).unwrap();
    let items = dditem::itl::ItemList::parse(&read("rItemList", r"etc\item\itemList")).unwrap();
    let lvup = dditem::lvup::EquipLvUp::parse(&read("rEquipLvUp", r"etc\item\equipLvUp")).unwrap();
    // Shop lists as uNpc::vf32 (0x00bace50) picks them.
    let mut shops = BTreeMap::new();
    let h = mtarc::type_hash("rShopList");
    for p in vfs.paths_of_type(h) {
        let no: u32 = p.rsplit('\\').next().unwrap()[1..4].parse().unwrap();
        let dl1 = p.starts_with("dl1");
        if (no == 784 || no == 156) != dl1 {
            continue;
        }
        shops.insert(no, dditem::shop::ShopList::parse(&vfs.read(p, h).unwrap()).unwrap());
    }
    // plAbilityParam record 0x54 (object 0x55 after the root).
    let x = mtxfs::parse(&read("rPlAbilityParam", r"param\pl\other\plAbilityParam")).unwrap();
    let objs = x.walk();
    let rec = objs[1 + 0x54];
    let f = |n: &str| match x.get(rec, n).unwrap()[0] {
        mtxfs::Value::F(ref v) => v[0] as f32,
        _ => panic!(),
    };
    Some(Catalog {
        items,
        shops,
        lvup,
        suasion: (f("mFVal0"), f("mFVal1")),
    })
}

fn party(p: usize) -> Party {
    Party {
        members: vec![Member {
            owner: Owner::Arisen(p),
            present: true,
            level: 1,
            augments: [u32::MAX; 6],
        }],
    }
}

#[test]
fn caxton_buy_sell_enhance_and_madeleine_restock() {
    let Some(cat) = catalog() else { return };
    assert_eq!(cat.shops.len(), 16);
    assert_eq!(cat.suasion, (1.15, 1.0));
    let mut w = ddstory::World::default();
    w.set_sce_no(8000, false);
    let pt = party(0);
    let mut e = Economy::new(w.day);
    e.wallet(0).gold = 100_000;

    // Caxton's Armory (149): Trusty Sword 2,700 (data), 70% with a Gold Idol.
    let s = e.open_shop(&cat, &w, &pt, 0, 149).unwrap();
    let price = |e: &mut Economy, item| {
        e.offers(&cat, &w, &pt, &s).unwrap().into_iter().find(|o| o.item == item).unwrap().unit_price
    };
    assert_eq!(price(&mut e, 307), 2700);
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, 170, 1, 0, &mut BuyBack::default());
    assert_eq!(price(&mut e, 307), 1890);
    assert_eq!(e.buy(&cat, &w, &pt, &s, 307, 1, Owner::Arisen(0)), Ok(1890));
    // Sell the Gold Idol: 30,000.
    let idol = e.inventory.bags[&Owner::Arisen(0)].slots.iter().position(|x| x.item == 170).unwrap();
    assert_eq!(e.sell(&cat, &pt, &s, Owner::Arisen(0), idol, 1), Ok(30_000));
    assert_eq!(e.wallets[&0].gold, 100_000 - 1890 + 30_000);
    e.close(s);

    // Enhance an Iron Sword to level 1 at Caxton: 70 gold.
    e.inventory.add(&cat.items, Owner::Arisen(0), 0, 301, 1, 0, &mut BuyBack::default());
    let sword = e.inventory.bags[&Owner::Arisen(0)].slots.iter().position(|x| x.item == 301).unwrap();
    let up = e.open_upgrade(0, 149, false).unwrap();
    assert_eq!(e.enhance(&cat, &pt, &up, Owner::Arisen(0), sword, 1), Ok((Currency::Gold, 70, None)));
    e.close(up);

    // Madeleine (7): Desiccated Herbs, 3 in stock, one more every 5 days,
    // open with quest 80 flag 18 (and her shop's own conditions).
    w.set_quest_bit(89, 2, true);
    w.set_quest_bit(80, 18, true);
    let s = e.open_shop(&cat, &w, &pt, 0, 7).unwrap();
    let herbs = |e: &mut Economy| e.offers(&cat, &w, &pt, &s).unwrap().into_iter().find(|o| o.item == 1339).map(|o| o.stock);
    assert_eq!(herbs(&mut e), Some(3));
    e.buy(&cat, &w, &pt, &s, 1339, 3, Owner::Arisen(0)).unwrap();
    assert_eq!(herbs(&mut e), Some(0));
    for day in 1..=5 {
        e.update(day, false);
    }
    assert_eq!(herbs(&mut e), Some(1));
}
