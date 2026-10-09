use ddenemyrt::fight::{Fight, FightData};

#[test]
fn falls_damage_only_the_named_living_seat_without_combat_defence() {
    let root = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.is_dir() { eprintln!("SKIP: no install"); return; }
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let exe = ddexe::ExeFile::next_to(&root).unwrap();
    let data = FightData::load(&vfs, &exe.image().unwrap()).unwrap();
    let mut fight = Fight::new(std::sync::Arc::new(data));
    fight.add_arisen(0);
    fight.add_arisen(1);
    let hp = fight.arisen[&0].fighter.victim.hp();
    let other = fight.arisen[&1].fighter.victim.hp();
    assert_eq!(fight.fall_damage(0, 200.0), 200.0);
    assert_eq!(fight.arisen[&0].fighter.victim.hp(), hp - 200.0);
    assert_eq!(fight.arisen[&1].fighter.victim.hp(), other);
    assert_eq!(fight.fall_damage(2, 200.0), 0.0);
    for amount in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(fight.fall_damage(1, amount), 0.0);
    }
    assert_eq!(fight.arisen[&1].fighter.victim.hp(), other);
    assert_eq!(fight.fall_damage(0, hp), hp - 200.0);
    assert!(fight.arisen[&0].fighter.victim.dead());
    assert_eq!(fight.fall_damage(0, 200.0), 0.0);
    assert_eq!(fight.arisen[&0].fighter.victim.hp(), 0.0);
}
