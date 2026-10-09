//! Story recruitment is host-owned, persists, and respects co-op seats.
use ddhost::{Control, Host, Member, ProfileId};
use ddstory::{data::InstallData, scripted::ScriptedHost, Story};

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

#[test]
fn rook_record_is_shared_protected_and_saved_without_a_rift_visit() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let story = Story::new_game(InstallData::open(native, None).unwrap());
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        ScriptedHost::default(),
        Box::new(Void),
    );
    let owner = host.join(ProfileId(17), Control::Local, [0.0; 3], 0.0).unwrap();
    let guest = host.join(ProfileId(29), Control::Remote, [0.0; 3], 0.0).unwrap();
    assert!(host.recruit_story_pawn(14, 1).unwrap());
    let rook = host.rift.party.hired[0].as_ref().unwrap();
    assert_eq!(rook.pawn.rom_id, 0x2000_000e);
    assert_eq!(rook.pawn.pawn_type, 2);
    assert_eq!(rook.pawn.param.level, 6);
    assert_eq!(host.party.slots[2], Some(Member::Hired { owner, slot: 1 }));
    assert_eq!(host.party.slots[3], Some(Member::Arisen(guest)));
    let bag = host
        .economy
        .inventory
        .bag(ddeconomy::inventory::Owner::Pawn(1))
        .unwrap()
        .clone();
    assert_eq!(bag.count(64), 1);
    for item in [414, 575, 634, 985, 1062] {
        assert_eq!(bag.count(item), 1);
        assert!(bag.slots.iter().find(|s| s.item == item).unwrap().flags & 2 != 0);
    }
    let saved = host.rift.save();
    assert_eq!(ddrift::Rift::load(&saved).unwrap().party, host.rift.party);
    host.frame += 100;
    assert!(!host.recruit_story_pawn(14, 1).unwrap());
    assert_eq!(
        host.rift.save(),
        saved,
        "duplicate request cannot reset the record"
    );
    assert_eq!(
        host.economy.inventory.bag(ddeconomy::inventory::Owner::Pawn(1)),
        Some(&bag)
    );
    assert!(
        host.recruit_story_pawn(15, 2).is_err(),
        "guest reservation is authoritative"
    );
    assert!(
        host.recruit_story_pawn(15, 1).is_err(),
        "occupied story slot cannot be overwritten"
    );
    assert!(
        host.rift.release(1, 100, true).is_none(),
        "pawn type 2 cannot be lost through death"
    );
}
