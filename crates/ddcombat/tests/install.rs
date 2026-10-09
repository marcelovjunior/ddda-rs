//! Local-install checks. No fixtures or game bytes are redistributed.
//! Set DDDA_NATIVE_PC to override the maintainer's path; skipped when absent.

fn native_pc() -> Option<std::path::PathBuf> {
    let root = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if root.is_dir() {
        Some(root)
    } else {
        eprintln!("skipped: nativePC absent; set DDDA_NATIVE_PC to run install validation");
        None
    }
}

#[test]
fn every_obj_collision_occurrence_parses() {
    let Some(root) = native_pc() else { return };
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let hash = mtarc::type_hash("rObjCollision");
    let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
    paths.sort_unstable();
    let (mut occurrences, mut groups, mut nodes, mut attacks, mut empty, mut seq) = (0, 0, 0, 0, 0, 0);
    let mut errors = Vec::new();
    for path in &paths {
        for loc in vfs.locate(path, hash).unwrap() {
            occurrences += 1;
            match vfs.read_at(loc).map_err(ddcombat::ocl::Error).and_then(|d| ddcombat::ObjCollision::parse(&d)) {
                Ok(o) => {
                    groups += o.groups.len();
                    nodes += o.groups.iter().map(|g| g.nodes.len()).sum::<usize>();
                    attacks += o.attacks.iter().filter(|a| a.is_some()).count();
                    empty += o.attacks.iter().filter(|a| a.is_none()).count();
                    seq += o.seq_index.len();
                }
                Err(e) => errors.push(format!("{path}: {e}")),
            }
        }
    }
    eprintln!(
        "rObjCollision: {} paths, {occurrences} occurrences, {groups} groups, {nodes} nodes, {attacks} attacks, {empty} empty slots, {seq} seq entries",
        paths.len()
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(occurrences > 0);
}

/// The goblin's own collision file holds hurt/hit volumes and the motion
/// index, but no attacks: those live in shared files (COMBAT.md).
#[test]
fn goblin_collision_shape() {
    let Some(root) = native_pc() else { return };
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let d = vfs
        .read("collision\\em\\e01\\e0100\\e0100", mtarc::type_hash("rObjCollision"))
        .unwrap();
    let o = ddcombat::ObjCollision::parse(&d).unwrap();
    assert_eq!(o.groups.len(), 46);
    assert!(o.attacks.is_empty());
    let hurt = &o.groups[0];
    assert!(hurt.is_damage && !hurt.is_attack);
    // head sphere: joint 136, radius 22, region 1
    assert_eq!((hurt.nodes[2].joint0, hurt.nodes[2].radius, hurt.nodes[2].region_id), (136, 22.0, 1));
}
