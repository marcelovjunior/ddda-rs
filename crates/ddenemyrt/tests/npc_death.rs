use ddenemyrt::npc_death::{CorpseIntel, DeathAction, DeathEffects, DeathPhase};

#[test]
fn corpse_intel_uses_main_clock_only_after_apparent_death() {
    let mut intel = CorpseIntel::new(30.0).unwrap();
    assert_eq!((intel.wait, intel.alpha), (1800.0, 1.0));
    for (apparent, permanent, flags) in [(false, false, 0), (true, true, 0), (true, false, 0x0040_0000)] {
        intel.advance(apparent, permanent, flags, 100.0).unwrap();
        assert_eq!(intel.wait, 1800.0);
    }
    intel.advance(true, false, 0, 1.25).unwrap();
    assert_eq!(intel.wait, 1798.75);
    intel.advance(true, false, 0, 2000.0).unwrap();
    assert_eq!(intel.wait, 0.0);
    assert_eq!(
        intel.alpha, 1.0,
        "intel countdown does not perform the actor fade"
    );
}

#[test]
fn death_action_emits_each_native_boundary_once_and_separates_wait_from_fade() {
    let mut action = DeathAction::new(false);
    let mut intel = CorpseIntel::new(30.0).unwrap();
    // An observed end cannot skip the native initialization phase.
    assert_eq!(
        action.advance(true, 0, Some(&mut intel), 1.0).unwrap(),
        DeathEffects::default()
    );
    assert_eq!(action.phase(), DeathPhase::Motion);
    assert_eq!(
        action.advance(false, 0, Some(&mut intel), 1.0).unwrap(),
        DeathEffects::default()
    );
    assert_eq!(
        action.advance(true, 0, Some(&mut intel), 1.0).unwrap(),
        DeathEffects {
            apparent: true,
            ..Default::default()
        }
    );
    assert_eq!(action.phase(), DeathPhase::CorpseWait);
    assert_eq!(intel.wait, 1800.0, "actor clock does not advance intel wait");
    assert_eq!(
        action.advance(true, 0, Some(&mut intel), 1.0).unwrap(),
        DeathEffects::default()
    );
    intel.wait = 0.0;
    assert_eq!(
        action.advance(true, 0x0040_0000, Some(&mut intel), 1.0).unwrap(),
        DeathEffects::default()
    );
    assert_eq!(action.phase(), DeathPhase::CorpseWait);
    assert_eq!(
        action.advance(true, 0, Some(&mut intel), 1.0).unwrap(),
        DeathEffects {
            begin_fade: true,
            ..Default::default()
        }
    );
    assert_eq!(intel.alpha, 1.0, "phase2 does not fall through into phase3");
    // Native phase3 does not re-check the hold flag after fading has begun.
    assert_eq!(
        action.advance(false, 0x0040_0000, Some(&mut intel), 3.0).unwrap(),
        DeathEffects::default()
    );
    assert!((intel.alpha - 0.9).abs() < 1e-6);
    intel.alpha = 0.1;
    assert_eq!(
        action.advance(false, 0, Some(&mut intel), 3.0).unwrap(),
        DeathEffects {
            permanent: true,
            ..Default::default()
        }
    );
    assert_eq!(action.phase(), DeathPhase::Finished);
    assert_eq!(
        intel.alpha, 0.1,
        "native removal branch does not store the terminal alpha"
    );
    assert_eq!(
        action.advance(false, 0, Some(&mut intel), 3.0).unwrap(),
        DeathEffects::default()
    );
}

#[test]
fn apparent_reload_and_missing_intel_follow_distinct_native_paths() {
    let mut held = DeathAction::new(true);
    let mut free = DeathAction::new(true);
    assert_eq!(held.phase(), DeathPhase::CorpseWait);
    assert_eq!(
        held.advance(false, 0x0040_0000, None, 1.0).unwrap(),
        DeathEffects::default()
    );
    assert_eq!(
        free.advance(false, 0, None, 1.0).unwrap(),
        DeathEffects {
            begin_fade: true,
            ..Default::default()
        }
    );
    for _ in 0..100 {
        assert_eq!(
            free.advance(false, 0, None, 1.0).unwrap(),
            DeathEffects::default()
        );
    }
    assert_eq!(
        free.phase(),
        DeathPhase::Fade,
        "missing intel re-reads alpha1 each frame"
    );
    assert_eq!(
        free.advance(false, 0, None, 30.0).unwrap(),
        DeathEffects {
            permanent: true,
            ..Default::default()
        }
    );
    assert_eq!(
        held.phase(),
        DeathPhase::CorpseWait,
        "other NPC actions have independent phases"
    );
}

#[test]
fn death_helpers_reject_invalid_clocks_and_do_not_hide_nan_alpha() {
    assert!(CorpseIntel::new(f32::INFINITY).is_err());
    assert!(CorpseIntel::new(f32::MAX).is_err());
    let mut action = DeathAction::new(true);
    assert!(action.advance(false, 0, None, -1.0).is_err());
    assert_eq!(action.phase(), DeathPhase::CorpseWait);
    action.advance(false, 0, None, 1.0).unwrap();
    let mut intel = CorpseIntel {
        wait: 0.0,
        alpha: f32::NAN,
    };
    assert!(action.advance(false, 0, Some(&mut intel), 1.0).is_err());
    assert_eq!(action.phase(), DeathPhase::Fade);
    intel.alpha = -1.0;
    assert!(action.advance(false, 0, Some(&mut intel), 0.0).unwrap().permanent);
}
