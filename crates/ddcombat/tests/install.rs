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

/// COMBAT.md section 8 from the installed files: a level-1 Fighter
/// (`param\pl\level\lvfighter` row 1) with an Iron Sword (attack 50,
/// magick 0: itemList item 301 at forge level 0, claude-6's dditem) uses
/// sword attack 1 of `collision\pl\m0001_00_lv1` on a goblin
/// (`charparam\em\em0100_cmn`, RST region type 0), scaled by
/// `param\game`: 105 HP.
#[test]
fn worked_example_from_install() {
    use ddcombat::damage::*;
    let Some(root) = native_pc() else { return };
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let read = |path: &str, class: &str| vfs.read(path, mtarc::type_hash(class)).unwrap();

    let ocl = ddcombat::ObjCollision::parse(&read("collision\\pl\\m0001_00_lv1", "rObjCollision")).unwrap();
    let row = AttackRow::from_attack(ocl.attacks[1].as_ref().unwrap());
    assert_eq!((row.attack, row.attack_rate, row.sword_rate, row.shrink_attack, row.blow_type), (50.0, 1.0, 100.0, 100.0, 3));

    let level = ddparam::Level::from_bytes(&read("param\\pl\\level\\lvfighter", "rPlLevel")).unwrap();
    let lv1 = &level.entries[1];
    let p = PlayerAttacker::new(lv1.attack as f32, lv1.magic_attack as f32, 50.0, 0.0);

    let game = Adjust::parse(&read("param\\game", "rAdjustParam")).unwrap();
    let dlc = Adjust::parse(&read("param\\dlc", "rAdjustParam")).unwrap();

    let gob = ddenemy::EnemyData::load(&vfs, ddenemy::EnemyId::parse("em0100").unwrap()).unwrap();
    let stats = &gob.common.as_ref().unwrap().1;
    let rst = gob.regions.as_ref().unwrap();
    let region = rst.regions.iter().find(|r| r.kind == 0).unwrap();
    let res = |k: &str| stats.resistances[k];
    let resist = Resist {
        element: [res("耐炎"), res("耐氷"), res("耐雷"), res("耐聖"), res("耐魔")],
        slash: res("耐斬"),
        strike: res("耐打"),
    };
    let mut regions: Vec<RegionPool> = region
        .elements
        .iter()
        .map(|e| {
            let mut r = RegionPool::from_element(
                e.id, e.hp_max, e.dp_max, e.dp_speed, e.bp_max, e.bp_speed, e.damage_adjustment,
                e.hit_stop_adjustment, e.attributes, e.surface, e.dp_reset_timer_max, e.bp_reset_timer_max,
            );
            r.resist = resist;
            r
        })
        .collect();
    let rates = CollisionRates::default();
    for r in &mut regions {
        tick_region(r, 0.0, &rates); // class from mSurface
    }
    let hit = player_attack(&p, &row, 0.5, &dlc);
    let out = resolve_enemy_hit(
        merge_frame(&[hit]).unwrap(),
        &EnemyDefender::new(stats.defense, stats.magic_defense),
        &mut regions,
        0,
        &rates,
        &game,
        1.0,
    );
    assert_eq!(out.total, 105.0);
    assert_eq!(regions[0].hp, 895.0);
    assert!(out.reaction.stagger);
}

