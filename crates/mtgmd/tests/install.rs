//! Optional read-only corpus tests. Set DDDA_NATIVE_PC to test another install.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

#[test]
fn every_archive_instance_and_known_values() {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !root.is_dir() {
        assert!(
            explicit.is_none(),
            "DDDA_NATIVE_PC does not exist: {}",
            root.display()
        );
        eprintln!("skipped: no DDDA install at {}", root.display());
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let hash = mtarc::type_hash("rGUIMessage");
    let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
    paths.sort();
    assert!(!paths.is_empty(), "install has no message resources");
    let mut instances = 0;
    let mut messages = 0;
    let mut labels = 0;
    let mut languages = BTreeMap::<u32, usize>::new();
    let mut distinct = HashSet::<Vec<u8>>::new();
    let mut distinct_messages = 0;
    let mut distinct_labels = 0;
    for path in &paths {
        for location in vfs.locate(path, hash).unwrap() {
            let bytes = vfs.read_at(location).unwrap();
            let file = mtgmd::MessageFile::parse(&bytes)
                .unwrap_or_else(|e| panic!("{}: {path}: {e}", vfs.archives[location.archive].display()));
            instances += 1;
            messages += file.messages.len();
            labels += file.labels.len();
            *languages.entry(file.language).or_default() += 1;
            assert_eq!(file.reserved, [0; 8], "{path}: unknown header data");
            for label in &file.labels {
                assert!(file.message(label.message_index).is_some());
            }
            if !distinct.contains(&bytes) {
                distinct_messages += file.messages.len();
                distinct_labels += file.labels.len();
                distinct.insert(bytes);
            }
        }
    }
    eprintln!("GMD: {} paths, {instances} archive instances, {} distinct payloads; {messages} messages, {labels} labels across instances; {distinct_messages} messages, {distinct_labels} labels across distinct payloads; languages {languages:?}", paths.len(), distinct.len());
    let bytes = vfs.read(r"id\message\item\itemName_eng", hash).unwrap();
    let items = mtgmd::MessageFile::parse(&bytes).unwrap();
    assert_eq!(items.language, 1);
    assert_eq!(items.message(0), Some("Airtight Flask"));
    assert_eq!(items.message(1), Some("Scrag of Beast"));
    let bytes = vfs.read(r"id\DDN\message\common\enemy_name_eng", hash).unwrap();
    let enemies = mtgmd::MessageFile::parse(&bytes).unwrap();
    assert_eq!(enemies.message_index("e0100_goblin"), Some(0));
    assert_eq!(enemies.by_label("e0100_goblin"), Some("Goblins"));
    assert_eq!(enemies.by_label("e0200_wolf"), Some("Wolves"));
}
