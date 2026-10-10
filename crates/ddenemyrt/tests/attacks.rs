//! The Arisen's attacks from input against a standing goblin, headless:
//! ddsim plays the attack actions (light and heavy presses, the weapon's
//! transition list, the cancel slots of the motions' events, stamina), the
//! fight poses the Arisen from them and runs the hit pass. The goblin is
//! not stepped (it stands in its motion 0), so every hit is the Arisen's.
//! Skipped without the install.

use ddenemyrt::body::MotionId;
use ddenemyrt::combat::HitEvent;
use ddenemyrt::fight::{Fight, FightData, Kit, SHELL_OWNER_BASE};
use ddsim::{Attack, Ground, Input, ShotPhase, Sim, State};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

struct Flat;

impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

fn data() -> Option<Arc<FightData>> {
    static D: OnceLock<Option<Arc<FightData>>> = OnceLock::new();
    D.get_or_init(|| {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !native.is_dir() {
            eprintln!("SKIP: no install");
            return None;
        }
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        let exe = ddexe::ExeFile::next_to(&native).ok()?;
        Some(Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap()))
    })
    .clone()
}

/// One engine frame of the fight: what the Arisen did and what was hit.
#[derive(Debug, Clone)]
struct Frame {
    state: State,
    /// The motion frame after the step (volumes are placed from it).
    frame: f32,
    stamina: f32,
    /// Hits at the start of the frame (from the last frame's volumes).
    hits: Vec<HitEvent>,
}

struct Bout {
    sim: Sim,
    fight: Fight,
    log: Vec<Frame>,
}

impl Bout {
    /// The Arisen (`kit`) at the origin facing +z, a goblin `gap` cm ahead
    /// facing it.
    fn new(data: Arc<FightData>, kit: Kit, gap: f32) -> Self {
        let mut sim = Sim::new(HashMap::new());
        sim.add_player(0, [0.0; 3], 0.0);
        let mut fight = Fight::new(data);
        fight.add_arisen_as(0, kit);
        fight.equip(&mut sim, 0);
        fight.add_goblin([0.0, 0.0, gap], std::f32::consts::PI);
        fight.volumes(&sim);
        Bout {
            sim,
            fight,
            log: vec![],
        }
    }

    /// The host's order (docs/FRAME.md): hits, players, players' after
    /// step, push-out, pass C. The goblin is not stepped.
    fn frame(&mut self, input: Input) -> &Frame {
        let hits = self.fight.hits(1.0).to_vec();
        self.sim.step(1.0 / 30.0, &HashMap::from([(0, input)]), &Flat);
        self.fight.players_moved(&mut self.sim, &Flat, 1.0);
        self.fight.push_out(&mut self.sim);
        self.fight.volumes(&self.sim);
        let p = self.sim.player(0).unwrap();
        self.log.push(Frame {
            state: p.state,
            frame: p.frame,
            stamina: p.stamina,
            hits,
        });
        self.log.last().unwrap()
    }

    /// Steps until `done` holds after a frame (at most `n` frames); the
    /// frame count.
    fn until(&mut self, n: usize, done: impl Fn(&Frame) -> bool) -> usize {
        for k in 1..=n {
            if done(self.frame(Input::default())) {
                return k;
            }
        }
        panic!("not reached in {n} frames: {:?}", self.log.last());
    }

    /// Steps until the playing motion reaches `frame`.
    fn play_to(&mut self, frame: f32) {
        let s = self.log.last().unwrap().state;
        self.until(200, |f| f.state != s || f.frame >= frame);
    }

    fn press_light(&mut self) -> State {
        self.frame(light()).state
    }

    fn press_heavy(&mut self) -> State {
        self.frame(heavy()).state
    }

    fn hits(&self) -> Vec<(usize, &HitEvent)> {
        self.log
            .iter()
            .enumerate()
            .flat_map(|(i, f)| f.hits.iter().map(move |h| (i, h)))
            .collect()
    }

