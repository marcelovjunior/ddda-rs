use ddenemyrt::npc_body::NpcBody;
use ddcombat::volumes::Volume;

fn installed() -> Option<(ddvfs::Vfs, ddexe::ExeFile)> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no installed game data"); return None; }
    let exe = ddexe::ExeFile::open(ddexe::locate(&native).unwrap()).unwrap();
    Some((ddvfs::Vfs::open(native).unwrap(), exe))
}

fn center(v: &Volume) -> [f32; 3] {
    match v { Volume::Sphere { c, .. } => *c,
        Volume::Capsule { p0, p1, .. } => std::array::from_fn(|i| (p0[i] + p1[i]) * 0.5) }
}

#[test]
fn installed_reynard_authored_motions_pose_real_hurt_nodes() {
    let Some((vfs, exe)) = installed() else { return };
    let body = NpcBody::load(&vfs, &exe.image().unwrap(), 304, 304, 0, 0, 0).unwrap();
    assert_eq!(body.look.sex, 0);
    assert!(!body.body.skeleton.joints.is_empty());
    // q0002 rescue motions resolved by the reviewed S32 action bank rule.
    for authored in [3073, 3074, 3089, 3226] {
        let id = body.resolve(authored).unwrap();
        let pose = body.pose(id, 0.0, [0.0; 3], 0.0, [1.0; 3]).unwrap();
        assert!(pose.joints.iter().flat_map(|(_, m)| m.iter().flatten()).all(|f| f.is_finite()));
        let hurts = body.hurts(id, 0.0, &pose, 501, [1.0; 3]);
        assert!(!hurts.is_empty(), "authored {authored} must enable its installed hurt nodes");
        assert!(hurts.iter().all(|h| h.side.owner == 501 && h.side.kind == 3 && h.region_id == 0));
    }
}

#[test]
fn installed_reynard_pose_keeps_unit_transform_and_owner_separate() {
    let Some((vfs, exe)) = installed() else { return };
    let body = NpcBody::load(&vfs, &exe.image().unwrap(), 304, 304, 0, 0, 0).unwrap();
    let id = body.resolve(3073).unwrap();
    let original = body.pose(id, 0.0, [0.0; 3], 0.0, [1.0; 3]).unwrap();
    let offset = [200.0, 10.0, -300.0];
    let translated = body.pose(id, 0.0, offset, 0.0, [1.0; 3]).unwrap();
    let a = body.hurts(id, 0.0, &original, 501, [1.0; 3]);
    let b = body.hurts(id, 0.0, &translated, 502, [1.0; 3]);
    assert_eq!(a.len(), b.len());
    assert!(!a.is_empty());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!((a.side.owner, b.side.owner), (501, 502));
        let delta: [f32; 3] = std::array::from_fn(|i| center(&b.volume)[i] - center(&a.volume)[i]);
        for i in 0..3 { assert!((delta[i] - offset[i]).abs() < 0.001); }
    }
    assert!(body.resolve(0xfff).is_err());
}
