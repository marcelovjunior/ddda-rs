use ddcombat::damage::Adjust;
use ddenemyrt::goblin::Target;
use ddenemyrt::npc_sensor::{DistanceHate, Sensor, SensorRow};

fn target(id: u32, x: f32) -> Target { Target { id, pos: [x, 0.0, 0.0], alive: true } }

#[test]
fn distance_hate_preserves_native_ties_memory_and_dead_gate() {
    let mut hate = DistanceHate::default();
    let all = [target(501, 100.0), target(0, -100.0), target(502, 3000.0)];
    assert_eq!(hate.select(&all, &all, [0.0; 3]).unwrap().id, 501);
    assert_eq!(hate.0.len(), 2);
    // Leaving the queried sensor does not erase an existing influence.
    assert_eq!(hate.select(&[], &all, [0.0; 3]).unwrap().id, 501);
    let mut dead = all;
    dead[0].alive = false;
    assert_eq!(hate.select(&[], &dead, [0.0; 3]).unwrap().id, 0);
    hate.retire(0);
    assert!(hate.select(&[], &dead, [0.0; 3]).is_none());
}

#[test]
fn unsupported_sensor_branches_are_reported_and_binary_rows_are_checked() {
    assert!(Sensor::parse(b"SNR2").is_err());
    let mut sensor = Sensor(vec![SensorRow { class: 2, types: 0x16, enabled_by: 1,
        shape: 5, radius: 100.0, fov: 360.0, flags: 0, joint: -1, offset: [0.0; 3], direction: [0.0, 0.0, 1.0] }]);
    assert!(sensor.admits(1, [0.0; 3], 0.0, target(501, 50.0), 4).unwrap());
    assert!(!sensor.admits(2, [0.0; 3], 0.0, target(501, 50.0), 4).unwrap());
    assert!(!sensor.admits(1, [0.0; 3], 0.0, target(501, 50.0), 3).unwrap());
    sensor.0[0].flags = 1;
    assert!(sensor.admits(1, [0.0; 3], 0.0, target(501, 50.0), 4).is_err());
}

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { (below >= 0.0).then_some(0.0) }
}

#[test]
fn installed_goblin_selects_and_damages_reynard_and_no_damage_holds() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no installed game data"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let image = exe.image().unwrap();
    let body = std::sync::Arc::new(ddenemyrt::npc_body::NpcBody::load(&vfs, &image, 304, 304, 0, 0, 0).unwrap());
    let data = std::sync::Arc::new(ddenemyrt::fight::FightData::load(&vfs, &image).unwrap());
    let game: &Adjust = &data.tables.game;
    let region = ddenemyrt::npc_combat::ordinary_region(&vfs, game, 1000.0).unwrap();
    let motion = body.resolve(3073).unwrap();
    let sim = ddsim::Sim::new(Default::default());
    for flags in [0, 0x0800_0000, 0x1000_0000] {
        let mut fight = ddenemyrt::fight::Fight::new(data.clone());
        let npc = fight.bind_npc(304, 1, body.clone(), region.clone(), motion);
        let goblin = fight.add_goblin([0.0, 0.0, 100.0], std::f32::consts::PI);
        assert_ne!(npc, goblin);
        fight.npcs.get_mut(&npc).unwrap().set_flags(flags);
        let mut contacts = 0;
        let mut history_seen = false;
        for _ in 0..360 {
            contacts += fight.hits(1.0).iter().filter(|e| e.victim == npc).count();
            history_seen |= !fight.npcs[&npc].fighter.history.entries.is_empty();
            fight.enemies(&sim, &Flat, 1.0);
            fight.volumes(&sim);
            assert_eq!(fight.goblins[0].target, Some(npc));
            assert!(fight.goblins[0].sensor_error.is_none());
            assert!(fight.npc_errors.is_empty());
            if flags == 0 && contacts > 0 { break; }
        }
        let hp = fight.npcs[&npc].fighter.victim.hp();
        if flags == 0 { assert!(hp < 1000.0, "actual goblin motions must damage installed NPC hurts"); }
        else { assert_eq!(hp, 1000.0); }
        if flags != 0x1000_0000 { assert!(contacts > 0); }
        else { assert_eq!(contacts, 0); }
        assert!(fight.kill_awards().is_empty());
        assert!(history_seen);
        fight.unbind_npc(npc);
        assert!(fight.npcs.is_empty());
        assert!(!fight.positions.contains_key(&npc));
        assert!(fight.goblins[0].distance_hate.0.iter().all(|(id, _)| *id != npc));
    }
}
