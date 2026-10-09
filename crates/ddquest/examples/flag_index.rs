//! Index of the global story flags (GSF): who writes and who reads each.
//!
//! Sources: script processes whose result flag has mFlagType 1 (GSF,
//! writer 0x005cc030) and script conditions on `GSF[n]`; quest control
//! tables (judgment 3/4 read, result 4/5 write); stage scenario entries
//! (cScenarioParam.mGsfNo gate, read by 0x00798620; cScenarioArg_GsfFlag
//! mFlagNo: handler 0x007997c0 sets GSF when mFlagType 0 and mSetType 0,
//! clears it when mSetType 1); NPC think scripts' "Set GSFFlag".
//!
//! cargo run -p ddquest --example flag_index -- <nativePC> > flags.md
use ddquest::order::{variables, Fields};
use ddquest::qct::{JudgmentKind, OutcomeKind, QuestCtrl};
use mtfsm::Parameter;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Uses {
    write: BTreeSet<String>,
    read: BTreeSet<String>,
}

fn short(p: &str) -> String {
    let p = p.to_lowercase();
    let p = p.strip_prefix("quest\\").unwrap_or(&p).to_string();
    let p = p.strip_prefix("scr\\").unwrap_or(&p).to_string();
    p.replace('\\', "/")
}

fn records<'a>(r: &'a mtfsm::Record, out: &mut Vec<&'a mtfsm::Record>) {
    out.push(r);
    for (_, vs) in &r.fields {
        for v in vs {
            if let Parameter::Object(Some(o)) = v {
                records(o, out);
            }
        }
    }
}

fn cluster(c: &mtfsm::Cluster, name: &str, idx: &mut BTreeMap<u32, Uses>) {
    for s in &c.states {
        for p in &s.processes {
            if let Some(r) = &p.parameter {
                let mut rs = vec![];
                records(r, &mut rs);
                for r in rs {
                    // NPC think scripts: "Set GSFFlag" (cThinkFSM::cThinkGSFFlg
                    // {FlagNo, Bool}); its handler is untraced.
                    if r.class_name == Some("cThinkFSM::cThinkGSFFlg") {
                        if let Some(n) = r.u("FlagNo").filter(|&n| n < 0x600) {
                            idx.entry(n as u32).or_default().write.insert(format!("{name} (NPC GSFFlg)"));
                        }
                    }
                    if r.u("mFlagType") == Some(1) {
                        if let Some(n) = r.u("mFlagNo").filter(|&n| n < 0x600) {
                            let what = String::from_utf8_lossy(&p.container);
                            idx.entry(n as u32).or_default().write.insert(format!("{name} ({what})"));
                        }
                    }
                }
            }
        }
        if let Some(sc) = &s.subcluster {
            cluster(sc, name, idx);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = ddvfs::Vfs::open(&a[1]).unwrap();
    let mut idx = BTreeMap::<u32, Uses>::new();
    let t = mtarc::type_hash("rAIFSM");
    for p in v.paths_of_type(t) {
        let g = mtfsm::Graph::parse(&v.read(p, t).unwrap()).unwrap();
        let name = short(p);
        if let Some(c) = &g.root {
            cluster(c, &name, &mut idx);
        }
        for c in &g.conditions {
            let mut vs = vec![];
            if let Some(r) = &c.root {
                variables(r, &mut vs);
            }
            for x in vs.into_iter().filter(|x| x.property == "GSF") {
                if let Some(i) = x.index {
                    idx.entry(i).or_default().read.insert(name.clone());
                }
            }
        }
    }
    let t = mtarc::type_hash("rQuestCtrlTbl");
    for p in v.paths_of_type(t) {
        let q = QuestCtrl::parse(&v.read(p, t).unwrap()).unwrap();
        let name = format!("{} (QCT)", short(p).trim_start_matches("etc/questctrl/"));
        for row in q.tables.iter().flat_map(|t| &t.rows) {
            for j in &row.judgments {
                if let JudgmentKind::GsfSet(n) | JudgmentKind::GsfClear(n) = j.kind() {
                    idx.entry(n).or_default().read.insert(name.clone());
                }
            }
            for r in &row.results {
                if let OutcomeKind::SetGsf(n) | OutcomeKind::ClearGsf(n) = r.kind() {
                    idx.entry(n).or_default().write.insert(name.clone());
                }
            }
        }
    }
    let t = mtarc::type_hash("rScenario");
    for p in v.paths_of_type(t) {
        let x = mtxfs::parse(&v.read(p, t).unwrap()).unwrap();
        let name = short(p).replace("/etc/", ":");
        for o in x.walk() {
            let int = |f: &str| match x.get(o, f).and_then(|vs| vs.first()) {
                Some(mtxfs::Value::S(n)) => Some(*n),
                Some(mtxfs::Value::U(n)) => Some(*n as i64),
                _ => None,
            };
            match x.class_of(o).name() {
                Some("cScenarioParam") => {
                    if let Some(n) = int("mGsfNo").filter(|n| (0..0x600).contains(n)) {
                        idx.entry(n as u32).or_default().read.insert(format!("{name} (scenario gate)"));
                    }
                }
                Some("cScenarioArg_GsfFlag") => {
                    if let Some(n) = int("mFlagNo").filter(|n| (0..0x600).contains(n)) {
                        idx.entry(n as u32).or_default().write.insert(format!("{name} (scenario GsfFlag)"));
                    }
                }
                _ => {}
            }
        }
    }
    println!("| GSF | written by | read by |");
    println!("|---:|---|---|");
    for (n, u) in &idx {
        let j = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
        println!("| {n} | {} | {} |", j(&u.write), j(&u.read));
    }
    eprintln!("{} flags", idx.len());
}
