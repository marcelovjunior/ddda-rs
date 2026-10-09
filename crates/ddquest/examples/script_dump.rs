//! Readable dump of one script: states, transitions with their conditions,
//! decoded orders (other processes by name).
//!
//! cargo run -p ddquest --example script_dump -- <nativePC> <resource path>
use ddquest::order::{expr_text, Order};

fn cluster(g: &mtfsm::Graph, c: &mtfsm::Cluster, depth: usize) {
    let pad = "  ".repeat(depth);
    println!("{pad}cluster {} initial state {}", c.id, c.initial_state);
    for s in &c.states {
        let from_all = s
            .from_all
            .map(|id| format!(" [entered from any state when {}]", cond(g, id)))
            .unwrap_or_default();
        println!("{pad}  state {} {:?}{from_all}", s.id, String::from_utf8_lossy(&s.name));
        for p in &s.processes {
            match Order::decode(p) {
                Order::Other { container, .. } => println!("{pad}    do {container}"),
                o => println!("{pad}    do {o:?}"),
            }
        }
        for l in &s.links {
            if l.enabled {
                println!("{pad}    -> {} when {}", l.destination, cond(g, l.condition));
            } else {
                println!("{pad}    -> {} (disabled)", l.destination);
            }
        }
        if let Some(sc) = &s.subcluster {
            cluster(g, sc, depth + 2);
        }
    }
}

fn cond(g: &mtfsm::Graph, id: u32) -> String {
    match g.condition(id).and_then(|c| c.root.as_ref()) {
        Some(e) => expr_text(e),
        None => format!("<condition {id} absent: false>"),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = ddvfs::Vfs::open(&a[1]).unwrap();
    let b = v.read(&a[2], mtarc::type_hash("rAIFSM")).unwrap();
    let g = mtfsm::Graph::parse(&b).unwrap();
    println!("{} owner {}", a[2], String::from_utf8_lossy(&g.owner));
    if let Some(c) = &g.root {
        cluster(&g, c, 0);
    }
}
