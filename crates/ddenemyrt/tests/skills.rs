use ddenemyrt::{arisen::ArisenData, skills::SkillSet};

#[test]
fn anodyne_receipt_uses_global_delta_white_cap_and_native_dead_marker_retention() {
    use ddenemyrt::skills::healing::Receipt;
    let mut living = Receipt::default();
    let mut fallen = Receipt::default();
    let mut hp = [20.0, 0.0];
    living.receive(4.0);
    fallen.receive(4.0);
    assert_eq!(living.apply(false, 0.25, &mut hp[0], 20.5, 100.0), 0.5);
    assert_eq!(hp[0], 20.5);
    assert_eq!(living, Receipt::default());
    assert_eq!(fallen.apply(true, 0.25, &mut hp[1], 50.0, 100.0), 0.0);
    assert_eq!(hp[1], 0.0);
    assert_eq!(
        fallen,
        Receipt {
            pending: false,
            amount: 4.0
        }
    );
    // Clearing +31EA (PawnReturn admission) alone does not reapply a msg.
    assert_eq!(fallen.apply(false, 0.25, &mut hp[1], 50.0, 100.0), 0.0);
    fallen.receive(2.0);
    assert_eq!(fallen.apply(false, 0.25, &mut hp[1], 50.0, 100.0), 1.5);
    assert_eq!(fallen, Receipt::default());
    // HP zero without +31EA is not an admission/receipt death substitute.
    hp[1] = 0.0;
    fallen.receive(2.0);
    assert_eq!(fallen.apply(false, 1.0, &mut hp[1], 50.0, 100.0), 2.0);
    assert_eq!(hp[0], 20.5); // Independent actor-local receipt.
}

#[test]
fn anodyne_native_sphere_and_unit_parent_exit_use_distinct_radii_and_real_deltas() {
    use ddenemyrt::skills::healing::{admits, UnitExit};
    assert!(admits([0.0; 3], [0.0, 349.0, 0.0]));
    assert!(!admits([0.0; 3], [0.0, 350.0, 0.0]));
    assert!(!admits([0.0; 3], [250.0, 250.0, 0.0]));
    let mut exit = UnitExit::default();
    assert!(exit.update([0.0, 400.0, 0.0], [0.0; 3], true, 0.25, 0.0));
    assert!(!exit.update([0.0, 400.01, 0.0], [0.0; 3], true, 0.25, 0.0));
    assert_eq!(exit.elapsed, 0.25);
    assert_eq!(exit.gain(8.0, 2.0, false, 0.5), 1.0);
    assert!(exit.update([0.0; 3], [0.0; 3], true, 0.25, 0.0));
    assert_eq!(exit.elapsed, 0.0);
    assert_eq!(exit.gain(8.0, 2.0, false, 0.5), 4.0);
    assert!(!exit.update([0.0; 3], [0.0; 3], false, 0.5, 0.0));
    assert_eq!(exit.gain(8.0, 2.0, true, 0.5), 2.0);
}

#[test]
fn healing_spawn_queries_actual_joint_then_ground_without_a_guessed_feet_center() {
    use ddcombat::volumes::{Mat4, Pose, IDENTITY};
    struct Posed(bool);
    impl Pose for Posed {
        fn model_matrix(&self) -> Mat4 {
            IDENTITY
        }
        fn joint_matrix(&self, id: i32) -> Option<Mat4> {
            if id != 4 || !self.0 {
                return None;
            }
            let mut m = IDENTITY;
            m[3][1] = 20.0;
            Some(m)
        }
    }
    let mut queries = Vec::new();
    let mut ray = |from, to| {
        queries.push((from, to));
        if queries.len() == 1 {
            Some([0.0, 20.0, 100.0])
        } else {
            None
        }
    };
    let center =
        ddenemyrt::skills::healing::generator_position(&Posed(true), [0.0, 20.0, 200.0], &mut ray).unwrap();
    assert_eq!(queries[0], ([0.0, 20.0, 0.0], [0.0, 20.0, 200.0]));
    assert_eq!(center, [0.0, 20.0, -20.0]);
    assert_eq!(queries[1], ([0.0, 50.0, -20.0], [0.0, -9980.0, -20.0]));
    assert!(
        ddenemyrt::skills::healing::generator_position(&Posed(false), [0.0; 3], &mut |_, _| panic!(
            "missing joint must reject before querying"
        ))
        .is_err()
    );
}

