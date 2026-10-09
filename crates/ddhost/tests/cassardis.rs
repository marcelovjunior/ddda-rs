//! The Cassardis slice of the opening as the viewer plays it
//! (m5-cassardis), headless: fast-forward to q0001 ("as after the
//! prologue"), the weapon table's vocation menu answered by the view, the
//! room exit and event st200 ev10 played by the view, a talk with Quina
//! through `Host::talk`, then the walk to the 1300 volume, event st200
//! ev15 and the boot of q0002. Two seats; skips without the install.

use ddhost::opening::{fast_forward, to_newly_arisen, OpeningWorld, CASSARDIS_ROWS};
use ddhost::systems::SystemData;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddnpcrt::TalkEvent;
use ddsim::{Ground, Sim};
use ddstory::data::InstallData;
use ddstory::{Menu, Request, Story};
use std::collections::HashMap;
use std::sync::Arc;

struct Void;
impl Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

fn paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    (np.exists() && exe.exists()).then_some((np, exe))
}

/// Stand seat 0 in waypoint `name`'s volume for one frame.
fn tick_at(h: &mut Host<InstallData, OpeningWorld>, name: &str, interact: bool) -> Vec<Request> {
    let route = ddstory::route::opening();
    let w = route.iter().find(|w| w.name == name).unwrap();
    let s = h.story.as_ref().unwrap();
    let (pos, yaw) = w.place(&s.stage.scenarios).unwrap();
    let p = h.sim.player_mut(0).unwrap();
    (p.position, p.yaw) = (pos, yaw);
    let mut inputs = FrameInputs::new();
    inputs.insert(
        0,
        PlayerInput {
            interact,
            ..Default::default()
        },
    );
    inputs.insert(1, PlayerInput::default());
    h.tick(&inputs).unwrap().requests
}

#[test]
fn cassardis_from_newly_arisen_to_upon_a_pawn() {
    let Some((np, exe)) = paths() else {
        eprintln!("SKIP: DDDA install absent");
        return;
    };
    let story = Story::new_game(InstallData::open(&np, Some(&exe)).unwrap());
    let mut h = Host::new(
        Sim::new(HashMap::new()),
        Some(story),
        OpeningWorld::new(),
        Box::new(Void),
    );
    h.data = SystemData {
        npc: Some(Arc::new(ddnpcrt::InstallData::open(&np, "eng").unwrap())),
        ..Default::default()
    };
    h.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    h.join(ProfileId(2), Control::Remote, [0.0; 3], 0.0).unwrap();

    // 1. As after the prologue: q0001 booted in Cassardis.
    let f = fast_forward(&mut h, &to_newly_arisen(), 240, 30_000, |h| {
        h.story.as_ref().is_some_and(|s| s.quest(1).is_some())
    })
    .unwrap();
    assert_eq!(
        f.arrival,
        Some((200, 2)),
        "ev05 party placement survives fast-forward"
    );
    let w = &h.story.as_ref().unwrap().world;
    assert_eq!((w.stage_no, w.sce_no), (200, 1100), "{:?}", f.ladder);
    assert!(h.story.as_ref().unwrap().quest(1).is_some());
    h.world.live(CASSARDIS_ROWS);

    // 2. The weapon table: the menu waits for the view, which picks the
    //    Fighter; closing it sets GSF 372.
    let mut opened = false;
    for frame in 0..600 {
        tick_at(&mut h, "cassardis: weapon table", frame % 2 == 0);
        if h.world.menu.is_some() {
            opened = true;
            break;
        }
    }
    assert!(opened, "no vocation menu");
    assert_eq!(h.world.menu.map(|m| m.0), Some(Menu::Vocation));
    h.world
        .close_menu(Some(ddskill::vocation::Vocation::Fighter as u32));
    for _ in 0..5 {
        tick_at(&mut h, "cassardis: weapon table", false);
    }
    let w = &h.story.as_ref().unwrap().world;
    assert!(w.gsf.get(372), "GSF 372 after the menu");
    assert_eq!(w.player_job, 1);

    // 3. The room exit requests event st200 ev10 (row 22); the view plays
    //    it, then SceNo 1200 and the party placement (start position 12).
    let mut row = None;
    for _ in 0..1200 {
        tick_at(&mut h, "cassardis: room exit", false);
        if let Some(r) = h.world.inner.awaiting.take() {
            row = Some(r);
            break;
        }
    }
    assert_eq!(row, Some(22));
    for _ in 0..60 {
        tick_at(&mut h, "cassardis: room exit", false);
    }
    h.world.event_done();
    let mut placed = None;
    for _ in 0..60 {
        for r in tick_at(&mut h, "cassardis: room exit", false) {
            if let Request::PlaceParty { start_pos, .. } = r {
                placed = start_pos;
            }
        }
    }
    let w = &h.story.as_ref().unwrap().world;
    assert_eq!(w.sce_no, 1200);
    assert_eq!(placed, Some(12));

    // 4. Quina on her stage-200 schedule; a talk under the lock.
    let vfs = ddvfs::Vfs::open(&np).unwrap();
    let ids = ddhost::install::stage_npcs(&vfs, 200, 1200);
    assert!(ids.contains(&2) && ids.contains(&1), "{ids:?}");
    assert!(h.spawn_npc(2, 1).unwrap());
    let mut lines = vec![];
    let mut out = h.talk(0, 2).unwrap();
    assert!(h.talk(1, 2).is_err(), "the guest finds Quina busy");
    for _ in 0..20 {
        let mut ended = false;
        for e in &out.events {
            match e {
                TalkEvent::Line { text, .. } => lines.push(text.clone()),
                TalkEvent::End { .. } => ended = true,
                _ => {}
            }
        }
        if ended {
            break;
        }
        out = h.talk_next(0).unwrap();
    }
    assert!(!lines.is_empty());
    eprintln!("Quina at SceNo 1200: {lines:?}");
    assert_eq!(h.npcs.talking_to(2), None);

    // 5. Walking out: the 1300 volume, then event st200 ev15 (row 23),
    //    SceNo 1400, the NPC script and the boot of q0002.
    let mut row = None;
    for _ in 0..1200 {
        tick_at(&mut h, "cassardis: 1300", false);
        if let Some(r) = h.world.inner.awaiting.take() {
            row = Some(r);
            break;
        }
    }
    assert_eq!(h.story.as_ref().unwrap().world.sce_no, 1300);
    let mut ladder = vec![1300];
    if row.is_none() {
        for _ in 0..1200 {
            tick_at(&mut h, "cassardis: q0002 event", false);
            if let Some(r) = h.world.inner.awaiting.take() {
                row = Some(r);
                break;
            }
        }
    }
    assert_eq!(row, Some(23));
    h.world.event_done();
    for _ in 0..600 {
        tick_at(&mut h, "cassardis: q0002 event", false);
        let s = h.story.as_ref().unwrap();
        if ladder.last() != Some(&s.world.sce_no) {
            ladder.push(s.world.sce_no);
        }
        if s.quest(2).is_some() {
            break;
        }
    }
    let s = h.story.as_ref().unwrap();
    assert_eq!(ladder, vec![1300, 1400]);
    assert!(s.world.gsf.get(381));
    assert!(s.quest(2).is_some(), "q0002 booted");
    assert_eq!(s.world.quest_bits(1).unwrap().words(), &[0x18000000, 0, 3]);
}
