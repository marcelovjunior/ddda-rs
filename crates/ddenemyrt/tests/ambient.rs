use ddenemyrt::ambient::Species;

#[test]
fn installed_ambient_resources_and_action_descriptors() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        return;
    }
    let file = ddexe::ExeFile::next_to(&root).unwrap();
    let exe = file.image().unwrap();
    let vfs = ddvfs::Vfs::open(root).unwrap();
    for (species, resources, actions) in [(Species::Em8300, 17, 18), (Species::Em8602, 15, 15)] {
        let res = species.resources(&exe).unwrap();
        assert_eq!(res.len(), resources);
        let model = res.iter().find(|r| r.class == "rModel").unwrap();
        assert!(!vfs
            .read(&model.path, mtarc::type_hash(&model.class))
            .unwrap()
            .is_empty());
        let banks: Vec<_> = res.iter().filter(|r| r.class == "rMotionList").collect();
        assert_eq!(banks.len(), 3);
        let banks: Vec<_> = banks
            .iter()
            .map(|r| ddenemyrt::body::load::bank(&vfs, &r.path).unwrap())
            .collect();
        let definitions = species.actions(&exe).unwrap();
        assert_eq!(definitions.len(), actions);
        for action in definitions.iter().filter(|a| {
            matches!(
                a.class.as_str(),
                "cEm8300ActWait" | "cEm8600ActWait" | "cEm8600ActWaitAir"
            )
        }) {
            let bank = (action.argument >> 8) as usize;
            let slot = (action.argument & 255) as usize;
            assert!(
                banks
                    .get(bank)
                    .and_then(|b| b.list.motions.get(slot))
                    .and_then(Option::as_ref)
                    .is_some(),
                "{species:?} {} motion {:#x}",
                action.class,
                action.argument
            );
        }
        let tables =
            ddai::thinktbl::Tables::read_roots(&exe, vec![species.default_think_root()], &[]).unwrap();
        assert!(tables.len() > 0);
    }
    let bird = Species::Em8602.resources(&exe).unwrap();
    let motions: Vec<_> = bird.iter().filter(|r| r.class == "rMotionList").collect();
    assert_eq!(
        motions[0].path, motions[1].path,
        "8602 deliberately shares common bank in slots 0 and 1"
    );
}
