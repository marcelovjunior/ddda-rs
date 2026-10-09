//! `ddtool`: command-line inspection of a Dragon's Dogma: Dark Arisen install.
//! Reads only; never writes into the game folder.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use mtarc::{decode_payload, Archive};

mod model;
mod msg_dump;
mod param_dump;
mod scan;
mod tex;
mod xfs;

use scan::find_arcs;

const USAGE: &str = "usage:
  ddtool msg-dump <nativePC dir> <resource path>
  ddtool param-dump <nativePC dir> <resource path>
  ddtool arc-ls <file.arc>
  ddtool arc-scan <nativePC dir> [--inflate]
  ddtool tex-scan <nativePC dir>
  ddtool tex-dds <file.arc> <entry path> <out.dds>
  ddtool mod-scan <nativePC dir>
  ddtool mrl-scan <nativePC dir>
  ddtool mod-info <nativePC dir> <file.arc> <entry path>
  ddtool xfs-scan <nativePC dir>
  ddtool xfs-dump <file.arc> <entry path>
  ddtool lot-scan <nativePC dir>
  ddtool lmt-scan <nativePC dir>
  ddtool sbc-scan <nativePC dir>
  ddtool xfs-get <nativePC dir> <resource path>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["msg-dump", dir, path] => msg_dump::msg_dump(Path::new(dir), path),
        ["param-dump", dir, path] => param_dump::run(Path::new(dir), path),
        ["arc-ls", file] => arc_ls(Path::new(file)),
        ["arc-scan", dir] => arc_scan(Path::new(dir), false),
        ["arc-scan", dir, "--inflate"] => arc_scan(Path::new(dir), true),
        ["tex-scan", dir] => tex::tex_scan(Path::new(dir)),
        ["tex-dds", arc, entry, out] => tex::tex_dds(Path::new(arc), entry, Path::new(out)),
        ["mod-scan", dir] => model::mod_scan(Path::new(dir)),
        ["mrl-scan", dir] => model::mrl_scan(Path::new(dir)),
        ["mod-info", dir, arc, entry] => model::mod_info(Path::new(dir), Path::new(arc), entry),
        ["xfs-scan", dir] => xfs::xfs_scan(Path::new(dir)),
        ["xfs-dump", arc, entry] => xfs::xfs_dump(Path::new(arc), entry),
        ["lot-scan", dir] => xfs::lot_scan(Path::new(dir)),
        ["lmt-scan", dir] => xfs::lmt_scan(Path::new(dir)),
        ["sbc-scan", dir] => xfs::sbc_scan(Path::new(dir)),
        ["xfs-get", dir, path] => xfs::xfs_get(Path::new(dir), path),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn arc_ls(path: &Path) -> Result<(), String> {
    let (arc, _) = Archive::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{} entries", arc.entries.len());
    for e in &arc.entries {
        println!(
            "{:>10} {:>10} {:08x} {:<24} {}",
            e.size,
            e.compressed_size,
            e.type_hash,
            e.type_name().unwrap_or("?"),
            e.path
        );
    }
    Ok(())
}

#[derive(Default)]
struct TypeStats {
    count: u64,
    packed: u64,
    size: u64,
}

fn arc_scan(dir: &Path, inflate: bool) -> Result<(), String> {
    let mut files = Vec::new();
    find_arcs(dir, &mut files).map_err(|e| format!("{}: {e}", dir.display()))?;
    files.sort();
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::<String>::new());
    let types = Mutex::new(BTreeMap::<u32, TypeStats>::new());
    let entries = AtomicUsize::new(0);
    let flags_other = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let started = std::time::Instant::now();

    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = files.get(i) else { break };
                let mut local = BTreeMap::<u32, TypeStats>::new();
                if let Err(e) = scan_one(path, inflate, &mut local, &entries, &flags_other) {
                    failures.lock().unwrap().push(format!("{}: {e}", path.display()));
                }
                let mut t = types.lock().unwrap();
                for (h, st) in local {
                    let g = t.entry(h).or_default();
                    g.count += st.count;
                    g.packed += st.packed;
                    g.size += st.size;
                }
            });
        }
    });

    let types = types.into_inner().unwrap();
    let failures = failures.into_inner().unwrap();
    let unknown: Vec<_> = types.keys().filter(|&&h| mtarc::type_name(h).is_none()).collect();
    let (packed, size) = types.values().fold((0, 0), |a, s| (a.0 + s.packed, a.1 + s.size));
    println!("archives:      {}", files.len());
    println!("entries:       {}", entries.load(Ordering::Relaxed));
    println!(
        "types:         {} ({} unknown: {:08x?})",
        types.len(),
        unknown.len(),
        unknown
    );
    println!("compressed:    {:.2} GB", packed as f64 / 1e9);
    println!(
        "decompressed:  {:.2} GB{}",
        size as f64 / 1e9,
        if inflate { " (inflated and checked)" } else { "" }
    );
    println!("flags != 2:    {}", flags_other.load(Ordering::Relaxed));
    println!("elapsed:       {:.1} s", started.elapsed().as_secs_f64());
    let mut by_size: Vec<_> = types.iter().collect();
    by_size.sort_by_key(|(_, s)| std::cmp::Reverse(s.size));
    println!("\nlargest types:");
    for (h, s) in by_size.iter().take(12) {
        println!(
            "  {:<26} {:>7} entries {:>9.2} GB",
            mtarc::type_name(**h).unwrap_or("?"),
            s.count,
            s.size as f64 / 1e9
        );
    }
    println!("\nfailures: {}", failures.len());
    for f in failures.iter().take(20) {
        println!("  {f}");
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{} archives failed", failures.len()))
    }
}

fn scan_one(
    path: &Path,
    inflate: bool,
    types: &mut BTreeMap<u32, TypeStats>,
    entries: &AtomicUsize,
    flags_other: &AtomicUsize,
) -> Result<(), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut file = BufReader::new(file);
    let arc = Archive::read_index(&mut file).map_err(|e| e.to_string())?;
    arc.check_bounds(len).map_err(|e| e.to_string())?;
    entries.fetch_add(arc.entries.len(), Ordering::Relaxed);
    let mut raw = Vec::new();
    for e in &arc.entries {
        if e.flags != 2 {
            flags_other.fetch_add(1, Ordering::Relaxed);
        }
        let st = types.entry(e.type_hash).or_default();
        st.count += 1;
        st.packed += u64::from(e.compressed_size);
        st.size += u64::from(e.size);
        if inflate {
            raw.resize(e.compressed_size as usize, 0);
            file.seek(SeekFrom::Start(u64::from(e.offset)))
                .map_err(|x| x.to_string())?;
            file.read_exact(&mut raw).map_err(|x| x.to_string())?;
            decode_payload(e, &raw).map_err(|x| x.to_string())?;
        }
    }
    Ok(())
}
