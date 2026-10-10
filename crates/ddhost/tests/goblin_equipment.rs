//! Authored rescue equipment reaches distinct host enemy lifetimes.
use ddhost::ddenemyrt::{fight::{Fight, FightData}, goblin::equipment::NativeRng};

#[test]
fn installed_rescue_equipment_is_host_owned_and_independent_of_player_count() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: DDDA install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let data = std::sync::Arc::new(FightData::load(&vfs, &file.image().unwrap()).unwrap());
    let groups = ddhost::spawns::group_list(&vfs, 100).unwrap();
    let world = ddspawn::World { scene: 1450, hour: 12, flag: &|_| true };
    let read = |path: &str| vfs.read(path, mtarc::type_hash("rLayout")).ok();
    let (_, spawns) = ddspawn::spawns(100, &groups, (64,53), &read, &world, &[]).unwrap();
    let rescue: Vec<_> = spawns.iter().filter(|spawn| spawn.group == 23).collect();
    assert_eq!(rescue.len(), 3);
    let mut snapshots = Vec::new();
    for players in [1,2,4] {
        let mut fight = Fight::new(data.clone());
        for seat in 0..players { fight.add_arisen(seat); }
        let mut rng = NativeRng::default();
        let mut expected = Vec::new();
        for spawn in &rescue {
            let placement = ddhost::spawns::placement_equipment(&spawn.record);
            assert_eq!(placement.equip_type, 0);
            let owner = ddhost::spawns::add_placed_goblin(&mut fight, spawn);
            let actor = fight.goblins.last().unwrap();
            assert_eq!(actor.fighter.owner, owner);
            assert_eq!(actor.appearance.as_ref().unwrap(), &data.goblin.equipment.choose(placement, &mut rng));
            assert_eq!(actor.appearance.as_ref().unwrap().unit_mask, 1);
            expected.push((owner, actor.appearance.clone()));
        }
        assert_eq!(fight.goblins.len(), rescue.len());
        assert_ne!(expected[0].0, expected[1].0);
        snapshots.push(expected);
    }
    assert!(snapshots.windows(2).all(|pair| pair[0] == pair[1]));
}
