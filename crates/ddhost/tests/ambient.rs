use ddsim::Ground;

#[test]
fn canonical_ambient_ownership_survives_duplicate_cells_but_not_retirement() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        return;
    }
    let exe_file = ddexe::ExeFile::next_to(&root).unwrap();
    let exe = exe_file.image().unwrap();
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let groups = ddhost::spawns::group_list(&vfs, 200).unwrap();
    let world = ddspawn::World {
        scene: 1100,
        hour: 12,
        flag: &|_| false,
    };
    let mut actors = ddhost::ambient::Population::default();
    actors
        .admit_cells(&vfs, &exe, 200, &groups, &[(0, 0), (0, 0)], &world, &[])
        .unwrap();
    assert_eq!(actors.iter().count(), 19);
    let initial_positions: std::collections::BTreeMap<_, _> =
        actors.iter().map(|a| (a.id, a.position)).collect();
    let terrain = ddhost::install::stage_collision(&vfs, 200).unwrap();
    let mut sim = ddsim::Sim::new(Default::default());
    sim.add_player(1, [3000.0, 0.0, 0.0], 0.0);
    sim.add_player(2, [5000.0, 0.0, 0.0], 0.0);
    for _ in 0..300 {
        actors.step(1.0, &sim.players, &terrain);
    }
    assert_eq!(
        actors.diagnostics.len(),
        17,
        "only bird AI remains explicitly unresolved: {:?}",
        actors.diagnostics
    );
    let ground: Vec<_> = actors
        .iter()
        .filter(|a| a.data.species == ddenemyrt::ambient::Species::Em8300)
        .collect();
    assert_eq!(ground.len(), 2);
    for actor in &ground {
        let [x, y, z] = actor.position;
        let height = terrain.height(x, z, y + 50.0).unwrap();
        println!("{:?}: position {:?}, ground {height}", actor.id, actor.position);
        assert!((y - height).abs() <= 5.0);
        assert!(actor.proposed_position.is_none());
        assert!(!actors.diagnostics.contains_key(&actor.id));
    }
    let mut actions: Vec<_> = ground.iter().map(|a| a.runtime.action).collect();
    actions.sort();
    assert_eq!(actions, vec![0x0100_0003, 0x0100_0004]);
    assert!(ground.iter().all(|a| a.runtime.frame > 1.0));

    // Missing scenery leaves accepted positions intact while the animation
    // clock continues. Rejected travel must not become a later catch-up jump.
    let accepted: std::collections::BTreeMap<_, _> = actors
        .iter()
        .filter(|a| a.data.species == ddenemyrt::ambient::Species::Em8300)
        .map(|a| (a.id, a.position))
        .collect();
    for _ in 0..3 {
        actors.step(1.0, &sim.players, &ddworld::Terrain::empty());
        for actor in actors.iter().filter(|a| accepted.contains_key(&a.id)) {
            assert_eq!(actor.position, accepted[&actor.id]);
            assert!(actor.proposed_position.is_some());
            assert!(actors.diagnostics[&actor.id].contains("no ground"));
        }
    }

    let (id, lifetime, position) = actors
        .iter()
        .find(|a| a.data.species == ddenemyrt::ambient::Species::Em8300)
        .map(|a| (a.id, a.lifetime, a.position))
        .unwrap();
    let before = actors.digest();
    let changed = [position[0] + 1.0, position[1], position[2]];
    assert!(actors.set_position(id, lifetime, changed));
    assert_ne!(before, actors.digest());
    actors
        .admit_cells(&vfs, &exe, 200, &groups, &[(0, 0)], &world, &[])
        .unwrap();
    assert_eq!(actors.iter().find(|a| a.id == id).unwrap().position, changed);
    actors.retire_cell(200, (0, 0));
    assert_eq!(actors.iter().count(), 0);
    actors
        .admit_cells(&vfs, &exe, 200, &groups, &[(0, 0)], &world, &[])
        .unwrap();
    assert!(!actors.set_position(id, lifetime, changed));
    assert_eq!(
        actors.iter().find(|a| a.id == id).unwrap().position,
        initial_positions[&id]
    );

    let mut removal = groups.clone();
    let group = removal.groups.iter_mut().find(|g| g.slot == 1).unwrap();
    group.despawn |= 2;
    group.s.0[13] = 2000;
    group.s.0[14] = 3000;
    assert_eq!(actors.remove_groups(200, &removal, &world).len(), 2);
    actors
        .admit_cells(&vfs, &exe, 200, &groups, &[(0, 0)], &world, &[])
        .unwrap();
    assert_eq!(
        actors.iter().count(),
        17,
        "retired group must not immediately respawn"
    );
    actors.retire_stage(200);
    actors
        .admit_cells(&vfs, &exe, 200, &groups, &[(0, 0)], &world, &[])
        .unwrap();
    assert_eq!(
        actors.iter().count(),
        19,
        "new stage controller clears removal latch"
    );
}
