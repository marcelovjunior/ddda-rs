//! M4's first slice, headless: one goblin against one Arisen in the host
//! (docs/tasks/m4-goblin-fight.md). The goblin is driven by its think
//! tables (ddai), plays its actions' motions with their root travel, and
//! hits with the volumes its motions switch on; the Arisen swings the
//! Fighter's sword attack 1 on a script. Hits, damage, pools and death
//! come from ddcombat, in the frame order of docs/FRAME.md. Skipped
//! without the install.

use ddcombat::damage::{enemy_attack, merge_frame, player_mitigate, AttackRow};
use ddenemyrt::body::MotionId;
use ddenemyrt::combat::HitEvent;
use ddenemyrt::fight::{Fight, FightData};
use ddenemyrt::goblin::action;
use ddhost::data::NoStoryData;
use ddhost::{Control, FrameInputs, Host, PlayerInput, ProfileId};
use ddsim::{Ground, Sim};
use ddstory::scripted::ScriptedHost;
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

/// What a scripted fight did.
struct Run {
    /// (host frame, event) of every hit.
    hits: Vec<(u64, HitEvent)>,
    /// The goblin's actions (unit frame, id).
    actions: Vec<(u32, i32)>,
    /// The goblin's motion and frame when its volumes were placed, per
    /// host frame (index = host frame - 1).
    goblin_motion: Vec<(MotionId, f32)>,
    /// The state hash after every frame.
    hashes: Vec<u64>,
    /// The goblin's HP at the end.
    goblin_hp: f32,
}

/// The Arisen stands at the origin facing +z; the goblin starts 6 m ahead,
/// facing it. With `swing`, the Arisen turns to the goblin and swings
/// sword attack 1 whenever it is free and the goblin is within 2.5 m.
fn fight(frames: u64, swing: bool) -> Run {
    let data = data().unwrap();
    let mut h = Host::new(
        Sim::new(HashMap::new()),
        None::<ddstory::Story<NoStoryData>>,
        ScriptedHost::default(),
        Box::new(Flat),
    );
    assert_eq!(h.join(ProfileId(1), Control::Local, [0.0; 3], 0.0), Some(0));
    let mut f = Fight::new(data);
    f.add_arisen(0);
    f.add_goblin([0.0, 0.0, 600.0], std::f32::consts::PI);
    h.fight = Some(f);
    let mut run = Run {
        hits: vec![],
        actions: vec![],
        goblin_motion: vec![],
        hashes: vec![],
        goblin_hp: 0.0,
    };
    for _ in 0..frames {
        let mut inputs = FrameInputs::new();
        inputs.insert(0, PlayerInput::default());
        let r = h.tick(&inputs).unwrap();
        let arisen = h.sim.player(0).unwrap().position;
        let f = h.fight.as_ref().unwrap();
        run.hits.extend(f.events.iter().map(|e| (r.frame, e.clone())));
        let g = &f.goblins[0];
        run.goblin_motion.push((g.motion, g.frame));
        let d = ((g.pos[0] - arisen[0]).powi(2) + (g.pos[2] - arisen[2]).powi(2)).sqrt();
        if swing && d < 250.0 && !g.dead() && f.arisen[&0].swing.is_none() {
            let yaw = (g.pos[0] - arisen[0]).atan2(g.pos[2] - arisen[2]);
            h.sim.player_mut(0).unwrap().yaw = yaw;
            h.fight.as_mut().unwrap().attack(0);
        }
        run.hashes.push(h.state_hash());
    }
    let g = &h.fight.as_ref().unwrap().goblins[0];
    run.actions = g.log.clone();
    run.goblin_hp = g.fighter.victim.hp();
    run
}

