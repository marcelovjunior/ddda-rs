//! Installed route contracts, independent of the viewer's demonstration driver.
use ddstory::{data::{Data, InstallData}, scenario::Scenarios, script::Script, story::Quest};

fn install() -> Option<InstallData> {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() { eprintln!("SKIP: DDDA install absent"); return None; }
    Some(InstallData::open(native, None).unwrap())
}

#[test]
fn encampment_voice_requires_the_encampment_and_an_actual_player_in_its_volume() {
    for stage in [100, 210] {
        for inside in [false, true] {
            let Some(data) = install() else { return };
            let scenarios = Scenarios::load(&data.scenarios(210).unwrap()).unwrap();
            let volume = ddstory::route::Waypoint { name: "encampment entry", stage: 210,
                entry: (4, 3), until: None, interact: false, npcs: vec![] };
            let (pos, yaw) = volume.place(&scenarios).unwrap();
            let mut script = Script::new(r"quest\q0003_b00", data.fsm(r"quest\q0003_b00").unwrap()).unwrap();
            script.owner.quest = Some(3 << 16);
            let ctrl = ddstory::qct::Ctrl::new(data.qct(r"etc\questCtrl\q0003_b00").unwrap().unwrap());
            let mut story = ddstory::Story::new_game(data);
            story.world.stage_no = stage;
            story.world.set_sce_no(1450, true);
            story.stage.stage_no = stage;
            story.stage.scenarios = scenarios;
            story.quests.push(Quest { id: 3 << 16, flags: 1, script: Some(script), ctrl: Some(ctrl), log: vec![] });
            let mut heard_voice = false;
            for _ in 0..120 {
                let requests = story.tick(&ddstory::Inputs {
                    // The second seat alone occupies the authored volume.
                    players: vec![ddstory::Player { pos: [0.0, -100_000.0, 0.0], yaw: 0.0 },
                        ddstory::Player { pos: if inside { pos } else { [0.0, -100_000.0, 0.0] }, yaw }],
                    ..Default::default()
                }, &ddstory::encounter::EncounterFacts::default()).unwrap();
                heard_voice |= requests.iter().any(|r| matches!(r,
                    ddstory::Request::Message { quest: 3, mes_id: 45, .. }));
                assert!(!requests.iter().any(|r| matches!(r, ddstory::Request::PlayEvent { .. })),
                    "arrival alone cannot manufacture the later cyclops/report event");
            }
            assert_eq!(heard_voice, stage == 210 && inside);
            assert_eq!(story.world.sce_no, 1450);
        }
    }
}
