use ddskill::magic::MagicList;

#[test]
fn installed_magic_lists_and_cast_parameters() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.exists() {
        eprintln!("SKIP: no installation");
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let ty = mtarc::type_hash(MagicList::RESOURCE_CLASS);
    let mut count = 0;
    for path in vfs.paths_of_type(ty) {
        let bytes = vfs.read(path, ty).unwrap();
        let list = MagicList::parse(&bytes).unwrap();
        assert!(!list.entries.is_empty());
        count += 1;
        if path == MagicList::STAFF_PATH {
            assert_eq!(list.entries.len(), 30);
            for (index, chant, stamina, group) in
                [(0, 30., -80., 4), (2, 50., -90., 6), (10, 140., -120., 14)]
            {
                let m = &list.entries[index];
                assert_eq!(m.levels[0].chant_frames, chant);
                assert_eq!(m.levels[0].stamina, stamina);
                assert_eq!(m.levels[0].shells[0].group, group);
            }
            for n in 0..bytes.len() {
                assert!(MagicList::parse(&bytes[..n]).is_err());
            }
        }
    }
    assert_eq!(count, 4);
}
