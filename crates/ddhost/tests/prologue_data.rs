//! Installed prologue data, without a synthetic fight or teleported route.
use ddstory::data::{Data, InstallData};
use std::collections::BTreeSet;

fn find_enemy_check(cluster: &mtfsm::Cluster) -> Option<&mtfsm::Process> {
    for state in &cluster.states {
        if let Some(process) = state.processes.iter().find(|p| p.container == b"EnemyCheck") {
            return Some(process);
        }
        if let Some(found) = state.subcluster.as_deref().and_then(find_enemy_check) {
            return Some(found);
        }
    }
    None
}

/// Exercise the actual installed final-encounter order with explicit host
/// facts. This is not a fight or route acceptance test: only reporting the
/// kill record satisfies this gate; elapsed time, absence and zero HP do not.
#[test]
fn installed_chimera_gate_waits_for_a_recorded_kill() {
    use ddstory::encounter::{EncounterFacts, UnitFacts};
    use ddstory::script::{Dispatch, OrderCtx, Owner, ProcMem};
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let data = InstallData::open(&root, None).unwrap();
    let path = r"scr\st380\fsm\fix\set_03";
    let graph = data.fsm(path).unwrap();
    let process = find_enemy_check(graph.root.as_ref().unwrap()).unwrap();
    let mut host = EncounterFacts::default();
    let target = ddstory::Target {
        kind: 2,
        group: 10,
        index: 0,
    };
    let mut world = ddstory::World::default();
    let mut stage = ddstory::story::StageState {
        stage_no: 380,
        ..Default::default()
    };
    let mut owner = Owner::default();
    let mut memory = ProcMem::default();
    let mut requests = vec![];
    let mut commands = vec![];
    let mut run = |host: &EncounterFacts| {
        let mut env = ddstory::Env {
            world: &mut world,
            stage: &mut stage,
            host,
            time: Default::default(),
            out: &mut requests,
            cmds: &mut commands,
            events: None,
        };
        let mut context = OrderCtx {
            script: path,
            owner: &mut owner,
            mem: &mut memory,
            env: &mut env,
            state_name: b"installed chimera gate",
        };
        ddstory::orders::Orders::state(&mut context, process).unwrap();
        (owner.free_flags, owner.free_ins[0])
    };
    for _ in 0..1200 {
        assert_eq!(run(&host), (0, 0));
    }
    host.report_unit(
        380,
        target,
        UnitFacts {
            present: true,
            hp: Some(0.0),
            max_hp: Some(100.0),
            ..Default::default()
        },
    );
    assert_eq!(run(&host), (0, 0));
    host.report_unit(
        380,
        target,
        UnitFacts {
            killed: true,
            ..Default::default()
        },
    );
    assert_eq!(run(&host), (1, 1));
    assert!(
        requests.is_empty(),
        "checking defeat does not create combat or presentation outcomes"
    );
}

