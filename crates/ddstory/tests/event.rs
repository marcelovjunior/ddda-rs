//! st200 ev10 (the wake-up scene, event row 22) played headless, alone and
//! inside the opening. Skips when the install is absent (set
//! DDDA_NATIVE_PC and DDDA_EXE to use another one).

use ddstory::data::InstallData;
use ddstory::event::{Cue, EventConfig, EventFrame, EventPlayer};
use ddstory::scripted::ScriptedHost;
use ddstory::{Request, Story};

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

const CUTS: [u32; 8] = [0, 460, 670, 790, 910, 1030, 1150, 1270];

/// (frame, label) whenever the subtitle label changes to a shown line.
fn labels(frames: &[EventFrame]) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = vec![];
    for f in frames {
        if let (Some(fr), Some(s)) = (f.frame, &f.subtitle) {
            if out.last().map(|(_, l)| l) != Some(&s.label) {
                out.push((fr as u32, s.label.clone()));
            }
        }
    }
    out
}

fn play(data: &InstallData, language: usize, sex: u8) -> Vec<EventFrame> {
    let cfg = EventConfig { language, sex, hour: 8, stage: 200 };
    let mut p = EventPlayer::open(data, 22, cfg).unwrap();
    assert_eq!(p.end_frame, 1701);
    let mut frames = vec![];
    for _ in 0..5000 {
        let f = p.step(1.0).unwrap();
        let done = f.finished;
        frames.push(f);
        if done {
            return frames;
        }
    }
    panic!("st200 ev10 did not end");
}

#[test]
fn st200_ev10_headless() {
    let Some(data) = install() else { return };
    let frames = play(&data, 1, 0);

    // One update per frame at mDeltaTime 1.0: frames 0..=1701, and the
    // event ends at frame count - 1 (0x00435fb0).
    assert_eq!(frames.len(), 1702);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f.frame, Some(i as f32), "update {i}");
    }
    assert!(frames[1701].finished && !frames[1700].finished);

    // Camera cuts: mCut steps 0..7 at the cut frames, the local camera
    // frame restarts at 0, every frame has a base pose, and each actor's
    // motion number is the cut number.
    let mut cuts = vec![];
    for f in &frames {
        let c = f.camera.as_ref().unwrap();
        assert!(c.pose.is_some(), "pose at {:?}", f.frame);
        if cuts.last().map(|(_, k)| *k) != Some(c.cut) {
            cuts.push((f.frame.unwrap() as u32, c.cut));
            assert_eq!(c.frame, 0.0);
        }
        for a in &f.actors {
            assert_eq!(a.motion_no, c.cut, "{} at {:?}", a.name, f.frame);
        }
    }
    let want: Vec<(u32, i32)> = CUTS.iter().enumerate().map(|(k, &f)| (f, k as i32)).collect();
    assert_eq!(cuts, want);
    // The local frame counts up within a cut (count mode).
    assert_eq!(frames[459].camera.as_ref().unwrap().frame, 459.0);
    assert_eq!(frames[700].camera.as_ref().unwrap().frame, 30.0);
    // Actors: the player, NPC 1 and NPC 2 (row 22's NPC list [1, 2]).
    let who: Vec<(i32, i32)> = frames[0].actors.iter().map(|a| (a.unit_type, a.unit_no)).collect();
    assert_eq!(who, vec![(0, 0), (2, 1), (2, 2)]);
    // The player is hidden in cuts 2-6.
    assert!(frames[700].actors[0].motion_no == 2 && !frames[700].actors[0].draw);
    assert!(frames[1300].actors[0].draw);

    // Subtitles by GMD label, in English, male variant (label 2, not 3).
    let l = labels(&frames);
    let want: Vec<(u32, String)> = [
        (30, "0"),
        (151, "1"),
        (326, "2"),
        (480, "4"),
        (610, "5"),
        (700, "6"),
        (729, "7"),
        (825, "8"),
        (931, "9"),
        (1052, "10"),
        (1186, "11"),
        (1271, "12"),
    ]
    .iter()
    .map(|(f, s)| (*f, s.to_string()))
    .collect();
    assert_eq!(l, want);
    for f in &frames {
        if let Some(s) = &f.subtitle {
            assert!(s.text.as_deref().is_some_and(|t| !t.is_empty()), "text for {}", s.label);
        }
    }
    // Hidden between lines (Visible false at 113).
    assert!(frames[120].subtitle.is_none());

    // Fade: black at the start, clear from 60, black again from 1643.
    assert_eq!(frames[0].fade, Some([0.0, 0.0, 0.0, 1.0]));
    assert!(frames[60..1643].iter().all(|f| f.fade.is_none()));
    assert!(frames[1701].fade.unwrap()[3] > 0.9);

    // Sound cues: both streams at frame 0, the Arisen's voice at 514 (two
    // variants by mConditionalNo).
    let cues: Vec<(u32, &Cue)> =
        frames.iter().flat_map(|f| f.cues.iter().map(move |c| (f.frame.unwrap() as u32, c))).collect();
    assert_eq!(cues.len(), 4);
    assert!(cues[..2].iter().all(|(f, c)| *f == 0 && matches!(c, Cue::Stream { .. })));
    assert!(cues[2..].iter().all(|(f, c)| *f == 514 && matches!(c, Cue::Se { se_id: 1, .. })));

    // The female variant shows label 3 at frame 326; German text differs.
    let ff = play(&data, 4, 1);
    let lf = labels(&ff);
    assert_eq!(lf[2], (326, "3".to_string()));
    let en = frames[30].subtitle.as_ref().unwrap().text.clone();
    let de = ff[30].subtitle.as_ref().unwrap().text.clone();
    assert!(en.is_some() && de.is_some() && en != de);
}

