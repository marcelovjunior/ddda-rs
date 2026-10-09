#[test]
fn installed_random_tables_and_soil_requests() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !std::path::Path::new(&root).is_dir() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none());
        return;
    }
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let kind = mtarc::type_hash("rSoundRandom");
    let mut count = 0;
    for path in vfs.paths_of_type(kind) {
        let bytes = vfs.read(path, kind).unwrap();
        let random = mtsound::random::RandomRequests::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        count += 1;
        assert!(!random.0.is_empty(), "{path}");
    }
    assert!(count > 0);
    let bytes = vfs.read(r"sound\se\mt\mt_soil\mt_soil", mtarc::type_hash("rSoundRequest")).unwrap();
    let requests = mtsound::Requests::parse(&bytes).unwrap();
    let bytes = vfs.read(requests.random.unwrap(), kind).unwrap();
    let mut random = mtsound::random::RandomRequests::parse(&bytes).unwrap();
    let request = requests.request(5).unwrap();
    assert_eq!(u32::from_le_bytes(request.raw[8..12].try_into().unwrap()), 4);
    let id = u32::from_le_bytes(request.raw[52..56].try_into().unwrap());
    let row = random.row_mut(id).unwrap();
    let mut resolved = 0;
    for draw in 0..100 {
        if let Some(id) = row.draw(draw) {
            let request = requests.single_wave(u16::try_from(id).unwrap()).unwrap();
            assert!(request.source.is_some());
            resolved += 1;
        }
    }
    assert!(resolved > 0);
    eprintln!("{count} random maps parsed; soil request 5 resolves ordinary alternatives");
}
