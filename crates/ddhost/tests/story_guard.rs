//! Installed NPC1 dialogue owns the missing GSF381 transition. This fixture
//! supplies actor admission explicitly; it never reports TalkEnd or movement.
use ddhost::{Control, Host, ProfileId};
use ddstory::{data::{Data, InstallData}, Story};
use std::sync::Arc;

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
}

#[test]
fn installed_first_pawn_guard_boots_quest_after_owned_dialogue_exit() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { return; }
    let exe = ddexe::locate(&native).unwrap();
    let mut story = Story::new_game(InstallData::open(&native, Some(&exe)).unwrap());
    story.world.stage_no = 200;
    story.stage.stage_no = 200;
    story.world.set_sce_no(1300, true);
    let path = r"scr\st200\fsm\bootev_st200ev15";
    story.tasks.push(ddstory::script::Script::new(path, story.data.fsm(path).unwrap()).unwrap());
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    let first = host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    let other = host.join(ProfileId(2), Control::Local, [0.0; 3], 0.0).unwrap();
    host.data.npc = Some(Arc::new(ddnpcrt::InstallData::open(&native, "eng").unwrap()));
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let groups = ddhost::npc_placements::group_list(&vfs, 200).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 200, &groups, &[(0, 0)]).unwrap();
    let gates = ddspawn::World { scene: 1400, hour: 12, flag: &|flag| flag == 32 };
    let placement = layouts.candidates(&gates).placements.into_iter()
        .find(|p| p.npc == 1 && p.target.group == 27 && p.target.index == 0).unwrap();
    let target = placement.target;
    host.load_story_npc_intel(placement.clone()).unwrap();
    host.apply_story_npc(200, target, r"ai\fsm\npc\st200\quest\q0002_n001_ai");
    for _ in 0..300 {
        host.tick(&Default::default()).unwrap();
        assert!(!host.story.as_ref().unwrap().world.gsf.get(381), "no elapsed completion");
    }
    assert!(host.npc_errors.is_empty(), "Guard graph errors: {:?}", host.npc_errors);
    let guard = host.story_npc_guard(1).unwrap();
    assert_eq!((guard.kind, guard.message_kind, guard.message, guard.quest), (1, 2, 1, 2));
    let owner = host.story_npc_lifetime(1).unwrap();
    let position = host.story_npc_position(1).unwrap();
    let motion = ddnpcrt::guard::TalkMotionObservation { mode: Some(3), ..Default::default() };
    assert!(host.talk_story_guard(first, owner, false, 0.0, motion).unwrap().is_none());
    host.sim.player_mut(first).unwrap().position = [position[0] + 300.0, position[1], position[2]];
    assert!(host.talk_story_guard(first, owner, true, 0.0, motion).unwrap().is_none());
    host.sim.player_mut(first).unwrap().position = position;
    host.sim.player_mut(other).unwrap().position = position;
    let outcome = host.talk_story_guard(first, owner, true, 0.0, motion).unwrap().unwrap();
    assert!(matches!(outcome.events.first(), Some(ddnpcrt::TalkEvent::Line {
        player, npc: 1, conversation: 1, .. }) if *player == first));
    assert!(host.talk_story_guard(other, owner, true, 0.0, motion).unwrap().is_none());
    assert!(!host.story.as_ref().unwrap().world.gsf.get(381));
    host.leave(first);
    host.tick(&Default::default()).unwrap();
    assert_eq!(host.story.as_ref().unwrap().stage.game_flags.s54, 0);
    assert!(!host.story.as_ref().unwrap().world.gsf.get(381), "seat departure cancels; it cannot finish the authored talk");
    assert!(host.talk_story_guard(other, owner, true, 0.0, motion).unwrap().is_some(),
        "manual Guard can be retried after its interacting seat departs");
    let mut end = false;
    for _ in 0..40 {
        let outcome = host.talk_next(other).unwrap();
        if outcome.events.iter().any(|event| matches!(event, ddnpcrt::TalkEvent::End { .. })) {
            end = true;
            break;
        }
        host.tick(&Default::default()).unwrap();
        assert!(!host.story.as_ref().unwrap().world.gsf.get(381));
    }
    assert!(end);
    assert_eq!(host.story.as_ref().unwrap().stage.game_flags.s54, 1,
        "text completion retains actor dialogue ownership until its exit phase");
    assert!(!host.story.as_ref().unwrap().world.gsf.get(381), "UI End is not the actor frame callback");
    for _ in 0..20 { host.tick(&Default::default()).unwrap(); }
    assert!(host.story.as_ref().unwrap().world.gsf.get(381));
    assert!(host.story.as_ref().unwrap().quest(2).is_some());
    assert_eq!(host.story.as_ref().unwrap().stage.game_flags.s54, 0);
    assert_eq!(host.story.as_ref().unwrap().world.sce_no, 1400,
        "quest boot does not rewrite SceNo; q0002 cleanup changes it later");
    host.unbind_story_npc(200, target);
    host.load_story_npc_intel(placement).unwrap();
    assert!(host.talk_story_guard(other, owner, true, 0.0, motion).unwrap().is_none(),
        "stale generation cannot own replacement dialogue");
}

#[test]
fn installed_reynard_flags_and_gauge_follow_authored_states_without_actor_completion() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { return; }
    let mut story = Story::new_game(ddhost::data::NoStoryData);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1410, true);
    let mut world = ddhost::opening::OpeningWorld::new();
    world.enable_live_encounters();
    let mut host = Host::new(ddsim::Sim::new(Default::default()), Some(story), world, Box::new(Void));
    host.data.npc = Some(Arc::new(ddnpcrt::InstallData::open(&native, "eng").unwrap()));
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let groups = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let gates = ddspawn::World { scene: 1410, hour: 12, flag: &|flag| flag == 3 };
    let placement = layouts.candidates(&gates).placements.into_iter()
        .find(|p| p.npc == 304 && p.target.group == 4 && p.target.index == 0).unwrap();
    let target = placement.target;
    host.load_story_npc_intel(placement).unwrap();
    assert_eq!(host.story_npc_gauge_visible(304), Some(false));
    for _ in 0..90 { host.tick(&Default::default()).unwrap(); }
    assert!(host.npc_errors.is_empty(), "Reynard must not quarantine at Invincible or SetGaugeDisp: {:?}", host.npc_errors);
    let initial = host.story_npc_intel_flags(304).unwrap();
    assert_eq!(initial & 0x1800_0200, 0x1800_0200);
    assert_eq!(host.story_npc_gauge_visible(304), Some(true));
    assert!(!host.story_npc_action(304).unwrap().3);
    // Explicit quest observations in this isolated graph fixture. These
    // are never manufactured by the actor/callback implementation.
    host.story.as_mut().unwrap().world.set_quest_bit(2, 25, true);
    for _ in 0..20 { host.tick(&Default::default()).unwrap(); }
    assert_eq!(host.story_npc_intel_flags(304), Some(initial & !0x1800_0000),
        "authored clear preserves unrelated intel bits");
    assert_eq!(host.story_npc_gauge_visible(304), Some(true));
    host.story.as_mut().unwrap().world.set_quest_bit(2, 20, true);
    for _ in 0..20 { host.tick(&Default::default()).unwrap(); }
    assert_eq!(host.story_npc_gauge_visible(304), Some(false));
    assert!(!host.story_npc_action(304).unwrap().3, "motion arrival has not been observed");
    assert!(host.npc_errors.is_empty());
    host.unbind_story_npc(100, target);
    assert_eq!(host.story_npc_gauge_visible(304), None);
    assert_eq!(host.story_npc_intel_flags(304), None);
}
