//! q0002 control-table regressions. Game resources are read at run time.
use ddquest::qct::{Judgment, Outcome, QuestCtrl, Row, Table};
use ddstory::{data::{Data, InstallData}, qct::Ctrl, scripted::ScriptedHost, story::StageState, Env, World};
use std::sync::Arc;

fn run(table: Arc<QuestCtrl>, world: &mut World) -> Result<(), ddstory::Error> {
    let mut stage = StageState::default();
    let host = ScriptedHost::default();
    let mut out = vec![];
    let mut cmds = vec![];
    let mut env = Env { world, stage: &mut stage, host: &host, time: Default::default(), out: &mut out, cmds: &mut cmds, events: None };
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
                    let row = Row { judgments: vec![Judgment([kind, 304, selector, 0, 0])], results: vec![Outcome([4, 42, 0, 0, 0, 0, 0, 0, 0, 0, 0])] };
                    let table = Arc::new(QuestCtrl { alloc_size: 0, tables: vec![Table { name: "test".into(), rows: vec![row] }], trailer: 0 });
                    run(table, &mut world).unwrap();
                    let dead = if selector == 0 { apparent } else { permanent };
                    assert_eq!(world.gsf.get(42), dead == (kind == 18));
                }
            }
        }
    }
}

#[test]
fn installed_q0002_death_cleanup_waits_until_leaving_the_field() {
    let native = std::env::var_os("DDDA_NATIVE_PC").map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    if !native.exists() { eprintln!("SKIP: DDDA install absent"); return; }
    let data = InstallData::open(native, None).unwrap();
    let table = data.qct(r"etc\questCtrl\q0002_b00").unwrap().unwrap();
    for field in [true, false] {
        for bank in [0, 1] {
            let mut world = World::default();
            world.stage_no = if field { 100 } else { 200 };
            world.set_lot(100, 7, true);
            if bank == 0 { world.npc_app_die.set(304, true); }
            else { world.npc_die.set(304, true); }
            run(table.clone(), &mut world).unwrap();
            assert_eq!(world.lot(100, 7), field);
            assert!(!world.quest_bit(2, 94), "a death cleanup must not clear the quest");
        }
    }
}
