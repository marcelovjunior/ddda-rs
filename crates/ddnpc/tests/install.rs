use ddnpc::{Ledger, MessageSerial, MessageSet};

#[test]
fn installed_corpus() {
    let explicit = std::env::var_os("DDDA_NATIVE_PC");
    let root = explicit
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "E:/SteamLibrary/steamapps/common/DDDA/nativePC".into());
    if !root.exists() {
        assert!(explicit.is_none(), "DDDA_NATIVE_PC does not exist");
        eprintln!("No install; corpus skipped");
        return;
    }
    let vfs = ddvfs::Vfs::open(root).unwrap();
    for class in ["rNpcLedgerList", "rMsgSet", "rMsgSerial"] {
        let hash = mtarc::type_hash(class);
        let mut paths = 0;
        let mut instances = 0;
        let mut records = 0;
        let mut unique = std::collections::HashSet::new();
        let mut distinct_records = 0;
        for path in vfs.paths_of_type(hash) {
            paths += 1;
            for loc in vfs.locate(path, hash).unwrap() {
                let data = vfs.read_at(loc).unwrap();
                let count = match class {
                    "rNpcLedgerList" => Ledger::parse(&data).map(|f| f.entries.len()),
                    "rMsgSet" => MessageSet::parse(&data).map(|f| f.conversations.len()),
                    _ => MessageSerial::parse(&data).map(|f| f.numbers.len()),
                }
                .unwrap_or_else(|e| panic!("{path} in {}: {e}", vfs.archives[loc.archive].display()));
                instances += 1;
                records += count;
                if unique.insert(data) {
                    distinct_records += count;
                }
            }
        }
        assert!(instances > 0, "missing {class}");
        eprintln!("{class}: {paths} paths, {instances} instances, {} distinct payloads, {records} rows ({distinct_records} distinct-payload rows)", unique.len());
    }
    quina_chain(&vfs);
}

// A data-and-executable trace, not a claim that this interaction was observed live.
fn quina_chain(vfs: &ddvfs::Vfs) {
    use mtxfs::{Object, Value, Xfs};
    fn one<'a>(x: &Xfs, o: &'a Object, key: &str) -> &'a Value {
        let values = x.get(o, key).unwrap();
        assert_eq!(values.len(), 1);
        &values[0]
    }
    fn object(v: &Value) -> &Object {
        match v {
            Value::Object(Some(o)) => o,
            _ => panic!("expected object"),
        }
    }
    let read = |path: &str, class: &str| vfs.read(path, mtarc::type_hash(class)).unwrap();
    let d = read("etc/item/npclist", "rNpcLedgerList");
    let ledger = Ledger::parse(&d).unwrap();
    let quina = ledger.entries.iter().find(|n| n.id == 2).unwrap();
    assert_eq!(quina.name, b"Quina");
    assert_eq!((quina.stage, quina.model_type, quina.model_index), (200, 1, 1));
    let lot = mtlot::parse(&read("scr/st100/etc/st100_55m45n_n180", "rLayout")).unwrap();
    let placement = lot
        .iter()
        .find(|r| r.kind == 47 && r.get("mNpcId") == Some(&mtlot::Value::Int(2)))
        .unwrap();
    assert_eq!(placement.id, 0);
    assert_eq!(placement.get("mUse24Schedule"), Some(&mtlot::Value::Int(1)));
    assert_eq!(placement.get("FSMPath"), Some(&mtlot::Value::Str(vec![])));
    assert_eq!(
        placement.placement().unwrap().position,
        [-43144.04, 38557.266, 52742.195]
    );
    let path = ddnpc::schedule_path(100, 3600, 2, 1).unwrap();
    let graph = mtxfs::parse(&read(&path, "rAIFSM")).unwrap();
    assert_eq!(
        one(&graph, &graph.root, "mOwnerObjectName"),
        &Value::Str(b"cAIConsiderFSM_24Schedule".to_vec())
    );
    let cluster = object(one(&graph, &graph.root, "mpRootCluster"));
    let states = graph.get(cluster, "mpNodeList").unwrap();
    let morning = states
        .iter()
        .map(object)
        .find(|s| one(&graph, s, "mId") == &Value::U(1))
        .unwrap();
    assert_eq!(
        one(&graph, morning, "mConditionTrainsitionFromAllId"),
        &Value::U(1)
    );
    let sub = object(one(&graph, morning, "mpSubCluster"));
    let state = object(&graph.get(sub, "mpNodeList").unwrap()[0]);
    let process = object(&graph.get(state, "mpProcessList").unwrap()[0]);
    assert_eq!(
        one(&graph, process, "mContainerName"),
        &Value::Str(b"LoadFSM".to_vec())
    );
    let param = object(one(&graph, process, "mpParameter"));
    let action = "AI\\FSM\\Npc\\st100\\action\\go002_morning";
    assert_eq!(
        one(&graph, param, "FSMPath"),
        &Value::Str(action.as_bytes().to_vec())
    );
    let tree = object(one(&graph, &graph.root, "mpConditionTree"));
    let cond = graph
        .get(tree, "mpTreeList")
        .unwrap()
        .iter()
        .map(object)
        .find(|c| {
            let name = object(one(&graph, c, "mName"));
            one(&graph, name, "mId") == &Value::U(1)
        })
        .unwrap();
    let expr = object(one(&graph, cond, "mpRootNode"));
    assert_eq!(one(&graph, expr, "mOperator"), &Value::U(16)); // AND, see AI.md.
    for (child, (operator, threshold)) in graph
        .get(expr, "mpChildList")
        .unwrap()
        .iter()
        .zip([(8, 36), (5, 72)])
    {
        let c = object(child);
        assert_eq!(one(&graph, c, "mOperator"), &Value::U(operator));
        let args = graph.get(c, "mpChildList").unwrap();
        let variable = object(one(&graph, object(&args[0]), "mVariable"));
        assert_eq!(
            one(&graph, variable, "mPropertyName"),
            &Value::Str(b"GlobalTime".to_vec())
        );
        assert_eq!(one(&graph, object(&args[1]), "mValue"), &Value::S(threshold));
    }
    let action = mtxfs::parse(&read(action, "rAIFSM")).unwrap();
    assert_eq!(
        one(&action, &action.root, "mOwnerObjectName"),
        &Value::Str(b"cAIActionFSMDemo".to_vec())
    );
    let base = "id/npc_wind/npc_base_talk/n002";
    let set = MessageSet::parse(&read(base, "rMsgSet")).unwrap();
    let serial = MessageSerial::parse(&read(base, "rMsgSerial")).unwrap();
    let d = read(&format!("{base}_eng"), "rGUIMessage");
    let text = mtgmd::MessageFile::parse(&d).unwrap();
    let c = set.by_number(7).unwrap();
    assert_eq!(c.npc_id, 2);
    assert_eq!(c.active_messages(), [6]);
    assert_eq!(set.loaded_message_index(7, 0, |_| false), Some(6));
    assert_eq!(text.message(6), Some("Till next time."));
    assert_eq!(serial.numbers[6], 850);
    assert_eq!(serial.numbers.len(), text.messages.len());
    // A second, two-line conversation verifies number is not the array index.
    assert_eq!(set.by_number(76).unwrap().active_messages(), [13, 14]);
    assert_eq!(text.message(14), Some("I worry for you, cos."));
    eprintln!("Quina: ledger -> placement -> 24-hour controller -> morning action; MSS conversations 7/76 -> English GMD, paired MSL lengths checked");
}
