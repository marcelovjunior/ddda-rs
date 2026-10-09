//! `xfs-scan` and `xfs-dump`: the XFS reader against the whole install.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use mtxfs::{Value, Xfs};

use crate::scan;

/// Payload of an XFS resource: plain, or wrapped in a 12-byte `PRPZ`
/// header (`.prp`). Returns `None` for non-XFS payloads.
fn xfs_payload(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.starts_with(b"XFS\0") {
        Some(bytes)
    } else if bytes.starts_with(b"PRPZ") && bytes.get(12..16) == Some(b"XFS\0") {
        Some(&bytes[12..])
    } else {
        None
    }
}

#[derive(Default)]
struct TypeStats {
    files: usize,
    objects: u64,
}

pub fn xfs_scan(dir: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let by_type = Mutex::new(BTreeMap::<String, TypeStats>::new());
    let unknown_classes = Mutex::new(BTreeMap::<u32, usize>::new());
    let not_xfs = Mutex::new(BTreeMap::<String, usize>::new());
    // Skip the bulk binary types; everything else is inflated and sniffed.
    let skip = [
        mtarc::type_hash("rTexture"),
        mtarc::type_hash("rModel"),
        mtarc::type_hash("rSoundPackage"),
        mtarc::type_hash("rShaderPackage"),
        mtarc::type_hash("rMotionList"),
        mtarc::type_hash("rCollision"),
        mtarc::type_hash("rMaterial"),
    ];
    let walk = scan::for_each_matching(
        dir,
        |e| !skip.contains(&e.type_hash),
        |_, e, bytes| {
            let tname = e.type_name().unwrap_or("?").to_string();
            let Some(payload) = xfs_payload(bytes) else {
                *not_xfs.lock().unwrap().entry(tname).or_default() += 1;
                return Ok(());
            };
            let x = mtxfs::parse(payload).map_err(|err| format!("{} ({tname}): {err}", e.path))?;
            for c in &x.classes {
                if c.name().is_none() {
                    *unknown_classes.lock().unwrap().entry(c.hash).or_default() += 1;
                }
            }
            let mut t = by_type.lock().unwrap();
            let st = t.entry(tname).or_default();
            st.files += 1;
            st.objects += u64::from(x.object_count);
            Ok(())
        },
    )?;
    let by_type = by_type.into_inner().unwrap();
    let unknown = unknown_classes.into_inner().unwrap();
    let files: usize = by_type.values().map(|s| s.files).sum();
    let objects: u64 = by_type.values().map(|s| s.objects).sum();
    println!("archives:        {}", walk.archives);
    println!("entries sniffed: {}", walk.visited);
    println!("xfs files:       {files} parsed, {} failed", walk.failures.len());
    println!("objects:         {objects}");
    println!("resource types:  {} hold XFS", by_type.len());
    println!(
        "class hashes not in the DTI registry: {} {:08x?}",
        unknown.len(),
        unknown.keys().take(10).collect::<Vec<_>>()
    );
    println!("elapsed:         {:.1} s\n", started.elapsed().as_secs_f64());
    let mut v: Vec<_> = by_type.iter().collect();
    v.sort_by_key(|(_, s)| std::cmp::Reverse(s.files));
    for (t, s) in v.iter().take(20) {
        println!("  {t:<28} {:>6} files {:>9} objects", s.files, s.objects);
    }
    for f in walk.failures.iter().take(15) {
        println!("FAIL {f}");
    }
    if walk.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} XFS files failed", walk.failures.len()))
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::U(x) => x.to_string(),
        Value::S(x) => x.to_string(),
        Value::U16s(x) => format!("{x:?}"),
        Value::F(x) if x.len() == 1 => format!("{}", x[0]),
        Value::F(x) => format!(
            "{:?}",
            x.iter()
                .map(|f| (*f * 1000.0).round() / 1000.0)
                .collect::<Vec<_>>()
        ),
        Value::Str(s) => format!("{:?}", String::from_utf8_lossy(s)),
        Value::Resource { class, path } => {
            format!(
                "<{}> {}",
                String::from_utf8_lossy(class),
                String::from_utf8_lossy(path)
            )
        }
        Value::Object(None) => "null".into(),
        Value::Object(Some(_)) => "{...}".into(),
    }
}

fn dump(x: &Xfs, o: &mtxfs::Object, indent: usize, out: &mut String) {
    let c = x.class_of(o);
    let pad = "  ".repeat(indent);
    out.push_str(&format!("{pad}#{} {}\n", o.number, c.name().unwrap_or("?")));
    for (p, vals) in c.properties.iter().zip(&o.fields) {
        let objs: Vec<&mtxfs::Object> = vals
            .iter()
            .filter_map(|v| {
                if let Value::Object(Some(b)) = v {
                    Some(b.as_ref())
                } else {
                    None
                }
            })
            .collect();
        if objs.is_empty() {
            let shown: Vec<String> = vals.iter().take(8).map(show).collect();
            let more = if vals.len() > 8 {
                format!(" ... ({} values)", vals.len())
            } else {
                String::new()
            };
            out.push_str(&format!("{pad}  {} = {}{more}\n", p.name, shown.join(", ")));
        } else {
            out.push_str(&format!("{pad}  {}: {} object(s)\n", p.name, objs.len()));
            for child in objs {
                dump(x, child, indent + 2, out);
            }
        }
    }
}

pub fn xfs_dump(arc: &Path, entry: &str) -> Result<(), String> {
    let (archive, mut file) = mtarc::Archive::open(arc).map_err(|e| e.to_string())?;
    let e = archive
        .entries
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(entry))
        .ok_or_else(|| format!("{entry} not in {}", arc.display()))?;
    let bytes = mtarc::read_entry(&mut file, e).map_err(|e| e.to_string())?;
    let payload = xfs_payload(&bytes).ok_or("not an XFS resource")?;
    let x = mtxfs::parse(payload).map_err(|e| e.to_string())?;
    let mut out = String::new();
    dump(&x, &x.root, 0, &mut out);
    print!("{out}");
    Ok(())
}

/// `lot-scan`: every rLayout in the install through `mtlot`.
pub fn lot_scan(dir: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let classes = Mutex::new(BTreeMap::<&'static str, usize>::new());
    let distinct = Mutex::new(std::collections::HashSet::<u64>::new());
    let walk = scan::for_each_resource(dir, mtarc::type_hash("rLayout"), |_, e, bytes| {
        let recs = mtlot::parse(bytes).map_err(|err| format!("{}: {err}", e.path))?;
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        if distinct.lock().unwrap().insert(h.finish()) {
            let mut c = classes.lock().unwrap();
            for r in &recs {
                *c.entry(r.class).or_default() += 1;
            }
        }
        Ok(())
    })?;
    let classes = classes.into_inner().unwrap();
    println!(
        "rLayout entries: {} parsed, {} failed",
        walk.visited - walk.failures.len(),
        walk.failures.len()
    );
    println!("distinct files:  {}", distinct.into_inner().unwrap().len());
    println!(
        "records:         {} (in distinct files), {} classes",
        classes.values().sum::<usize>(),
        classes.len()
    );
    println!("elapsed:         {:.1} s\n", started.elapsed().as_secs_f64());
    let mut v: Vec<_> = classes.iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    for (c, n) in v.iter().take(12) {
        println!("  {c:<26} {n:>6}");
    }
    for f in walk.failures.iter().take(15) {
        println!("FAIL {f}");
    }
    if walk.failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} layouts failed", walk.failures.len()))
    }
}