#[test]
fn opening_plays_st200_ev10_and_returns_control() {
    let Some(data) = install() else { return };
    let mut story = Story::new_game(data);
    let mut host = ScriptedHost::opening();
    host.play_rows.insert(22);
    let route = ddstory::route::opening();
    let t = ddstory::route::run(&mut story, &mut host, &route, 240, 45_000, |s| s.quest(2).is_some()).unwrap();

    // The same ladder and boots as with the fixed-length stand-in.
    assert_eq!(t.sce_ladder, vec![100, 200, 350, 370, 650, 750, 800, 850, 1000, 1025, 1050, 1100, 1200, 1300, 1400]);
    assert_eq!(t.boots, vec![28, 0, 31, 1, 2]);

    // Row 22 played for all its 1702 frames.
    let (row, start, frames) = &t.events[0];
    assert_eq!((*row, frames.len()), (22, 1702));
    assert_eq!(labels(frames).len(), 12);

    // Control returns through the story: the end step raises SceNo to 1200
    // (raise-only) and places the party at start position 12 of stage 200,
    // after the event's last frame.
    let end = start + frames.len() as u64;
    let place = t
        .requests
        .iter()
        .find(|(f, r)| *f >= *start && matches!(r, Request::PlaceParty { .. }))
        .unwrap();
    assert!(place.0 >= end, "placed at {} before the end {end}", place.0);
    assert!(matches!(place.1, Request::PlaceParty { start_pos: Some(12), .. }));
    let w = &t.checkpoints.iter().find(|(n, _)| *n == "cassardis: room exit").unwrap().1;
    assert_eq!(w.sce_no, 1200);
    assert!(w.quest_bit(1, 3) && w.quest_bit(1, 95));
}

/// The two NPC actors of st200 ev10 get their event outfits: NPC 1 (male,
/// armour set 0x8a) and NPC 2 (female, set 0x89), plus hair.
#[test]
fn st200_ev10_npc_looks() {
    let Some(data) = install() else { return };
    let a = ddstory::look::npc_look(&data, 1, 1, 22).unwrap();
    let b = ddstory::look::npc_look(&data, 2, 2, 22).unwrap();
    for l in [&a, &b] {
        eprintln!("{} {:?} set {:#x} base {} models {:?} on {:?} off {:?}", l.npc, l.edit_path, l.armor_set, l.base, l.models, l.parts_on, l.parts_off);
    }
    assert_eq!((a.edit.sex, a.armor_set), (0, 0x8a));
    assert_eq!((b.edit.sex, b.armor_set), (1, 0x89));
    assert!(a.base.ends_with("m000") && b.base.ends_with("f000"));
    assert!(a.models[0].1.ends_with(r"e_npc004\e_npc004"));
    assert!(b.models[0].1.ends_with(r"e_npc005\e_npc005"));
    // Hair from h_parts (edp +0xb0).
    assert!(a.models.iter().any(|(_, p)| p.contains(r"\hair\")));
    assert!(b.models.iter().any(|(_, p)| p.contains(r"\hair\")));
}
