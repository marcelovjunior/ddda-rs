//! Source shot scheduling, normal Input edges and independent seats.
use ddsim::attack::BowShotRow;
use ddsim::{
    BowAccuracy, BowParam, BowPhase, BowShotParam, BowShotState, Ground, Input, RootMotion, Sim, Weapon,
};
use std::collections::HashMap;

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> {
        (below >= 0.0).then_some(0.0)
    }
}
fn row(wait_frame: f32, charge_level: u32, index: u32) -> BowShotRow {
    BowShotRow {
        wait_frame,
        charge_level,
        group: 2,
        index,
        angle: [0.0; 2],
        spread: [0.0; 2],
        shell_wait: 0.0,
    }
}
fn param() -> BowShotParam {
    BowShotParam {
        upper: 20,
        lower: 21,
        reload_speed: 1.5,
        shot_wait: 0.5,
        charge_begin_frame: 1.0,
        stamina: 9.0,
        stamina_attack_rate: 2.0,
        weapon_stamina: 0.0,
        accuracy_wait: 0.2,
        rows: vec![row(0.0, 0, 7), row(0.25, 1, 8), row(0.5, 0, 9)],
    }
}
fn sim() -> Sim {
    let mut s = Sim::new(HashMap::new());
    s.bow_param = Some(BowParam {
        draw_upper: 0,
        draw_lower: 1,
        release_upper: 2,
        release_lower: 3,
        wait_upper: 4,
        wait_lower: 5,
        fall: 6,
        end_at_zero_stamina: false,
        wait_stamina: 0.0,
    });
    s.bow_shot_param = Some(param());
    s.weapon_motions.insert(
        Weapon::Bow,
        [0, 1, 2, 3, 4, 5, 20, 21]
            .into_iter()
            .map(|slot| (0x900 | slot, RootMotion::none(if slot == 20 { 8 } else { 2 })))
            .collect(),
    );
    for id in [3, 7] {
        s.add_player(id, [0.0; 3], 0.0);
        let p = s.player_mut(id).unwrap();
        p.weapon = Weapon::Dagger;
        p.secondary_weapon = Weapon::Bow;
    }
    s
}
fn tick(s: &mut Sim, lights: [bool; 2], held: [bool; 2]) {
    s.step(
        1.0 / 120.0,
        &HashMap::from([
            (
                3,
                Input {
                    bow: held[0],
                    light: lights[0],
                    ..Default::default()
                },
            ),
            (
                7,
                Input {
                    bow: held[1],
                    light: lights[1],
                    ..Default::default()
                },
            ),
        ]),
        &Flat,
    );
}
fn ready(s: &mut Sim) {
    for _ in 0..20 {
        tick(s, [false; 2], [true; 2]);
        if s.players
            .iter()
            .all(|p| p.bow.is_some_and(|b| b.phase == BowPhase::Ready))
        {
            return;
        }
    }
    panic!("draw did not finish");
}

#[test]
fn ordered_cursor_does_not_reorder_future_or_unmatched_rows() {
    let rows = [row(0.0, 1, 0), row(0.5, 0, 1), row(0.0, 0, 2)];
    let mut shot = BowShotState::default();
    assert!(shot.advance(&rows, 1.0).is_empty());
    shot.begin(0, 3.0, 2.0);
    assert!(shot.advance(&rows, 0.25).is_empty());
    assert_eq!(shot.cursor, 1); // consumes unmatched charge
    assert!(shot.advance(&rows, 0.25).is_empty());
    assert_eq!(shot.advance(&rows, 0.25), rows[1..]);
    assert!(shot.complete);
    assert_eq!(shot.stamina_attack, 6.0);
    assert!(shot.advance(&rows, 50.0).is_empty());
}

#[test]
fn ready_light_edge_starts_upper_then_rows_and_does_not_repeat_on_hold() {
    let mut s = sim();
    ready(&mut s);
    let stamina = s.player(3).unwrap().stamina;
    let accuracy = s.player(3).unwrap().bow_accuracy.value;
    tick(&mut s, [true, false], [true; 2]);
    let p = s.player(3).unwrap();
    assert_eq!(p.bow.unwrap().phase, BowPhase::Shot);
    assert!(p.bow_shots.is_empty()); // countdown still positive
    assert_eq!(p.stamina, stamina);
    tick(&mut s, [true, false], [true; 2]);
    let p = s.player(3).unwrap();
    assert_eq!(p.bow.unwrap().upper.motion, 0x914);
    assert_eq!(p.bow_shots.len(), 1);
    assert_eq!(p.bow_shots[0].row.index, 7);
    assert_eq!(p.bow_shots[0].stamina_attack, 18.0);
    assert!(p.bow_shots[0].accuracy < accuracy); // preceding update filter
    assert_eq!(p.stamina, stamina - 9.0);
    for _ in 0..30 {
        tick(&mut s, [true, false], [true; 2]);
    }
    let p = s.player(3).unwrap();
    assert_eq!(p.bow.unwrap().phase, BowPhase::Ready);
    assert_eq!(
        p.bow_shots.iter().map(|e| e.row.index).collect::<Vec<_>>(),
        [7, 9]
    );
    assert_eq!(p.stamina, stamina - 9.0);
    assert!(s.player(7).unwrap().bow_shots.is_empty());
    assert_eq!(p.weapon, Weapon::Dagger);
    assert_eq!((p.attacks_started, p.volleys_fired), (0, 0));
}

