//! Synthetic checks of the timed statuses (`ddcombat::cstatus`).
use ddcombat::cstatus::*;

fn e(category: u32, timer: f32, p0: f32, p1: f32) -> Option<StatusEntry> {
    Some(StatusEntry { category, is_overwrite: false, overwrite_num: 1, timer, param0: p0, param1: p1 })
}

/// A table shaped like `param\status\enemy` (categories as stored: group
/// in the low byte), with round test values.
fn table() -> StatusTable {
    let mut t = vec![None; 40];
    t[0] = e(0x1, 10.0, 5.0, 1.0); // poison
    t[1] = e(0x1, 2.0, 0.5, 0.0); // slow
    t[4] = e(0x1_0101, 2.0, 1.5, 2.0); // sleep
    t[5] = e(0x1_0104, 3.0, 2.0, 2.0); // oil
    t[6] = e(0x104, 3.0, 0.5, 2.0); // drenched
    t[9] = e(0x3, 3.0, 0.6, 1.4); // curse
    t[10] = e(0x104, 1.0, 33.0, 1.0); // burning
    t[11] = e(0x104, 1.0, 2.0, 0.083); // frozen
    t[12] = e(0x2, 10000.0, 10.0, 180.0); // petrify
    for (k, slot) in t.iter_mut().enumerate().take(18).skip(14) {
        *slot = Some(StatusEntry { category: 3, is_overwrite: true, overwrite_num: 4, timer: 3.0, param0: if k % 2 == 0 { 0.7 } else { 1.3 }, param1: 0.0 });
    }
    t[0x22] = e(0x7, 1.0, 0.0, 0.0);
    StatusTable(t)
}

#[test]
fn item_cures_remove_stacks_without_removing_other_statuses() {
    let t = table();
    let mut s = Statuses::default();
    s.add(kind::POISON, &t);
    s.add(kind::SLOW, &t);
    s.add(kind::ATTACK_DOWN, &t);
    s.add(kind::ATTACK_DOWN, &t);
    assert_eq!(s.cure_item((1 << kind::POISON) | (1 << kind::ATTACK_DOWN), false),
        vec![kind::POISON, kind::ATTACK_DOWN]);
    assert_eq!(s.mask(), 1 << kind::SLOW);
    let mut possessed = s.works[0];
    possessed.kind = kind::POSSESSED;
    s.works.push(possessed);
    assert!(s.cure_item(1 << kind::POSSESSED, false).is_empty());
    assert_eq!(s.cure_item(1 << kind::POSSESSED, true), vec![kind::POSSESSED]);
}

#[test]
fn item_healing_recovers_wounded_hp_but_does_not_revive() {
    let (mut hp, mut recoverable) = (100.0, 120.0);
    assert_eq!(recover_item_hp(&mut hp, &mut recoverable, 300.0, 150.0), 150.0);
    assert_eq!((hp, recoverable), (250.0, 250.0));
    assert_eq!(recover_item_hp(&mut hp, &mut recoverable, 300.0, 150.0), 50.0);
    assert_eq!(recoverable, 300.0);
    hp = 0.0;
    assert_eq!(recover_item_hp(&mut hp, &mut recoverable, 300.0, 150.0), 0.0);
}

#[test]
fn add_refreshes_ticks_and_wears_off() {
    let t = table();
    let mut s = Statuses::default();
    assert!(s.add(kind::POISON, &t));
    assert_eq!(s.works[0].timer, 300.0);
    for _ in 0..100 {
        s.tick(1.0);
    }
    assert!(s.add(kind::POISON, &t)); // refresh, not a second stack
    assert_eq!((s.works.len(), s.works[0].timer), (1, 300.0));
    let mut ended = Vec::new();
    for _ in 0..300 {
        ended.extend(s.tick(1.0));
    }
    assert_eq!(ended, vec![kind::POISON]);
    assert!(!s.has(kind::POISON));
}

