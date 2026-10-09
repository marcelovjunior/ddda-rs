#[test]
fn installed_motion_sound_maps_parse_and_preserve_sparse_ids() {
    let root = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.is_dir() { eprintln!("SKIP: no install"); return; }
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let hash = mtarc::type_hash("rSoundMotionSe");
    let mut count = 0;
    for path in vfs.paths_of_type(hash) {
        let bytes = vfs.read(path, hash).unwrap();
        let map = mtsound::motion::MotionRequests::parse(&bytes)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(!map.0.is_empty(), "{path}");
        if path == r"sound\se\pl_com\pl_com_bmse" {
            assert!(map.get(0).unwrap().surface);
            assert_eq!(map.get(0).unwrap().request_type, 12);
            assert!(map.get(u16::MAX).is_none());
            assert_eq!(map.get(13).unwrap().request(0), 10);
        }
        count += 1;
    }
    assert!(count > 0);
    eprintln!("parsed {count} motion sound maps");
    let hash = mtarc::type_hash("rSoundAttributeSe");
    let mut attributes = 0;
    for path in vfs.paths_of_type(hash) {
        let bytes = vfs.read(path, hash).unwrap();
        let map = mtsound::motion::SurfaceRequests::parse(&bytes)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(!map.0.is_empty(), "{path}");
        attributes += 1;
    }
    assert!(attributes > 0);
    eprintln!("parsed {attributes} surface sound maps");
}