/// The goblin closes in and attacks a standing Arisen. Each of its hits
/// comes from volumes placed inside the hit window of the attack motion
/// playing (the frames its events switch an attack sequence on), and
/// deals the attack's value through the player defence curve.
#[test]
fn the_goblin_approaches_and_its_hits_land_in_the_windows() {
    let Some(data) = data() else { return };
    let r = fight(600, false);
    let a: Vec<i32> = r.actions.iter().map(|x| x.1).collect();
    // taunt, dash in, then an attack of the close-range table
    assert_eq!(&a[..2], [71, action::DASH_TRACE], "{a:?}");
    assert!(
        a.iter()
            .any(|&x| [action::N_ATTACK1, action::N_ATTACK2, action::N_ATTACK12].contains(&x)),
        "{a:?}"
    );
    let on_arisen: Vec<_> = r.hits.iter().filter(|(_, e)| e.victim == 0).collect();
    assert!(on_arisen.len() >= 3, "{:?}", r.hits);
    let body = &data.goblin.body;
    let set = &body.collision[0];
    for (frame, e) in &on_arisen {
        assert_eq!(e.attacker, 100);
        // the volumes of the frame before (pass C) hit at this frame's start
        let (m, f) = r.goblin_motion[*frame as usize - 2];
        let attack = body
            .activations(m, f)
            .into_iter()
            .filter_map(|act| ddcombat::volumes::activated(&set.body, set.attacks.as_ref(), &act))
            .find(|(g, a, _, _)| g.is_attack && a.is_some())
            .and_then(|(_, a, _, _)| a)
            .unwrap_or_else(|| panic!("hit at {frame} outside a window: motion {m:?} frame {f}"));
        let hit = enemy_attack(&data.goblin.attacker, &AttackRow::from_attack(attack));
        let mut merged = merge_frame(&[hit]).unwrap();
        player_mitigate(&mut merged, &data.arisen.defender, &data.tables.game);
        assert!((e.total - (merged.phys + merged.mag)).abs() < 1e-3, "{e:?}");
    }
    // The first is NAttack1 (e0100_at motion 1): its window is frames
    // 45..48 (sequence 2, attack 0: 0.8 x attack 250 = 200 strike), and a
    // level-1 Fighter (basic defence 80, no armour) takes
    // 200 x (1 - 80 / (80 + 100)) = 111.1 (COMBAT.md 8).
    let (frame, first) = on_arisen[0];
    let (m, f) = r.goblin_motion[*frame as usize - 2];
    assert_eq!(m, MotionId::new(1, 1));
    assert!((45.0..49.0).contains(&f), "{f}");
    assert!((first.total - 200.0 * 100.0 / 180.0).abs() < 1e-3, "{first:?}");
}

/// The Arisen's sword attack 1 deals the worked example's 105 per hit; the
/// goblin flinches at each and dies on the tenth (HP 1000).
#[test]
fn ten_sword_hits_kill_the_goblin() {
    if data().is_none() {
        return;
    }
    let r = fight(700, true);
    let on_goblin: Vec<&HitEvent> = r
        .hits
        .iter()
        .filter(|(_, e)| e.victim == 100)
        .map(|(_, e)| e)
        .collect();
    assert_eq!(on_goblin.len(), 10, "{:?}", r.hits);
    assert!(on_goblin
        .iter()
        .all(|e| e.total == 105.0 && e.attacker == 0 && e.reaction.stagger));
    assert!(on_goblin[..9].iter().all(|e| !e.killed));
    assert!(on_goblin[9].killed);
    assert!(r.goblin_hp <= 0.0);
    // every hit but the last made it flinch; the last one plays its death
    let a: Vec<i32> = r.actions.iter().map(|x| x.1).collect();
    let flinches = a
        .iter()
        .filter(|x| action::SHRINK_FRONT.contains(x) || **x == action::SHRINK_PLAIN_FRONT)
        .count();
    assert_eq!(flinches, 9, "{a:?}");
    assert_eq!(a.last(), Some(&action::DIE_FRONT), "{a:?}");
}

/// The same inputs give the same fight, frame by frame.
#[test]
fn the_fight_replays() {
    if data().is_none() {
        return;
    }
    let a = fight(500, true).hashes;
    let b = fight(500, true).hashes;
    assert_eq!(a, b);
}

/// A goblin with a quest FSM (q0002_goburin: one CtrlAction, action 1):
/// the FSM's action comes first (WaitAtck, served as a forced action), then
/// the think tables take over and fight.
#[test]
fn an_fsm_goblin_hands_over_to_its_think_tables() {
    let Some(data) = data() else { return };
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let mut h = Host::new(
        Sim::new(HashMap::new()),
        None::<ddstory::Story<NoStoryData>>,
        ScriptedHost::default(),
        Box::new(Flat),
    );
    h.join(ProfileId(1), Control::Local, [0.0; 3], 0.0);
    let mut f = Fight::new(data);
    f.add_arisen(0);
    f.add_goblin_with_fsm(
        &vfs,
        r"ai\fsm\enemy\st100\quest\q0002_goburin",
        [0.0, 0.0, 600.0],
        std::f32::consts::PI,
    )
    .unwrap();
    h.fight = Some(f);
    for _ in 0..300 {
        let mut inputs = FrameInputs::new();
        inputs.insert(0, PlayerInput::default());
        h.tick(&inputs).unwrap();
    }
    let g = &h.fight.as_ref().unwrap().goblins[0];
    assert!(g.fsm_error.is_none(), "{:?}", g.fsm_error);
    let a: Vec<i32> = g.log.iter().map(|x| x.1).collect();
    assert_eq!(&a[..3], [action::WAIT_ATCK, 71, action::DASH_TRACE], "{a:?}");
    assert!(a.contains(&action::N_ATTACK1), "{a:?}");
}
