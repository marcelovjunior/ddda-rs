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
fn installed_beach_damage_entry_uses_model_bone_map_and_current_joint_attributes() {
    let Some((vfs, file)) = installed() else { return };
    let exe = file.image().unwrap();
    for npc in 704..=710 {
        let body = NpcBody::load(&vfs, &exe, npc, npc, 0, 0, 0).unwrap();
        let bytes = vfs.read(&body.look.base, mtarc::type_hash("rModel")).unwrap();
        let model = mtmod::parse(&bytes).unwrap();
        let map = model.bone_map.expect("ordinary installed beach model has its native bone map");
        let mut expected = vec![0xa5; model.bones.len()];
        for id in [0x96, 0x97, 0x99, 0x9b] {
            if map[id] != 0xff { expected[usize::from(map[id])] |= 0x40; }
        }
        let mut attributes = vec![0xa5; model.bones.len()];
        body.damage_joint_attributes(&mut attributes, 0, false).unwrap();
        assert_eq!(attributes, expected, "NPC{npc} uses the actual installed ID-to-joint map");
        body.damage_joint_attributes(&mut attributes, 0x200, false).unwrap();
        assert_eq!(attributes, vec![0xa5; model.bones.len()], "NPC{npc} preserves every unrelated runtime mAttr bit");
        attributes.pop();
        let before = attributes.clone();
        assert!(body.damage_joint_attributes(&mut attributes, 0, false).is_err());
        assert_eq!(attributes, before, "attributes from a different body lifetime are rejected");
    }
}

#[test]
fn ordinary_mode3_capsule_shortens_real_height_and_collapses_short_spans() {
    use ddenemyrt::npc_body::{ordinary_mode3_capsule, ORDINARY_NPC_SCENERY_RADIUS};
    let radius = ORDINARY_NPC_SCENERY_RADIUS;
    let tall = ordinary_mode3_capsule(200.0, radius).unwrap();
    assert_eq!((tall.start, tall.end, tall.radius), ([0.0, radius, 0.0], [0.0, 200.0 - radius, 0.0], radius));
    for height in [0.0, 20.0, radius * 2.0] {
        let short = ordinary_mode3_capsule(height, radius).unwrap();
        assert_eq!(short.start, short.end);
        assert!((short.start[1] - radius).abs() < 0.0001);
    }
    for invalid in [f32::NAN, f32::INFINITY, f32::MAX, -1.0] {
        assert!(ordinary_mode3_capsule(invalid, radius).is_err());
        assert!(ordinary_mode3_capsule(200.0, invalid).is_err());
    }
    let changed_radius = ordinary_mode3_capsule(200.0, 20.0).unwrap();
    assert_eq!((changed_radius.start[1], changed_radius.end[1], changed_radius.radius), (20.0, 180.0, 20.0));
    assert!(ordinary_mode3_capsule(200.0, 0.0).is_err());
}

#[test]
fn installed_beach_mode3_capsule_uses_shared_adjustment_height_not_human_factor() {
    let Some((vfs, file)) = installed() else { return };
    let exe = file.image().unwrap();
    // Manager0x0047E6F8..0x0047E71A loads table163E660 into+970;
    // 0x00B5B950 binds that first resource to each NPC's body+3348.
    let path = exe.cstr(exe.u32(0x0163_e660).unwrap()).unwrap();
    let bytes = vfs.read(&path, mtarc::type_hash("rAdjustParam")).unwrap();
    assert_eq!(&bytes[..4], b"ajp\0");
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let adjust: Vec<f32> = bytes[12..12 + count * 4].chunks_exact(4)
        .map(|word| f32::from_le_bytes(word.try_into().unwrap())).collect();
    let mut body = NpcBody::load(&vfs, &exe, 704, 704, 0, 0, 0).unwrap();
    let expected = ddenemyrt::npc_body::ordinary_mode3_capsule(adjust[0xd3], adjust[0xd4]).unwrap();
    assert_eq!(body.mode3_scenery_capsule(&adjust).unwrap(), Some(expected));
    let look = ddlook::npc_look(&vfs, 704, 704, 0).unwrap();
    let human_bytes = vfs.read(&look.edit.human_edit, mtarc::type_hash("rHumanEdit")).unwrap();
    let human = ddlook::morph::HumanEdit::parse(&human_bytes).unwrap();
    let human_factor = human.floats[1].max(human.floats[2]);
    assert_ne!(human_factor, adjust[0xd3], "installed HumanEdit D84 and body heightD88 have distinct values");
    assert_ne!(expected, ddenemyrt::npc_body::ordinary_mode3_capsule(human_factor, adjust[0xd4]).unwrap(),
        "HumanEdit factor must not supply the mode3 height");
    // Changing the model's HumanEdit factor cannot mutate the mode3 height:
    // native mode3 jumps past the D84 multiplication used by mode1.
    body.look.body.as_mut().unwrap().model_scale = [0.25, 5.0, 2.0];
    assert_eq!(body.mode3_scenery_capsule(&adjust).unwrap(), Some(expected));
    assert!(body.mode3_scenery_capsule(&[]).is_err());
    let mut changed = adjust.clone();
    changed[0xd4] = 20.0;
    let changed_capsule = body.mode3_scenery_capsule(&changed).unwrap().unwrap();
    assert_eq!(changed_capsule.radius, 20.0, "active D4 replaces the constructor radius");
    let mut invalid = adjust;
    invalid[0xd3] = f32::NAN;
    assert!(body.mode3_scenery_capsule(&invalid).is_err());
    body.look.body = None;
    assert_eq!(body.mode3_scenery_capsule(&invalid).unwrap(), None, "unsupported controller preserves existing load policy");
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
