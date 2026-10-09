//! Drive the opening with the scripted host and print the SceNo ladder,
//! the quests booted and the requests.
//!
//! cargo run -p ddstory --example opening -- <nativePC> <DDDA.exe>
use ddstory::data::InstallData;
use ddstory::scripted::ScriptedHost;
use ddstory::Story;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = InstallData::open(&a[1], Some(std::path::Path::new(&a[2]))).unwrap();
    let mut story = Story::new_game(data);
    let mut host = ScriptedHost::opening();
    let route = ddstory::route::opening();
    match ddstory::route::run(&mut story, &mut host, &route, 240, 40_000, |s| s.quest(2).is_some()) {
        Ok(t) => {
            for (f, r) in &t.requests {
                match r {
                    ddstory::Request::Order { .. } | ddstory::Request::Scenario { .. } => {}
                    other => println!("frame {f}: {other:?}"),
                }
            }
            println!("SceNo ladder {:?}", t.sce_ladder);
            println!("quests booted {:?}", t.boots);
            for q in [0usize, 1, 2, 28, 31] {
                let w = story.world.quest_bits(q).unwrap().words();
                println!("quest {q}: {:08x} {:08x} {:08x}", w[0], w[1], w[2]);
            }
        }
        Err(e) => println!("ERROR {e}"),
    }
}
