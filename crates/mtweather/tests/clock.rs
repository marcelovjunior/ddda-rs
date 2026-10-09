use mtweather::clock::{hms, step, Frame, State, DAY_MS};

fn state() -> State {
    State {
        milliseconds: 0,
        sync_game_time: false,
        pause: false,
        time_scale: 1.0,
        last_battle_mode: false,
        scenario: 0,
        boss_hp_rate: 1.0,
    }
}
fn frame() -> Frame {
    Frame {
        delta: 1.0,
        base_fps: 30.0,
        main_pause_gate: false,
        event_pause_gate: false,
        saved_milliseconds: 12345,
    }
}

#[test]
fn local_rate_and_fractional_truncation() {
    assert_eq!(step(state(), frame()).unwrap(), 1000);
    let f = Frame {
        delta: 0.3125,
        ..frame()
    };
    assert_eq!(step(state(), f).unwrap(), 312); // 96 fps input, no fractional carry
    let s = State {
        milliseconds: 70_000_001,
        ..state()
    };
    assert_eq!(step(s, f).unwrap(), 70_000_313); // no f32 rounding of stored time
    assert_eq!(
        step(
            state(),
            Frame {
                base_fps: f32::EPSILON,
                ..frame()
            }
        )
        .unwrap(),
        0
    );
}

#[test]
fn saved_clock_copy_precedes_pause_and_scale() {
    let s = State {
        sync_game_time: true,
        pause: true,
        time_scale: f32::NAN,
        ..state()
    };
    assert_eq!(step(s, frame()).unwrap(), 12345);
    assert_eq!(hms(DAY_MS + 3_723_999), [1, 2, 3]);
}

#[test]
fn local_and_battle_pause_gates_differ() {
    assert_eq!(
        step(
            State {
                pause: true,
                ..state()
            },
            frame()
        )
        .unwrap(),
        0
    );
    assert_eq!(
        step(
            state(),
            Frame {
                main_pause_gate: true,
                event_pause_gate: true,
                ..frame()
            }
        )
        .unwrap(),
        1000
    );
    for battle in [
        State {
            scenario: 8800,
            pause: true,
            ..state()
        },
        State {
            last_battle_mode: true,
            pause: true,
            ..state()
        },
    ] {
        assert_eq!(step(battle, frame()).unwrap(), 1000);
        assert_eq!(
            step(
                battle,
                Frame {
                    event_pause_gate: true,
                    ..frame()
                }
            )
            .unwrap(),
            0
        );
        assert_eq!(
            step(
                battle,
                Frame {
                    main_pause_gate: true,
                    ..frame()
                }
            )
            .unwrap(),
            0
        );
    }
}

#[test]
fn wrap_is_before_truncated_add_in_both_directions() {
    assert_eq!(
        step(
            State {
                milliseconds: DAY_MS - 1000,
                ..state()
            },
            frame()
        )
        .unwrap(),
        0
    );
    assert_eq!(
        step(
            State {
                milliseconds: 500,
                time_scale: -1.0,
                ..state()
            },
            frame()
        )
        .unwrap(),
        DAY_MS - 500
    );
    assert_eq!(
        step(
            State {
                milliseconds: 1000,
                time_scale: -1.0,
                ..state()
            },
            frame()
        )
        .unwrap(),
        DAY_MS // -1000.000061... wraps before the increment truncates to -1000.
    );
}

#[test]
fn battle_multiplier_and_hp_boundaries() {
    let s = State {
        scenario: 8800,
        sync_game_time: true,
        ..state()
    };
    assert_eq!(step(s, frame()).unwrap(), 24000);
    for (hp, cap) in [
        (1.0, 16_200_000),
        (0.8, 46_800_000),
        (0.6, 75_600_000),
        (0.4, 86_399_000),
        (0.0, 86_399_000),
    ] {
        assert_eq!(
            step(
                State {
                    milliseconds: cap - 100,
                    boss_hp_rate: hp,
                    ..s
                },
                frame()
            )
            .unwrap(),
            cap
        );
    }
    // A paused clock beyond its cap is still clamped.
    assert_eq!(
        step(
            State {
                milliseconds: 20_000_000,
                ..s
            },
            Frame {
                event_pause_gate: true,
                ..frame()
            }
        )
        .unwrap(),
        16_200_000
    );
}

