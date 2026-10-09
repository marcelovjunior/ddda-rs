use ddparam::*;
use std::path::PathBuf;
use std::sync::OnceLock;

fn install() -> Option<&'static ddvfs::Vfs> {
    static VFS: OnceLock<Option<ddvfs::Vfs>> = OnceLock::new();
    VFS.get_or_init(|| {
        let path = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
        if !path.exists() {
            eprintln!("SKIP install tests: {} does not exist", path.display());
            None
        } else {
            Some(ddvfs::Vfs::open(path).expect("index install"))
        }
    })
    .as_ref()
}
fn bytes(class: &str, path: &str) -> Option<Vec<u8>> {
    install().map(|v| v.read(path, mtarc::type_hash(class)).unwrap())
}

#[test]
fn every_archive_copy() {
    let Some(vfs) = install() else {
        return;
    };
    let expected = [33, 6, 9, 1, 9, 1, 1, 1];
    for (&class, expected) in RESOURCE_CLASSES.iter().zip(expected) {
        let hash = mtarc::type_hash(class);
        let paths: Vec<_> = vfs.paths_of_type(hash).collect();
        assert_eq!(paths.len(), expected, "{class}: shipped path count changed");
        let mut copies = 0;
        for path in paths {
            for loc in vfs.locate(path, hash).unwrap() {
                let data = vfs.read_at(loc).unwrap();
                Parameter::from_bytes(class, &data).unwrap_or_else(|e| {
                    panic!("{class} {path} in {}: {e}", vfs.archives[loc.archive].display())
                });
                copies += 1;
            }
        }
        println!("{class}: {expected} paths, {copies} archive copies, 0 failures");
    }
}
#[test]
fn stamina_values() {
    let Some(b) = bytes("rPlStamina", r"param\pl\stamina\plstaminadash") else {
        return;
    };
    let p = Stamina::from_bytes(&b).unwrap();
    assert_eq!(p.quality, 2);
    assert_eq!(
        p.entries.iter().map(|r| r.stamina).collect::<Vec<_>>(),
        [1.25, 1.5, 1.75, 2., 2.5, 3.]
    );
    assert_eq!(
        p.entries.iter().map(|r| r.motion_speed).collect::<Vec<_>>(),
        [1.2, 1.15, 1.1, 1., 0.9, 0.8]
    );
    assert!(p.entries.iter().all(|r| r.frame == 1.));
}
#[test]
fn weight_values() {
    let Some(b) = bytes("rPlWeight", r"param\pl\weight\pltotalweightm") else {
        return;
    };
    assert_eq!(
        Weight::from_bytes(&b)
            .unwrap()
            .entries
            .iter()
            .map(|r| r.weight)
            .collect::<Vec<_>>(),
        [0., 85., 95., 115., 125., 135.]
    );
}
#[test]
fn level_values() {
    let Some(b) = bytes("rPlLevel", r"param\pl\level\lvfighter") else {
        return;
    };
    let p = Level::from_bytes(&b).unwrap();
    assert_eq!(
        p.entries[1],
        LevelEntry {
            level: 1,
            hp_max: 450,
            stamina_max: 0,
            attack: 80,
            defend: 80,
            magic_attack: 60,
            magic_defend: 60,
            next_exp: 0
        }
    );
    assert_eq!(p.entries.len(), 6);
    assert_eq!(p.entries[0].level, 0);
}
#[test]
fn exp_values() {
    let Some(b) = bytes("rPlExp", r"param\pl\level\exp") else {
        return;
    };
    let p = Exp::from_bytes(&b).unwrap();
    assert_eq!(p.entries[2], ExpEntry { level: 2, rate: 1.3 });
    assert_eq!(p.entries.len(), 13);
    assert_eq!(p.entries[0], ExpEntry { level: 200, rate: 1. });
}
#[test]
fn job_level_values() {
    let Some(b) = bytes("rPlJobLevel", r"param\pl\joblevel\joblvfighter") else {
        return;
    };
    let p = JobLevel::from_bytes(&b).unwrap();
    assert_eq!(
        p.entries[1],
        JobLevelEntry {
            level: 1,
            next_exp: 500
        }
    );
}
#[test]
fn ability_values() {
    let Some(b) = bytes("rPlAbilityParam", r"param\pl\other\plabilityparam") else {
        return;
    };
    let p = Ability::from_bytes(&b).unwrap();
    assert_eq!(p.entries.len(), 115);
    assert_eq!(
        p.entries[0],
        AbilityEntry {
            float_0: 0.5,
            float_1: 1.,
            signed_0: 0,
            signed_1: 0
        }
    );
    assert_eq!(p.entries[10].signed_0, -1);
}
#[test]
fn defend_values() {
    let Some(b) = bytes("rPlDefendParam", r"param\pl\other\pldefendparam") else {
        return;
    };
    let p = Defend::from_bytes(&b).unwrap();
    assert_eq!(p.entries.len(), 10);
    assert_eq!(
        p.entries[0],
        DefendEntry {
            shrink_max: 100.,
            shrink_speed: 0.3,
            blow_max: 100.,
            blow_speed: 0.3
        }
    );
}
#[test]
fn neck_values() {
    let Some(b) = bytes("rPlNeckPos", r"param\pl\other\plneckpos") else {
        return;
    };
    let p = NeckPos::from_bytes(&b).unwrap();
    assert_eq!(p.entries.len(), 10);
    assert_eq!(p.entries[0].position, [-50., 0., 25., 0.]);
}
