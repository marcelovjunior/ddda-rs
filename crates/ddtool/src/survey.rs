//! Read-only, reproducible resource counts and sample structures.

use std::path::Path;

pub fn survey(dir: &Path, class: Option<&str>) -> Result<(), String> {
    if let Some(name) = class {
        if !mtarc::RESOURCE_CLASSES.contains(&name) {
            return Err(format!("unknown resource class {name}"));
        }
    }
    let vfs = ddvfs::Vfs::open(dir)?;
    println!(
        "archives\t{}\nunique_resources\t{}",
        vfs.archives.len(),
        vfs.len()
    );
    println!("class\tunique_paths\tarchive_entries\tfirst_path");
    for &name in mtarc::RESOURCE_CLASSES {
        if class.is_some_and(|selected| selected != name) {
            continue;
        }
        let hash = mtarc::type_hash(name);
        let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
        paths.sort_unstable();
        let copies: usize = paths.iter().map(|p| vfs.locate(p, hash).unwrap().len()).sum();
        println!(
            "{name}\t{}\t{copies}\t{}",
            paths.len(),
            paths.first().copied().unwrap_or("-")
        );
        if class.is_none() {
            continue;
        }
        if let Some(&path) = paths.first() {
            let bytes = vfs.read(path, hash)?;
            println!("sample_bytes\t{}", bytes.len());
            println!("sample_prefix\t{:02x?}", &bytes[..bytes.len().min(32)]);
            let payload = if bytes.starts_with(b"XFS\0") {
                Some(bytes.as_slice())
            } else if bytes.starts_with(b"PRPZ") && bytes.get(12..16) == Some(b"XFS\0") {
                Some(&bytes[12..])
            } else {
                None
            };
            if let Some(payload) = payload {
                let x = mtxfs::parse(payload).map_err(|e| format!("{path}: {e}"))?;
                println!("xfs_objects\t{}", x.object_count);
                for definition in &x.classes {
                    println!("xfs_class\t{}", definition.name().unwrap_or("unknown"));
                    for property in &definition.properties {
                        println!("property\t{}\t{:?}", property.name, property.ty);
                    }
                }
            }
        }
        for path in paths {
            println!("path\t{path}");
        }
    }
    Ok(())
}
