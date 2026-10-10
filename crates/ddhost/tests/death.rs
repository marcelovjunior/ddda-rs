use ddhost::death::{
    DeadClock, FatalCompletion, FatalRequest, Parameters, PawnDownClock, PawnDownGuards, ReviveContact,
    SessionDeaths, fatal_completion, pawn_recovery_hp,
};
use ddhost::{Control, ProfileId, Roster};

#[test]
fn fatal_request_preserves_native_ground_air_and_special_guards() {
    let ground = FatalRequest {
        dead: true,
        movement_flags: 1,
        kind_flags: 2,
        ..Default::default()
    };
    assert_eq!(ground.action(), Some(0x1e));
    for blocked in [
        FatalRequest {
            dead: false,
            ..ground
        },
        FatalRequest {
            linked_actor: true,
            ..ground
        },
        FatalRequest {
            action_locks: 0x2000_0000,
            ..ground
        },
        FatalRequest {
            action_locks: 0x0100_0000,
            ..ground
        },
        FatalRequest {
            movement_flags: 0x21,
            ..ground
        },
        FatalRequest {
            damage_gate: true,
            ..ground
        },
        FatalRequest {
            petrified: true,
            ..ground
        },
    ] {
        assert_eq!(blocked.action(), None);
    }
    assert_eq!(
        FatalRequest {
            stone_dead: true,
            damage_gate: true,
            ..ground
        }
        .action(),
        Some(0x10)
    );
    assert_eq!(
        FatalRequest {
            action_locks: 0x8000,
            petrified: true,
            ..ground
        }
        .action(),
        Some(0xec)
    );
    assert_eq!(
        FatalRequest {
            kind_flags: 8,
            ..ground
        }
        .action(),
        Some(0x0300_0040)
    );
    assert_eq!(
        FatalRequest {
            kind_flags: 8,
            subtype_flags: 0x10,
            ..ground
        }
        .action(),
        Some(0x1e)
    );
    let air = FatalRequest {
        movement_flags: 0,
        ..ground
    };
    assert_eq!(air.action(), Some(0x16));
    assert_eq!(
        FatalRequest {
            airborne_gate: true,
            ..air
        }
        .action(),
        None
    );
    assert_eq!(FatalRequest { stage: 0x259, ..air }.action(), Some(0xd6));
    assert_eq!(
        FatalRequest {
            kind_flags: 4,
            stage: 0x259,
            ..air
        }
        .action(),
        Some(0x16)
    );
}

#[test]
fn fatal_animation_completion_separates_arisen_and_pawn() {
    assert_eq!(fatal_completion(false, 0, false, 0), None);
    assert_eq!(
        fatal_completion(false, 1, false, 2),
        Some(FatalCompletion::ArisenDead)
    );
    assert_eq!(
        fatal_completion(true, 0, true, 0),
        Some(FatalCompletion::PawnNearDeath)
    );
    assert_eq!(
        fatal_completion(true, 0, true, 2),
        Some(FatalCompletion::PawnLost)
    );
}

#[test]
fn death_delay_is_arisen_only_one_shot_and_inclusive() {
    let mut clock = DeadClock::default();
    assert!(!clock.step(30.0, 4.0, false).unwrap());
    assert_eq!(clock.elapsed(), 0.0);
    assert!(!clock.step(3.75, 4.0, true).unwrap());
    assert!(clock.step(0.25, 4.0, true).unwrap());
    assert!(!clock.step(100.0, 4.0, true).unwrap());
    assert_eq!(clock.elapsed(), 4.0);
    assert!(clock.step(f32::NAN, 4.0, true).is_err());
}

