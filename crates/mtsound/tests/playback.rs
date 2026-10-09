use mtsound::{decode, Package, StreamRequests};

#[test]
fn installed_ev10_streams_and_adpcm() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !std::path::Path::new(&root).is_dir() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none());
        eprintln!("install absent; skipped");
        return;
    }
    let v = ddvfs::Vfs::open(&root).unwrap();
    for (path, prepare) in [
        (r"sound\stream\event_bgm\event_bgm", 24),
        (r"sound\stream\event_vo_se\event_vo_se", 203),
    ] {
        let bytes = v.read(path, mtarc::type_hash("rSoundStreamRequest")).unwrap();
        let requests = StreamRequests::parse(&bytes).unwrap();
        let source = requests.event_source(3, prepare).unwrap();
        let loose = mtsound::stream_path(source.path).unwrap();
        let bytes = std::fs::read(std::path::Path::new(&root).join(&loose)).unwrap();
        let pcm = decode::vorbis(&bytes, 40_000_000).unwrap();
        assert!(pcm.samples.iter().any(|s| s.abs() > 0.01));
        let seconds = pcm.samples.len() as f64 / pcm.channels as f64 / pcm.sample_rate as f64;
        assert!(seconds > 50.0 && seconds < 65.0, "{loose}: {seconds}");
        println!("{loose}: {}ch {}Hz {seconds:.3}s", pcm.channels, pcm.sample_rate);
    }
    let bytes = v
        .read(
            r"sound\se\vo_ev\st200ev10_vo_m0\st200ev10_vo_m0",
            mtarc::type_hash("rSoundPackage"),
        )
        .unwrap();
    let package = Package::parse(&bytes).unwrap();
    let wave = package.waves.iter().flatten().next().unwrap();
    assert_eq!(wave.format_tag, 2);
    let pcm = decode::wave(&wave.riff(), 1_000_000).unwrap();
    assert_eq!((pcm.channels, pcm.sample_rate), (wave.channels, wave.sample_rate));
    assert!(pcm.samples.iter().any(|s| s.abs() > 0.01));
    println!("ADPCM: {} samples", pcm.samples.len());
}
