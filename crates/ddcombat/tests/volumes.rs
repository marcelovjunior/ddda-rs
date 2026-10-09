//! Synthetic checks of hit volumes, shape tests and the hit history.
use ddcombat::damage::{enemy_attack, merge_frame, EnemyAttacker, HitValue};
use ddcombat::events::{activations, EventGroup, MotionEvents};
use ddcombat::ocl::{Group, Node, ObjCollision, SeqIndex};
use ddcombat::volumes::*;

/// Every joint sits at the model's root (identity rotation).
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

#[allow(clippy::too_many_arguments)]
fn node(shape: u32, r: f32, j0: i32, o0: Vec3, j1: i32, o1: Vec3, region: u32, prio: u32) -> Node {
    Node { no: 0, shape, radius: r, joint0: j0, offset0: o0, joint1: j1, offset1: o1, region_id: region, hit_prio: prio, node_attr: 0, node_effect_attr: 0, node_free_work: 0 }
}

fn group(no: u32, kind: u32, attack: bool, damage: bool, nodes: Vec<Node>) -> Group {
    Group { no, kind, is_attack: attack, is_damage: damage, is_scr_adj: false, is_obj_adj: false, is_hang: false, is_hanged: false, is_check: false, is_notice: false, is_notice2: false, nodes }
}

/// A goblin-like attacker: hurt group 0, attack group 1 (a sphere r 60 in
/// front of joint 8), sequence 0 = hurt, sequence 1 = group 1 with attack 0.
fn attacker() -> ObjCollision {
    let mut atk = ddcombat::ocl::Attack { no: 0, words: [0; 83], flag_words: [0, 0] };
    atk.words[1] = 0.8f32.to_bits(); // mAttackRate
    atk.words[3] = 100f32.to_bits(); // mHitRate
    atk.words[9] = 3; // mBlowType none
    ObjCollision {
        version: ddcombat::ocl::VERSION,
        resource_id: 0,
        groups: vec![
            group(0, 1, false, true, vec![node(1, 35.0, 1, [0.0, 0.0, 0.0], -1, [0.0; 3], 0, 5)]),
            group(1, 1, true, false, vec![node(1, 60.0, 8, [30.0, 20.0, 50.0], -1, [0.0; 3], 0, 5)]),
        ],
        attacks: vec![Some(atk)],
        seq_index: vec![
            SeqIndex { no: 0, model_id: 0, group_no: 0, attack_no: 0, relation: 0 },
            SeqIndex { no: 1, model_id: 0, group_no: 1, attack_no: 0, relation: 0 },
        ],
    }
}

/// An Arisen-like victim: a head sphere r 25 and a body capsule r 30
/// (shape 4) from y -45 to y 35.
fn victim() -> ObjCollision {
    ObjCollision {
        version: ddcombat::ocl::VERSION,
        resource_id: 0,
        groups: vec![group(
            0,
            0,
            false,
            true,
            vec![node(1, 25.0, 4, [0.0; 3], -1, [0.0; 3], 0, 5), node(4, 30.0, 1, [0.0, -45.0, 0.0], 2, [0.0, 35.0, 0.0], 0, 5)],
        )],
        attacks: vec![],
        seq_index: vec![SeqIndex { no: 0, model_id: 0, group_no: 0, attack_no: 0, relation: 0 }],
    }
}

/// Motion: bit 0 (hurt) always on, bit 1 (the attack) on frames 45..48.
fn swing() -> MotionEvents {
    let mut g = EventGroup::default();
    g.remap[0] = 0;
    g.remap[1] = 1;
    g.events = vec![(0b01, 45), (0b11, 4), (0b01, 30)];
    MotionEvents { groups: [g, EventGroup::default(), EventGroup::default(), EventGroup::default()] }
}

fn run(distance: f32) -> Vec<(u32, HitValue)> {
    let (att, vic, motion) = (attacker(), victim(), swing());
    let vic_acts = [ddcombat::events::Activation { resource_id: 0, seq: 0 }];
    let mut history = HitHistory::default();
    let mut out = Vec::new();
    for frame in 0..79 {
        let acts = activations(&motion, frame as f32);
        let attacks = attack_volumes(&att, None, &acts, &Rigid([0.0; 3]), 1.0, 1);
        let hurts = hurt_volumes(&vic, &vic_acts, &Rigid([0.0, 0.0, distance]), 1.0, 2);
        for (ai, _region) in resolve_frame(&attacks, &hurts, &mut history, 1.0) {
            out.push((frame, enemy_attack(&EnemyAttacker::new(250.0, 80.0), &attacks[ai].row)));
        }
    }
    out
}

#[test]
fn goblin_swing_hits_once_in_frames_45_to_48() {
    // body capsule after shape-4 pull-in: x 0, y -15..5, z = distance, r 30;
    // limit: 30^2 + 15^2 + (50 - d)^2 <= 90^2  ->  d <= 50 + sqrt(6975) = 133.516
    let hits = run(133.0);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].0, 45);
    let m = merge_frame(&[hits[0].1]).unwrap();
    assert_eq!((m.phys, m.hits), (200.0, 1));
    assert!(run(133.6).is_empty());
    assert_eq!(run(0.0).len(), 1);
}

