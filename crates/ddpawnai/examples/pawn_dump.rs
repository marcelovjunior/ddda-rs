//! Prints the pawn priority table and some goal-planning sets:
//! `pawn_dump [goap-path...]`.
use ddpawnai::goap::GoalPlanning;
use ddpawnai::prio::PriorityThink;

fn main() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        let p = PriorityThink::parse(
            &vfs.read(r"AI\PrioThink\cmc", mtarc::type_hash("rAIPriorityThink"))
                .unwrap(),
        )
        .unwrap();
        println!("type {}", p.kind);
        for g in &p.groups {
            for (i, q) in g.params.iter().enumerate() {
                let pers: Vec<String> = q
                    .personal
                    .iter()
                    .map(|v| {
                        let c: Vec<String> = v
                            .checks
                            .iter()
                            .map(|c| format!("{}/{}", c.personality, c.state))
                            .collect();
                        format!(
                            "{:+}{}[{}]",
                            v.add_s32,
                            if v.stop_after_apply { "!" } else { "" },
                            c.join(",")
                        )
                    })
                    .collect();
                let ord: Vec<String> = q
                    .orders
                    .iter()
                    .map(|o| format!("{}->{}", o.order_type, o.value))
                    .collect();
                println!(
                    "{:<16} #{i} sensor {} code {} cat {} obj {} extra {} pers {:?} orders {:?}",
                    g.name, q.sensor, q.code, q.category, q.object_id, q.extra, pers, ord
                );
            }
        }
    }
    let hash = mtarc::type_hash("rAIGoalPlanning");
    for path in &args {
        let g = GoalPlanning::parse(&vfs.read(path, hash).unwrap()).unwrap();
        println!("{path}: premises {:?}", g.premises);
        for goal in &g.goals {
            println!("  goal {} needs {:?}", goal.name, goal.premises);
        }
        for a in &g.actions {
            println!(
                "  action if {} -> {} iface {} {:?} elem {} atk {} use {}",
                a.premise, a.effect, a.interface_id, a.class, a.element_attr, a.atk_attr, a.use_attr
            );
        }
    }
}
