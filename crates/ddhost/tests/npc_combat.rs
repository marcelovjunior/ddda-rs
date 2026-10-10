use ddhost::{npc_combat::DeathObservation, Host};
use ddstory::{Host as _, Story};
use std::sync::Arc;

struct Flat;
impl ddsim::Ground for Flat {
    fn height(&self, _: f32, _: f32, below: f32) -> Option<f32> { Some(below.min(0.0)) }
}

type TestHost = Host<ddhost::data::NoStoryData, ddhost::opening::OpeningWorld>;
struct Fixture {
    host: TestHost,
    placement: ddhost::npc_placements::Placement,
    lifetime: ddhost::story_npcs::NpcLifetime,
    owner: u32,
    body: Arc<ddhost::ddenemyrt::npc_body::NpcBody>,
    region: ddcombat::damage::RegionPool,
}

fn fixture() -> Option<Fixture> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() { eprintln!("SKIP: no installed game data"); return None; }
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
    host.load_story_npc_intel(placement.clone()).unwrap();
    let lifetime = host.story_npc_lifetime(304).unwrap();
    host.bind_story_npc_motion_body(lifetime, banks).unwrap();
    let combat = host.bind_story_npc_combat(lifetime, body.clone(), region.clone(), body.resolve(3226).unwrap())
        .unwrap().unwrap();
    Some(Fixture { host, placement, lifetime, owner: combat, body, region })
}

#[test]
fn installed_host_npc_hp_observations_keep_lifetime_and_death_stages_separate() {
    let Some(Fixture { mut host, placement, lifetime, owner: combat, body, region }) = fixture() else { return };
    let target = placement.target;
    let Some(mtlot::Value::F32(rate)) = placement.record.get("HPの倍率") else { panic!("installed HP rate absent") };
    let expected_max = ddhost::ddenemyrt::npc_combat::ORDINARY_NPC_HP * rate;
    assert_eq!(host.npc_region(), ddhost::ddenemyrt::npc_combat::Region::Other);
    assert_eq!(host.story_npc_combat_facts(lifetime).unwrap().hp, expected_max);
    let same = host.bind_story_npc_combat(lifetime, body.clone(), region.clone(), body.resolve(3226).unwrap());
    assert_eq!(same.unwrap(), Some(combat));
    for _ in 0..90 { host.tick(&Default::default()).unwrap(); }
    assert!(host.world.npc_intel(100, &target).unwrap().hp_positive);
    assert_eq!(host.story_npc_combat_facts(lifetime).unwrap().hp, expected_max);
    let ddhost::ddenemyrt::combat::Victim::Npc { region: live, .. } =
        &mut host.fight.as_mut().unwrap().npcs.get_mut(&combat).unwrap().fighter.victim else { unreachable!() };
    live.hp -= 100.0;
    assert_eq!(host.bind_story_npc_combat(lifetime, body.clone(), region.clone(), body.resolve(3226).unwrap()).unwrap(), Some(combat));
    let facts = host.story_npc_combat_facts(lifetime).unwrap();
    assert_eq!(facts.hp, expected_max - 100.0, "rebinding must retain damaged current HP");
    assert_eq!(facts.max_hp, expected_max);
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

#[test]
fn installed_host_npc_death_uses_real_layer_then_intel_wait_then_body_retirement() {
    use ddhost::ddenemyrt::npc_death::DeathPhase;
    let Some(Fixture { mut host, placement, lifetime, owner, body, region }) = fixture() else { return };
    for _ in 0..90 { host.tick(&Default::default()).unwrap(); }
    assert!(host.story_npc_death_state(lifetime).is_none());
    let death = body.resolve(0xcc6 + i32::from(body.look.sex != 0)).unwrap();
    let length = body.body.motion(death).unwrap().frame_count.saturating_sub(1);
    assert!(length > 0);
    // Explicit HP-loss fixture isolates actor dispatch. Natural installed
    // goblin damage and NoDamage behavior are covered by npc_sensor tests.
    let npc = host.fight.as_mut().unwrap().npcs.get_mut(&owner).unwrap();
    let ddhost::ddenemyrt::combat::Victim::Npc { region: live, .. } = &mut npc.fighter.victim else { unreachable!() };
    live.hp = 0.0;
    host.tick(&Default::default()).unwrap();
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().phase, DeathPhase::Motion);
    let actor = host.story_npc_motion(304).unwrap();
    assert_eq!(actor.motion, (u16::from(death.bank) << 8) | death.slot);
    assert_eq!((actor.frame, actor.authored.blend_frames, actor.authored.speed), (0.0, 15.0, 1.0));
    assert!(!host.story.as_ref().unwrap().world.npc_app_die.get(304));
    for elapsed in 1..=length {
        host.tick(&Default::default()).unwrap();
        assert_eq!(host.story.as_ref().unwrap().world.npc_app_die.get(304), elapsed == length);
        assert!(!host.story.as_ref().unwrap().world.npc_die.get(304));
    }
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().wait, 1800.0,
        "line12 ran before the line15 apparent-death write");
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().phase, DeathPhase::CorpseWait);
    // Explicit accelerated host-clock fixture, not an ordinary-route timing
    // measurement: native independent clocks still consume frame units.
    host.clock.delta = 30.0;
    for _ in 0..59 { host.tick(&Default::default()).unwrap(); }
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().wait, 30.0);
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().alpha, 1.0);
    host.tick(&Default::default()).unwrap();
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().phase, DeathPhase::Fade);
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().alpha, 1.0);
    assert!(!host.story_npc_body_retired(304));
    host.tick(&Default::default()).unwrap();
    assert_eq!(host.story_npc_death_state(lifetime).unwrap().phase, DeathPhase::Finished);
    assert!(host.story.as_ref().unwrap().world.npc_die.get(304));
    assert!(host.story_npc_body_retired(304));
    assert!(!host.fight.as_ref().unwrap().npcs.contains_key(&owner));
    assert_eq!(host.story_npc_lifetime(304), Some(lifetime), "intel survives body removal");
    assert_eq!(host.story_npc_combat_facts(lifetime).unwrap().hp, 0.0);
    assert!(!host.world.npc_intel(100, &placement.target).unwrap().hp_positive);
    assert_eq!(host.bind_story_npc_combat(lifetime, body.clone(), region, death).unwrap(), None,
        "same intel lifetime cannot rematerialize its permanently removed body");
    host.tick(&Default::default()).unwrap();
    assert!(!host.fight.as_ref().unwrap().npcs.contains_key(&owner));
}
