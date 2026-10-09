//! Read-only tests against the install. Set DDDA_NATIVE_PC for another one.

use ddpawn::rift::{self, PawnRng, XorShift128, POOL_LEVEL_OFFSETS, POOL_SLOTS};
use ddpawn::{nora_path, NoraFile, PawnNames, RomPawnId, RomPawnInfo, NORA_FILES, NORA_LEVEL_BANDS};
use std::collections::{BTreeMap, HashSet};
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

fn load(vfs: &ddvfs::Vfs) -> (Vec<NoraFile>, RomPawnInfo, PawnNames) {
    let nora = (0..NORA_FILES)
        .map(|i| NoraFile::parse(&vfs.read(&nora_path(i), mtarc::type_hash("rRomNoraPawn")).unwrap()).unwrap())
        .collect();
    let info = RomPawnInfo::parse(
        &vfs.read(r"etc\editPattern\romPawnInfo", mtarc::type_hash("rRomPawnInfo")).unwrap(),
    )
    .unwrap();
    let names = PawnNames::parse(
        &vfs.read(r"etc\editPattern\romPawnNameAlphabet", mtarc::type_hash("rRomPawnName")).unwrap(),
    )
    .unwrap();
    (nora, info, names)
}

#[test]
fn offline_pawn_data() {
    let Some(vfs) = vfs() else { return };
    let (nora, info, names) = load(&vfs);
    assert_eq!(nora.len(), 33);
    let mut jobs = BTreeMap::<u8, usize>::new();
    for (i, f) in nora.iter().enumerate() {
        assert_eq!(f.pawns.len(), 900, "romNoraPawn{i:02}");
        for p in &f.pawns {
            *jobs.entry(p.job).or_default() += 1;
            assert!(p.edit.gender <= 1);
            assert!(
                names.name(p.job, p.edit.gender, p.name_no as usize).is_some(),
                "romNoraPawn{i:02}: name {} out of range",
                p.name_no
            );
            assert_eq!(p.equip_item_id.len(), 11);
            assert_eq!(p.custom_skill.len(), 6);
        }
    }
    // Fighter, Strider, Mage and the three advanced pawn vocations
    // (7 Warrior, 8 Ranger, 9 Sorcerer: 0x00795610 picks LvBattler,
    // LvHunter, LvWizard for them).
    assert_eq!(jobs.keys().copied().collect::<Vec<_>>(), vec![1, 2, 3, 7, 8, 9], "nora vocations");
    let list_sizes: Vec<usize> = names.lists.iter().map(Vec::len).collect();
    assert_eq!(list_sizes, vec![78, 72, 78, 79, 69, 64]);
    assert_eq!(info.pawns.len(), 100);
    eprintln!("nora: 33 x 900, vocations {jobs:?}; names {list_sizes:?}");
    let story: Vec<String> =
        info.pawns[..20].iter().map(|p| format!("Lv{} job{} name_no {}", p.level, p.job, p.name_no)).collect();
    eprintln!("romPawnInfo 0..20 (story stock): {story:?}");
}

#[test]
fn offline_pool_shape() {
    let Some(vfs) = vfs() else { return };
    let (nora, _, _) = load(&vfs);
    for player in [1u32, 5, 30, 150, 200] {
        let mut pool = [u32::MAX; POOL_SLOTS];
        let mut rng = XorShift128 { state: [0x1234_5678, 0x9abc_def0, 0x0fed_cba9, 0x8765_4321 ^ player] };
        rift::fill_offline_pool(&nora, Some(player), POOL_SLOTS, &mut pool, &mut rng).unwrap();
        let unique: HashSet<u32> = pool.iter().copied().collect();
        assert_eq!(unique.len(), POOL_SLOTS, "player {player}: duplicate IDs");
        // Slot 99 is filled first, with the first table entry's level.
        let base = player.clamp(5, 200) as i32;
        let mut expect = Vec::new();
        for (off, n) in POOL_LEVEL_OFFSETS {
            let l = base + off as i32;
            let l = if (1..=200).contains(&l) { l } else { base };
            expect.extend(std::iter::repeat(l as u8).take(n as usize));
        }
        for (k, &id) in pool.iter().rev().enumerate() {
            let RomPawnId::Nora { level, index } = RomPawnId::decode(id) else { panic!("not nora: {id:#x}") };
            assert_eq!(level, expect[k], "player {player}, pawn {k}");
            let (_, file, idx) = rift::nora_template(&nora, id).unwrap();
            assert_eq!(idx, index as usize);
            assert!(level <= NORA_LEVEL_BANDS[file]);
        }
        // Refilling over a full pool (the exe never clears it) still gives
        // 100 distinct IDs.
        rift::fill_offline_pool(&nora, Some(player), POOL_SLOTS, &mut pool, &mut rng).unwrap();
        assert_eq!(pool.iter().copied().collect::<HashSet<u32>>().len(), POOL_SLOTS);
        let mut r2 = XorShift128 { state: [1, 2, 3, 4] };
        let id = rift::random_nora_id(&nora, 250, player, &mut r2).unwrap();
        assert!(matches!(RomPawnId::decode(id), RomPawnId::Nora { level: 200, .. }));
        let _ = r2.next_u32();
    }
}


