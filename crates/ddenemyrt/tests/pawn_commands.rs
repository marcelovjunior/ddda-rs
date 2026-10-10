//! Native order messages rebuild installed priorities and follow modes.
use ddenemyrt::{fight::FightData, pawn::Command};
use ddpawnai::pawn::{Pawn, Senses};

#[test]
fn installed_go_come_help_change_priorities_and_battle_follow_distances() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no DDDA install"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let data = FightData::load(&vfs, &exe.image().unwrap()).unwrap();
    let mut pawn = Pawn::new(&data.pawn, [500.0; 9], 1, 7);
    let senses = Senses { pos: [0.0; 3], yaw: 0.0, action: None, slot: 2,
        owner: Some([0.0; 3]), enemies: &[], battle: true, dt: 1.0 };
    let original = pawn.list.clone();
    let mut lists = Vec::new();
    for command in [Command::Go, Command::Come, Command::Help] {
        pawn.orders.deliver(command);
        pawn.step(&data.pawn, &senses);
        assert_eq!(pawn.command, command as i64);
        lists.push(pawn.list.clone());
        let range = pawn.follow_range();
        eprintln!("{command:?}: battle follow {range:?}, cooldown={}", data.pawn.order_cooldown);
        assert_eq!(range.max, match command { Command::Go => 3500.0,
            Command::Come => 750.0, Command::Help => 1500.0, _ => unreachable!() });
    }
    assert_ne!(lists[0], original);
    assert_ne!(lists[0], lists[1]);
    assert_ne!(lists[1], lists[2]);
    pawn.orders.duration = 1.0;
    pawn.step(&data.pawn, &senses);
    assert_eq!(pawn.command, 8);
    assert_eq!(pawn.list, original);
}
