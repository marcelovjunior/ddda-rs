use ddmap::*;
#[test]
fn every_resource_copy() {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let path = explicit
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !path.is_dir() {
        assert!(explicit.is_none(), "configured install absent");
        eprintln!("SKIP: no local install");
        return;
    }
    let v = ddvfs::Vfs::open(path).unwrap();
    let text_bytes = v
        .read(
            "id/DDN/message/common/map_placelist_eng",
            mtarc::type_hash("rGUIMessage"),
        )
        .unwrap();
    let text = mtgmd::MessageFile::parse(&text_bytes).unwrap();
    let mut referenced = std::collections::BTreeSet::new();
    let mut unnamed = 0;
    for name in ["rMapLndMarkPos", "rStartPos", "rStagePlaceName"] {
        let hash = mtarc::type_hash(name);
        let mut count = 0;
        let mut records = 0;
        for path in v.paths_of_type(hash) {
            for loc in v.locate(path, hash).unwrap() {
                let b = v.read_at(loc).unwrap();
                let ids: Vec<u32> = match name {
                    "rMapLndMarkPos" => Landmarks::parse(&b)
                        .unwrap()
                        .entries
                        .iter()
                        .map(|e| e.name_id)
                        .collect(),
                    "rStagePlaceName" => PlaceNames::parse(&b)
                        .unwrap()
                        .entries
                        .iter()
                        .map(|e| e.message_number as u32)
                        .collect(),
                    _ => vec![],
                };
                for id in ids {
                    if id == u32::MAX {
                        unnamed += 1;
                        continue;
                    }
                    assert!(text.message(id).is_some(), "unresolved {name} {path} id={id}");
                    referenced.insert(id);
                }
                let n = match name {
                    "rMapLndMarkPos" => Landmarks::parse(&b).map(|p| p.entries.len()),
                    "rStartPos" => StartPositions::parse(&b).map(|p| p.entries.len()),
                    _ => PlaceNames::parse(&b).map(|p| p.entries.len()),
                }
                .unwrap_or_else(|e| panic!("{name} {path}: {e}"));
                count += 1;
                records += n;
            }
        }
        assert!(count > 0);
        eprintln!(
            "{name}: {} paths, {count} copies, {records} records",
            v.paths_of_type(hash).count()
        );
    }
    eprintln!(
        "{} distinct ids resolve into {} map messages",
        referenced.len(),
        text.messages.len()
    );
    let hash = mtarc::type_hash("rGUIMessage");
    let mut languages = 0;
    for path in v
        .paths_of_type(hash)
        .filter(|p| p.starts_with("id\\ddn\\message\\common\\map_placelist_"))
    {
        let bytes = v.read(path, hash).unwrap();
        let table = mtgmd::MessageFile::parse(&bytes).unwrap();
        for &id in &referenced {
            assert!(table.message(id).is_some(), "{path}: missing {id}");
        }
        languages += 1;
    }
    assert!(languages > 0);
    eprintln!("{languages} language tables checked, {unnamed} unnamed landmark occurrences");
    let b = v
        .read("scr/st000/etc/st000", mtarc::type_hash("rStartPos"))
        .unwrap();
    let p = StartPositions::parse(&b).unwrap();
    assert_eq!(
        p.entries[0].offset_positions,
        [[0., 0., 2., 0.], [1., 0., 1., 0.], [2., 0., 0., 0.]]
    );
    assert!(Landmarks::parse(&b).is_err());
    for end in 0..b.len() {
        assert!(StartPositions::parse(&b[..end]).is_err(), "truncated start {end}");
    }
}
