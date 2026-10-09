use ddnpcrt::{guard::{quest_path, quest_preloads, ResourceKey, Resources}, schedule::Guard,
    InstallData, NpcRuntime, Story, TalkError, TalkEvent, TalkSet};
use std::sync::Arc;

fn set(number: i32, text: &str) -> Arc<TalkSet> {
    let mut messages = [-1; 15]; messages[0] = 0;
    Arc::new(TalkSet {
        set: ddnpc::MessageSet { conversations: vec![ddnpc::Conversation {
            number, npc_id: 1, choices: [-1; 6], choice_jumps: [-1; 6], messages,
            motions: [-1; 15], faces: [-1; 15], global_flags: [-1; 15],
            quest_numbers: [-1; 15], quest_flags: [-1; 15], season: -1,
            season_sequence: -1, season_sequence_type: -1, quest_number: -1,
            quest_flag: -1, friendship: -1, friendship_max: -1, global_flag: -1,
        }] }, text: vec![text.into()],
    })
}
fn guard(kind: u32, message: i32) -> Guard {
    Guard { kind: 1, center: [0.0; 3], radius: 1.0, direction: 0.0, range: 0.0,
        height: 1.0, message_kind: kind, message, quest: 2, guard: false,
        jump_kind: 0, start_position: 0 }
}
#[test]
fn authored_resource_selection_and_seat_lock_are_independent_of_completion() {
    let mut resources = Resources::default();
    let base = set(7, "base"); let quest = set(12, "quest"); let over = set(9, "override");
    resources.insert(ResourceKey::Base, base.clone());
    assert!(resources.select(&guard(2, 12)).is_none(), "missing quest never uses base");
    resources.insert(ResourceKey::Quest(2), quest);
    resources.insert(ResourceKey::Override, over);
    assert_eq!(resources.select(&guard(4, 9)).unwrap().text[0], "override");
    assert_eq!(resources.select(&guard(4, 7)).unwrap().text[0], "base");
    assert_eq!(resources.select(&guard(5, 9)).unwrap().text[0], "base");
    assert!(resources.select(&guard(0, 7)).is_none());
    let world = ddstory::World::default();
    let story = Story { world: &world, equip_menus: false };
    let mut runtime = NpcRuntime::default();
    let chosen = resources.select(&guard(2, 12)).unwrap();
    assert!(matches!(&runtime.talk_authored(&story, 1, 304, chosen.clone(), 12).unwrap()[0],
        TalkEvent::Line { player: 1, npc: 304, conversation: 12, text, .. } if text == "quest"));
    assert_eq!(runtime.talk_authored(&story, 3, 304, chosen.clone(), 12), Err(TalkError::Busy(1)));
    assert!(matches!(&runtime.talk_authored(&story, 3, 1, base, 7).unwrap()[0],
        TalkEvent::Line { player: 3, npc: 1, .. }));
    assert_eq!(runtime.talk_next(1).unwrap(), vec![TalkEvent::End { player: 1, npc: 304 }]);
    assert_eq!(runtime.talking_to(1), Some(3));
    assert_eq!(runtime.talk_authored(&story, 1, 304, chosen, 999), Err(TalkError::Silent));
}
#[test]
fn installed_preload_whitelist_and_authored_quest_text() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.exists() { return; }
    let file = ddexe::ExeFile::next_to(&native).unwrap();
    let exe = file.image().unwrap();
    assert!(quest_preloads(&exe, 1).unwrap().contains(&2));
    assert!(quest_preloads(&exe, 304).unwrap().contains(&2));
    assert!(!quest_preloads(&exe, 2).unwrap().contains(&2));
    assert!(quest_preloads(&exe, u32::MAX).is_err());
    let data = InstallData::open(&native, "eng").unwrap();
    for npc in [1, 304] {
        let resource = data.authored_talk_set(&quest_path(npc, 2)).unwrap();
        assert!(!resource.set.conversations.is_empty());
        assert!(!resource.text.is_empty());
    }
}
