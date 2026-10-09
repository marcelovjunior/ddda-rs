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
    }
}

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.)
    }
}
