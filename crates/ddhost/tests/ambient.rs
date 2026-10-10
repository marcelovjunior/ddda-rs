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
}
