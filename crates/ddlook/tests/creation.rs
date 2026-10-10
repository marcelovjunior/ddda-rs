//! S56 uses installed resources, never copies preset or name tables into the repo.
#[test]
fn installed_editor_presets_apply_both_sexes_and_native_names_use_exe_tables() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let presets = ddlook::creation::Presets::load(&vfs).unwrap();
    assert_eq!(presets.all.each_ref().map(Vec::len), [16, 16]);
    for sex in 0..2 {
        for index in 0..16 {
            let mut edit = ddpawn::cmc::Edit {
                fields: [0; 41],
                name: [0; 25],
                nickname: 0,
            };
            presets.apply(&mut edit, sex, index).unwrap();
            let fields: Vec<_> = ddpawn::cmc::EDIT_U8_FIELDS
                .iter()
                .zip(edit.fields)
                .map(|(field, value)| ((*field).to_owned(), u32::from(value)))
                .collect();
            let look = ddlook::save::edit_look_dressed(&vfs, &fields).unwrap();
            assert_eq!(look.sex, sex);
            assert!(!look.base.is_empty());
        }
    }
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let rules = ddlook::creation::NameRules::load(&vfs, &exe.image().unwrap()).unwrap();
    let name = rules.normalize("ABCDEFGHIJKLM");
    assert_eq!(&name[..13], b"ABCDEFGHIJKL\0");
    assert!(std::str::from_utf8(&rules.normalize("あいうえおかきくけ")).is_ok());
}