#[test]
fn installed_anodyne_reader_follows_generator_spot_and_unit_without_attack_rows() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    assert!(
        native.exists(),
        "installed gate requires the player's installation"
    );
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let tables = ddskill::catalogue::ExeTables::from_image(&file.image().unwrap()).unwrap();
    let action = &tables.actions[5][(tables.skills[220].action & 0xffff) as usize];
    let magic = ddskill::magic::MagicList::parse(
        &ddenemyrt::body::load::read(&vfs, r"param\mag_act_param_07", "rMagicActParamList").unwrap(),
    )
    .unwrap();
    let magic = &magic.entries[action.params[2] as usize];
    let data = ddenemyrt::skills::healing::Data::load(&vfs, magic).unwrap();
    assert_eq!((magic.manual_type, data.joint), (0, -1));
    assert_eq!(data.offset, magic.levels[0].shells[0].offset);
    assert!(data.spot_lifetime > 0.0);
    assert!(data.target_count > 0 && data.target_count <= 10);
    assert!(data.reset_child && !data.clear_target_without_child);
    assert!(!data.one_hit && data.end_frame == 0.0);
    assert!(data.value > data.value_end && data.value_end > 0.0);
    assert!(data.generator_rows.iter().all(|(wait, _)| *wait == 0.0));
    // The recipient applies the installed rate at GLOBAL nominal delta,
    // with independent white caps; the reader produces no attack payload.
    let mut receipt = ddenemyrt::skills::healing::Receipt::default();
    let mut hp = 10.0;
    receipt.receive(data.value);
    let gain = receipt.apply(false, 0.25, &mut hp, 11.0, 100.0);
    assert!((gain - data.value * 0.25).abs() < 0.0001);
}

#[test]
fn native_contact_flags_use_actor_callback_facts_and_keep_unknown_faction_explicit() {
    use ddenemyrt::combat::{AcceptedContact, ContactActor};
    let mut contact = AcceptedContact {
        attacker: 0,
        victim: 100,
        key: 0,
        region: 0,
        attack_kind: 0,
        target: Some(ContactActor {
            character: true,
            enemy: true,
            vf117: false,
            faction: None,
        }),
        attacker_faction: None,
    };
    assert_eq!(contact.known_flags(), 0x19);
    contact.target.as_mut().unwrap().vf117 = true;
    assert_eq!(contact.known_flags(), 0x29);
    contact.target.as_mut().unwrap().enemy = false;
    contact.target.as_mut().unwrap().vf117 = false;
    contact.target.as_mut().unwrap().faction = Some(true);
    contact.attacker_faction = Some(false);
    assert_eq!(contact.known_flags(), 0xd1);
    contact.target.as_mut().unwrap().character = false;
    assert_eq!(contact.known_flags(), 1);
}

