use ddeconomy::inventory::{Inventory, Owner, Slot};
use ddeconomy::item_use::{Recovery, UseError};
use dditem::itl::{ItemList, ItemRecord};

fn item(hp: i16, stamina: i16) -> ItemRecord {
    let mut raw = [0; 128];
    raw[0x70..0x72].copy_from_slice(&hp.to_le_bytes());
    raw[0x72..0x74].copy_from_slice(&stamina.to_le_bytes());
    ItemRecord { raw }
}

#[test]
fn owner_isolation_and_failed_use_is_atomic() {
    let items = ItemList {
        records: vec![item(123, 45)],
    };
    let mut inventory = Inventory::default();
    for owner in [Owner::Arisen(0), Owner::Arisen(1), Owner::Pawn(0)] {
        inventory.bag_mut(owner).slots.push(Slot {
            item: 0,
            num: 2,
            flags: 1,
        });
    }
    assert_eq!(
        inventory.use_recovery(&items, Owner::Arisen(1), 0, |_| Err::<(), _>(
            UseError::Unsupported
        )),
        Err(UseError::Unsupported)
    );
    let effect = inventory.use_recovery(&items, Owner::Arisen(1), 0, Ok).unwrap();
    assert_eq!((effect.hp, effect.stamina), (123.0, 45.0));
    assert_eq!(inventory.bag(Owner::Arisen(1)).unwrap().count(0), 1);
    assert_eq!(inventory.bag(Owner::Arisen(0)).unwrap().count(0), 2);
    assert_eq!(inventory.bag(Owner::Pawn(0)).unwrap().count(0), 2);
    assert_eq!(
        inventory.recovery(&items, Owner::Storage(1), 0),
        Err(UseError::Storage)
    );
}

#[test]
fn cure_bytes_are_signed_and_sharing_does_not_cure() {
    let mut record = item(100, 80);
    record.raw[0x25] = 1;
    record.raw[0x26] = 255;
    record.raw[0x29] = 1;
    record.raw[0x32] = 1;
    let slot = Slot {
        item: 0,
        num: 1,
        flags: 1,
    };
    let effect = Recovery::from_record(&record, slot).unwrap();
    assert_eq!(effect.cures, (1 << 0) | (1 << 6) | (1 << 16));
    let shared = effect.for_recipient(1.5, true);
    assert_eq!((shared.hp, shared.stamina, shared.cures), (37.5, 30.0, 0));
    record.raw[0x25] = 2;
    assert_eq!(Recovery::from_record(&record, slot), Err(UseError::Unsupported));
}

#[test]
fn installed_recovery_items() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        eprintln!("skipped: no install");
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let items = ItemList::parse(
        &vfs.read(r"etc\item\itemList", mtarc::type_hash("rItemList"))
            .unwrap(),
    )
    .unwrap();
    let mut supported = 0;
    for (id, record) in items.records.iter().enumerate() {
        if let Ok(effect) = Recovery::from_record(
            record,
            Slot {
                item: id as i16,
                num: 1,
                flags: 1,
            },
        ) {
            supported += 1;
            assert!(effect.hp.is_finite() && effect.stamina.is_finite());
        }
    }
    assert!(supported > 0);
    let effect = |id| {
        Recovery::from_record(
            items.get(id).unwrap(),
            Slot {
                item: id as i16,
                num: 1,
                flags: 1,
            },
        )
        .unwrap()
    };
    // Named by the installed itemName_eng; values read from ITL, not
    // supplied by runtime constants. Regression examples for the brief.
    assert_eq!(effect(99).hp, 1500.0); // Salubrious Brew
    assert_eq!(effect(43).hp, 120.0); // Greenwarish
    assert_eq!(effect(45).stamina, 250.0); // Mushroom
    assert_eq!(effect(48).cures, 1); // Mithridate: poison only
}
