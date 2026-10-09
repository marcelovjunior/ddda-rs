//! Statuses in the goblin fight: a status applied to a goblin ticks with
//! the fight's frames and wears off after its `param\status\enemy` time.
//! Skipped without the install.

use ddcombat::cstatus::kind;
use ddenemyrt::fight::{Fight, FightData};
use std::sync::Arc;

fn data() -> Option<Arc<FightData>> {
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
}

#[test]
fn item_recovery_is_per_seat_and_does_not_revive() {
    use ddenemyrt::combat::Victim;
    let Some(data) = data() else { return };
    let mut fight = Fight::new(data.clone());
    fight.add_arisen(0);
    fight.add_arisen(1);
    for seat in [0, 1] {
        let fighter = &mut fight.arisen.get_mut(&seat).unwrap().fighter;
        assert!(fighter.statuses.add(kind::POISON, &data.tables.status_player));
        let Victim::Player { hp, .. } = &mut fighter.victim else { unreachable!() };
        *hp = 50.0;
    }
    let mask = 1u64 << kind::POISON;
    assert_eq!(fight.recover_item(0, 20.0, mask), Some((20.0, vec![kind::POISON])));
    assert_eq!(fight.arisen[&0].fighter.victim.hp(), 70.0);
    assert_eq!(fight.arisen[&1].fighter.victim.hp(), 50.0);
    assert!(fight.arisen[&1].fighter.statuses.has(kind::POISON));
    assert_eq!(fight.recover_item(255, 20.0, mask), None);
    let Victim::Player { hp, hp_max, .. } = &mut fight.arisen.get_mut(&0).unwrap().fighter.victim
        else { unreachable!() };
    *hp = *hp_max - 5.0;
    assert_eq!(fight.recover_item(0, 20.0, mask), Some((5.0, vec![])));
    let Victim::Player { hp, .. } = &mut fight.arisen.get_mut(&1).unwrap().fighter.victim
        else { unreachable!() };
    *hp = 0.0;
    assert_eq!(fight.recover_item(1, 20.0, mask), None);
    assert_eq!(fight.arisen[&1].fighter.victim.hp(), 0.0);
    assert!(fight.arisen[&1].fighter.statuses.has(kind::POISON));
}

#[test]
fn poison_ticks_and_wears_off_in_the_goblin_fight() {
    let Some(data) = data() else { return };
    let poison = *data.tables.status_enemy.get(kind::POISON).unwrap();
    assert_eq!((poison.timer, poison.param0, poison.param1), (180.0, 5.0, 1.0));
    let mut fight = Fight::new(data.clone());
    fight.add_arisen(0);
    fight.add_goblin([0.0, 0.0, 500.0], 0.0);
    let gob = fight.goblins[0].fighter.owner;
    assert!(fight.goblins[0].fighter.statuses.add(kind::POISON, &data.tables.status_enemy));
    let hp0 = fight.goblins[0].fighter.victim.hp();
    let mut bites = Vec::new();
    let mut ended_at = None;
    for frame in 0..5500u32 {
        fight.hits(1.0);
        for e in fight.status_events.iter().filter(|e| e.owner == gob) {
            if e.hp < 0.0 {
                bites.push(frame);
            }
            if e.ended.contains(&kind::POISON) {
                ended_at = Some(frame);
            }
        }
    }
    // 5 HP at onset, then every 31 frames (the interval counter must go
    // below zero, 0x00AAFE10), for 180 s x 30 frames
    assert_eq!(&bites[..3], &[0, 31, 62]);
    assert_eq!(ended_at, Some(5399));
    let hp = fight.goblins[0].fighter.victim.hp();
    assert_eq!(hp, hp0 - 5.0 * bites.len() as f32);
    assert!(!fight.goblins[0].fighter.statuses.has(kind::POISON));
    assert!(*bites.last().unwrap() < 5400);
}

#[test]
fn burning_and_attack_down_in_the_fight() {
    let Some(data) = data() else { return };
    let mut fight = Fight::new(data.clone());
    fight.add_arisen(0);
    fight.add_goblin([0.0, 0.0, 500.0], 0.0);
    let t = &data.tables.status_enemy;
    let g = &mut fight.goblins[0].fighter;
    assert!(g.statuses.add(kind::BURNING, t));
    assert!(g.statuses.add(kind::ATTACK_DOWN, t));
    let burn = *t.get(kind::BURNING).unwrap();
    let hp0 = g.victim.hp();
    for _ in 0..30 {
        fight.hits(1.0);
    }
    let hp = fight.goblins[0].fighter.victim.hp();
    assert!((hp0 - hp - burn.param0).abs() < 0.01, "{hp0} -> {hp}");
    let (p, _) = fight.goblins[0].fighter.statuses.attack_muls();
    assert_eq!(p, t.get(kind::ATTACK_DOWN).unwrap().param0);
    // burning lasts mTimer seconds
    for _ in 30..(burn.timer * 30.0) as u32 + 1 {
        fight.hits(1.0);
    }
    assert!(!fight.goblins[0].fighter.statuses.has(kind::BURNING));
}