#[test]
fn checking_contact_precedes_invincibility_history_and_victim_damage_guard() {
    use ddcombat::damage::{AttackRow, EnemyAttacker, PlayerAttacker, RegionPool};
    use ddcombat::volumes::{AttackVolume, HurtVolume, Side, Volume};
    use ddenemyrt::combat::{Attacker, ContactActor, ContactObserver, Fighter, Tables, Victim};
    use ddenemyrt::npc_combat::{
        ordinary_collision_rates, ordinary_defender, DamageOwnerClass, GaugeHitGuard,
    };
    // Synthetic volumes isolate callback ordering; installed Blink below
    // independently uses the actual authored groups and posed skeletons.
    let attacker = Fighter {
        owner: 100,
        attacker: Attacker::Enemy(EnemyAttacker::new(0.0, 0.0)),
        victim: Victim::Player {
            defender: ordinary_defender(),
            hp: 100.0,
            hp_max: 100.0,
        },
        attacks: vec![AttackVolume {
            side: Side { owner: 100, kind: 1 },
            volume: Volume::Sphere { c: [0.0; 3], r: 1.0 },
            row: AttackRow {
                blow_type: 3,
                ..Default::default()
            },
            flags: Default::default(),
            reset_time: 0.0,
            key: 1 << 16,
            relation: 2,
        }],
        hurts: vec![],
        history: Default::default(),
        statuses: Default::default(),
        dots: Default::default(),
        pools: Default::default(),
    };
    let victim = Fighter {
        owner: 500,
        attacker: Attacker::Player(PlayerAttacker::new(0.0, 0.0, 0.0, 0.0)),
        victim: Victim::Npc {
            defender: ordinary_defender(),
            region: RegionPool::from_element(0, 100.0, 100.0, 0.3, 100.0, 0.3, 1.0, 1.0, 1, 0, 75.0, 30.0),
            rates: ordinary_collision_rates(0),
        },
        attacks: vec![],
        hurts: vec![HurtVolume {
            side: Side { owner: 500, kind: 3 },
            volume: Volume::Sphere { c: [0.0; 3], r: 1.0 },
            region_id: 0,
            hit_prio: 0,
        }],
        history: Default::default(),
        statuses: Default::default(),
        dots: Default::default(),
        pools: Default::default(),
    };
    let actor = |_| {
        Some(ContactActor {
            character: true,
            enemy: false,
            vf117: false,
            faction: None,
        })
    };
    let guards = [(
        500,
        GaugeHitGuard {
            gauge_enabled: true,
            ..Default::default()
        },
    )];
    let owners = [(100, DamageOwnerClass::Enemy, false)];
    let mut fighters = vec![attacker, victim];
    for pass in 0..4 {
        if pass == 2 {
            let Victim::Npc { rates, .. } = &mut fighters[1].victim else {
                unreachable!()
            };
            rates.flags |= 4; // Native collision Invincible gate, after vf60.
        }
        let mut observed = Vec::new();
        let events = ddenemyrt::combat::hit_pass_with_npc_guards_and_contacts(
            &mut fighters,
            &Tables::default(),
            0.25,
            &mut || 0.0,
            &|_| true,
            &guards,
            &owners,
            None,
            Some(ContactObserver {
                is_check: &|_| pass != 3,
                actor: &actor,
                output: &mut observed,
            }),
        );
        assert!(events.is_empty()); // Gauge rejects zero damage; then history/Invincible.
        assert_eq!(observed.len(), usize::from(pass != 3));
        assert_eq!(fighters[1].victim.hp(), 100.0);
        if let Some(contact) = observed.first() {
            assert_eq!(
                (contact.attacker, contact.victim, contact.key),
                (100, 500, 1 << 16)
            );
        }
    }
}

