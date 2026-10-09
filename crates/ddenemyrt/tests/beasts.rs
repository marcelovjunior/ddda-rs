//! Installed family data: skip explicitly when the game is unavailable.
use ddai::thinktbl::families::Family;
use ddenemyrt::beast::BeastData;
use ddenemyrt::body::MotionId;

#[test]
fn family_bodies_have_their_own_attack_and_hurt_volumes() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: no install");
        return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let path = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| ddexe::locate(&native).unwrap());
    let exe = ddexe::ExeFile::open(path).unwrap();
    let img = exe.image().unwrap();
    for (family, slot) in [(Family::Wolf, 1), (Family::Harpy, 2)] {
        let data = BeastData::load(&vfs, &img, family).unwrap();
        let motion = MotionId::new(1, slot);
        assert!(data.body.frames(motion) > 0, "{family:?} attack motion");
        let mut attack_frames = 0;
        let mut hurt_frames = 0;
        for frame in 0..data.body.frames(motion) {
            let pose = data.body.pose(motion, frame as f32, [0.0; 3], 0.0);
            let acts = data.body.activations(motion, frame as f32);
            let (attacks, hurts) = data.body.volumes(&acts, &pose, 100);
            attack_frames += usize::from(!attacks.is_empty());
            hurt_frames += usize::from(!hurts.is_empty());
        }
        assert!(attack_frames > 0, "{family:?} no activated attacks");
        assert!(hurt_frames > 0, "{family:?} no activated hurt volumes");
    }
}

#[test]
fn selected_family_actions_advance_installed_motions_and_preserve_owner() {
    use ddenemyrt::beast::Beast;
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
            Some(0.0)
        }
    }
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.is_dir() {
        eprintln!("SKIP: no install");
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let exe = ddexe::ExeFile::open(ddexe::locate(native).unwrap()).unwrap();
    let image = exe.image().unwrap();
    for (family, class) in [
        (Family::Wolf, "cEm0200Bite"),
        (Family::Wolf, "cEm0200Howling"),
        (Family::Wolf, "cEm0200AttackRun"),
        (Family::Harpy, "cEm0600ActGroundToHover"),
        (Family::Harpy, "cEm0600ActDie"),
    ] {
        let data = BeastData::load(&vfs, &image, family).unwrap();
        let id = data
            .actions
            .iter()
            .position(|a| a.class == class && (class != "cEm0200Die" || a.variant == 0))
            .unwrap();
        let run = || {
            let mut actor = Beast::new(&data, 310, [0.0; 3], 0.0);
            actor.start_action(&data, id).unwrap();
            let mut motions = vec![actor.motion];
            let mut attack_frames = 0;
            let mut flew = false;
            for _ in 0..3000 {
                actor.step_action(&data, Some([0.0, 0.0, 200.0]), &Flat, 0.5);
                if motions.last() != Some(&actor.motion) {
                    motions.push(actor.motion);
                }
                attack_frames += usize::from(!actor.fighter.attacks.is_empty());
                assert!(actor.fighter.attacks.iter().all(|a| a.side.owner == 310));
                flew |= actor.movement_flags & 1 != 0;
                if actor.done {
                    break;
                }
            }
            assert!(actor.done, "{class} failed to complete");
            if class == "cEm0200Bite" {
                assert_eq!(
                    motions,
                    vec![MotionId::new(1, 0), MotionId::new(1, 1), MotionId::new(1, 2)]
                );
                assert!(attack_frames > 0);
            }
            if class == "cEm0600ActGroundToHover" {
                assert!(flew);
                assert_eq!(motions, vec![MotionId::new(0, 0x29), MotionId::new(0, 0x1f)]);
                assert!(actor.frame >= 57.0);
            }
            let previous = (actor.action, actor.motion, actor.frame);
            assert!(actor.start_action(&data, usize::MAX).is_err());
            assert_eq!(previous, (actor.action, actor.motion, actor.frame));
            (motions, actor.pos, actor.yaw, actor.frame, attack_frames)
        };
        assert_eq!(run(), run(), "{class} nondeterministic");
        if class == "cEm0200AttackRun" {
            let param = data.wolf.unwrap();
            for (target, expected) in [
                ([0.0, 0.0, param.attack_range - 1.0], true),
                ([0.0, 0.0, param.attack_range], false),
                ([0.0, 0.0, -param.attack_range + 1.0], false),
            ] {
                let mut actor = Beast::new(&data, 310, [0.0; 3], 0.0);
                actor.start_action(&data, id).unwrap();
                actor.step_action(&data, Some(target), &Flat, 0.5);
                assert_eq!(actor.attack_ready, expected);
                assert!(actor.done); // permits think on the very next frame
            }
        }
    }
}

