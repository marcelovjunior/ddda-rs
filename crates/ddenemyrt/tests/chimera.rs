// Isolated until the shared lib.rs owner adds `pub mod chimera`.
#[path = "../src/chimera.rs"]
mod chimera;
use chimera::*;

#[test]
fn installed_catalogues_keep_children_and_death_variants_distinct() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !native.is_dir() {
        eprintln!("DDDA install absent; skipped catalogue regression");
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    for (unit, count, resource_count, death, class) in [
        (Unit::Body, 142, 86, 0x57, "cEm5200ActDie"),
        (Unit::Child00, 85, 34, 0x32, "cEm5200_00ActDie"),
        (Unit::Child01, 90, 27, 0x38, "cEm5200_01ActDie"),
    ] {
        let data = Catalogue::read(&exe, unit).unwrap();
        assert_eq!(data.unit, unit);
        assert_eq!(data.actions.len(), count);
        assert_eq!(data.resources.len(), resource_count);
        let expected_bound = match unit {
            Unit::Body => 41,
            Unit::Child00 => 28,
            Unit::Child01 => 27,
        };
        assert_eq!(
            data.resources.iter().filter(|r| r.flags & 1 != 0).count(),
            expected_bound
        );
        assert!(data.resources.iter().all(|r| r.flags & 6 == 2));
        assert_eq!(data.actions[death].class, class);
        assert!(data.actions.iter().all(|r| r.class.starts_with('c')));
        if unit == Unit::Body {
            assert_eq!(data.actions[0x3a].class, "cEm5200ActFall");
            assert_eq!(data.actions[0x3a].argument, 1);
            for variant in 0..7 {
                assert_eq!(data.actions[0x52 + variant].class, "cEm5200ActDie");
                assert_eq!(data.actions[0x52 + variant].argument, variant as u32);
            }
        } else {
            assert_eq!(data.actions[death].argument, 10);
        }
        let regions = data.regions(&vfs).unwrap();
        assert_eq!(regions.len(), if unit == Unit::Body { 3 } else { 1 });
        let own = regions[0].1.regions.iter().find(|r| r.kind == 0).unwrap();
        assert_eq!(own.elements.len(), if unit == Unit::Body { 6 } else { 2 });
        assert_eq!(data.hit_to_pool.len(), 12);
        for hit_region in 0..12 {
            let index = data.pool_index(hit_region, &own.elements).unwrap();
            assert_eq!(own.elements[index].id, data.hit_to_pool[hit_region]);
        }
        let mut reordered = own.elements.clone();
        reordered.reverse();
        assert_eq!(data.pool_index(0, &reordered), Some(reordered.len() - 1));
        assert_eq!(data.pool_index(12, &own.elements), None);
        assert_eq!(data.pool_index(0, &[]), None);
        for (path, region) in regions {
            assert!(region.regions.iter().any(|r| r.kind == 0));
            eprintln!("{unit:?} {path}: {} region sets", region.regions.len());
        }
        for request in &data.resources {
            if matches!(request.class.as_str(), "rModel" | "rMotionList" | "rObjCollision") {
                assert!(
                    vfs.locate(&request.path, mtarc::type_hash(&request.class))
                        .is_some(),
                    "missing {request:?}"
                );
            }
        }
    }
}

#[test]
fn resource_initializers_preserve_upper_flags_and_binding_exceptions() {
    for (unit, index, bound) in [
        (Unit::Body, 0, true),
        (Unit::Body, 1, false),
        (Unit::Body, 39, true),
        (Unit::Body, 40, false),
        (Unit::Body, 55, false),
        (Unit::Body, 56, true),
        (Unit::Body, 57, false),
        (Unit::Body, 85, true),
        (Unit::Child00, 25, true),
        (Unit::Child00, 26, false),
        (Unit::Child00, 31, false),
        (Unit::Child00, 32, true),
        (Unit::Child00, 33, true),
        (Unit::Child01, 0, true),
        (Unit::Child01, 26, true),
    ] {
        for stored in [0, 7, 0xffff_ffff, 0x1234_5678] {
            let flags = initialized_resource_flags(unit, index, stored);
            assert_eq!(flags & !7, stored & !7);
            assert_eq!(flags & 7, if bound { 3 } else { 2 });
        }
    }
}

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
