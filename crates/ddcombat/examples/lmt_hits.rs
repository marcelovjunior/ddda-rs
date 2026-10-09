//! For each motion of an rMotionList, print the frame runs whose
//! collision event bits are set and the sequences they activate.
//! cargo run -p ddcombat --example lmt_hits -- <nativePC> <motion list path>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&a[1]).unwrap();
    let d = vfs.read(&a[2], mtarc::type_hash("rMotionList")).unwrap();
    let motions = ddcombat::events::parse_lmt_events(&d).unwrap();
    for (i, m) in motions.iter().enumerate() {
        let Some(m) = m else { continue };
        let g = &m.groups[0];
        let mut start = 0u32;
        let mut line = String::new();
        for &(bits, frames) in &g.events {
            if bits & 0xffff != 0 {
                let acts: Vec<String> = (0..16)
                    .filter(|b| bits >> b & 1 != 0)
                    .map(|b| format!("{}:{}", g.remap[b] / 1000, g.remap[b] % 1000))
                    .collect();
                line += &format!(" [{}..{}) {}", start, start + frames, acts.join(","));
            }
            start += frames;
        }
        if !line.is_empty() {
            println!("motion {i}:{line}");
        }
    }
}
