//! The goblin's built-in think tables from the installed `DDDA.exe`, run
//! against synthetic targets. `DDDA_EXE` (default the exe next to the
//! maintainer's nativePC, `ddexe::locate`); skipped when absent.
//!
//! The unit side is synthetic: the 32 think flags, the slot measurements
//! and the answers to the world-facing callbacks (paths, target state) are
//! set by each test; the tables and the interpreter are the game's.
use ddai::thinktbl::em0100::{self, Driver};
use ddai::thinktbl::{Host, Slot, Tables};
use ddai::Error;
use std::path::PathBuf;
use std::sync::OnceLock;

struct Exe {
    tables: Tables,
    classes: Vec<String>,
}

fn exe() -> Option<&'static Exe> {
    static E: OnceLock<Option<Exe>> = OnceLock::new();
    E.get_or_init(|| {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC"));
        let path = std::env::var_os("DDDA_EXE")
            .map(PathBuf::from)
            .or_else(|| ddexe::locate(&native).ok());
        let Some(path) = path.filter(|p| p.is_file()) else {
            eprintln!("SKIP think table tests: no DDDA.exe");
            return None;
        };
        let file = ddexe::ExeFile::open(path).unwrap();
        let img = file.image().unwrap();
        Some(Exe {
            tables: Tables::read(&img).unwrap(),
            classes: em0100::action_classes(&img).unwrap(),
        })
    })
    .as_ref()
}

/// Think flag bits (think +0x04 + n, unit uEm0100 +0x6154 + n).
const fn flag(n: u32) -> u32 {
    1 << n
}
/// The unit's alert state 2 (0x008d1280 writes +0x6156), the one that
/// takes the default table into combat table 1 (0x017ddbf8 row 1).
const ALERT2: u32 = flag(2);
/// Flags the melee rows of tables 0x017d8798 / 0x017cd1b8 / 0x017d0098
/// require (unit +0x6157, +0x615c, +0x6168; meaning not traced).
const MELEE: u32 = ALERT2 | flag(3) | flag(8) | flag(20);

/// The unit as the think sees it.
struct Unit {
    action: i32,
    /// Ids the world-facing checks answer true.
    yes: Vec<u32>,
    /// Constant percent roll for every random row.
    roll: u32,
    sets: Vec<u32>,
}

impl Unit {
    fn new(yes: &[u32], roll: u32) -> Self {
        Unit {
            action: 0,
            yes: yes.to_vec(),
            roll,
            sets: vec![],
        }
    }
}

impl Host for Unit {
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, Error> {
        Ok(em0100::check(id, arg, self.action).unwrap_or(self.yes.contains(&id)))
    }
    fn set(&mut self, id: u32, _arg: u32) -> Result<u32, Error> {
        self.sets.push(id);
        Ok(0)
    }
    fn rand(&mut self) -> u32 {
        ((f64::from(self.roll) + 0.5) / 100.0 * 4_294_967_296.0) as u32
    }
}

/// A target `dist` away at bearing `angle` degrees (0 ahead, positive to
/// the right of +z), `height` above.
fn target(dist: f32, angle: f32, height: f32) -> Slot {
    let a = angle.to_radians();
    Slot::measure([0.0; 3], 0.0, [dist * a.sin(), height, dist * a.cos()], 0.0, 1)
}

/// Runs `n` thinks, ticking `dt` frames after each; returns what each
/// think asked the unit to do.
fn run(d: &mut Driver, u: &mut Unit, t: &Tables, n: usize, dt: f32) -> Vec<Option<i32>> {
    (0..n)
        .map(|_| {
            u.action = d.action;
            let r = d.think(t, u).unwrap();
            d.mgr.tick(dt);
            r
        })
        .collect()
}

fn goblin(flags: u32, slot0: Slot) -> Driver {
    let mut d = Driver::new(1.0);
    d.mgr.flags = flags;
    d.mgr.slots[0] = slot0;
    d
}

#[test]
fn tables_and_action_classes() {
    let Some(e) = exe() else { return };
    assert_eq!(e.tables.root(22), Some(0x017e_7798));
    assert_eq!(e.tables.root(0), Some(em0100::DEFAULT_TABLE));
    assert_eq!(e.tables.root(2), Some(em0100::DEFAULT_TABLE));
    assert_eq!(e.tables.root(1), Some(0x017d_b5d8));
    assert!(e.tables.len() > 1000, "{} records", e.tables.len());
    let c = |i: usize| e.classes[i].as_str();
    assert_eq!(c(0), "cEm0100ActWait");
    assert_eq!(c(13), "cEm0100ActDashTrace");
    assert_eq!(
        (c(18), c(19), c(20), c(21)),
        (
            "cEm0100ActTurn90",
            "cEm0100ActTurn180",
            "cEm0100ActTurn90",
            "cEm0100ActTurn180"
        )
    );
    assert_eq!((c(71), c(72)), ("cEm0100ActFindFinger", "cEm0100ActThreatHowl"));
    assert_eq!(
        (c(101), c(102), c(104)),
        ("cEm0100ActNAttack1", "cEm0100ActNAttack2", "cEm0100ActNAttack12")
    );
    assert_eq!((c(108), c(112)), ("cEm0100ActJumpAttack", "cEm0100ActDwnAtKick"));
    assert_eq!(c(236), "cEmGoblinAidMagic");
}

