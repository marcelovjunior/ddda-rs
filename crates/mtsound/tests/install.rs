use std::collections::BTreeSet;
#[test]
fn installation() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !std::path::Path::new(&root).is_dir() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none());
        eprintln!("install absent; skipped");
        return;
    }
    let v = ddvfs::Vfs::open(&root).unwrap();
    let mut packages = BTreeSet::new();
    let mut streams = BTreeSet::new();
    for c in ["rSoundPackage", "rSoundRequest", "rSoundStreamRequest"] {
        let t = mtarc::type_hash(c);
        let mut paths: Vec<_> = v.paths_of_type(t).collect();
        paths.sort();
        let mut copies = 0;
        let mut records = 0;
        for p in &paths {
            for l in v.locate(p, t).unwrap() {
                let b = v.read_at(l).unwrap();
                let result: Result<usize, String> = (|| match c {
                    "rSoundPackage" => {
                        let r = mtsound::Package::parse(&b)?;
                        for w in r.waves.iter().flatten() {
                            mtsound::riff_chunks(&w.riff())?;
                        }
                        Ok(r.waves.len())
                    }
                    "rSoundRequest" => {
                        let r = mtsound::Requests::parse(&b)?;
                        for r in &r.requests {
                            if let Some(s) = r.source {
                                packages.insert(s.to_ascii_lowercase());
                            }
                        }
                        Ok(r.requests.len())
                    }
                    _ => {
                        let r = mtsound::StreamRequests::parse(&b)?;
                        for s in &r.sources {
                            streams.insert(mtsound::stream_path(s.path)?);
                        }
                        Ok(r.requests.len())
                    }
                })();
                records += result.unwrap_or_else(|e| panic!("{c} {p}: {e}"));
                copies += 1;
            }
        }
        println!("{c}: {} paths, {copies} copies, {records} records", paths.len());
    }
    let mut missing = Vec::new();
    for p in &packages {
        if v.locate(p, mtarc::type_hash("rSoundPackage")).is_none() {
            missing.push(p)
        }
    }
    println!("package links {} missing {}", packages.len(), missing.len());
    for p in missing.iter().take(12) {
        println!("missing package {p}")
    }
    let mut found = 0;
    let mut absent = Vec::new();
    for p in &streams {
        if std::path::Path::new(&root).join(p).is_file() {
            found += 1
        } else {
            absent.push(p)
        }
    }
    println!(
        "stream links {} found {found} missing {}",
        streams.len(),
        absent.len()
    );
    for p in absent.iter().take(12) {
        println!("missing stream {p}")
    }
    let b = std::fs::read(std::path::Path::new(&root).join("sound/stream/bgm/wave/Bridge.sngw")).unwrap();
    println!("Bridge {:?}", mtsound::validate_vorbis(&b).unwrap());
}
