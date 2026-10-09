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