    /// The (state, motion frame) whose volumes made the hits of log frame `i`.
    fn source(&self, i: usize) -> (State, f32) {
        let f = &self.log[i - 1];
        (f.state, f.frame)
    }
}

fn light() -> Input {
    Input {
        light: true,
        ..Default::default()
    }
}

fn heavy() -> Input {
    Input {
        heavy: true,
        ..Default::default()
    }
}

fn attack(a: Attack) -> State {
    State::Attack { attack: a }
}

/// Whether `state` at motion `frame` switches on an attack group of the
/// kit's collision (the hit window of its motion events).
fn in_window(data: &FightData, kit: Kit, state: State, frame: f32) -> bool {
    let a = data.kit(kit);
    let id = state.motion_id();
    let m = MotionId::new((id >> 8) as u8, id & 0xff);
    a.body.activations(m, frame).iter().any(|act| {
        a.body.collision.iter().any(|set| {
            ddcombat::volumes::activated(&set.body, set.attacks.as_ref(), act)
                .is_some_and(|(g, at, _, _)| g.is_attack && at.is_some())
        })
    })
}

/// The frame of the log at which `state` first appears, and the motion
/// frame the state before it had reached.
fn started(b: &Bout, state: State) -> (usize, State, f32) {
    let i = b.log.iter().position(|f| f.state == state).unwrap();
    (i, b.log[i - 1].state, b.log[i - 1].frame)
}

#[test]
fn a_sword_light_combo_chains_in_its_windows_and_hits_the_goblin() {
    use Attack::*;
    let Some(data) = data() else { return };
    let mut b = Bout::new(data.clone(), Kit::Fighter, 120.0);
    assert_eq!(b.press_light(), attack(SwordLight1));
    // S1_1: slot 1 buffers on frames 8..13 and executes from 13
    // (m0001_at motion 0, group 0 bits 19 / 18)
    b.play_to(10.0);
    assert_eq!(b.press_light(), attack(SwordLight1));
    // S1_2: buffers 9..14, executes 14..19, bit 31 on 19 (motion 1)
    b.until(60, |f| f.state == attack(SwordLight2) && f.frame >= 11.0);
    assert_eq!(b.press_light(), attack(SwordLight2));
    b.until(60, |f| f.state == attack(SwordLight3));
    // S1_3 to its end (bit 30 on frame 49), then the wait
    b.until(80, |f| f.state == State::Wait);
    let (_, from, at) = started(&b, attack(SwordLight2));
    assert_eq!(from, attack(SwordLight1));
    assert!((12.0..13.0).contains(&at), "S1_2 after S1_1 frame {at}");
    let (_, from, at) = started(&b, attack(SwordLight3));
    assert_eq!(from, attack(SwordLight2));
    assert!((13.0..14.0).contains(&at), "S1_3 after S1_2 frame {at}");
    // each attack hits the goblin from inside its hit window
    let hits = b.hits();
    for (i, h) in &hits {
        let (state, frame) = b.source(*i);
        eprintln!(
            "hit at {i}: {state:?} frame {frame} total {} hits {}",
            h.total, h.hits
        );
        assert_eq!((h.attacker, h.victim), (0, 100));
        assert!(in_window(&data, Kit::Fighter, state, frame), "{state:?} {frame}");
    }
    for a in [SwordLight1, SwordLight2, SwordLight3] {
        assert!(
            hits.iter().any(|(i, _)| b.source(*i).0 == attack(a)),
            "no hit from {a:?}"
        );
    }
    // the first: COMBAT.md's worked example (attack 1 of m0001_00_lv1)
    assert_eq!(hits[0].1.total, 105.0);
    let gob = b.fight.goblins[0].fighter.victim.hp();
    assert_eq!(gob, 1000.0 - hits.iter().map(|h| h.1.total).sum::<f32>());
    // a level-1 Fighter's Iron Sword is not above its level: no stamina
    assert!(b.log.iter().all(|f| f.stamina == b.log[0].stamina));
}

