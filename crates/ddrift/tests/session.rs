//! Full Rift sessions against the install (and a private save copy when
//! present): load, list, summon, hire at the right cost, dismiss; single
//! player and two players. Read-only; skips without the install.

use ddpawn::rift::XorShift128;
use ddpawn::RomPawnId;
use ddrift::*;
use std::collections::HashSet;
use std::path::PathBuf;

fn data() -> Option<RiftData> {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !root.is_dir() {
        eprintln!("skipped: no install");
        return None;
    }
    let exe = ddexe::locate(&root).ok()?;
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let ledger_bytes = vfs.read("etc/item/npclist", mtarc::type_hash("rNpcLedgerList")).unwrap();
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).unwrap();
    let npc = ledger.entries.iter().map(|e| (e.id as u32, e.name_j.to_vec())).collect();
    Some(RiftData::load(&vfs, &std::fs::read(exe).unwrap(), npc, 1).unwrap())
}

fn save_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DDDA_SAVE") {
        return Some(PathBuf::from(p));
    }
    let mut dirs: Vec<_> =
        std::fs::read_dir(r"E:\ddda-re\save-backups").ok()?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    dirs.sort();
    dirs.into_iter().rev().map(|d| d.join(r"367500\remote\DDDA.sav")).find(|p| p.is_file())
}

fn level(id: u32) -> u8 {
    match RomPawnId::decode(id) {
        RomPawnId::Nora { level, .. } => level,
        other => panic!("{other:?}"),
    }
}

/// The pool the original held after a load (oracle, board #358): player
/// level 7, offline, with the save's favourite 0x1004005E placed first.
/// Levels only (the RNG state is the session's).
const LIVE_LEVELS: [(u8, usize); 18] = [
    (1, 2),
    (3, 2),
    (4, 1),
    (5, 2),
    (7, 27),
    (9, 16),
    (11, 16),
    (13, 10),
    (15, 6),
    (17, 6),
    (19, 2),
    (23, 2),
    (27, 2),
    (37, 2),
    (47, 1),
    (57, 1),
    (67, 1),
    (87, 1),
];

#[test]
fn single_player_session_from_the_save() {
    let Some(data) = data() else { return };
    let Some(save) = save_path() else {
        eprintln!("skipped: no save copy");
        return;
    };
    let save = ddsave::SaveFile::parse(&std::fs::read(save).unwrap()).unwrap();
    let doc = save.document().unwrap();
    let player = doc.root().child("mPlayerDataManual").unwrap();
    let log = PawnLog::from_save(player).unwrap();
    let mut rift = Rift::new(1, log);
    // The party as saved: main pawn and the hired pawns that are present.
    let present: Vec<bool> = player.child("mPlGameData").unwrap().array("mCmcBeFlag").unwrap();
    for (i, n) in player.path("mPlCmcEditAndParam/mCmc").unwrap().children().enumerate() {
        if !present[i] {
            continue;
        }
        let pawn = ddpawn::cmc::Cmc::from_save(n).unwrap();
        let online = PawnRef::from_save_info(n.child("mOnlinePawnInfo").unwrap()).unwrap();
        let p = PartyPawn { pawn, online, hire_time: 0, quests_cleared: 0 };
        if i == 0 {
            rift.party.main_pawn = Some(p);
        } else {
            rift.party.hired[i - 1] = Some(p);
        }
    }
    let arisen_level: u32 = player.path("mPlCmcEditAndParam/mPl/mParam").unwrap().get("mLevel").unwrap();
    let rc: i32 = player.path("mPlCmcEditAndParam/mPl/mParam").unwrap().get("OnlinePoint").unwrap();
    let mut p1 = Arisen { id: 0, level: arisen_level, rift_crystals: rc };
    assert_eq!((p1.level, p1.rift_crystals), (7, 114), "the save the live read was taken from");

    // Load: the pool equals the live pool's level multiset.
    let mut rng = XorShift128::seeded(0x1105);
    rift.refill(&data, p1.level, 1, &mut rng).unwrap();
    let ids: Vec<u32> = rift.pool().iter().map(|c| c.rom_id).collect();
    assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), 100);
    assert_eq!(ids[0], 0x1004_005e, "favourite with a built-in ID first; the hired favourite is skipped");
    let mut counts = std::collections::BTreeMap::<u8, usize>::new();
    for &id in &ids {
        *counts.entry(level(id)).or_default() += 1;
    }
    assert_eq!(counts.into_iter().collect::<Vec<_>>(), LIVE_LEVELS.to_vec());
    for (i, c) in rift.stock[..20].iter().enumerate() {
        assert_eq!(c.rom_id, 0x2000_0000 + i as u32);
    }
    assert_eq!(rift.stock[14].edit.name_str(), "ルーク");

    // Lists.
    let world = ddstory::World::default();
    rift.open_room();
    let mut rows = rift.list(&data, &world, Category::ByLevel(10), p1.level, &mut rng).unwrap();
    assert_eq!(rows.len(), 100);
    assert!(rows.iter().all(|r| r.pawn.param.level == 10 && r.flags == 0));
    assert_eq!(rows.iter().map(|r| r.online.rom_id).collect::<HashSet<_>>().len(), 100);
    assert!(rows.iter().all(|r| rift.cost(&p1, &r.pawn) == 171));
    let past = rift.list(&data, &world, Category::PastSummons, p1.level, &mut rng).unwrap();
    assert!(!past.is_empty() && past.iter().all(|r| r.online.rom_id != u32::MAX));
    assert_eq!(
        rift.list(&data, &world, Category::MainPawnKnows, p1.level, &mut rng),
        Err(ListError::NeedsNetwork)
    );

    // Summon five, the sixth is refused.
    for r in rows.iter_mut().take(5) {
        rift.summon(0, r).unwrap();
    }
    assert_eq!(rift.summon(0, &mut rows[5]), Err(SummonError::RoomFull));
    // A pawn standing in the room is flagged in later lists (0x800).
    let summoned = rows[0].online.rom_id;
    rift.log.favorites.push(PawnRef::rom(summoned));
    let favs = rift.list(&data, &world, Category::Favorites, p1.level, &mut rng).unwrap();
    assert_eq!(favs.iter().find(|r| r.online.rom_id == summoned).unwrap().flags, flags::IN_RIFT);
    rift.log.favorites.pop();

    // Hire: 171 > 114 crystals.
    assert_eq!(rift.hire(&mut p1, 0, 50_000), Err(HireError::NotEnoughCrystals { cost: 171 }));
    p1.rift_crystals = 1000;
    // Both hired slots are taken in this save (Eloyse and Ro).
    assert_eq!(rift.hire(&mut p1, 0, 50_000), Err(HireError::PartyFull));
    // Dismiss Eloyse (slot 1) after an hour: the log takes her.
    let past_before = rift.log.past_summons.len();
    let history_before = rift.log.history.len();
    let out = rift.release(1, 3600, false).unwrap();
    assert_eq!(out.rom_id, 0x1004_027a);
    assert!(rift.log.past_summons.len() <= past_before + 1);
    assert_eq!(rift.log.past_summons.last(), Some(&out));
    assert_eq!(rift.log.history.len(), history_before + 1);
    assert_eq!(rift.log.history.last().unwrap().online, out);
    // Eloyse is a favourite, so the favourite holds her totals.
    let fav = rift.log.favorites.iter().find(|f| f.rom_id == 0x1004_027a).unwrap();
    assert_eq!(fav, &out);
    assert_eq!(out.employ, 1 + 0, "first recorded departure (saved favourite had no counters)");
    // The freed slot 1 goes to the next hire, at the listed cost.
    assert_eq!(rift.hire(&mut p1, 0, 50_000), Ok(1));
    assert_eq!(p1.rift_crystals, 829);
    // The hired pawn shows as in the rift in a new list of the same pawns.
    let favs = rift.list(&data, &world, Category::PastSummons, p1.level, &mut rng).unwrap();
    let eloyse = favs.iter().find(|r| r.online.rom_id == 0x1004_027a).unwrap();
    assert_eq!(eloyse.flags, 0, "released pawns can be summoned again");
    rift.close_room();
}

