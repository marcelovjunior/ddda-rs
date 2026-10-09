//! Print every element of an rRegionStatus resource.
//! cargo run -p ddcombat --example rst_dump -- <nativePC> charparam\em\em0100
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&a[1]).unwrap();
    let d = vfs.read(&a[2], mtarc::type_hash("rRegionStatus")).unwrap();
    let r = ddenemy::RegionStatus::parse(&d).unwrap();
    for g in &r.regions {
        println!("region no {} type {}", g.no, g.kind);
        for e in &g.elements {
            println!("  {e:?}");
        }
    }
}
