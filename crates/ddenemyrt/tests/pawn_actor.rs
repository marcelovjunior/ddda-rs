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
    let story = ddstory::data::InstallData::open(&native, None).unwrap();
    for stock in 10..=14 {
        let record = ddstory::prologue::story_stock(&story, stock, 0).unwrap();
        // Fixture explicitly enables installed starting gear. The general
        // 0x004647e0 exclusion predicate is a caller input, not validated here.
        let actor = data.convert(stock as u32 + 100, &record, |_| false).unwrap();
        assert_eq!(actor.record, record);
        assert_eq!(actor.base_attacker(false).base_attack, record.param.attack);
        assert_eq!(
            actor.base_attacker(false).base_mgc_attack,
            record.param.mgc_attack
        );
        assert_eq!(actor.base_defender().base_defence, record.param.defend);
        assert!(actor.equipment.defence[0] > 0.0);
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
