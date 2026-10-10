//! A main pawn with the Arisen against a goblin, headless and
//! deterministic: the pawn's think (ddpawnai) picks follow or its sword
//! goal, its virtual pad moves and attacks through ddsim like a player, and
//! the fight's hit pass applies its swings. Skipped without the install.

use ddenemyrt::fight::{Fight, FightData};
use ddpawnai::pawn::{Goal, Outcome, GOAL_FOLLOW, GOAL_SWORD};
use ddsim::{Ground, Input, Sim};
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

const ARISEN: u8 = 0;
const PAWN: u8 = 1;
/// Balanced values (no active inclination).
const NEUTRAL: [f32; 9] = [500.0; 9];

struct Bout {
    sim: Sim,
    fight: Fight,
    /// Hits by the pawn (frame, damage).
    pawn_hits: Vec<(usize, f32)>,
    digests: Vec<String>,
    frames: usize,
}

impl Bout {
    fn new(data: Arc<FightData>, pawn_at: [f32; 3], goblin_at: Option<[f32; 3]>, incl: [f32; 9]) -> Self {
        let mut sim = Sim::new(HashMap::new());
        sim.add_player(ARISEN, [0.0; 3], 0.0);
        let mut fight = Fight::new(data);
        fight.add_arisen(ARISEN);
        fight.equip(&mut sim, ARISEN);
        fight.add_pawn(&mut sim, PAWN, ARISEN, pawn_at, 0.0, incl, 7);
        if let Some(g) = goblin_at {
            fight.add_goblin(g, std::f32::consts::PI);
        }
        fight.volumes(&sim);
        Bout {
            sim,
            fight,
            pawn_hits: vec![],
            digests: vec![],
            frames: 0,
        }
    }

    /// One frame in the host's order (docs/FRAME.md); the Arisen stands
    /// unless `arisen` moves it.
    fn frame(&mut self, arisen: Input) {
        let hits = self.fight.hits(1.0).to_vec();
        for h in hits.iter().filter(|h| h.attacker == u32::from(PAWN)) {
            self.pawn_hits.push((self.frames, h.total));
        }
        let mut inputs = self.fight.pawn_inputs(&self.sim, 1.0);
        inputs.insert(ARISEN, arisen);
        self.sim.step(1.0 / 30.0, &inputs, &Flat);
        self.fight.players_moved(&mut self.sim, &Flat, 1.0);
        self.fight.enemies(&self.sim, &Flat, 1.0);
        self.fight.push_out(&mut self.sim, &Flat);
        self.fight.volumes(&self.sim);
        self.digests.push(self.fight.digest());
        self.frames += 1;
    }

    fn pawn(&self) -> &ddpawnai::pawn::Pawn {
        &self.fight.pawns[&PAWN].brain
    }

    fn pawn_pos(&self) -> [f32; 3] {
        self.sim.player(PAWN).unwrap().position
    }
}

fn dist_xz(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[test]
fn pawn_attacks_the_goblin() {
    let Some(data) = data() else { return };
    let mut b = Bout::new(data, [-200.0, 0.0, 0.0], Some([0.0, 0.0, 900.0]), NEUTRAL);
    for _ in 0..900 {
        b.frame(Input::default());
        if b.fight.goblins[0].dead() {
            break;
        }
    }
    let goals: Vec<u32> = b.pawn().history.iter().map(|h| h.0).collect();
    assert_eq!(goals.first(), Some(&GOAL_SWORD), "{goals:?}");
    assert!(
        !b.pawn_hits.is_empty(),
        "the pawn never hit: {:?}",
        b.pawn().history
    );
    // Every attack began with an approach into the light combo's range
    // (AIPlActParamSword record 0, Lv1: 0..150 cm).
    eprintln!(
        "pawn hits {} (first at frame {}), goals {:?}, goblin hp {:?}, unported codes {:?}",
        b.pawn_hits.len(),
        b.pawn_hits[0].0,
        &b.pawn().history[..b.pawn().history.len().min(12)],
        b.fight.goblins[0].fighter.victim.hp(),
        b.pawn().unported
    );
    let done = b
        .pawn()
        .history
        .iter()
        .filter(|h| h.1 == Some(Outcome::Done))
        .count();
    assert!(done > 0, "no combo finished: {:?}", b.pawn().history);
}

#[test]
fn pawn_follows_the_arisen_in_battle() {
    let Some(data) = data() else { return };
    // The pawn 25 m from the Arisen, a goblin near the Arisen: battle; the
    // follow entry (Wait_Follow-01, level 1) outranks the battle entry
    // (Enemy-04, level 4), so it first closes to the follow range's
    // minimum (mode 2: follow beyond 1500, stop inside 750).
    let mut b = Bout::new(data, [0.0, 0.0, -2500.0], Some([0.0, 0.0, 1200.0]), NEUTRAL);
    b.frame(Input::default());
    assert_eq!(b.pawn().history[0].0, GOAL_FOLLOW);
    assert!(matches!(b.pawn().goal, Some(Goal::Follow { min }) if min == 750.0));
    for _ in 0..300 {
        b.frame(Input::default());
        if b.pawn().history.len() > 1 {
            break;
        }
    }
    assert!(dist_xz(b.pawn_pos(), [0.0; 3]) <= 760.0, "{:?}", b.pawn_pos());
    assert_eq!(b.pawn().history[0].1, Some(Outcome::Done));
    assert_eq!(b.pawn().history[1].0, GOAL_SWORD);
}

#[test]
fn pawn_fight_is_deterministic() {
    let Some(data) = data() else { return };
    let run = || {
        let mut b = Bout::new(data.clone(), [-200.0, 0.0, 0.0], Some([0.0, 0.0, 900.0]), NEUTRAL);
        for _ in 0..300 {
            b.frame(Input::default());
        }
        b.digests
    };
    assert_eq!(run(), run());
}

#[test]
#[ignore]
fn debug_trace() {
    let Some(data) = data() else { return };
    let mut b = Bout::new(data, [-200.0, 0.0, 0.0], Some([0.0, 0.0, 900.0]), NEUTRAL);
    for _ in 0..400 {
        let before = format!("{:?}", b.fight.pawns[&PAWN].brain.goal);
        b.frame(Input::default());
        let p = b.sim.player(PAWN).unwrap();
        let g = &b.fight.goblins[0];
        eprintln!(
            "{:4} {:?} f{:.1} d{:.0} gob a{} | {}",
            b.frames,
            p.state,
            p.frame,
            dist_xz(p.position, g.pos),
            g.action,
            before
        );
    }
}

/// The inclination effect: Guardian at status 2 (value >= 750) adds 3 to
/// the battle follow entry (Wait_Follow-01 code 4, `+3[5/2]` in
/// `AI\PrioThink\cmc`), which drops it from level 1 to level 4, behind the
/// battle entry (Enemy-04 code 7: same level, lower group). A pawn 25 m from
/// its Arisen with a goblin next to it follows first when neutral and
/// fights first as a Guardian.
#[test]
fn guardian_fights_before_following() {
    let Some(data) = data() else { return };
    let first_goal = |incl: [f32; 9]| {
        let mut b = Bout::new(data.clone(), [0.0, 0.0, -2500.0], Some([0.0, 0.0, -2100.0]), incl);
        b.frame(Input::default());
        b.pawn().history[0].0
    };
    assert_eq!(first_goal(NEUTRAL), GOAL_FOLLOW);
    let mut guardian = NEUTRAL;
    guardian[ddpawnai::incl::GUARDIAN as usize] = 800.0;
    assert_eq!(first_goal(guardian), GOAL_SWORD);
}
