use ddnpcrt::quest_talk::{Context, Control, Effect, RowKey};
use ddnpcrt::{InstallData, NpcData, NpcRuntime, Story, TalkEvent};
use ddquest::qct::{Judgment, Outcome, QuestCtrl, Row, Table};
use std::sync::Arc;

fn result(kind: u32, a: u32, b: u32) -> Outcome {
    let mut fields = [0; 11];
    fields[0] = kind;
    fields[1] = a;
    fields[2] = b;
    Outcome(fields)
}

fn fixture() -> Control {
    Control::new(Arc::new(QuestCtrl {
        alloc_size: 0,
        trailer: 0,
        tables: vec![
            Table {
                name: "fixture root".into(),
                rows: vec![Row {
                    judgments: vec![Judgment([7, 2, 94, 0, 0])],
                    results: vec![result(1, 1, 2), result(22, 0, 0)],
                }],
            },
            Table {
                name: "fixture quest".into(),
                rows: vec![
                    Row {
                        judgments: vec![Judgment([1, 2, 20, 0, 0])],
                        results: vec![result(13, 2, 12), result(11, 1058, 0), result(2, 2, 88)],
                    },
                    Row {
                        judgments: vec![Judgment([1, 2, 88, 0, 0])],
                        results: vec![result(13, 2, 13), result(22, 0, 0)],
                    },
                ],
            },
        ],
    }))
}

#[test]
fn phase1_never_grants_and_phase2_consumes_once_then_repeats() {
    let mut control = fixture();
    let mut world = ddstory::World::default();
    world.set_quest_bit(2, 20, true); // Explicit observed-rescue fixture.
    world.active_quest = 2; // Continue's b field, not c (=zero).
    let key = RowKey { table: 1, row: 0 };
    let selected = control
        .begin(&Context {
            world: &world,
            npc: 304,
            talk_counter: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(selected.row, key);
    assert_eq!(selected.conversation, 12);
    assert!(!world.quest_bit(2, 88));
    assert!(!control.disabled(key));
    assert!(control
        .begin(&Context {
            world: &world,
            npc: 304,
            talk_counter: None
        })
        .is_err());
    assert!(control.complete(RowKey { table: 1, row: 1 }).is_none());
    assert_eq!(
        control.complete(key).unwrap(),
        vec![
            Effect::Item {
                item: 1058,
                count: 1,
                level: 0
            },
            Effect::QuestBit {
                quest: 2,
                bit: 88,
                value: true
            }
        ]
    );
    assert!(control.disabled(key));
    assert!(control.complete(key).is_none());
    world.set_quest_bit(2, 88, true); // Host applies the actual returned effect.
    for _ in 0..3 {
        let selected = control
            .begin(&Context {
                world: &world,
                npc: 304,
                talk_counter: None,
            })
            .unwrap()
            .unwrap();
        assert_eq!(selected.conversation, 13);
        assert!(control.complete(selected.row).unwrap().is_empty());
        assert!(!control.disabled(selected.row));
    }
}

#[test]
fn abort_retries_without_reward_and_clear_else_disable_survives_cleanup() {
    let mut control = fixture();
    let mut world = ddstory::World::default();
    world.set_quest_bit(2, 20, true);
    let selected = control
        .begin(&Context {
            world: &world,
            npc: 304,
            talk_counter: None,
        })
        .unwrap()
        .unwrap();
    control.abort();
    assert!(control.complete(selected.row).is_none());
    assert!(control
        .begin(&Context {
            world: &world,
            npc: 304,
            talk_counter: None
        })
        .unwrap()
        .is_some());
    control.abort();
    world.set_quest_bit(2, 94, true);
    assert!(control
        .begin(&Context {
            world: &world,
            npc: 304,
            talk_counter: None
        })
        .unwrap()
        .is_none());
    assert!(control.disabled(RowKey { table: 0, row: 0 }));
    control.abort();
    world.set_quest_bit(2, 94, false);
    assert!(control
        .begin(&Context {
            world: &world,
            npc: 304,
            talk_counter: None
        })
        .unwrap()
        .is_none());
}

#[test]
fn installed_reynard_selects_actual_quest_resource_and_rewards_only_read_text() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP: install unavailable");
        return;
    }
    let data = InstallData::open(native, "eng").unwrap();
    let mut world = ddstory::World::default();
    world.stage_no = 100;
    world.set_sce_no(1410, false);
    world.set_quest_bit(2, 95, true); // Explicit quest-start fixture.
    world.set_quest_bit(2, 20, true); // Explicit rescue observation fixture.
    let table = data.quest_control(304).unwrap().unwrap();
    assert_eq!(table.tables[3].rows[0].results[0].0[2], 12);
    let mut runtime = NpcRuntime::default();
    let events = runtime
        .talk_quest(
            &data,
            &Story {
                world: &world,
                equip_menus: false,
            },
            1,
            304,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(
        matches!(events.first(), Some(TalkEvent::Line { player: 1, npc: 304, conversation: 12, text, .. }) if !text.is_empty())
    );
    assert!(runtime
        .talk_quest(
            &data,
            &Story {
                world: &world,
                equip_menus: false
            },
            0,
            304,
            None
        )
        .is_err());
    assert!(!world.quest_bit(2, 88));
    let mut effects = Vec::new();
    for _ in 0..50 {
        let events = runtime.talk_next(1).unwrap();
        for event in &events {
            if let TalkEvent::QuestEffects {
                player,
                npc,
                effects: applied,
            } = event
            {
                assert_eq!((*player, *npc), (1, 304));
                effects.extend(applied.clone());
            }
        }
        if events.iter().any(|event| matches!(event, TalkEvent::End { .. })) {
            break;
        }
    }
    assert_eq!(
        effects,
        vec![
            Effect::Item {
                item: 1058,
                count: 1,
                level: 0
            },
            Effect::QuestBit {
                quest: 2,
                bit: 88,
                value: true
            }
        ]
    );
    assert!(runtime.talk_next(1).is_err());
    world.set_quest_bit(2, 88, true);
    let events = runtime
        .talk_quest(
            &data,
            &Story {
                world: &world,
                equip_menus: false,
            },
            0,
            304,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(matches!(
        events.first(),
        Some(TalkEvent::Line {
            player: 0,
            conversation: 13,
            ..
        })
    ));
}
