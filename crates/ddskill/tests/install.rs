//! Checks against the player's install (DDDA_NATIVE_PC, default the
//! maintainer's path). Skips with a message when the directory is absent.
use ddskill::growth::{self, Progress};
use ddskill::skill_list::SkillList;
use ddskill::vocation::Vocation;
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

fn read(class: &str, path: &str) -> Vec<u8> {
    install().unwrap().read(path, mtarc::type_hash(class)).unwrap()
}

#[test]
fn skill_list_every_copy() {
    let Some(vfs) = install() else { return };
    let hash = mtarc::type_hash(SkillList::RESOURCE_CLASS);
    let paths: Vec<_> = vfs.paths_of_type(hash).collect();
    assert_eq!(paths.len(), 1);
    let mut copies = 0;
    for loc in vfs.locate(SkillList::PATH, hash).unwrap() {
        let data = vfs.read_at(loc).unwrap();
        let list = SkillList::from_bytes(&data).unwrap();
        assert_eq!(list.to_bytes(), data, "re-encoding must give the same bytes");
        copies += 1;
    }
    assert_eq!(copies, 2);
}

#[test]
fn skill_list_values() {
    if install().is_none() {
        return;
    }
    let list = SkillList::from_bytes(&read("rSkillList", SkillList::PATH)).unwrap();
    assert_eq!((list.core.len(), list.weapon.len(), list.augments.len()), (18, 262, 78));
    // Skill 30 (Dire Onslaught): 800 discipline at rank 1.
    assert_eq!((list.core[0].skill_id, list.core[0].cost, list.core[0].rank), (30, 800, 1));
    // Every weapon skill has exactly levels 1 and 2.
    for pair in list.weapon.chunks(2) {
        assert_eq!(pair[0].skill_id, pair[1].skill_id);
        assert_eq!((pair[0].level, pair[1].level), (1, 2));
    }
    // Augments under 100: seven per vocation, ids v*10 .. v*10+6.
    for v in 0..9 {
        let ids: Vec<_> = list
            .augments
            .iter()
            .filter(|a| a.augment_id < 100 && a.augment_id / 10 == v)
            .map(|a| a.augment_id % 10)
            .collect();
        assert_eq!(ids, (0..7).collect::<Vec<_>>(), "vocation {}", v + 1);
    }
    assert!(list.augments.iter().filter(|a| a.augment_id >= 100).all(|a| a.rank == 0));
}

#[test]
fn growth_tables() {
    if install().is_none() {
        return;
    }
    let exp = ddparam::Exp::from_bytes(&read("rPlExp", "param\\pl\\level\\exp")).unwrap().entries;
    let total: u32 = (1..200).map(|l| growth::next_exp(l, &exp)).sum();
    assert_eq!(total, growth::TOTAL_EXP_AT_MAX, "shipped curve sums to the exe constant");
    for v in Vocation::ALL {
        let path = format!("param\\pl\\level\\Lv{}", v.param_name());
        let rows = ddparam::Level::from_bytes(&read("rPlLevel", &path)).unwrap().entries;
        let p = Progress::at_level(200, &rows, &exp);
        assert_eq!(p.level, 200);
        assert!(p.stats.hp_max > 0.0 && p.stats.hp_max <= growth::HP_CAP);
        let path = format!("param\\pl\\JobLevel\\JobLv{}", v.param_name());
        let ranks = ddparam::JobLevel::from_bytes(&read("rPlJobLevel", &path)).unwrap().entries;
        assert_eq!(growth::rank_next_exp(9, &ranks), 0);
        assert!(growth::rank_next_exp(1, &ranks) > 0);
    }
}

#[test]
fn vocation_names() {
    if install().is_none() {
        return;
    }
    let b = read("rGUIMessage", "id\\DDN\\message\\common\\common_message_eng");
    let m = mtgmd::MessageFile::parse(&b).unwrap();
    let names: Vec<_> = Vocation::ALL.iter().map(|v| m.by_label(v.label()).unwrap()).collect();
    assert_eq!(
        names,
        [
            "Fighter", "Strider", "Mage", "Mystic Knight", "Assassin", "Magick Archer",
            "Warrior", "Ranger", "Sorcerer"
        ]
    );
}
