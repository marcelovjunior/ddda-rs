use ddsave::knowledge::*;

fn entry(category: Category, id: u32, bits: &[u32]) -> Entry {
    Entry {
        category,
        id,
        bits: bits.to_vec(),
        completion_bit: None,
        name_message: 0,
        metadata: vec![],
    }
}
#[test]
fn word_edges_duplicates_invalid_bits_and_completion_sentinel() {
    let mut flags = [0u32; 322];
    flags[0] = 1 | (1 << 31);
    flags[1] = 1;
    flags[321] = 1 << 31;
    let mut row = entry(Category::Quest, 1, &[0, 31, 32, 0x283f, 0x2840, 31]);
    assert_eq!(
        row.progress(&flags),
        Progress {
            learned: 5,
            total: 6,
            stars: Some(2)
        }
    );
    row.completion_bit = Some(0);
    assert!(!row.quest_known(&flags));
    row.completion_bit = Some(31);
    assert!(row.quest_known(&flags));
    row.completion_bit = Some(0x2840);
    assert!(!row.quest_known(&flags));
    assert!(!has_bit(&flags, u32::MAX));
}
#[test]
fn source_star_boundaries_and_float_rounding() {
    for (have, total, want) in [
        (0, 0, None),
        (0, 6, Some(0)),
        (1, 6, Some(1)),
        (2, 6, Some(1)),
        (3, 6, Some(2)),
        (5, 6, Some(2)),
        (6, 6, Some(3)),
        (7, 6, Some(3)),
        (u32::MAX - 1, u32::MAX, Some(3)),
    ] {
        assert_eq!(stars(have, total), want);
    }
}
#[test]
fn counter_thresholds_equalities_and_unique_array_boundary() {
    let threshold = Threshold {
        encounter_units: 2.,
        kills: 5,
    };
    assert!(!threshold.met(59.99, 5));
    assert!(!threshold.met(60., 4));
    assert!(threshold.met(60., 5));
    assert!(!threshold.met(f32::NAN, 5));
    let mut counts = [0; 116];
    counts[115] = 7;
    assert!(UniqueRule {
        bit: 32,
        counter_index: 115,
        required: 7
    }
    .met(&counts));
    assert!(!UniqueRule {
        bit: 32,
        counter_index: 116,
        required: 0
    }
    .met(&counts));
    assert!(!UniqueRule {
        bit: 32,
        counter_index: 115,
        required: 8
    }
    .met(&counts));
}
#[test]
fn reverse_area_mapping_preserves_original_last_row_precedence() {
    let table = Tables {
        quests: vec![],
        enemies: vec![],
        enemy_counters: vec![],
        unique_rules: vec![],
        areas: vec![
            entry(Category::Area, 100, &[0x2040, 0x2041]),
            entry(Category::Area, 200, &[0x2041, 0x283f]),
        ],
    };
    assert_eq!(table.area_for_bit(0x2040), Some(100));
    assert_eq!(table.area_for_bit(0x2041), Some(200));
    assert_eq!(table.area_for_bit(0x283f), Some(200));
    assert_eq!(table.area_for_bit(0x203f), None);
    assert_eq!(table.area_for_bit(0x2840), None);
}
#[test]
fn installed_tables_and_private_original_save_examples() {
    use std::path::PathBuf;
    let explicit = std::env::var_os("DDDA_EXE");
    let exe_path = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/DDDA.exe".into());
    if !exe_path.is_file() {
        assert!(explicit.is_none());
        eprintln!("SKIP installed knowledge tables: no exe");
        return;
    }
    let exe = ddexe::ExeFile::open(exe_path).unwrap();
    let tables = Tables::from_image(&exe.image().unwrap()).unwrap();
    assert_eq!(
        (tables.quests.len(), tables.enemies.len(), tables.areas.len()),
        (52, 97, 18)
    );
    assert_eq!(
        (tables.enemy_counters.len(), tables.unique_rules.len()),
        (92, 113)
    );
    assert!(tables.quests.windows(2).all(|w| w[0].id < w[1].id));
    assert!(tables
        .quests
        .iter()
        .chain(&tables.enemies)
        .chain(&tables.areas)
        .all(|e| e.bits.iter().all(|&b| b < BIT_COUNT)));
    for counter in &tables.enemy_counters {
        // Counter mappings include enemy ids with no knowledge-menu row;
        // the original lookup skips those rather than requiring a full join.
        assert!(counter.counter_index < 72);
        assert!(counter.thresholds.iter().all(|t| t.encounter_units.is_finite()));
    }
    let save_path = if let Some(p) = std::env::var_os("DDDA_SAVE") {
        let p = PathBuf::from(p);
        assert!(p.is_file(), "configured save unavailable");
        Some(p)
    } else {
        let mut paths: Vec<_> = std::fs::read_dir("E:/ddda-re/save-backups")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|p| p.path().join("367500/remote/DDDA.sav"))
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        paths.pop()
    };
    let Some(path) = save_path else {
        eprintln!("SKIP private save examples: no backup");
        return;
    };
    let save = ddsave::SaveFile::parse(&std::fs::read(path).unwrap()).unwrap();
    let doc = save.document().unwrap();
    let manual = doc
        .iter()
        .find(|n| n.name().as_deref() == Some("mPlayerDataManual"))
        .unwrap();
    let pawn = manual
        .path("mPlCmcEditAndParam/mCmc")
        .unwrap()
        .children()
        .next()
        .unwrap();
    let knowledge = ddsave::pawns::PawnFields::read(pawn).unwrap().knowledge;
    let flags = knowledge.study_flags.unwrap();
    for rows in [&tables.quests, &tables.enemies, &tables.areas] {
        let example = rows
            .iter()
            .find(|r| r.progress(&flags).learned > 0)
            .expect("one learned example per class");
        let p = example.progress(&flags);
        eprintln!(
            "{:?} id{}: learned {}/{} stars{:?}",
            example.category, example.id, p.learned, p.total, p.stars
        );
        assert!(p.learned <= p.total);
    }
    let counters = tables
        .enemy_counters
        .iter()
        .find(|r| knowledge.kill_counts.as_ref().unwrap()[r.counter_index] > 0)
        .expect("positive saved kill counter");
    eprintln!(
        "enemy{} counter{}: frames{} kills{}",
        counters.enemy_id,
        counters.counter_index,
        knowledge.encounter_frames.unwrap()[counters.counter_index],
        knowledge.kill_counts.unwrap()[counters.counter_index]
    );
    let unique = tables
        .unique_rules
        .iter()
        .find(|r| knowledge.unique_counts.as_ref().unwrap()[r.counter_index] > 0)
        .expect("positive saved unique counter");
    eprintln!(
        "unique counter{}: value{} bit{} threshold{}",
        unique.counter_index,
        knowledge.unique_counts.unwrap()[unique.counter_index],
        unique.bit,
        unique.required
    );
}
