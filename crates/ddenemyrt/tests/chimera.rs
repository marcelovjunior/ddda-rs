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
    use ddcombat::volumes::{AttackVolume, HurtVolume, Side, Volume};
    use ddenemyrt::combat::{apply_linked_enemy_hit, Attacker, Fighter, HitEvent, Victim};
    use ddenemyrt::combat::{hit_pass, hit_pass_configured, EnemyCollisionConfig, Tables};
    use ddenemyrt::combat::{hit_pass_configured_with_policy, CombatOwnerState, EnemyHitPolicy};

    fn config(owner: u32, linked_owners: Vec<u32>) -> EnemyCollisionConfig {
        EnemyCollisionConfig {
            owner,
            hit_to_pool: vec![0, 1],
            linked_region_remap: None,
            linked_owners,
            status_rates: [1.0, 1.0],
        }
    }

    fn configured_scene(attacker: u32) -> (Vec<Fighter>, Tables) {
        let shape = Volume::Sphere { c: [0.0; 3], r: 1.0 };
        let mut attack = victim();
        attack.owner = attacker;
        attack.attacker = Attacker::Enemy(EnemyAttacker::new(60.0, 0.0));
        attack.attacks.push(AttackVolume {
            side: Side {
                owner: attacker,
                kind: 0,
            },
            volume: shape,
            row: AttackRow {
                attack_rate: 1.0,
                sword_rate: 100.0,
                ..Default::default()
            },
            flags: Default::default(),
            reset_time: 0.0,
            key: 0x10000,
            relation: 0,
        });
        let mut child = victim();
        child.owner = 104;
        child.hurts.push(HurtVolume {
            side: Side { owner: 104, kind: 1 },
            volume: shape,
            region_id: 1,
            hit_prio: 0,
        });
        if let Victim::Enemy { defender, rates, .. } = &mut child.victim {
            *defender = EnemyDefender::new(20.0, 0.0);
            *rates = CollisionRates::default();
        }
        let parent = victim(); // Defence 10000 and damage rate 0 must not mitigate relayed hits.
        let tables = Tables {
            game: Adjust(vec![1.0; 0x40]),
            ..Default::default()
        };
        (vec![attack, child, parent], tables)
    }

    #[test]
    fn configured_mapping_and_immediate_relay_preserve_n_owner_damage() {
        for attacker in [0, 1, 37] {
            let (mut fighters, mut tables) = configured_scene(attacker);
            tables.game.0[0x25] = 0.5;
            regions_mut(&mut fighters[1])[0].hp = 15.0;
            regions_mut(&mut fighters[2])[0].hp = 15.0;
            let mut later = fighters[1].clone();
            later.owner = 105;
            later.hurts[0].side.owner = 105;
            regions_mut(&mut later)[0].hp = 100.0;
            fighters.push(later);
            let mut child_config = config(104, vec![103]);
            fighters[1].hurts[0].region_id = 7;
            child_config.hit_to_pool = vec![999; 8];
            child_config.hit_to_pool[7] = 1;
            let mut parent_config = config(103, vec![]);
            // Remap raw region7 -> region0 -> pool1. Mirror HP still targets main.
            parent_config.linked_region_remap = Some(vec![0; 8]);
            parent_config.hit_to_pool = vec![1, 0];
            let configs = vec![child_config, parent_config, config(105, vec![])];
            let mut calls = Vec::new();
            let events = hit_pass_configured(
                &mut fighters,
                &tables,
                1.0,
                &mut || 1.0,
                &|_| true,
                &configs,
                &mut |owner, id, hit, state| {
                    calls.push((owner, id, hit.phys));
                    if owner == 105 {
                        assert!(!state.iter().find(|s| s.owner == 104).unwrap().has_hp);
                        assert!(!state.iter().find(|s| s.owner == 103).unwrap().has_hp);
                    }
                },
            )
            .unwrap();
            assert_eq!(
                events.iter().map(|e| (e.attacker, e.victim)).collect::<Vec<_>>(),
                vec![(attacker, 104), (attacker, 103), (attacker, 105)]
            );
            assert_eq!(events.iter().map(|e| e.total).collect::<Vec<_>>(), vec![20.0; 3]);
            assert!(events[0].killed && events[1].killed && !events[2].killed);
            assert_eq!(calls, vec![(104, 1, 40.0), (105, 1, 40.0)]);
            assert_eq!(regions(&fighters[2])[1].hp, 100.0);
            assert_eq!(regions(&fighters[3])[0].hp, 80.0);
        }
    }

    #[test]
    fn configured_pass_retains_legacy_without_links_or_overrides() {
        let (original, tables) = configured_scene(37);
        let mut legacy = original.clone();
        let mut configured = original;
        let expected = hit_pass(&mut legacy, &tables, 1.0, &mut || 1.0, &|_| true);
        let actual = hit_pass_configured(
            &mut configured,
            &tables,
            1.0,
            &mut || 1.0,
            &|_| true,
            &[config(104, vec![])],
            &mut |_, _, _, _| {},
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual[0].total, 40.0);
        for (a, b) in legacy.iter().zip(&configured) {
            assert_eq!(regions(a), regions(b));
            assert_eq!(a.history, b.history);
        }
        assert_eq!(configured[1].victim.hp(), 60.0);
    }

    #[test]
    fn configured_callback_cannot_modify_unconfigured_enemy() {
        let (mut fighters, tables) = configured_scene(37);
        let mut unconfigured = fighters[1].clone();
        unconfigured.owner = 105;
        unconfigured.hurts[0].side.owner = 105;
        fighters.push(unconfigured);
        #[derive(Default)]
        struct Policy {
            modified: Vec<u32>,
            observed: Vec<u32>,
        }
        impl EnemyHitPolicy for Policy {
            fn after_mitigation(&mut self, owner: u32, _: u32, hit: &mut MergedHit, _: &[CombatOwnerState]) {
                self.modified.push(owner);
                hit.phys *= 2.0;
            }
            fn after_hit(&mut self, event: &HitEvent) {
                self.observed.push(event.victim);
            }
        }
        let mut policy = Policy::default();
        let events = hit_pass_configured_with_policy(
            &mut fighters,
            &tables,
            1.0,
            &mut || 1.0,
            &|_| true,
            &[config(104, vec![])],
            &mut policy,
        )
        .unwrap();
        assert_eq!(policy.modified, vec![104]);
        assert_eq!(policy.observed, vec![104]);
        assert_eq!(
            events.iter().map(|e| (e.victim, e.total)).collect::<Vec<_>>(),
            vec![(104, 80.0), (105, 40.0)]
        );
        assert_eq!(fighters[1].victim.hp(), 20.0);
        assert_eq!(fighters[3].victim.hp(), 60.0);
    }

    #[test]
    fn three_simultaneous_owners_merge_once_before_linked_relay() {
        let (mut fighters, tables) = configured_scene(0);
        for owner in [1, 37] {
            let mut attacker = fighters[0].clone();
            attacker.owner = owner;
            attacker.attacks[0].side.owner = owner;
            fighters.insert(1, attacker);
        }
        let events = hit_pass_configured(
            &mut fighters,
            &tables,
            1.0,
            &mut || 1.0,
            &|_| true,
            &[config(104, vec![103]), config(103, vec![])],
            &mut |_, _, _, _| {},
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        for event in events {
            assert_eq!(event.attacker, 0); // first merged contact owns aggregate metadata
            assert_eq!(event.hits, 3);
            assert_eq!(event.total, 120.0); // (3*60) - (3*20), once per collision
            assert!(event.killed);
        }
        let child = fighters.iter().find(|f| f.owner == 104).unwrap();
        assert_eq!(
            child
                .history
                .entries
                .iter()
                .map(|e| e.attacker)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([0, 1, 37])
        );
    }

    #[test]
    fn hook_observes_current_hp_before_later_body_bonus() {
        let (mut fighters, tables) = configured_scene(0);
        fighters[0].attacker = Attacker::Enemy(EnemyAttacker::new(60.0, 60.0));
        let mut spell = fighters[0].attacks[0].clone();
        spell.volume = Volume::Sphere {
            c: [10.0, 0.0, 0.0],
            r: 1.0,
        };
        spell.key = 0x20000;
        spell.row.attack_rate = 0.0;
        spell.row.mgc_attack_rate = 1.0;
        fighters[0].attacks.push(spell.clone());
        regions_mut(&mut fighters[1])[0].hp = 15.0;
        fighters[2].hurts.push(HurtVolume {
            side: Side { owner: 103, kind: 1 },
            volume: spell.volume,
            region_id: 1,
            hit_prio: 0,
        });
        if let Victim::Enemy { defender, rates, .. } = &mut fighters[2].victim {
            *defender = EnemyDefender::new(10000.0, 20.0);
            *rates = CollisionRates::default();
        }
        let bonus = super::BodyDamageBonus {
            child00_alive: 2.0,
            child00_missing_or_dead: 3.0,
        };
        let mut body_calls = 0;
        let events = hit_pass_configured(
            &mut fighters,
            &tables,
            1.0,
            &mut || 1.0,
            &|_| true,
            &[config(104, vec![103]), config(103, vec![])],
            &mut |owner, pool, hit, state| {
                if owner == 103 {
                    body_calls += 1;
                    assert_eq!(hit.mag, 40.0); // common defence already subtracted
                                               // This synthetic policy exercises hook ordering, not original
                                               // actor +0x5bd0 death-marker timing. Host policy must supply that.
                    let alive = state.iter().find(|s| s.owner == 104).unwrap().has_hp;
                    assert!(!alive);
                    bonus.apply(hit, pool, false, alive);
                }
            },
        )
        .unwrap();
        assert_eq!(body_calls, 1); // mirrored hit never re-enters the modifier
        assert_eq!(
            events.iter().map(|e| (e.victim, e.total)).collect::<Vec<_>>(),
            vec![(104, 40.0), (103, 40.0), (103, 120.0)]
        );
        assert!(events[0].killed && events[2].killed);
    }

    #[test]
    fn configured_rejection_precedes_hp_history_and_rng_mutation() {
        for case in 0..7 {
            let (mut fighters, tables) = configured_scene(37);
            let mut configs = vec![config(104, vec![103]), config(103, vec![])];
            match case {
                0 => configs[0].hit_to_pool = vec![0],
                1 => configs[0].owner = 999,
                2 => configs.push(configs[0].clone()),
                3 => configs[0].linked_owners.push(103),
                4 => fighters[2].owner = 104,
                5 => configs[0].linked_owners = vec![999],
                6 => configs[0].hit_to_pool = vec![0, 999],
                _ => unreachable!(),
            }
            let before = fighters.clone();
            let result = hit_pass_configured(
                &mut fighters,
                &tables,
                1.0,
                &mut || panic!("preflight must not consume RNG"),
                &|_| panic!("preflight must not advance history"),
                &configs,
                &mut |_, _, _, _| panic!("preflight must not value damage"),
            );
            assert!(result.is_err(), "case {case}");
            for (a, b) in fighters.iter().zip(before) {
                assert_eq!(regions(a), regions(&b));
                assert_eq!(a.history, b.history);
            }
        }
    }

    #[test]
    fn immediate_policy_distinguishes_accepted_and_priority_blocked_death() {
        struct Policy {
            priority: super::Priorities,
            accepted: bool,
            order: Vec<(&'static str, u32)>,
        }
        impl EnemyHitPolicy for Policy {
            fn after_mitigation(
                &mut self,
                owner: u32,
                pool: u32,
                hit: &mut MergedHit,
                state: &[CombatOwnerState],
            ) {
                self.order.push(("mitigate", owner));
                if owner == 103 {
                    assert!(!state.iter().find(|s| s.owner == 104).unwrap().has_hp);
                    // HP is zero in both cases. Use accepted actor state instead.
                    super::BodyDamageBonus {
                        child00_alive: 2.0,
                        child00_missing_or_dead: 3.0,
                    }
                    .apply(hit, pool, false, !self.accepted);
                }
            }
            fn after_hit(&mut self, event: &HitEvent) {
                self.order.push(("observe", event.victim));
                if event.victim == 104 && event.killed {
                    let action = super::child_death(
                        super::Child::Unit00,
                        &mut self.priority,
                        super::DeathContext {
                            has_damage_context: true,
                            already_dead: self.accepted,
                        },
                        super::Child01State::default(),
                    );
                    if action.is_some() {
                        // Accepted child vf72 calls common vf72 -> vf119,
                        // 0x00ab37d0, which writes actor +0x5bd0 before relay.
                        self.accepted = true;
                    }
                }
            }
        }
        for blocked in [false, true] {
            let (mut fighters, tables) = configured_scene(0);
            fighters[0].attacker = Attacker::Enemy(EnemyAttacker::new(60.0, 60.0));
            let mut spell = fighters[0].attacks[0].clone();
            spell.volume = Volume::Sphere {
                c: [10.0, 0.0, 0.0],
                r: 1.0,
            };
            spell.key = 0x20000;
            spell.row.attack_rate = 0.0;
            spell.row.mgc_attack_rate = 1.0;
            fighters[0].attacks.push(spell.clone());
            regions_mut(&mut fighters[1])[0].hp = 15.0;
            fighters[2].hurts.push(HurtVolume {
                side: Side { owner: 103, kind: 1 },
                volume: spell.volume,
                region_id: 1,
                hit_prio: 0,
            });
            if let Victim::Enemy { defender, rates, .. } = &mut fighters[2].victim {
                *defender = EnemyDefender::new(10000.0, 20.0);
                *rates = CollisionRates::default();
            }
            let mut policy = Policy {
                priority: super::Priorities {
                    current: u32::from(blocked),
                    pending: 0,
                },
                accepted: false,
                order: Vec::new(),
            };
            let events = hit_pass_configured_with_policy(
                &mut fighters,
                &tables,
                1.0,
                &mut || 1.0,
                &|_| true,
                &[config(104, vec![103]), config(103, vec![])],
                &mut policy,
            )
            .unwrap();
            assert_eq!(fighters[1].victim.hp(), 0.0);
            assert_eq!(policy.accepted, !blocked);
            assert_eq!(events[2].total, if blocked { 80.0 } else { 120.0 });
            assert_eq!(
                policy.order,
                vec![
                    ("mitigate", 104),
                    ("observe", 104),
                    ("observe", 103),
                    ("mitigate", 103),
                    ("observe", 103)
                ]
            );
        }
    }

    #[test]
    fn configured_missing_mirror_pool_does_not_fall_back_to_hp() {
        let (mut fighters, tables) = configured_scene(0);
        let mut parent = config(103, vec![]);
        parent.hit_to_pool = vec![999];
        let events = hit_pass_configured(
            &mut fighters,
            &tables,
            1.0,
            &mut || 1.0,
            &|_| true,
            &[config(104, vec![103]), parent],
            &mut |_, _, _, _| {},
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].total, 40.0);
        assert_eq!(events[1].total, 0.0);
        assert_eq!(fighters[2].victim.hp(), 100.0);
    }

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
