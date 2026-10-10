use ddsim::{BowInput, BowParam, BowPhase, BowUnsupported, Ground, Input, RootMotion, Sim, Weapon};
use std::collections::HashMap;

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { (below >= 0.0).then_some(0.0) }
}

fn sim() -> Sim {
    let mut sim = Sim::new(HashMap::new());
    // Synthetic record deliberately uses different ids and unequal lengths;
    // this verifies runtime selection and independent clocks, not shipped values.
    sim.bow_param = Some(BowParam { draw_upper: 20, draw_lower: 21,
        release_upper: 22, release_lower: 23, wait_upper: 24, wait_lower: 25,
        fall: 26, end_at_zero_stamina: false, wait_stamina: 0.0 });
    sim.weapon_motions.insert(Weapon::Bow, [(0x914, 6), (0x915, 19),
        (0x916, 5), (0x917, 12), (0x918, 9), (0x919, 13)]
        .into_iter().map(|(id, frames)| (id, RootMotion::none(frames))).collect());
    for seat in [3, 7] {
        sim.add_player(seat, [0.0; 3], 0.0);
        let player = sim.player_mut(seat).unwrap();
        player.weapon = Weapon::Dagger;
        player.secondary_weapon = Weapon::Bow;
    }
    sim
}

fn step(sim: &mut Sim, seat: u8, input: BowInput) {
    sim.player_mut(seat).unwrap().bow_context = input;
    sim.step(1.0 / 30.0, &HashMap::from([(seat, Input { bow: input.held,
        bow_toggle: input.toggle, ..Default::default() })]), &Flat);
}

#[test]
fn draw_is_upper_gated_and_release_cannot_interrupt_it() {
    let mut sim = sim();
    step(&mut sim, 3, BowInput { held: true, ..Default::default() });
    let bow = sim.player(3).unwrap().bow.unwrap();
    assert_eq!((bow.upper.motion, bow.lower.motion), (0x914, 0x915));
    assert_eq!(bow.phase, BowPhase::Draw);
    assert_eq!(sim.player(3).unwrap().weapon, Weapon::Dagger);
    assert!(sim.player(7).unwrap().bow.is_none());
    for _ in 0..4 { step(&mut sim, 3, BowInput::default()); }
    assert_eq!(sim.player(3).unwrap().bow.unwrap().phase, BowPhase::Draw);
    step(&mut sim, 3, BowInput::default());
    assert_eq!(sim.player(3).unwrap().bow.unwrap().phase, BowPhase::Released);
    assert_eq!(sim.player(3).unwrap().bow.unwrap().upper.rate, 1.2);
    for _ in 0..4 { step(&mut sim, 3, BowInput::default()); }
    assert!(sim.player(3).unwrap().bow.is_none());
    assert_eq!(sim.player(3).unwrap().attacks_started, 0);
    assert_eq!(sim.player(3).unwrap().volleys_fired, 0);
}

#[test]
fn toggle_is_per_seat_and_release_restrictions_are_observed() {
    let mut sim = sim();
    step(&mut sim, 7, BowInput { toggle: true, ..Default::default() });
    for _ in 0..10 { step(&mut sim, 7, BowInput { toggle: true, ..Default::default() }); }
    let bow = sim.player(7).unwrap().bow.unwrap();
    assert_eq!(bow.phase, BowPhase::Ready);
    assert_eq!((bow.upper.motion, bow.lower.motion), (0x918, 0x919));
    step(&mut sim, 7, BowInput::default()); // key release does not release latch
    assert_eq!(sim.player(7).unwrap().bow.unwrap().phase, BowPhase::Ready);
    step(&mut sim, 7, BowInput { toggle: true, release_blocked: true, ..Default::default() });
    assert_eq!(sim.player(7).unwrap().bow.unwrap().phase, BowPhase::Ready);
    step(&mut sim, 7, BowInput::default());
    assert_eq!(sim.player(7).unwrap().bow.unwrap().phase, BowPhase::Released);
    assert!(sim.player(3).unwrap().bow.is_none());
}

#[test]
fn lower_draw_finishing_first_does_not_finish_upper_draw() {
    let mut sim = sim();
    let bank = sim.weapon_motions.get_mut(&Weapon::Bow).unwrap();
    bank.insert(0x914, RootMotion::none(19));
    bank.insert(0x915, RootMotion::none(3));
    for _ in 0..10 { step(&mut sim, 3, BowInput { held: true, ..Default::default() }); }
    let bow = sim.player(3).unwrap().bow.unwrap();
    assert_eq!(bow.phase, BowPhase::Draw);
    assert!(bow.lower.frame >= 2.0);
    for _ in 0..9 { step(&mut sim, 3, BowInput { held: true, ..Default::default() }); }
    assert_eq!(sim.player(3).unwrap().bow.unwrap().phase, BowPhase::Ready);
}

#[test]
fn upper_ready_request_does_not_reorder_the_lower_handler() {
    // vf06 runs B17DA0/B18CB0 before B1AC90/vf25. Upper draw end
    // requests upper Ready now; lower still has the earlier draw request.
    let sim = sim();
    let param = sim.bow_param.unwrap();
    let mut bow = ddsim::BowState::draw(param);
    assert!(!bow.update(param, true, true, true));
    assert_eq!(bow.phase, BowPhase::Ready);
    assert_eq!(bow.upper.motion, 0x918);
    assert_eq!(bow.lower.motion, 0x915);
}

#[test]
fn check34_and_secondary_slot_gate_entry() {
    let mut sim = sim();
    step(&mut sim, 3, BowInput { held: true, unit_mode: 15, stage_mode: 1, ..Default::default() });
    assert!(sim.player(3).unwrap().bow.is_none());
    step(&mut sim, 3, BowInput { held: true, unit_mode: 15, stage_mode: 2, ..Default::default() });
    assert!(sim.player(3).unwrap().bow.is_some());
    sim.player_mut(3).unwrap().secondary_weapon = Weapon::None;
    step(&mut sim, 3, BowInput { held: true, ..Default::default() });
    assert!(sim.player(3).unwrap().bow.is_none());
}

#[test]
fn untranslated_shot_and_movement_do_not_fire_or_run_dagger_attacks() {
    let mut sim = sim();
    sim.step(1.0 / 30.0,
        &HashMap::from([(3, Input { bow: true, light: true, ..Default::default() })]), &Flat);
    assert_eq!(sim.player(3).unwrap().bow.unwrap().unsupported, Some(BowUnsupported::Shot));
    assert_eq!(sim.player(3).unwrap().attacks_started, 0);
    sim.step(1.0 / 30.0,
        &HashMap::from([(3, Input { bow: true, stick: [0.0, 1.0], ..Default::default() })]), &Flat);
    assert_eq!(sim.player(3).unwrap().bow.unwrap().unsupported, Some(BowUnsupported::Movement));
    assert_eq!(sim.player(3).unwrap().position, [0.0; 3]);
}

#[test]
fn missing_bow_motion_cannot_finish_using_a_default_timer() {
    let mut sim = sim();
    sim.weapon_motions.clear();
    for _ in 0..100 { step(&mut sim, 3, BowInput { held: true, ..Default::default() }); }
    let bow = sim.player(3).unwrap().bow.unwrap();
    assert_eq!(bow.phase, BowPhase::Draw);
    assert_eq!(bow.unsupported, Some(BowUnsupported::MissingMotion(0x914)));
}