#[test]
fn installed_skill_banks_and_shells() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.exists() {
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let sword = ArisenData::fighter(&vfs).unwrap();
    let mage = ArisenData::mage(&vfs).unwrap();
    let strider = ArisenData::strider(&vfs).unwrap();
    let skills = SkillSet::load(&vfs, &file.image().unwrap(), &sword, &mage, &strider).unwrap();

    assert_eq!(skills.0.len(), 7);
    for id in [41, 43, 210, 212] {
        let mut sim = ddsim::Sim::new(Default::default());
        sim.add_player(0, [0.; 3], 0.);
        sim.add_player(1, [1000., 0., 0.], 0.);
        skills.install(&mut sim);
        let skill = &skills.0[&id];
        let p = sim.player_mut(0).unwrap();
        p.weapon = skill.definition.weapon;
        p.skills.equipped[0] = Some(id);
        let initial = p.stamina;
        sim.request_skill(0, 0);
        let mut paid = None;
        let mut old = initial;
        for tick in 0..1000 {
            sim.step(1. / 120., &Default::default(), &Flat);
            let p = sim.player(0).unwrap();
            if p.stamina < old - 1. {
                assert!(paid.is_none(), "skill {id} charged twice");
                assert!((old - p.stamina - skill.definition.cost).abs() < 0.01);
                paid = Some(tick);
            }
            old = p.stamina;
            if tick > 0 && p.skills.active.is_none() {
                break;
            }
        }
        assert!(paid.is_some(), "skill {id} never charged");
        assert!(sim.player(0).unwrap().skills.active.is_none());
        let releases = &sim.player(0).unwrap().skills.released;
        assert_eq!(
            releases.as_slice(),
            if id >= 210 { std::slice::from_ref(&id) } else { &[] }
        );
        assert_eq!(sim.player(1).unwrap().stamina, initial);
        assert!(sim.player(1).unwrap().skills.active.is_none());

        // A new cast cannot override an unrelated staff attack, and a
        // host-forced fall interrupts an active skill before another tick.
        sim.player_mut(0).unwrap().state = ddsim::State::Shot {
            phase: ddsim::ShotPhase::Start,
        };
        sim.request_skill(0, 0);
        sim.step(1. / 120., &Default::default(), &Flat);
        assert!(sim.player(0).unwrap().skills.active.is_none());
        sim.player_mut(0).unwrap().state = ddsim::State::Wait;
        sim.request_skill(0, 0);
        sim.step(1. / 120., &Default::default(), &Flat);
        assert!(sim.player(0).unwrap().skills.active.is_some());
        sim.player_mut(0).unwrap().state = ddsim::State::Fall;
        sim.step(1. / 120., &Default::default(), &Flat);
        assert!(sim.player(0).unwrap().skills.active.is_none());
    }
    let mut spells = ddenemyrt::skills::spells::SpellWorld::default();
    let mut owner = 1000;
    let data = skills.0[&210].spell.as_ref().unwrap();
    spells.fire(
        data,
        210,
        0,
        &skills.0[&210].release_pose([0.; 3], 0.),
        Some([0., 100., 500.]),
        &mut owner,
    );
    assert_eq!(spells.shells.len(), 1);
    assert_eq!(spells.shells[0].speed, 80.);
    spells.step(&skills, 1.);
    assert_eq!(spells.shells[0].speed, 94.);
    assert_eq!(spells.shells[0].distance, 80.);
    assert!(!spells.shells[0].volumes(data).is_empty());
    spells.hit(&[1000], &skills, &mut owner);
    assert_eq!(spells.shells.len(), 1);
    assert!(spells.shells[0].impact);
    assert_eq!(spells.shells[0].owner, 1001);
    assert!(!spells.shells[0].volumes(data).is_empty());
    spells.step(&skills, 2.);
    assert!(spells.shells.is_empty());
    let data = skills.0[&212].spell.as_ref().unwrap();
    spells.fire(
        data,
        212,
        1,
        &skills.0[&212].release_pose([0.; 3], 0.),
        Some([0., 0., 500.]),
        &mut owner,
    );
    assert_eq!(spells.shells.len(), 3);
    assert_eq!(
        spells.shells.iter().map(|s| s.delay).collect::<Vec<_>>(),
        [0., 7., 14.]
    );
    assert!(spells.shells[0].volumes(data).is_empty());
    spells.step(&skills, 1.);
    assert!(!spells.shells[0].volumes(data).is_empty());
    assert!(spells.shells[1].volumes(data).is_empty());
    for _ in 0..20 {
        spells.step(&skills, 1.);
    }
    assert!(spells.shells.is_empty());
}

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.)
    }
}

