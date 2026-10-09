//! Catalogue checks against the player's install and DDDA.exe.
//! `DDDA_NATIVE_PC` (default the maintainer's nativePC) and `DDDA_EXE`
//! (default `<nativePC>\..\DDDA.exe`); skipped when either is absent.
use ddskill::catalogue::{Catalogue, ExeTables, SkillKind, Text};
use ddskill::skill_list::SkillList;
use ddskill::vocation::{Vocation, WeaponCategory};
use std::path::PathBuf;
use std::sync::OnceLock;

struct Install {
    vfs: ddvfs::Vfs,
    exe: ExeTables,
}

fn install() -> Option<&'static Install> {
    static I: OnceLock<Option<Install>> = OnceLock::new();
    I.get_or_init(|| {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
        let exe = std::env::var_os("DDDA_EXE")
            .map(PathBuf::from)
            .unwrap_or_else(|| native.parent().unwrap_or(&native).join("DDDA.exe"));
        if !native.exists() || !exe.exists() {
            eprintln!("SKIP catalogue tests: {} or {} absent", native.display(), exe.display());
            return None;
        }
        let exe = ExeTables::from_exe(&std::fs::read(&exe).unwrap()).expect("exe tables");
        Some(Install { vfs: ddvfs::Vfs::open(native).expect("index install"), exe })
    })
    .as_ref()
}

fn messages(vfs: &ddvfs::Vfs, path: &str) -> Vec<String> {
    let b = vfs.read(&format!("{path}_eng"), mtarc::type_hash("rGUIMessage")).unwrap();
    mtgmd::MessageFile::parse(&b).unwrap().messages.iter().map(|s| s.to_string()).collect()
}

fn catalogue() -> Option<&'static Catalogue> {
    static C: OnceLock<Option<Catalogue>> = OnceLock::new();
    C.get_or_init(|| {
        let i = install()?;
        let list = SkillList::from_bytes(
            &i.vfs.read(SkillList::PATH, mtarc::type_hash(SkillList::RESOURCE_CLASS)).unwrap(),
        )
        .unwrap();
        let files = [Text::SKILL_NAME, Text::SKILL_COMMAND, Text::ABILITY_NAME, Text::ABILITY_INFO]
            .map(|p| messages(&i.vfs, p));
        let refs: Vec<Vec<&str>> = files.iter().map(|f| f.iter().map(String::as_str).collect()).collect();
        let text = Text {
            skill_name: &refs[0],
            skill_command: &refs[1],
            ability_name: &refs[2],
            ability_info: &refs[3],
        };
        Some(Catalogue::build(&i.exe, &list, &text).expect("catalogue"))
    })
    .as_ref()
}

#[test]
fn exe_tables() {
    let Some(i) = install() else { return };
    let e = &i.exe;
    assert_eq!(e.skills.len(), 430);
    assert_eq!(e.texts.len(), 149);
    assert_eq!(e.augments.len(), 115);
    // Text indices 0..148 are used exactly once, by the type-1/2 records.
    let mut used: Vec<i32> = e.skills.iter().filter(|s| s.kind != 0).map(|s| s.text).collect();
    used.sort();
    assert_eq!(used, (0..149).collect::<Vec<_>>());
    // Every non-empty action record names a class.
    for t in &e.actions {
        assert!(t.iter().all(|r| (r.dti == 0) == r.class.is_none()));
    }
    let dagger = &e.actions[WeaponCategory::Dagger as usize];
    assert_eq!(dagger[0x2a].class.as_deref(), Some("cPlActWpnDaggerCstmHyakuretsu"));
}

