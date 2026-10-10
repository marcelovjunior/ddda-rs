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
