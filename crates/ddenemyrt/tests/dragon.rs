//! Installed-data check; no original gameplay comparison is asserted.
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