#[test]
fn pawn_down_expiration_obeys_native_pauses_and_reset_order() {
    let mut clock = PawnDownClock::default();
    assert!(!clock.step(3.0, 4.0, PawnDownGuards::default()).unwrap());
    for guards in [
        PawnDownGuards {
            game_flags: 0x0100_0000,
            ..Default::default()
        },
        PawnDownGuards {
            transient_flags: 2,
            ..Default::default()
        },
        PawnDownGuards {
            transient_flags: 0x0200_0000,
            ..Default::default()
        },
        PawnDownGuards {
            upper_flags: 0x0080_0000,
            ..Default::default()
        },
        PawnDownGuards {
            active_pause_count: 1,
            ..Default::default()
        },
        PawnDownGuards {
            arisen_reviving: true,
            ..Default::default()
        },
        PawnDownGuards {
            scene_gate: true,
            ..Default::default()
        },
        PawnDownGuards {
            return_gate: true,
            ..Default::default()
        },
        PawnDownGuards {
            event_gate: true,
            ..Default::default()
        },
        PawnDownGuards {
            scenario_gate: true,
            ..Default::default()
        },
        PawnDownGuards {
            weather_gate: true,
            ..Default::default()
        },
    ] {
        assert!(!clock.step(20.0, 4.0, guards).unwrap());
        assert_eq!(clock.elapsed(), 3.0);
    }
    let reset = PawnDownGuards {
        reset_expiration: true,
        ..Default::default()
    };
    assert!(!clock.step(1.0, 4.0, reset).unwrap());
    assert_eq!(clock.elapsed(), 0.0);
    assert!(clock.step(4.0, 4.0, PawnDownGuards::default()).unwrap());
    let fatal = PawnDownGuards {
        irreversible_death: true,
        ..reset
    };
    assert!(clock.step(0.0, 4.0, fatal).unwrap());
}

#[test]
fn revive_contact_requires_pawn_real_type_and_native_guards() {
    let valid = ReviveContact {
        pawn: true,
        queued: true,
        kind: 0x18,
        ..Default::default()
    };
    assert!(valid.accepts());
    for invalid in [
        ReviveContact { pawn: false, ..valid },
        ReviveContact {
            queued: false,
            ..valid
        },
        ReviveContact { kind: 0xc, ..valid },
        ReviveContact {
            linked_actor: true,
            ..valid
        },
        ReviveContact {
            irreversible_death: true,
            ..valid
        },
        ReviveContact {
            blocked_revival: true,
            ..valid
        },
    ] {
        assert!(!invalid.accepts());
    }
    assert_eq!(pawn_recovery_hp(750.0, 0.25, 0.0).unwrap(), 187.5);
    assert_eq!(pawn_recovery_hp(750.0, 0.25, 0.5).unwrap(), 375.0);
    assert_eq!(pawn_recovery_hp(750.0, 0.25, 1.25).unwrap(), 750.0);
    assert!(pawn_recovery_hp(750.0, f32::NAN, 0.0).is_err());
}

#[test]
fn coop_death_uses_profiles_and_disconnected_players_do_not_veto() {
    let mut roster = Roster::default();
    let a = roster.join(ProfileId(10), Control::Local).unwrap();
    let b = roster.join(ProfileId(20), Control::Remote).unwrap();
    let mut deaths = SessionDeaths::default();
    deaths.enter_dead(ProfileId(10));
    deaths.step(&roster, 2.0, 2.0).unwrap();
    assert_eq!(deaths.spectator_target(&roster, a), Some(b));
    assert!(!deaths.game_over(&roster));
    roster.leave(b);
    assert!(deaths.game_over(&roster));
    let c = roster.join(ProfileId(30), Control::Remote).unwrap();
    assert_eq!(b, c);
    assert!(!deaths.is_dead(ProfileId(30)));
    assert!(!deaths.game_over(&roster));
    deaths.enter_dead(ProfileId(30));
    deaths.step(&roster, 2.0, 2.0).unwrap();
    assert!(deaths.game_over(&roster));
    assert_eq!(deaths.spectator_target(&roster, a), None);
    deaths.restored();
    assert!(!deaths.game_over(&roster));
    roster.leave(a);
    roster.leave(c);
    assert!(!deaths.game_over(&roster));
}

