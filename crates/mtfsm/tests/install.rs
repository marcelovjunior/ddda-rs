#[test]
fn whole_install() {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !path.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let v = ddvfs::Vfs::open(path).unwrap();
    let t = mtarc::type_hash("rAIFSM");
    let mut count = 0;
    let mut failures = vec![];
    for p in v.paths_of_type(t) {
        for l in v.locate(p, t).unwrap() {
            count += 1;
            let b = v.read_at(l).unwrap();
            if let Err(e) = mtfsm::Graph::parse(&b) {
                failures.push(format!("{p}: {e}"));
            }
        }
    }
    println!(
        "parsed {count} occurrences, {} paths; {} failures",
        v.paths_of_type(t).count(),
        failures.len()
    );
    assert!(count > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
