//! Goblin FSMs from the install, driven by synthetic inputs.
//! `DDDA_NATIVE_PC` (default the maintainer's install); skipped when absent.
use ddai::think::{Actor, Candidate, Inputs, Request, Thinker};
use ddai::Error;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

fn vfs() -> Option<&'static ddvfs::Vfs> {
    static V: OnceLock<Option<ddvfs::Vfs>> = OnceLock::new();
    V.get_or_init(|| {
        let p = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
        if !p.is_dir() {
            eprintln!("SKIP goblin tests: {} absent", p.display());
            return None;
        }
        Some(ddvfs::Vfs::open(p).unwrap())
    })
    .as_ref()
}

fn thinker(path: &str) -> Thinker {
    let b = vfs().unwrap().read(path, mtarc::type_hash("rAIFSM")).unwrap();
    Thinker::new(path, Arc::new(mtfsm::Graph::parse(&b).unwrap())).unwrap()
}

const GATE: &str = "ai\\fsm\\enemy\\st100\\quest\\q0007_gate2goblin0000_ai";
/// The two gate points the FSM checks (its Set Check Pos vectors).
const POINT_A: [f32; 3] = [-85.520_39, 35301.773, 73322.516];
const POINT_B: [f32; 3] = [1639.383, 35228.164, 74376.91];

fn at(p: [f32; 3], dx: f32) -> Inputs {
    Inputs {
        actor: Some(Actor {
            pos: p,
            world_pos: [p[0] + dx, p[1], p[2]],
            action_done: false,
        }),
        players: vec![[0.0; 3]],
        ..Inputs::default()
    }
}

/// Runs frames until the thinker finishes; returns the visited state names
/// (in order, without repeats) and every request.
fn run(t: &mut Thinker, inputs: &Inputs, max: usize) -> (Vec<String>, Vec<Request>, usize) {
    let mut names: Vec<String> = vec![];
    let mut reqs = vec![];
    for frame in 1..=max {
        let r = t.frame(inputs).unwrap();
        for q in &r {
            if let Request::FreeCondition { bits } = q {
                // The FSM held by this goblin's AI controller is this one.
                t.or_free_condition(*bits);
            }
        }
        reqs.extend(r);
        let n = t.current_names()[0].clone();
        if names.last() != Some(&n) {
            names.push(n);
        }
        if t.finished() {
            return (names, reqs, frame);
        }
    }
    panic!("{GATE} did not finish in {max} frames: {names:?}");
}

/// The gate goblin of q0007: wait, run to its gate point, then notice and
/// bark once it stands there. Each run ends in Finish; the host restarts
/// the FSM (the restart policy is the host's), FreeCondition carries over.
#[test]
fn gate_goblin_idle_approach_notice() {
    if vfs().is_none() {
        return;
    }
    let mut t = thinker(GATE);
    let far = at(POINT_B, 5000.0);
    // 1. Idle: s0000, then "6秒後" (after 6 s) with Timer[0] running, then
    //    "フリーフラグ0on" sets FreeCondition 1, then Finish.
    let (names, reqs, frames) = run(&mut t, &far, 400);
    assert_eq!(names, ["s0000", "6秒後", "フリーフラグ0on", "Finish"]);
    assert_eq!(reqs, [Request::FreeCondition { bits: 1 }]);
    // Timer[0] keeps running through the last two states (nothing stops it).
    assert!(
        t.fields.timers[0] > 6.0 && t.fields.timers[0] < 6.0 + 4.0 / 30.0,
        "{}",
        t.fields.timers[0]
    );
    assert!((180..=186).contains(&frames), "{frames}");
    assert_eq!(t.fields.free_condition, 1);
    // 2. Approach: far from gate point B, it dashes there (escape point +
    //    think table 22 of uEm0100, the native movement AI).
    t.restart().unwrap();
    let (names, reqs, _) = run(&mut t, &far, 10);
    assert_eq!(names, ["s0000", "ポジションチェック", "ダッシュ", "Finish"]);
    assert_eq!(reqs[0], Request::EscapePos { pos: POINT_B });
    assert_eq!(
        reqs[1],
        Request::ThinkTable {
            table: 22,
            mode: 0,
            class: "uEm0100".into()
        }
    );
    // 3. At gate point B (within 200), it marks FreeCondition 2.
    t.restart().unwrap();
    let (names, reqs, _) = run(&mut t, &at(POINT_B, 150.0), 10);
    assert_eq!(
        names,
        ["s0000", "ポジションチェック", "フリーフラグ0on", "Finish"]
    );
    assert_eq!(reqs, [Request::FreeCondition { bits: 2 }]);
    assert_eq!(t.fields.free_condition, 3);
    // 4. Notice and attack request: with FreeCondition 2 it checks gate
    //    point A; standing there, "発見～威嚇吠え" (found -> threat roar)
    //    requests action 72.
    t.restart().unwrap();
    let (names, reqs, _) = run(&mut t, &at(POINT_A, 0.0), 10);
    assert_eq!(names, ["s0000", "ポジションチェック", "発見～威嚇吠え", "Finish"]);
    assert_eq!(reqs, [Request::Action { action: 72 }]);
}

