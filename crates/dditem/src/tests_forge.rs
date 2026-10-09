//! Regression tests for the enhancement, dragonforge, curse and RNG
//! translations.

use crate::forge::*;
use crate::lvparam::{LevelEntry, LevelRow, ENTRIES};
use crate::lvup::{LvUpRow, LvUpStep};
use crate::rng::XorShift;
use crate::rules::{is_rarity, Currency, RARITY_ITEMS};
use crate::tables::CurseConversion;

/// Vectors from an instruction-level emulation of DDDA.exe `0x00d006c0`
/// (seed) and `0x00d00690` (next).
#[test]
fn rng_matches_exe() {
    let cases: [(u32, [u32; 4], [u32; 3]); 5] = [
        (0, [0x0194fd72, 0x79e6c985, 0x08dd9701, 0x41cfce91], [0x3910ee3a, 0x25e6e2ef, 0xea8e25b1]),
        (1, [0xa0bd372e, 0x2d4e6884, 0x446990d9, 0xd051ceaf], [0xe8c95c71, 0xf055f53b, 0x7490b36d]),
        (0x15, [0x87112ec5, 0x741b8fb5, 0x00cd6f62, 0xb567d480], [0xa4134502, 0x1cee5f6c, 0xa0e5f61f]),
        (12345, [0xbaf0c6ad, 0xb9c28b33, 0x6a09ef0e, 0x3e536ce5], [0xea6f4fd0, 0x19f1f303, 0x8da7f232]),
        (0xffff_ffff, [0x474d11e0, 0x800c3410, 0xa9b8e1a1, 0xc1735f29], [0x023599dc, 0x9191ee9c, 0x456f0dab]),
    ];
    for (seed, state, outs) in cases {
        let mut r = XorShift::new(seed);
        assert_eq!(r.state, state, "seed {seed:#x}");
        for o in outs {
            assert_eq!(r.next_u32(), o, "seed {seed:#x}");
        }
    }
}

fn row(entries: &[(usize, i32, f32, bool)]) -> LevelRow {
    let mut e = [LevelEntry {
        kind: -1,
        rate: 0.0,
        direct: false,
    }; ENTRIES];
    for &(i, kind, rate, direct) in entries {
        e[i] = LevelEntry { kind, rate, direct };
    }
    LevelRow { entries: e }
}

#[test]
fn level_stats() {
    // Level 1 = entries 0-1, level 2 = 2-5, level 3 = 6-11, level 4 = 12-19.
    let r = row(&[(0, 0, 16.0, true), (2, 0, 32.0, true), (6, 0, 1.5, false), (12, 13, 9.0, true)]);
    assert_eq!(level_stat(Some(&r), 50.0, 0, 0), 50.0);
    assert_eq!(level_stat(Some(&r), 50.0, 0, 1), 66.0);
    assert_eq!(level_stat(Some(&r), 50.0, 0, 2), 82.0);
    // Each level replaces the value from the base, not the previous level.
    assert_eq!(level_stat(Some(&r), 50.0, 0, 3), 75.0);
    assert_eq!(level_stat(Some(&r), 50.0, 0, 4), 75.0);
    assert_eq!(level_stat(Some(&r), 50.0, 13, 4), 59.0);
    assert_eq!(level_stat(Some(&r), 50.0, 0, 7), 50.0);
    assert_eq!(level_stat(None, 50.0, 0, 3), 50.0);
    assert_eq!(level_table(7), Some(LevelTable::Weapon));
    assert_eq!(level_table(27), Some(LevelTable::Armor));
    assert_eq!(level_table(25), Some(LevelTable::Accessory));
    assert_eq!(level_table(26), None);
}

