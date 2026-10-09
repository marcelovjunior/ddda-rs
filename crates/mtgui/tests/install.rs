use std::collections::BTreeSet;
#[test]
#[ignore = "requires DDDA_NATIVE_PC, reads player's install"]
fn all_instances() {
    let vfs = ddvfs::Vfs::open(std::env::var("DDDA_NATIVE_PC").expect("set DDDA_NATIVE_PC")).unwrap();
    let hash = mtarc::type_hash("rGUI");
    let mut instances = 0;
    let mut nodes = 0;
    let mut refs = 0;
    let mut missing = BTreeSet::new();
    let mut errors = Vec::new();
    let mut messages = 0;
    for path in vfs.paths_of_type(hash) {
        for loc in vfs.locate(path, hash).unwrap() {
            instances += 1;
            let bytes = vfs.read_at(loc).unwrap();
            match mtgui::Gui::parse(&bytes) {
                Err(e) => errors.push(format!("{path}: {e}")),
                Ok(g) => {
                    nodes += g.nodes.len();
                    for r in g.resources {
                        if let Some(p) = r.path {
                            refs += 1;
                            if r.class == "rGUIMessage" {
                                messages += 1;
                            }
                            if vfs.locate(p, mtarc::type_hash(r.class)).is_none() {
                                missing.insert(format!("{} {p}", r.class));
                            }
                        }
                    }
                }
            }
        }
    }
    println!("paths={} instances={instances} nodes={nodes} resource_refs={refs} message_refs={messages} missing={} parse_errors={}",vfs.paths_of_type(hash).count(),missing.len(),errors.len());
    for e in &errors {
        println!("ERROR {e}");
    }
    for m in &missing {
        println!("MISSING {m}");
    }
    assert!(errors.is_empty());
    assert!(missing.is_empty());
    assert!(instances > 0);
}

#[test]
#[ignore = "requires DDDA_NATIVE_PC, reads player's install"]
fn option_runtime_message_links() {
    // Resource load traced from 0x006a6160; language suffix from 0x00607610
    // (DDDA.exe 2017-12-13 Steam). GUI's own message table is empty.
    let vfs = ddvfs::Vfs::open(std::env::var("DDDA_NATIVE_PC").expect("set DDDA_NATIVE_PC")).unwrap();
    let mut instances = 0;
    let mut messages = 0;
    for language in ["jpn", "eng", "fre", "spa", "ger", "ita", "zht"] {
        let path = format!("id/ddn/message/pause/pause_option/pause_option_{language}");
        let locations = vfs
            .locate(&path, mtarc::type_hash("rGUIMessage"))
            .unwrap_or_else(|| panic!("missing {path}"));
        for loc in locations {
            let b = vfs.read_at(loc).unwrap();
            let gmd = mtgmd::MessageFile::parse(&b).unwrap();
            instances += 1;
            messages += gmd.messages.len();
        }
    }
    println!("option runtime messages: 7 paths, {instances} occurrences, {messages} messages");
}