#[test]
fn installed_scarlet_kisses_plays_native_phases_and_cost_edges_for_each_owner() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    assert!(
        native.exists(),
        "installed gate requires the player's installation"
    );
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let sword = ArisenData::fighter(&vfs).unwrap();
    let mage = ArisenData::mage(&vfs).unwrap();
    let strider = ArisenData::strider(&vfs).unwrap();
    let skills = SkillSet::load(&vfs, &file.image().unwrap(), &sword, &mage, &strider).unwrap();
    let skill = &skills.0[&152];
    let ddsim::skills::Kind::DaggerSeries {
        rate, maximum_cycles, ..
    } = skill.definition.kind
    else {
        panic!("Scarlet Kisses has no native series definition")
    };
    assert!(rate > 0.0 && maximum_cycles >= 1);
    for motion in [0x71e, 0x71f, 0x726, 0x727, 0x720] {
        assert!(skill.definition.motions[&motion].frame_count > 1);
    }
    assert!(skill.magic.is_none() && skill.spell.is_none());
    for owner in [0, 1] {
        let other = 1 - owner;
        let mut sim = ddsim::Sim::new(Default::default());
        sim.add_player(0, [0.0; 3], 0.0);
        sim.add_player(1, [1000.0, 0.0, 0.0], 0.0);
        skills.install(&mut sim);
        for seat in [0, 1] {
            let p = sim.player_mut(seat).unwrap();
            p.weapon = ddsim::Weapon::Dagger;
            p.skills.equipped[0] = Some(152);
        }
        let initial = sim.player(owner).unwrap().stamina;
        sim.request_skill(owner, 0);
        let mut sequence = Vec::new();
        let mut debits = 0.0;
        let mut previous_stamina = initial;
        for tick in 0..4000 {
            sim.step(1.0 / 120.0, &Default::default(), &Flat);
            let p = sim.player(owner).unwrap();
            if previous_stamina - p.stamina > skill.definition.cost * 0.5 {
                debits += previous_stamina - p.stamina;
            }
            previous_stamina = p.stamina;
            if let Some(active) = p.skills.active {
                if sequence.last() != Some(&active.motion) {
                    sequence.push(active.motion);
                }
            } else if tick > 0 {
                break;
            }
        }
        // B23F10: one press plays intro -> one strike -> end. It does not
        // silently repeat, fire a spell, or charge another stable owner.
        assert_eq!(sequence, [0x71e, 0x71f, 0x720]);
        assert!(sim.player(owner).unwrap().skills.active.is_none());
        assert!(sim.player(owner).unwrap().skills.released.is_empty());
        let mut falling_edges = 0;
        for motion in sequence {
            let root = &skill.definition.motions[&motion];
            let mut previous = 0;
            for frame in 0..root.frame_count {
                let current = root.event_bits(1, frame as i32, frame as i32);
                falling_edges += usize::from(ddskill::magic::weapon_cost_edge(previous, current));
                previous = current;
            }
        }
        assert!(falling_edges > 0, "native dagger bank has no cost edge");
        assert!((debits - falling_edges as f32 * skill.definition.cost).abs() < 0.1);
        assert_eq!(sim.player(other).unwrap().stamina, initial);
        assert!(sim.player(other).unwrap().skills.active.is_none());
    }
}

#[test]
fn installed_shield_summons_queues_pass_c_provoke_with_strict_sphere_and_no_damage() {
    use ddenemyrt::fight::{Fight, FightData};
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    assert!(
        native.exists(),
        "installed gate requires the player's installation"
    );
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let data = std::sync::Arc::new(FightData::load(&vfs, &file.image().unwrap()).unwrap());
    let ddsim::skills::Kind::ShieldProvoke { radius, amount, .. } = data.skills.0[&272].definition.kind
    else {
        panic!("missing Summons definition")
    };
    assert!(data.skills.0[&272].body.banks[9].is_some());
    assert!(data.skills.0[&272].definition.motions.contains_key(&0x92f));
    for owner in [0, 1] {
        let mut sim = ddsim::Sim::new(Default::default());
        let mut fight = Fight::new(data.clone());
        for seat in [0, 1] {
            sim.add_player(seat, [seat as f32 * 10000.0, 0.0, 0.0], 0.0);
            fight.add_arisen(seat);
            fight.equip(&mut sim, seat);
        }
        let origin = sim.player(owner).unwrap().position;
        for offset in [[0.0, 0.0, 100.0], [0.0, 0.0, radius], [0.0, radius + 1.0, 0.0]] {
            fight.add_goblin(std::array::from_fn(|i| origin[i] + offset[i]), 0.0);
        }
        let hp: Vec<_> = fight.goblins.iter().map(|g| g.fighter.victim.hp()).collect();
        let actor = sim.player_mut(owner).unwrap();
        // Authoritative equipped-category fixture, not a vocation grant.
        // Host profile projection is exercised independently in ddhost.
        actor.secondary_weapon = ddsim::Weapon::Shield;
        actor.skills.equipped[3] = Some(272);
        sim.request_skill(owner, 3);
        sim.step(1.0 / 120.0, &Default::default(), &Flat);
        fight.players_moved(&mut sim, &Flat, 0.25);
        assert_eq!(fight.goblins[0].distance_hate.score(u32::from(owner)), 0.0);
        fight.volumes(&sim); // Original msg delivery is pass C, after think.
        assert_eq!(
            fight.goblins[0].distance_hate.score(u32::from(owner)),
            amount as f32
        );
        assert_eq!(fight.goblins[1].distance_hate.score(u32::from(owner)), 0.0);
        assert_eq!(fight.goblins[2].distance_hate.score(u32::from(owner)), 0.0);
        assert!(fight.goblins.iter().all(|g| g.target.is_none()));
        assert_eq!(
            hp,
            fight
                .goblins
                .iter()
                .map(|g| g.fighter.victim.hp())
                .collect::<Vec<_>>()
        );
        assert!(fight.arisen[&owner].fighter.attacks.is_empty());
        fight.volumes(&sim); // One native entry message, not one per pose.
        assert_eq!(
            fight.goblins[0].distance_hate.score(u32::from(owner)),
            amount as f32
        );
        assert_eq!(fight.goblins[0].distance_hate.score(u32::from(1 - owner)), 0.0);
        for _ in 0..4000 {
            if sim.player(owner).unwrap().skills.active.is_none() {
                break;
            }
            sim.step(1.0 / 120.0, &Default::default(), &Flat);
            fight.players_moved(&mut sim, &Flat, 0.25);
            fight.volumes(&sim);
            assert!(
                fight.arisen[&owner].fighter.attacks.is_empty(),
                "Summons emits attack volumes"
            );
            assert_eq!(
                fight.goblins[0].distance_hate.score(u32::from(owner)),
                amount as f32
            );
        }
        assert!(sim.player(owner).unwrap().skills.active.is_none());
        assert_eq!(
            hp,
            fight
                .goblins
                .iter()
                .map(|g| g.fighter.victim.hp())
                .collect::<Vec<_>>()
        );

        // An actor lifetime that retires before pass-C delivery cannot
        // transfer its queued source message to a new profile on the seat.
        sim.request_skill(owner, 3);
        sim.step(1.0 / 120.0, &Default::default(), &Flat);
        fight.players_moved(&mut sim, &Flat, 0.25);
        let generation = fight.arisen[&owner].generation;
        fight.add_arisen(owner);
        assert_ne!(fight.arisen[&owner].generation, generation);
        fight.volumes(&sim);
        assert_eq!(fight.goblins[0].distance_hate.score(u32::from(owner)), 0.0);
    }
}

