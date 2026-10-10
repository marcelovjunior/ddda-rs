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
fn installed_npc_inherits_actual_player_base_motion_registrations() {
    let Some((vfs, file)) = installed() else { return };
    let exe = file.image().unwrap();
    let body = NpcBody::load(&vfs, &exe, 304, 304, 0, 0, 0).unwrap();
    let ledger_bytes = vfs.read(r"etc\item\npclist", mtarc::type_hash("rNpcLedgerList")).unwrap();
    let ledger = ddnpc::Ledger::parse(&ledger_bytes).unwrap();
    assert_eq!(body.civilian, ledger.entries.iter().find(|npc| npc.id == 304).unwrap().civilian);
    let mut entry = 0x014f_1d30;
    let mut registered = Vec::new();
    loop {
        let key = exe.u32(entry).unwrap();
        if key == u32::MAX { break; }
        let bank = ((key >> 8) & 15) as usize;
        let path = exe.cstr(exe.u32(entry + 4).unwrap()).unwrap();
        let resource = body.body.banks[bank].as_ref().expect("inherited resource must be registered");
        assert_eq!(resource.path, path);
        assert!(resource.list.motions.iter().any(Option::is_some));
        registered.push(bank);
        entry += 8;
    }
    assert_eq!(registered, [0, 1, 2]);
    assert!(body.body.banks[7].is_none(), "collision survey must not manufacture a motion bank7");
    // The correction is independent of the authored NPC family banks.
    for authored in [3226, 3270, 3271] {
        let motion = body.resolve(authored).unwrap();
        assert_eq!(motion.bank, 12);
        assert!(body.body.motion(motion).is_some());
    }
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
        let bounds = body.bounds(&pose, 3).unwrap();
        assert!(bounds.center.iter().chain(&bounds.extents).all(|v| v.is_finite()));
        assert!(bounds.extents.iter().all(|v| *v > 0.0));
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
    let base_bounds = body.bounds(&original, 3).unwrap();
    let moved_bounds = body.bounds(&translated, 3).unwrap();
    for axis in 0..3 {
        assert!((moved_bounds.center[axis] - base_bounds.center[axis] - offset[axis]).abs() < 0.001);
        assert!((moved_bounds.extents[axis] - base_bounds.extents[axis]).abs() < 0.001);
    }
    assert!(body.bounds(&original, 4).is_err());
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
    let native_scale = body.unit_scale([1.0; 3]);
    for axis in 0..3 {
        let length = original.model[axis][..3].iter().map(|f| f * f).sum::<f32>().sqrt();
        assert!((length - native_scale[axis]).abs() < 0.00001);
    }
    // Native hurt radius uses the actor's mean unit scale (0x00773830),
    // including HumanEdit's writes to uCoord, not merely LOT scale1.
    let doubled_pose = body.pose(id, 0.0, [0.0; 3], 0.0, [2.0; 3]).unwrap();
    let doubled_bounds = body.bounds(&doubled_pose, 3).unwrap();
    for axis in 0..3 {
        assert!((doubled_bounds.center[axis] - 2.0 * base_bounds.center[axis]).abs() < 0.001);
        assert!((doubled_bounds.extents[axis] - 2.0 * base_bounds.extents[axis]).abs() < 0.001);
    }
    let doubled = body.hurts(id, 0.0, &doubled_pose, 503, [2.0; 3]);
    let radius = |volume: &Volume| match volume { Volume::Sphere { r, .. } | Volume::Capsule { r, .. } => *r };
    for (one, two) in a.iter().zip(&doubled) {
        assert!((radius(&two.volume) - 2.0 * radius(&one.volume)).abs() < 0.0001);
    }
}
