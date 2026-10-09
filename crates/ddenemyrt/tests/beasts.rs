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
