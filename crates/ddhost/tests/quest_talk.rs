//! Installed NPC control and real read-line completion; quest-start and
//! rescue observations below are explicit fixtures, never route output.
use ddeconomy::inventory::Owner;
use ddhost::data::NoStoryData;
use ddhost::opening::OpeningWorld;
use ddhost::story_npcs::NpcLifetime;
use ddhost::{Control, FrameInputs, Host, ProfileId};
use ddnpcrt::{guard::TalkMotionObservation, TalkEvent};
use std::sync::Arc;

struct Void;
impl ddsim::Ground for Void {
    fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> {
        None
    }
}

fn installed_host() -> Option<(
    Host<NoStoryData, OpeningWorld>,
    NpcLifetime,
    ddhost::npc_placements::Placement,
)> {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: install absent");
        return None;
    }
    let vfs = ddvfs::Vfs::open(&native).unwrap();
    let mut story = ddstory::Story::new_game(NoStoryData);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1410, false);
    story.world.set_quest_bit(2, 95, true); // Explicit quest-start fixture.
    story.world.set_quest_bit(2, 20, true); // Explicit successful-rescue fixture.
    let mut host = Host::new(
        ddsim::Sim::new(Default::default()),
        Some(story),
        OpeningWorld::new(),
        Box::new(Void),
    );
    host.data.npc = Some(Arc::new(ddnpcrt::InstallData::open(&native, "eng").unwrap()));
    host.join(ProfileId(1), Control::Local, [0.0; 3], 0.0).unwrap();
    host.join(ProfileId(2), Control::Remote, [0.0; 3], 0.0).unwrap();
    let groups = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
    let layouts = ddhost::npc_placements::LoadedLayouts::read(&vfs, 100, &groups, &[(0, 0)]).unwrap();
    let gates = ddspawn::World {
        scene: 1410,
        hour: 12,
        flag: &|flag| flag == 3,
    };
    let placement = layouts
        .candidates(&gates)
        .placements
        .into_iter()
        .find(|placement| placement.npc == 304 && placement.target.group == 4 && placement.target.index == 0)
        .unwrap();
    host.load_story_npc_intel(placement.clone()).unwrap();
    let owner = host.story_npc_lifetime(304).unwrap();
    Some((host, owner, placement))
}

fn finish(host: &mut Host<NoStoryData, OpeningWorld>, player: u8) -> Vec<TalkEvent> {
    let mut out = Vec::new();
    for _ in 0..50 {
        let next = host.talk_next(player).unwrap();
        let ended = next
            .events
            .iter()
            .any(|event| matches!(event, TalkEvent::End { .. }));
        out.extend(next.events);
        if ended {
            return out;
        }
    }
    panic!("installed NPC dialogue did not end after reading its actual lines");
}

fn count(host: &Host<NoStoryData, OpeningWorld>, seat: u8) -> u32 {
    host.economy
        .inventory
        .bag(Owner::Arisen(seat))
        .map_or(0, |bag| bag.count(1058))
}

#[test]
fn installed_reynard_thanks_reward_is_once_and_owned_by_interacting_seat() {
    let Some((mut host, owner, _)) = installed_host() else {
        return;
    };
    let motion = TalkMotionObservation {
        mode: Some(3),
        ..Default::default()
    };
    assert!(host.talk_story_npc(1, owner, false, motion).unwrap().is_none());
    let first = host.talk_story_npc(1, owner, true, motion).unwrap().unwrap();
    assert!(matches!(
        first.events.first(),
        Some(TalkEvent::Line {
            player: 1,
            conversation: 12,
            ..
        })
    ));
    assert_eq!(host.locks.holder(304), Some(1));
    assert!(host.talk_story_npc(0, owner, true, motion).unwrap().is_none());
    assert!(
        host.talk_story_npc(1, owner, true, motion).unwrap().is_none(),
        "same seat cannot restart active stock dialogue"
    );
    assert!(host.talk_next(0).is_err());
    for _ in 0..100 {
        host.tick(&FrameInputs::new()).unwrap();
    }
    assert!(
        !host.story.as_ref().unwrap().world.quest_bit(2, 88),
        "time alone cannot acknowledge text"
    );
    assert_eq!((count(&host, 0), count(&host, 1)), (0, 0));
    let events = finish(&mut host, 1);
    assert!(events
        .iter()
        .any(|event| matches!(event, TalkEvent::QuestEffects { player: 1, .. })));
    assert!(host.story.as_ref().unwrap().world.quest_bit(2, 88));
    assert_eq!((count(&host, 0), count(&host, 1)), (0, 1));
    assert!(host.talk_next(1).is_err(), "late repeat cannot grant again");
    host.tick(&FrameInputs::new()).unwrap(); // Actual stock-free actor/slot cleanup.
    assert_eq!(host.locks.holder(304), None);
    for seat in [0, 1, 0] {
        let next = host.talk_story_npc(seat, owner, true, motion).unwrap().unwrap();
        assert!(matches!(
            next.events.first(),
            Some(TalkEvent::Line { conversation: 13, .. })
        ));
        finish(&mut host, seat);
        host.tick(&FrameInputs::new()).unwrap();
    }
    assert_eq!((count(&host, 0), count(&host, 1)), (0, 1));
}

