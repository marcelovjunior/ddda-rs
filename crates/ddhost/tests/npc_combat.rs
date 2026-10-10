use ddhost::{npc_combat::DeathObservation, Host};
use ddstory::{Host as _, Story};
use std::sync::Arc;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { Some(below.min(0.0)) }
}

#[test]
fn installed_host_npc_hp_observations_keep_lifetime_and_death_stages_separate() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no installed game data"); return; }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let exe = ddexe::ExeFile::next_to(&native).unwrap();
    let image = exe.image().unwrap();
    let body = Arc::new(ddhost::ddenemyrt::npc_body::NpcBody::load(&vfs, &image, 304, 304, 0, 0, 0).unwrap());
    let banks = Arc::new(ddhost::npc_motion::Banks::load(&vfs, &image, 0, 0).unwrap());
    let data = Arc::new(ddhost::ddenemyrt::fight::FightData::load(&vfs, &image).unwrap());
    let region = ddhost::ddenemyrt::npc_combat::ordinary_region(&vfs, &data.tables.game, 1000.0).unwrap();
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 100; story.world.stage_no = 100;
    story.world.set_sce_no(1410, false); story.world.set_clock(12, 0);
    story.world.set_lot(100, 3, true);
    let mut world = ddhost::opening::OpeningWorld::new(); world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Flat));
    host.data.npc = Some(Arc::new(ddnpcrt::InstallData::open(&native, "eng").unwrap()));
    host.fight = Some(ddhost::ddenemyrt::fight::Fight::new(data));
    let groups = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let gates = ddspawn::World { scene: 1410, hour: 12, flag: &|f| f == 3 };
    let placement = layouts.candidates(&gates).placements.into_iter().find(|p| p.npc == 304).unwrap();
    let target = placement.target;
    host.load_story_npc_intel(placement.clone()).unwrap();
    let lifetime = host.story_npc_lifetime(304).unwrap();
    host.bind_story_npc_motion_body(lifetime, banks).unwrap();
    let combat = host.bind_story_npc_combat(lifetime, body.clone(), region.clone(), body.resolve(3226).unwrap())
        .unwrap().unwrap();
    let same = host.bind_story_npc_combat(lifetime, body.clone(), region.clone(), body.resolve(3226).unwrap());
    assert_eq!(same.unwrap(), Some(combat));
    for _ in 0..90 { host.tick(&Default::default()).unwrap(); }
    assert!(host.world.npc_intel(100, &target).unwrap().hp_positive);
    assert_eq!(host.story_npc_combat_facts(lifetime).unwrap().hp, 1000.0);
    assert!(!host.observe_story_npc_death(lifetime, DeathObservation::FadeFinished));
    assert!(host.fight.as_ref().unwrap().npc_errors.is_empty());
    assert!(!host.fight.as_ref().unwrap().npcs[&combat].fighter.hurts.is_empty());
    // Explicit HP fixture isolates the observation/death-bank adapters;
    // natural installed goblin hits are separately tested in ddenemyrt.
    let npc = host.fight.as_mut().unwrap().npcs.get_mut(&combat).unwrap();
    let ddhost::ddenemyrt::combat::Victim::Npc { region: live, .. } = &mut npc.fighter.victim else { unreachable!() };
    live.hp = 0.0;
    host.tick(&Default::default()).unwrap();
    assert!(!host.world.npc_intel(100, &target).unwrap().hp_positive);
    assert!(!host.story.as_ref().unwrap().world.npc_app_die.get(304));
    assert!(!host.story.as_ref().unwrap().world.npc_die.get(304));
    assert!(host.observe_story_npc_death(lifetime, DeathObservation::MotionEnded));
    assert!(host.story.as_ref().unwrap().world.npc_app_die.get(304));
    assert!(!host.story.as_ref().unwrap().world.npc_die.get(304));
    assert!(host.observe_story_npc_death(lifetime, DeathObservation::FadeFinished));
    assert!(host.story.as_ref().unwrap().world.npc_die.get(304));
    host.unbind_story_npc(100, target);
    host.tick(&Default::default()).unwrap();
    assert!(!host.fight.as_ref().unwrap().npcs.contains_key(&combat));
    host.load_story_npc_intel(placement).unwrap();
    let replacement = host.story_npc_lifetime(304).unwrap();
    assert_ne!(replacement, lifetime);
    assert!(host.bind_story_npc_combat(lifetime, body.clone(), region.clone(), body.resolve(3226).unwrap()).unwrap().is_none());
    assert!(!host.observe_story_npc_death(lifetime, DeathObservation::FadeFinished));
}
