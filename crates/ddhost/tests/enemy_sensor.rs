//! Installed group250 stays out of the rescue until actual perception.
use ddhost::{ddenemyrt::{fight::{Fight, FightData}, goblin::{Target, action}}, npc_placements, spawns};
use std::sync::Arc;
struct Flat(f32);
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { (below >= self.0).then_some(self.0) }
}

#[test]
fn installed_group250_perception_and_authored_rescue_targets() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: DDDA install absent"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let image = exe.image().unwrap();
    let data = Arc::new(FightData::load(&vfs, &image).unwrap());
    let groups = spawns::group_list(&vfs, 100).unwrap();
    let world = ddspawn::World { scene: 1450, hour: 12, flag: &|_| true };
    let (_, placements) = ddspawn::spawns(100, &groups, (64,53),
        &|path| vfs.read(path, mtarc::type_hash("rLayout")).ok(), &world, &[]).unwrap();
    let npcs = npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = npc_placements::LoadedLayouts::read(&vfs, 100, &npcs, &[(0,0)]).unwrap();
    let reynard = layouts.candidates(&world).placements.into_iter().find(|p| p.npc == 304).unwrap();
    let npc = Target { id: 106, pos: reynard.transform.position, alive: true };
    let ordinary: Vec<_> = placements.iter().filter(|p| p.group == 250).collect();
    let rescue: Vec<_> = placements.iter().filter(|p| p.group == 23).collect();
    assert_eq!((ordinary.len(), rescue.len()), (3, 3));
    eprintln!("Reynard {:?}; group250 distances {:?}", npc.pos, ordinary.iter().map(|p|
        (0..3).map(|i| (p.position[i]-npc.pos[i]).powi(2)).sum::<f32>().sqrt()).collect::<Vec<_>>());
    for placement in ordinary {
        let mut fight = Fight::new(data.clone());
        spawns::add_placed_goblin(&mut fight, placement);
        let goblin = &mut fight.goblins[0];
        let initial = goblin.pos;
        // All player seats and the pawn stay outside the resource sphere;
        // the native fallback slot still exists, but cannot set combat flag2.
        let far = Target { id: 0, pos: [initial[0] + 4000.0, initial[1], initial[2]], alive: true };
        let far_pawn = Target { id: 1, ..far };
        for _ in 0..300 {
            goblin.step_with_subtypes(&data.goblin, &[far, far_pawn], &[npc], &[1], &Flat(initial[1]), 1.0);
            assert_eq!(goblin.target, None);
            assert_eq!(goblin.alert.state, 0);
            assert!(goblin.sensor_error.is_none(), "{:?}", goblin.sensor_error);
            assert_ne!(goblin.action, action::RUN_TRACE);
        }
        assert!((0..3).all(|i| (goblin.pos[i] - initial[i]).abs() < 1.0));
        let near = Target { id: 1, pos: [goblin.pos[0], goblin.pos[1], goblin.pos[2] + 100.0], alive: true };
        goblin.step_with_subtypes(&data.goblin, &[far, near], &[], &[1], &Flat(initial[1]), 1.0);
        assert_eq!(goblin.target, Some(1));
        // Keep an already selected actor while its action cannot rethink,
        // even when another live proxy enters the sensor. Influence decay
        // still runs; an action-done update may then select the new proxy.
        let score = goblin.distance_hate.0.iter().find(|(owner, _)| *owner == 1).unwrap().1;
        let nearby_npc = Target { id: 106, pos: [goblin.pos[0], goblin.pos[1], goblin.pos[2] + 50.0], alive: true };
        let departed = Target { id: 1, ..far };
        goblin.done = false;
        goblin.step_with_subtypes(&data.goblin, &[far, departed], &[nearby_npc], &[1], &Flat(initial[1]), 1.0);
        assert_eq!(goblin.target, Some(1));
        assert_eq!(goblin.distance_hate.0.iter().find(|(owner, _)| *owner == 1).unwrap().1, score - 1.0);
        assert!(goblin.distance_hate.0.iter().all(|(owner, _)| *owner != 106));
        goblin.done = true;
        goblin.step_with_subtypes(&data.goblin, &[far, departed], &[nearby_npc], &[1], &Flat(initial[1]), 1.0);
        assert_eq!(goblin.target, Some(106));
    }
    for placement in rescue {
        let mut fight = Fight::new(data.clone());
        spawns::add_placed_goblin(&mut fight, placement);
        let goblin = &mut fight.goblins[0];
        let height = goblin.pos[1];
        assert_eq!(goblin.driver.mgr.root, data.goblin.tables.root(1).unwrap());
        assert_eq!(goblin.sensor_radius_scale, match placement.record.get("センサー半径倍率を使用する") {
            Some(mtlot::Value::Int(0)) => None,
            Some(mtlot::Value::Int(_)) => match placement.record.get("センサー半径倍率") {
                Some(mtlot::Value::F32(scale)) => Some(*scale), _ => panic!("missing scale") },
            _ => panic!("missing scale flag"),
        });
        for _ in 0..60 {
            goblin.step_with_npcs(&data.goblin, &[], &[npc], &Flat(height), 1.0);
            assert_eq!(goblin.target, Some(106));
            assert!(goblin.sensor_error.is_none(), "{:?}", goblin.sensor_error);
        }
    }
}