#[test]
fn collapse_spectates_without_starting_or_resetting_native_dead_delay() {
    let mut roster = Roster::default();
    let a = roster.join(ProfileId(10), Control::Local).unwrap();
    let b = roster.join(ProfileId(20), Control::Remote).unwrap();
    let mut deaths = SessionDeaths::default();
    deaths.enter_fatal(ProfileId(10));
    assert!(deaths.is_dead(ProfileId(10)));
    assert_eq!(deaths.spectator_target(&roster, a), Some(b));
    roster.leave(b);
    deaths.step(&roster, 100.0, 4.0).unwrap();
    assert!(!deaths.game_over(&roster), "collapse time is not cPlActDead time");
    deaths.enter_dead(ProfileId(10));
    deaths.step(&roster, 3.0, 4.0).unwrap();
    deaths.enter_dead(ProfileId(10));
    deaths.enter_fatal(ProfileId(10));
    deaths.step(&roster, 1.0, 4.0).unwrap();
    assert!(
        deaths.game_over(&roster),
        "repeated observations must not reset the delay"
    );
}

#[test]
fn death_parameters_reject_short_and_nonfinite_installed_tables() {
    let mut table = vec![0.0; 0x137];
    table[0x136] = 4.0;
    table[1] = 12.0;
    table[0xfb] = 0.4;
    let params = Parameters::from_adjust(&table).unwrap();
    assert_eq!(params.game_over_delay, 4.0);
    assert_eq!(params.pawn_down_limit, 12.0);
    assert_eq!(params.pawn_recovery_ratio, 0.4);
    assert!(Parameters::from_adjust(&table[..0x136]).is_err());
    for at in [1, 0xfb, 0x136] {
        let mut malformed = table.clone();
        malformed[at] = f32::NAN;
        assert!(Parameters::from_adjust(&malformed).is_err());
    }
}

#[test]
fn installed_death_parameters_and_action_identity() {
    let native = std::path::PathBuf::from(
        std::env::var_os("DDDA_NATIVE_PC")
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into()),
    );
    if !native.exists() {
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let params = Parameters::load(&vfs).unwrap();
    // A zero installed delay is valid: the preceding fatal motion supplies
    // the collapse time. Exercise the actual runtime threshold inclusively.
    let mut clock = DeadClock::default();
    assert!(
        clock
            .step(params.game_over_delay, params.game_over_delay, true)
            .unwrap()
    );
    assert!(!clock.step(1.0, params.game_over_delay, true).unwrap());
    assert!(params.pawn_down_limit > 0.0);
    assert!(params.pawn_recovery_ratio > 0.0);
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let names = exe.class_names();
    // 0x00B608B5 registers 0x103 common records at 0x01866C48,
    // stride0x44. Read bulk DTI pointers from the player's own exe.
    for (id, expected) in [
        (0x1e, "cPlActDmgCrumbleDead"),
        (0x26, "cPlActDead"),
        (0x33, "cPlReviveCMC"),
        (0x80, "cPlActCmcNeardeath"),
        (0x81, "cPlActCmcReturn"),
        (0x82, "cPlActCmcDead"),
    ] {
        let dti = exe.u32(0x0186_6c48 + id * 0x44).unwrap();
        assert_eq!(names.get(&dti).map(String::as_str), Some(expected));
    }
}

