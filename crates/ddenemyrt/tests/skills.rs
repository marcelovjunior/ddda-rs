use ddenemyrt::{arisen::ArisenData, skills::SkillSet};

#[test]
fn installed_skill_banks_and_shells() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.exists() {
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let sword = ArisenData::fighter(&vfs).unwrap();
    let mage = ArisenData::mage(&vfs).unwrap();
    let skills = SkillSet::load(&vfs, &file.image().unwrap(), &sword, &mage).unwrap();

    assert_eq!(skills.0.len(), 4);
    for id in [41, 43, 210, 212] {
        let mut sim = ddsim::Sim::new(Default::default());
        sim.add_player(0, [0.; 3], 0.);
        sim.add_player(1, [1000., 0., 0.], 0.);
        skills.install(&mut sim);
        let skill = &skills.0[&id];
        let p = sim.player_mut(0).unwrap();
        p.weapon = skill.definition.weapon;
        p.skills.equipped[0] = Some(id);
        let initial = p.stamina;
        sim.request_skill(0, 0);
        let mut paid = None;
        let mut old = initial;
        for tick in 0..1000 {
            sim.step(1. / 120., &Default::default(), &Flat);
            let p = sim.player(0).unwrap();
            if p.stamina < old - 1. {
                assert!(paid.is_none(), "skill {id} charged twice");
                assert!((old - p.stamina - skill.definition.cost).abs() < 0.01);
                paid = Some(tick);
            }
            old = p.stamina;
            if tick > 0 && p.skills.active.is_none() {
                break;
            }
        }
        assert!(paid.is_some(), "skill {id} never charged");
        assert!(sim.player(0).unwrap().skills.active.is_none());
        let releases = &sim.player(0).unwrap().skills.released;
        assert_eq!(
            releases.as_slice(),
            if id >= 210 { std::slice::from_ref(&id) } else { &[] }
        );
        assert_eq!(sim.player(1).unwrap().stamina, initial);
        assert!(sim.player(1).unwrap().skills.active.is_none());

        // A new cast cannot override an unrelated staff attack, and a
        // host-forced fall interrupts an active skill before another tick.
        sim.player_mut(0).unwrap().state = ddsim::State::Shot {
            phase: ddsim::ShotPhase::Start,
        };
        sim.request_skill(0, 0);
        sim.step(1. / 120., &Default::default(), &Flat);
        assert!(sim.player(0).unwrap().skills.active.is_none());
        sim.player_mut(0).unwrap().state = ddsim::State::Wait;
        sim.request_skill(0, 0);
        sim.step(1. / 120., &Default::default(), &Flat);
        assert!(sim.player(0).unwrap().skills.active.is_some());
        sim.player_mut(0).unwrap().state = ddsim::State::Fall;
        sim.step(1. / 120., &Default::default(), &Flat);
        assert!(sim.player(0).unwrap().skills.active.is_none());
    }
    let mut spells = ddenemyrt::skills::spells::SpellWorld::default();
    let mut owner = 1000;
    let data = skills.0[&210].spell.as_ref().unwrap();
    spells.fire(
        data,
        210,
        0,
        &skills.0[&210].release_pose([0.; 3], 0.),
        Some([0., 100., 500.]),
        &mut owner,
    );
    assert_eq!(spells.shells.len(), 1);
    assert_eq!(spells.shells[0].speed, 80.);
    spells.step(&skills, 1.);
    assert_eq!(spells.shells[0].speed, 94.);
    assert_eq!(spells.shells[0].distance, 80.);
    assert!(!spells.shells[0].volumes(data).is_empty());
    spells.hit(&[1000], &skills, &mut owner);
    assert_eq!(spells.shells.len(), 1);
    assert!(spells.shells[0].impact);
    assert_eq!(spells.shells[0].owner, 1001);
    assert!(!spells.shells[0].volumes(data).is_empty());
    spells.step(&skills, 2.);
    assert!(spells.shells.is_empty());
    let data = skills.0[&212].spell.as_ref().unwrap();
    spells.fire(
        data,
        212,
        1,
        &skills.0[&212].release_pose([0.; 3], 0.),
        Some([0., 0., 500.]),
        &mut owner,
    );
    assert_eq!(spells.shells.len(), 3);
    assert_eq!(
        spells.shells.iter().map(|s| s.delay).collect::<Vec<_>>(),
        [0., 7., 14.]
    );
    assert!(spells.shells[0].volumes(data).is_empty());
    spells.step(&skills, 1.);
    assert!(!spells.shells[0].volumes(data).is_empty());
    assert!(spells.shells[1].volumes(data).is_empty());
    for _ in 0..20 {
        spells.step(&skills, 1.);
    }
    assert!(spells.shells.is_empty());
}

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.)
    }
}