#[test]
fn two_player_session() {
    let Some(data) = data() else { return };
    let mut rift = Rift::new(2, PawnLog::default());
    let mut rng = XorShift128::seeded(7);
    rift.refill(&data, 30, 1, &mut rng).unwrap();
    let world = ddstory::World::default();
    let mut host = Arisen { id: 0, level: 30, rift_crystals: 5000 };
    let mut guest = Arisen { id: 1, level: 12, rift_crystals: 20_000 };
    rift.open_room();
    // Each player lists at their own level; costs are per viewer.
    let mut host_rows = rift.list(&data, &world, Category::ByLevel(31), host.level, &mut rng).unwrap();
    let mut guest_rows = rift.list(&data, &world, Category::ByLevel(31), guest.level, &mut rng).unwrap();
    assert_eq!(rift.cost(&host, &host_rows[0].pawn), (61 + 18));
    assert_eq!(rift.cost(&guest, &guest_rows[0].pawn), 43 * 19 * 19 + 18);
    let h = rift.summon(0, &mut host_rows[0]).unwrap();
    let g = rift.summon(1, &mut guest_rows[0]).unwrap();
    // The guest talks to the host's pawn: the host cannot hire it meanwhile.
    rift.talk(1, h).unwrap();
    assert_eq!(rift.hire(&mut host, h, 0), Err(HireError::Locked(1)));
    rift.stop_talking(1, h);
    // One hired slot in co-op: the first hire wins, paid by its Arisen.
    assert_eq!(rift.hire(&mut guest, g, 0), Ok(1));
    assert_eq!(guest.rift_crystals, 20_000 - (43 * 19 * 19 + 18));
    assert_eq!(rift.hire(&mut host, h, 0), Err(HireError::PartyFull));
    assert_eq!(host.rift_crystals, 5000);
    // Dismissal frees the slot; the host then hires.
    rift.release(1, 1800, false).unwrap();
    assert_eq!(rift.hire(&mut host, h, 1800), Ok(1));
    assert_eq!(host.rift_crystals, 5000 - 79);
    assert!(rift.party.hired[1].is_none(), "slot 2 belongs to the second Arisen");
}

#[test]
fn knowledge_sort_with_an_active_quest() {
    let Some(data) = data() else { return };
    let rift = Rift::new(1, PawnLog::default());
    let mut world = ddstory::World::default();
    let (quest, _) = data.quest_knowledge[0];
    world.active_quest = quest as i32;
    let mut rng = XorShift128::seeded(3);
    let rows = rift.list(&data, &world, Category::ByLevel(20), 20, &mut rng).unwrap();
    let knows: Vec<bool> = rows.iter().map(|r| data.knows_quest(&r.pawn.study_flag, quest)).collect();
    let first_unknowing = knows.iter().position(|k| !k).unwrap_or(knows.len());
    assert!(knows[first_unknowing..].iter().all(|k| !k), "knowing rows first");
    eprintln!("quest {quest}: {first_unknowing} of 100 pawns know it");
}
