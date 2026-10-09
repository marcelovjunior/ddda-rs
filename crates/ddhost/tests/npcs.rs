//! NPCs in a two-player host: talking to Caxton opens his menu under the
//! interaction lock, the menu's Buy opens the shop session, closing it
//! plays his after-buy line; Quina keeps her daily schedule on the host
//! clock. Needs the install (DDDA_NATIVE_PC); skips without it.

use ddeconomy::inventory::Owner;
use ddhost::data::NoStoryData;
use ddhost::lock::LockError;
use ddhost::systems::SystemData;
use ddhost::{Control, FrameInputs, Host, HostError, ProfileId};
use ddnpcrt::talk::{Action, MenuKind};
use ddnpcrt::TalkEvent;
use ddsim::{Ground, Sim};
use ddstory::scripted::ScriptedHost;
use ddstory::Story;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

fn host() -> Option<Host<NoStoryData, ScriptedHost>> {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !native.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let data = SystemData {
        catalog: Some(Arc::new(ddhost::install::catalog(&vfs).unwrap())),
        npc: Some(Arc::new(ddnpcrt::InstallData::open(&native, "eng").unwrap())),
        ..Default::default()
    };
    let mut h = Host::new(Sim::new(HashMap::new()), Some(Story::new_game(NoStoryData)), ScriptedHost::default(), Box::new(Flat));
    h.data = data;
    h.join(ProfileId(1), Control::Local, [0.0, 0.0, 0.0], 0.0).unwrap();
    h.join(ProfileId(2), Control::Remote, [200.0, 0.0, 0.0], 0.0).unwrap();
    Some(h)
}

const CAXTON: u32 = 149;

#[test]
fn talk_to_a_shopkeeper_under_the_lock() {
    let Some(mut h) = host() else { return };
    h.story.as_mut().unwrap().world.set_sce_no(8000, false);
    h.economy.wallet(0).gold = 10_000;
    let out = h.talk(0, CAXTON).unwrap();
    assert!(matches!(&out.events[0], TalkEvent::Line { conversation: 100, .. }));
    // The other Arisen finds Caxton busy, for talking and for his shop.
    assert!(matches!(h.talk(1, CAXTON), Err(HostError::Lock(LockError::Held(0)))));
    assert!(matches!(h.open_shop(1, CAXTON), Err(HostError::Lock(LockError::Held(0)))));
    let out = h.talk_next(0).unwrap();
    assert!(matches!(&out.events[0], TalkEvent::Choices { options, .. } if options[0] == "Buy"));
    // "Buy" opens the shop session; the lock stays with player 0.
    let out = h.talk_choose(0, 0).unwrap();
    assert_eq!(out.events[0], TalkEvent::Action { player: 0, npc: CAXTON, action: Action::Shop { mode: 0 } });
    let s = out.session.expect("shop session");
    assert_eq!(h.locks.holder(CAXTON), Some(0));
    assert_eq!(h.buy(&s, 307, 1, Owner::Arisen(0)).unwrap(), 2700);
    // Closing the screen: "Take proper care of that.", then the talk ends.
    let out = h.close_shop_talk(s, Some(MenuKind::AfterBuy)).unwrap();
    assert!(matches!(&out.events[0], TalkEvent::Line { conversation: 105, .. }));
    let out = h.talk_next(0).unwrap();
    assert!(matches!(out.events.last(), Some(TalkEvent::End { .. })));
    assert_eq!(h.locks.holder(CAXTON), None);
    // Now the guest can talk to him; leaving the session frees him.
    h.talk(1, CAXTON).unwrap();
    h.leave(1);
    assert_eq!(h.locks.holder(CAXTON), None);
    assert_eq!(h.npcs.talking_to(CAXTON), None);
}

#[test]
fn quina_keeps_her_schedule_in_the_host() {
    let Some(mut h) = host() else { return };
    {
        let w = &mut h.story.as_mut().unwrap().world;
        w.stage_no = 100;
        w.set_sce_no(1000, false);
        w.hour = 13;
    }
    assert!(h.spawn_npc(2, 1).unwrap());
    let mut moves = 0;
    for f in 0..300 {
        let _ = f;
        let r = h.tick(&FrameInputs::new()).unwrap();
        moves += r.npc_requests.iter().filter(|q| matches!(q, ddnpcrt::schedule::Request::Goto { .. })).count();
    }
    assert!(moves > 0);
    let path = h.npcs.npcs[&2].action_path.clone().unwrap();
    assert!(path.to_ascii_lowercase().ends_with("go002_afternoon"), "{path}");
    assert!(h.npcs.position(2).is_some());
}
