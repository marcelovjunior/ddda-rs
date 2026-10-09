use mtweather::*;

fn word(b: &mut Vec<u8>, x: u32) {
    b.extend(x.to_le_bytes());
}
fn float(b: &mut Vec<u8>, x: f32) {
    word(b, x.to_bits());
}
fn wep() -> Vec<u8> {
    let mut b = b"wep\0".to_vec();
    word(&mut b, 1);
    for count in [2, 0, 0, 0, 0, 0] {
        word(&mut b, count);
    }
    for (hour, color, value) in [(0, [0, 255, 10, 255], 0.), (12, [255, 0, 20, 255], 8.)] {
        word(&mut b, hour);
        word(&mut b, 0);
        b.extend(color);
        for _ in 0..4 {
            float(&mut b, value);
        }
        b.extend(color);
        for _ in 0..3 {
            float(&mut b, value);
        }
    }
    b
}
fn wfp() -> Vec<u8> {
    let mut b = b"wfp\0".to_vec();
    word(&mut b, 1);
    word(&mut b, 2);
    for hour in [6, 18] {
        word(&mut b, hour);
        word(&mut b, 0);
        for value in [hour as f32, 99., 100., 200., 1., 2., 3.] {
            float(&mut b, value);
        }
    }
    b
}
fn sky() -> Vec<u8> {
    let mut b = b"SKY ".to_vec();
    word(&mut b, 6);
    // 4+1+4+4+1 floats, two integers, 8+3 floats.
    for _ in 0..27 {
        word(&mut b, 0);
    }
    b.extend(b"sun\0");
    for _ in 0..(16 + 5 + 4 + 1 + 3 + 1) {
        float(&mut b, 0.);
    }
    b.extend(b"moon\0");
    b
}
#[test]
fn effect_interpolation_and_midnight() {
    let file = WeatherEffect::parse(&wep()).unwrap();
    let noon = file.sample(0, 720).unwrap();
    assert_eq!(noon.intensity, 8.);
    let six = file.sample(0, 360).unwrap();
    assert_eq!(six.intensity, 4.);
    assert_eq!(six.color, [127, 127, 15, 255]);
    assert_eq!(six.shadow_color, six.color);
    assert_eq!(file.sample(0, 1080).unwrap().intensity, 4.);
    assert_eq!(file.sample(0, 1440).unwrap().intensity, 0.);
    assert!(file.sample(1, 0).is_none());
    assert!(file.sample(6, 0).is_none());
    let quantized = file.sample(0, 240).unwrap();
    assert_eq!(quantized.color, [84, 170, 13, 255]);
    assert_eq!(correction_list(0), None);
    assert_eq!(correction_list(10), None);
    for (id, list) in [
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 1),
        (5, 1),
        (6, 2),
        (7, 3),
        (8, 4),
        (9, 5),
    ] {
        assert_eq!(correction_list(id), Some(list));
    }
}
#[test]
fn fog_interpolation_clamps_and_preserves_unused_exponent() {
    let file = WeatherFog::parse(&wfp()).unwrap();
    assert_eq!(file.keys[0].exponent_density, 99.);
    assert_eq!(file.sample(0).unwrap().density, 6.);
    assert_eq!(file.sample(720).unwrap().density, 12.);
    assert_eq!(file.sample(1439).unwrap().density, 18.);
    assert_eq!(file.sample(720).unwrap().color, [1., 2., 3.]);
}
#[test]
fn truncation_counts_versions_and_trailing_bytes() {
    for (bytes, parse) in [(wep(), WeatherEffect::parse as fn(&[u8]) -> Result<WeatherEffect>)] {
        for n in 0..bytes.len() {
            assert!(parse(&bytes[..n]).is_err(), "prefix {n}");
        }
        let mut b = bytes.clone();
        b.push(0);
        assert!(parse(&b).is_err());
        let mut b = bytes.clone();
        b[4] = 2;
        assert!(parse(&b).is_err());
        let mut b = bytes;
        b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&b).is_err());
    }
    for bytes in [wfp(), sky()] {
        for n in 0..bytes.len() {
            if bytes[0] == b'w' {
                assert!(WeatherFog::parse(&bytes[..n]).is_err());
            } else {
                assert!(Sky::parse(&bytes[..n]).is_err());
            }
        }
    }
}
#[test]
fn bad_time_and_nonfinite_rejected_but_file_order_retained() {
    let mut b = wep();
    b[32..36].copy_from_slice(&25u32.to_le_bytes());
    assert!(WeatherEffect::parse(&b).is_err());
    let mut b = wep();
    b[36..40].copy_from_slice(&60u32.to_le_bytes());
    assert!(WeatherEffect::parse(&b).is_err());
    let mut b = wep();
    b[32..36].copy_from_slice(&20u32.to_le_bytes());
    let file = WeatherEffect::parse(&b).unwrap();
    assert_eq!(file.lists[0][0].time.hour, 20);
    assert_eq!(file.lists[0][1].time.hour, 12);
    assert_eq!(file.sample(0, 900).unwrap().intensity, 0.);
    let mut b = wep();
    b[44..48].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
    assert!(WeatherEffect::parse(&b).is_err());
    assert!(TimeKey::new(24, 0).is_ok());
    assert!(TimeKey::new(24, 1).is_err());
}
#[test]
fn sky_full_record_and_bounded_paths() {
    let file = Sky::parse(&sky()).unwrap();
    assert_eq!(file.sun_texture, b"sun");
    assert_eq!(file.moon_texture, b"moon");
    let mut b = sky();
    b.splice(116..120, std::iter::repeat_n(b'x', 259).chain([0]));
    assert!(Sky::parse(&b).is_err());
}
#[test]
#[ignore = "requires DDDA_NATIVE_PC pointing to player's read-only nativePC"]
fn installed_corpus() {
    let root = std::env::var_os("DDDA_NATIVE_PC").expect("set DDDA_NATIVE_PC");
    let vfs = ddvfs::Vfs::open(root).unwrap();
    for class in ["rWeatherEffectParam", "rWeatherFogParam", "rSky"] {
        let hash = mtarc::type_hash(class);
        let mut instances = 0;
        let mut rows = 0;
        let mut backwards = 0;
        let mut duplicates = 0;
        let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
        paths.sort();
        for path in &paths {
            for loc in vfs.locate(path, hash).unwrap() {
                let bytes = vfs.read_at(loc).unwrap();
                let result = match class {
                    "rWeatherEffectParam" => WeatherEffect::parse(&bytes).map(|x| {
                        rows += x.lists.iter().map(Vec::len).sum::<usize>();
                        for (i, list) in x.lists.iter().enumerate() {
                            let n = list.windows(2).filter(|pair| pair[0].time > pair[1].time).count();
                            if n > 0 {
                                println!("backwards WEP keys: {path} list {i}: {n}");
                            }
                            backwards += n;
                            duplicates += list
                                .windows(2)
                                .filter(|pair| pair[0].time == pair[1].time)
                                .count();
                            for minute in 0..1440 {
                                if let Some(v) = x.sample(i, minute) {
                                    assert!(v.intensity.is_finite());
                                }
                            }
                        }
                    }),
                    "rWeatherFogParam" => WeatherFog::parse(&bytes).map(|x| {
                        rows += x.keys.len();
                        backwards += x
                            .keys
                            .windows(2)
                            .filter(|pair| pair[0].time > pair[1].time)
                            .count();
                        duplicates += x
                            .keys
                            .windows(2)
                            .filter(|pair| pair[0].time == pair[1].time)
                            .count();
                        for minute in 0..1440 {
                            if let Some(v) = x.sample(minute) {
                                assert!(v.density.is_finite());
                            }
                        }
                    }),
                    _ => Sky::parse(&bytes).map(|_| ()),
                };
                assert!(
                    result.is_ok(),
                    "{class} {path} {}: {result:?}",
                    vfs.archives[loc.archive].display()
                );
                instances += 1;
            }
        }
        assert!(instances > 0);
        println!(
            "{class}: {} paths, {instances} instances, {rows} rows, {backwards} backwards / {duplicates} duplicate adjacent keys",
            paths.len()
        );
    }
}
