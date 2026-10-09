use ddenemyrt::chimera::*;

#[test]
fn installed_units_bind_bodies_and_keep_owner_pools_independent() {
    use ddcombat::damage::CollisionRates;
    use ddenemyrt::combat::Victim;
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !native.is_dir() {
        eprintln!("DDDA install absent; skipped chimera body regression");
        return;
    }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    for unit in [Unit::Body, Unit::Child00, Unit::Child01] {
        let data = ChimeraData::load(&vfs, &exe, unit, 1.25).unwrap();
        assert_eq!(data.body.scale, 1.25);
        assert_eq!(data.damage_bonus.is_some(), unit == Unit::Body);
        if let Some(bonus) = data.damage_bonus {
            let resource =
                ddenemy::EnemyData::load(&vfs, ddenemy::EnemyId::parse("em5200").unwrap()).unwrap();
            let object = &resource.specific.unwrap().object;
            assert_eq!(bonus, BodyDamageBonus::from_object(object).unwrap());
        }
        assert!(!data.body.skeleton.joints.is_empty());
        let motions: Vec<_> = data
            .catalogue
            .resources
            .iter()
            .filter(|r| r.flags & 1 != 0 && r.class == "rMotionList")
            .collect();
        assert_eq!(data.body.banks.len(), motions.len());
        assert!(!motions.is_empty());
        for (bank, request) in data.body.banks.iter().zip(motions) {
            assert_eq!(bank.as_ref().unwrap().path, request.path);
        }
        assert!(!data.body.collision.is_empty());
        assert_eq!(
            data.body.collision.len(),
            data.catalogue
                .resources
                .iter()
                .filter(|r| r.flags & 1 != 0 && r.class == "rObjCollision")
                .count()
        );
        let mut first = data.fighter(
            0,
            CollisionRates {
                flags: 0x19,
                damage: 0.5,
                ..Default::default()
            },
        );
        let second = data.fighter(37, CollisionRates::default());
        assert_eq!((first.owner, second.owner), (0, 37));
        let original = data.regions[0].hp;
        let Victim::Enemy { regions, rates, .. } = &mut first.victim else {
            panic!()
        };
        assert_eq!(rates.flags, 0x19);
        assert_eq!(rates.damage, 0.5);
        regions[0].hp = 0.0;
        assert_eq!(second.victim.hp(), original);
        assert_eq!(data.regions[0].hp, original);
        assert!(first.attacks.is_empty() && first.hurts.is_empty());
    }
}

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

mod linked_hits {
    use ddcombat::cstatus::StatusTable;
    use ddcombat::damage::*;
    use ddenemyrt::combat::{apply_linked_enemy_hit, Attacker, Fighter, HitEvent, Victim};

