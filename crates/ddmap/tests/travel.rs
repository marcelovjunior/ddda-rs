use ddmap::travel::*;
use ddmap::{StartPosition, StartPositions};

#[test]
fn selector_scenario_boundary_and_domain() {
    for (selector, scenario, stage, start) in [
        (0, 7799, 220, 13),
        (0, 7800, 230, 8),
        (1, 0, 100, 56),
        (2, 9000, 400, 7),
        (3, 0, 999, 0),
        (12, 0, 999, 9),
    ] {
        assert_eq!(
            ferrystone_request(selector, scenario),
            Some(StageRequest { stage, start })
        );
    }
    for selector in [-1, 13, i32::MIN, i32::MAX] {
        assert!(ferrystone_request(selector, 0).is_none());
    }
}

#[test]
fn saved_slot_resolution_preserves_original_edge_cases() {
    let slots = [100, 0, 220, 230, 400, 500, 600, 700, 800, 900];
    let resolve = |start| resolve_stage(StageRequest { stage: 999, start }, &slots).unwrap();
    assert_eq!(resolve(0).stage, 100);
    assert_eq!(resolve(1).stage, 0); // No invented occupied-slot validation.
    assert_eq!(resolve(9).stage, 900);
    assert_eq!(resolve(10).stage, 10);
    assert_eq!(resolve(256).stage, 100); // Executable truncates only the lookup byte.
    assert_eq!(resolve(256).start, 256);
    assert_eq!(resolve(-1).source, ArrivalSource::Portcrystal(255));
    assert!(resolve_stage(StageRequest { stage: -1, start: 0 }, &slots).is_none());
}

#[test]
fn arrival_seed_selects_position_angle_and_rejects_missing_data() {
    let starts = StartPositions {
        quality: 0,
        entries: vec![StartPosition {
            position: [12., 34., 56., 99.],
            angle: 0.75,
            offset_positions: vec![[1., 2., 3., 0.]; 3],
            offset_angles: vec![0.; 3],
        }],
    };
    let mut positions = [[0.; 4]; 10];
    positions[2] = [70., 80., 90., 123.];
    let fixed = ResolvedStage {
        stage: 220,
        start: -1,
        source: ArrivalSource::StartPosition,
    };
    assert_eq!(
        arrival_seed(fixed, Some(&starts), &positions),
        Some(ArrivalSeed {
            position: [12., 34., 56.],
            angle: 0.75
        })
    );
    assert!(arrival_seed(fixed, None, &positions).is_none());
    assert!(arrival_seed(ResolvedStage { start: 1, ..fixed }, Some(&starts), &positions).is_none());
    let placed = ResolvedStage {
        stage: 100,
        start: 2,
        source: ArrivalSource::Portcrystal(2),
    };
    assert_eq!(
        arrival_seed(placed, None, &positions),
        Some(ArrivalSeed {
            position: [70., 80., 90.],
            angle: 0.
        })
    );
    assert!(arrival_seed(
        ResolvedStage {
            source: ArrivalSource::Portcrystal(10),
            ..placed
        },
        None,
        &positions
    )
    .is_none());
}

#[test]
fn installed_builtin_arrival_rows_exist() {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.is_dir() {
        assert!(explicit.is_none(), "configured install absent");
        eprintln!("SKIP: no local install");
        return;
    }
    let v = ddvfs::Vfs::open(root).unwrap();
    for (selector, scenario) in [(0, 7799), (0, 7800), (1, 0), (2, 0)] {
        let request = ferrystone_request(selector, scenario).unwrap();
        let path = format!("scr/st{0:03}/etc/st{0:03}", request.stage);
        let bytes = v.read(&path, mtarc::type_hash("rStartPos")).unwrap();
        let starts = StartPositions::parse(&bytes).unwrap();
        let resolved = resolve_stage(request, &[0; 10]).unwrap();
        let seed = arrival_seed(resolved, Some(&starts), &[[0.; 4]; 10]).unwrap();
        assert!(seed.position.iter().all(|x| x.is_finite()) && seed.angle.is_finite());
        eprintln!(
            "selector {selector}, scenario {scenario}: stage {} row {} exists",
            request.stage, request.start
        );
    }
}
