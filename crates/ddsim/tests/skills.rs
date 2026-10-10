use ddsim::skills::{Definition, Kind, Phase};
use ddsim::{Ground, RootMotion, Sim, Weapon};

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
}

fn series() -> Sim {
    let mut sim = Sim::new(Default::default());
    for seat in [0, 1] {
        sim.add_player(seat, [seat as f32 * 1000.0, 0.0, 0.0], 0.0);
        let p = sim.player_mut(seat).unwrap();
        p.weapon = Weapon::Dagger;
        p.unit_rate = 1.0;
        p.skills.equipped[0] = Some(152);
    }
    // Synthetic short motion endpoints isolate native B23F10's repeat
    // decisions from installed animation duration. No game data is copied.
    sim.skill_definitions.insert(
        152,
        Definition {
            weapon: Weapon::Dagger,
            kind: Kind::DaggerSeries {
                intro: 0x71e,
                strikes: [0x71f, 0x726, 0x727],
                end: 0x720,
                rate: 1.2,
                maximum_cycles: 3,
            },
            cost: 0.0,
            motions: [0x71e, 0x71f, 0x726, 0x727, 0x720]
                .into_iter()
                .map(|id| (id, RootMotion::none(6)))
                .collect(),
        },
    );
    sim
}

fn frame(sim: &mut Sim) {
    sim.step(1.0 / 120.0, &Default::default(), &Flat);
}

fn until_phase(sim: &mut Sim, phase: Phase) {
    for _ in 0..100 {
        if sim
            .player(0)
            .unwrap()
            .skills
            .active
            .is_some_and(|a| a.phase == phase)
        {
            return;
        }
        frame(sim);
    }
    panic!("phase {phase:?} was not reached");
}

#[test]
fn scarlet_kisses_requires_two_strike_presses_and_keeps_other_seat_independent() {
    let mut sim = series();
    sim.request_skill(0, 0);
    frame(&mut sim);
    let active = sim.player(0).unwrap().skills.active.unwrap();
    assert_eq!((active.phase, active.motion), (Phase::Intro, 0x71e));
    sim.request_skill(0, 0); // Intro presses cannot extend the first strike.
    frame(&mut sim);
    assert!((sim.player(0).unwrap().frame - 0.3).abs() < 0.0001);
    until_phase(&mut sim, Phase::Strike);
    let active = sim.player(0).unwrap().skills.active.unwrap();
    assert_eq!(
        (active.phase, active.motion, active.presses),
        (Phase::Strike, 0x71f, 0)
    );
    sim.request_skill(0, 0); // One strike press still ends the action.
    frame(&mut sim);
    until_phase(&mut sim, Phase::End);
    let active = sim.player(0).unwrap().skills.active.unwrap();
    assert_eq!((active.phase, active.motion), (Phase::End, 0x720));
    frame(&mut sim);
    assert_eq!(sim.player(0).unwrap().frame, 0.25); // End rate is native 1.
    for _ in 0..30 {
        if sim.player(0).unwrap().skills.active.is_none() {
            break;
        }
        frame(&mut sim);
    }
    assert!(sim.player(0).unwrap().skills.active.is_none());
    assert!(sim.player(1).unwrap().skills.active.is_none());
    assert_eq!(sim.player(1).unwrap().attacks_started, 0);
}

#[test]
fn scarlet_kisses_repeat_variants_stop_at_resource_cycle_limit() {
    let mut sim = series();
    sim.request_skill(0, 0);
    frame(&mut sim);
    until_phase(&mut sim, Phase::Strike);
    for (cycle, motion) in [0x71f, 0x726, 0x727].into_iter().enumerate() {
        let active = sim.player(0).unwrap().skills.active.unwrap();
        assert_eq!(
            (active.phase, active.motion, active.cycles),
            (Phase::Strike, motion, cycle as u32)
        );
        for tick in 0..100 {
            if sim.player(0).unwrap().skills.active.unwrap().motion != motion {
                break;
            }
            if tick < 2 {
                sim.request_skill(0, 0);
            }
            frame(&mut sim);
        }
    }
    let active = sim.player(0).unwrap().skills.active.unwrap();
    assert_eq!(
        (active.phase, active.motion, active.cycles),
        (Phase::End, 0x720, 3)
    );
    assert!(sim.player(0).unwrap().skills.released.is_empty());
}

#[test]
fn shield_summons_requires_equipped_secondary_and_emits_entry_once() {
    let mut sim = series();
    sim.skill_definitions.insert(
        272,
        Definition {
            weapon: Weapon::Sword,
            kind: Kind::ShieldProvoke {
                motion: 0x92f,
                radius: 3000.0,
                amount: 500,
            },
            cost: 0.0,
            motions: [(0x92f, RootMotion::none(6))].into_iter().collect(),
        },
    );
    let actor = sim.player_mut(0).unwrap();
    actor.weapon = Weapon::Sword;
    actor.skills.equipped[3] = Some(272);
    sim.request_skill(0, 3);
    frame(&mut sim);
    assert!(sim.player(0).unwrap().skills.active.is_none());
    assert!(sim.player(0).unwrap().skills.entered.is_empty());
    sim.player_mut(0).unwrap().secondary_weapon = Weapon::Shield;
    sim.request_skill(0, 3);
    frame(&mut sim);
    assert_eq!(sim.player(0).unwrap().skills.active.unwrap().motion, 0x92f);
    assert_eq!(sim.player(0).unwrap().skills.entered, [272]);
    sim.request_skill(0, 3); // A press cannot replace an active Summons.
    for _ in 0..30 {
        frame(&mut sim);
    }
    assert!(sim.player(0).unwrap().skills.active.is_none());
    assert_eq!(sim.player(0).unwrap().skills.entered, [272]);
    assert!(sim.player(0).unwrap().skills.released.is_empty());
    assert!(sim.player(1).unwrap().skills.entered.is_empty());
}
