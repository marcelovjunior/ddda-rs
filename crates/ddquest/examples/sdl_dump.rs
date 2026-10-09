//! Print a scheduler (SDL) timeline: tracks as a tree, keys with frame and
//! mode. Unit classes are shown by JAMCRC hash (look them up in the DTI
//! registry).
//!
//! cargo run -p ddquest --example sdl_dump -- <nativePC> <resource path> [max keys per track]
use ddquest::sdl::{Kind, Scheduler, Value};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let v = ddvfs::Vfs::open(&a[1]).unwrap();
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(12);
    let b = v.read(&a[2], mtarc::type_hash("rScheduler")).unwrap();
    let s = Scheduler::parse(&b).unwrap();
    println!("{} frames {} cut track {} tracks {}", a[2], s.frames, s.cut_track, s.tracks.len());
    let mut depth = vec![0usize; s.tracks.len()];
    for (i, t) in s.tracks.iter().enumerate() {
        let d = match t.kind {
            Kind::Root | Kind::Folder => 0,
            Kind::Unit | Kind::Bind => 1,
            _ => depth.get(t.link as usize).copied().unwrap_or(1) + 1,
        };
        depth[i] = d;
        let pad = "  ".repeat(d);
        let extra = match t.kind {
            Kind::Unit => format!(" class {:#010x} line {}", t.type_hash, t.link),
            Kind::Bind => format!(" class {:#010x}", t.type_hash),
            _ => String::new(),
        };
        println!("{pad}[{i}] {:?} {:?}{extra}", t.kind, t.name);
        for k in t.keys.iter().take(max) {
            let val = match &k.value {
                Value::Resource(Some((h, p))) => format!("{h:#010x}:{p}"),
                other => format!("{other:?}"),
            };
            println!("{pad}    @{} {:?} {val}", k.frame, k.mode);
        }
        if t.keys.len() > max {
            println!("{pad}    ... {} more", t.keys.len() - max);
        }
    }
}
