//! Installed record adapter check, not an AI or original-game comparison.
use ddenemyrt::pawn_actor::EquipmentData;

#[test]
fn rook_and_prologue_records_keep_their_actual_stats_and_equipment() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let data = EquipmentData::load(&vfs, &exe.image().unwrap()).unwrap();
    let skills = ddskill::catalogue::ExeTables::from_image(&exe.image().unwrap()).unwrap();
    // Equality branches, switch-range boundaries, gaps, final fallback,
    // and signed/out-of-range behavior of0x004647e0.
    for (item, excluded) in [
        (-1, false),
        (0x46, false),
        (0x47, true),
        (0x48, false),
        (0x12c, true),
        (0x12d, false),
        (0x17d, true),
        (0x17e, false),
        (0x298, true),
        (0x3a0, true),
        (0x53a, true),
        (0x6a4, true),
        (0x736, false),
        (0x740, false),
        (0x76c, false),
        (0x76d, true),
    ] {
        assert_eq!(data.exclusions.contains(item), excluded, "item {item:#x}");
    }
    let story = ddstory::data::InstallData::open(&native, None).unwrap();
    for stock in 10..=14 {
        let record = ddstory::prologue::story_stock(&story, stock, 0).unwrap();
        let actor = data.convert_installed(stock as u32 + 100, &record).unwrap();
        assert_eq!(actor.record, record);
        assert_eq!(actor.base_attacker(false).base_attack, record.param.attack);
        assert_eq!(
            actor.base_attacker(false).base_mgc_attack,
            record.param.mgc_attack
        );
        assert_eq!(actor.base_defender().base_defence, record.param.defend);
        assert!(actor.equipment.defence[0] > 0.0);
        assert_eq!(actor.skill_level(&skills, u32::MAX, &[0; 14], false).unwrap(), 0);
        assert!(actor.skill_level(&skills, 430, &[0; 14], false).is_err());
        // Basic staff input command0xb4 has no learning-text metadata;
        // 0x00780ba0 returns zero, so 0x00542620 selects the first tier.
        assert_eq!(skills.skills[180].text, -1);
        assert_eq!(
            actor.skill_level(&skills, 180, &[u32::MAX; 14], false).unwrap(),
            0
        );
        if stock == 14 {
            assert_eq!((actor.record.param.job, actor.record.param.level), (3, 6));
            let staff = actor.equipped[0].unwrap();
            let item = data.items.get(staff.item_no as u32).unwrap();
            assert_eq!(dditem::rules::instance_level(staff.flag), 0);
            assert_eq!(
                actor.equipment.weapon[0],
                [item.attack() as f32, item.magic_attack() as f32]
            );
        }
    }
}