    #[test]
    fn body_magick_bonus_uses_resolved_pool_and_child_liveness_only() {
        let bonus = super::BodyDamageBonus {
            child00_alive: 2.0,
            child00_missing_or_dead: 3.0,
        };
        let mut original = hit();
        original.mag = 7.0;
        original.has_mag = true;
        for pool_id in [0, 1, 2, 11, u32::MAX] {
            for suppressed in [false, true] {
                for alive in [false, true] {
                    let mut actual = original.clone();
                    bonus.apply(&mut actual, pool_id, suppressed, alive);
                    let mut expected = original.clone();
                    if pool_id == 1 && !suppressed {
                        expected.mag = if alive { 14.0 } else { 21.0 };
                    }
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    // Synthetic fixtures deliberately differ from installed creature values.
    fn pools() -> Vec<RegionPool> {
        (0..2)
            .map(|id| {
                RegionPool::from_element(
                    id,
                    100.0,
                    30.0,
                    0.0,
                    40.0,
                    0.0,
                    1.0,
                    1.0,
                    attr::LINK_HP,
                    1,
                    75.0,
                    30.0,
                )
            })
            .collect()
    }

    fn hit() -> MergedHit {
        MergedHit {
            row: AttackRow {
                sword_rate: 100.0,
                shrink_type: 1,
                blow_type: 0,
                ..Default::default()
            },
            phys: 60.0,
            mag: 0.0,
            element_buildup: [0.0; 6],
            element: 0,
            shrink: 10.0,
            blow: 10.0,
            stamina: 0.0,
            status: [0.0; 15],
            hits: 1,
            has_phys: true,
            has_mag: false,
        }
    }

    fn victim() -> Fighter {
        Fighter {
            owner: 103,
            attacker: Attacker::Enemy(EnemyAttacker::new(1.0, 1.0)),
            victim: Victim::Enemy {
                defender: EnemyDefender::new(10000.0, 10000.0),
                regions: pools(),
                rates: CollisionRates {
                    damage: 0.0,
                    ..Default::default()
                },
            },
            attacks: vec![],
            hurts: vec![],
            history: Default::default(),
            statuses: Default::default(),
            dots: Default::default(),
            pools: Default::default(),
        }
    }

    fn source(owner: u32) -> HitEvent {
        HitEvent {
            attacker: owner,
            victim: 104,
            region: 4,
            hits: 2,
            total: 17.0,
            reaction: Reaction::default(),
            killed: false,
        }
    }

    fn regions(v: &Fighter) -> &[RegionPool] {
        let Victim::Enemy { regions, .. } = &v.victim else {
            unreachable!()
        };
        regions
    }

    fn regions_mut(v: &mut Fighter) -> &mut Vec<RegionPool> {
        let Victim::Enemy { regions, .. } = &mut v.victim else {
            unreachable!()
        };
        regions
    }

    #[test]
    fn valued_wrapper_preserves_nonlinked_damage_and_exposes_scaled_hit() {
        let mut game = Adjust(vec![1.0; 0x40]);
        game.0[0x22] = 0.5;
        game.0[0x25] = 0.5;
        let def = EnemyDefender::new(20.0, 0.0);
        let mut old = pools();
        let mut new = old.clone();
        let rates = CollisionRates::default();
        let expected = resolve_enemy_hit(hit(), &def, &mut old, 1, &rates, &game, 1.0);
        let (actual, valued) = resolve_enemy_hit_valued(hit(), &def, &mut new, 1, &rates, &game, 1.0);
        assert_eq!(actual, expected);
        assert_eq!(old, new);
        // Synthetic hit: (60 physical - 20 defence) * 0.5 region-class
        // scaling = 20, propagated from pool 1 to main through LINK_HP.
        assert_eq!(valued.hit.phys, 20.0);
        assert_eq!(valued.hit.mag, 0.0);
        assert_eq!(actual.total, 20.0);
        assert_eq!(new.iter().map(|r| r.hp).collect::<Vec<_>>(), [80.0, 80.0]);
        assert_eq!(valued.total, actual.total);
        assert!(valued.hit.phys < hit().phys);
        assert_eq!(valued.total, valued.hit.phys + valued.hit.mag);
    }

    #[test]
    fn linked_hit_skips_second_mitigation_and_keeps_owner_metadata() {
        for owner in [0, 1, 37] {
            let mut v = victim();
            let untouched = regions(&v)[1].clone();
            // Total intentionally differs from phys+mag: the carried total
            // is authoritative, as with an original HitInfo+0x7c override.
            let valued = ValuedEnemyHit {
                hit: hit(),
                total: 17.0,
            };
            let out = apply_linked_enemy_hit(
                &mut v,
                &source(owner),
                &valued,
                Some(1),
                [1.0; 2],
                &StatusTable::default(),
                &mut || 0.0,
            )
            .unwrap();
            assert_eq!(
                (out.attacker, out.victim, out.region, out.hits),
                (owner, 103, 4, 2)
            );
            assert_eq!(out.total, 17.0);
            assert!(!out.killed);
            assert_eq!(regions(&v)[0].hp, 83.0);
            assert_eq!(regions(&v)[0].dp, 20.0);
            assert_eq!(regions(&v)[0].bp, 30.0);
            assert_eq!(regions(&v)[1], untouched);
        }
    }

    #[test]
    fn linked_hp_requires_mapping_and_attribute_but_break_and_status_do_not() {
        for mapping in [None, Some(99), Some(1)] {
            let mut v = victim();
            regions_mut(&mut v)[1].attr = 0;
            let mut valued = ValuedEnemyHit {
                hit: hit(),
                total: 500.0,
            };
            valued.hit.status[0] = 5.0;
            let out = apply_linked_enemy_hit(
                &mut v,
                &source(1),
                &valued,
                mapping,
                [2.0, 3.0],
                &StatusTable::default(),
                &mut || 0.0,
            )
            .unwrap();
            assert_eq!(out.total, 0.0);
            assert!(!out.killed);
            assert_eq!(regions(&v)[0].hp, 100.0);
            assert_eq!(regions(&v)[0].dp, 20.0);
            assert_eq!(regions(&v)[0].status.acc[0], 10.0);
        }
    }

    #[test]
    fn linked_main_hp_no_death_and_break_flags_are_preserved() {
        for no_death in [false, true] {
            let mut v = victim();
            let Victim::Enemy { rates, .. } = &mut v.victim else {
                unreachable!()
            };
            rates.flags = 0x18;
            let mut valued = ValuedEnemyHit {
                hit: hit(),
                total: 500.0,
            };
            valued.hit.row.is_no_death = no_death;
            let out = apply_linked_enemy_hit(
                &mut v,
                &source(0),
                &valued,
                Some(0),
                [1.0; 2],
                &StatusTable::default(),
                &mut || 0.0,
            )
            .unwrap();
            assert_eq!(regions(&v)[0].hp, if no_death { 1.0 } else { 0.0 });
            assert_eq!(out.killed, !no_death);
            assert_eq!((regions(&v)[0].dp, regions(&v)[0].bp), (30.0, 40.0));
            assert_eq!(regions(&v)[1].hp, 100.0);
        }
    }

    #[test]
    fn linked_status_uses_carried_magick_and_frozen_cancels_break() {
        let mut v = victim();
        let mut valued = ValuedEnemyHit {
            hit: hit(),
            total: 1.0,
        };
        valued.hit.mag = 1.0;
        valued.hit.status[0] = 5.0;
        valued.hit.shrink = 100.0;
        valued.hit.blow = 100.0;
        valued.hit.element = 2;
        valued.hit.element_buildup[2] = 5.0;
        regions_mut(&mut v)[0].status.threshold[11] = 10.0;
        let out = apply_linked_enemy_hit(
            &mut v,
            &source(0),
            &valued,
            Some(1),
            [2.0, 3.0],
            &StatusTable::default(),
            &mut || 0.0,
        )
        .unwrap();
        assert_eq!(regions(&v)[0].status.acc[0], 15.0);
        assert_eq!(out.reaction, Reaction::default());
        assert_eq!((regions(&v)[0].dp, regions(&v)[0].bp), (30.0, 40.0));
    }
}
