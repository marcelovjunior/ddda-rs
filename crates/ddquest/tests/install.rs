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
