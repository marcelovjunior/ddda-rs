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

