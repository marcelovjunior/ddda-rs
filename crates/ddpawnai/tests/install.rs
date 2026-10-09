//! Reads every pawn AI resource of the install (read-only; skips without it).

use ddpawnai::goap::GoalPlanning;
use ddpawnai::prio::PriorityThink;
use std::path::PathBuf;

fn vfs() -> Option<ddvfs::Vfs> {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !root.is_dir() {
        eprintln!("skipped: no install");
        return None;
    }
    Some(ddvfs::Vfs::open(root).unwrap())
}

#[test]
fn pawn_priority_table() {
    let Some(vfs) = vfs() else { return };
    let bytes = vfs.read(r"AI\PrioThink\cmc", mtarc::type_hash("rAIPriorityThink")).unwrap();
    let p = PriorityThink::parse(&bytes).unwrap();
    let kinds: Vec<&str> = p.groups.iter().filter(|g| g.level == 0).map(|g| g.kind.as_str()).collect();
    assert_eq!(kinds, ["QUEST", "PL_Party", "Situ_Personal", "Enemy", "Wait_Follow", "Etc"]);
    assert_eq!(p.groups.len(), 48);
    let params: usize = p.groups.iter().map(|g| g.params.len()).sum();
    let personal: usize = p.groups.iter().flat_map(|g| &g.params).map(|q| q.personal.len()).sum();
    let orders: usize = p.groups.iter().flat_map(|g| &g.params).map(|q| q.orders.len()).sum();
    eprintln!("cmc: type {}, {params} params, {personal} inclination values, {orders} orders", p.kind);
    // Every inclination check names one of the nine inclinations.
    for q in p.groups.iter().flat_map(|g| &g.params) {
        for v in &q.personal {
            assert!(v.checks.iter().all(|c| (0..9).contains(&c.personality)));
        }
    }
    // All priority tables parse.
    let hash = mtarc::type_hash("rAIPriorityThink");
    let paths: Vec<String> = vfs.paths_of_type(hash).map(str::to_string).collect();
    for path in &paths {
        PriorityThink::parse(&vfs.read(path, hash).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    eprintln!("{} priority tables parse", paths.len());
}

#[test]
fn pawn_goal_planning_sets() {
    let Some(vfs) = vfs() else { return };
    let hash = mtarc::type_hash("rAIGoalPlanning");
    let mut paths: Vec<String> = vfs.paths_of_type(hash).map(str::to_string).collect();
    paths.sort();
    let mut cmc = 0;
    for path in &paths {
        let g = GoalPlanning::parse(&vfs.read(path, hash).unwrap()).unwrap_or_else(|e| panic!("{path}: {e}"));
        if path.to_ascii_lowercase().contains(r"goap\cmc\") {
            cmc += 1;
        }
        for a in &g.actions {
            assert!(a.premise == -1 || a.premise >= 0);
        }
    }
    assert!(cmc >= 70, "pawn sets: {cmc}");
    let sword = GoalPlanning::parse(&vfs.read(r"AI\Goap\Cmc\WpnSwordAtk", hash).unwrap()).unwrap();
    assert_eq!(sword.goals[0].name, "NormalAttack");
    assert_eq!(sword.actions.len(), 12);
    assert_eq!(sword.actions[0].class.as_deref(), Some("cCmcSwordCombo"));
    eprintln!("{} goal-planning sets parse ({cmc} pawn sets)", paths.len());
}

/// The priority build on the install's table (0x005AC240): the battle
/// entries' places and the inclination and command effects.
#[test]
fn pawn_priority_build() {
    use ddpawnai::incl::{self, Active};
    use ddpawnai::think::{build, NO_COMMAND};
    let Some(vfs) = vfs() else { return };
    let p = PriorityThink::parse(&vfs.read(r"AI\PrioThink\cmc", mtarc::type_hash("rAIPriorityThink")).unwrap()).unwrap();
    let place = |list: &[ddpawnai::think::Entry], code: i64| {
        list.iter().position(|e| e.param.code == code).map(|i| (i, list[i].level, list[i].group))
    };
    let neutral = build(&p, &[], NO_COMMAND);
    let (follow, follow_level, _) = place(&neutral, 4).unwrap();
    let (battle, battle_level, _) = place(&neutral, 7).unwrap();
    assert_eq!((follow_level, battle_level), (1, 4));
    assert!(follow < battle);
    // Guardian 2: +3 on code 4 -> level 4, after Enemy-04 (group 3 < 4).
    let guardian = build(&p, &[Active { inclination: incl::GUARDIAN, status: 2 }], NO_COMMAND);
    let (follow, level, _) = place(&guardian, 4).unwrap();
    assert_eq!(level, 4);
    assert!(place(&guardian, 7).unwrap().0 < follow);
    // Guardian 1 does not match the `5/2` check.
    let g1 = build(&p, &[Active { inclination: incl::GUARDIAN, status: 1 }], NO_COMMAND);
    assert_eq!(place(&g1, 4).unwrap().1, 1);
    // Scather 2 adds 5 to Etc-04 code 54: level 9 is out of range, so it
    // stays at 4 (the data quirk noted in PAWN_AI.md).
    let scather = build(&p, &[Active { inclination: incl::SCATHER, status: 2 }], NO_COMMAND);
    assert_eq!(place(&scather, 54).unwrap().1, 4);
    // Command 10 sets code 4 to level 4 and code 12 to level 0.
    let go = build(&p, &[], 10);
    assert_eq!((place(&go, 4).unwrap().1, place(&go, 12).unwrap().1), (4, 0));
    // Every entry is placed once.
    let total: usize = p.groups.iter().map(|g| g.params.len()).sum();
    assert_eq!(neutral.len(), total);
}