/// Table 22 (0x017e7798), the gate goblin's "dash" state: run to the escape
/// point in slot 1 (SetEscapePos). 1017 true / 1016 false keep it on its
/// own rows (rows 5 and 6 jump to 0x017c7358 otherwise).
fn escape(point: Slot, action: i32) -> (Driver, Unit) {
    let e = exe().unwrap();
    let mut d = Driver::new(1.0);
    d.mgr.request_table(e.tables.root(22).unwrap());
    d.mgr.slots[1] = point;
    d.action = action;
    (d, Unit::new(&[1017], 0))
}

#[test]
fn table22_dashes_to_the_escape_point() {
    let Some(e) = exe() else { return };
    // Ahead within +-100 degrees: row 7, 13 cEm0100ActDashTrace.
    for a in [0.0, 60.0, -99.0] {
        let (mut d, mut u) = escape(target(800.0, a, 0.0), 0);
        assert_eq!(run(&mut d, &mut u, &e.tables, 1, 1.0), [Some(13)], "angle {a}");
    }
    // Otherwise 0x017c6e58 turns: 45..125 -> 20, -125..-45 -> 18,
    // 125..180 -> 21, -180..-125 -> 19.
    for (a, want) in [(110.0, 20), (-110.0, 18), (170.0, 21), (-170.0, 19)] {
        let (mut d, mut u) = escape(target(800.0, a, 0.0), 0);
        assert_eq!(run(&mut d, &mut u, &e.tables, 1, 1.0), [Some(want)], "angle {a}");
    }
    // Moving away from it: stop first (0x017c6e58 rows 0..2, callbacks 4,
    // 5, 6 of 0x008ea7e0): Walk -> 7 WalkEnd, Run -> 10 RunEnd, Dash -> 15.
    for (now, want) in [(5, 7), (8, 10), (12, 15)] {
        let (mut d, mut u) = escape(target(800.0, 170.0, 0.0), now);
        assert_eq!(
            run(&mut d, &mut u, &e.tables, 1, 1.0),
            [Some(want)],
            "action {now}"
        );
    }
}

#[test]
fn table22_path_query_every_30_frames() {
    let Some(e) = exe() else { return };
    // Row 1 asks the unit for a path (set 1002) when timer 6 is free and
    // restarts it at 30; row 4 sets 1003 on every pass.
    let (mut d, mut u) = escape(target(800.0, 0.0, 0.0), 0);
    let count = |u: &Unit, id| u.sets.iter().filter(|&&s| s == id).count();
    run(&mut d, &mut u, &e.tables, 1, 0.0);
    run(&mut d, &mut u, &e.tables, 1, 30.0);
    assert_eq!((count(&u, 1002), count(&u, 1003)), (1, 2));
    // 30 frames later the timer reads 0, which still counts as running.
    run(&mut d, &mut u, &e.tables, 1, 0.5);
    assert_eq!(count(&u, 1002), 1);
    run(&mut d, &mut u, &e.tables, 1, 0.0);
    assert_eq!(count(&u, 1002), 2, "free once the timer goes below 0");
}

#[test]
fn combat_opens_with_a_taunt() {
    let Some(e) = exe() else { return };
    // Default table -> combat table 1 -> 0x017dac78 -> 0x017da138: a 50/50
    // random block of 71 FindFinger / 72 ThreatHowl, two table switches
    // (-6, no action) before it.
    for (roll, taunt) in [(10, 71), (49, 71), (50, 72), (90, 72)] {
        let mut d = goblin(MELEE, target(100.0, 0.0, 0.0));
        let mut u = Unit::new(&[], roll);
        assert_eq!(
            run(&mut d, &mut u, &e.tables, 3, 10.0),
            [None, None, Some(taunt)],
            "roll {roll}"
        );
    }
}

#[test]
fn close_range_attack_choice() {
    let Some(e) = exe() else { return };
    // Under 300 (0x017d9058 row 5) with the melee flags: 0x017cd1b8 ->
    // 0x017ccf38 -> 0x017cccb8: 30% 104 NAttack12, else 50/50 101 NAttack1 /
    // 102 NAttack2.
    for dist in [100.0, 250.0] {
        for (roll, want) in [(10, 104), (29, 104), (30, 101), (49, 101), (50, 102), (99, 102)] {
            let mut d = goblin(MELEE, target(dist, 0.0, 0.0));
            let mut u = Unit::new(&[], roll);
            let r = run(&mut d, &mut u, &e.tables, 5, 10.0);
            assert_eq!(r[4], Some(want), "dist {dist} roll {roll}: {r:?}");
        }
    }
}

