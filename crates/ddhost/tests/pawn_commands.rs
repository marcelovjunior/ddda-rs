//! Installed Rook record, native priority table and per-seat command delivery.
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddenemyrt::{fight::{Fight, FightData}, pawn::Command};
use ddstory::{data::InstallData, scripted::ScriptedHost, Story};
use ddsim::{Input, PawnOrderButtons, Sim};
use std::sync::Arc;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, y: f32) -> Option<f32> { (y >= 0.0).then_some(0.0) }
}

fn installed_host() -> Option<Host<InstallData, ScriptedHost>> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no DDDA install"); return None; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let data = Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    let mut host = Host::new(Sim::new(Default::default()),
        Some(Story::new_game(InstallData::open(native, None).unwrap())),
        ScriptedHost::default(), Box::new(Flat));
    assert_eq!(host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0), Some(0));
    assert_eq!(host.join(ProfileId(2), Control::Remote, [100.0, 0.0, 0.0], 0.0), Some(1));
    assert!(host.recruit_story_pawn(14, 1).unwrap());
    assert_eq!(host.rift.party.hired[0].as_ref().unwrap().pawn.rom_id, 0x2000_000e);
    // Fixture isolates host combat/input; recruiting used the real record.
    host.story = None;
    let mut fight = Fight::new(data);
    for seat in [0, 1] { fight.add_arisen(seat); fight.equip(&mut host.sim, seat); }
    fight.add_pawn(&mut host.sim, Host::<InstallData, ScriptedHost>::main_pawn_seat(),
        0, [0.0, 0.0, 200.0], 0.0, [500.0; 9], 7);
    // A separate owned support actor tests the N-seat recipient policy.
    fight.add_pawn(&mut host.sim, 8, 1, [100.0, 0.0, 200.0], 0.0, [500.0; 9], 8);
    host.fight = Some(fight);
    Some(host)
}

fn input(buttons: PawnOrderButtons) -> PlayerInput {
    PlayerInput { sim: Input { pawn_orders: buttons, ..Default::default() }, ..Default::default() }
}

#[test]
fn rook_and_shared_main_pawn_receive_authorized_orders_in_host_frame_order() {
    let Some(mut host) = installed_host() else { return };
    let inputs = FrameInputs::from([
        (0, input(PawnOrderButtons { up: true, ..Default::default() })),
        (1, input(PawnOrderButtons { down: true, ..Default::default() })),
    ]);
    let report = host.tick(&inputs).unwrap();
    assert_eq!(report.pawn_order_requests.iter().map(|d| (d.sender, d.command)).collect::<Vec<_>>(),
        [(0, Command::Go), (1, Command::Come)]);
    let recipients = |index: usize| report.pawn_order_requests[index].replies.iter()
        .map(|r| r.pawn).collect::<Vec<_>>();
    assert_eq!(recipients(0), [4, 6]);
    assert_eq!(recipients(1), [4, 8]);
    let fight = host.fight.as_ref().unwrap();
    assert_eq!(fight.pawns[&4].brain.orders.mailbox, Command::Come, "last valid seat wins shared tie");
    assert_eq!(fight.pawns[&6].brain.orders.mailbox, Command::Go, "Rook obeys its hiring seat");
    assert_eq!(fight.pawns[&8].brain.orders.mailbox, Command::Come);
    assert_eq!(fight.pawns[&6].brain.command, 8, "pass C cannot change this frame's chosen pawn pad");
    host.tick(&inputs).unwrap();
    let fight = host.fight.as_ref().unwrap();
    assert_eq!(fight.pawns[&4].brain.command, 10);
    assert_eq!(fight.pawns[&6].brain.command, 9);
    assert_eq!(fight.pawns[&8].brain.command, 10);
    assert!(host.tick(&inputs).unwrap().pawn_order_requests.is_empty(), "reuploads never repeat");
    host.tick(&Default::default()).unwrap();
    let elapsed = host.fight.as_ref().unwrap().data.pawn.order_cooldown.ceil() as usize + 2;
    for _ in 0..elapsed { host.tick(&Default::default()).unwrap(); }
    let help = FrameInputs::from([(0, input(PawnOrderButtons { left: true, right: true, ..Default::default() }))]);
    assert_eq!(host.tick(&help).unwrap().pawn_order_requests[0].command, Command::Help);
    host.tick(&Default::default()).unwrap();
    let rook = &host.fight.as_ref().unwrap().pawns[&6].brain;
    assert_eq!(rook.command, 11);
    assert_eq!(rook.orders.active, Command::Help);
    assert!(rook.list.iter().any(|entry| entry.param.orders.iter().any(|order| order.order_type == 11)),
        "Help rebuilds the installed priority table, without fabricating a support goal");
}