#[test]
fn stage_380_scripts_and_enemy_layouts() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let data = InstallData::open(&root, None).unwrap();
    // New game 0x00445be1 sets stage-index12's first layout bit.
    // The layout permission alone does not override the group's separate
    // disabled bit; scripted placement activation is still required.
    let initial = ddstory::Story::new_game(InstallData::open(&root, None).unwrap());
    assert!(initial.world.lot(380, 0));
    let scripts = data.stage_scripts(380).unwrap();
    let startup: Vec<_> = scripts.iter().filter(|p| p.contains("fsm\\fix")).collect();
    assert!(!startup.is_empty());
    for path in startup {
        let graph = data.fsm(path).unwrap();
        assert!(graph.root.is_some(), "{path}");
    }
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let list = ddhost::spawns::group_list(&vfs, 380).unwrap();
    let (scene, hour, flag) = ddhost::spawns::world_inputs(&initial.world, 380);
    assert_eq!(
        ddspawn::gate(
            list.group(0).unwrap(),
            &ddspawn::World {
                scene,
                hour,
                flag: &flag
            }
        ),
        ddspawn::Gate::Disabled
    );
    let cells: BTreeSet<_> = list
        .groups
        .iter()
        .flat_map(|g| g.cells.iter().map(|c| (c.m, c.n)))
        .collect();
    let mut families = BTreeSet::new();
    let mut total = 0;
    // Examine each group's physical placements independently of its gate.
    // This is a reader coverage test, not a claim that all groups activate.
    for group in &list.groups {
        let mut isolated = list.clone();
        isolated.groups = vec![group.clone()];
        let g = &mut isolated.groups[0];
        g.disabled = false;
        g.scene_window = None;
        g.hours = None;
        g.other_conditions.clear();
        for &cell in &cells {
            let (_, spawns) = ddspawn::spawns(
                380,
                &isolated,
                cell,
                &|path| Some(vfs.read(path, mtarc::type_hash("rLayout")).unwrap()),
                &ddspawn::World {
                    scene: 100,
                    hour: 12,
                    flag: &|_| true,
                },
                &[],
            )
            .unwrap();
            for spawn in spawns {
                assert!(spawn.position.iter().all(|v| v.is_finite()));
                families.insert(spawn.class);
                total += 1;
            }
        }
        eprintln!(
            "group {}: kinds {:?}, scene {:?}, flags {:?}/{:?}, other {:?}",
            group.slot,
            group.units.iter().map(|u| &u.name).collect::<Vec<_>>(),
            group.scene_window,
            group.layout_flag,
            group.layout_flag2,
            group.other_conditions
        );
    }
    assert!(total > 0);
    eprintln!(
        "stage380: {} groups, cells {cells:?}, {total} placements, families {families:?}",
        list.groups.len()
    );
}

#[test]
fn lightweight_stock_reads_prologue_party_and_rook() {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !root.is_dir() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let data = InstallData::open(&root, None).unwrap();
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let bytes = vfs
        .read(r"etc\item\npclist", mtarc::type_hash("rNpcLedgerList"))
        .unwrap();
    let ledger = ddnpc::Ledger::parse(&bytes).unwrap();
    let raw = ddpawn::RomPawnInfo::parse(
        &vfs.read(r"etc\editPattern\romPawnInfo", mtarc::type_hash("rRomPawnInfo"))
            .unwrap(),
    )
    .unwrap();
    for language in [0, 1] {
        let actual = ddstory::prologue::load_party(&data, language, 1234).unwrap();
        for (seat, member) in std::iter::once(&actual.hero)
            .chain(&actual.companions)
            .enumerate()
        {
            assert_eq!(member.param, raw.records[10 + seat].param);
            let npc = ledger.entries.iter().find(|n| n.id == 896 + seat as i16).unwrap();
            let mut expected_name = ddpawn::cmc::Cmc::initial().edit;
            expected_name.set_name(npc.name_j);
            assert_eq!(member.edit.name, expected_name.name);
            assert_eq!(member.pawn_type, 2);
            assert_eq!(member.rom_id, 0x2000_000a + seat as u32);
            // Embedded edit override in 0x00796420, not a data-file table.
            assert_eq!(member.edit.get("mFaceBase"), Some(201 + seat as u8));
        }
        let rook = ddstory::prologue::story_stock(&data, 14, language).unwrap();
        let gear: BTreeSet<_> = rook
            .param
            .equip
            .iter()
            .filter(|e| e.num > 0)
            .map(|e| e.item_no)
            .collect();
        // Short override list embedded by 0x00796420, not a data-file table.
        assert_eq!(gear, [414, 575, 634, 985, 1062].into_iter().collect());
        eprintln!(
            "prologue jobs {:?}, levels {:?}",
            std::iter::once(&actual.hero)
                .chain(&actual.companions)
                .map(|p| p.param.job)
                .collect::<Vec<_>>(),
            std::iter::once(&actual.hero)
                .chain(&actual.companions)
                .map(|p| p.param.level)
                .collect::<Vec<_>>()
        );
    }
    assert!(ddstory::prologue::story_stock(&data, 20, 1).is_err());
}