#[test]
fn retry_rejects_late_chunk_corruption_without_partial_host_replacement() {
    use ddhost::data::NoStoryData;
    use ddhost::save::{Owner, TAG_NPC_QUEST, TAG_PLACE, TAG_RIFT, place_bytes};
    use ddhost::{FrameInputs, Host};
    use ddstory::{Story, scripted::ScriptedHost};
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(Story::new_game(NoStoryData)),
        ScriptedHost::default(),
        Box::new(Flat),
    );
    let a = host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    let b = host
        .join(ProfileId(2), Control::Remote, [100.0, 0.0, 0.0], 0.0)
        .unwrap();
    host.economy.wallet(a).gold = 10;
    host.economy.wallet(b).gold = 20;
    let checkpoint = host.save().unwrap();
    host.tick(&FrameInputs::new()).unwrap();
    host.sim.player_mut(a).unwrap().position = [600.0, 0.0, 900.0];
    host.sim.player_mut(b).unwrap().position = [700.0, 0.0, 900.0];
    host.economy.wallet(a).gold = 30;
    host.economy.wallet(b).gold = 40;
    let before = host.save().unwrap().to_bytes();
    for (tag, owner, bytes) in [
        (TAG_RIFT, Owner::Shared, vec![255]),
        (TAG_NPC_QUEST, Owner::Shared, vec![2]),
        (TAG_NPC_QUEST, Owner::Shared, vec![1, 1, 0, 0, 0]),
        (TAG_PLACE, Owner::Profile(ProfileId(1)), vec![0]),
        (
            TAG_PLACE,
            Owner::Profile(ProfileId(2)),
            place_bytes([f32::NAN, 0.0, 0.0], 0.0),
        ),
    ] {
        let mut corrupt = checkpoint.clone();
        corrupt.put(tag, owner, bytes);
        assert!(host.load(&corrupt, NoStoryData).is_err());
        assert_eq!(
            host.save().unwrap().to_bytes(),
            before,
            "failed retry replaced {tag:?}"
        );
    }
    host.load(&checkpoint, NoStoryData).unwrap();
    assert_eq!(host.save().unwrap().to_bytes(), checkpoint.to_bytes());
}

struct DeathTestGround;
impl ddsim::Ground for DeathTestGround {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
}

fn death_motion_fixture() -> ddsim::Sim {
    // Deliberately short synthetic layers test exhaustion, not game durations.
    let motions = [(0x109, 4), (0x128, 3), (0x129, 5), (0x12a, 3)]
        .into_iter().map(|(id, frames)| (id, ddsim::RootMotion::none(frames))).collect();
    ddsim::Sim::new(motions)
}

#[test]
fn grounded_collapse_exhausts_actual_layer_before_dead_and_survivor_moves() {
    use ddsim::{DeathPhase, Input, State};
    let mut sim = death_motion_fixture();
    sim.add_player(0, [0.0; 3], 0.0);
    sim.add_player(1, [100.0, 0.0, 0.0], 0.0);
    let inputs = [(0, Input { stick: [0.0, 1.0], jump: true, light: true,
        bow: true, ..Default::default() }), (1, Input { stick: [0.0, 1.0],
        ..Default::default() })].into_iter().collect();
    sim.begin_grounded_death(0, false, true).unwrap();
    sim.step(2.0 / ddsim::MOTION_FPS, &inputs, &DeathTestGround);
    assert_eq!(sim.player(0).unwrap().state, State::Death { phase: DeathPhase::Collapse { pawn: false } });
    assert_eq!(sim.player(0).unwrap().position, [0.0; 3]);
    assert!(sim.player(1).unwrap().position[2] > 0.0);
    sim.step(1.0 / ddsim::MOTION_FPS, &inputs, &DeathTestGround);
    assert_eq!(sim.player(0).unwrap().state, State::Death { phase: DeathPhase::ArisenDead });
    assert_eq!(sim.player(0).unwrap().frame, 3.0);
    sim.step(2.0 / ddsim::MOTION_FPS, &inputs, &DeathTestGround);
    assert_eq!(sim.player(0).unwrap().frame, 3.0, "Dead retains completed layer");
    assert_eq!(sim.player(0).unwrap().attacks_started, 0);
}

#[test]
fn pawn_down_phases_require_motion_exhaustion_and_return_is_pawn_only() {
    use ddsim::{DeathPhase, State};
    let mut sim = death_motion_fixture();
    sim.add_player(0, [0.0; 3], 0.0);
    sim.add_player(1, [100.0, 0.0, 0.0], 0.0);
    sim.begin_grounded_death(0, true, true).unwrap();
    sim.begin_grounded_death(1, false, true).unwrap();
    sim.step(3.0 / ddsim::MOTION_FPS, &Default::default(), &DeathTestGround);
    assert_eq!(sim.player(0).unwrap().state, State::Death { phase: DeathPhase::PawnDownStart });
    sim.step(1.0 / ddsim::MOTION_FPS, &Default::default(), &DeathTestGround);
    assert_eq!(sim.player(0).unwrap().state, State::Death { phase: DeathPhase::PawnDownStart });
    sim.step(1.0 / ddsim::MOTION_FPS, &Default::default(), &DeathTestGround);
    assert_eq!(sim.player(0).unwrap().state, State::Death { phase: DeathPhase::PawnDown });
    assert!(sim.begin_pawn_return(1).is_err());
    sim.begin_pawn_return(0).unwrap();
    assert_eq!(sim.player(0).unwrap().motion_id(), 0x12a);
    sim.step(1.0 / ddsim::MOTION_FPS, &Default::default(), &DeathTestGround);
    assert!(sim.player(0).unwrap().incapacitated(), "HP recovery does not bypass get-up");
}