#[test]
fn a_late_press_starts_the_sword_chain_over() {
    use Attack::*;
    let Some(data) = data() else { return };
    let mut b = Bout::new(data, Kit::Fighter, 120.0);
    b.press_light();
    // bit 31 closes S1_1's chain on frame 30 (check 22)
    b.play_to(31.0);
    assert_eq!(b.press_light(), attack(SwordLight1));
    assert!(b.log.last().unwrap().frame < 1.0);
}

#[test]
fn the_sword_heavy_attack_hits() {
    let Some(data) = data() else { return };
    let mut b = Bout::new(data.clone(), Kit::Fighter, 120.0);
    assert_eq!(b.press_heavy(), attack(Attack::SwordHeavy));
    b.until(80, |f| f.state == State::Wait);
    let hits = b.hits();
    assert!(!hits.is_empty());
    for (i, h) in &hits {
        let (state, frame) = b.source(*i);
        eprintln!("heavy hit at {i}: {state:?} frame {frame} total {}", h.total);
        assert_eq!(state, attack(Attack::SwordHeavy));
        assert!(in_window(&data, Kit::Fighter, state, frame));
    }
}

#[test]
fn the_staff_strikes_chain_and_hit() {
    use Attack::*;
    let Some(data) = data() else { return };
    let mut b = Bout::new(data.clone(), Kit::Mage, 120.0);
    assert_eq!(b.press_heavy(), attack(StaffStrike1));
    // Strike1: slot 1 buffers on 20..25, executes from 25 (m0007_at 35)
    b.play_to(22.0);
    assert_eq!(b.press_heavy(), attack(StaffStrike1));
    b.until(80, |f| f.state == attack(StaffStrike2));
    let (_, _, at) = started(&b, attack(StaffStrike2));
    assert!((24.0..25.0).contains(&at), "Strike2 after Strike1 frame {at}");
    b.until(100, |f| f.state == State::Wait);
    let hits = b.hits();
    for (i, h) in &hits {
        let (state, frame) = b.source(*i);
        eprintln!("strike hit at {i}: {state:?} frame {frame} total {}", h.total);
        assert!(in_window(&data, Kit::Mage, state, frame), "{state:?} {frame}");
    }
    for a in [StaffStrike1, StaffStrike2] {
        assert!(
            hits.iter().any(|(i, _)| b.source(*i).0 == attack(a)),
            "no hit from {a:?}"
        );
    }
}

#[test]
fn the_staff_light_attack_fires_shells_that_hit() {
    let Some(data) = data() else { return };
    let mut b = Bout::new(data, Kit::Mage, 400.0);
    assert_eq!(
        b.press_light(),
        State::Shot {
            phase: ShotPhase::Start
        }
    );
    let looping = State::Shot {
        phase: ShotPhase::Loop,
    };
    b.until(40, |f| f.state == looping && f.frame >= 4.0);
    // light again in the loop: a second volley
    b.press_light();
    b.until(120, |f| f.state == State::Wait);
    assert_eq!(b.sim.player(0).unwrap().volleys_fired, 2);
    let hits = b.hits();
    eprintln!("{hits:?}");
    assert_eq!(hits.len(), 2, "{hits:?}");
    for (_, h) in &hits {
        assert!(h.attacker >= SHELL_OWNER_BASE);
        assert_eq!(b.fight.shooters[&h.attacker], 0);
        assert_eq!(h.victim, 100);
        assert!(h.total > 0.0);
    }
    assert!(b.fight.shells.is_empty());
}

