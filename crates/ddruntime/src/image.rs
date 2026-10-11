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

/// One imported function: where the guest's import slot is, which token the
/// runtime wrote into it, and what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub dll: String,
    /// The function name, or `#ordinal`.
    pub name: String,
    /// The IAT slot the guest calls through (`call dword ptr [slot]`).
    pub slot: u32,
    /// The guest address written into the slot; services bind to it.
    pub token: u32,
}

impl Import {
    pub fn full_name(&self) -> String {
        format!("{}!{}", self.dll, self.name)
    }
}

/// Import tokens live on reserved pages no image section uses; they are
/// addresses for the dispatcher, never executed or read as code.
pub const IMPORT_TOKENS: u32 = 0x7FF0_0000;
const TOKEN_STRIDE: u32 = 0x10;

/// Bind the exe's normal imports (data directory 1): every IAT slot gets a
/// distinct token address (read-only token pages are mapped so the guest may
/// compare or copy such pointers). Delay-load imports (directory 13) are not
/// bound here; they go through the CRT's delay-load helper.
pub fn bind_imports(mem: &mut GuestMemory, exe: &ExeFile) -> Result<Vec<Import>, LoadError> {
    let b = &exe.bytes;
    let rd32 = |o: usize| -> u32 { u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) };
    let rd16 = |o: usize| -> u16 { u16::from_le_bytes(b[o..o + 2].try_into().unwrap()) };
    let image = exe.image().map_err(LoadError::Exe)?;
    let base = image.image_base();
    // file offset of an RVA, through the section table
    let off = |rva: u32| -> Option<usize> {
        image.sections().iter().find(|s| rva >= s.va - base && rva < s.va - base + s.raw_size.max(s.virtual_size))
            .map(|s| (rva - (s.va - base) + s.raw_offset) as usize)
    };
    let cstr = |o: usize| -> String { b[o..].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect() };
    let pe = rd32(0x3c) as usize;
    let opt = pe + 24;
    let (dir_rva, _dir_size) = (rd32(opt + 96 + 8), rd32(opt + 96 + 12));
    let mut out = Vec::new();
    let mut desc = off(dir_rva).expect("import directory in a section");
    loop {
        let (ilt, name_rva, iat) = (rd32(desc), rd32(desc + 12), rd32(desc + 16));
        if name_rva == 0 && iat == 0 {
            break;
        }
        let dll = cstr(off(name_rva).expect("dll name"));
        // the lookup table (or the IAT itself when there is none) names the functions
        let mut lookup = off(if ilt != 0 { ilt } else { iat }).expect("import lookup table");
        let mut slot = base + iat;
        loop {
            let entry = rd32(lookup);
            if entry == 0 {
                break;
            }
            let name = if entry & 0x8000_0000 != 0 {
                format!("#{}", entry & 0xFFFF)
            } else {
                let hint_name = off(entry).expect("hint/name");
                let _hint = rd16(hint_name);
                cstr(hint_name + 2)
            };
            let token = IMPORT_TOKENS + TOKEN_STRIDE * out.len() as u32;
            out.push(Import { dll: dll.clone(), name, slot, token });
            slot += 4;
            lookup += 4;
        }
        desc += 20;
    }
    let span = (TOKEN_STRIDE * out.len() as u32).max(1);
    mem.map(IMPORT_TOKENS, span, READ).map_err(LoadError::Overlap)?;
    for i in &out {
        mem.poke(i.slot, &i.token.to_le_bytes()).expect("IAT inside a mapped section");
    }
    Ok(out)
}
