use std::collections::{BTreeMap, BTreeSet};
fn stats(c: &mtfsm::Cluster, n: &mut [usize; 4]) {
    n[0] += 1;
    n[1] += c.states.len();
    for s in &c.states {
        n[2] += s.links.len();
        n[3] += s.links.iter().filter(|l| !l.enabled).count();
        if let Some(c) = &s.subcluster {
            stats(c, n);
        }
    }
}
fn main() {
    let path = std::env::args().nth(1).expect("nativePC path");
    let v = ddvfs::Vfs::open(path).unwrap();
    for kind in [
        "rAIFSM",
        "rAIGoalPlanning",
        "rAIPriorityThink",
        "rAIEnemyActionParameter",
        "rAISensorExt",
    ] {
        let t = mtarc::type_hash(kind);
        let mut ps: Vec<_> = v.paths_of_type(t).collect();
        ps.sort();
        let mut contents = BTreeSet::new();
        let mut instances = 0;
        let mut magic = BTreeMap::new();
        let mut totals = [0; 4];
        let mut schedule_example = None;
        let mut minimum = (usize::MAX, String::new());
        for p in &ps {
            for l in v.locate(p, t).unwrap() {
                instances += 1;
                let b = v.read_at(l).unwrap();
                *magic.entry(b[..4.min(b.len())].to_vec()).or_insert(0usize) += 1;
                if kind == "rAIFSM" {
                    let g = mtfsm::Graph::parse(&b).unwrap();
                    if schedule_example.is_none() && p.starts_with("ai\\fsm\\npc\\") {
                        let x = mtxfs::parse(&b).unwrap();
                        if x.walk().iter().any(|o| {
                            x.get(o, "mPropertyName").is_some_and(|vs| {
                                vs.iter()
                                    .any(|v| matches!(v,mtxfs::Value::Str(s) if s==b"GameHour"))
                            })
                        }) {
                            schedule_example = Some(p.to_string());
                        }
                    }
                    if let Some(c) = &g.root {
                        if !contents.contains(&b) {
                            stats(c, &mut totals);
                        }
                    }
                    if p.starts_with("ai\\fsm\\enemy\\") && b.len() < minimum.0 {
                        minimum = (b.len(), p.to_string());
                    }
                }
                contents.insert(b);
            }
        }
        println!(
            "{kind}: {} paths / {instances} occurrences / {} byte-distinct; magic={magic:?}; unique graph totals(clusters,states,links,disabled)={totals:?}",
            ps.len(),
            contents.len()
        );
        for p in ps.iter().take(3) {
            println!("  {p}");
        }
        if let Some(p) = schedule_example {
            println!("NPC GameHour example: {p}");
        }
        if minimum.0 != usize::MAX {
            println!("smallest enemy: {minimum:?}");
        }
    }
}
