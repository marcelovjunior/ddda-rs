//! Whole-install checks. Skips when the install is absent; set
//! DDDA_NATIVE_PC to use another one.
use ddquest::order::Order;
use ddquest::sdl::Scheduler;

fn vfs() -> Option<ddvfs::Vfs> {
    let path = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !path.exists() {
        eprintln!("SKIP: DDDA install absent");
        return None;
    }
    Some(ddvfs::Vfs::open(path).unwrap())
}

#[test]
fn every_scheduler_parses() {
    let Some(v) = vfs() else { return };
    let t = mtarc::type_hash("rScheduler");
    let (mut n, mut tracks, mut keys) = (0, 0, 0);
    let mut failures = vec![];
    for p in v.paths_of_type(t) {
        for l in v.locate(p, t).unwrap() {
            n += 1;
            let b = v.read_at(l).unwrap();
            match Scheduler::parse(&b).and_then(|s| s.validate().map(|_| s)) {
                Ok(s) => {
                    tracks += s.tracks.len();
                    keys += s.tracks.iter().map(|t| t.keys.len()).sum::<usize>();
                }
                Err(e) => failures.push(format!("{p}: {e}")),
            }
        }
    }
    println!("{n} schedulers, {tracks} tracks, {keys} keys, {} failures", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Measured 2026-10-09 (Steam install): 1,253 entries, 126,729 tracks,
    // 204,938 keys.
    assert_eq!((n, tracks, keys), (1253, 126_729, 204_938));
}

#[test]
fn opening_wake_up_event_timeline() {
    let Some(v) = vfs() else { return };
    let b = v.read(r"event\st200\ev10\st200ev10", mtarc::type_hash("rScheduler")).unwrap();
    let s = Scheduler::parse(&b).unwrap();
    assert_eq!(s.frames, 1702);
    // The camera list it loads.
    let has_camera = s.tracks.iter().any(|t| {
        t.keys.iter().any(|k| {
            matches!(&k.value, ddquest::sdl::Value::Resource(Some((_, p))) if p == r"event\st200\ev10\list\camera\camera")
        })
    });
    assert!(has_camera);
}

#[test]
fn opening_quest_scripts_decode() {
    let Some(v) = vfs() else { return };
    let t = mtarc::type_hash("rAIFSM");
    let g = mtfsm::Graph::parse(&v.read(r"quest\q0001_b00", t).unwrap()).unwrap();
    let mut events = vec![];
    fn walk(c: &mtfsm::Cluster, out: &mut Vec<Order>) {
        for s in &c.states {
            out.extend(s.processes.iter().map(Order::decode));
            if let Some(sc) = &s.subcluster {
                walk(sc, out);
            }
        }
    }
    let mut orders = vec![];
    walk(g.root.as_ref().unwrap(), &mut orders);
    for o in &orders {
        if let Order::Event { stage, event, sce_no, .. } = o {
            events.push((*stage, *event, *sce_no));
        }
    }
    assert_eq!(events, vec![(200, 10, Some(1200))]);
    assert!(orders.iter().any(|o| matches!(o, Order::Finish)));
}

#[test]
fn every_quest_control_table_parses() {
    let Some(v) = vfs() else { return };
    let t = mtarc::type_hash("rQuestCtrlTbl");
    let (mut n, mut rows) = (0, 0);
    let mut failures = vec![];
    for p in v.paths_of_type(t) {
        for l in v.locate(p, t).unwrap() {
            n += 1;
            match ddquest::qct::QuestCtrl::parse(&v.read_at(l).unwrap()) {
                Ok(q) => rows += q.tables.iter().map(|t| t.rows.len()).sum::<usize>(),
                Err(e) => failures.push(format!("{p}: {e}")),
            }
        }
    }
    println!("{n} quest control tables, {rows} rows, {} failures", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(n, 338);
}

#[test]
fn newly_arisen_control_rows() {
    use ddquest::qct::{JudgmentKind as J, OutcomeKind as O, QuestCtrl};
    let Some(v) = vfs() else { return };
    let b = v.read(r"etc\questCtrl\q0001_b00", mtarc::type_hash("rQuestCtrlTbl")).unwrap();
    let q = QuestCtrl::parse(&b).unwrap();
    let rows = &q.tables[0].rows;
    // Row 0: no judgment; starts the quest at log entry 0 ("Take a Weapon")
    // and sets the quest's "started" bit 95.
    assert!(rows[0].judgments.is_empty());
    let r0: Vec<_> = rows[0].results.iter().map(|r| r.kind()).collect();
    assert!(r0.contains(&O::Start { log: 0 }));
    assert!(r0.contains(&O::SetQuestBit { quest: 1, bit: 95 }));
    // Row 1: quest bit 3 (set by the script at the room exit) -> entry 1.
    assert_eq!(rows[1].judgments[0].kind(), J::QuestBitSet { quest: 1, bit: 3 });
    assert!(rows[1].results.iter().any(|r| r.kind() == O::Objective { log: 1 }));
    // Row 2: quest bit 4 -> cleared, entry 4 ("Quest Successful"), bit 94.
    assert_eq!(rows[2].judgments[0].kind(), J::QuestBitSet { quest: 1, bit: 4 });
    let r2: Vec<_> = rows[2].results.iter().map(|r| r.kind()).collect();
    assert!(r2.contains(&O::Clear { log: 4 }));
    assert!(r2.contains(&O::SetQuestBit { quest: 1, bit: 94 }));
    // The log entries those rows name.
    let log_bytes = v.read(r"id\questlog_data\q0001_log_eng", mtarc::type_hash("rGUIMessage")).unwrap();
    let log = mtgmd::MessageFile::parse(&log_bytes).unwrap();
    let text = |e| log.message(ddquest::qct::log_message_index(e)).unwrap().to_string();
    assert_eq!(text(0), "Take a Weapon");
    assert_eq!(text(1), "Exit the Room");
    assert_eq!(text(4), "Quest Successful");
}

#[test]
fn event_table_from_the_installed_exe() {
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let Ok(bytes) = std::fs::read(&exe) else {
        eprintln!("SKIP: DDDA.exe absent");
        return;
    };
    let t = ddquest::events::EventTable::from_exe(&bytes).unwrap();
    assert_eq!(t.rows.len(), 98);
    let i = t.find(200, 10).unwrap();
    assert_eq!(i, 22);
    assert_eq!(t.rows[i].path, r"st200\ev10\st200ev10");
    assert_eq!(t.rows[i].npcs, vec![1, 2]);
    assert_eq!(t.find(100, 0), Some(0));
    assert_eq!(t.find(200, 2), None);
    // Every row's scheduler exists in the data.
    let Some(v) = vfs() else { return };
    let sdl = mtarc::type_hash("rScheduler");
    let missing: Vec<_> = t
        .rows
        .iter()
        .filter(|r| v.locate(&format!("event\\{}", r.path), sdl).map_or(true, |l| l.is_empty()))
        .map(|r| r.path.clone())
        .collect();
    println!("event rows without a scheduler: {missing:?}");
}
