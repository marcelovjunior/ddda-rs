//! Prints the skill and augment catalogue as tab-separated rows.
//! Usage: cargo run -p ddskill --example catalogue_dump -- <nativePC> [DDDA.exe] [lang]
//! The exe defaults to `<nativePC>\..\DDDA.exe`, the language to `eng`.
use ddskill::catalogue::{Catalogue, ExeTables, Text};
use ddskill::skill_list::SkillList;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let native = PathBuf::from(args.get(1).expect("usage: catalogue_dump <nativePC> [DDDA.exe] [lang]"));
    let exe_path = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| native.parent().unwrap().join("DDDA.exe"));
    let lang = args.get(3).map(String::as_str).unwrap_or("eng");
    let vfs = ddvfs::Vfs::open(&native).expect("open install");
    let exe = ExeTables::from_exe(&std::fs::read(&exe_path).expect("read DDDA.exe")).expect("exe tables");
    let list = SkillList::from_bytes(
        &vfs.read(SkillList::PATH, mtarc::type_hash(SkillList::RESOURCE_CLASS)).unwrap(),
    )
    .unwrap();
    let gmd = mtarc::type_hash("rGUIMessage");
    let read = |p: &str| vfs.read(&format!("{p}_{lang}"), gmd).expect(p);
    let files = [Text::SKILL_NAME, Text::SKILL_COMMAND, Text::ABILITY_NAME, Text::ABILITY_INFO].map(read);
    let parsed: Vec<_> = files.iter().map(|b| mtgmd::MessageFile::parse(b).unwrap()).collect();
    let text = Text {
        skill_name: &parsed[0].messages,
        skill_command: &parsed[1].messages,
        ability_name: &parsed[2].messages,
        ability_info: &parsed[3].messages,
    };
    let cat = Catalogue::build(&exe, &list, &text).expect("build");
    println!("skill\tkind\tnames\tcost/rank\tvocations\tweapons\taction\tclass\tparam\tmotion list");
    for s in &cat.skills {
        let names: Vec<_> = s.levels.iter().map(|l| l.name.as_str()).collect();
        let costs: Vec<_> = s
            .levels
            .iter()
            .filter_map(|l| Some(format!("{}/{}", l.cost?, l.rank?)))
            .collect();
        println!(
            "{}\t{:?}\t{}\t{}\t{:?}\t{:?}\t{}\t{}\t{}\t{}",
            s.id,
            s.kind,
            names.join(" / "),
            costs.join(" "),
            s.vocations,
            s.weapons,
            s.action.map_or("-".into(), |a| format!("{a:#010x}")),
            s.action_class.as_deref().unwrap_or("-"),
            s.action_param.map_or("-".into(), |p| format!("{p:#x}")),
            s.motion_list.as_ref().map_or("-".into(), |m| format!("{:#x} {}", m.bank, m.path)),
        );
    }
    println!("augment\tname\tvocation\tcost/rank\tdescription");
    for a in &cat.augments {
        println!(
            "{}\t{}\t{:?}\t{}/{}\t{}",
            a.id,
            a.name.as_deref().unwrap_or("-"),
            a.vocation,
            a.cost,
            a.rank,
            a.description.as_deref().unwrap_or("-").replace("\r\n", " "),
        );
    }
}
