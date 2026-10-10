//! Co-op wire decisions: local resource names and authoritative enemy pose.
use ddnet::mirror::{Enemy, EnemyFamily, EnemyIdentity, EnemyRotation};
use ddnet::proto::Snapshot;

fn enemy(id: u32, family: EnemyFamily) -> Enemy {
    Enemy {
        id,
        identity: EnemyIdentity {
            family,
            model: format!("fixture/model/{id}"),
            banks: vec![
                (0, format!("fixture/motion/{id}/co")),
                (2, format!("fixture/motion/{id}/dm")),
            ],
        },
        pos: [1.0, 2.0, 3.0],
        rotation: EnemyRotation::Yaw(0.5),
        scale: [1.0; 3],
        action: 52,
        bank: 0,
        slot: 100,
        frame: 8.25,
        hp: 900.0,
        hp_max: 1000.0,
        dead: false,
    }
}
fn snap(frame: u64, base: Option<u64>, enemies: Vec<Enemy>) -> Snapshot {
    Snapshot {
        frame,
        base,
        cmd_newest: None,
        bodies: vec![],
        vitals: vec![],
        npcs: vec![],
        enemies,
    }
}

#[test]
fn mixed_enemy_whole_and_delta_round_trip_every_transform_component() {
    let goblin = enemy(256, EnemyFamily::Goblin);
    let mut dragon = enemy(257, EnemyFamily::Dragon);
    dragon.rotation = EnemyRotation::Quaternion([0.1, 0.2, 0.3, 0.92736185]);
    dragon.scale = [1.5, 0.75, 2.0];
    let base = snap(1, None, vec![goblin, dragon]);
    let bytes = base.encode(None);
    assert_eq!(Snapshot::decode(&bytes, &|_| None).unwrap(), base);
    let mut next = snap(2, Some(1), base.enemies.clone());
    next.enemies[0].pos[0] = 9.0;
    next.enemies[0].frame = 9.25;
    next.enemies[1].rotation = EnemyRotation::Quaternion([-0.1, 0.2, 0.3, 0.92736185]);
    next.enemies[1].scale[2] = 3.0;
    next.enemies[1].hp = 800.0;
    let bytes = next.encode(Some(&base));
    assert_eq!(Snapshot::decode(&bytes, &|_| Some(base.clone())).unwrap(), next);
    assert!(bytes.len() < next.encode(None).len());
    // The third snapshot drops the goblin and changes the dragon identity:
    // spawn/despawn is determined by the authoritative list, not order.
    let mut changed = next.enemies[1].clone();
    changed.identity.model = "fixture/replacement/model".into();
    changed.identity.banks[0].1 = "fixture/replacement/motion".into();
    let third = snap(3, Some(2), vec![changed]);
    assert_eq!(
        Snapshot::decode(&third.encode(Some(&next)), &|_| Some(next.clone())).unwrap(),
        third
    );
}

#[test]
fn unchanged_and_moving_goblin_deltas_do_not_resend_resource_paths() {
    use ddid::{Reader, Writer};
    let g = enemy(1, EnemyFamily::Goblin);
    let mut w = Writer::new();
    ddnet::mirror::write_enemies(&mut w, std::slice::from_ref(&g), std::slice::from_ref(&g));
    assert_eq!(w.0.len(), 4 + 5);
    let mut moved = g.clone();
    moved.pos[2] = 5.0;
    let mut w = Writer::new();
    ddnet::mirror::write_enemies(&mut w, std::slice::from_ref(&moved), std::slice::from_ref(&g));
    assert_eq!(w.0.len(), 4 + 5 + 12);
    let bytes = w.finish();
    assert_eq!(
        ddnet::mirror::read_enemies(&mut Reader::new(&bytes), std::slice::from_ref(&g)).unwrap(),
        vec![moved]
    );
    assert!(ddnet::mirror::read_enemies(&mut Reader::new(&bytes), &[]).is_err());
}

#[test]
fn interpolation_preserves_flight_rotation_and_avoids_identity_replacement_blend() {
    let mut a = enemy(1, EnemyFamily::Dragon);
    a.rotation = EnemyRotation::Quaternion([0.0, 0.0, 0.0, 1.0]);
    let mut b = a.clone();
    b.rotation = EnemyRotation::Quaternion([0.0, 0.0, -1.0, 0.0]);
    b.pos[1] = 12.0;
    b.scale[0] = 3.0;
    let mid = Enemy::lerp(&a, &b, 0.5);
    let q = mid.rotation.quaternion();
    assert!((q[2].abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    assert!((q.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-6);
    assert_eq!(mid.pos[1], 7.0);
    assert_eq!(mid.scale[0], 2.0);
    b.identity.model = "replacement".into();
    assert_eq!(Enemy::lerp(&a, &b, 0.25), a);
    assert_eq!(Enemy::lerp(&a, &b, 0.75), b);
}

#[test]
fn unknown_family_survives_wire_without_becoming_a_goblin() {
    let s = snap(1, None, vec![enemy(7, EnemyFamily::Unknown(99))]);
    assert_eq!(Snapshot::decode(&s.encode(None), &|_| None).unwrap(), s);
}
