//! Installed-data check; no original gameplay comparison is asserted.
// The module is tested by path until its shared lib.rs export is integrated.
use ddenemyrt::body;
#[path = "../src/dragon.rs"]
mod dragon;

#[test]
fn opening_dragon_definitions_are_loaded_from_the_install() {
    let path = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/DDDA.exe".into());
    if !path.is_file() {
        eprintln!("SKIP dragon definitions: install absent");
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    let exe = ddexe::Exe::parse(&bytes).unwrap();
    let data = dragon::DragonAiData::opening(&exe).unwrap();
    assert_eq!(data.tables.root(0), Some(dragon::OPENING_THINK_ROOT));
    assert_eq!(data.tables.len(), 52);
    assert_eq!(data.action_classes.len(), 93);
    assert_eq!(data.resources.len(), 57);
    assert_eq!(
        data.resources.iter().filter(|r| r.class == "rMotionList").count(),
        5
    );
    assert_eq!(
        data.resources
            .iter()
            .filter(|r| r.class == "rObjCollision")
            .count(),
        3
    );
    assert!(data.resources.iter().all(|r| !r.path.is_empty()));
    assert!(data.resources[..49].iter().all(|r| r.flags == 3));
    assert!(data.resources[49..].iter().all(|r| r.flags == 2));
    assert_eq!(data.resources[4].class, "rModel");
    assert_eq!(data.resources[5].class, "rMotionList");
    assert_eq!(data.resources[44].class, "rRegionStatus");
    // The installed entrance graph's action IDs resolve through this table.
    for (action, class) in [
        (10, "cEm5800TargetFly"),
        (21, "cEm5800BreathWide"),
        (33, "cEm5800PressAttack"),
        (52, "cEm5800HoverUp"),
        (53, "cEm5800KakkuHover"),
        (54, "cEm5800HoverKakku"),
    ] {
        assert_eq!(data.action_classes[action], class);
    }
    // All roots reached by the two nonterminal branches remain resolvable.
    for root in [0x0169_7f38, 0x0169_8b18, 0x0169_5d78] {
        assert_eq!(data.tables.get(root).unwrap().va, root);
    }
}

#[test]
fn registered_dragon_body_contains_the_entrance_motions() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP dragon body: install absent");
        return;
    }
    let exe_file = ddexe::ExeFile::next_to(&native).unwrap();
    let data = dragon::DragonAiData::opening(&exe_file.image().unwrap()).unwrap();
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let body = data.load_body(&vfs, 1.0).unwrap();
    assert!(!body.skeleton.joints.is_empty());
    assert_eq!(body.banks.len(), 5);
    assert_eq!(body.collision.len(), 3);
    // Action vf05/vf06: a6ba90, a6bd80, a6bf00, a60b70, a6aed0,
    // a6af70, a56a30, a56d10. IDs encode bank << 8 | slot.
    for id in [100u16, 121, 103, 126, 298, 299, 263, 264] {
        let motion = body::MotionId::new(((id >> 8) & 0xf) as u8, id & 0xff);
        assert!(body.frames(motion) > 0, "missing entrance motion {id}");
    }
}
