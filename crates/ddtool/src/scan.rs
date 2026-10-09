//! Parallel walk over every resource of one type in a nativePC tree.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use mtarc::{decode_payload, Archive, Entry};

pub fn find_arcs(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            find_arcs(&p, out)?;
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arc")) {
            out.push(p);
        }
    }
    Ok(())
}

/// Result of a walk: how many resources were visited and which failed.
pub struct Walk {
    pub archives: usize,
    pub visited: usize,
    pub failures: Vec<String>,
}

/// Inflates every entry whose type hash is `type_hash` in every archive
/// under `dir` and hands it to `visit` (from several threads). Errors from
/// `visit` are collected as "archive: entry: message".
pub fn for_each_resource<F>(dir: &Path, type_hash: u32, visit: F) -> Result<Walk, String>
where
    F: Fn(&Path, &Entry, &[u8]) -> Result<(), String> + Sync,
{
    for_each_matching(dir, |e| e.type_hash == type_hash, visit)
}

/// Like [`for_each_resource`], for every entry that `wanted` accepts.
pub fn for_each_matching<P, F>(dir: &Path, wanted: P, visit: F) -> Result<Walk, String>
where
    P: Fn(&Entry) -> bool + Sync,
    F: Fn(&Path, &Entry, &[u8]) -> Result<(), String> + Sync,
{
    let mut files = Vec::new();
    find_arcs(dir, &mut files).map_err(|e| format!("{}: {e}", dir.display()))?;
    files.sort();
    let next = AtomicUsize::new(0);
    let visited = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut raw = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let res = (|| -> Result<(), String> {
                        let mut file = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
                        let arc = Archive::read_index(&mut file).map_err(|e| e.to_string())?;
                        for e in arc.entries.iter().filter(|e| wanted(e)) {
                            raw.resize(e.compressed_size as usize, 0);
                            file.seek(SeekFrom::Start(u64::from(e.offset)))
                                .map_err(|x| x.to_string())?;
                            file.read_exact(&mut raw).map_err(|x| x.to_string())?;
                            let bytes = decode_payload(e, &raw).map_err(|x| x.to_string())?;
                            visited.fetch_add(1, Ordering::Relaxed);
                            if let Err(msg) = visit(path, e, &bytes) {
                                failures.lock().unwrap().push(format!(
                                    "{}: {}: {msg}",
                                    path.display(),
                                    e.path
                                ));
                            }
                        }
                        Ok(())
                    })();
                    if let Err(msg) = res {
                        failures
                            .lock()
                            .unwrap()
                            .push(format!("{}: {msg}", path.display()));
                    }
                }
            });
        }
    });
    Ok(Walk {
        archives: files.len(),
        visited: visited.into_inner(),
        failures: failures.into_inner().unwrap(),
    })
}

/// Like [`for_each_resource`] but per archive: `visit` gets every entry
/// whose type is in `types`, inflated, in archive order.
pub fn for_each_archive<F>(dir: &Path, types: &[u32], visit: F) -> Result<Walk, String>
where
    F: Fn(&Path, &[(Entry, Vec<u8>)]) -> Vec<String> + Sync,
{
    let mut files = Vec::new();
    find_arcs(dir, &mut files).map_err(|e| format!("{}: {e}", dir.display()))?;
    files.sort();
    let next = AtomicUsize::new(0);
    let visited = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(path) = files.get(i) else { break };
                let res = (|| -> Result<Vec<(Entry, Vec<u8>)>, String> {
                    let mut file = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
                    let arc = Archive::read_index(&mut file).map_err(|e| e.to_string())?;
                    let mut out = Vec::new();
                    for e in arc.entries.iter().filter(|e| types.contains(&e.type_hash)) {
                        let bytes = mtarc::read_entry(&mut file, e).map_err(|x| x.to_string())?;
                        out.push((e.clone(), bytes));
                    }
                    Ok(out)
                })();
                match res {
                    Ok(items) => {
                        visited.fetch_add(items.len(), Ordering::Relaxed);
                        let errs = visit(path, &items);
                        if !errs.is_empty() {
                            let mut f = failures.lock().unwrap();
                            f.extend(errs.into_iter().map(|e| format!("{}: {e}", path.display())));
                        }
                    }
                    Err(msg) => failures
                        .lock()
                        .unwrap()
                        .push(format!("{}: {msg}", path.display())),
                }
            });
        }
    });
    Ok(Walk {
        archives: files.len(),
        visited: visited.into_inner(),
        failures: failures.into_inner().unwrap(),
    })
}

/// Reads one entry from an archive by its in-archive path and type hash.
pub fn read_one(arc_path: &Path, entry_path: &str, type_hash: u32) -> Result<Vec<u8>, String> {
    let (arc, mut file) = Archive::open(arc_path).map_err(|e| format!("{}: {e}", arc_path.display()))?;
    let want = entry_path.replace('/', "\\");
    let e = arc
        .entries
        .iter()
        .find(|e| e.type_hash == type_hash && e.path.eq_ignore_ascii_case(&want))
        .ok_or_else(|| format!("{entry_path}: no such entry of type {type_hash:08x}"))?;
    mtarc::read_entry(&mut file, e).map_err(|e| e.to_string())
}