#[test]
fn mid_range_jump_attack_or_approach() {
    let Some(e) = exe() else { return };
    // 350..450 ahead (+-15 deg): 0x017d0098 row 3, 40% 108 JumpAttack.
    let mut d = goblin(MELEE, target(400.0, 0.0, 0.0));
    let mut u = Unit::new(&[1017], 10);
    assert_eq!(run(&mut d, &mut u, &e.tables, 5, 10.0)[4], Some(108));
    // The 60% case and anything farther approach, given a path (1017):
    // 0x017cb9f8 row 5 dashes (13, table 0x017ca878) under 2000, row 6
    // runs (9, table 0x017cb138) beyond. One more think after the table
    // switch.
    for (dist, roll, want) in [
        (400.0, 40, 13),
        (1000.0, 10, 13),
        (1999.0, 90, 13),
        (2000.0, 10, 9),
        (5000.0, 90, 9),
    ] {
        let mut d = goblin(MELEE, target(dist, 0.0, 0.0));
        let mut u = Unit::new(&[1017], roll);
        let r = run(&mut d, &mut u, &e.tables, 6, 10.0);
        assert_eq!(&r[3..], [None, None, Some(want)], "dist {dist}: {r:?}");
        assert!(
            !r.iter().flatten().any(|&a| (101..=112).contains(&a)),
            "no attack at {dist}"
        );
    }
}

#[test]
fn long_run_stumbles_after_240_frames() {
    let Some(e) = exe() else { return };
    // 0x017cb138: while the unit runs (callback 0: action 9) timers 3 and 4
    // count up (command 0x4a); row 4 turns 240 frames on timer 4 into 11
    // RunStumble (cEm0100ActRunStumble).
    let mut d = goblin(MELEE, target(5000.0, 0.0, 0.0));
    let mut u = Unit::new(&[1017], 50);
    let r = run(&mut d, &mut u, &e.tables, 40, 10.0);
    let run_at = r.iter().position(|a| *a == Some(9)).unwrap();
    let stumble_at = r.iter().position(|a| *a == Some(11)).unwrap();
    assert_eq!(e.classes[11], "cEm0100ActRunStumble");
    // Timer 4 starts on the think after the run is requested (row 1 starts
    // it while the action is not yet 9) and counts 10 per think.
    let frames = (stumble_at - run_at) as f32 * 10.0;
    assert!((240.0..=260.0).contains(&frames), "{r:?}");
}

#[test]
fn turns_to_face_the_target_first() {
    let Some(e) = exe() else { return };
    // 0x017dac78 rows 4..7 come before the taunt and the attacks. They pick
    // 18..21, but combat table 1 row 12 has set think state bit 2, so the
    // unit's mode (vf108 0x008d1240) is 2 and 0x008d0fb0 moves the turns to
    // 26..29 (cEm0100ActTurn90 / Turn180 again).
    for (a, want) in [(90.0, 28), (-100.0, 26), (170.0, 29), (-170.0, 27)] {
        let mut d = goblin(MELEE, target(100.0, a, 0.0));
        let mut u = Unit::new(&[], 10);
        let r = run(&mut d, &mut u, &e.tables, 3, 10.0);
        assert_eq!(r, [None, None, Some(want)], "angle {a}");
        assert_eq!(d.mgr.trace.last().unwrap().1, want - 8, "the table's own pick");
        assert_eq!(em0100::mode(d.mgr.state, false), 2);
    }
    let names: Vec<&str> = (26..30).map(|i| e.classes[i].as_str()).collect();
    assert_eq!(
        names,
        [
            "cEm0100ActTurn90",
            "cEm0100ActTurn180",
            "cEm0100ActTurn90",
            "cEm0100ActTurn180"
        ]
    );
}

#[test]
fn downed_kick_cooldown_holds_900_frames() {
    let Some(e) = exe() else { return };
    // 0x017d8798 row 0 (flags 3 and 21) calls 0x017cbef8: with flag 29 the
    // kick (112 DwnAtKick, callback 16 true) checks and restarts timer 13
    // at 900 frames; without flag 29 row 1 kicks with no timer.
    let kicks = |flags: u32| {
        let mut d = goblin(flags, target(100.0, 0.0, 0.0));
        let mut u = Unit::new(&[16], 40);
        let r = run(&mut d, &mut u, &e.tables, 30, 100.0);
        r.iter()
            .enumerate()
            .filter(|(_, a)| **a == Some(112))
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let base = ALERT2 | flag(3) | flag(21);
    let with_timer = kicks(base | flag(29));
    // First kick at think 4; timer 13 is set to 900 there and ticked 100
    // per think: 0 (still running) at think 13, free from think 14. After a
    // kick the thinks alternate between the combat table (odd thinks) and
    // its -6 switch to 0x017cb9f8 (even thinks, Wait), so the next kick
    // lands on think 15, not 14.
    assert_eq!(with_timer, [4, 15, 26], "{with_timer:?}");
    let without = kicks(base);
    assert_eq!(without, (4..30).collect::<Vec<_>>());
}
