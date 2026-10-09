//! Dumps an rAIPlayerActionParameter resource as XFS: `plact_dump [path]`.
fn dump(x: &mtxfs::Xfs, o: &mtxfs::Object, depth: usize, out: &mut String) {
    let c = x.class_of(o);
    out.push_str(&format!("{}{}\n", "  ".repeat(depth), c.name().unwrap_or("?")));
    for p in &c.properties {
        let vals = x.get(o, &p.name).unwrap_or(&[]);
        for v in vals {
            match v {
                mtxfs::Value::Object(Some(b)) => {
                    out.push_str(&format!("{}{}:\n", "  ".repeat(depth + 1), p.name));
                    dump(x, b, depth + 2, out)
                }
                other => out.push_str(&format!("{}{} = {:?}\n", "  ".repeat(depth + 1), p.name, other)),
            }
        }
    }
}
fn main() {
    let root = std::env::var("DDDA_NATIVE_PC")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC".into());
    let vfs = ddvfs::Vfs::open(root).unwrap();
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| r"AI\AIPlayerActionParameter\AIPlActParamSword".into());
    let b = vfs
        .read(&path, mtarc::type_hash("rAIPlayerActionParameter"))
        .unwrap();
    println!("{} bytes, magic {:?}", b.len(), &b[..4]);
    match mtxfs::parse(&b) {
        Ok(x) => {
            let mut s = String::new();
            dump(&x, &x.root, 0, &mut s);
            println!("{s}");
        }
        Err(e) => println!("not xfs: {e:?}"),
    }
}
