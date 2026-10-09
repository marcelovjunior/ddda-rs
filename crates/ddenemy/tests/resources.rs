use ddenemy::{resources::*, Region, RegionStatus};
#[test]
fn variants_alias_shells_and_reject_untraced_kinds() {
    let normal = GoblinResources::select(5).unwrap();
    let variant = GoblinResources::select(6).unwrap();
    assert_eq!(normal.requests.len(), 48);
    assert_eq!(variant.requests.len(), 49);
    assert_eq!(normal.equipment_requests.len(), 12);
    assert_eq!(normal.shells, variant.shells);
    assert_ne!(normal.common, variant.common);
    assert_eq!(variant.common.path, r"charparam\em\em0101_cmn");
    assert_eq!(variant.requests[34], variant.regions);
    assert_eq!(normal.requests[34], normal.regions);
    assert_eq!(normal.requests[0], variant.requests[0]);
    assert_eq!(normal.requests[1..6], variant.requests[1..6]);
    assert_eq!(normal.requests[12], normal.requests[20]);
    for kind in 0..=255 {
        assert_eq!(GoblinResources::select(kind).is_some(), kind == 5 || kind == 6);
    }
}
#[test]
fn region_lookup_is_first_exact_match_without_fallback() {
    let status = RegionStatus {
        regions: vec![
            Region {
                no: 8,
                kind: 3,
                elements: vec![],
            },
            Region {
                no: 4,
                kind: 0,
                elements: vec![],
            },
            Region {
                no: 9,
                kind: 0,
                elements: vec![],
            },
        ],
    };
    assert_eq!(region_by_type(&status, 0).unwrap().no, 4);
    assert_eq!(region_by_type(&status, 3).unwrap().no, 8);
    assert!(region_by_type(&status, 1).is_none());
    assert!(region_by_type(&RegionStatus { regions: vec![] }, 0).is_none());
}
#[test]
fn selected_installed_bundles() {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = std::path::PathBuf::from(
        explicit
            .clone()
            .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into()),
    );
    if !root.is_dir() {
        assert!(explicit.is_none(), "explicit install path absent");
        eprintln!("install absent; skipped");
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    for kind in [5, 6] {
        let bundle = GoblinResources::select(kind).unwrap();
        let mut missing = Vec::new();
        let mut occurrences = 0;
        for resource in bundle
            .requests
            .iter()
            .chain(&bundle.equipment_requests)
            .chain([&bundle.status])
        {
            occurrences += resource.alternatives(&vfs).len();
            if resource.alternatives(&vfs).is_empty() {
                missing.push((resource.class, resource.path.clone()));
            }
        }
        println!(
            "kind={kind} requests={}+{} located occurrences={occurrences} missing={missing:?}",
            bundle.requests.len(),
            bundle.equipment_requests.len()
        );
        for resource in [
            &bundle.common,
            &bundle.regions,
            &bundle.adjust,
            &bundle.shells,
            &bundle.status,
        ] {
            let alternatives = resource.alternatives(&vfs);
            assert!(!alternatives.is_empty(), "{} absent", resource.path);
            for loc in alternatives {
                let bytes = vfs.read_at(loc).unwrap();
                match resource.class {
                    "rPropParam" => {
                        let p = mtprp::parse(&bytes).unwrap();
                        ddenemy::CommonStats::from_prop(&p).unwrap();
                    }
                    "rRegionStatus" => {
                        let s = RegionStatus::parse(&bytes).unwrap();
                        let selected = region_by_type(&s, 0).unwrap();
                        println!(
                            "kind={kind} RST region={} type={} element IDs={:?}",
                            selected.no,
                            selected.kind,
                            selected.elements.iter().map(|e| (e.no, e.id)).collect::<Vec<_>>()
                        );
                    }
                    "rAdjustParam" => {
                        ddenemy::AdjustParam::parse(&bytes).unwrap();
                    }
                    "rShlParamList" => {
                        let s = ddenemy::ShellParamList::parse(&bytes).unwrap();
                        println!(
                            "kind={kind} shell collision={}",
                            String::from_utf8_lossy(&s.collision_path)
                        );
                    }
                    "rStatusParam" => {
                        ddenemy::StatusParam::parse(&bytes).unwrap();
                    }
                    _ => unreachable!(),
                }
            }
        }
        assert!(missing.is_empty(), "selected resource references absent");
    }
}
