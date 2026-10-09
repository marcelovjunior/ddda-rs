//! Local-install checks. No fixtures or game bytes are redistributed.
use ddenemy::*;

#[test]
fn every_archive_occurrence() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.is_dir() {
        eprintln!("skipped: nativePC absent; set DDDA_NATIVE_PC to run install validation");
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let mut errors = Vec::new();
    for class in [
        "rPropParam",
        "rRegionStatus",
        "rStatusParam",
        "rShlParamList",
        "rAdjustParam",
    ] {
        let hash = mtarc::type_hash(class);
        let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
        paths.sort_unstable();
        let mut count = 0;
        let mut common = 0;
        for path in &paths {
            for loc in vfs.locate(path, hash).unwrap() {
                count += 1;
                let result = vfs.read_at(loc).and_then(|d| match class {
                    "rPropParam" => mtprp::parse(&d).and_then(|p| {
                        if p.class_hash == 0x7e50_9fe9 {
                            CommonStats::from_prop(&p)?;
                            common += 1;
                        }
                        Ok(())
                    }),
                    "rRegionStatus" => RegionStatus::parse(&d).map(|_| ()),
                    "rStatusParam" => StatusParam::parse(&d).map(|_| ()),
                    "rShlParamList" => ShellParamList::parse(&d).map(|_| ()),
                    "rAdjustParam" => AdjustParam::parse(&d).map(|_| ()),
                    _ => unreachable!(),
                });
                if let Err(e) = result {
                    errors.push(format!(
                        "{}: {path} {class}: {e}",
                        vfs.archives[loc.archive].display()
                    ));
                }
            }
        }
        eprintln!(
            "{class}: {count} archive occurrences, {} paths, {common} common-stat records",
            paths.len()
        );
        assert!(count > 0, "no {class} resources");
    }
    assert!(
        errors.is_empty(),
        "{} errors:\n{}",
        errors.len(),
        errors.join("\n")
    );

    // Test the path-convention loader against every matching common-stat path,
    // including missing optional per-id resources.
    let mut loaded = 0;
    for path in vfs.paths_of_type(mtarc::type_hash("rPropParam")) {
        if let Some(id) = path
            .strip_prefix("charparam\\em\\")
            .and_then(|s| s.strip_suffix("_cmn"))
        {
            if let Ok(id) = EnemyId::parse(id) {
                let enemy = EnemyData::load(&vfs, id).unwrap();
                assert!(enemy.common.is_some());
                loaded += 1;
            }
        }
    }
    eprintln!("EnemyData: {loaded} matching ids");
    assert!(loaded > 0);
}