#[test]
fn shape_tests_touching_counts() {
    let s = |c: Vec3, r: f32| Volume::Sphere { c, r };
    assert!(overlap(&s([0.0; 3], 1.0), &s([2.0, 0.0, 0.0], 1.0)));
    assert!(!overlap(&s([0.0; 3], 1.0), &s([2.001, 0.0, 0.0], 1.0)));
    let cap = Volume::Capsule { p0: [0.0, -10.0, 0.0], p1: [0.0, 10.0, 0.0], r: 1.0 };
    assert!(overlap(&cap, &s([3.0, 10.0, 0.0], 2.0)));
    assert!(!overlap(&cap, &s([3.0, 12.5, 0.0], 2.0)));
    let cap2 = Volume::Capsule { p0: [2.0, 0.0, -5.0], p1: [2.0, 0.0, 5.0], r: 1.0 };
    assert!(overlap(&cap, &cap2));
    let cap3 = Volume::Capsule { p0: [2.5, 0.0, -5.0], p1: [2.5, 0.0, 5.0], r: 1.0 };
    assert!(!overlap(&cap, &cap3));
    // a capsule with coincident points acts as a sphere
    let dot = Volume::Capsule { p0: [0.0; 3], p1: [0.0; 3], r: 1.0 };
    assert!(overlap(&dot, &s([1.5, 0.0, 0.0], 0.5)));
}

#[test]
fn shape4_pull_in_and_degenerate_case() {
    let pose = Rigid([0.0; 3]);
    let long = node(4, 10.0, 1, [0.0, 0.0, 0.0], 2, [0.0, 50.0, 0.0], 0, 5);
    assert_eq!(build_volume(&long, &pose, 1.0), Some(Volume::Capsule { p0: [0.0, 10.0, 0.0], p1: [0.0, 40.0, 0.0], r: 10.0 }));
    let short = node(4, 10.0, 1, [0.0, 0.0, 0.0], 2, [0.0, 15.0, 0.0], 0, 5);
    assert_eq!(build_volume(&short, &pose, 1.0), Some(Volume::Sphere { c: [0.0, 10.0, 0.0], r: 10.0 }));
    assert_eq!(build_volume(&node(1, 10.0, 1, [0.0; 3], -1, [0.0; 3], 0, 5), &pose, 1.5), Some(Volume::Sphere { c: [0.0; 3], r: 15.0 }));
}

#[test]
fn history_multi_hit_and_relation() {
    let mut h = HitHistory::default();
    h.begin_frame(1.0);
    assert!(!h.already_hit(1, 0x0001_0000, 0));
    h.record(1, 0x0001_0000, 0, true, 3.0);
    h.end_frame(|_| true);
    // timed entry blocks until mResetTime frames have passed
    for _ in 0..2 {
        h.begin_frame(1.0);
        assert!(h.already_hit(1, 0x0001_0000, 0));
        h.end_frame(|_| true);
    }
    h.begin_frame(1.0);
    assert!(!h.already_hit(1, 0x0001_0000, 0));
    // a plain entry lapses once the contact stops
    let mut h = HitHistory::default();
    h.begin_frame(1.0);
    h.record(1, 0x0002_0000, 0, false, 0.0);
    h.end_frame(|_| true);
    h.begin_frame(1.0);
    assert!(h.already_hit(1, 0x0002_0000, 0));
    h.end_frame(|_| true); // no contact this frame
    h.begin_frame(1.0);
    assert!(!h.already_hit(1, 0x0002_0000, 0));
    // the same non-zero relation blocks another sequence of the attacker
    let mut h = HitHistory::default();
    h.begin_frame(1.0);
    h.record(1, 0x0003_0000, 7, false, 0.0);
    h.end_frame(|_| true);
    h.begin_frame(1.0);
    assert!(h.already_hit(1, 0x0004_0000, 7));
    assert!(!h.already_hit(2, 0x0004_0000, 7));
}

#[test]
fn pair_filter_kinds() {
    let f = AttackFlags::default();
    let goblin = Side { owner: 1, kind: 1 };
    let arisen = Side { owner: 2, kind: 0 };
    let other_goblin = Side { owner: 3, kind: 1 };
    assert_eq!(pair_filter(&f, goblin, arisen), Some(1.0));
    assert_eq!(pair_filter(&f, arisen, goblin), Some(1.0));
    assert_eq!(pair_filter(&f, goblin, other_goblin), None);
    assert_eq!(pair_filter(&f, goblin, goblin), None);
    let em_no_hit = AttackFlags { em_no_hit: true, ..f };
    assert_eq!(pair_filter(&em_no_hit, arisen, goblin), None);
    let friendly = AttackFlags { friend_hit1_2: true, ..f };
    assert_eq!(pair_filter(&friendly, goblin, other_goblin), Some(0.5));
}