/// Every motion list's event blocks read; group-0 bits are counted.
/// The goblin's first attack motion opens its hit window (sequence 2:
/// group 2 with attack 0 of `e0100_00`) on frames 45..49.
#[test]
fn motion_collision_events() {
    let Some(root) = native_pc() else { return };
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let hash = mtarc::type_hash("rMotionList");
    let mut paths: Vec<_> = vfs.paths_of_type(hash).collect();
    paths.sort_unstable();
    let (mut files, mut motions, mut high_bits, mut errors) = (0, 0, 0, Vec::new());
    for p in &paths {
        let d = vfs.read(p, hash).unwrap();
        match ddcombat::events::parse_lmt_events(&d) {
            Ok(ms) => {
                files += 1;
                for m in ms.iter().flatten() {
                    motions += 1;
                    if m.groups[0].events.iter().any(|e| e.0 >> 16 != 0) {
                        high_bits += 1;
                    }
                }
            }
            Err(e) => errors.push(format!("{p}: {e}")),
        }
    }
    eprintln!("rMotionList: {files} paths, {motions} motions with events, {high_bits} with group-0 bits >= 16");
    assert!(errors.is_empty(), "{errors:#?}");

    let d = vfs.read("motion\\em\\e01\\e0100_at\\e0100_at", hash).unwrap();
    let ms = ddcombat::events::parse_lmt_events(&d).unwrap();
    let m1 = ms[1].as_ref().unwrap();
    let seqs = |f: f32| ddcombat::events::activations(m1, f).iter().map(|a| (a.resource_id, a.seq)).collect::<Vec<_>>();
    assert!(!seqs(44.9).contains(&(0, 2)));
    assert!(seqs(45.0).contains(&(0, 2)));
    assert!(seqs(48.9).contains(&(0, 2)));
    assert!(!seqs(49.0).contains(&(0, 2)));
}

/// The goal test of the combat-volumes brief from the installed files: the
/// goblin's first attack motion (`e0100_at` motion 1) against a standing
/// Arisen (hurt group 1 of `collision\pl\m0000`) gives exactly one hit, on
/// frame 45, inside the traced distance limit, and none beyond it. The
/// poses are rigid (every joint at the model root), so the limit is the
/// rest-pose one: |dz| <= 50 + sqrt(90^2 - 30^2 - 15^2) = 133.52.
#[test]
fn goblin_swing_against_arisen_from_install() {
    use ddcombat::volumes::*;
    struct Rigid(Vec3);
    impl Pose for Rigid {
        fn model_matrix(&self) -> Mat4 {
            let mut m = IDENTITY;
            m[3] = [self.0[0], self.0[1], self.0[2], 1.0];
            m
        }
        fn joint_matrix(&self, _id: i32) -> Option<Mat4> {
            Some(self.model_matrix())
        }
    }
    let Some(root) = native_pc() else { return };
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let ocl = |p: &str| ddcombat::ObjCollision::parse(&vfs.read(p, mtarc::type_hash("rObjCollision")).unwrap()).unwrap();
    let goblin = ocl("collision\\em\\e01\\e0100\\e0100");
    let goblin_attacks = ocl("collision\\em\\e01\\e0100\\e0100_00");
    let arisen = ocl("collision\\pl\\m0000");
    let lmt = vfs.read("motion\\em\\e01\\e0100_at\\e0100_at", mtarc::type_hash("rMotionList")).unwrap();
    let motions = ddcombat::events::parse_lmt_events(&lmt).unwrap();
    let swing = motions[1].as_ref().unwrap();
    let arisen_acts = [ddcombat::events::Activation { resource_id: arisen.resource_id, seq: 1 }];

    let run = |dz: f32| {
        let mut history = HitHistory::default();
        let mut frames = Vec::new();
        for frame in 0..79 {
            let acts = ddcombat::events::activations(swing, frame as f32);
            let attacks = attack_volumes(&goblin, Some(&goblin_attacks), &acts, &Rigid([0.0; 3]), 1.0, 1);
            let hurts = hurt_volumes(&arisen, &arisen_acts, &Rigid([0.0, 0.0, dz]), 1.0, 2);
            for (ai, _) in resolve_frame(&attacks, &hurts, &mut history, 1.0) {
                frames.push((frame, attacks[ai].row));
            }
        }
        frames
    };
    let hits = run(133.0);
    assert_eq!(hits.len(), 1, "{:?}", hits.iter().map(|h| h.0).collect::<Vec<_>>());
    assert_eq!(hits[0].0, 45);
    let h = ddcombat::damage::enemy_attack(&ddcombat::damage::EnemyAttacker::new(250.0, 80.0), &hits[0].1);
    let m = ddcombat::damage::merge_frame(&[h]).unwrap();
    assert_eq!((m.phys, m.hits), (200.0, 1));
    assert!(run(133.6).is_empty());
}
