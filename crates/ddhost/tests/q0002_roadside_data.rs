//! Installed q0002 encounter identities and admission, not a combat replay.
//! GPL gates: cLayoutSetCharaBase::vf12 0x0075faa0; layout record ids:
//! 0x00cc1f60 -> 0x007613d0 -> 0x007643d0 (2017-12-13 Steam).

use ddhost::{npc_placements, spawns};
use ddspawn::{Gate, World};
use ddstory::{
    data::{Data, InstallData},
    Target,
};
use mtlot::Value;

#[test]
fn installed_q0002_roadside_groups_and_rescue_actor() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let ledger_bytes = vfs
        .read("etc/item/npclist", mtarc::type_hash("rNpcLedgerList"))
        .unwrap();
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).unwrap();
    assert_eq!(
        ledger.entries.iter().find(|n| n.id == 304).unwrap().name,
        b"Reynard"
    );
    let enemies = spawns::group_list(&vfs, 100).unwrap();
    let npcs = npc_placements::group_list(&vfs, 100).unwrap();
    let goblin_group = enemies.group(23).unwrap();
    let unused_group = enemies.group(24).unwrap();
    let rescue_group = npcs.group(4).unwrap();
    assert_eq!(
        goblin_group.cells.iter().map(|c| (c.m, c.n)).collect::<Vec<_>>(),
        [(64, 53)]
    );
    assert_eq!(
        rescue_group.cells.iter().map(|c| (c.m, c.n)).collect::<Vec<_>>(),
        [(0, 0)]
    );
    assert_eq!(goblin_group.layout_flag, Some(7));
    assert_eq!(rescue_group.layout_flag, Some(3));
    for group in [goblin_group, rescue_group] {
        assert_eq!(group.scene_window, None);
        assert_eq!(group.hours, None);
        assert_eq!(group.layout_flag2, None);
        assert!(!group.disabled && group.other_conditions.is_empty());
    }
    assert!(unused_group.disabled);
    assert_eq!(unused_group.layout_flag, Some(7));
    assert_eq!(unused_group.hours, Some((20, 5)));

    let read = |path: &str| vfs.read(path, mtarc::type_hash("rLayout")).ok();
    let loaded = npc_placements::LoadedLayouts::read(&vfs, 100, &npcs, &[(0, 0)]).unwrap();
    // Quest state selects the population; it is independent of time of day.
    for hour in [0, 12, 23] {
        let mut story = ddstory::World::default();
        story.set_sce_no(1400, false);
        story.set_clock(hour, 0);
        let (scene, hour, flag) = spawns::world_inputs(&story, 100);
        let before = World {
            scene,
            hour,
            flag: &flag,
        };
        assert_eq!(ddspawn::gate(goblin_group, &before), Gate::Flag(7));
        assert_eq!(ddspawn::gate(rescue_group, &before), Gate::Flag(3));
        drop(flag);
        // These are authored q0002_b00 admission writes, not fake deaths.
        story.set_lot(100, 3, true);
        story.set_lot(100, 7, true);
        let (scene, hour, flag) = spawns::world_inputs(&story, 100);
        let world = World {
            scene,
            hour,
            flag: &flag,
        };
        assert_eq!(ddspawn::gate(goblin_group, &world), Gate::Spawn);
        assert_eq!(ddspawn::gate(unused_group, &world), Gate::Disabled);
        let (_, actors) = ddspawn::spawns(100, &enemies, (64, 53), &read, &world, &[]).unwrap();
        let goblins: Vec<_> = actors.iter().filter(|s| s.group == 23).collect();
        assert_eq!(goblins.iter().map(|s| s.set_id).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(!actors.iter().any(|s| s.group == 24));
        for goblin in goblins {
            assert_eq!(goblin.class, "cSetInfoEnemy0100");
            assert_eq!(goblin.record.get("mUseFirstSetThinkTbl"), Some(&Value::Int(1)));
            assert_eq!(goblin.record.get("mStartTableNo"), Some(&Value::Int(1)));
            assert_eq!(goblin.record.get("mFsmFilePath"), Some(&Value::Str(vec![])));
            // the rescue goblins hit at 0.2 of their attack; HP keeps vf33's default
            assert_eq!(
                spawns::placement_rates(&goblin.record),
                ddhost::ddenemyrt::goblin::PlacementRates { hp: None, attack: Some(0.2) }
            );
        }
        if hour == 12 {
            let exe_file = ddexe::ExeFile::next_to(&native).unwrap();
            let fight_data =
                ddhost::ddenemyrt::fight::FightData::load(&vfs, &exe_file.image().unwrap()).unwrap();
            let mut fight = ddhost::ddenemyrt::fight::Fight::new(std::sync::Arc::new(fight_data));
            let out = spawns::spawn_cells(&mut fight, &vfs, 100, &enemies, &[(64, 53)], &world, &[]).unwrap();
            assert_eq!(out.goblins.iter().filter(|(_, s)| s.group == 23).count(), 3);
            assert!(!out.goblins.iter().any(|(_, s)| s.group == 24));
            assert_eq!(fight.goblins.len(), out.goblins.len());
        }
        let candidates = loaded.candidates(&world);
        let rescue: Vec<_> = candidates
            .placements
            .iter()
            .filter(|p| p.target.group == 4)
            .collect();
        assert_eq!(rescue.len(), 1);
        let actor = rescue[0];
        assert_eq!(
            actor.target,
            Target {
                kind: 5,
                group: 4,
                index: 0
            }
        );
        assert_eq!(actor.npc, 304);
        assert_eq!(actor.use_24_schedule, 0);
        assert!(actor
            .authored_action
            .eq_ignore_ascii_case(r"ai\fsm\npc\st100\quest\q0002_n304"));
        assert_eq!(
            spawns::cell_of(actor.transform.position),
            (64, 53),
            "global layout, physical roadside position"
        );
    }
    let data = InstallData::open(&native, None).unwrap();
    let victim_graph = data.fsm(r"ai\fsm\npc\st100\quest\q0002_n304").unwrap();
    let root = victim_graph.root.as_ref().unwrap();
    let first = root.states.iter().find(|s| s.id == root.initial_state).unwrap();
    assert!(first.processes.iter().any(|p| p.container == b"Set Invincible"));
    let script = data.fsm(r"quest\q0002_b00").unwrap();
    let orders: Vec<_> = script
        .root
        .as_ref()
        .unwrap()
        .states
        .iter()
        .flat_map(|s| s.processes.iter().map(ddquest::order::Order::decode))
        .collect();
    for flag in [3, 7] {
        assert!(orders.contains(&ddquest::order::Order::SetLayout {
            act: 0,
            stage: 100,
            flag
        }));
    }
    assert!(orders.contains(&ddquest::order::Order::SceNo(1450)));
    let table = data.qct(r"etc\questCtrl\q0002_b00").unwrap().unwrap();
    assert_eq!(table.tables[0].rows.len(), 7);
    // Authored rescue reward uses the same NPC identity, not a guessed actor.
    assert!(table.tables[0]
        .rows
        .iter()
        .any(|row| row.results.iter().any(|r| r.kind()
            == ddquest::qct::OutcomeKind::FriendUp {
                npc: 304,
                amount: 100
            })));
}