#[test]
fn fatal_replacement_clears_queued_actions_and_rejects_untranslated_air() {
    let mut sim = death_motion_fixture();
    sim.add_player(0, [0.0; 3], 0.0);
    let player = sim.player_mut(0).unwrap();
    player.skills.request = Some(0);
    player.skills.released.push(123);
    player.bow_latched = true;
    assert!(sim.begin_grounded_death(0, false, false).unwrap_err().contains("airborne"));
    assert_eq!(sim.player(0).unwrap().state, ddsim::State::Wait);
    sim.begin_grounded_death(0, false, true).unwrap();
    let player = sim.player(0).unwrap();
    assert_eq!(player.skills.request, None);
    assert!(player.skills.released.is_empty());
    assert!(!player.bow_latched);
    assert_eq!(player.push_rank(), 13);
    let mut missing = ddsim::Sim::new(Default::default());
    missing.add_player(0, [0.0; 3], 0.0);
    assert!(missing.begin_grounded_death(0, false, true).unwrap_err().contains("missing installed"));
    assert_eq!(missing.player(0).unwrap().state, ddsim::State::Wait);
}

#[test]
fn installed_damage_bank_poses_each_kit_and_zero_hp_cannot_emit_queued_attacks() {
    use ddenemyrt::fight::{Fight, FightData, Kit};
    let native = std::path::PathBuf::from(std::env::var_os("DDDA_NATIVE_PC")
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into()));
    if !native.exists() { return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    let data = std::sync::Arc::new(FightData::load(&vfs, &exe).unwrap());
    let mut fight = Fight::new(data);
    let mut sim = ddsim::Sim::new(ddhost::install::root_motions(&vfs).unwrap());
    for (seat, kit) in [(0, Kit::Fighter), (1, Kit::Mage), (2, Kit::Strider)] {
        sim.add_player(seat, [seat as f32 * 100.0, 0.0, 0.0], 0.0);
        fight.add_arisen_as(seat, kit);
        fight.equip(&mut sim, seat);
        let body = &fight.kit(kit).body;
        for motion in [0x109, 0x128, 0x129, 0x12a] {
            let id = ddenemyrt::body::MotionId::new(1, motion & 255);
            let actual = body.motion(id).expect("installed damage layer");
            assert!(actual.frame_count > 1);
            assert_eq!(sim.motion_for(sim.player(seat).unwrap().weapon, motion).unwrap().frame_count,
                actual.frame_count);
        }
        if let ddenemyrt::combat::Victim::Player { hp, .. } =
            &mut fight.arisen.get_mut(&seat).unwrap().fighter.victim { *hp = 0.0; }
        let p = sim.player_mut(seat).unwrap();
        p.volleys_fired = 1;
        p.attacks_started = 1;
        p.skills.released.push(123);
        sim.begin_grounded_death(seat, false, true).unwrap();
    }
    fight.players_moved(&mut sim, &DeathTestGround, 1.0);
    fight.volumes(&sim);
    assert!(fight.shells.is_empty());
    assert!(fight.arrows.is_empty());
    assert!(fight.spells.shells.is_empty());
    for seat in 0..3 {
        assert!(fight.arisen[&seat].fighter.attacks.is_empty());
        assert_eq!(fight.arisen[&seat].volleys_seen, 1);
        assert_eq!(fight.arisen[&seat].attacks_seen, 1);
        assert!(sim.player(seat).unwrap().skills.released.is_empty());
        assert!(!fight.bow_errors.contains_key(&seat));
    }
}

