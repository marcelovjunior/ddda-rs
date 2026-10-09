//! The opening, headless: new game -> prologue -> q0000 -> q0001 -> q0002
//! boot, driven by the scripted host. Skips when the install is absent
//! (set DDDA_NATIVE_PC and DDDA_EXE to use another one).

use ddstory::data::InstallData;
use ddstory::scripted::ScriptedHost;
use ddstory::{Request, Story, World};

fn install() -> Option<InstallData> {
    let np = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    if !np.exists() || !exe.exists() {
        eprintln!("SKIP: DDDA install absent");
        return None;
    }
    Some(InstallData::open(np, Some(&exe)).unwrap())
}

fn words(w: &World, q: usize) -> Vec<u32> {
    w.quest_bits(q).unwrap().words().to_vec()
}

#[test]
fn opening_from_new_game_to_q0002() {
    let Some(data) = install() else { return };
    let mut story = Story::new_game(data);
    let mut host = ScriptedHost::opening();
    let route = ddstory::route::opening();
    let t = ddstory::route::run(&mut story, &mut host, &route, 240, 40_000, |s| s.quest(2).is_some()).unwrap();

    // The SceNo ladder (QUESTS.md). 150 and 600 are camera-end raises the
    // route passes without (see QUESTS.md, Runtime); 900 is raised by the
    // end of st380 ev00 in the same frame as the forced 1000.
    assert_eq!(t.sce_ladder, vec![100, 200, 350, 370, 650, 750, 800, 850, 1000, 1025, 1050, 1100, 1200, 1300, 1400]);
    // Quests in boot order: the prologue's q0028 (by 100ck_evmes), q0000
    // and q0031 at stage 800 (0x004827e0), q0001 by q0000, q0002 by
    // bootEv_st200ev15.
    assert_eq!(t.boots, vec![28, 0, 31, 1, 2]);

    // Cutscenes in order: st380 ev00, st100 ev00 (movie), st200 ev00, ev01,
    // ev05, ev10, ev15.
    let rows: Vec<usize> = t
        .requests
        .iter()
        .filter_map(|(_, r)| match r {
            Request::PlayEvent { row, .. } => Some(*row),
            _ => None,
        })
        .collect();
    assert_eq!(rows, vec![62, 0, 19, 20, 21, 22, 23]);

    // Checkpoints.
    let at = |name: &str| &t.checkpoints.iter().find(|(n, _)| *n == name).unwrap().1;
    let w = at("prologue: chimera");
    assert_eq!((w.sce_no, w.stage_no), (1000, 800));
    // q0028 cleared (QCT row 6 on bit 94; 94 is set by set_03).
    assert!(w.quest_bit(28, 94));
    let w = at("cassardis: dragon attack");
    assert_eq!(w.sce_no, 1100);
    // q0000 bits its script (2, 30) and the dragon's AI (5, 20, 21) set by
    // the end of st200 ev05; 31 and 0 follow in the clean-up states.
    for b in [2, 5, 20, 21, 30] {
        assert!(w.quest_bit(0, b), "q0 bit {b}");
    }
    let w = at("cassardis: room exit");
    assert_eq!(w.sce_no, 1200);
    assert!(w.gsf.get(372), "vocation chosen");
    assert!(w.quest_bit(1, 3) && w.quest_bit(1, 95));

    // End state, compared with the original where a live read exists
    // (claude-4's oracle, maintainer's save at SceNo 3400, 2026-10-09):
    let w = &story.world;
    assert_eq!(w.sce_no, 1400);
    // q0001's words are exactly the live ones.
    assert_eq!(words(w, 1), vec![0x1800_0000, 0, 3]);
    // q0000: cleared (94) and never "started" (95), as live; its other bits
    // are a subset of the live word.
    assert_eq!(words(w, 0)[2], 2);
    assert_eq!(words(w, 0)[0] & !0xe43e_0c03, 0);
    assert!(w.quest_bit(0, 0) && w.quest_bit(0, 31));
    // Flags the live save has set / clear.
    for g in [88, 372, 378, 381] {
        assert!(w.gsf.get(g), "GSF {g} set");
    }
    for g in [360, 377, 520, 530] {
        assert!(!w.gsf.get(g), "GSF {g} clear");
    }
    // q0001's rewards (rQuestReward).
    assert!(t.requests.iter().any(|(_, r)| matches!(r,
        Request::Reward { quest: 1, gold: 2000, exp: 50, rift: 3, items } if items == &vec![(64, 1), (1, 5), (43, 5)])));
    // The world survives a save round trip.
    assert_eq!(&World::load(&w.save()).unwrap(), w);
}
