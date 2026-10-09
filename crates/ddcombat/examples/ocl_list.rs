//! List every rObjCollision path with group/attack/seq counts.
//! cargo run -p ddcombat --example ocl_list -- <nativePC>
fn main() {
    let root = std::env::args().nth(1).expect("usage: ocl_list <nativePC>");
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let hash = mtarc::type_hash("rObjCollision");
    let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
    paths.sort_unstable();
    for p in paths {
        let o = ddcombat::ObjCollision::parse(&vfs.read(p, hash).unwrap()).unwrap();
        let atk = o.attacks.iter().filter(|a| a.is_some()).count();
        println!("{p}\tgroups {}\tattacks {atk}/{}\tseq {}", o.groups.len(), o.attacks.len(), o.seq_index.len());
    }
}
