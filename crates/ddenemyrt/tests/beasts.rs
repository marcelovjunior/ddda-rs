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
    }
}
