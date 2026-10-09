// Isolated until the shared lib.rs owner adds `pub mod chimera`.
#[path = "../src/chimera.rs"]
mod chimera;
use chimera::*;

fn hit() -> DeathContext {
    DeathContext {
        has_damage_context: true,
        already_dead: false,
    }
}

#[test]
fn body_death_requires_context_and_priority_acceptance() {
    for context in [
        DeathContext::default(),
        DeathContext {
            already_dead: true,
            ..hit()
        },
    ] {
        let mut p = Priorities::default();
        assert_eq!(body_death(&mut p, context, BodyState::default()), None);
        assert_eq!(p.pending, 0);
    }
    for bit in 0..32 {
        for current in [false, true] {
            let mut p = Priorities::default();
            if current {
                p.current = 1 << bit;
            } else {
                p.pending = 1 << bit;
            }
            assert_eq!(
                body_death(&mut p, hit(), BodyState::default()),
                (bit >= 2).then_some(0x3a)
            );
            assert_ne!(p.pending & 4, 0);
        }
    }
}

#[test]
fn body_death_pose_and_motion_overrides() {
    for (motion, expected) in [
        (0x212, 0x55),
        (0x213, 0x56),
        (0x227, 0x53),
        (0x228, 0x54),
        (0x239, 0x52),
        (0x233, 0x58),
        (0, 0x58),
    ] {
        let body = BodyState {
            motion,
            alternate_pose: true,
            action: 0,
        };
        assert_eq!(
            body_death(&mut Priorities::default(), hit(), body),
            Some(expected)
        );
        assert_eq!(
            body_death(
                &mut Priorities::default(),
                hit(),
                BodyState { action: 0x89, ..body }
            ),
            Some(0x57)
        );
        assert_eq!(
            body_death(
                &mut Priorities::default(),
                hit(),
                BodyState {
                    alternate_pose: false,
                    ..body
                }
            ),
            Some(0x3a)
        );
    }
}

#[test]
fn child01_guards_do_not_change_pending_mask() {
    for state in [
        Child01State {
            mode: 1,
            ..Default::default()
        },
        Child01State {
            action: 0x35,
            ..Default::default()
        },
        Child01State {
            action: 0x36,
            ..Default::default()
        },
        Child01State {
            parent_dead: true,
            ..Default::default()
        },
    ] {
        let mut p = Priorities::default();
        assert_eq!(child_death(Child::Unit01, &mut p, hit(), state), None);
        assert_eq!(p.pending, 0);
        assert_eq!(child_death(Child::Unit00, &mut p, hit(), state), Some(0x32));
    }
    for action in [0, 0x34, 0x37, u32::MAX] {
        assert_eq!(
            child_death(
                Child::Unit01,
                &mut Priorities::default(),
                hit(),
                Child01State {
                    action,
                    ..Default::default()
                }
            ),
            Some(0x38)
        );
    }
}

#[test]
fn child_notifications_cannot_override_parent_death() {
    for child in [Child::Unit00, Child::Unit01] {
        for alternate_pose in [false, true] {
            let body = BodyState {
                alternate_pose,
                ..Default::default()
            };
            let mut p = Priorities::default();
            assert!(body_death(&mut p, hit(), body).is_some());
            assert_eq!(parent_reaction(child, &mut p, body, Some(1.0)), None);
            assert_ne!(p.pending & (1 << if alternate_pose { 12 } else { 11 }), 0);
        }
    }
}

#[test]
fn parent_reactions_preserve_child_and_pose() {
    for (child, plain, delta, special, positive, negative) in [
        (Child::Unit00, 0x3c, 0, 0x8b, 0x45, 0x46),
        (Child::Unit01, 0x3d, 5, 0x8c, 0x47, 0x48),
    ] {
        assert_eq!(
            parent_reaction(child, &mut Priorities::default(), BodyState::default(), None),
            Some(plain)
        );
        for (motion, expected) in [
            (0x212, 0x67 + delta),
            (0x213, 0x68 + delta),
            (0x227, 0x65 + delta),
            (0x228, 0x66 + delta),
            (0x239, 0x64 + delta),
            (0x233, special),
            (0x234, special),
        ] {
            assert_eq!(
                parent_reaction(
                    child,
                    &mut Priorities::default(),
                    BodyState {
                        motion,
                        alternate_pose: true,
                        ..Default::default()
                    },
                    None
                ),
                Some(expected)
            );
        }
        for (side, expected) in [
            (Some(0.0), positive),
            (Some(-1.0), negative),
            (None, negative),
            (Some(f32::NAN), negative),
        ] {
            assert_eq!(
                parent_reaction(
                    child,
                    &mut Priorities::default(),
                    BodyState {
                        alternate_pose: true,
                        ..Default::default()
                    },
                    side
                ),
                Some(expected)
            );
        }
    }
}
