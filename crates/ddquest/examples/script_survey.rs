//! Survey every cFSMOrder script (quest and stage scripts): process names,
//! decoded orders, and the story flags they read and write.
//!
//! cargo run -p ddquest --example script_survey -- <nativePC> [path filter] [--orders]
use ddquest::order::{variables, FlagBank, Order};
use std::collections::{BTreeMap, BTreeSet};

fn walk<'a>(c: &'a mtfsm::Cluster, out: &mut Vec<(&'a mtfsm::State, usize)>, depth: usize) {
    for s in &c.states {
        out.push((s, depth));
        if let Some(sc) = &s.subcluster {
            walk(sc, out, depth + 1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let v = ddvfs::Vfs::open(&args[1]).unwrap();
    let filter = args.get(2).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let print_orders = args.iter().any(|a| a == "--orders");
    let t = mtarc::type_hash("rAIFSM");
    let mut paths: Vec<_> = v.paths_of_type(t).map(|p| p.to_string()).collect();
    paths.sort();
    let mut owners = BTreeMap::<String, usize>::new();
    let mut containers = BTreeMap::<String, usize>::new();
    let mut vars = BTreeMap::<String, usize>::new();
    let mut gsf_write = BTreeMap::<u16, BTreeSet<String>>::new();
    let mut gsf_read = BTreeMap::<u32, BTreeSet<String>>::new();
    let mut sce = BTreeMap::<u32, BTreeSet<String>>::new();
    let mut events = BTreeMap::<(i32, i32), BTreeSet<String>>::new();
    let mut boots = BTreeSet::<(String, u16, u16, u8)>::new();
    let mut qflag = BTreeMap::<(u8, u32), usize>::new();
    let mut scripts = 0;
    for p in &paths {
        if !p.to_lowercase().contains(&filter.to_lowercase()) {
            continue;
        }
        let b = v.read(p, t).unwrap();
        let g = mtfsm::Graph::parse(&b).unwrap();
        let owner = String::from_utf8_lossy(&g.owner).into_owned();
        *owners.entry(owner.clone()).or_default() += 1;
        if owner != "cFSMOrder" {
            continue;
        }
        scripts += 1;
        let mut states = vec![];
        if let Some(c) = &g.root {
            walk(c, &mut states, 0);
        }
        if print_orders {
            println!("== {p}");
        }
        for (s, depth) in states {
            if print_orders {
                println!("{}state {} {:?}", "  ".repeat(depth + 1), s.id, String::from_utf8_lossy(&s.name));
            }
            for pr in &s.processes {
                *containers.entry(String::from_utf8_lossy(&pr.container).into_owned()).or_default() += 1;
                let o = Order::decode(pr);
                if print_orders && !matches!(o, Order::Other { .. }) {
                    println!("{}  {:?}", "  ".repeat(depth + 1), o);
                }
                let flags = match &o {
                    Order::SetFlag { flag, .. } => vec![*flag],
                    Order::Event { played, .. } => vec![*played],
                    Order::QuestFlag { result, .. } => vec![*result],
                    Order::Message { done, .. } => vec![*done],
                    Order::AreaHit { result, .. } => vec![*result],
                    _ => vec![],
                };
                for f in flags {
                    if let FlagBank::Gsf(n) = f.bank() {
                        gsf_write.entry(n).or_default().insert(p.clone());
                    }
                }
                match o {
                    Order::Event { stage, event, sce_no, .. } => {
                        events.entry((stage, event)).or_default().insert(p.clone());
                        if let Some(n) = sce_no {
                            sce.entry(n).or_default().insert(format!("{p} (event st{stage} ev{event:02})"));
                        }
                    }
                    Order::SceNo(n) => {
                        sce.entry(n as u32).or_default().insert(p.clone());
                    }
                    Order::JumpArea { stage, sce_no: Some(n), .. } => {
                        sce.entry(n).or_default().insert(format!("{p} (jump to st{stage})"));
                    }
                    Order::Fade { sce_no: Some(n), .. } => {
                        sce.entry(n).or_default().insert(format!("{p} (fade)"));
                    }
                    Order::Camera { sce_no: n, .. } if n > 0 => {
                        sce.entry(n).or_default().insert(format!("{p} (camera end, raise only)"));
                    }
                    Order::BootQuest { act, quest, branch } => {
                        boots.insert((p.clone(), quest, branch, act));
                    }
                    Order::QuestFlag { act, result, .. } => {
                        *qflag.entry((act, result.kind)).or_default() += 1;
                    }
                    _ => {}
                }
            }
        }
        for c in &g.conditions {
            let mut vs = vec![];
            if let Some(r) = &c.root {
                variables(r, &mut vs);
            }
            for x in vs {
                *vars.entry(format!("{}.{}", x.owner, x.property)).or_default() += 1;
                if x.property == "GSF" {
                    if let Some(i) = x.index {
                        gsf_read.entry(i).or_default().insert(p.clone());
                    }
                }
            }
        }
    }
    println!("scripts (owner cFSMOrder): {scripts}");
    println!("owners: {owners:?}");
    println!("process containers: {containers:?}");
    println!("condition variables: {vars:?}");
    println!("QuestFlag (mActType, mFlagType) counts: {qflag:?}");
    println!("GSF written: {} flags; read: {} flags", gsf_write.len(), gsf_read.len());
    let all: BTreeSet<u32> = gsf_write.keys().map(|&k| k as u32).chain(gsf_read.keys().copied()).collect();
    for n in all {
        let w: Vec<_> = gsf_write.get(&(n as u16)).map(|s| s.iter().cloned().collect()).unwrap_or_default();
        let r: Vec<_> = gsf_read.get(&n).map(|s| s.iter().cloned().collect()).unwrap_or_default();
        println!("GSF {n}\tset by {w:?}\tread by {r:?}");
    }
    for (n, by) in &sce {
        println!("SceNo {n}\t{by:?}");
    }
    for ((s, e), by) in &events {
        println!("event st{s} ev{e:02}\t{by:?}");
    }
    for (p, q, b, a) in &boots {
        println!("boot {p} -> q{q:04} branch {b} act {a}");
    }
}