/// q0002_goburin: one state, one CtrlAction (action 1) on entry; its state
/// callback arms on the first frame and reports completion through
/// SysCondition 0x20000000 once the actor's action is done.
#[test]
fn single_action_completion() {
    if vfs().is_none() {
        return;
    }
    let mut t = thinker("ai\\fsm\\enemy\\st100\\quest\\q0002_goburin");
    let mut i = at([0.0; 3], 0.0);
    assert_eq!(t.frame(&i).unwrap(), [Request::Action { action: 1 }]);
    assert_eq!(t.fields.sys_condition & 0x2000_0000, 0);
    assert_eq!(t.frame(&i).unwrap(), []);
    i.actor.as_mut().unwrap().action_done = true;
    t.frame(&i).unwrap();
    assert_ne!(t.fields.sys_condition & 0x2000_0000, 0);
}

/// q0003_goburin: "NPCを狙う" (target the NPC) searches group 2 within 100
/// and finishes when a target is found or not.
#[test]
fn search_target_then_finish() {
    if vfs().is_none() {
        return;
    }
    let mut t = thinker("ai\\fsm\\enemy\\st100\\quest\\q0003_goburin");
    let mut i = at([0.0; 3], 0.0);
    i.groups = vec![
        vec![],
        vec![],
        vec![
            Candidate {
                id: 7,
                pos: [150.0, 0.0, 0.0],
                alive: true,
            },
            Candidate {
                id: 8,
                pos: [60.0, 0.0, 0.0],
                alive: true,
            },
            Candidate {
                id: 9,
                pos: [10.0, 0.0, 0.0],
                alive: false,
            },
        ],
    ];
    let mut reqs = vec![];
    for _ in 0..4 {
        reqs.extend(t.frame(&i).unwrap());
        if t.finished() {
            break;
        }
    }
    assert!(t.finished());
    assert_eq!(reqs.first(), Some(&Request::Target { group: 2, id: 8 }));
    assert_eq!(t.current_names(), ["s0001"]);
}

/// Every goblin graph in the install either runs 300 frames with idle
/// inputs or stops at an explicit error naming what is missing.
#[test]
fn goblin_corpus_runs_or_names_the_gap() {
    let Some(v) = vfs() else { return };
    let h = mtarc::type_hash("rAIFSM");
    let mut paths: Vec<String> = v
        .paths_of_type(h)
        .filter(|p| {
            let p = p.to_lowercase();
            p.contains("gob") || p.contains("em0100") || p.contains("em0101")
        })
        .map(String::from)
        .collect();
    paths.sort();
    assert!(paths.len() >= 30, "{}", paths.len());
    let mut ran = 0;
    let mut gaps = std::collections::BTreeSet::new();
    for p in &paths {
        let g = Arc::new(mtfsm::Graph::parse(&v.read(p, h).unwrap()).unwrap());
        let mut t = match Thinker::new(p, g) {
            Ok(t) => t,
            Err(e) => panic!("{p}: {e}"),
        };
        let i = at([0.0; 3], 0.0);
        let mut err = None;
        for _ in 0..300 {
            if let Err(e) = t.frame(&i) {
                err = Some(e);
                break;
            }
        }
        match err {
            None => ran += 1,
            Some(Error::Unimplemented(s)) => {
                gaps.insert(
                    s.split(": ")
                        .nth(1)
                        .unwrap_or(&s)
                        .split(" in state")
                        .next()
                        .unwrap()
                        .to_string(),
                );
            }
            Some(e) => panic!("{p}: {e}"),
        }
    }
    eprintln!("{ran} of {} goblin graphs ran; gaps: {gaps:?}", paths.len());
    assert!(ran > 0);
}
