//! Lists install entries whose path contains a substring, with their
//! resource types and archives: `find <needle>`.
fn main() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let needle = std::env::args().nth(1).unwrap().to_ascii_lowercase();
    let mut out = std::collections::BTreeSet::new();
    for a in &vfs.archives {
        let (arc, _) = mtarc::Archive::open(a).unwrap();
        for e in arc.entries {
            if e.path.to_ascii_lowercase().contains(&needle) {
                out.insert(format!(
                    "{}\t{}\t{}",
                    e.type_name().unwrap_or("?"),
                    e.path,
                    a.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    for l in out {
        println!("{l}");
    }
}