/// `mRomPawnUniqueID` of stock slots 20..119 read live from the original
/// game by the oracle (claude-4, board #358, 2026-10-09): offline (sNetwork
/// mode 1), player level 7, load mode 1, after the placement shuffle.
const LIVE_POOL_P7: [u32; 100] = [
    0x10090017, 0x100501bb, 0x10090239, 0x100b017a, 0x10090011, 0x100b0164, 0x100b00a6, 0x10090237,
    0x10090186, 0x100b0025, 0x100b034c, 0x100b0064, 0x1005026f, 0x100b0253, 0x10090045, 0x100900c8,
    0x100b02b2, 0x1009002d, 0x10090277, 0x100b013f, 0x100b01ec, 0x100b006d, 0x100901fc, 0x100b0313,
    0x1009025b, 0x100b0350, 0x10090077, 0x100b02ae, 0x100b00c9, 0x100b0326, 0x100d032b, 0x1003036f,
    0x100d0009, 0x100d035a, 0x100d0312, 0x100d032f, 0x100d02a0, 0x100302bc, 0x100f0121, 0x1011029f,
    0x100f0365, 0x1013002e, 0x100f0155, 0x101100bf, 0x101701d5, 0x104300e5, 0x100d0326, 0x100d0363,
    0x101b0163, 0x10110097, 0x101102d7, 0x1001016a, 0x100d00b6, 0x10250150, 0x100f0137, 0x105702f4,
    0x10130137, 0x100f0179, 0x100101f8, 0x101b0075, 0x1011004f, 0x1011027c, 0x102f0142, 0x102501d8,
    0x101700c5, 0x100d0336, 0x10390170, 0x100f001f, 0x10070074, 0x100702f0, 0x100700e0, 0x1007017b,
    0x100700df, 0x100701d8, 0x100700a0, 0x1007028a, 0x100700ec, 0x1007021e, 0x10070217, 0x1007008f,
    0x100900cf, 0x10070121, 0x10070112, 0x10070214, 0x100701d7, 0x100701b1, 0x1007015c, 0x1007002e,
    0x100700b3, 0x1007028b, 0x100901e1, 0x1007018d, 0x100700e9, 0x1007015f, 0x1004005e, 0x100901f8,
    0x10090244, 0x100700b8, 0x100701c5, 0x100701bc,
];

/// The one favourite with a built-in ID that `0x0047d330` put first (the
/// other favourite was in the party, so skipped); it is in the save's
/// `mFavoritePawn`.
const LIVE_FAVOURITE: u32 = 0x1004_005e;

#[test]
fn live_pool_level_multiset() {
    // Compared with the original: 1 favourite + fill_offline_pool(99) at
    // P = 7 must give the live pool's level multiset (order is shuffled by
    // the placement step, and the RNG seed is unknown, so only levels and
    // distinctness are compared).
    let level = |id: u32| match RomPawnId::decode(id) {
        RomPawnId::Nora { level, .. } => level,
        other => panic!("{other:?}"),
    };
    let mut live: Vec<u8> = LIVE_POOL_P7.iter().copied().filter(|&id| id != LIVE_FAVOURITE).map(level).collect();
    live.sort_unstable();
    assert_eq!(live.len(), 99);
    let mut expected = Vec::new();
    let mut left = 99usize;
    for (off, n) in POOL_LEVEL_OFFSETS {
        let l = 7 + off as i32;
        let l = if (1..=200).contains(&l) { l } else { 7 };
        let n = (n as usize).min(left);
        expected.extend(std::iter::repeat(l as u8).take(n));
        left -= n;
    }
    expected.sort_unstable();
    assert_eq!(live, expected);
    assert_eq!(LIVE_POOL_P7.iter().copied().collect::<HashSet<u32>>().len(), 100);

    // The same through the translation, when the install is present.
    let Some(vfs) = vfs() else { return };
    let (nora, _, _) = load(&vfs);
    let mut pool = [u32::MAX; POOL_SLOTS];
    pool[0] = LIVE_FAVOURITE;
    let mut rng = XorShift128 { state: [7, 77, 777, 7777] };
    rift::fill_offline_pool(&nora, Some(7), 99, &mut pool, &mut rng).unwrap();
    let mut ours: Vec<u8> = pool.iter().copied().filter(|&id| id != LIVE_FAVOURITE).map(level).collect();
    ours.sort_unstable();
    assert_eq!(ours, live);
    // Every live index is a valid template of its band file.
    for id in LIVE_POOL_P7 {
        let (_, file, idx) = rift::nora_template(&nora, id).unwrap();
        assert_eq!(idx as u32, id & 0xffff, "romNoraPawn{file:02}");
    }
}
