//! Synthetic checks of the translated damage rules (no game data needed).
//! The tables below carry the documented values (COMBAT.md 1.5); the
//! install test `install.rs` repeats the worked example from the real files.
use ddcombat::damage::*;

fn game() -> Adjust {
    let mut v = vec![0.0f32; 0x3E];
    v[5] = 100.0;
    v[6] = 0.5;
    v[0x1F] = 2.0;
    v[0x20] = 8.0;
    v[0x21] = 8.0;
    for (c, (d, s, b)) in [(1.25, 1.25, 1.25), (1.0, 1.0, 1.0), (0.75, 0.75, 0.75), (0.4, 0.5, 0.5), (0.0, 0.3, 0.3)]
        .into_iter()
        .enumerate()
    {
        v[0x22 + 3 * c] = d;
        v[0x23 + 3 * c] = s;
        v[0x24 + 3 * c] = b;
    }
    Adjust(v)
}

fn dlc() -> Adjust {
    let mut v = vec![0.0f32; 0x1E];
    v[0x0C] = 10.0;
    v[0x0D] = 2.0;
    v[0x0E] = 10.0;
    v[0x0F] = 2.0;
    v[0x19] = 25.0;
    v[0x1A] = 9.0;
    v[0x1B] = 0.1;
    Adjust(v)
}

/// Player sword attack 1 of `collision\pl\m0001_00_lv1`.
fn sword_row() -> AttackRow {
    AttackRow {
        attack: 50.0,
        attack_rate: 1.0,
        sword_rate: 100.0,
        shrink_type: 1,
        shrink_attack: 100.0,
        blow_type: 3,
        stamina_attack: 60.0,
        wp_phy_atk_rate: 1.0,
        wp_mgc_atk_rate: 1.0,
        wp_critical_rate: 1.0,
        ..Default::default()
    }
}

/// Goblin regions: body (id 0) and head (id 1), `mAttr` 0x118.
fn goblin_regions() -> Vec<RegionPool> {
    (0..2)
        .map(|id| {
            let mut r = RegionPool::from_element(id, 1000.0, 100.0, 0.3, 100.0, 0.3, 1.0, 1.0, 0x118, 1, 75.0, 30.0);
            r.resist.element[4] = 0.6;
            r
        })
        .collect()
}

fn fighter() -> PlayerAttacker {
    // level-1 Fighter: base attack 80, base magick 60; Iron Sword 50 / 0
    PlayerAttacker::new(80.0, 60.0, 50.0, 0.0)
}

#[test]
fn worked_example_iron_sword_on_goblin_is_105() {
    let hit = player_attack(&fighter(), &sword_row(), 0.5, &dlc());
    assert_eq!((hit.phys, hit.mag, hit.shrink, hit.blow), (180.0, 0.0, 100.0, 0.0));
    let merged = merge_frame(&[hit]).unwrap();
    let mut regions = goblin_regions();
    let out = resolve_enemy_hit(merged, &EnemyDefender::new(75.0, 75.0), &mut regions, 0, &CollisionRates::default(), &game(), 1.0);
    assert_eq!(out.total, 105.0);
    assert_eq!(regions[0].hp, 895.0);
    assert!(out.reaction.stagger && !out.reaction.knockdown);
    assert_eq!((regions[0].dp, regions[0].dp_timer), (0.0, Some(0.0)));
    assert_eq!(regions[1].hp, 1000.0);
}

#[test]
fn head_hit_drains_head_and_body() {
    let merged = merge_frame(&[player_attack(&fighter(), &sword_row(), 0.5, &dlc())]).unwrap();
    let mut regions = goblin_regions();
    let out = resolve_enemy_hit(merged, &EnemyDefender::new(75.0, 75.0), &mut regions, 1, &CollisionRates::default(), &game(), 1.0);
    assert_eq!((regions[0].hp, regions[1].hp), (895.0, 895.0));
    assert!(out.reaction.stagger && out.main_reaction.stagger);
}

#[test]
fn ten_hits_kill_and_defence_counts_per_merged_hit() {
    let hit = player_attack(&fighter(), &sword_row(), 0.5, &dlc());
    let mut regions = goblin_regions();
    for i in 0..10 {
        let out = resolve_enemy_hit(merge_frame(&[hit]).unwrap(), &EnemyDefender::new(75.0, 75.0), &mut regions, 0, &CollisionRates::default(), &game(), 1.0);
        assert_eq!(out.dead_parts.contains(&0), i == 9, "hit {i}");
    }
    // two hits in one frame: 360 - 2*75
    let m = merge_frame(&[hit, hit]).unwrap();
    assert_eq!(m.hits, 2);
    let mut regions = goblin_regions();
    let out = resolve_enemy_hit(m, &EnemyDefender::new(75.0, 75.0), &mut regions, 0, &CollisionRates::default(), &game(), 1.0);
    assert_eq!(out.total, 210.0);
    // a shared mIsSameFrameNoHit bit keeps the second hit out
    let mut row = sword_row();
    row.same_frame_no_hit = 1;
    let h = HitValue { row, ..hit };
    assert_eq!(merge_frame(&[h, h]).unwrap().hits, 1);
}

