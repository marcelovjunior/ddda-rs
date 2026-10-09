//! Against the player's install (skips without one; DDDA_NATIVE_PC
//! overrides the path).
use ddnpcrt::schedule::{Graphs, Request};
use ddnpcrt::talk::{Action, MenuKind};
use ddnpcrt::*;
use std::path::PathBuf;

fn data() -> Option<InstallData> {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    if !root.is_dir() {
        assert!(explicit.is_none(), "DDDA_NATIVE_PC does not exist");
        eprintln!("skipped: no install");
        return None;
    }
    Some(InstallData::open(root, "eng").unwrap())
}

/// Every route point (Goto, DemoSetPos, InitPos) of a graph.
fn points(d: &InstallData, path: &str) -> Vec<[f32; 3]> {
    let g = d.graph(path).unwrap();
    let mut out = vec![];
    fn walk(c: &mtfsm::Cluster, out: &mut Vec<[f32; 3]>) {
        for s in &c.states {
            for p in &s.processes {
                if let Some(r) = &p.parameter {
                    collect(r, out);
                }
            }
            if let Some(sub) = &s.subcluster {
                walk(sub, out);
            }
        }
    }
    fn collect(r: &mtfsm::Record, out: &mut Vec<[f32; 3]>) {
        for (name, vals) in &r.fields {
            for v in vals {
                match v {
                    mtfsm::Parameter::Scalar(mtxfs::Value::F(f)) if f.len() == 4 && (name == "ポジション" || name == "InitPos") => {
                        out.push([f[0] as f32, f[1] as f32, f[2] as f32]);
                    }
                    mtfsm::Parameter::Object(Some(o)) => collect(o, out),
                    _ => {}
                }
            }
        }
    }
    walk(g.root.as_ref().unwrap(), &mut out);
    out
}

#[test]
fn quina_across_a_day() {
    let Some(d) = data() else { return };
    let mut w = ddstory::World::default();
    w.stage_no = 100;
    w.set_sce_no(1000, false);
    let mut rt = NpcRuntime::default();
    assert!(rt.spawn(&d, &w, 2, 1).unwrap());
    rt.npcs.get_mut(&2).unwrap().complete_moves(true);
    let mut seen = vec![];
    for (hour, action) in [(7, "go002_morning"), (13, "go002_afternoon"), (20, "go002_night"), (3, "go002_night")] {
        w.hour = hour;
        w.minute = 0;
        let mut reqs = vec![];
        for _ in 0..600 {
            reqs.extend(rt.step(&d, &w, 0.0, &[]).unwrap());
        }
        let npc = &rt.npcs[&2];
        let path = npc.action_path.clone().unwrap();
        assert!(path.to_ascii_lowercase().ends_with(action), "{hour}:00 -> {path}");
        // Where Quina stands is a point of that action's route.
        let pos = rt.position(2).expect("placed");
        assert!(points(&d, &path).contains(&pos), "{hour}:00 at {pos:?}");
        eprintln!("{hour}:00 {path} {:?} {pos:?} {} requests", npc.action_states(), reqs.len());
        if hour != 3 {
            assert!(reqs.iter().any(|r| matches!(r, Request::Goto { .. } | Request::Place { .. })), "{hour}:00");
        }
        seen.push((hour, pos));
    }
    // Morning, afternoon and night put her in different places.
    assert_ne!(seen[0].1, seen[1].1);
    assert_ne!(seen[1].1, seen[2].1);
}

#[test]
fn quina_talks_by_band_and_flags() {
    let Some(d) = data() else { return };
    let mut w = ddstory::World::default();
    w.set_sce_no(1000, false);
    assert_eq!(w.sce_band, 1);
    let mut rt = NpcRuntime::default();
    let mut said = vec![];
    for _ in 0..3 {
        let story = Story { world: &w, equip_menus: false };
        let ev = rt.talk(&d, &story, 0, 2, false).unwrap();
        let TalkEvent::Line { conversation, text, .. } = &ev[0] else { panic!("{ev:?}") };
        said.push((*conversation, text.clone()));
        while !matches!(rt.talk_next(0).unwrap().last(), Some(TalkEvent::End { .. })) {}
    }
    assert_eq!(said[0], (76, "You should be abed. I wish you would not strain yourself so.".to_string()));
    assert_eq!(said[1].0, 13);
    assert_eq!(said[2].0, 90);
    // Band 2 (SceNo 2000): 20, 18, 21, then 21 again until quest 52 flag 94
    // is set, then 22.
    w.set_sce_no(2000, false);
    let next = |w: &ddstory::World, rt: &mut NpcRuntime| {
        let story = Story { world: w, equip_menus: false };
        let ev = rt.talk(&d, &story, 0, 2, false).unwrap();
        rt.talk_abort(0);
        match &ev[0] {
            TalkEvent::Line { conversation, .. } => *conversation,
            e => panic!("{e:?}"),
        }
    };
    let band2: Vec<i32> = (0..4).map(|_| next(&w, &mut rt)).collect();
    assert_eq!(band2, vec![20, 18, 21, 21]);
    w.set_quest_bit(52, 94, true);
    assert_eq!(next(&w, &mut rt), 22);
    assert_eq!(rt.memory[&2].last, 22);
}

#[test]
fn caxton_menu_opens_the_enhance_screen() {
    let Some(d) = data() else { return };
    let w = ddstory::World::default();
    let story = Story { world: &w, equip_menus: false };
    let mut rt = NpcRuntime::default();
    let ev = rt.talk(&d, &story, 0, 149, true).unwrap();
    assert!(matches!(&ev[0], TalkEvent::Line { conversation: 100, text, .. } if text.starts_with("<UPGR>")));
    assert_eq!(rt.talk(&d, &story, 1, 149, true), Err(TalkError::Busy(0)));
    let ev = rt.talk_next(0).unwrap();
    let TalkEvent::Choices { options, .. } = &ev[0] else { panic!("{ev:?}") };
    assert_eq!(options, &["Buy", "Sell", "Enhance", "Never mind"]);
    let ev = rt.talk_choose(&story, 0, 0).unwrap();
    assert_eq!(ev[0], TalkEvent::Action { player: 0, npc: 149, action: Action::Shop { mode: 0 } });
    // After buying: "Take proper care of that."
    let ev = rt.talk_after_shop(&d, &story, 0, 149, MenuKind::AfterBuy).unwrap();
    assert!(matches!(&ev[0], TalkEvent::Line { conversation: 105, text, .. } if text == "Take proper care of that."));
}
