//! The install's group lists and the area outside Cassardis (stage 100,
//! cell 64m53n, next to the Cassardis start position 56 in 64m54n).
//! Skipped without the install.

use ddspawn::gpl::GroupList;
use ddspawn::respawn::{self, Record};
use ddspawn::{gate, spawns, Gate, World};
use std::path::PathBuf;
use std::sync::OnceLock;

fn vfs() -> Option<&'static ddvfs::Vfs> {
    static V: OnceLock<Option<ddvfs::Vfs>> = OnceLock::new();
    V.get_or_init(|| {
        let root = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
        if !root.is_dir() {
            eprintln!("skipped: no install");
            return None;
        }
        Some(ddvfs::Vfs::open(root).unwrap())
    })
    .as_ref()
}

const GPL: &str = "rLayoutGroupParamList";

fn st100() -> GroupList {
    let b = vfs()
        .unwrap()
        .read(r"scr\st100\etc\st100_e", mtarc::type_hash(GPL))
        .unwrap();
    GroupList::parse(&b).unwrap()
}

#[test]
fn every_group_list_parses() {
    let Some(vfs) = vfs() else { return };
    let mut paths: Vec<&str> = vfs.paths_of_type(mtarc::type_hash(GPL)).collect();
    paths.sort();
    let mut groups = 0;
    for p in &paths {
        let l = GroupList::parse(&vfs.read(p, mtarc::type_hash(GPL)).unwrap())
            .unwrap_or_else(|e| panic!("{p}: {e}"));
        groups += l.groups.len();
    }
    assert!(paths.len() >= 190, "{} lists", paths.len());
    eprintln!("{} group lists, {groups} groups", paths.len());
    let l = st100();
    assert_eq!((l.slots.len(), l.groups.len()), (295, 292));
}

/// Group 22 lists cells 58m50n and 51m58n... the layouts `st100_58m50n_e22`
/// exist for every cell a group names (spot check on the cells of 64m53n's
/// groups).
#[test]
fn group_cells_name_their_layouts() {
    let Some(vfs) = vfs() else { return };
    let l = st100();
    let g = l.group(22).unwrap();
    assert!(g.cells.iter().any(|c| (c.m, c.n) == (58, 50)));
    for g in l
        .groups
        .iter()
        .filter(|g| g.cells.iter().any(|c| (c.m, c.n) == (64, 53)))
    {
        for c in &g.cells {
            let p = ddspawn::layout_path(100, c.m, c.n, 'e', g.slot);
            assert!(
                vfs.locate(&ddvfs::key(&p), mtarc::type_hash("rLayout")).is_some(),
                "{p}"
            );
        }
    }
}

fn world(scene: i32, hour: u32, flags: &[u16]) -> (i32, u32, Vec<u16>) {
    (scene, hour, flags.to_vec())
}

fn run(scene: i32, hour: u32, flags: &[u16], records: &[Record]) -> (ddspawn::Gates, Vec<(u32, String)>) {
    let vfs = vfs().unwrap();
    let l = st100();
    let (scene, hour, on) = world(scene, hour, flags);
    let flag = move |n: u16| on.contains(&n);
    let w = World {
        scene,
        hour,
        flag: &flag,
    };
    let read = |p: &str| vfs.read(p, mtarc::type_hash("rLayout")).ok();
    let (gates, s) = spawns(100, &l, (64, 53), &read, &w, records).unwrap();
    (gates, s.into_iter().map(|s| (s.group, s.name)).collect())
}

fn names(s: &[(u32, String)], group: u32) -> Vec<&str> {
    s.iter().filter(|x| x.0 == group).map(|x| x.1.as_str()).collect()
}

