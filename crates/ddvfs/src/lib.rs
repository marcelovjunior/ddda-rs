//! Resource index over a nativePC tree: every entry of every `.arc`,
//! keyed by (lower-case path, type hash). Reads only.
//!
//! Some resources exist in several archives (shared textures, models
//! duplicated across stage archives); [`Vfs::read`] returns the first one
//! found in sorted archive order. *Unresolved:* the engine's own
//! precedence when copies differ.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mtarc::{Archive, Entry};

#[derive(Debug, Clone)]
pub struct Location {
    pub archive: usize,
    pub entry: Entry,
}

pub struct Vfs {
    pub root: PathBuf,
    pub archives: Vec<PathBuf>,
    index: HashMap<(String, u32), Vec<Location>>,
    /// Open file handles, one per archive, opened on demand.
    files: Vec<Mutex<Option<BufReader<File>>>>,
}

fn find_arcs(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            find_arcs(&p, out)?;
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arc")) {
            out.push(p);
        }
    }
    Ok(())
}

pub fn key(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

impl Vfs {
    /// Indexes every archive under `native_pc` (about 1-2 s for the full
    /// install: only the 8-byte headers and entry tables are read).
    pub fn open(native_pc: impl AsRef<Path>) -> Result<Self, String> {
        let root = native_pc.as_ref().to_path_buf();
        let mut archives = Vec::new();
        find_arcs(&root, &mut archives).map_err(|e| format!("{}: {e}", root.display()))?;
        archives.sort();
        let mut index: HashMap<(String, u32), Vec<Location>> = HashMap::new();
        for (i, p) in archives.iter().enumerate() {
            let mut f = BufReader::new(File::open(p).map_err(|e| format!("{}: {e}", p.display()))?);
            let arc = Archive::read_index(&mut f).map_err(|e| format!("{}: {e}", p.display()))?;
            for entry in arc.entries {
                index
                    .entry((key(&entry.path), entry.type_hash))
                    .or_default()
                    .push(Location { archive: i, entry });
            }
        }
        let files = archives.iter().map(|_| Mutex::new(None)).collect();
        Ok(Vfs {
            root,
            archives,
            index,
            files,
        })
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Every resource at `path`, whatever its type: (type hash, locations).
    pub fn find(&self, path: &str) -> Vec<(u32, &[Location])> {
        let k = key(path);
        self.index
            .iter()
            .filter(|((p, _), _)| *p == k)
            .map(|((_, t), l)| (*t, l.as_slice()))
            .collect()
    }

    /// Lower-case paths of every resource of one type.
    pub fn paths_of_type(&self, type_hash: u32) -> impl Iterator<Item = &str> {
        self.index
            .keys()
            .filter(move |(_, t)| *t == type_hash)
            .map(|(p, _)| p.as_str())
    }

    pub fn locate(&self, path: &str, type_hash: u32) -> Option<&[Location]> {
        self.index.get(&(key(path), type_hash)).map(Vec::as_slice)
    }

    /// Inflated bytes of the first copy of a resource.
    pub fn read(&self, path: &str, type_hash: u32) -> Result<Vec<u8>, String> {
        let loc = self
            .locate(path, type_hash)
            .and_then(|l| l.first())
            .ok_or_else(|| format!("{path} ({type_hash:08x}) not found"))?;
        self.read_at(loc)
    }

    pub fn read_at(&self, loc: &Location) -> Result<Vec<u8>, String> {
        let mut slot = self.files[loc.archive].lock().unwrap();
        if slot.is_none() {
            let p = &self.archives[loc.archive];
            *slot = Some(BufReader::new(
                File::open(p).map_err(|e| format!("{}: {e}", p.display()))?,
            ));
        }
        mtarc::read_entry(slot.as_mut().unwrap(), &loc.entry).map_err(|e| e.to_string())
    }
}
