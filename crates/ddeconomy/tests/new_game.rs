use ddeconomy::{inventory::{Inventory, Owner, FLAG_IN_USE}, new_game::{self, Grants, Phase}};
use ddstory::data::{Data, InstallData};

fn native() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    path.is_dir().then_some(path)
}

#[test]
fn post_prologue_bags_exclude_temporary_hero_potions() {
    let challenge = new_game::bag(Phase::DragonChallenge);
    assert_eq!(challenge.slots.iter().map(|s| (s.item, s.num)).collect::<Vec<_>>(), [(301, 1), (1184, 1)]);
    let arisen = new_game::bag(Phase::NewlyArisen);
    assert_eq!(arisen.slots.iter().map(|s| (s.item, s.num)).collect::<Vec<_>>(), [(1185, 1)]);
    for bag in [challenge, arisen] {
        assert_eq!(bag.count(43), 0);
        assert_eq!(bag.count(44), 0);
        assert!(bag.slots.iter().all(|s| s.flags == 3));
    }
}

#[test]
fn installed_story_rewards_arrive_after_clear_and_are_independent_per_seat() {
    let Some(native) = native() else { return };
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let table = dditem::itl::ItemList::parse(&vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList")).unwrap()).unwrap();
    let data = InstallData::open(&native, None).unwrap();
    let mut inventory = Inventory::default();
    let mut deliveries = Grants::default();
    for seat in [0, 1] { inventory.bags.insert(Owner::Arisen(seat), new_game::bag(Phase::DragonChallenge)); }
    // A collected restorative must survive the heart-taking gear swap.
    inventory.add(&table, Owner::Arisen(0), 0, 43, 1, 0, &mut Default::default());
    new_game::heart_taken(&mut inventory, &table, 0);
    let bag = inventory.bag(Owner::Arisen(0)).unwrap();
    assert_eq!(bag.count(301), 0);
    assert_eq!(bag.count(1184), 0);
    assert_eq!(bag.count(1185), 1);
    assert_eq!(bag.count(43), 1);
    assert_eq!(inventory.bag(Owner::Arisen(1)).unwrap().count(301), 1);
    for request in ddstory::orders::reward(&data, 0).unwrap() {
        if let ddstory::Request::Reward { items, .. } = request { assert!(items.is_empty()); }
    }
    // Read the actual resource; do not put its bulk reward rows in code.
    let requests = ddstory::orders::reward(&data, 1).unwrap();
    assert!(data.qct(r"etc\questCtrl\q0001_b00").unwrap().unwrap().tables[0].rows[2].results.iter().any(|r| r.0[0] == 16));
    let ddstory::Request::Reward { quest, items, .. } = &requests[0] else { panic!("missing q1 reward") };
    assert_eq!(items, &[(64, 1), (1, 5), (43, 5)]);
    for seat in [0, 1] {
        deliveries.reward(&mut inventory, &table, seat, *quest, items);
        deliveries.reward(&mut inventory, &table, seat, *quest, items);
    }
    assert_eq!(inventory.bag(Owner::Arisen(0)).unwrap().count(43), 6);
    assert_eq!(inventory.bag(Owner::Arisen(1)).unwrap().count(43), 5);
    deliveries.first_choice(&mut inventory, &table, 0, &[0x195, 0x1f8, 0x227, 0x228]);
    deliveries.first_choice(&mut inventory, &table, 0, &[0x195, 0x1f8, 0x227, 0x228]);
    let bag = inventory.bag(Owner::Arisen(0)).unwrap();
    assert_eq!(bag.count(0x195), 1);
    assert_eq!(bag.slots.iter().find(|s| s.item == 1185).unwrap().flags & FLAG_IN_USE, 0);
    assert!(bag.slots.iter().filter(|s| [0x195, 0x1f8, 0x227, 0x228].contains(&s.item)).all(|s| s.flags & FLAG_IN_USE != 0));
    assert_eq!(inventory.bag(Owner::Arisen(1)).unwrap().count(0x195), 0);
}
