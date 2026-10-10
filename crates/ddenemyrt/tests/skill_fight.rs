//! Installed sword skills against a standing goblin. This exercises the
//! headless hit pipeline, not enemy AI or an original-game comparison.
use ddenemyrt::fight::{Fight, FightData};
use std::sync::Arc;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
}

/// Fixture placement follows the installed attack geometry, without
/// changing its root motion to make a forward-facing test target fit.
/// This checks collision/damage integration, not live targeting fidelity.
fn target_in_reach(data: &Arc<FightData>, id: u32) -> [f32; 3] {
    let mut sim = ddsim::Sim::new(Default::default());
    let mut fight = Fight::new(data.clone());
    sim.add_player(0, [0.0; 3], 0.0);
    fight.add_arisen(0);
    fight.equip(&mut sim, 0);
    sim.player_mut(0).unwrap().skills.equipped[0] = Some(id);
    sim.request_skill(0, 0);
    for _ in 0..180 {
        sim.step(1.0 / 30.0, &Default::default(), &Flat);
        fight.volumes(&sim);
        for attack in &fight.arisen[&0].fighter.attacks {
            if let ddcombat::volumes::Volume::Sphere { c, .. } = attack.volume {
                return [c[0], 0.0, c[2]];
            }
        }
    }
    panic!("skill {id} produced no fixture sphere");
}

#[test]
fn sword_skills_use_their_own_bank_and_damage_a_goblin() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.is_dir() {
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let exe = ddexe::ExeFile::next_to(native).unwrap();
    let data = Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    for id in [41, 43] {
        let mut sim = ddsim::Sim::new(Default::default());
        let mut fight = Fight::new(data.clone());
        for seat in [0, 1] {
            sim.add_player(seat, [seat as f32 * 1000.0, 0.0, 0.0], 0.0);
            fight.add_arisen(seat);
            fight.equip(&mut sim, seat);
        }
        sim.player_mut(0).unwrap().skills.equipped[0] = Some(id);
        sim.player_mut(1).unwrap().skills.equipped[0] = Some(if id == 41 { 43 } else { 41 });
        let initial_stamina = sim.player(1).unwrap().stamina;
        fight.add_goblin(
            if id == 41 {
                [0.0, 0.0, 100.0]
            } else {
                target_in_reach(&data, id)
            },
            std::f32::consts::PI,
        );
        let initial_hp = fight.goblins[0].fighter.victim.hp();
        fight.volumes(&sim);
        sim.request_skill(0, 0);
        let mut emitted_attack = false;
        let mut hits = 0;
        for _ in 0..180 {
            hits += fight.hits(1.0).len();
            sim.step(1.0 / 30.0, &Default::default(), &Flat);
            if sim.player(0).unwrap().skills.active.is_some() {
                assert_eq!(sim.player(0).unwrap().push_rank(), if id == 41 { 15 } else { 14 });
            }
            fight.players_moved(&mut sim, &Flat, 1.0);
            fight.push_out(&mut sim, &Flat);
            fight.volumes(&sim);

            emitted_attack |= !fight.arisen[&0].fighter.attacks.is_empty();
            assert!(fight.arisen[&1].fighter.attacks.is_empty());
        }
        assert!(emitted_attack, "skill {id} produced no attack volumes");
        assert!(hits > 0, "skill {id} missed the standing goblin");
        assert!(fight.goblins[0].fighter.victim.hp() < initial_hp);
        assert_eq!(sim.player(1).unwrap().stamina, initial_stamina);
        assert!(sim.player(1).unwrap().skills.active.is_none());
    }
}

#[test]
fn mage_spells_damage_with_the_casting_seats_shell_ownership() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.is_dir() {
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let exe = ddexe::ExeFile::next_to(native).unwrap();
    let data = Arc::new(FightData::load(&vfs, &exe.image().unwrap()).unwrap());
    for id in [210, 212] {
        let mut sim = ddsim::Sim::new(Default::default());
        let mut fight = Fight::new(data.clone());
        sim.add_player(0, [1000.0, 0.0, 0.0], 0.0);
        sim.add_player(1, [0.0; 3], 0.0);
        fight.add_arisen(0);
        fight.add_arisen_as(1, ddenemyrt::fight::Kit::Mage);
        fight.equip(&mut sim, 0);
        fight.equip(&mut sim, 1);
        sim.player_mut(1).unwrap().skills.equipped[0] = Some(id);
        fight.add_goblin([0.0, 0.0, 400.0], std::f32::consts::PI);
        let initial_hp = fight.goblins[0].fighter.victim.hp();
        let other_stamina = sim.player(0).unwrap().stamina;
        fight.volumes(&sim);
        sim.request_skill(1, 0);
        let mut emitted = false;
        let mut spell_hit = false;
        for _ in 0..450 {
            let hits = fight.hits(1.0).to_vec();
            for hit in hits {
                if hit.attacker >= ddenemyrt::fight::SHELL_OWNER_BASE {
                    assert_eq!(fight.shooters.get(&hit.attacker), Some(&1));
                    spell_hit = true;
                }
            }
            sim.step(1.0 / 30.0, &Default::default(), &Flat);
            fight.players_moved(&mut sim, &Flat, 1.0);
            fight.volumes(&sim);
            for spell in &fight.spells.shells {
                assert_eq!((spell.skill, spell.shooter), (id, 1));
                emitted = true;
            }
        }
        assert!(emitted, "skill {id} did not spawn");
        assert!(spell_hit, "skill {id} did not hit");
        assert!(fight.goblins[0].fighter.victim.hp() < initial_hp);
        assert_eq!(sim.player(0).unwrap().stamina, other_stamina);
        assert!(sim.player(1).unwrap().skills.released.is_empty());
    }
}
