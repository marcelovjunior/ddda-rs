//! Native slot projection through live, stable-profile host ownership.
use ddhost::data::NoStoryData;
use ddhost::{Control, Host, ProfileId};
use ddskill::vocation::{Vocation, WeaponCategory};
use ddstory::scripted::ScriptedHost;
use std::sync::Arc;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        Some(0.0)
    }
}

#[test]
fn installed_equipped_skill_refresh_is_owned_by_profiles_across_seat_reuse() {
    let native = std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    if !native.is_dir() {
        eprintln!("SKIP: no installed game for skill profile regression");
        return;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let file = ddexe::ExeFile::next_to(native).unwrap();
    let tables = Arc::new(ddprogress::Tables::load(&vfs, &file.image().unwrap()).unwrap());
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        None,
        ScriptedHost::default(),
        Box::new(Flat),
    );
    let fighter = host.join(ProfileId(11), Control::Local, [0.0; 3], 0.0).unwrap();
    let mage = host
        .join(ProfileId(22), Control::Local, [1000.0, 0.0, 0.0], 0.0)
        .unwrap();
    host.data.tables = Some(tables);
    let fighter_record = host.create_arisen(fighter, Vocation::Fighter).unwrap();
    let mage_record = host.create_arisen(mage, Vocation::Mage).unwrap();
    let mage_slots = [Some(210), None, None, Some(220), None, None];
    let fighter_slots = [Some(40), None, None, Some(272), None, None];
    assert_eq!(
        host.refresh_equipped_skills(mage, WeaponCategory::Staff, None)
            .unwrap(),
        mage_slots
    );
    assert_eq!(
        host.refresh_equipped_skills(fighter, WeaponCategory::Sword, Some(WeaponCategory::Shield))
            .unwrap(),
        fighter_slots
    );
    assert_eq!(host.progression.characters[fighter_record].active, fighter_slots);
    assert_eq!(host.progression.characters[mage_record].active, mage_slots);
    assert_eq!(
        host.sim.player(mage).unwrap().skills.equipped,
        mage_slots.map(|skill| skill.map(u32::from))
    );
    // Customized emptiness is durable; no preview grant fills it in.
    host.progression.characters[fighter_record].weapon_sets[WeaponCategory::Sword as usize][0] = None;
    let customized = [None, None, None, Some(272), None, None];
    assert_eq!(
        host.refresh_equipped_skills(fighter, WeaponCategory::Sword, Some(WeaponCategory::Shield))
            .unwrap(),
        customized
    );
    let mage_before = host.sim.player(mage).unwrap().skills.equipped;
    host.leave(fighter);
    let strider = host.join(ProfileId(33), Control::Local, [0.0; 3], 0.0).unwrap();
    assert_eq!(strider, fighter);
    // A plain host has no equipped fight kit; create the new profile's
    // progression record if the generic lifecycle did not create one.
    if !host
        .progression
        .party
        .iter()
        .flatten()
        .any(|member| member.role == ddprogress::Role::Arisen(strider))
    {
        host.create_arisen(strider, Vocation::Strider).unwrap();
    }
    let strider_slots = host
        .refresh_equipped_skills(strider, WeaponCategory::Dagger, Some(WeaponCategory::Bow))
        .unwrap();
    assert_eq!(strider_slots, [Some(152), None, None, Some(350), None, None]);
    assert_eq!(host.sim.player(mage).unwrap().skills.equipped, mage_before);
    // The current two-Arisen policy needs one vacant seat. The returning
    // Fighter takes Mage's old seat; Strider still owns Fighter's old seat.
    host.leave(mage);
    let returning = host
        .join(ProfileId(11), Control::Local, [2000.0, 0.0, 0.0], 0.0)
        .unwrap();
    assert_ne!(returning, fighter);
    assert_eq!(
        host.refresh_equipped_skills(returning, WeaponCategory::Sword, Some(WeaponCategory::Shield))
            .unwrap(),
        customized
    );
    assert_eq!(
        host.sim.player(strider).unwrap().skills.equipped,
        strider_slots.map(|skill| skill.map(u32::from))
    );
    let before = host.saved_hash();
    assert!(host
        .refresh_equipped_skills(99, WeaponCategory::Sword, None)
        .is_err());
    assert_eq!(host.saved_hash(), before);
    // Keep generic story data typed explicitly; no scene callback fixture.
    let _: &Host<NoStoryData, ScriptedHost> = &host;
}
