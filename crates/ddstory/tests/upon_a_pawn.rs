//! q0002 control-table regressions. Game resources are read at run time.
use ddquest::qct::{Judgment, Outcome, QuestCtrl, Row, Table};
use ddstory::{
    data::{Data, InstallData},
    qct::Ctrl,
    scripted::ScriptedHost,
    story::StageState,
    Env, World,
};
use std::sync::Arc;

fn run(table: Arc<QuestCtrl>, world: &mut World) -> Result<(), ddstory::Error> {
    let mut stage = StageState::default();
    let host = ScriptedHost::default();
    let mut out = vec![];
    let mut cmds = vec![];
    let mut env = Env {
        world,
        stage: &mut stage,
        host: &host,
        time: Default::default(),
        out: &mut out,
        cmds: &mut cmds,
        events: None,
    };
    Ctrl::new(table).frame(&mut env, 2)
}

#[test]
fn npc_death_judgments_select_the_requested_bank_and_polarity() {
    for apparent in [false, true] {
        for permanent in [false, true] {
            for selector in [0, 1, 7] {
                for kind in [18, 19] {
                    let mut world = World::default();
                    world.npc_app_die.set(304, apparent);
                    world.npc_die.set(304, permanent);
                    let row = Row {
                        judgments: vec![Judgment([kind, 304, selector, 0, 0])],
                        results: vec![Outcome([4, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0])],
                    };
                    let table = Arc::new(QuestCtrl {
                        alloc_size: 0,
                        tables: vec![Table {
                            name: "test".into(),
                            rows: vec![row],
                        }],
                        trailer: 0,
                    });
                    run(table, &mut world).unwrap();
                    let dead = if selector == 0 { apparent } else { permanent };
                    assert_eq!(world.gsf.get(42), dead == (kind == 18));
                }
            }
        }
    }
}

#[test]
fn pawn_message_busy_getter_includes_missing_gui_and_exact_capacity() {
    for slots in [None, Some(0), Some(1), Some(2), Some(3), Some(4)] {
        let mut world = World::default();
        let mut stage = StageState {
            pawn_message_slots: slots,
            ..Default::default()
        };
        let host = ScriptedHost::default();
        let mut out = vec![];
        let mut cmds = vec![];
        let env = Env {
            world: &mut world,
            stage: &mut stage,
            host: &host,
            time: Default::default(),
            out: &mut out,
            cmds: &mut cmds,
            events: None,
        };
        let variable = ddstory::cond::Var {
            owner: "cFSMOrder",
            property: "IsDispCmcMess",
            index: None,
        };
        assert_eq!(
            ddstory::vars::var(&Default::default(), &env, &variable).unwrap(),
            ddstory::cond::Value::Bool(slots != Some(3))
        );
    }
}

#[test]
fn installed_q0002_death_cleanup_waits_until_leaving_the_field() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let data = InstallData::open(native, None).unwrap();
    let table = data.qct(r"etc\questCtrl\q0002_b00").unwrap().unwrap();
    for field in [true, false] {
        for bank in [0, 1] {
            let mut world = World::default();
            world.stage_no = if field { 100 } else { 200 };
            world.set_lot(100, 7, true);
            if bank == 0 {
                world.npc_app_die.set(304, true);
            } else {
                world.npc_die.set(304, true);
            }
            run(table.clone(), &mut world).unwrap();
            assert_eq!(world.lot(100, 7), field);
            assert!(
                !world.quest_bit(2, 94),
                "a death cleanup must not clear the quest"
            );
        }
    }
}

