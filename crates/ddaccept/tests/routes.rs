//! Replays every private route trace through ddsim and prints the
//! comparison (docs/ACCEPTANCE.md). Skips when the traces or the install
//! are absent.
//!
//! `cargo test -p ddaccept -- --nocapture` prints the table;
//! `cargo test -p ddaccept -- --ignored` is the strict gate, failing on any
//! quantity outside tolerance (the M2 gate, red until ddsim matches).

use ddaccept::*;

fn run_all() -> Option<Vec<Row>> {
    let dir = trace_dir();
    let traces = load_traces(&dir);
    if traces.is_empty() {
        eprintln!("skip: no traces in {}", dir.display());
        return None;
    }
    let Ok(vfs) = ddvfs::Vfs::open(NATIVE_PC) else {
        eprintln!("skip: no install at {NATIVE_PC}");
        return None;
    };
    let mut rows = Vec::new();
    let mut worlds = std::collections::HashMap::new();
    for (name, t) in traces {
        let t = t.unwrap_or_else(|e| panic!("{name}: {e}"));
        let world = worlds.entry((t.stage, t.cell)).or_insert_with(|| load_world(&vfs, t.stage, t.cell));
        let sim = new_sim(&vfs).expect("sim from the install");
        let r = replay(sim, world, &t);
        rows.extend(compare(&t, &r));
    }
    println!("{}", table(&rows));
    Some(rows)
}

#[test]
fn routes_replay() {
    let Some(rows) = run_all() else { return };
    let bad = rows.iter().filter(|r| !r.ok()).count();
    println!("{bad} of {} quantities outside tolerance or not modelled", rows.len());
    assert!(!rows.is_empty());
}

#[test]
#[ignore = "strict M2 gate: fails while any route quantity is outside tolerance"]
fn routes_within_tolerance() {
    let Some(rows) = run_all() else { return };
    let bad: Vec<_> = rows.iter().filter(|r| !r.ok()).map(|r| format!("{}: {}", r.route, r.quantity)).collect();
    assert!(bad.is_empty(), "outside tolerance: {bad:#?}");
}