#[test]
fn unsupported_inputs_are_explicit() {
    assert!(step(
        State {
            milliseconds: DAY_MS + 1,
            ..state()
        },
        frame()
    )
    .is_err());
    assert!(step(
        State {
            time_scale: f32::INFINITY,
            ..state()
        },
        frame()
    )
    .is_err());
    assert!(step(
        State {
            time_scale: -1.0,
            scenario: 8800,
            sync_game_time: true,
            ..state()
        },
        frame()
    )
    .is_err());
    assert!(step(
        state(),
        Frame {
            delta: 100_000.0,
            ..frame()
        }
    )
    .is_err());
}

/// Read-only resource-chain regression. This is data sampling, not a live-game
/// or rendered primary-light comparison. SDL layout: loader 0x00db8e50.
#[test]
#[ignore = "requires DDDA_NATIVE_PC"]
fn installed_st200_chain_and_samples() {
    let root = std::env::var_os("DDDA_NATIVE_PC").expect("set DDDA_NATIVE_PC");
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let read = |path, class| vfs.read(path, mtarc::type_hash(class)).unwrap();
    let bytes = read("scr/st200/sdl/st200_sky", "rScheduler");
    let u32_at = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!(&bytes[..4], b"SDL\0");
    assert_eq!(u16::from_le_bytes(bytes[4..6].try_into().unwrap()), 19);
    let strings = u32_at(20) as usize;
    let text = |offset: usize| {
        let end = bytes[offset..].iter().position(|&b| b == 0).unwrap() + offset;
        std::str::from_utf8(&bytes[offset..end]).unwrap()
    };
    for track in [3usize, 24] {
        let offset = 24 + track * 24;
        assert_eq!(bytes[offset], 11); // resource
        assert_eq!(text(strings + u32_at(offset + 8) as usize), "mpResource");
        let value_offset = u32_at(offset + 20) as usize;
        let resource = strings + u32_at(value_offset) as usize;
        assert_eq!(u32_at(resource), mtarc::type_hash("rSky"));
        assert_eq!(text(resource + 4), "scr\\sky\\st_all_sky_F");
        mtweather::Sky::parse(&read(text(resource + 4), "rSky")).unwrap();
    }
    let light_track = 24 + 80 * 24;
    assert_eq!(bytes[light_track], 2);
    assert_eq!(u32_at(light_track + 12), mtarc::type_hash("uSkyInfiniteLight"));
    for (track, value) in [(83, 0.1f32), (86, 3.0), (87, 3.6), (88, 1.26)] {
        let offset = 24 + track * 24;
        assert_eq!(bytes[offset], 8);
        let values = u32_at(offset + 20) as usize;
        assert_eq!(f32::from_bits(u32_at(values)), value);
    }
    let light = read("scr/st200/sdl/st200_light", "rScheduler");
    assert_eq!(
        u32::from_le_bytes(light[12..16].try_into().unwrap()) & 0x00ff_ffff,
        86400
    );
    let fog = mtweather::WeatherFog::parse(&read("scr/st200/etc/st200_fog_f", "rWeatherFogParam")).unwrap();
    let effects =
        mtweather::WeatherEffect::parse(&read("effect/wep/w_st200_f", "rWeatherEffectParam")).unwrap();
    for (hour, expected_rgb, expected_rgba) in [
        (6, [0.122360006, 0.18728134, 0.24553332], [254, 129, 17, 255]),
        (12, [0.188254, 0.347661, 0.607746], [255, 83, 0, 255]),
        (23, [0.005, 0.005, 0.005], [0, 212, 255, 255]),
    ] {
        let fog_value = fog.sample(hour * 60).unwrap();
        for (actual, expected) in fog_value.color.into_iter().zip(expected_rgb) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(effects.sample(0, hour * 60).unwrap().color, expected_rgba);
        println!(
            "st200 fine {hour}:00 fog={:?}; WEP correction0={:?}",
            fog.sample(hour * 60).unwrap(),
            effects.sample(0, hour * 60).unwrap()
        );
    }
}