#[test]
fn installed_blink_uses_native_dash_loop_and_distance_for_each_owner() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    assert!(
        native.exists(),
        "installed gate requires the player's installation"
    );
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let sword = ArisenData::fighter(&vfs).unwrap();
    let mage = ArisenData::mage(&vfs).unwrap();
    let strider = ArisenData::strider(&vfs).unwrap();
    let skills = SkillSet::load(&vfs, &file.image().unwrap(), &sword, &mage, &strider).unwrap();
    let skill = &skills.0[&40];
    let ddsim::skills::Kind::Blink {
        loop_frame,
        maximum_distance,
        ..
    } = skill.definition.kind
    else {
        panic!("missing Blink definition")
    };
    assert_eq!(
        loop_frame,
        skill.body.banks[7].as_ref().unwrap().list.motions[0x29]
            .as_ref()
            .unwrap()
            .loop_frame as f32
    );
    for owner in [0, 1] {
        let mut sim = ddsim::Sim::new(Default::default());
        for seat in [0, 1] {
            sim.add_player(seat, [seat as f32 * 10000.0, 0.0, 0.0], 0.0);
            let p = sim.player_mut(seat).unwrap();
            p.weapon = ddsim::Weapon::Sword;
            p.skills.equipped[0] = Some(40);
        }
        skills.install(&mut sim);
        let origin = sim.player(owner).unwrap().position;
        let initial_stamina = sim.player(owner).unwrap().stamina;
        sim.request_skill(owner, 0);
        let mut phases = Vec::new();
        let mut travelled = 0.0;
        let mut last_position = origin;
        let mut debits = 0;
        let mut stamina = initial_stamina;
        for tick in 0..4000 {
            sim.step(1.0 / 120.0, &Default::default(), &Flat);
            let p = sim.player(owner).unwrap();
            if stamina - p.stamina > skill.definition.cost * 0.5 {
                debits += 1;
            }
            stamina = p.stamina;
            if let Some(active) = p.skills.active {
                if phases.last() != Some(&active.motion) {
                    phases.push(active.motion);
                }
                if active.phase == ddsim::skills::Phase::Strike {
                    travelled += (0..3)
                        .map(|i| (p.position[i] - last_position[i]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    assert!((active.distance - travelled).abs() < 0.01);
                } else {
                    assert!(travelled >= maximum_distance);
                    assert_eq!(active.distance, maximum_distance);
                }
            } else if tick > 0 {
                break;
            }
            last_position = p.position;
        }
        assert_eq!(phases, [0x729, 0x72a]);
        assert_eq!(debits, 1);
        assert!(sim.player(owner).unwrap().skills.active.is_none());
        assert_eq!(sim.player(1 - owner).unwrap().stamina, initial_stamina);
        assert_eq!(
            sim.player(1 - owner).unwrap().position,
            [(1 - owner) as f32 * 10000.0, 0.0, 0.0]
        );
    }
}

#[test]
fn installed_blink_goblin_contact_ends_dash_but_world_wall_does_not_acknowledge() {
    use ddenemyrt::fight::{Fight, FightData};
    use ddsim::skills::Phase;
    struct Wall;
    impl ddsim::Ground for Wall {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
        fn wall(&self, _: [f32; 3], _: f32, _: f32, _: f32) -> Option<[f32; 2]> {
            Some([0.0, -1.0])
        }
    }
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    assert!(
        native.exists(),
        "installed gate requires the player's installation"
    );
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let data = std::sync::Arc::new(FightData::load(&vfs, &file.image().unwrap()).unwrap());
    for owner in [0, 1] {
        let mut fight = Fight::new(data.clone());
        let mut sim = ddsim::Sim::new(Default::default());
        for seat in [0, 1] {
            sim.add_player(seat, [seat as f32 * 10000.0, 0.0, 0.0], 0.0);
            fight.add_arisen(seat);
            fight.equip(&mut sim, seat);
        }
        let origin = sim.player(owner).unwrap().position;
        fight.add_goblin([origin[0], 0.0, 100.0], std::f32::consts::PI);
        sim.player_mut(owner).unwrap().skills.equipped[0] = Some(40);
        sim.request_skill(owner, 0);
        let mut acknowledged = false;
        for _ in 0..1000 {
            fight.hits(0.25);
            fight.apply_skill_contacts(&mut sim);
            if sim.player(owner).unwrap().skills.contact.ends_blink() {
                let active = sim.player(owner).unwrap().skills.active.unwrap();
                assert_eq!(active.phase, Phase::Strike);
                let ddsim::skills::Kind::Blink { maximum_distance, .. } = data.skills.0[&40].definition.kind
                else {
                    unreachable!()
                };
                assert!(
                    active.distance < maximum_distance,
                    "contact proof reached distance endpoint first"
                );
                sim.step(1.0 / 120.0, &Default::default(), &Flat);
                let active = sim.player(owner).unwrap().skills.active.unwrap();
                assert_eq!((active.phase, active.motion), (Phase::End, 0x72a));
                acknowledged = true;
                break;
            }
            sim.step(1.0 / 120.0, &Default::default(), &Flat);
            fight.players_moved(&mut sim, &Flat, 0.25);
            fight.volumes(&sim);
        }
        assert!(
            acknowledged,
            "installed checking attack never acknowledged goblin contact"
        );
        assert!(!sim.player(1 - owner).unwrap().skills.contact.valid);

        // Source retirement between callback and consumption drops the old
        // lifetime's ack. It cannot finish a replacement profile's skill.
        fight.hits(0.25); // Raw checking ack repeats even with hit history held.
        fight.add_arisen(owner);
        fight.apply_skill_contacts(&mut sim);
        assert!(!sim.player(owner).unwrap().skills.contact.valid);

        let mut fight = Fight::new(data.clone());
        let mut sim = ddsim::Sim::new(Default::default());
        sim.add_player(owner, [0.0; 3], 0.0);
        fight.add_arisen(owner);
        fight.equip(&mut sim, owner);
        sim.player_mut(owner).unwrap().skills.equipped[0] = Some(40);
        sim.request_skill(owner, 0);
        for _ in 0..500 {
            fight.hits(0.25);
            fight.apply_skill_contacts(&mut sim);
            assert!(!sim.player(owner).unwrap().skills.contact.valid);
            sim.step(1.0 / 120.0, &Default::default(), &Wall);
            fight.players_moved(&mut sim, &Wall, 0.25);
            fight.volumes(&sim);
        }
        let active = sim.player(owner).unwrap().skills.active.unwrap();
        assert_eq!((active.phase, active.motion), (Phase::Strike, 0x729));
    }
}