#[test]
fn critical_roll_and_magick_part() {
    let mut p = PlayerAttacker::new(80.0, 60.0, 50.0, 30.0);
    let row = sword_row();
    assert_eq!(critical_multiplier(&p, &row, 0.1, &dlc()), 1.0);
    p.crit_capable = true;
    assert_eq!(critical_multiplier(&p, &row, 0.25, &dlc()), 9.0);
    assert_eq!(critical_multiplier(&p, &row, 0.26, &dlc()), 0.1);
    let h = player_attack(&p, &row, 0.1, &dlc());
    assert_eq!((h.phys, h.mag), (80.0 + 450.0 + 50.0, 270.0));
    let no = AttackRow { is_no_critical: true, ..row };
    assert_eq!(critical_multiplier(&p, &no, 0.1, &dlc()), 1.0);
}

#[test]
fn enemy_attack_and_player_curve() {
    // goblin attack 0: mAttackRate 0.8, strike 100 %
    let row = AttackRow { attack_rate: 0.8, hit_rate: 100.0, shrink_type: 1, shrink_attack: 100.0, blow_type: 3, ..Default::default() };
    let h = enemy_attack(&EnemyAttacker::new(250.0, 80.0), &row);
    assert_eq!((h.phys, h.mag), (200.0, 0.0));
    let mut m = merge_frame(&[h]).unwrap();
    player_mitigate(&mut m, &PlayerDefender::new(80.0, 80.0), &game());
    assert!((m.phys - 111.111).abs() < 1e-3, "{}", m.phys);
    let v = defence_curve(200.0, 40.0, 60.0, 1.0, 0.0, 1.2, &game());
    assert!((v - 200.0 * 100.0 / 180.0 * 1.2).abs() < 1e-3, "{v}");
}

#[test]
fn element_resist_applies_to_magick_only() {
    let row = AttackRow { mgc_attack_rate: 1.0, element: 5, hit_rate: 100.0, attack_rate: 1.0, ..Default::default() };
    let h = enemy_attack(&EnemyAttacker::new(100.0, 200.0), &row);
    let r = Resist { element: [1.0, 1.0, 1.0, 1.0, 0.6], ..Default::default() };
    let mut m = merge_frame(&[h]).unwrap();
    enemy_mitigate(&mut m, &EnemyDefender::new(75.0, 75.0), &r);
    assert_eq!((m.phys, m.mag), (25.0, 200.0 * 0.6 - 75.0));
    // mIsNoCalcEnemyDefence skips both defences and the element factor
    let row2 = AttackRow { is_no_calc_enemy_defence: true, ..row };
    let mut m = merge_frame(&[HitValue { row: row2, ..h }]).unwrap();
    enemy_mitigate(&mut m, &EnemyDefender::new(75.0, 75.0), &r);
    assert_eq!((m.phys, m.mag), (100.0, 200.0));
}

#[test]
fn fully_blocked_hit_deals_minimum_and_drops_stagger() {
    let row = AttackRow { attack_rate: 0.1, hit_rate: 100.0, shrink_type: 1, shrink_attack: 100.0, blow_type: 3, ..Default::default() };
    let h = enemy_attack(&EnemyAttacker::new(100.0, 0.0), &row);
    let mut regions = goblin_regions();
    let out = resolve_enemy_hit(merge_frame(&[h]).unwrap(), &EnemyDefender::new(75.0, 75.0), &mut regions, 0, &CollisionRates::default(), &game(), 1.0);
    assert_eq!(out.total, 1.0);
    assert!(!out.reaction.stagger);
    assert_eq!(regions[0].dp, 100.0);
}

#[test]
fn regen_refill_and_class() {
    let mut r = goblin_regions().remove(0);
    r.dp = 0.0;
    r.dp_timer = Some(0.0);
    let rates = CollisionRates::default();
    for _ in 0..75 {
        tick_region(&mut r, 1.0, &rates);
    }
    assert!((r.dp - 22.5).abs() < 1e-3, "{}", r.dp);
    tick_region(&mut r, 1.0, &rates);
    assert_eq!((r.dp, r.dp_timer), (100.0, None));
    assert_eq!(region_class(1, 0, 0, 1), 1);
    assert_eq!(region_class(0, -1, 0, 1), 0);
    assert_eq!(region_class(4, 1, 0, 1), 4);
    assert_eq!(region_class(1, 0, 1, 3), 0);
    assert_eq!(region_class(0, 0, 1, 3), 3);
}
