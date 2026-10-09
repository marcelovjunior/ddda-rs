//! Runs the goblin's think tables against a synthetic target and prints
//! what each think picks: `think_trace [table-va-hex] [dist...]`.
use ddai::thinktbl::em0100::{self, Driver};
use ddai::thinktbl::{Host, Slot, Tables};
use ddai::Error;
use std::collections::BTreeMap;

struct Probe {
    action: i32,
    asked: BTreeMap<u32, u32>,
    rolls: u32,
}

impl Host for Probe {
    fn check(&mut self, id: u32, arg: u32) -> Result<bool, Error> {
        if let Some(v) = em0100::check(id, arg, self.action) {
            return Ok(v);
        }
        *self.asked.entry(id).or_default() += 1;
        Ok(std::env::var("TRUE")
            .ok()
            .is_some_and(|v| v.split(',').any(|x| x.parse::<u32>().ok() == Some(id))))
    }
    fn set(&mut self, _id: u32, _arg: u32) -> Result<u32, Error> {
        Ok(0)
    }
    fn rand(&mut self) -> u32 {
        if let Some(r) = std::env::var("ROLL").ok().and_then(|v| v.parse::<f64>().ok()) {
            return ((r + 0.5) / 100.0 * 4_294_967_296.0) as u32;
        }
        self.rolls = self.rolls.wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.rolls
    }
}

fn main() {
    let native = std::path::PathBuf::from("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
    let exe = std::env::var_os("DDDA_EXE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| ddexe::locate(&native).unwrap());
    let file = ddexe::ExeFile::open(&exe).unwrap();
    let img = file.image().unwrap();
    let tables = Tables::read(&img).unwrap();
    let names = em0100::action_classes(&img).unwrap();
    eprintln!("{} records", tables.len());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let table = args
        .first()
        .map(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap())
        .unwrap_or(em0100::DEFAULT_TABLE);
    let dists: Vec<f32> = if args.len() > 1 {
        args[1..].iter().map(|s| s.parse().unwrap()).collect()
    } else {
        vec![50.0, 150.0, 250.0, 400.0, 600.0, 1000.0, 2000.0]
    };
    for &d in &dists {
        for &ang in &std::env::var("ANG").ok().map_or(vec![0.0f32], |v| {
            v.split(',').map(|x| x.parse().unwrap()).collect::<Vec<f32>>()
        }) {
            let mut drv = Driver::new(1.0);
            drv.mgr.request_table(table);
            drv.mgr.flags = std::env::var("FLAGS")
                .ok()
                .map_or(0x4, |v| u32::from_str_radix(&v, 16).unwrap());
            let yaw = 0.0;
            let r = ang.to_radians();
            let target = [d * r.sin(), 0.0, d * r.cos()];
            drv.mgr.slots[0] = Slot::measure([0.0; 3], yaw, target, 0.0, 1);
            let mut host = Probe {
                action: 0,
                asked: BTreeMap::new(),
                rolls: 7,
            };
            let mut seq = vec![];
            for _ in 0..std::env::var("N").ok().map_or(12, |v| v.parse().unwrap()) {
                host.action = drv.action;
                match drv.think(&tables, &mut host) {
                    Ok(Some(a)) => seq.push(format!(
                        "{a}:{}",
                        names
                            .get(a as usize)
                            .map_or("?", |s| s.trim_start_matches("cEm0100Act"))
                    )),
                    Ok(None) => seq.push("-".into()),
                    Err(e) => {
                        seq.push(format!("ERR {e}"));
                        break;
                    }
                }
                if std::env::var("ROWS").is_ok() {
                    seq.push(format!("{:x?}", drv.mgr.trace));
                }
                drv.mgr
                    .tick(std::env::var("DT").ok().map_or(10.0, |v| v.parse().unwrap()));
            }
            println!("d={d} ang={ang}: {}  asked={:?}", seq.join(" "), host.asked);
        }
    }
}
