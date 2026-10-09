//! Deterministic replay: the same inputs give the same state, frame by
//! frame (docs/adr/0002-game-host.md, "Determinism").

use ddhost::data::NoStoryData;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddsim::{Ground, Input, Sim};
use ddstory::scripted::ScriptedHost;
use ddstory::Story;
use std::collections::HashMap;

/// Flat ground at y = 0 (no walls, no ledges).
struct Flat;

impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

/// A scripted two-player session: P1 runs in a circle, sprints and jumps;
/// P2 walks, turns and presses interact now and then.
fn inputs(frame: u64, tweak: Option<u64>) -> FrameInputs {
    let t = frame as f32;
    let mut p1 = Input {
        stick: [(t * 0.05).sin(), (t * 0.05).cos()],
        sprint: frame % 300 > 200,
        jump: frame % 97 < 3,
        camera_yaw: t * 0.01,
        ..Default::default()
    };
    if tweak == Some(frame) {
        // Let go of the stick for one frame.
        p1.stick = [0.0, 0.0];
    }
    let p2 = Input {
        stick: [0.0, if frame % 200 < 150 { 1.0 } else { 0.0 }],
        walk: frame % 400 < 100,
        camera_yaw: -(t * 0.003),
        jump: frame % 151 == 0,
        ..Default::default()
    };
    let mut m = FrameInputs::new();
    m.insert(0, PlayerInput { sim: p1, interact: false });
    m.insert(1, PlayerInput { sim: p2, interact: frame % 120 < 2 });
    m
}

fn session() -> Host<NoStoryData, ScriptedHost> {
    let story = Story::new_game(NoStoryData);
    let mut h = Host::new(Sim::new(HashMap::new()), Some(story), ScriptedHost::default(), Box::new(Flat));
    assert_eq!(h.join(ProfileId(1), Control::Local, [0.0, 0.0, 0.0], 0.0), Some(0));
    assert_eq!(h.join(ProfileId(2), Control::Remote, [300.0, 0.0, 0.0], 1.0), Some(1));
    h
}

fn run(frames: u64, tweak: Option<u64>) -> Vec<u64> {
    let mut h = session();
    (0..frames)
        .map(|f| {
            h.tick(&inputs(f, tweak)).unwrap();
            h.state_hash()
        })
        .collect()
}

#[test]
fn same_inputs_same_states() {
    let a = run(900, None);
    let b = run(900, None);
    assert_eq!(a, b);
    // The state really changes over the run.
    let distinct: std::collections::HashSet<_> = a.iter().collect();
    assert!(distinct.len() > 800, "{} distinct states", distinct.len());
}

#[test]
fn one_changed_input_diverges_from_its_frame() {
    let a = run(600, None);
    // Frame index 450 is on the ground, running, between jumps.
    let b = run(600, Some(450));
    assert_eq!(a[..450], b[..450]);
    assert!(a[450..].iter().zip(&b[450..]).all(|(x, y)| x != y));
}

#[test]
fn players_move_and_the_story_boots() {
    let mut h = session();
    let mut stage_loaded = false;
    for f in 0..300 {
        h.tick(&inputs(f, None)).unwrap();
        stage_loaded |= h.story.as_ref().unwrap().stage.stage_no == 380;
    }
    // New game names stage 380; the host asked the world to load it and
    // the story saw StageLoaded.
    assert!(stage_loaded);
    let p1 = h.sim.player(0).unwrap();
    let p2 = h.sim.player(1).unwrap();
    assert!(p1.position != [0.0, 0.0, 0.0]);
    assert!(p2.position != [300.0, 0.0, 0.0]);
}

#[test]
fn unknown_seat_is_refused() {
    let mut h = session();
    let mut i = inputs(0, None);
    i.insert(5, PlayerInput::default());
    assert!(h.tick(&i).is_err());
    assert_eq!(h.frame, 0);
}

#[test]
fn save_and_load_round_trip() {
    let mut h = session();
    for f in 0..200 {
        h.tick(&inputs(f, None)).unwrap();
    }
    let bytes = h.save().to_bytes();
    let file = ddhost::save::SaveFile::from_bytes(&bytes).unwrap();
    let mut g = session();
    g.load(&file, NoStoryData).unwrap();
    assert_eq!(g.story.as_ref().unwrap().world, h.story.as_ref().unwrap().world);
    for id in [0, 1] {
        assert_eq!(g.sim.player(id).unwrap().position, h.sim.player(id).unwrap().position);
    }
}

#[test]
fn leaving_releases_locks() {
    let mut h = session();
    h.locks.acquire(505, 1).unwrap();
    h.leave(1);
    assert!(h.locks.available(505, 0));
    assert_eq!(h.sim.players.len(), 1);
    assert_eq!(h.party.arisen().collect::<Vec<_>>(), vec![0]);
}