type DeathHost = ddhost::Host<ddhost::data::NoStoryData, ddstory::scripted::ScriptedHost>;

fn installed_death_host() -> Option<DeathHost> {
    use ddenemyrt::fight::{Fight, FightData};
    let native = std::path::PathBuf::from(std::env::var_os("DDDA_NATIVE_PC")
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into()));
    if !native.exists() { return None; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let data = std::sync::Arc::new(FightData::load(&vfs, &file.image().unwrap()).unwrap());
    let mut host = DeathHost::new(ddsim::Sim::new(ddhost::install::root_motions(&vfs).unwrap()),
        None, ddstory::scripted::ScriptedHost::default(), Box::new(DeathTestGround));
    host.join(ProfileId(100), Control::Local, [0.0; 3], 0.0).unwrap();
    host.join(ProfileId(200), Control::Remote, [400.0, 0.0, 0.0], 0.0).unwrap();
    let mut fight = Fight::new(data);
    for seat in host.roster.ids() {
        fight.add_arisen(seat);
        fight.equip(&mut host.sim, seat);
    }
    host.fight = Some(fight);
    Some(host)
}

fn kill_arisen(host: &mut DeathHost, seat: ddhost::PlayerId) {
    let actor = &mut host.fight.as_mut().unwrap().arisen.get_mut(&seat).unwrap().fighter;
    let ddenemyrt::combat::Victim::Player { hp, .. } = &mut actor.victim else { panic!("Arisen HP"); };
    *hp = 0.0;
}

fn complete_installed_death(host: &mut DeathHost, seat: ddhost::PlayerId) -> ddhost::FrameReport {
    let body = host.sim.player(seat).unwrap();
    let period = host.sim.motion_for(body.weapon, 0x109).unwrap().period();
    let delay = Parameters::from_adjust(&host.fight.as_ref().unwrap().data.player_adjust)
        .unwrap().game_over_delay;
    // Bound comes from the installed layer plus actual native delay, not a
    // guessed fatal duration. Extra frames allow discrete entry/exit updates.
    let mut report = Default::default();
    for _ in 0..(period.ceil() + delay.ceil()) as usize + 3 {
        report = host.tick(&ddhost::FrameInputs::new()).unwrap();
    }
    assert!(matches!(host.sim.player(seat).unwrap().state, ddsim::State::Death {
        phase: ddsim::DeathPhase::ArisenDead }));
    report
}

