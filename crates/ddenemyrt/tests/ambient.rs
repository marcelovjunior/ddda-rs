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
    assert_eq!(Species::Em8300.think_root(&exe, Some(5)).unwrap(), 0x0165_ce50);
    assert_eq!(Species::Em8300.think_root(&exe, Some(6)).unwrap(), 0x0165_d0d0);
    assert!(Species::Em8300.think_root(&exe, Some(-1)).is_err());
    assert_eq!(
        Species::Em8602.think_root(&exe, Some(6)).unwrap(),
        Species::Em8602.default_think_root()
    );
    for (index, motion) in [(3, 10), (4, 11)] {
        let action = ddenemyrt::ambient::Action::common(&exe, index).unwrap();
        assert_eq!(action.class, "cEmActWait");
        assert_eq!(ddenemyrt::ambient::enter_action(&action).unwrap().id, motion);
    }
    struct NoCallbacks;
    impl ddai::thinktbl::Host for NoCallbacks {
        fn check(&mut self, id: u32, _: u32) -> Result<bool, ddai::Error> {
            panic!("unexpected ambient idle check {id}")
        }
        fn set(&mut self, id: u32, _: u32) -> Result<u32, ddai::Error> {
            panic!("unexpected ambient idle commit {id}")
        }
        fn rand(&mut self) -> u32 {
            0
        }
    }
    for (index, expected) in [(5, 0x0100_0004), (6, 0x0100_0003)] {
        let root = Species::Em8300.think_root(&exe, Some(index)).unwrap();
        let tables = ddai::thinktbl::Tables::read_roots(&exe, vec![root], &[]).unwrap();
        let mut think = ddai::thinktbl::ThinkMgr::new();
        think.request_table(root);
        think.slots[0] = ddai::thinktbl::Slot::measure([0.0; 3], 0.0, [3000.0, 0.0, 0.0], 0.0, 1);
        assert_eq!(think.think(&tables, &mut NoCallbacks).unwrap(), expected);
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    for (species, resources, actions) in [(Species::Em8300, 17, 18), (Species::Em8602, 15, 15)] {
        let res = species.resources(&exe).unwrap();
        assert_eq!(res.len(), resources);
        let model = res.iter().find(|r| r.class == "rModel").unwrap();
        assert!(
            !vfs.read(&model.path, mtarc::type_hash(&model.class))
                .unwrap()
                .is_empty()
        );
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