#[test]
fn attacks_spend_stamina_when_the_weapon_outranks_the_arisen() {
    use Attack::*;
    let Some(data) = data() else { return };
    let p = data.attack_param;
    // m_pl000_00: sword 0x1B8, staff 0x1BB, tiers from a gap of 15
    assert_eq!((p.sword_cost, p.staff_cost), (80.0, 30.0));
    let gap = p.tier_gap[0] as i32;
    for (kit, input, cost) in [
        (Kit::Fighter, light(), p.sword_cost),
        (Kit::Fighter, heavy(), p.sword_cost),
        (Kit::Mage, heavy(), p.staff_cost),
    ] {
        let mut b = Bout::new(data.clone(), kit, 400.0);
        b.sim.player_mut(0).unwrap().weapon_level_gap = gap;
        let before = b.sim.player(0).unwrap().stamina;
        b.frame(input);
        let after = b.log.last().unwrap().stamina;
        // the cost, then one frame of recovery (no motion suppresses it on
        // frame 0)
        let spent = before - after;
        eprintln!("{kit:?} {:?}: {spent}", b.log.last().unwrap().state);
        assert!((spent - cost).abs() < 2.0, "{kit:?}: {spent}");
    }
    // WandShot: one charge per volley
    let mut b = Bout::new(data.clone(), Kit::Mage, 4000.0);
    b.sim.player_mut(0).unwrap().weapon_level_gap = gap;
    b.press_light();
    let s0 = b.log.last().unwrap().stamina;
    b.until(40, |f| {
        f.state
            == State::Shot {
                phase: ShotPhase::Loop,
            }
            && f.frame >= 3.0
    });
    let k = b.log.len() - 1;
    let drop = b
        .log
        .windows(2)
        .skip(1)
        .map(|w| w[0].stamina - w[1].stamina)
        .fold(0.0, f32::max);
    assert!((drop - p.staff_cost).abs() < 2.0, "{drop}");
    assert!(b.log[k].stamina < s0);
    let _ = StaffStrike1;
}

#[test]
fn a_dagger_light_combo_chains_and_hits_the_goblin() {
    use Attack::*;
    let Some(data) = data() else { return };
    let mut b = Bout::new(data.clone(), Kit::Strider, 120.0);
    assert_eq!(b.press_light(), attack(DaggerLight1));
    // keep pressing: each press waits in slot 1 until it executes
    for _ in 0..200 {
        if b.log.last().unwrap().state == attack(DaggerLight3) {
            break;
        }
        b.press_light();
        b.frame(Input::default());
    }
    b.until(120, |f| f.state == State::Wait);
    for a in [DaggerLight1, DaggerLight2, DaggerLight3] {
        assert!(b.log.iter().any(|f| f.state == attack(a)), "no {a:?}");
    }
    let hits = b.hits();
    for (i, h) in &hits {
        let (state, frame) = b.source(*i);
        eprintln!("dagger hit at {i}: {state:?} frame {frame} total {}", h.total);
        assert_eq!((h.attacker, h.victim), (0, 100));
        assert!(in_window(&data, Kit::Strider, state, frame), "{state:?} {frame}");
    }
    for a in [DaggerLight1, DaggerLight2, DaggerLight3] {
        assert!(hits.iter().any(|(i, _)| b.source(*i).0 == attack(a)), "no hit from {a:?}");
    }
}

#[test]
fn the_dagger_heavy_attack_hits() {
    let Some(data) = data() else { return };
    let mut b = Bout::new(data.clone(), Kit::Strider, 120.0);
    assert_eq!(b.press_heavy(), attack(Attack::DaggerHeavy));
    b.until(120, |f| f.state == State::Wait);
    let hits = b.hits();
    assert!(!hits.is_empty());
    for (i, _) in &hits {
        let (state, frame) = b.source(*i);
        assert_eq!(state, attack(Attack::DaggerHeavy));
        assert!(in_window(&data, Kit::Strider, state, frame));
    }
}

#[test]
fn the_last_hit_goblin_shows_the_small_gauge_for_450_frames() {
    let Some(data) = data() else { return };
    let mut b = Bout::new(data, Kit::Fighter, 120.0);
    assert_eq!(b.fight.small_gauge, None);
    b.press_light();
    b.until(80, |f| f.state == State::Wait);
    let goblin = b.fight.goblins[0].fighter.owner;
    let (owner, left) = b.fight.small_gauge.expect("a hit shows the gauge");
    assert_eq!(owner, goblin);
    assert!(left > 0.0 && left <= ddenemyrt::fight::SMALL_GAUGE_FRAMES);
    // no more hits: it counts down and drops
    for _ in 0..500 {
        b.frame(Input::default());
    }
    assert_eq!(b.fight.small_gauge, None);
}
