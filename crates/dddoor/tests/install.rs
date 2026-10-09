use dddoor::{placement, runtime};

#[test]
fn installed_cassardis_door_candidates_and_authored_motions() {
    let root = std::env::var("DDDA_NATIVE_PC").unwrap_or_else(|_| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !std::path::Path::new(&root).exists() { return; }
    let vfs = ddvfs::Vfs::open(&root).unwrap();
    let file = ddexe::ExeFile::next_to(std::path::Path::new(&root)).unwrap();
    let families = placement::Families::read(&file.image().unwrap()).unwrap();
    assert_eq!(families.select("om7121").unwrap().class_dti, 0x019a5b64);
    let groups = placement::groups(&vfs, 200).unwrap();
    let world = ddspawn::World { scene: 1400, hour: 12, flag: &|_| false };
    let candidates = placement::candidates(&vfs, &families, 200, &groups, &[(0,0)], &world).unwrap();
    let doorway = candidates.doors.iter().find(|d| d.id.group == 18 && d.id.record == 2).unwrap();
    assert_eq!(doorway.object, 7121);
    eprintln!("Doorway controls: {:?}", doorway.record.fields.iter().filter(|(n,_)|
        ["mFree00","mFree01","mAngleType","mFlag","mCheckAngle","mCheckRange"].contains(n)).collect::<Vec<_>>());
    assert!(doorway.transform.position.iter().all(|v| v.is_finite()));
    let mut checked = std::collections::BTreeSet::new();
    for door in &candidates.doors {
        if runtime::hinged_pair_half_width(door.object).is_none() || !checked.insert(door.object) { continue; }
        let paths = placement::resources(door.object);
        assert!(vfs.locate(&paths.model, mtarc::type_hash("rModel")).is_some());
        assert!(paths.collision.iter().any(|p| vfs.locate(p, mtarc::type_hash("rCollision")).is_some()));
        let bytes = vfs.read(&paths.motion, mtarc::type_hash("rMotionList")).unwrap();
        let motions = mtlmt::parse(&bytes).unwrap();
        for slot in 0..=6 {
            let motion = motions.motions[slot].as_ref().unwrap();
            assert!(motion.frame_count > 0);
        }
        eprintln!("om{}: authored slots0..6 verified; {:?}",door.object,
            motions.motions[..7].iter().map(|m|m.as_ref().map(|m|m.frame_count)).collect::<Vec<_>>());
    }
    assert!(!checked.is_empty());
    eprintln!("Cassardis: {} eligible door candidates", candidates.doors.len());
}