#[test]
fn installed_host_fatality_gates_only_dead_profile_and_waits_for_dead_entry() {
    use ddhost::{FrameInputs, PlayerInput};
    let Some(mut host) = installed_death_host() else { return };
    let delay = Parameters::from_adjust(&host.fight.as_ref().unwrap().data.player_adjust)
        .unwrap().game_over_delay;
    assert_eq!(delay, Parameters::load(&ddvfs::Vfs::open(std::path::PathBuf::from(
        std::env::var_os("DDDA_NATIVE_PC").unwrap_or_else(||
            "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into()))).unwrap()).unwrap().game_over_delay);
    kill_arisen(&mut host, 0);
    let inputs: FrameInputs = [(0, PlayerInput { sim: ddsim::Input {
        stick: [0.0, 1.0], light: true, jump: true, ..Default::default() }, interact: true }),
        (1, PlayerInput { sim: ddsim::Input { stick: [0.0, 1.0], ..Default::default() }, interact: true })]
        .into_iter().collect();
    let report = host.tick(&inputs).unwrap();
    assert!(report.death.seats[0].terminal);
    assert_eq!(report.death.seats[0].spectator, Some(1));
    assert!(!report.death.seats[1].terminal);
    assert!(!report.death.game_over);
    assert!(host.player_actions_blocked(0));
    assert!(!host.player_actions_blocked(1));
    assert!(!ddhost::StoryWorld::player_actions_blocked(&host.world, 0),
        "personal death must not set the shared authored-camera lock");
    assert!(matches!(host.sim.player(0).unwrap().state, ddsim::State::Death {
        phase: ddsim::DeathPhase::Collapse { pawn: false } }));
    assert_eq!(host.sim.player(0).unwrap().position, [0.0; 3]);
    assert!(host.sim.player(1).unwrap().position[2] > 0.0);
    assert_eq!(host.tick(&inputs).unwrap().interact, Some(1), "dead seat's prompt must be gated");
    // Removing the survivor during collapse must not begin the Dead clock.
    host.leave(1);
    let report = host.tick(&FrameInputs::new()).unwrap();
    assert!(!report.death.game_over);
    let report = complete_installed_death(&mut host, 0);
    assert!(report.death.game_over, "zero or positive installed delay reached only after Dead entry");
}

#[test]
fn installed_host_death_reconnect_and_seat_reuse_preserve_profile_attempt() {
    let Some(mut host) = installed_death_host() else { return };
    kill_arisen(&mut host, 0);
    let report = complete_installed_death(&mut host, 0);
    assert!(!report.death.game_over, "active living profile vetoes shared game over");
    let dead_frame = host.sim.player(0).unwrap().frame;
    host.leave(0);
    let newcomer = host.join(ProfileId(300), Control::Local, [800.0, 0.0, 0.0], 0.0).unwrap();
    assert_eq!(newcomer, 0);
    assert!(!host.player_actions_blocked(newcomer));
    assert!(!host.sim.player(newcomer).unwrap().incapacitated());
    assert!(!host.tick(&ddhost::FrameInputs::new()).unwrap().death.game_over);
    host.leave(newcomer);
    let reattached = host.join(ProfileId(100), Control::Local, [0.0; 3], 0.0).unwrap();
    assert_eq!(reattached, 0);
    assert!(host.player_actions_blocked(reattached));
    assert!(matches!(host.sim.player(reattached).unwrap().state, ddsim::State::Death {
        phase: ddsim::DeathPhase::ArisenDead }));
    assert_eq!(host.sim.player(reattached).unwrap().frame, dead_frame);
    host.leave(1);
    assert!(host.tick(&ddhost::FrameInputs::new()).unwrap().death.game_over,
        "disconnected living profiles do not veto");
}

#[test]
fn installed_host_airborne_fatality_is_reported_without_grounded_fallback() {
    let Some(mut host) = installed_death_host() else { return };
    host.sim.player_mut(0).unwrap().state = ddsim::State::Fall;
    host.sim.player_mut(0).unwrap().position[1] = 100.0;
    kill_arisen(&mut host, 0);
    let report = host.tick(&ddhost::FrameInputs::new()).unwrap();
    assert!(report.death.seats[0].terminal);
    assert!(report.death.seats[0].unsupported.as_deref().unwrap().contains("airborne"));
    assert!(!host.sim.player(0).unwrap().incapacitated());
    assert!(!host.player_actions_blocked(1));
    // Landing in the port cannot silently change the original fatal branch.
    host.sim.player_mut(0).unwrap().state = ddsim::State::Wait;
    host.sim.player_mut(0).unwrap().position[1] = 0.0;
    let report = host.tick(&ddhost::FrameInputs::new()).unwrap();
    assert!(!host.sim.player(0).unwrap().incapacitated());
    assert!(report.death.seats[0].unsupported.is_some());
    assert!(!report.death.game_over);
}

fn poison_attempt_combat(host: &mut DeathHost) {
    use ddenemyrt::{arrow::Arrow, shell::Shell, skills::spells::Spell};
    let fight = host.fight.as_mut().unwrap();
    let enemy = fight.add_goblin([800.0, 0.0, 0.0], 0.0);
    fight.add_pawn(&mut host.sim, 4, 0, [600.0, 0.0, 0.0], 0.0, [500.0; 9], 7);
    assert!(fight.deliver_pawn_order(4, ddenemyrt::pawn::Command::Go));
    fight.shells.push(Shell::fire(&fight.data.shell, 1000, 0, [0.0; 3], 0.0, Some(enemy)));
    fight.arrows.push(Arrow::fire(&fight.data.arrow.param, 1001, 1,
        [0.0; 3], [0.0, 0.0, 1.0], 1.0).unwrap());
    // Synthetic spell identity/age exercises lifetime removal only, never
    // spell selection, native timing, damage or projectile movement.
    fight.spells.shells.push(Spell { owner: 1002, shooter: 1, skill: 0,
        pos: [0.0; 3], direction: [0.0, 0.0, 1.0], age: 1.0, delay: 0.0,
        distance: 0.0, speed: 0.0, gravity: 0.0, impact: false });
    fight.shooters.insert(1000, 0);
    fight.shooters.insert(1001, 1);
    fight.shooters.insert(1002, 1);
    fight.small_gauge = Some((enemy, 1.0));
}

#[test]
fn installed_retry_replaces_dead_seats_projectiles_enemy_and_pawn_brains() {
    let Some(mut host) = installed_death_host() else { return };
    host.frame = 7;
    let checkpoint = host.save().unwrap();
    kill_arisen(&mut host, 0);
    kill_arisen(&mut host, 1);
    assert!(complete_installed_death(&mut host, 0).death.game_over);
    poison_attempt_combat(&mut host);
    assert_eq!(host.fight.as_ref().unwrap().pawns[&4].brain.orders.mailbox,
        ddenemyrt::pawn::Command::Go);
    for seat in host.roster.ids() {
        let body = host.sim.player_mut(seat).unwrap();
        body.skills.request = Some(123);
        body.skills.released.push(123);
        body.bow_latched = true;
        body.velocity = [1.0, 2.0, 3.0];
    }
    let template = host.sim.fresh_attempt();
    let fight_data = host.fight.as_ref().unwrap().data.clone();
    let result = host.prepare_retry(&checkpoint, ddhost::data::NoStoryData, |seed| {
        let mut candidate = DeathHost::new(seed.sim(&template, &Default::default())?,
            None, Default::default(), Box::new(DeathTestGround));
        candidate.roster = seed.roster.clone();
        candidate.fight = Some(ddenemyrt::fight::Fight::new(fight_data));
        // Poison the independent factory result too: the dedicated retry
        // must rebuild combat rather than trust its empty constructor.
        poison_attempt_combat(&mut candidate);
        Ok(candidate)
    }).unwrap().commit_retry();
    assert_eq!((result.frame, result.epoch), (7, 1));
    assert!(!host.death.report(&host.roster).game_over);
    let fight = host.fight.as_ref().unwrap();
    assert!(fight.goblins.is_empty());
    assert!(fight.pawns.is_empty());
    assert!(fight.npcs.is_empty());
    assert!(fight.shells.is_empty());
    assert!(fight.arrows.is_empty());
    assert!(fight.spells.shells.is_empty());
    assert!(fight.shooters.is_empty());
    assert!(fight.positions.is_empty());
    assert!(fight.small_gauge.is_none());
    assert_eq!(host.sim.players.len(), host.roster.len());
    for seat in host.roster.ids() {
        assert!(!host.player_actions_blocked(seat));
        assert!(!fight.arisen[&seat].fighter.victim.dead());
        assert!(fight.arisen[&seat].fighter.attacks.is_empty());
        let body = host.sim.player(seat).unwrap();
        assert_eq!(body.state, ddsim::State::Wait);
        assert_eq!(body.frame, 0.0);
        assert_eq!(body.velocity, [0.0; 3]);
        assert!(body.skills.request.is_none());
        assert!(body.skills.released.is_empty());
        assert!(!body.bow_latched);
    }
    let inputs = host.roster.ids().map(|seat| (seat, ddhost::PlayerInput {
        sim: ddsim::Input { stick: [0.0, 1.0], ..Default::default() },
        ..Default::default() })).collect();
    let before: Vec<_> = host.roster.ids().map(|seat|
        (seat, host.sim.player(seat).unwrap().position[2])).collect();
    host.tick(&inputs).unwrap();
    for (seat, z) in before {
        assert!(host.sim.player(seat).unwrap().position[2] > z);
    }
}