#[test]
fn enhancement_charges_and_materials() {
    let s = |gold, m: Option<u16>, c| LvUpStep {
        gold,
        material: m,
        material_count: c,
    };
    let r = LvUpRow {
        item: 301,
        unnamed: 0,
        steps: [
            s(70, None, 0),
            s(220, Some(1416), 1),
            s(690, Some(1227), 4),
            s(4000, Some(1730), 3),
            s(5000, Some(1770), 5),
        ],
    };
    assert_eq!(enhance_charge(&r, 1), Some((Currency::Gold, 70, None)));
    assert_eq!(enhance_charge(&r, 3), Some((Currency::Gold, 690, Some((1227, 4)))));
    assert_eq!(enhance_charge(&r, 5), Some((Currency::RiftCrystals, 4000, Some((1730, 3)))));
    assert_eq!(enhance_charge(&r, 6), Some((Currency::RiftCrystals, 5000, Some((1770, 5)))));
    assert_eq!(enhance_charge(&r, 7), None);
    // Owners: player, pawn 1, pawn 2, storage. Bags drain from the last
    // bag owner down, storage last.
    assert_eq!(take_materials(&[3, 1, 2, 10], 4), vec![(2, 2), (1, 1), (0, 1)]);
    assert_eq!(take_materials(&[1, 0, 0, 10], 4), vec![(0, 1), (3, 3)]);
    assert_eq!(take_materials(&[1, 0, 0, 10], 0), vec![]);
}

#[test]
fn dragonforge() {
    assert_eq!(dragonforge_chance(0x5c, 0, false), Some(0));
    assert_eq!(dragonforge_chance(0x5d, 3, false), Some(18));
    assert_eq!(dragonforge_chance(0xb4, 2, false), Some(65));
    assert_eq!(dragonforge_chance(0x5b, 1, false), Some(100));
    assert_eq!(dragonforge_chance(0x5b, 1, true), Some(2));
    assert_eq!(dragonforge_chance(0x05, 1, false), None);
    // chance + bonus > roll % 100
    assert!(dragonforge_roll(0x5c, 3, false, 0, 17));
    assert!(!dragonforge_roll(0x5c, 3, false, 0, 18));
    assert!(dragonforge_roll(0x5c, 3, false, 1, 118));
    assert!(!dragonforge_roll(0x5c, 0, false, 0, 0));
    assert!(!dragonforge_roll(0xb4, 4, false, 100, 0));
}

fn curse(items: &[i16], rates: &[i16]) -> CurseConversion {
    let mut c = CurseConversion {
        level: 1,
        items: [-1; 20],
        rates: [0; 20],
    };
    c.items[..items.len()].copy_from_slice(items);
    c.rates[..rates.len()].copy_from_slice(rates);
    c
}

#[test]
fn curse_rows_and_roll() {
    // Novelty items ignore the vocation.
    assert_eq!(curse_row(0x6a5, 7, 3, 3), 0);
    assert_eq!(curse_row(0x6a7, 7, 3, 3), 2);
    // Gear Lv.1 = row 3 + 0 / 9 / 18 for Fighter / Strider / Mage groups.
    assert_eq!(curse_row(0x6a8, 7, 1, 1), 3);
    assert_eq!(curse_row(0x6a8, 7, 8, 8), 12);
    assert_eq!(curse_row(0x6b0, 7, 9, 9), 11 + 18);
    // Which of player / pawn decides: bit 0 of the first value seeded with
    // seed + 0x15. Seed 0: 0x...0x15 seed gives 0xa4134502 (even): player.
    assert_eq!(curse_row(0x6a8, 0, 2, 3), 12);
    // Hybrid coin from seed + 0x28.
    let coin = XorShift::new(0x28).next_u32() & 1 != 0;
    assert_eq!(curse_row(0x6a8, 0, 4, 4), if coin { 3 } else { 21 });
    // Roll: first slot whose running sum >= next(seed) % total.
    let mut table = vec![curse(&[10], &[100]); 30];
    table[0] = curse(&[10, 11, 12], &[30, 30, 40]);
    let d = XorShift::new(5).next_u32() % 100;
    let want = if d <= 30 {
        10
    } else if d <= 60 {
        11
    } else {
        12
    };
    assert_eq!(curse_roll(&table, 0x6a5, 5, 1, 1), want);
    // Out-of-range rows read row 0; an empty table falls back to 0x6a5.
    assert_eq!(curse_roll(&table[..1], 0x6b0, 5, 9, 9), want);
    assert_eq!(curse_roll(&[], 0x6a5, 5, 1, 1), CURSED_FIRST);
}

#[test]
fn rarities_and_augments() {
    assert!(RARITY_ITEMS.windows(2).all(|w| w[0] < w[1]));
    assert!(is_rarity(1300));
    assert!(!is_rarity(66));
    assert!(!is_rarity(70_000));
    assert_eq!(AUGMENT_NAME_INDEX[AUGMENT_SUASION as usize], 64);
    assert_eq!(AUGMENT_NAME_INDEX[7], 255);
    assert_eq!(AUGMENT_GROUP[7], 13);
}
