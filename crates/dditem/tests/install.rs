//! Optional read-only tests against the player's install. Set
//! DDDA_NATIVE_PC to test another install; they skip when the default path
//! is absent.
use dditem::*;
use std::path::PathBuf;

fn vfs() -> Option<ddvfs::Vfs> {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !root.is_dir() {
        assert!(explicit.is_none(), "DDDA_NATIVE_PC does not exist: {}", root.display());
        eprintln!("skipped: no DDDA install at {}", root.display());
        return None;
    }
    Some(ddvfs::Vfs::open(root).unwrap())
}

fn read(vfs: &ddvfs::Vfs, class: &str, path: &str) -> Vec<u8> {
    vfs.read(path, mtarc::type_hash(class)).unwrap()
}

#[test]
fn item_list_and_prices() {
    let Some(vfs) = vfs() else { return };
    let items = itl::ItemList::parse(&read(&vfs, "rItemList", r"etc\item\itemList")).unwrap();
    assert_eq!(items.records.len(), 1901);
    for (i, r) in items.records.iter().enumerate() {
        assert_eq!(r.id() as usize, i);
    }
    let names_bytes = read(&vfs, "rGUIMessage", r"id\message\item\itemName_eng");
    let names = mtgmd::MessageFile::parse(&names_bytes).unwrap();
    let check = |id: u32, name: &str, buy: u32, sell: u32| {
        assert_eq!(names.message(id), Some(name));
        let r = items.get(id).unwrap();
        assert_eq!((r.buy_price(), r.sell_price()), (buy, sell), "{name}");
    };
    check(0, "Airtight Flask", 320, 128);
    check(301, "Iron Sword", 1000, 400);
    check(170, "Gold Idol", 125000, 30000);
    check(66, "Ferrystone", 2000, 200);
    let sword = items.get(301).unwrap();
    assert_eq!(sword.kind(), 7);
    assert_eq!(sword.level_up_type(), 1);
}

#[test]
fn enhancement_costs() {
    let Some(vfs) = vfs() else { return };
    let t = lvup::EquipLvUp::parse(&read(&vfs, "rEquipLvUp", r"etc\item\equipLvUp")).unwrap();
    assert_eq!(t.rows.len(), 900);
    assert!(t.rows.iter().all(|r| r.unnamed == 0));
    assert!(t
        .rows
        .iter()
        .flat_map(|r| r.steps.iter())
        .all(|s| s.material.is_none_or(|m| m < 1901)));
    // Iron Sword: 70G; 220G + Cassardite; 690G + 4 Hunks of Ore; Lv5/Lv6
    // (paid in Rift Crystals) 4000 + 3 Lava Rock, 5000 + 5 Cyclops Fang.
    let r = t.row(301).unwrap();
    let s = |l| *r.step(l).unwrap();
    assert_eq!((s(1).gold, s(1).material), (70, None));
    assert_eq!((s(2).gold, s(2).material, s(2).material_count), (220, Some(1416), 1));
    assert_eq!((s(3).gold, s(3).material, s(3).material_count), (690, Some(1227), 4));
    assert_eq!((s(5).gold, s(5).material, s(5).material_count), (4000, Some(1730), 3));
    assert_eq!((s(6).gold, s(6).material, s(6).material_count), (5000, Some(1770), 5));
    assert!(r.step(4).is_none());
}

#[test]
fn level_tables_and_small_tables() {
    let Some(vfs) = vfs() else { return };
    for (path, rows) in [
        (r"etc\item\LvParamWepon", 260),
        (r"etc\item\LvParamArmor", 423),
        (r"etc\item\LvParamAccessory", 43),
    ] {
        let t = lvparam::ItemLevelParam::parse(&read(&vfs, "rItemLevelParam", path)).unwrap();
        assert_eq!(t.rows.len(), rows, "{path}");
    }
    let mix = tables::parse_item_mix(&read(&vfs, "rItemMix", r"etc\item\itemMix")).unwrap();
    assert_eq!(mix.len(), 435);
    let sets = tables::parse_item_set(&read(&vfs, "rItemSetTbl", r"etc\item\itemSetTbl")).unwrap();
    assert_eq!(sets.len(), 2800);
    let em = tables::parse_item_set(&read(&vfs, "rItemSetTbl", r"etc\item\ItemEmListSetTbl")).unwrap();
    assert_eq!(em.len(), 1116);
    let curse = tables::parse_item_curse(&read(&vfs, "rItemCurseCnv", r"etc\item\itemCurse")).unwrap();
    assert_eq!(curse.len(), 30);
    let rand = tables::parse_item_rand(&read(&vfs, "rItemRandParam", r"etc\item\itemRandParam")).unwrap();
    assert_eq!(rand.len(), 24);
}

#[test]
fn every_shop_list() {
    let Some(vfs) = vfs() else { return };
    let npcs = npc::parse_npc_ledger(&read(&vfs, "rNpcLedgerList", r"etc\item\NpcList")).unwrap();
    assert_eq!(npcs.len(), 727);
    let h = mtarc::type_hash("rShopList");
    let mut paths: Vec<_> = vfs.paths_of_type(h).collect();
    paths.sort();
    assert_eq!(paths.len(), 18);
    let mut lineup = 0;
    for p in &paths {
        for loc in vfs.locate(p, h).unwrap() {
            let s = shop::ShopList::parse(&vfs.read_at(loc).unwrap()).unwrap();
            assert!(s.lineup.iter().all(|l| l.item < 1901), "{p}");
            lineup += s.lineup.len();
        }
        // Every shop file names a ledger NPC: etc\shop\n%03dShopList.
        let no: u16 = p.rsplit('\\').next().unwrap()[1..4].parse().unwrap();
        assert!(npcs.iter().any(|n| n.no == no), "{p}");
    }
    assert!(lineup > 0);
    let caxton = shop::ShopList::parse(&read(&vfs, "rShopList", r"etc\shop\n149ShopList")).unwrap();
    assert_eq!(npcs.iter().find(|n| n.no == 149).unwrap().name, "Caxton ");
    assert_eq!(caxton.shop_type, 1);
    assert_eq!(caxton.discount_ok_item, [170, 279, 198, 280, 199, 281, 218, 152]);
    assert_eq!(caxton.discount_rate, [70, 70, 80, 80, 90, 90, 92, 95]);
    assert_eq!(caxton.default_rate, 100);
    let fournival = shop::ShopList::parse(&read(&vfs, "rShopList", r"etc\shop\n133ShopList")).unwrap();
    assert_eq!((fournival.discount_gsf, fournival.discount_gsf_rate), (0x20d, 90));
}
