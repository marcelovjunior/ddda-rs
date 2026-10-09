//! The world's own enemies outside Cassardis (stage 100, cell 64m53n) in the
//! host's fight, from the install's group lists and layouts, gated by the
//! story state (SceNo, hour, layout flags). Skipped without the install.

use ddhost::ddenemyrt::fight::{Fight, FightData};
use ddhost::spawns::{cell_of, cells_around, group_list, spawn_cells, world_inputs};
use ddsim::{Ground, Sim};
use ddspawn::World;
use std::collections::HashMap;
use std::sync::Arc;

struct Flat;

impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        Some(below_y.min(1.0e9))
    }
}

fn install() -> Option<(ddvfs::Vfs, Arc<FightData>)> {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).ok()?;
    let data = Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    Some((vfs, data))
}

/// Cassardis' start position 56 (stage 100) is in 64m54n; its 3x3 block
/// takes in 64m53n, where group 250's three goblins spawn by day before
/// the dragon and groups 9 and 12 by night.
#[test]
fn cassardis_area_goblins_join_the_fight() {
    let Some((vfs, data)) = install() else { return };
    let list = group_list(&vfs, 100).unwrap();
    let start = [41522.656, 31094.824, 142168.1];
    assert_eq!(cell_of(start), (64, 54));
    let cells = cells_around(start);
    assert!(cells.contains(&(64, 53)));

    let mut story = ddstory::World::default();
    story.set_sce_no(1500, false);
    story.set_clock(12, 0);
    let (scene, hour, flag) = world_inputs(&story, 100);
    let w = World {
        scene,
        hour,
        flag: &flag,
    };
    let mut fight = Fight::new(data.clone());
    let out = spawn_cells(&mut fight, &vfs, 100, &list, &[(64, 53)], &w, &[]).unwrap();
    drop(flag);
    let groups: Vec<u32> = out.goblins.iter().map(|g| g.1.group).collect();
    assert_eq!(groups, [250, 250, 250]);
    assert_eq!(fight.goblins.len(), 3);
    assert!(out.placeholders.iter().all(|s| s.class != "cSetInfoEnemy0100"));
    assert!(
        !out.placeholders.is_empty(),
        "the critters (em8000 ...) are placeholders"
    );
    for (owner, s) in &out.goblins {
        let g = fight.goblins.iter().find(|g| g.fighter.owner == *owner).unwrap();
        assert_eq!(g.pos, s.position);
    }

    // By night: groups 9 and 12.
    story.set_clock(22, 0);
    let (scene, hour, flag) = world_inputs(&story, 100);
    let w = World {
        scene,
        hour,
        flag: &flag,
    };
    let mut night = Fight::new(data.clone());
    let out = spawn_cells(&mut night, &vfs, 100, &list, &[(64, 53)], &w, &[]).unwrap();
    let mut groups: Vec<u32> = out.goblins.iter().map(|g| g.1.group).collect();
    groups.dedup();
    assert_eq!(groups, [9, 12]);
    assert_eq!(night.goblins.len(), 7);

    // The spawned goblins think and move with an Arisen near them.
    let mut sim = Sim::new(HashMap::new());
    let near = out.goblins[0].1.position;
    sim.add_player(0, [near[0] + 800.0, near[1], near[2]], 0.0);
    night.add_arisen(0);
    night.equip(&mut sim, 0);
    night.volumes(&sim);
    for _ in 0..60 {
        night.hits(1.0);
        sim.step(1.0 / 30.0, &HashMap::new(), &Flat);
        night.players_moved(&mut sim, &Flat, 1.0);
        night.enemies(&sim, &Flat, 1.0);
        night.push_out(&mut sim);
        night.volumes(&sim);
    }
    assert!(night.goblins.iter().any(|g| g.log.len() > 1), "the goblins act");
}

/// The maintainer's save (skipped without it): its Arisen stands near
/// Cassardis or elsewhere; the goblins of the cells around it come from the
/// data, gated by the save's SceNo, hour and flags.
#[test]
fn goblins_from_a_save() {
    let Some((vfs, data)) = install() else { return };
    let p = std::env::var("DDDA_SAVE")
        .unwrap_or_else(|_| "E:/ddda-re/save-backups/20261009-012612/remote/DDDA.sav".into());
    let Ok(bytes) = std::fs::read(&p) else { return };
    let f = ddsave::SaveFile::parse(&bytes).unwrap();
    let s = ddspawn::save::SaveState::from_xml(&f.xml).unwrap();
    let mut fight = Fight::new(data);
    // At the Cassardis start position (stage 100, start 56).
    let out =
        ddhost::spawns::spawn_from_save(&mut fight, &vfs, 100, &s, [41522.656, 31094.824, 142168.1]).unwrap();
    eprintln!(
        "save SceNo {} hour {}: {} goblins, {} placeholders",
        s.scene,
        s.hour,
        out.goblins.len(),
        out.placeholders.len()
    );
    assert_eq!(fight.goblins.len(), out.goblins.len());
    assert!(
        out.goblins.iter().any(|g| g.1.group == 250),
        "noon before the dragon: group 250"
    );
}
