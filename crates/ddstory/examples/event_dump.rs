//! Play an event headless and print what changes: camera cuts, actor
//! motions, fade, subtitles and sound cues.
//!
//! cargo run -p ddstory --example event_dump -- <nativePC> <DDDA.exe> <row> [language] [sex] [hour]
use ddstory::data::InstallData;
use ddstory::event::{EventConfig, EventPlayer};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = InstallData::open(&a[1], Some(std::path::Path::new(&a[2]))).unwrap();
    let row: usize = a[3].parse().unwrap();
    let arg = |i: usize, d: u32| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let cfg = EventConfig { language: arg(4, 1) as usize, sex: arg(5, 0) as u8, hour: arg(6, 8), stage: 200 };
    let mut p = EventPlayer::open(&data, row, cfg).unwrap();
    println!("{} end frame {}", p.path, p.end_frame);
    for (prop, _, path) in p.resources() {
        println!("  resource {prop} = {path}");
    }
    let (mut cut, mut sub, mut fade_on, mut motions) = (None, None, None, vec![]);
    for tick in 0.. {
        let f = p.step(1.0).unwrap();
        let fr = f.frame.map(|x| format!("{x}")).unwrap_or("-".into());
        if let Some(c) = &f.camera {
            if cut != Some(c.cut) {
                cut = Some(c.cut);
                let pose = c.pose.map(|q| format!("eye {:?} target {:?} fov {:.1}", q.position, q.target, q.fov_degrees));
                println!("tick {tick} frame {fr}: cut {} (local {}) {}", c.cut, c.frame, pose.unwrap_or_default());
                for a in &c.adjust {
                    println!("    adjust {a:?}");
                }
            }
        }
        let m: Vec<(i32, bool)> = f.actors.iter().map(|a| (a.motion_no, a.draw)).collect();
        if m != motions {
            println!(
                "tick {tick} frame {fr}: actors {}",
                f.actors.iter().map(|a| format!("{}#{} draw {}", a.name, a.motion_no, a.draw)).collect::<Vec<_>>().join(", ")
            );
            motions = m;
        }
        let on = f.fade.map(|c| c[3] > 0.0);
        if on != fade_on {
            println!("tick {tick} frame {fr}: fade {:?}", f.fade);
            fade_on = on;
        }
        let s = f.subtitle.as_ref().map(|s| (s.label.clone(), s.text.clone()));
        if s != sub {
            println!("tick {tick} frame {fr}: subtitle {:?}", f.subtitle);
            sub = s;
        }
        for c in &f.cues {
            println!("tick {tick} frame {fr}: cue {c:?}");
        }
        if f.finished {
            println!("tick {tick} frame {fr}: finished");
            break;
        }
    }
}
