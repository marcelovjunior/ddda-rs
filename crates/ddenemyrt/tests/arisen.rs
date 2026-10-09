//! The Arisen kits from the install. Skipped without it.

use ddenemyrt::arisen::{self, ArisenData};
use ddenemyrt::body::MotionId;

fn vfs() -> Option<ddvfs::Vfs> {
    let p = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !p.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    Some(ddvfs::Vfs::open(p).unwrap())
}

#[test]
fn a_level_1_mage_loads() {
    let Some(vfs) = vfs() else { return };
    let m = ArisenData::mage(&vfs).unwrap();
    assert!(m.body.motion(MotionId::new(arisen::WEAPON_AT_BANK, 35)).is_some());
    assert_eq!(m.weapon(), ddsim::Weapon::Staff);
    // the starting staff's level (item +0x66) is 3: 2 above level 1
    assert_eq!((m.level, m.weapon_level), (1, 3));
    eprintln!("mage attack {:?} hp {}", m.attacker.base_attack, m.hp_max);
}

#[test]
fn the_weapon_banks_go_to_ddsim_with_their_events() {
    let Some(vfs) = vfs() else { return };
    let f = ArisenData::fighter(&vfs).unwrap();
    assert_eq!(f.weapon(), ddsim::Weapon::Sword);
    let m = f.sim_motions();
    // S1_1 (0x700): 44 frames, slot 1 buffering from frame 8
    let s1 = &m[&0x700];
    assert_eq!(s1.frame_count, 44);
    assert_eq!(ddsim::attack::slot_state(s1.event_bits(0, 8, 8), 1), 1);
    assert_eq!(ddsim::attack::slot_state(s1.event_bits(0, 13, 13), 1), 2);
    assert!(m.contains_key(&0x600));
}
