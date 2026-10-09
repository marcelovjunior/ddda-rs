//! Lists rAIFSM paths matching substrings with cluster/state/link/process counts.
//! cargo run -p ddai --example fsm_list -- <nativePC> <substring>...
fn count(c: &mtfsm::Cluster, n: &mut [usize; 4]) {
    n[0] += 1;
    n[1] += c.states.len();
    for s in &c.states {
        n[2] += s.links.iter().filter(|l| l.enabled).count() + usize::from(s.from_all.is_some());
        n[3] += s.processes.len();
        if let Some(c) = &s.subcluster {
            count(c, n);
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&a[1]).unwrap();
    let h = mtarc::type_hash("rAIFSM");
    let mut ps: Vec<String> = vfs.paths_of_type(h).map(|s| s.to_string()).collect();
    ps.sort();
    for p in ps {
        if !a[2..].iter().all(|s| p.to_lowercase().contains(&s.to_lowercase())) {
            continue;
        }
        let g = mtfsm::Graph::parse(&vfs.read(&p, h).unwrap()).unwrap();
        let mut n = [0; 4];
        if let Some(r) = &g.root {
            count(r, &mut n);
        }
        println!("{p}\tattr={:#x}\tclusters={}\tstates={}\tlinks={}\tprocs={}\tconds={}", g.attribute, n[0], n[1], n[2], n[3], g.conditions.len());
    }
}
