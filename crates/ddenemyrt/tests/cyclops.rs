use ddenemyrt::cyclops::*;

#[test]
fn installed_cyclops_catalogue_and_controller_roots() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !native.is_dir() {
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let catalogue = Catalogue::read(&exe).unwrap();
    assert_eq!(catalogue.actions.len(), 115);
    assert_eq!(catalogue.resources.len(), 43);
    assert_eq!(catalogue.hit_to_pool.len(), 13);
    let motion_banks: Vec<_> = catalogue
        .resources
        .iter()
        .filter(|resource| resource.class == "rMotionList" && resource.flags & 1 != 0)
        .map(|resource| resource.path.as_str())
        .collect();
    assert_eq!(
        motion_banks,
        [
            r"motion\em\e50\e5000_co\e5000_co",
            r"motion\em\e50\e5000_at\e5000_at",
            r"motion\em\e50\e5000_dm\e5000_dm"
        ]
    );
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let groups = ddspawn::gpl::GroupList::parse(
        &vfs.read(
            r"scr\st100\etc\st100_e",
            mtarc::type_hash("rLayoutGroupParamList"),
        )
        .unwrap(),
    )
    .unwrap();
    let group = groups.group(20).unwrap();
    assert_eq!(group.layout_flag, Some(30));
    assert_eq!(group.scene_window, None);
    assert_eq!(group.despawn, 1);
    assert!(group.units.iter().any(|unit| unit.name == "em5000"));
    assert!(group.cells.iter().any(|cell| cell.m == 63 && cell.n == 52));
    let placements = mtlot::parse(
        &vfs.read(r"scr\st100\etc\st100_63m52n_e20", mtarc::type_hash("rLayout"))
            .unwrap(),
    )
    .unwrap();
    let cyclops = placements.iter().find(|record| record.id == 0).unwrap();
    assert_eq!(cyclops.class, "cSetInfoEnemy5000");
    assert_eq!(cyclops.get("覚者の証サイクロ"), Some(&mtlot::Value::Int(1)));
    assert_eq!(
        cyclops.get("覚者の証サイクロのときの残りHPの倍率"),
        Some(&mtlot::Value::F32(0.2))
    );
    assert_eq!(cyclops.get("mUseFirstSetThinkTbl"), Some(&mtlot::Value::Int(0)));
    for resource in &catalogue.resources {
        assert!(
            vfs.locate(&ddvfs::key(&resource.path), mtarc::type_hash(&resource.class))
                .is_some(),
            "missing {} {}",
            resource.class,
            resource.path
        );
    }
    for controller in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)] {
        let initialization = ThinkInitialization::read(&exe, controller).unwrap();
        let selected = controller
            .map(|index| exe.u32(0x0173_6ee8 + index * 4).unwrap())
            .unwrap_or(0x0173_4b48);
        assert_eq!(
            (
                initialization.current,
                initialization.base,
                initialization.default
            ),
            (selected, selected, 0x0173_4b48)
        );
        assert!(initialization.tables.get(selected).is_ok());
        let mut manager = initialization.manager(2.0);
        assert_eq!(manager.lock, controller.is_some());
        assert_eq!(manager.scale, 2.0);
        manager.reset_to_base();
        assert_eq!((manager.table, manager.root), (selected, selected));
    }
    assert!(ThinkInitialization::read(&exe, Some(6)).is_err());
    assert!(ThinkInitialization::read(&exe, Some(u32::MAX)).is_err());
}
