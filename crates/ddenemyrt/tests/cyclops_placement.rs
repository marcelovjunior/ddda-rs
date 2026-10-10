use ddcombat::damage::RegionPool;
use ddenemyrt::cyclops::{initialize_placement, PlacementValues};

#[test]
fn installed_q0003_cyclops_health_and_legs() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !native.is_dir() {
        eprintln!("DDDA install absent; skipped cyclops placement regression");
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let placements = mtlot::parse(
        &vfs.read(r"scr\st100\etc\st100_63m52n_e20", mtarc::type_hash("rLayout"))
            .unwrap(),
    )
    .unwrap();
    let placement = placements.iter().find(|record| record.id == 0).unwrap();
    assert_eq!(placement.class, "cSetInfoEnemy5000");
    let scalar = |label| match placement.get(label) {
        Some(mtlot::Value::F32(value)) => *value,
        other => panic!("missing placement scalar {label}: {other:?}"),
    };
    let values = PlacementValues {
        arisen_proof: match placement.get("覚者の証サイクロ") {
            Some(mtlot::Value::Int(value)) => *value != 0,
            other => panic!("missing Arisen proof flag: {other:?}"),
        },
        remaining_hp_ratio: scalar("覚者の証サイクロのときの残りHPの倍率"),
        leg_stagger: scalar("覚者の証サイクロのときの足ののけぞり値"),
        leg_knockdown: scalar("覚者の証サイクロのときの足のぶっとび値"),
    };
    assert!(values.arisen_proof);
    let parsed = ddenemy::RegionStatus::parse(
        &vfs.read(r"charparam\em\em5000", mtarc::type_hash("rRegionStatus"))
            .unwrap(),
    )
    .unwrap();
    let elements = &parsed
        .regions
        .iter()
        .find(|region| region.kind == 0)
        .unwrap()
        .elements;
    let original: Vec<_> = elements
        .iter()
        .map(|element| {
            RegionPool::from_element(
                element.id,
                element.hp_max,
                element.dp_max,
                element.dp_speed,
                element.bp_max,
                element.bp_speed,
                element.damage_adjustment,
                element.hit_stop_adjustment,
                element.attributes,
                element.surface,
                element.dp_reset_timer_max,
                element.bp_reset_timer_max,
            )
        })
        .collect();
    // This placement does not request a common HP multiplier, so the
    // ordinary region initialization is the input to the special override.
    assert_eq!(placement.get("HP倍率設定の有無"), Some(&mtlot::Value::Int(0)));
    let mut actual = original.clone();
    initialize_placement(&exe, values, &mut actual).unwrap();
    assert_eq!(actual[0].hp, original[0].hp_max * values.remaining_hp_ratio);
    for (before, after) in original.iter().zip(&actual) {
        let mut expected = before.clone();
        if before.id == exe.u32(0x014f_f680 + 5 * 4).unwrap()
            || before.id == exe.u32(0x014f_f680 + 6 * 4).unwrap()
        {
            expected.dp = values.leg_stagger;
            expected.bp = values.leg_knockdown;
        }
        if before == &original[0] {
            expected.hp = original[0].hp_max * values.remaining_hp_ratio;
        }
        assert_eq!(after, &expected);
    }
    eprintln!(
        "q0003 initial HP={}/{}; legs DP={} BP={}",
        actual[0].hp, actual[0].hp_max, values.leg_stagger, values.leg_knockdown
    );

    let mut disabled = original.clone();
    initialize_placement(
        &exe,
        PlacementValues {
            arisen_proof: false,
            ..values
        },
        &mut disabled,
    )
    .unwrap();
    assert_eq!(disabled, original);
    for (ratio, expected) in [(2.0, original[0].hp_max), (-0.5, original[0].hp_max * -0.5)] {
        initialize_placement(
            &exe,
            PlacementValues {
                remaining_hp_ratio: ratio,
                ..values
            },
            &mut actual,
        )
        .unwrap();
        assert_eq!(actual[0].hp, expected);
    }
    // The maximum getter uses slot zero; the legs are found by identity even
    // when the supplied array's order differs, and missing legs are skipped.
    let mut reordered = original.clone();
    reordered.reverse();
    let expected_hp = reordered[0].hp_max * values.remaining_hp_ratio;
    initialize_placement(&exe, values, &mut reordered).unwrap();
    assert_eq!(reordered[0].hp, expected_hp);
    for id in [5, 6] {
        let leg = reordered.iter().find(|region| region.id == id).unwrap();
        assert_eq!((leg.dp, leg.bp), (values.leg_stagger, values.leg_knockdown));
    }
    initialize_placement(&exe, values, &mut []).unwrap();
}