#[test]
fn wolf_common_ground_death_uses_installed_record_and_never_completes() {
    use ddenemyrt::beast::Beast;
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
    }
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.is_dir() { eprintln!("SKIP: no install"); return; }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let exe = ddexe::ExeFile::next_to(native).unwrap();
    let data = BeastData::load(&vfs, &exe.image().unwrap(), Family::Wolf).unwrap();
    assert_eq!(data.common_death_motion, MotionId::new(2, 0x39));
    let run = || {
        let mut actor = Beast::new(&data, 310, [0.0; 3], 0.0);
        if let ddenemyrt::combat::Victim::Enemy { regions, .. } = &mut actor.fighter.victim {
            regions[0].hp = 0.0;
        }
        actor.start_action(&data, 0x0300_0042).unwrap();
        for _ in 0..data.body.frames(actor.motion) + 10 {
            actor.step_action(&data, None, &Flat, 1.0);
            assert!(!actor.done); // 0x008a6930 never completes a valid owner.
            assert!(actor.fighter.attacks.is_empty());
        }
        assert!(actor.motion_ended);
        assert_eq!(actor.motion, MotionId::new(2, 0x39));
        (actor.pos, actor.frame)
    };
    assert_eq!(run(), run());
}

/// This is a shared hit/targeting-pipeline check with stationary enemies,
/// not acceptance of the still-unconnected family think controllers.
#[test]
fn mixed_family_fight_targets_damage_and_owner_ids_are_separate() {
    use ddenemyrt::fight::{Fight, FightData, Kit};
    use std::sync::Arc;
    struct Flat;
    impl ddsim::Ground for Flat {
        fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
    }
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.is_dir() { return; }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let exe = ddexe::ExeFile::next_to(native).unwrap();
    let data = Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    for family in [Family::Wolf, Family::Harpy] {
        let run = || {
            let mut sim = ddsim::Sim::new(Default::default());
            let mut fight = Fight::new(data.clone());
            for seat in [0, 1] {
                sim.add_player(seat, [if seat == 0 { 10000.0 } else { 0.0 }, 0.0, 0.0], 0.0);
                fight.add_arisen_as(seat, Kit::Mage);
                fight.equip(&mut sim, seat);
            }
            let first = fight.add_beast(family, [0.0, 0.0, 400.0], 0.0);
            let second = fight.add_goblin([10000.0, 0.0, 10000.0], 0.0);
            let third = fight.add_beast(family, [10000.0, 0.0, 12000.0], 0.0);
            assert!(first != second && second != third && first != third);
            assert_eq!(fight.enemy_fighters().count(), 3);
            let initial = fight.beasts[0].fighter.victim.hp();
            let other = fight.beasts[1].fighter.victim.hp();
            fight.volumes(&sim);
            sim.player_mut(1).unwrap().skills.equipped[0] = Some(210);
            sim.request_skill(1, 0);
            let mut hit = false;
            for _ in 0..400 {
                let events = fight.hits(1.0).to_vec();
                for event in events {
                    if event.victim == first {
                        assert_eq!(fight.shooters.get(&event.attacker), Some(&1));
                        hit = true;
                    }
                }
                sim.step(1.0 / 30.0, &Default::default(), &Flat);
                fight.players_moved(&mut sim, &Flat, 1.0);
                fight.push_out(&mut sim);
                fight.volumes(&sim);
            }
            assert!(hit, "{family:?} not hit");
            assert!(fight.beasts[0].fighter.victim.hp() < initial);
            assert_eq!(fight.beasts[1].fighter.victim.hp(), other);
            if family == Family::Wolf {
                if let ddenemyrt::combat::Victim::Enemy { regions, .. } = &mut fight.beasts[0].fighter.victim {
                    regions[0].hp = 0.0;
                }
                for _ in 0..100 {
                    fight.enemies(&sim, &Flat, 1.0);
                }
                let dead = &fight.beasts[0];
                assert_eq!(dead.action, Some(0x0300_0042));
                assert_eq!(dead.motion, MotionId::new(2, 0x39));
                assert!(dead.frame > 0.0, "death must advance rather than restart");
                assert!(!dead.done);
                assert!(dead.fighter.attacks.is_empty());
                assert_eq!(dead.target, None);
            }
            fight.digest()
        };
        assert_eq!(run(), run());
    }
}
