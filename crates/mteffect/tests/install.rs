use ddvfs::Vfs;
use mteffect::{EffectList, Provider};
#[test]
fn all_instances() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !std::path::Path::new(&root).exists() {
        assert!(
            std::env::var_os("DDDA_NATIVE_PC").is_none(),
            "explicit install missing"
        );
        eprintln!("install absent; skipped");
        return;
    }
    let v = Vfs::open(root).unwrap();
    for class in ["rEffectProvider", "rEffectList"] {
        let hash = mtarc::type_hash(class);
        let mut paths: Vec<_> = v.paths_of_type(hash).collect();
        paths.sort();
        let mut copies = 0;
        let mut rows = 0;
        let mut refs = 0;
        let mut unresolved = std::collections::BTreeSet::new();
        let mut targets = std::collections::BTreeSet::new();
        let mut events = 0;
        for path in &paths {
            for loc in v.locate(path, hash).unwrap() {
                let b = v.read_at(loc).unwrap();
                copies += 1;
                if class == "rEffectProvider" {
                    let p = Provider::parse(&b).unwrap_or_else(|e| panic!("{path}: {e}"));
                    rows += p.motions.len();
                    for m in &p.motions {
                        assert!(
                            p.element(m.index, m.element).is_some(),
                            "{path}: bad motion link {m:?}"
                        );
                    }
                    for e in &p.events {
                        events += 1;
                        assert!(
                            p.element(e.index, e.element).is_some(),
                            "{path}: bad event link {e:?}"
                        );
                    }
                    for e in p.indices.iter().flatten() {
                        for path in e.lists.iter().filter(|x| !x.is_empty()) {
                            refs += 1;
                            targets.insert(("rEffectList", ddvfs::key(path)));
                        }
                        if !e.effect_2d.is_empty() {
                            refs += 1;
                            targets.insert(("rEffect2D", ddvfs::key(&e.effect_2d)));
                        }
                    }
                } else {
                    let e = EffectList::parse(&b).unwrap_or_else(|e| panic!("{path}: {e}"));
                    rows += e.units.len();
                    refs += e.resources.len();
                    for r in e.resources {
                        targets.insert((r.class, ddvfs::key(&r.path)));
                    }
                }
            }
        }
        for (class, path) in &targets {
            if v.locate(path, mtarc::type_hash(class)).is_none()
                && !(*class == "rTexture"
                    && v.locate(path, mtarc::type_hash("rRenderTargetTexture")).is_some())
            {
                unresolved.insert(format!("{class} {path}"));
            }
        }
        println!("{class}: {} paths {copies} copies {rows} rows {events} events {refs} references {} unique targets {} unresolved: {:?}",paths.len(),targets.len(),unresolved.len(),unresolved.iter().take(20).collect::<Vec<_>>());
        assert!(copies > 0, "empty {class} corpus");
        assert!(
            unresolved.is_empty(),
            "unresolved {class} resource links: {unresolved:?}"
        );
    }
    // cPlActAssassinAttack::vf05 (0x00b28d00) requests 0x723; custom
    // motion table at 0x014f1fb0 supplies bank 7 m0005_at_19.
    let motion = mtlmt::parse(
        &v.read(
            "motion/pl/m/m00/m0005_at/m0005_at_19",
            mtarc::type_hash("rMotionList"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(motion.motions[0x23].as_ref().unwrap().frame_count, 91);
    let provider = Provider::parse(
        &v.read("effect/epv/pl/p_pl_dag", mtarc::type_hash("rEffectProvider"))
            .unwrap(),
    )
    .unwrap();
    let links: Vec<_> = provider.motions.iter().filter(|m| m.motion == 0x723).collect();
    assert_eq!(links.len(), 3);
    assert_eq!((links[0].index, links[0].element), (19, 1));
    assert_eq!(links[0].start_frame, 1.);
    assert_eq!(
        (links[1].start_frame, links[1].end_frame, links[1].frame_parameter),
        (0.1, 15., 8.)
    );
    assert_eq!(links[2].start_frame, 42.);
    let element = provider.element(19, 1).unwrap();
    assert_eq!(element.lists[0], "effect\\efl\\sk\\dag\\l_c_dag020_02");
    let efl = EffectList::parse(
        &v.read(&element.lists[0], mtarc::type_hash("rEffectList"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(efl.units.len(), 28);
    assert_eq!(efl.resources.len(), 51);
    assert!(efl
        .resources
        .iter()
        .any(|r| r.class == "rTexture" && r.path == "effect\\tex\\cm\\t_cm000_00_GM"));
    assert!(efl
        .resources
        .iter()
        .any(|r| r.class == "rModel" && r.path == "effect\\mod\\m_cm001_10\\m_cm001_10_02"));
    for r in efl.resources {
        assert!(v.locate(&r.path, mtarc::type_hash(r.class)).is_some());
    }
}
