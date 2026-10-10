//! Installed shortbow parameters and actual motion-gated action phases.
use ddsim::{BowPhase, Ground, Input, Weapon};
use std::collections::HashMap;

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { (below >= 0.0).then_some(0.0) }
}

#[test]
fn installed_shortbow_draw_ready_release_and_secondary_slot() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() {
        assert!(std::env::var_os("DDDA_NATIVE_PC").is_none(), "explicit install absent");
        eprintln!("SKIP: DDDA install absent"); return;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let rows = ddhost::install::bow_params(&vfs).unwrap();
    assert_eq!(rows.len(), 30);
    let param = rows[0];
    assert_eq!((param.draw_upper, param.draw_lower, param.wait_upper, param.wait_lower), (0, 1, 6, 7));
    assert_eq!((param.release_upper, param.release_lower, param.fall), (4, 5, 68));
    let mut sim = ddhost::install::sim(&vfs).unwrap();
    sim.add_player(2, [0.0; 3], 0.0);
    sim.add_player(6, [200.0, 0.0, 0.0], 0.0);
    let player = sim.player_mut(2).unwrap();
    player.weapon = Weapon::Dagger;
    player.secondary_weapon = Weapon::Bow;
    let held = HashMap::from([(2, Input { bow: true, ..Default::default() })]);
    let mut phases = Vec::new();
    for _ in 0..200 {
        sim.step(1.0 / 30.0, &held, &Flat);
        let player = sim.player(2).unwrap();
        let bow = player.bow.unwrap();
        phases.push(bow.phase);
        assert_eq!(player.weapon, Weapon::Dagger);
        assert!(sim.player(6).unwrap().bow.is_none());
        if bow.phase == BowPhase::Ready { break; }
    }
    assert_eq!(phases[0], BowPhase::Draw);
    assert_eq!(*phases.last().unwrap(), BowPhase::Ready);
    let bow = sim.player(2).unwrap().bow.unwrap();
    assert_eq!((bow.upper.motion, bow.lower.motion), (0x906, 0x907));
    for _ in 0..200 {
        sim.step(1.0 / 30.0, &HashMap::new(), &Flat);
        let Some(bow) = sim.player(2).unwrap().bow else { break };
        assert_eq!(bow.phase, BowPhase::Released);
        assert_eq!((bow.upper.motion, bow.lower.motion), (0x904, 0x905));
    }
    assert!(sim.player(2).unwrap().bow.is_none());
    assert_eq!(sim.player(2).unwrap().volleys_fired, 0);
    eprintln!("installed bow: {} records, draw observed for {} engine frames", rows.len(), phases.len());
}
