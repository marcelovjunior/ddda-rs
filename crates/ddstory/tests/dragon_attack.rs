//! Pure source-regression checks; actor inputs are synthetic, not recordings.
// The shared lib.rs export is coordinated with the prologue branch.
#[path = "../src/dragon.rs"]
mod dragon;
use dragon::{ChallengeDragon, ChallengePlayer, DragonChallenge};

fn hp(hp: f32) -> Option<ChallengeDragon> {
    Some(ChallengeDragon { life_hp: Some(hp) })
}

#[test]
fn dragon_damage_is_strictly_more_than_four_since_first_enabled_sample() {
    let mut gate = DragonChallenge::default();
    assert!(!gate.update(false, false, hp(1000.0), &[]));
    assert_eq!(gate.baseline_hp, None);
    assert!(!gate.update(true, false, None, &[]));
    assert_eq!(gate.baseline_hp, None);
    assert!(!gate.update(true, false, hp(100.0), &[]));
    assert!(!gate.update(true, false, hp(96.0), &[]));
    assert!(gate.update(true, false, hp(95.999), &[]));
    assert_eq!(gate.baseline_hp, Some(100.0));
    assert!(!gate.update(true, true, hp(0.0), &[]));
    assert!(!gate.update(true, false, hp(f32::NAN), &[]));
}

#[test]
fn any_valid_arisen_can_end_the_fight_independently_of_dragon_flag() {
    let mut gate = DragonChallenge::default();
    let healthy = ChallengePlayer {
        life_hp: Some(100.0),
        effective_max_hp: 100.0,
    };
    let low = ChallengePlayer {
        life_hp: Some(1.0),
        effective_max_hp: 100.0,
    };
    assert!(!gate.update(false, false, None, &[healthy]));
    assert!(gate.update(false, false, None, &[healthy, low]));
    assert_eq!(gate.baseline_hp, None);
    assert!(!gate.update(false, true, None, &[low]));
    let dead = ChallengePlayer {
        effective_max_hp: 0.0,
        ..low
    };
    assert!(!gate.update(false, false, None, &[dead]));
    let missing = ChallengePlayer { life_hp: None, ..low };
    assert!(gate.update(false, false, None, &[missing]));
    assert!(!gate.update(
        false,
        false,
        None,
        &[ChallengePlayer {
            effective_max_hp: f32::NAN,
            ..low
        }]
    ));
    assert!(!gate.update(
        false,
        false,
        None,
        &[ChallengePlayer {
            life_hp: Some(f32::NAN),
            ..low
        }]
    ));
}

#[test]
fn present_dragon_without_life_region_latches_zero_once() {
    let mut gate = DragonChallenge::default();
    assert!(!gate.update(true, false, Some(ChallengeDragon { life_hp: None }), &[]));
    assert_eq!(gate.baseline_hp, Some(0.0));
    assert!(!gate.update(true, false, hp(100.0), &[]));
    assert!(!gate.update(true, false, hp(0.0), &[]));
}
