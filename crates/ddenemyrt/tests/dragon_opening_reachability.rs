//! Structural reachability in the installed beach encounter, not a live
//! execution trace: actor/world predicates can suppress individual rows.
use ddai::fields::Fields;
use ddenemyrt::dragon;
use mtfsm::Graph;
use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;

#[test]
fn installed_opening_dragon_action_set_excludes_table_terminator_labels() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    let exe_path = std::env::var_os("DDDA_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| native.parent().unwrap().join("DDDA.exe"));
    if !native.is_dir() || !exe_path.is_file() {
        eprintln!("SKIP opening dragon reachability: install absent");
        return;
    }
    let bytes = std::fs::read(exe_path).unwrap();
    let exe = ddexe::Exe::parse(&bytes).unwrap();
    let data = dragon::DragonAiData::opening(&exe).unwrap();
    let mut pending = VecDeque::from([dragon::OPENING_THINK_ROOT]);
    let mut records = BTreeSet::new();
    let mut table_actions = BTreeSet::new();
    let mut terminators = 0;
    while let Some(mut address) = pending.pop_front() {
        while records.insert(address) {
            let row = data.tables.get(address).unwrap();
            if row.kind() >= 2 {
                break;
            }
            // cThinkMgr::think 0x008b81a0 tests +8 bit0 before row
            // selection. A terminal row's +0x30 is not an action request.
            // DDDA.exe 2017-12-13 Steam; see also 0x008b8528.
            if row.kind() == 0 && row.flags() & 1 != 0 {
                terminators += 1;
                if row.flags() & 0x8000_0000 == 0 {
                    break;
                }
            } else {
                if row.kind() == 0 && row.action() >= 0 {
                    table_actions.insert(row.action() as u32);
                }
                if row.table() != 0 && exe.section_of(row.table()) == Some(".data") {
                    pending.push_back(row.table());
                }
            }
            address += row.step();
        }
    }
    assert_eq!(records.len(), data.tables.len());
    assert_eq!(records.len(), 52);
    assert_eq!(terminators, 4);
    assert_eq!(
        table_actions,
        BTreeSet::from([0, 4, 6, 19, 21, 26, 27, 28, 29, 34, 72, 74, 86])
    );

    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let graph = Graph::parse(
        &vfs.read("AI/FSM/Enemy/st200/quest/Dragon_OP", mtarc::type_hash("rAIFSM"))
            .unwrap(),
    )
    .unwrap();
    let cluster = graph.root.as_ref().unwrap();
    let mut pending = VecDeque::from([cluster.initial_state]);
    let mut states = BTreeSet::new();
    let mut fsm_actions = BTreeSet::new();
    while let Some(id) = pending.pop_front() {
        if !states.insert(id) {
            continue;
        }
        let state = cluster.states.iter().find(|s| s.id == id).unwrap();
        assert!(state.subcluster.is_none());
        for link in state.links.iter().filter(|l| l.enabled) {
            pending.push_back(link.destination);
        }
        for process in &state.processes {
            if matches!(process.container.as_slice(), b"CtrlAction" | b"CtrlActionEx") {
                let action = process.parameter.as_ref().unwrap().s("アクション番号").unwrap();
                fsm_actions.insert(u32::try_from(action).unwrap());
            }
        }
    }
    assert_eq!(states.len(), 11);
    assert_eq!(fsm_actions, BTreeSet::from([10, 21, 33, 52, 53, 54]));
    let actions: BTreeSet<_> = table_actions.union(&fsm_actions).copied().collect();
    assert_eq!(actions.len(), 18);
    for (id, class) in [
        (0, "cEm5800Wait"),
        (4, "cEm5800Scratch"),
        (6, "cEm5800Walk"),
        (10, "cEm5800TargetFly"),
        (19, "cEm5800Turn"),
        (21, "cEm5800BreathWide"),
        (26, "cEm5800HabatakiAttack"),
        (27, "cEm5800Humitsuke"),
        (28, "cEm5800Scratch"),
        (29, "cEm5800Catch"),
        (33, "cEm5800PressAttack"),
        (34, "cEm5800Huriotoshi"),
        (52, "cEm5800HoverUp"),
        (53, "cEm5800KakkuHover"),
        (54, "cEm5800HoverKakku"),
        (72, "cEm5800Houkou"),
        (74, "cEm5800HabatakiBreath"),
        (86, "cEm5800TargetWalk"),
    ] {
        assert!(actions.contains(&id));
        assert_eq!(data.action_classes[id as usize], class);
    }
}