#[test]
fn installed_reynard_unload_cancels_selected_text_without_reward_or_stale_completion() {
    let Some((mut host, owner, placement)) = installed_host() else {
        return;
    };
    let motion = TalkMotionObservation {
        mode: Some(3),
        ..Default::default()
    };
    host.talk_story_npc(0, owner, true, motion).unwrap().unwrap();
    host.unbind_story_npc(100, placement.target);
    assert!(host.talk_next(0).is_err());
    assert_eq!(host.locks.holder(304), None);
    assert!(!host.story.as_ref().unwrap().world.quest_bit(2, 88));
    assert_eq!(count(&host, 0), 0);
    host.load_story_npc_intel(placement).unwrap();
    let replacement = host.story_npc_lifetime(304).unwrap();
    assert_ne!(replacement, owner);
    assert!(host.talk_story_npc(0, owner, true, motion).unwrap().is_none());
    let next = host
        .talk_story_npc(1, replacement, true, motion)
        .unwrap()
        .unwrap();
    assert!(matches!(
        next.events.first(),
        Some(TalkEvent::Line { conversation: 12, .. })
    ));
    finish(&mut host, 1);
    assert_eq!((count(&host, 0), count(&host, 1)), (0, 1));
}

#[test]
fn installed_reynard_once_mask_save_roundtrip_does_not_reaward_and_absent_chunk_resets() {
    let Some((mut host, owner, _)) = installed_host() else {
        return;
    };
    let motion = TalkMotionObservation {
        mode: Some(3),
        ..Default::default()
    };
    host.talk_story_npc(1, owner, true, motion).unwrap().unwrap();
    assert!(
        host.save().is_err(),
        "active text cannot be saved with a pending reward"
    );
    finish(&mut host, 1);
    host.tick(&FrameInputs::new()).unwrap();
    let before_hash = host.saved_hash();
    let save = host.save().unwrap();
    assert!(save
        .get(ddhost::save::TAG_NPC_QUEST, ddhost::save::Owner::Shared)
        .is_some());
    let Some((mut restored, restored_owner, _)) = installed_host() else {
        return;
    };
    restored.load(&save, NoStoryData).unwrap();
    assert_eq!(restored.saved_hash(), before_hash);
    assert_eq!((count(&restored, 0), count(&restored, 1)), (0, 1));
    let out = restored
        .talk_story_npc(0, restored_owner, true, motion)
        .unwrap()
        .unwrap();
    assert!(matches!(
        out.events.first(),
        Some(TalkEvent::Line { conversation: 13, .. })
    ));
    finish(&mut restored, 0);
    assert_eq!((count(&restored, 0), count(&restored, 1)), (0, 1));

    let mut old = save.clone();
    old.chunks
        .retain(|chunk| chunk.tag != ddhost::save::TAG_NPC_QUEST);
    restored.load(&old, NoStoryData).unwrap();
    assert!(
        restored.npcs.quest_controls.is_empty(),
        "old save cannot inherit current-runtime masks"
    );
}

#[test]
fn installed_npc_control_save_rejects_corruption_before_replacing_row_memory() {
    let Some((mut host, owner, _)) = installed_host() else {
        return;
    };
    let motion = TalkMotionObservation {
        mode: Some(3),
        ..Default::default()
    };
    host.talk_story_npc(0, owner, true, motion).unwrap().unwrap();
    finish(&mut host, 0);
    host.tick(&FrameInputs::new()).unwrap();
    let original = host.npcs.quest_controls[&304].saved_disabled_rows();
    for bytes in [
        vec![2],             // Unsupported NPCQ version.
        vec![1, 1, 0, 0, 0], // Truncated first control.
        {
            let mut writer = ddid::Writer::new();
            writer.u8(1).len(1).u32(304).len(1).u32(999).u32(0);
            writer.finish() // Row outside the installed table.
        },
    ] {
        let mut bad = host.save().unwrap();
        bad.put(ddhost::save::TAG_NPC_QUEST, ddhost::save::Owner::Shared, bytes);
        assert!(host.load(&bad, NoStoryData).is_err());
        assert_eq!(host.npcs.quest_controls[&304].saved_disabled_rows(), original);
    }
}