#[test]
fn every_skill_resolves() {
    let Some(c) = catalogue() else { return };
    let core = c.skills.iter().filter(|s| s.kind == SkillKind::Core).count();
    assert_eq!((core, c.skills.len() - core), (18, 131));
    for s in &c.skills {
        assert!(!s.levels.is_empty(), "skill {}", s.id);
        for l in &s.levels {
            assert!(!l.name.trim().is_empty(), "skill {} level {} unnamed", s.id, l.level);
            assert!(!l.command.trim().is_empty(), "skill {} level {} no command", s.id, l.level);
        }
        assert!(!s.weapons.is_empty(), "skill {}", s.id);
        if s.kind == SkillKind::Weapon {
            let lv: Vec<_> = s.levels.iter().map(|l| (l.level, l.cost.is_some())).collect();
            assert!(lv.starts_with(&[(1, true), (2, true)]), "skill {} levels {lv:?}", s.id);
            assert!(lv.len() == 2 || lv[2] == (3, false), "skill {} levels {lv:?}", s.id);
            assert!(!s.vocations.is_empty(), "skill {}", s.id);
            assert!(s.action_class.is_some(), "skill {} has no action class", s.id);
            assert!(s.motion_list.is_some(), "skill {} has no motion list", s.id);
            // A vocation that has the skill can use one of its weapons
            // (0x0163e784, kept as a constant list in `vocation`).
            for v in &s.vocations {
                assert!(
                    s.weapons.iter().any(|w| v.weapon_categories().contains(w)),
                    "skill {} {v:?} {:?}",
                    s.id,
                    s.weapons
                );
            }
            // The action's first parameter is the skill id, except for the
            // shared bow actions and two skills that reuse other actions.
            let shared = s.weapons.iter().any(|w| (10..=12).contains(&(*w as u8)));
            if !shared && s.id != 58 && s.id != 109 {
                assert_eq!(s.action_param, Some(u32::from(s.id)), "skill {}", s.id);
            }
        }
    }
    // 58 weapon skills have a level 3 (Dark Arisen): command[2] >= 0. For
    // the other 73 the name file holds " " at the level-3 slot.
    let lv3 = c.skills.iter().filter(|s| s.levels.len() == 3).count();
    assert_eq!(lv3, 58);
}

#[test]
fn known_names() {
    let Some(c) = catalogue() else { return };
    let skill = |id: u16| c.skills.iter().find(|s| s.id == id).unwrap();
    let names = |id: u16| skill(id).levels.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(40), ["Blink Strike", "Burst Strike", "Blitz Strike"]);
    assert_eq!(names(30), ["Dire Onslaught"]);
    assert_eq!(names(152), ["Scarlet Kisses", "Hundred Kisses", "Thousand Kisses"]);
    assert_eq!(skill(152).vocations, [Vocation::Strider, Vocation::Assassin, Vocation::MagickArcher, Vocation::Ranger]);
    assert_eq!(skill(152).action, Some(0x0105_002a));
    assert_eq!(
        skill(152).motion_list.as_ref().map(|m| m.path.as_str()),
        Some("motion\\pl\\m\\m00\\m0005_at\\m0005_at_02")
    );
    let aug = |id: u16| c.augments.iter().find(|a| a.id == id).unwrap();
    assert_eq!(aug(1).name.as_deref(), Some("Sinew"));
    assert_eq!(aug(62).name.as_deref(), Some("Proficiency"));
    assert_eq!(aug(62).vocation, Some(Vocation::Warrior));
    assert_eq!(aug(108).name.as_deref(), Some("Athleticism"));
    assert_eq!(c.augments.len(), 78);
    // Only augment 106 (cost 0, never offered) has no name.
    let unnamed: Vec<_> = c.augments.iter().filter(|a| a.name.is_none()).map(|a| a.id).collect();
    assert_eq!(unnamed, [106]);
}

#[test]
fn motion_lists_exist() {
    let (Some(i), Some(c)) = (install(), catalogue()) else { return };
    let hash = mtarc::type_hash("rMotionList");
    for s in c.skills.iter().filter_map(|s| s.motion_list.as_ref().map(|m| (s.id, m))) {
        assert!(i.vfs.locate(&s.1.path, hash).is_some(), "skill {} {}", s.0, s.1.path);
        assert!(s.1.bank == 0x700 || s.1.bank == 0x900);
    }
}
