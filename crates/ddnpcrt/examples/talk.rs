//! Print an NPC's base talk set with lines. usage: talk <nativePC> <npc no>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let vfs = ddvfs::Vfs::open(&a[1]).unwrap();
    let n: u32 = a[2].parse().unwrap();
    let base = format!(r"id\npc_wind\npc_base_talk\n{n:03}");
    let mss = vfs.read(&base, mtarc::type_hash("rMsgSet")).unwrap();
    let set = ddnpc::MessageSet::parse(&mss).unwrap();
    let gmd = vfs.read(&format!("{base}_eng"), mtarc::type_hash("rGUIMessage")).unwrap();
    let g = mtgmd::MessageFile::parse(&gmd).unwrap();
    for c in &set.conversations {
        println!(
            "conv {} npc {} season {}/{}/{} quest {}:{} friend {}..{} gsf {} choices {:?} jumps {:?}",
            c.number, c.npc_id, c.season, c.season_sequence, c.season_sequence_type, c.quest_number,
            c.quest_flag, c.friendship, c.friendship_max, c.global_flag, c.active_choices(), c.choice_jumps
        );
        for (i, &m) in c.active_messages().iter().enumerate() {
            println!("   [{i}] gsf {} {}: {}", c.global_flags[i], m, g.message(m as u32).unwrap_or("?").replace('\n', " / "));
        }
        for &s in c.active_choices() {
            println!("   choice {}: {}", s, g.message(s as u32).unwrap_or("?").replace('\n', " / "));
        }
    }
}
