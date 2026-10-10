//! Installed S26 normal Input -> source bow model pose -> Fight hit pass.
//! Stationary upper-pose provider (head4263); no live-original comparison.
use ddcombat::volumes::{Pose, Volume, contacts};
use ddenemyrt::{
    body::Posed,
    combat::{HitEvent, Victim},
    fight::{BowContext, Fight, FightData, Kit},
};
use ddsim::{BowPhase, Ground, Input, Sim, Weapon};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> {
        (below >= 0.0).then_some(0.0)
    }
}
fn data() -> Option<Arc<FightData>> {
    static DATA: OnceLock<Option<Arc<FightData>>> = OnceLock::new();
    DATA.get_or_init(|| {
        let path = std::env::var_os("DDDA_NATIVE_PC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
        if !path.is_dir() {
            eprintln!("SKIP: no install");
            return None;
        }
        let vfs = ddvfs::Vfs::open(&path).unwrap();
        let exe = ddexe::ExeFile::next_to(&path).unwrap();
        Some(Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap()))
    })
    .clone()
}
fn bout(data: Arc<FightData>, seats: &[u8]) -> (Sim, Fight) {
    let mut s = Sim::new(HashMap::new());
    let mut f = Fight::new(data);
    for &id in seats {
        s.add_player(id, [0.0; 3], 0.0);
        f.add_arisen_as(id, Kit::Strider);
        f.equip(&mut s, id);
    }
    f.add_goblin([0.0, 0.0, 1000.0], std::f32::consts::PI);
    f.volumes(&s);
    (s, f)
}
fn held(seats: &[u8], light: bool) -> HashMap<u8, Input> {
    seats
        .iter()
        .map(|&id| {
            (
                id,
                Input {
                    bow: true,
                    light,
                    ..Default::default()
                },
            )
        })
        .collect()
}
fn ready(s: &mut Sim, f: &mut Fight, seats: &[u8]) {
    for _ in 0..120 {
        s.step(1.0 / 30.0, &held(seats, false), &Flat);
        f.players_moved(s, &Flat, 1.0);
        f.volumes(s);
        if seats.iter().all(|&id| {
            s.player(id)
                .unwrap()
                .bow
                .is_some_and(|b| b.phase == BowPhase::Ready)
        }) {
            return;
        }
    }
    panic!("installed draw did not become Ready");
}
fn target(f: &Fight) -> [f32; 3] {
    match f.goblins[0].fighter.hurts[0].volume {
        Volume::Sphere { c, .. } => c,
        _ => panic!("first goblin hurt is not sphere"),
    }
}
fn context(f: &Fight, pose: Posed, target: [f32; 3]) -> BowContext {
    BowContext {
        pose,
        target,
        random: [0x80000000; 16].into(),
        augments: [u32::MAX; 6],
        secondary_attacker: f.data.bow.starting_attacker,
    }
}
fn expected(f: &Fight, arrow: &ddenemyrt::arrow::Arrow, secondary: bool) -> Option<f32> {
    let g = &f.goblins[0].fighter;
    let volumes = arrow.volumes(&f.data.arrow);
    let contacts = contacts(&volumes, &g.hurts);
    let c = contacts.first()?;
    let row = &volumes[c.attack].row;
    let mut p = if secondary {
        f.data.bow.starting_attacker
    } else {
        f.data.strider.attacker
    };
    let (phys, mag) = f.arisen[&(arrow.shooter as u8)].fighter.statuses.attack_muls();
    p.phys_status_mul *= phys;
    p.mag_status_mul *= mag;
    let value = ddcombat::damage::player_attack(&p, row, 0.5, &f.data.tables.dlc);
    let merged = ddcombat::damage::merge_frame(&[value]).unwrap();
    let Victim::Enemy {
        defender,
        regions,
        rates,
    } = &g.victim
    else {
        panic!()
    };
    let mut regions = regions.clone();
    let at = regions
        .iter()
        .position(|r| r.id == g.hurts[c.hurt].region_id)
        .unwrap_or(0);
    Some(
        ddcombat::damage::resolve_enemy_hit(
            merged,
            defender,
            &mut regions,
            at,
            rates,
            &f.data.tables.game,
            1.0,
        )
        .total,
    )
}

#[test]
fn normal_input_fires_from_actual_bow_joint_and_hits_ten_metre_goblin_with_secondary_damage() {
    let Some(data) = data() else { return };
    let (mut s, mut f) = bout(data, &[3]);
    ready(&mut s, &mut f, &[3]);
    let pose = f.bow_pose(&s, 3).unwrap();
    let origin = pose.joint_matrix(0).unwrap()[3];
    assert!((origin[0] * origin[0] + origin[1] * origin[1] + origin[2] * origin[2]).sqrt() > 10.0);
    let aim = target(&f);
    f.bow_contexts.insert(3, context(&f, pose, aim));
    let before = f.goblins[0].fighter.victim.hp();
    let mut hit = None;
    for frame in 0..40 {
        let predicted = f.arrows.first().and_then(|a| expected(&f, a, true));
        let dagger = f.arrows.first().and_then(|a| expected(&f, a, false));
        let events = f.hits(1.0).to_vec();
        if let Some(event) = events.iter().find(|e| e.victim == f.goblins[0].fighter.owner) {
            let expected = predicted.expect("arrow contact before hit pass");
            assert!((event.total - expected).abs() < 1e-4, "{event:?} vs {expected}");
            assert!(
                (expected - dagger.unwrap()).abs() > 0.1,
                "test must distinguish dagger stats"
            );
            assert_eq!(f.shooters[&event.attacker], 3);
            assert_eq!(event.hits, 1);
            eprintln!("source bow hit frame{frame}: damage{}", event.total);
            hit = Some(event.clone());
            break;
        }
        s.step(1.0 / 30.0, &held(&[3], true), &Flat);
        let pose = f.bow_pose(&s, 3).unwrap();
        let joint = pose.joint_matrix(0).unwrap()[3];
        f.bow_contexts.get_mut(&3).unwrap().pose = pose;
        f.players_moved(&mut s, &Flat, 1.0);
        f.volumes(&s);
        if frame == 0 {
            let a = &f.arrows[0];
            assert_eq!([joint[0], joint[1], joint[2]], a.previous_position);
            assert_eq!(f.bow_contexts[&3].random.len(), 12); // authored2 + accuracy2
        }
    }
    let hit = hit.expect("no 10m normal-input bow hit");
    assert!((f.goblins[0].fighter.victim.hp() - (before - hit.total)).abs() < 1e-3);
    assert!(f.arrows.is_empty());
    assert!(f.bow_errors.is_empty());
    assert_eq!(s.player(3).unwrap().weapon, Weapon::Dagger);
}

