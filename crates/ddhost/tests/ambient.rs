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
    let mut sim = ddsim::Sim::new(Default::default());
    sim.add_player(1, [3000.0, 0.0, 0.0], 0.0);
    sim.add_player(2, [5000.0, 0.0, 0.0], 0.0);
    for _ in 0..300 {
        actors.step(1.0, &sim.players);
    }
    assert_eq!(
        actors.diagnostics.len(),
        19,
        "physics and bird AI remain explicitly unresolved: {:?}",
        actors.diagnostics
    );
    let ground: Vec<_> = actors
        .iter()
        .filter(|a| a.data.species == ddenemyrt::ambient::Species::Em8300)
        .collect();
    assert!(
        ground
            .iter()
            .all(|a| actors.diagnostics[&a.id].contains("root travel"))
    );
    assert!(
        ground
            .iter()
            .all(|a| a.proposed_position.is_some_and(|p| p != a.position))
    );
    let mut actions: Vec<_> = ground.iter().map(|a| a.runtime.action).collect();
    actions.sort();
    assert_eq!(actions, vec![0x0100_0003, 0x0100_0004]);
    assert!(ground.iter().all(|a| a.runtime.frame > 1.0));

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
    assert_eq!(actors.iter().find(|a| a.id == id).unwrap().position, position);

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