/// The area outside Cassardis, before the dragon (SceNo 1500, after the
/// first Cassardis quests): by day group 250 (goblins, 4..20 h), by night
/// groups 9, 12 and 252 (20..4 h). Group 23 waits for layout flag 7,
/// group 24 is disabled, 134 needs SceNo 4700 and flag 150, 132 is
/// post-dragon (from 7800).
#[test]
fn cassardis_outskirts_day_and_night() {
    if vfs().is_none() {
        return;
    }
    let l = st100();
    let g250 = l.group(250).unwrap();
    assert_eq!(g250.hours, Some((4, 20)));
    assert_eq!(g250.scene_window, Some((0, 7799)));
    assert_eq!(g250.respawn & 7, 1);
    assert_eq!((g250.respawn >> 3) & 7, 1, "respawns after a day");
    assert_eq!(l.group(23).unwrap().layout_flag, Some(7));
    assert!(l.group(24).unwrap().disabled);

    let (gates, day) = run(1500, 12, &[], &[]);
    let gate_of = |g: u32| gates.iter().find(|x| x.0 == g).map(|x| x.1.clone());
    assert_eq!(gate_of(250), Some(Gate::Spawn));
    assert_eq!(gate_of(12), Some(Gate::Hour));
    assert_eq!(gate_of(23), Some(Gate::Flag(7)));
    assert_eq!(gate_of(24), Some(Gate::Disabled));
    assert_eq!(gate_of(132), Some(Gate::Scene));
    assert_eq!(names(&day, 250), ["em0100", "em0100", "em0100"]);

    let (_, night) = run(1500, 22, &[], &[]);
    assert!(names(&night, 250).is_empty());
    assert_eq!(names(&night, 12), ["em0100", "em0100"]);
    assert!(names(&night, 252).iter().all(|n| *n == "em0200"));

    // After the dragon (SceNo 7800) none of the pre-dragon groups spawn.
    let (gates, late) = run(7800, 12, &[], &[]);
    assert!(
        late.iter()
            .all(|x| x.0 == 132 || x.0 == 13 || x.0 == 254 || x.0 == 23),
        "{gates:?}"
    );
    // Flag 7 brings group 23's goblins.
    let (_, flagged) = run(1500, 12, &[7], &[]);
    assert_eq!(names(&flagged, 23).len(), 3);
}

/// Kill group 250's goblins: they are gone until a day passes.
#[test]
fn killed_goblins_return_after_a_day() {
    if vfs().is_none() {
        return;
    }
    let l = st100();
    let g = l.group(250).unwrap();
    let (_, before) = run(1500, 12, &[], &[]);
    let ids: Vec<u32> = {
        let vfs = vfs().unwrap();
        let read = |p: &str| vfs.read(p, mtarc::type_hash("rLayout")).ok();
        let flag = |_: u16| false;
        let w = World {
            scene: 1500,
            hour: 12,
            flag: &flag,
        };
        spawns(100, &l, (64, 53), &read, &w, &[])
            .unwrap()
            .1
            .iter()
            .filter(|s| s.group == 250)
            .map(|s| s.set_id as u32)
            .collect()
    };
    assert_eq!(names(&before, 250).len(), ids.len());
    let mut records = vec![Record::default(); 295];
    for &u in &ids {
        respawn::kill(&mut records[250], g, 13, u);
    }
    let (_, after) = run(1500, 12, &[], &records);
    assert!(names(&after, 250).is_empty());
    respawn::age(&mut records[250], 1, &[0; 4]);
    let (_, next_day) = run(1500, 12, &[], &records);
    assert_eq!(names(&next_day, 250).len(), ids.len());
    // the gate itself is unchanged by kills
    let flag = |_: u16| false;
    assert_eq!(
        gate(
            g,
            &World {
                scene: 1500,
                hour: 12,
                flag: &flag
            }
        ),
        Gate::Spawn
    );
}

/// A real save (the maintainer's backup; skipped without it): SceNo 3400,
/// noon, and the stage-100 groups' records decode; spawns at 64m53n follow.
#[test]
fn save_state_drives_the_spawns() {
    let Some(vfs) = vfs() else { return };
    let p = std::env::var("DDDA_SAVE")
        .unwrap_or_else(|_| "E:/ddda-re/save-backups/20261009-012612/remote/DDDA.sav".into());
    let Ok(bytes) = std::fs::read(&p) else {
        eprintln!("skipped: no save at {p}");
        return;
    };
    let f = ddsave::SaveFile::parse(&bytes).unwrap();
    let s = ddspawn::save::SaveState::from_xml(&f.xml).unwrap();
    assert_eq!(s.stage_flags.len(), 65);
    eprintln!("save: SceNo {} hour {} day {}", s.scene, s.hour, s.day);
    let records = s.records(0);
    for (g, r) in records.iter().enumerate().filter(|(_, r)| r.in_use()) {
        assert_eq!(((r.bits >> 8) & 0x1ff) as usize, g);
    }
    let l = st100();
    let flag = |n: u16| s.flag(0, n);
    let w = World {
        scene: s.scene,
        hour: s.hour,
        flag: &flag,
    };
    let read = |p: &str| vfs.read(p, mtarc::type_hash("rLayout")).ok();
    let (gates, sp) = spawns(100, &l, (64, 53), &read, &w, &records).unwrap();
    eprintln!("64m53n from the save: {gates:?}; {} spawns", sp.len());
}
