use ddsave::{pawns::PawnFields, xml::Document};
fn read(body: &str) -> Result<PawnFields, ddsave::pawns::FieldError> {
    let xml = format!("<class type=\"cSAVE_DATA_CMC\">{body}</class>");
    PawnFields::read(Document::parse(&xml).unwrap().root())
}
fn array(name: &str, ty: &str, n: usize, value: &str) -> String {
    format!(
        "<array name=\"{name}\" type=\"{ty}\" count=\"{n}\">{}</array>",
        (0..n)
            .map(|_| format!("<{ty} value=\"{value}\"/>"))
            .collect::<String>()
    )
}
#[test]
fn missing_is_not_zero() {
    let p = read("").unwrap();
    assert!(p.equipment.is_none());
    assert!(p.knowledge.study_flags.is_none());
    assert!(p.inclinations[0].value.is_none());
    let p = read(&array("mStudyFlag", "u32", 322, "0")).unwrap();
    assert_eq!(p.knowledge.study_flags, Some([0; 322]));
}
#[test]
fn exact_values_and_signed_sentinels() {
    let body = format!(
        "{}{}<class name=\"mParam\" type=\"cSAVE_DATA_PARAM\">{}{}</class><u32 name=\"mInfo[ INFO_TYPE_BELLIGERENT ].mInfoType\" value=\"3\"/><f32 name=\"mInfo[ INFO_TYPE_BELLIGERENT ].mValue\" value=\"2.5\"/>",
        array("mStudyFlag", "u32", 322, "4294967295"),
        array("mStudyData.EncountFrame", "f32", 72, "1.25"),
        array("mWeaponSkill[nWeapon::SWORD]", "s16", 6, "-1"),
        array("mAbility", "s8", 6, "-128")
    );
    let p = read(&body).unwrap();
    assert_eq!(p.knowledge.study_flags, Some([u32::MAX; 322]));
    assert_eq!(p.knowledge.encounter_frames, Some([1.25; 72]));
    assert_eq!(p.skills.weapon[0], Some([-1; 6]));
    assert_eq!(p.skills.abilities, Some([-128; 6]));
    assert_eq!(p.inclinations[0].info_type, Some(3));
    assert_eq!(p.inclinations[0].value, Some(2.5));
    assert_eq!(p.inclinations[0].status, None);
}
#[test]
fn rejects_lengths_types_duplicates_and_bad_values() {
    let valid = array("mStudyFlag", "u32", 322, "0");
    for bad in [
        array("mStudyFlag", "u32", 321, "0"),
        valid.replacen("count=\"322\"", "count=\"323\"", 1),
        valid.replacen("<u32 value=\"0\"/>", "", 1),
        valid.replacen("</array>", "<u32 value=\"0\"/></array>", 1),
        array("mStudyFlag", "u32", 322, "4294967296"),
        array("mStudyFlag", "s32", 322, "0"),
        format!("{valid}{valid}"),
        array("mStudyData.EncountFrame", "f32", 72, "NaN"),
        "<f32 name=\"mInfo[ INFO_TYPE_BELLIGERENT ].mValue\" value=\"inf\"/>".into(),
    ] {
        assert!(read(&bad).is_err());
    }
}
#[test]
fn full_equipment_fields_and_partial_item() {
    let item = "<class type=\"sItemManager::cITEM_PARAM_DATA\"><s16 name=\"data.mNum\" value=\"2\"/><s16 name=\"data.mItemNo\" value=\"-1\"/><u32 name=\"data.mFlag\" value=\"4294967295\"/><u16 name=\"data.mChgNum\" value=\"65535\"/><u16 name=\"data.mDay1\" value=\"1\"/><u16 name=\"data.mDay2\" value=\"2\"/><u16 name=\"data.mDay3\" value=\"3\"/><s8 name=\"data.mMutationPool\" value=\"-1\"/><s8 name=\"data.mOwnerId\" value=\"-128\"/><u32 name=\"data.mKey\" value=\"42\"/></class>";
    let body = format!(
        "<class name=\"mParam\" type=\"cSAVE_DATA_PARAM\"><array name=\"mEquipItem\" type=\"class\" count=\"12\">{}</array><class name=\"mPresentItem\" type=\"sItemManager::cITEM_PARAM_DATA\"/></class>",
        item.repeat(12)
    );
    let p = read(&body).unwrap();
    let e = &p.equipment.unwrap()[0];
    assert_eq!(e.num, Some(2));
    assert_eq!(e.item_no, Some(-1));
    assert_eq!(e.flag, Some(u32::MAX));
    assert_eq!(e.change_num, Some(u16::MAX));
    assert_eq!(e.days, [Some(1), Some(2), Some(3)]);
    assert_eq!(e.mutation_pool, Some(-1));
    assert_eq!(e.owner_id, Some(-128));
    assert_eq!(e.key, Some(42));
    assert_eq!(p.present_item.unwrap().num, None);
    assert!(read(&body.replace("value=\"-128\"", "value=\"128\"")).is_err());
    assert!(read(&body.replace("sItemManager::cITEM_PARAM_DATA", "OtherItem")).is_err());
    assert!(read(&body.replace("cSAVE_DATA_PARAM", "OtherParam")).is_err());
}
#[test]
fn private_copy_all_records() {
    use std::path::PathBuf;
    let explicit = std::env::var_os("DDDA_SAVE");
    let path = if let Some(p) = explicit {
        let p = PathBuf::from(p);
        assert!(p.is_file(), "configured private save unavailable");
        Some(p)
    } else {
        let mut candidates: Vec<_> = std::fs::read_dir("E:/ddda-re/save-backups")
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|d| d.path().join("367500/remote/DDDA.sav"))
            .filter(|p| p.is_file())
            .collect();
        candidates.sort();
        candidates.pop()
    };
    let Some(path) = path else {
        eprintln!("skipped: no private save copy");
        return;
    };
    let s = ddsave::SaveFile::parse(&std::fs::read(path).unwrap()).unwrap();
    let doc = s.document().unwrap();
    let mut count = 0;
    for n in doc
        .iter()
        .filter(|n| n.tag() == "class" && n.ty().as_deref() == Some("cSAVE_DATA_CMC"))
    {
        let p = PawnFields::read(n).expect("typed pawn fields");
        assert!(p.knowledge.study_flags.is_some());
        assert!(p.knowledge.local_study_flags.is_some());
        assert!(p.knowledge.encounter_frames.is_some());
        assert!(p.knowledge.kill_counts.is_some());
        assert!(p.knowledge.unique_counts.is_some());
        assert!(p.equipment.is_some());
        assert!(p.present_item.is_some());
        assert!(p.skills.weapon.iter().all(Option::is_some));
        assert!(
            p.skills.level_one.is_some()
                && p.skills.level_two.is_some()
                && p.skills.abilities.is_some()
                && p.skills.ability_info.is_some()
        );
        assert!(
            p.inclinations
                .iter()
                .all(|i| i.info_type.is_some() && i.value.is_some() && i.status.is_some())
        );
        assert!(
            p.extra_inclinations
                .iter()
                .all(|i| i.info_type.is_some() && i.value.is_some())
        );
        // Compare every stored array word to the source XML without logging values.
        assert!(
            p.knowledge
                .study_flags
                .unwrap()
                .iter()
                .copied()
                .eq(n.array::<u32>("mStudyFlag").unwrap())
        );
        count += 1;
    }
    assert!(count > 0);
    eprintln!("validated {count} full pawn records (values withheld)");
}