#[test]
fn release_exits_without_shooting_and_seats_keep_independent_queues() {
    let mut s = sim();
    ready(&mut s);
    tick(&mut s, [false, true], [false, true]);
    assert_eq!(s.player(3).unwrap().bow.unwrap().phase, BowPhase::Released);
    assert!(s.player(3).unwrap().bow_shots.is_empty());
    tick(&mut s, [false, true], [false, true]);
    assert_eq!(s.player(7).unwrap().bow_shots.len(), 1);
    assert!(s.player(3).unwrap().bow_shots.is_empty());
}

#[test]
fn actual_stamina_spent_scales_additive_and_charge_reset_is_strict() {
    let mut s = sim();
    ready(&mut s);
    s.player_mut(3).unwrap().stamina = 2.0;
    tick(&mut s, [true, false], [true; 2]);
    tick(&mut s, [true, false], [true; 2]);
    assert_eq!(s.player(3).unwrap().bow_shots[0].stamina_attack, 4.0);
    assert_eq!(s.player(3).unwrap().stamina, 0.0);
    // Before each handler, upper frames are .375, .75, 1.125.
    tick(&mut s, [true, false], [true; 2]);
    tick(&mut s, [true, false], [true; 2]);
    assert!(!s.player(3).unwrap().bow_shot.charge_reset);
    tick(&mut s, [true, false], [true; 2]);
    assert!(s.player(3).unwrap().bow.is_none());
    assert!(!s.player(3).unwrap().bow_shot.charge_reset); // zero ends before charge reset
}

#[test]
fn accuracy_filter_matches_original_scalar_vtable_at_variable_steps() {
    // Private Unicorn execution of original 0x007BC800 and scalar vtable
    // 0x01599174, synthetic value1/velocity0/target.25/stiffness.15.
    let mut filter = BowAccuracy::default();
    let cases = [
        (0.25, 0.971875012, -0.028125001),
        (0.25, 0.943219841, -0.028655181),
        (0.25, 0.915609360, -0.027610488),
        (0.25, 0.889093161, -0.026516225),
        (1.0, 0.787252307, -0.101840824),
        (2.7, 0.539811254, -0.035449911),
        (0.0, 0.539811254, -0.035449911),
        (-0.5, 0.25, -0.035449911),
    ];
    for (frames, value, velocity) in cases {
        filter.advance(0.25, frames);
        assert!((filter.value - value).abs() < 2e-7, "{frames}: {filter:?}");
        assert!((filter.velocity - velocity).abs() < 2e-7, "{frames}: {filter:?}");
    }
}

#[test]
fn shot_weapon_tier_uses_secondary_level_gap_and_missing_motion_does_not_emit() {
    let mut s = sim();
    ready(&mut s);
    s.bow_shot_param.as_mut().unwrap().stamina = 0.0;
    s.bow_shot_param.as_mut().unwrap().weapon_stamina = 20.0;
    s.player_mut(3).unwrap().weapon_level_gap = 20;
    s.player_mut(7).unwrap().secondary_weapon_level_gap = 20;
    let before = s.player(7).unwrap().stamina;
    tick(&mut s, [true; 2], [true; 2]);
    tick(&mut s, [true; 2], [true; 2]);
    assert_eq!(s.player(3).unwrap().stamina, before); // primary level cannot debit bow
    assert_eq!(s.player(7).unwrap().stamina, before - 30.0);
    let mut s = sim();
    ready(&mut s);
    s.weapon_motions.get_mut(&Weapon::Bow).unwrap().remove(&0x914);
    for _ in 0..40 {
        tick(&mut s, [true, false], [true; 2]);
    }
    let p = s.player(3).unwrap();
    assert_eq!(p.bow.unwrap().phase, BowPhase::Shot);
    assert_eq!(
        p.bow.unwrap().unsupported,
        Some(ddsim::BowUnsupported::MissingMotion(0x914))
    );
    assert!(p.bow_shots.is_empty());
}
