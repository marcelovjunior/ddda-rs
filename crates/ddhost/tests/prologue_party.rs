use ddhost::prologue::for_roster;
use ddhost::{Control, ProfileId, Roster};
use ddpawn::cmc::Cmc;
use ddstory::prologue::PartySetup;

fn original() -> PartySetup {
    let mut stock = vec![Cmc::initial(); 14];
    for (index, record) in stock.iter_mut().enumerate() {
        record.rom_id = 0x2000_0000 + index as u32;
    }
    ddstory::prologue::party_from_stock(&stock, &|id| Some(format!("name{id}").into_bytes()), 0).unwrap()
}

#[test]
fn original_single_seat_has_all_three_shared_companions() {
    let mut roster = Roster::default();
    roster.join(ProfileId(31), Control::Local).unwrap();
    let setup = for_roster(&roster, &original()).unwrap();
    assert_eq!(setup.heroes.len(), 1);
    assert_eq!(setup.pawns.main_pawn.unwrap().pawn.rom_id, 0x2000_000b);
    assert_eq!(setup.pawns.hired[0].as_ref().unwrap().pawn.rom_id, 0x2000_000c);
    assert_eq!(setup.pawns.hired[1].as_ref().unwrap().pawn.rom_id, 0x2000_000d);
    assert_eq!(setup.pawns.reserved, [false, false]);
}

#[test]
fn coop_uses_independent_hero_records_and_one_shared_pawn_party() {
    let mut roster = Roster::default();
    let host = roster.join(ProfileId(31), Control::Local).unwrap();
    let guest = roster.join(ProfileId(54), Control::Remote).unwrap();
    let mut setup = for_roster(&roster, &original()).unwrap();
    assert_eq!(setup.heroes.len(), 2);
    assert_eq!(setup.heroes[&host], setup.heroes[&guest]);
    setup.heroes.get_mut(&guest).unwrap().record.param.hp = 3.0;
    assert_ne!(
        setup.heroes[&host].record.param.hp,
        setup.heroes[&guest].record.param.hp
    );
    assert_eq!(setup.pawns.reserved, [false, true]);
    assert!(setup.pawns.main_pawn.is_some());
    assert!(setup.pawns.hired[0].is_some());
    assert!(setup.pawns.hired[1].is_none());
    assert!(for_roster(&Roster::default(), &original()).is_err());
}

#[test]
fn new_game_host_installs_records_before_ticking_and_refuses_a_reset() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    struct Ground;
    impl ddsim::Ground for Ground {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }
    let story = ddstory::Story::new_game(ddstory::data::InstallData::open(&root, None).unwrap());
    let mut host = ddhost::Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        ddstory::scripted::ScriptedHost::default(),
        Box::new(Ground),
    );
    assert!(host.prepare_prologue(1, 0).is_err(), "seat Arisen first");
    let p1 = host
        .join(ProfileId(100), Control::Local, [1.0, 2.0, 3.0], 0.0)
        .unwrap();
    let p2 = host
        .join(ProfileId(200), Control::Remote, [4.0, 5.0, 6.0], 0.0)
        .unwrap();
    let setup = host.prepare_prologue(1, 0).unwrap();
    assert_eq!(setup.heroes.len(), 2);
    assert_eq!(host.economy.wallet(p1).gold, 38_000);
    assert_eq!(host.economy.wallet(p2).rift_crystals, 785);
    assert_eq!(host.rift.party, setup.pawns);
    assert_eq!(host.party.slots[0], Some(ddhost::Member::Arisen(p1)));
    assert_eq!(host.party.slots[3], Some(ddhost::Member::Arisen(p2)));
    assert_eq!(host.frame, 0, "setup must not advance the story or combat");
    assert_eq!(host.story.as_ref().unwrap().world.sce_no, 100);
    assert_eq!(
        host.story.as_ref().unwrap().world.player_job,
        u32::from(setup.heroes[&p1].record.param.job)
    );
    assert_eq!(host.sim.player(p2).unwrap().position, [4.0, 5.0, 6.0]);
    host.economy.wallet(p1).gold = 42;
    assert!(host.prepare_prologue(1, 100).is_err());
    assert_eq!(
        host.economy.wallet(p1).gold,
        42,
        "refused replay must not reset wallets"
    );
}
