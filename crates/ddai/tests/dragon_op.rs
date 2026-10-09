//! Installed Dragon_OP graph, driven by explicit synthetic actor feedback.
//! These tests validate the FSM bridge, not the dragon's action simulation.
use ddai::think::{Actor, Inputs, Request, Thinker};
use mtfsm::{Cluster, Graph, Parameter, Process, Record, State};
use mtxfs::Value;
use std::{path::PathBuf, sync::Arc};

fn installed() -> Option<Arc<Graph>> {
    let native = std::env::var_os("DDDA_NATIVE_PC")
        .map(PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !native.is_dir() {
        eprintln!("SKIP Dragon_OP: install absent");
        return None;
    }
    let vfs = ddvfs::Vfs::open(native).unwrap();
    let bytes = vfs
        .read(r"AI\FSM\Enemy\st200\quest\Dragon_OP", mtarc::type_hash("rAIFSM"))
        .unwrap();
    Some(Arc::new(Graph::parse(&bytes).unwrap()))
}

fn actor() -> Inputs {
    Inputs {
        stage: Some(200),
        actor: Some(Actor::default()),
        ..Inputs::default()
    }
}

#[test]
fn installed_dragon_waits_for_actor_and_area_feedback() {
    let Some(graph) = installed() else { return };
    let mut thinker = Thinker::new("Dragon_OP", graph).unwrap();
    let mut inputs = actor();
    let mut requests = Vec::new();
    for _ in 0..180 {
        requests.extend(thinker.frame(&inputs).unwrap());
    }
    assert_eq!(thinker.current_states(), [4]);
    assert!(requests.contains(&Request::Action { action: 52 }));
    assert!(requests.contains(&Request::Gsf {
        no: 282,
        value: false
    }));
    assert!(!requests.iter().any(|r| matches!(r, Request::QuestFlag { .. })));

    inputs.actor.as_mut().unwrap().action_done = true;
    for _ in 0..30 {
        requests.extend(thinker.frame(&inputs).unwrap());
    }
    assert_eq!(thinker.current_states(), [0]);
    // Landing waits for AreaStayFlag[0], independent of action completion.
    inputs.area_stay = 0x8000_0000;
    for _ in 0..10 {
        requests.extend(thinker.frame(&inputs).unwrap());
    }
    assert_eq!(thinker.current_states(), [7]);
    inputs.motion_frame = 10.0;
    for _ in 0..5 {
        requests.extend(thinker.frame(&inputs).unwrap());
    }
    assert_eq!(thinker.current_states(), [8]);
    // Motion End is separate from the action controller's completion.
    assert!(!requests.contains(&Request::QuestFlag {
        quest: 0,
        bit: 20,
        value: true
    }));
    inputs.motion_ended = true;
    for _ in 0..250 {
        requests.extend(thinker.frame(&inputs).unwrap());
    }
    assert_eq!(thinker.current_states(), [6]);
    // Kill requests removal of the FSM (0x10), not Finish's bit 0.
    assert_eq!(thinker.fields.sys_condition & 0x10, 0x10);
    let bits: Vec<_> = requests
        .iter()
        .filter_map(|r| match r {
            Request::QuestFlag {
                quest: 0,
                bit,
                value: true,
            } => Some(*bit),
            _ => None,
        })
        .collect();
    assert_eq!(bits, [1, 20, 21, 5]);
    assert!(requests.contains(&Request::LayoutFlag {
        stage: 200,
        no: 80,
        value: true
    }));
    assert!(requests.contains(&Request::Gsf { no: 282, value: true }));
    assert!(requests.iter().any(
        |r| matches!(r, Request::ActionEx { action: 10, signed, unsigned, floats }
        if signed[0] == 2 && *unsigned == [0; 4] && floats[3..6] == [115.0, 1.0, 300.0])
    ));
}

fn process(name: &str, fields: Vec<(&str, Vec<Value>)>) -> Process {
    Process {
        container: name.as_bytes().to_vec(),
        category: vec![],
        parameter: Some(Record {
            class_hash: 0,
            class_name: None,
            fields: fields
                .into_iter()
                .map(|(n, v)| (n.into(), v.into_iter().map(Parameter::Scalar).collect()))
                .collect(),
        }),
    }
}

fn commands(processes: Vec<Process>, inputs: &Inputs) -> Vec<Request> {
    let graph = Graph {
        owner: b"cAILevelAbstractFSM".to_vec(),
        attribute: 0,
        conditions: vec![],
        root: Some(Cluster {
            id: 0,
            owner_node_unique_id: 0,
            initial_state: 0,
            states: vec![State {
                id: 0,
                unique_id: 0,
                owner_id: 0,
                name: b"synthetic".to_vec(),
                setting: 0,
                user_attribute: 0,
                from_all: None,
                links: vec![],
                processes,
                subcluster: None,
            }],
        }),
    };
    Thinker::new("synthetic", Arc::new(graph))
        .unwrap()
        .frame(inputs)
        .unwrap()
}

#[test]
fn generic_world_writes_enforce_exe_bounds() {
    let gsf = |no| {
        process(
            "Set GSFFlag",
            vec![("FlagNo", vec![Value::U(no)]), ("Bool", vec![Value::Bool(false)])],
        )
    };
    let quest = |quest, bit| {
        process(
            "Set QuestFlag",
            vec![
                ("QuestNo", vec![Value::U(quest)]),
                ("BitNo", vec![Value::U(bit)]),
                ("Bool", vec![Value::Bool(true)]),
            ],
        )
    };
    let lot = |no| {
        process(
            "Set LotFlag",
            vec![("FlagNo", vec![Value::U(no)]), ("Bool", vec![Value::Bool(true)])],
        )
    };
    assert_eq!(
        commands(
            vec![
                gsf(1535),
                gsf(1536),
                quest(127, 95),
                quest(128, 0),
                quest(0, 96),
                lot(127),
                lot(128)
            ],
            &actor()
        ),
        [
            Request::Gsf {
                no: 1535,
                value: false
            },
            Request::QuestFlag {
                quest: 127,
                bit: 95,
                value: true
            },
            Request::LayoutFlag {
                stage: 200,
                no: 127,
                value: true
            },
        ]
    );
    let mut inputs = actor();
    inputs.stage = Some(100);
    assert_eq!(
        commands(vec![lot(255), lot(256)], &inputs),
        [Request::LayoutFlag {
            stage: 100,
            no: 255,
            value: true
        }]
    );
    inputs.stage = None;
    assert!(commands(vec![lot(0)], &inputs).is_empty());
}

#[test]
fn generic_extended_action_preserves_every_lane_and_target_offset() {
    let action = || {
        process(
            "CtrlActionEx",
            vec![
                ("アクション番号", vec![Value::S(7)]),
                (
                    "S32パラメータ",
                    [-1, 2, -3, 4].into_iter().map(Value::S).collect(),
                ),
                (
                    "U32パラメータ",
                    [u32::MAX as u64, 6, 7, 8].into_iter().map(Value::U).collect(),
                ),
                (
                    "F32パラメータ",
                    (0..8).map(|n| Value::F(vec![n as f64 + 0.25])).collect(),
                ),
            ],
        )
    };
    let target = || {
        process(
            "SetTargetPos",
            vec![("ターゲット座標", vec![Value::F(vec![1.0, 2.0, 3.0, 0.0])])],
        )
    };
    let mut inputs = actor();
    inputs.world_offset_on = true;
    inputs.world_offset = [10.0, 20.0, 30.0];
    assert_eq!(
        commands(vec![target(), action()], &inputs),
        [
            Request::TargetPos {
                pos: [11.0, 22.0, 33.0]
            },
            Request::ActionEx {
                action: 7,
                signed: [-1, 2, -3, 4],
                unsigned: [u32::MAX, 6, 7, 8],
                floats: [0.25, 1.25, 2.25, 3.25, 4.25, 5.25, 6.25, 7.25]
            }
        ]
    );
    inputs.world_offset_on = false;
    assert_eq!(
        commands(vec![target()], &inputs),
        [Request::TargetPos { pos: [1.0, 2.0, 3.0] }]
    );
    inputs.actor = None;
    assert!(commands(vec![action()], &inputs).is_empty());
}