#[test]
fn downs_stack_to_four_and_multiply() {
    let t = table();
    let mut s = Statuses::default();
    for _ in 0..6 {
        s.add(kind::ATTACK_DOWN, &t);
    }
    assert_eq!(s.works.iter().filter(|w| w.kind == kind::ATTACK_DOWN).count(), 4);
    let (p, m) = s.attack_muls();
    assert!((p - 0.7f32.powi(4)).abs() < 1e-6 && m == 1.0, "{p} {m}");
    s.add(kind::CURSE, &t);
    let (p2, _) = s.attack_muls();
    assert!((p2 - 0.7f32.powi(4) * 0.6).abs() < 1e-6);
    s.add(kind::DEFENCE_DOWN, &t);
    assert!((s.defence_muls().0 - 1.3 * 1.4).abs() < 1e-6);
}

#[test]
fn petrify_blocks_and_displaces_cleanse_clears() {
    let t = table();
    let mut s = Statuses::default();
    s.add(kind::POISON, &t);
    s.add(kind::OIL, &t);
    s.add(kind::CURSE, &t);
    assert!(s.add(kind::PETRIFY, &t));
    // group 2 displaces groups 1, 3, 4 (CONFLICT[2])
    assert_eq!(s.works.iter().map(|w| w.kind).collect::<Vec<_>>(), vec![kind::PETRIFY]);
    assert!(!s.add(kind::POISON, &t)); // blocked while petrified
    // permanent: never wears off by time
    for _ in 0..20000 {
        s.tick(1.0);
    }
    assert!(s.has(kind::PETRIFY));
    let mut s = Statuses::default();
    s.add(kind::POISON, &t);
    s.add(kind::SLOW, &t);
    assert!(s.add(kind::CLEANSE, &t));
    assert!(!s.has(kind::POISON) && !s.has(kind::SLOW));
    assert!(!s.add(kind::POISON, &t)); // group 7 blocks group 1
}

#[test]
fn victim_scale_and_slow() {
    let t = table();
    let mut s = Statuses::default();
    s.add(kind::SLEEP, &t);
    assert_eq!(s.victim_scale(0, Some((3.0, 10.0))), (6.0, 6.0, 15.0));
    assert_eq!(s.victim_scale(0, None), (2.0, 2.0, 1.5));
    let mut s = Statuses::default();
    s.add(kind::OIL, &t);
    assert_eq!(s.victim_scale(1, None), (2.0, 2.0, 1.0));
    assert_eq!(s.victim_scale(2, None), (1.0, 1.0, 1.0));
    s.add(kind::FROZEN, &t);
    assert_eq!(s.victim_scale(0, None).0, 2.0);
    let mut d = Statuses::default();
    d.add(kind::DRENCHED, &t);
    assert_eq!(d.victim_scale(1, None).1, 0.5);
    assert_eq!(d.victim_scale(3, None).1, 2.0);
    d.add(kind::SLOW, &t);
    assert_eq!(d.step_scale(), 0.5);
}

#[test]
fn poison_and_burning_damage() {
    let t = table();
    let mut s = Statuses::default();
    s.add(kind::POISON, &t);
    let mut st = DotState::default();
    let hp: f32 = (0..61).map(|_| poison_enemy(&mut st, &s, 1.0)).sum();
    // frames 0, 31, (62...): 5 HP at onset and every 30 frames + 1
    assert_eq!(hp, -10.0);
    let mut st = DotState::default();
    let ticks: Vec<usize> = (0..70).filter(|_| poison_player(&mut st, &s, 1.0) < 0.0).collect();
    assert_eq!(ticks, vec![2, 32, 62]); // every 30 frames
    let mut b = Statuses::default();
    b.add(kind::BURNING, &t);
    let total: f32 = (0..30).map(|_| burning(&b, 1.0)).sum();
    assert!((total + 33.0).abs() < 1e-3, "{total}");
}
