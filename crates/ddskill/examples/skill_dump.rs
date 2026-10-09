//! Prints `etc\skill\skill` (rSkillList) as tab-separated rows.
//! Usage: cargo run -p ddskill --example skill_dump -- <nativePC>
use ddskill::skill_list::SkillList;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: skill_dump <nativePC>");
    let vfs = ddvfs::Vfs::open(&dir).expect("open install");
    let bytes = vfs
        .read(SkillList::PATH, mtarc::type_hash(SkillList::RESOURCE_CLASS))
        .expect("read etc\\skill\\skill");
    let list = SkillList::from_bytes(&bytes).expect("parse");
    println!("kind\tid\tlevel\tcost\trank");
    for s in &list.core {
        println!("core\t{}\t\t{}\t{}", s.skill_id, s.cost, s.rank);
    }
    for s in &list.weapon {
        println!("weapon\t{}\t{}\t{}\t{}", s.skill_id, s.level, s.cost, s.rank);
    }
    for a in &list.augments {
        println!("augment\t{}\t\t{}\t{}", a.augment_id, a.cost, a.rank);
    }
}
