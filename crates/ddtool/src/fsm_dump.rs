use mtfsm::{Cluster, Graph, Parameter, Record};
use std::path::Path;
fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
fn record(r: &Record, pad: &str) {
    println!("{pad}{}", r.class_name.unwrap_or("?"));
    for (name, values) in &r.fields {
        for v in values {
            match v {
                Parameter::Object(Some(c)) => {
                    println!("{pad}  {name}:");
                    record(c, &format!("{pad}    "));
                }
                Parameter::Object(None) => println!("{pad}  {name}=null"),
                Parameter::Scalar(mtxfs::Value::Str(s)) => println!("{pad}  {name}={:?}", text(s)),
                Parameter::Scalar(v) => println!("{pad}  {name}={v:?}"),
            }
        }
    }
}
fn cluster(c: &Cluster, pad: &str) {
    println!("{pad}cluster {} initial={}", c.id, c.initial_state);
    for s in &c.states {
        println!(
            "{pad}  state {} {:?} unique={} setting={:#x} from_all={:?}",
            s.id,
            text(&s.name),
            s.unique_id,
            s.setting,
            s.from_all
        );
        for l in &s.links {
            println!(
                "{pad}    link {:?} -> {} condition={} enabled={}",
                text(&l.name),
                l.destination,
                l.condition,
                l.enabled
            );
        }
        for p in &s.processes {
            println!("{pad}    process {}::{}", text(&p.category), text(&p.container));
            if let Some(r) = &p.parameter {
                record(r, &format!("{pad}      "));
            }
        }
        if let Some(c) = &s.subcluster {
            cluster(c, &format!("{pad}    "));
        }
    }
}
pub fn run(dir: &Path, path: &str) -> Result<(), String> {
    let v = ddvfs::Vfs::open(dir)?;
    let bytes = v.read(path, mtarc::type_hash("rAIFSM"))?;
    let g = Graph::parse(&bytes)?;
    println!("{path}: owner={} attribute={:#x}", text(&g.owner), g.attribute);
    if let Some(c) = &g.root {
        cluster(c, "");
    }
    for c in &g.conditions {
        println!("condition {} {:?}", c.id, text(&c.name));
        if let Some(e) = &c.root {
            record(&e.record, "  ");
        } else {
            println!("  null");
        }
    }
    Ok(())
}
