//! Maps the player's installed DDDA.exe into guest memory (docs/RUNTIME.md,
//! "Image loading"): the headers and every section at its virtual address,
//! file-backed bytes copied, virtual tails (BSS) zero, permissions by
//! section. No code is executed from these pages; they are data for the
//! lifted functions (which read jump tables, constants and globals there).
use crate::memory::{GuestMemory, Overlap, READ, WRITE};
use ddexe::{ExeFile, Section};

/// One mapped section.
#[derive(Debug, Clone)]
pub struct Mapped {
    pub name: String,
    pub va: u32,
    pub size: u32,
    pub perm: u8,
}

#[derive(Debug)]
pub enum LoadError {
    Exe(ddexe::Error),
    Overlap(Overlap),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Exe(e) => write!(f, "{e}"),
            LoadError::Overlap(o) => write!(f, "section overlaps mapped page {:#010x}", o.page),
        }
    }
}

/// Permissions the guest's data accesses get for a section. Code is
/// readable (the lifted code reads jump tables placed in it) but never
/// writable; `.data` is the only writable image section.
pub fn section_perm(name: &str) -> u8 {
    match name {
        ".data" => READ | WRITE,
        _ => READ,
    }
}

/// Load the exe at `exe` (checked to be the supported build) into `mem`.
pub fn load(mem: &mut GuestMemory, exe: &ExeFile) -> Result<Vec<Mapped>, LoadError> {
    let image = exe.image().map_err(LoadError::Exe)?;
    let base = image.image_base();
    let mut out = Vec::new();
    // the headers: the CRT and the game may read their own PE headers
    let first_raw = image.sections().iter().map(|s| s.raw_offset).filter(|&o| o > 0).min().unwrap_or(0x1000);
    let headers = &exe.bytes[..(first_raw as usize).min(exe.bytes.len())];
    mem.map(base, headers.len() as u32, READ).map_err(LoadError::Overlap)?;
    mem.poke(base, headers).expect("just mapped");
    out.push(Mapped { name: "headers".into(), va: base, size: headers.len() as u32, perm: READ });
    for s in image.sections() {
        let size = s.virtual_size.max(s.raw_size);
        let perm = section_perm(&s.name);
        mem.map(s.va, size, perm).map_err(LoadError::Overlap)?;
        let data = file_bytes(exe, s);
        mem.poke(s.va, data).expect("just mapped");
        out.push(Mapped { name: s.name.clone(), va: s.va, size, perm });
    }
    Ok(out)
}

/// The bytes of `s` that exist in the file; the rest of its virtual size
/// stays zero (the loader zero-fills it, as Windows does).
fn file_bytes<'a>(exe: &'a ExeFile, s: &Section) -> &'a [u8] {
    let len = s.virtual_size.min(s.raw_size) as usize;
    let start = (s.raw_offset as usize).min(exe.bytes.len());
    &exe.bytes[start..(start + len).min(exe.bytes.len())]
}