#[test]
fn independent_seats_can_hit_and_miss_without_nearest_enemy_retargeting() {
    let Some(data) = data() else { return };
    let (mut s, mut f) = bout(data, &[3, 7]);
    ready(&mut s, &mut f, &[3, 7]);
    let aim = target(&f);
    for seat in [3, 7] {
        let mut t = aim;
        if seat == 7 {
            t[0] += 1000.0;
        }
        let pose = f.bow_pose(&s, seat).unwrap();
        f.bow_contexts.insert(seat, context(&f, pose, t));
    }
    let mut hits: Vec<HitEvent> = Vec::new();
    for _ in 0..35 {
        hits.extend(f.hits(1.0).iter().cloned());
        s.step(1.0 / 30.0, &held(&[3, 7], true), &Flat);
        for seat in [3, 7] {
            let pose = f.bow_pose(&s, seat).unwrap();
            f.bow_contexts.get_mut(&seat).unwrap().pose = pose;
        }
        f.players_moved(&mut s, &Flat, 1.0);
        f.volumes(&s);
    }
    assert_eq!(hits.len(), 1);
    assert_eq!(f.shooters[&hits[0].attacker], 3);
    assert_eq!(f.arrows.len(), 1);
    assert_eq!(f.arrows[0].shooter, 7);
    assert_eq!(f.bow_contexts[&3].random.len(), 12);
    assert_eq!(f.bow_contexts[&7].random.len(), 12);
    assert!(f.digest().contains("arrow"));
    let digest = f.digest();
    f.arrows[0].previous_position[0] += 1.0;
    assert_ne!(digest, f.digest()); // previous endpoint affects next hit
    let digest = f.digest();
    f.bow_contexts.get_mut(&7).unwrap().random.pop_front();
    assert_ne!(digest, f.digest()); // next shot RNG is authoritative state
}

#[test]
fn missing_context_or_rng_is_explicit_and_does_not_spawn_a_guessed_arrow() {
    let Some(data) = data() else { return };
    let (mut s, mut f) = bout(data, &[3, 7]);
    ready(&mut s, &mut f, &[3, 7]);
    let mut ctx = context(&f, f.bow_pose(&s, 7).unwrap(), target(&f));
    ctx.random.clear();
    f.bow_contexts.insert(7, ctx);
    s.step(1.0 / 30.0, &held(&[3, 7], true), &Flat);
    f.players_moved(&mut s, &Flat, 1.0);
    assert!(f.arrows.is_empty());
    assert!(s.player(3).unwrap().bow_shots.is_empty());
    assert!(f.bow_errors[&3].contains("context"));
    assert!(f.bow_errors[&7].contains("RNG"));
    assert_eq!(
        s.player(3).unwrap().bow.unwrap().unsupported,
        Some(ddsim::BowUnsupported::Shot)
    );
}

#[test]
fn arrow_hit_reads_current_shooter_attack_status_instead_of_clearing_it() {
    let Some(data) = data() else { return };
    let (mut s, mut f) = bout(data, &[3]);
    ready(&mut s, &mut f, &[3]);
    let pose = f.bow_pose(&s, 3).unwrap();
    f.bow_contexts.insert(3, context(&f, pose, target(&f)));
    s.step(1.0 / 30.0, &held(&[3], true), &Flat);
    f.bow_contexts.get_mut(&3).unwrap().pose = f.bow_pose(&s, 3).unwrap();
    f.players_moved(&mut s, &Flat, 1.0);
    f.volumes(&s);
    // Synthetic timed owner buff, added after firing; damage follows the
    // shooter's current status (88A910), not a launch snapshot/default.
    f.arisen
        .get_mut(&3)
        .unwrap()
        .fighter
        .statuses
        .works
        .push(ddcombat::cstatus::Work {
            kind: ddcombat::cstatus::kind::ATTACK_UP,
            category: 0,
            timer: 100.0,
            speed: [1.0; 2],
            param0: 2.0,
            param1: 0.0,
        });
    let mut hit = false;
    for _ in 0..20 {
        let predicted = f.arrows.first().and_then(|a| expected(&f, a, true));
        let events = f.hits(1.0).to_vec();
        if let Some(event) = events.first() {
            assert!((event.total - predicted.unwrap()).abs() < 1e-4);
            assert!(event.total > 42.72);
            hit = true;
            break;
        }
        s.step(1.0 / 30.0, &held(&[3], true), &Flat);
        f.bow_contexts.get_mut(&3).unwrap().pose = f.bow_pose(&s, 3).unwrap();
        f.players_moved(&mut s, &Flat, 1.0);
        f.volumes(&s);
    }
    assert!(hit);
}
