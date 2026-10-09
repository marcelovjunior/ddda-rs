//! The Arisen's scripted sword attack against a standing goblin, posed
//! from the installed skeletons and motions. Skipped without the install.

use ddcombat::damage::{Adjust, CollisionRates, EnemyAttacker, EnemyDefender, RegionPool, Resist};
use ddcombat::volumes::HitHistory;
use ddenemyrt::arisen::{self, ArisenData, Swing, SWORD_ATTACK_1};
use ddenemyrt::body::{load, Body, MotionId};
use ddenemyrt::combat::{hit_pass, Attacker, Fighter, Tables, Victim};

fn vfs() -> Option<ddvfs::Vfs> {
    let p = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !p.is_dir() {
        eprintln!("SKIP: no install");
        return None;
    }
    Some(ddvfs::Vfs::open(p).unwrap())
}

/// A goblin body standing in `e0100_co` motion 0 (bank 0 here), with its
/// region pools and stats from `em0100`.
fn goblin(vfs: &ddvfs::Vfs) -> (Body, Victim, EnemyAttacker) {
    let body = Body {
        skeleton: load::skeleton(vfs, r"model\em\e01\e0100\e0100").unwrap(),
        banks: vec![Some(load::bank(vfs, r"motion\em\e01\e0100_co\e0100_co").unwrap())],
        collision: vec![load::collision(
            vfs,
            r"collision\em\e01\e0100\e0100",
            Some(r"collision\em\e01\e0100\e0100_00"),
        )
        .unwrap()],
        scale: 1.0,
    };
    let gob = ddenemy::EnemyData::load(vfs, ddenemy::EnemyId::parse("em0100").unwrap()).unwrap();
    let stats = &gob.common.as_ref().unwrap().1;
    let rst = gob.regions.as_ref().unwrap();
    let region = rst.regions.iter().find(|r| r.kind == 0).unwrap();
    let res = |k: &str| stats.resistances[k];
    let resist = Resist {
        element: [res("耐炎"), res("耐氷"), res("耐雷"), res("耐聖"), res("耐魔")],
        slash: res("耐斬"),
        strike: res("耐打"),
    };
    let regions = region
        .elements
        .iter()
        .map(|e| {
            let mut r = RegionPool::from_element(
                e.id,
                e.hp_max,
                e.dp_max,
                e.dp_speed,
                e.bp_max,
                e.bp_speed,
                e.damage_adjustment,
                e.hit_stop_adjustment,
                e.attributes,
                e.surface,
                e.dp_reset_timer_max,
                e.bp_reset_timer_max,
            );
            r.resist = resist;
            r
        })
        .collect();
    let victim = Victim::Enemy {
        defender: EnemyDefender::new(stats.defense, stats.magic_defense),
        regions,
        rates: CollisionRates::default(),
    };
    (body, victim, EnemyAttacker::new(stats.attack, stats.magic_attack))
}

#[test]
fn sword_attack_1_hits_a_goblin_once_for_105() {
    let Some(vfs) = vfs() else { return };
    let a = ArisenData::fighter(&vfs).unwrap();
    let (gb, victim, gatt) = goblin(&vfs);
    let tables = Tables {
        game: Adjust::parse(&load::read(&vfs, r"param\game", "rAdjustParam").unwrap()).unwrap(),
        dlc: Adjust::parse(&load::read(&vfs, r"param\dlc", "rAdjustParam").unwrap()).unwrap(),
    };
    // The goblin faces the Arisen 120 cm ahead of it (+z).
    let gob_pos = [0.0, 0.0, 120.0];
    let gob_yaw = std::f32::consts::PI;
    let mut fighters = vec![
        Fighter {
            owner: 0,
            attacker: Attacker::Player(a.attacker),
            victim: Victim::Player {
                defender: a.defender,
                hp: a.hp_max,
                hp_max: a.hp_max,
            },
            attacks: vec![],
            hurts: vec![],
            history: HitHistory::default(),
        },
        Fighter {
            owner: 100,
            attacker: Attacker::Enemy(gatt),
            victim,
            attacks: vec![],
            hurts: vec![],
            history: HitHistory::default(),
        },
    ];
    let mut swing = Swing {
        motion: SWORD_ATTACK_1,
        frame: 0.0,
    };
    let mut pos = [0.0f32; 3];
    let mut hits = vec![];
    for frame in 0..60 {
        // hits from last frame's volumes
        for e in hit_pass(&mut fighters, &tables, 1.0, &mut || 0.5, &|_| true) {
            hits.push((frame, swing.frame, e));
        }
        // the swing advances one motion frame and moves the Arisen forward
        if let Some(d) = arisen::advance(&a.body, &mut swing, 1.0) {
            pos[2] += d[2];
            pos[0] += d[0];
        }
        // volumes for the next frame
        let ap = a.body.pose(swing.motion, swing.frame, pos, 0.0);
        let acts = a.body.activations(swing.motion, swing.frame);
        (fighters[0].attacks, fighters[0].hurts) = a.body.volumes(&acts, &ap, 0);
        let gm = MotionId::new(0, 0);
        let gp = gb.pose(gm, frame as f32, gob_pos, gob_yaw);
        let gacts = gb.activations(gm, frame as f32);
        (fighters[1].attacks, fighters[1].hurts) = gb.volumes(&gacts, &gp, 100);
    }
    eprintln!("{hits:?}");
    assert_eq!(hits.len(), 1, "{hits:?}");
    let (_, motion_frame, e) = &hits[0];
    assert_eq!((e.attacker, e.victim, e.total), (0, 100, 105.0));
    // the volumes placed at the end of a frame (motion frame 6, the first
    // of the window 6..10) hit at the start of the next frame
    assert_eq!(*motion_frame, 6.0);
    assert!(e.reaction.stagger);
    assert_eq!(fighters[1].victim.hp(), 895.0);
}

#[test]
fn a_level_1_mage_loads() {
    let Some(vfs) = vfs() else { return };
    let m = ArisenData::mage(&vfs).unwrap();
    assert!(m.body.motion(MotionId::new(arisen::WEAPON_AT_BANK, 35)).is_some());
    eprintln!("mage attack {:?} hp {}", m.attacker.base_attack, m.hp_max);
}