#[test]
fn ev15_preparation_recruits_once_before_playback() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    for row in [22, 23, 24] {
        let data = InstallData::open(&native, None).unwrap();
        let mut story = ddstory::Story::new_game(data);
        story.stage.stage_no = 200;
        story.world.stage_no = 200;
        story.stage.event.pending = Some(ddstory::story::EventRequest {
            row,
            stage: 200,
            event: 15,
            sce_no: None,
            set_pos_type: 0,
            start_pos: 0,
        });
        let host = ddstory::encounter::EncounterFacts::default();
        let requests = story.tick(&Default::default(), &host).unwrap();
        let recruitment = requests
            .iter()
            .position(|r| matches!(r, ddstory::Request::StoryPawn { stock: 14, slot: 1 }));
        assert_eq!(recruitment.is_some(), row == 23, "dispatch is by event-table row");
        if let Some(recruitment) = recruitment {
            let play = requests
                .iter()
                .position(|r| matches!(r, ddstory::Request::PlayEvent { .. }))
                .unwrap();
            assert!(recruitment < play);
        }
        assert!(!story
            .tick(&Default::default(), &host)
            .unwrap()
            .iter()
            .any(|r| matches!(r, ddstory::Request::StoryPawn { .. })));
    }
}

/// Run the installed quest and its exit-volume script, with an explicit
/// fact registry. No enemy is declared dead and no timed host sets flags.
#[test]
fn installed_exit_script_can_clear_q0002_without_rescuing_the_victim() {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() {
        eprintln!("SKIP: DDDA install absent");
        return;
    }
    let data = InstallData::open(native, None).unwrap();
    let mut script =
        ddstory::script::Script::new(r"quest\q0002_b00", data.fsm(r"quest\q0002_b00").unwrap()).unwrap();
    script.owner.quest = Some(2 << 16);
    let ctrl = Ctrl::new(data.qct(r"etc\questCtrl\q0002_b00").unwrap().unwrap());
    let mut story = ddstory::Story::new_game(data);
    story.stage.stage_no = 100;
    story.world.stage_no = 100;
    story.world.set_sce_no(1400, true);
    story.world.gsf.set(381, true);
    story.quests.push(ddstory::story::Quest {
        id: 2 << 16,
        flags: 1,
        script: Some(script),
        ctrl: Some(ctrl),
        log: vec![],
    });
    let host = ddstory::encounter::EncounterFacts::default();
    let mut events = vec![];
    let mut tick = |story: &mut ddstory::Story<InstallData>| {
        let requests = story
            .tick(
                &ddstory::Inputs {
                    events: std::mem::take(&mut events),
                    ..Default::default()
                },
                &host,
            )
            .unwrap();
        // Test presentation adapter: acknowledge an explicitly requested
        // camera. It does not report combat or quest success.
        if requests
            .iter()
            .any(|r| matches!(r, ddstory::Request::Camera { .. }))
        {
            events.push(ddstory::HostEvent::CameraFinished);
        }
        requests
    };
    for _ in 0..300 {
        tick(&mut story);
    }
    assert!(story.world.quest_bit(2, 95));
    assert!(!story.world.quest_bit(2, 10));
    assert!(
        !story.world.quest_bit(2, 94),
        "elapsed time cannot clear the quest"
    );
    let exit = r"scr\st100\fsm\q0002_event2";
    story
        .tasks
        .push(ddstory::script::Script::new(exit, story.data.fsm(exit).unwrap()).unwrap());
    let mut reward_count = 0;
    for _ in 0..120 {
        reward_count += tick(&mut story)
            .iter()
            .filter(|r| matches!(r, ddstory::Request::Reward { quest: 2, .. }))
            .count();
        if story.world.sce_no == 1450 {
            break;
        }
    }
    assert!(story.world.quest_bit(2, 10));
    assert!(story.world.quest_bit(2, 94));
    assert!(!story.world.quest_bit(2, 5), "rescue encounter was never entered");
    assert_eq!(
        story.world.sce_no, 1400,
        "quest cleanup waits until the party leaves stage 100"
    );
    // Isolated quest fixture: the host has completed the stage handoff.
    // Other stages' scenarios are exercised by their own integration tests.
    story.stage.stage_no = 210;
    story.world.stage_no = 210;
    for _ in 0..10 {
        tick(&mut story);
    }
    assert_eq!(story.world.sce_no, 1450);
    assert!(story.quest(3).is_some(), "the exit script boots the next quest");
    assert_eq!(reward_count, 1);
}
