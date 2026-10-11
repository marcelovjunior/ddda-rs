//! File services: the KERNEL32 file API over three roots (docs/RUNTIME.md,
//! "Platform services").
//!
//! - The **install** (the directory of the player's DDDA.exe): read-only.
//!   The runtime never creates, writes, renames or deletes anything there.
//! - The **sandbox** ([`crate::services::sandbox_home`]): an overlay that
//!   mirrors install-relative paths. Every write lands here; reads look here
//!   first, so the guest sees its own writes.
//! - The **home** (the rest of the sandbox): the guest's known folders
//!   (crate::services::sh_get_folder_path_w), e.g. the game's settings file
//!   `<Documents>\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini` (MtRegistry
//!   0x00CFFF00). Read and written in place.
//!
//! Guest paths are resolved against the guest's current directory (the
//! install, as when Steam launches the game). An absolute path must lie in
//! the install or the sandbox; anything else is refused with
//! ERROR_ACCESS_DENIED, so the guest cannot touch the rest of the machine.
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

const INVALID_HANDLE_VALUE: u32 = 0xFFFF_FFFF;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_NO_MORE_FILES: u32 = 18;
const ERROR_HANDLE_EOF: u32 = 38;
const ERROR_FILE_EXISTS: u32 = 80;
const ERROR_BAD_PATHNAME: u32 = 161;
const ERROR_ALREADY_EXISTS: u32 = 183;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const INVALID_FILE_ATTRIBUTES: u32 = 0xFFFF_FFFF;
const FIRST_HANDLE: u32 = 0x0000_1004;

enum Open {
    File(File),
    Find(Vec<Found>),
}

#[derive(Clone)]
struct Found {
    name: String,
    dir: bool,
    size: u64,
}

#[derive(Default)]
pub struct Files {
    install: PathBuf,
    /// The overlay (sandbox\overlay).
    sandbox: PathBuf,
    /// The sandbox root (the home).
    home: PathBuf,
    /// The guest's current directory, relative to the install root.
    cwd: PathBuf,
    handles: HashMap<u32, Open>,
    next: u32,
}

fn with_files<R>(f: impl FnOnce(&mut Files) -> R) -> R {
    crate::runtime::with(|rt| rt.files.with(f))
}

/// Set the roots: the install directory (read-only) and the sandbox.
pub fn set_roots(install: &Path, sandbox: &Path) {
    with_files(|f| {
        f.install = install.to_path_buf();
        f.sandbox = sandbox.join("overlay");
        f.home = sandbox.to_path_buf();
        f.cwd = PathBuf::new();
    });
}

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
}

/// Where a guest path lives: (install-relative path) or refused.
fn relative(f: &Files, guest: &str) -> Option<PathBuf> {
    let p = Path::new(guest);
    let rel = if p.is_absolute() || guest.as_bytes().get(1) == Some(&b':') {
        if let Ok(r) = p.strip_prefix(&f.install) {
            r.to_path_buf()
        } else if let Ok(r) = p.strip_prefix(&f.sandbox) {
            r.to_path_buf()
        } else {
            return None;
        }
    } else {
        f.cwd.join(p)
    };
    // normalise `..` and `.` without leaving the root
    let mut out = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some(out)
}

/// The host path to read: the overlay copy if there is one, else the install.
fn read_path(f: &Files, rel: &Path) -> PathBuf {
    let over = f.sandbox.join(rel);
    if over.exists() { over } else { f.install.join(rel) }
}

/// A guest path resolved to host paths.
pub(crate) struct Place {
    /// Where the guest's view of the file is read from.
    pub(crate) read: PathBuf,
    /// Where writes go (the overlay, or the home path itself).
    pub(crate) write: PathBuf,
    /// The install's file under this path (None in the home), never written.
    pub(crate) install: Option<PathBuf>,
}

/// Resolve a guest path in the install (with its overlay) or the home;
/// None if it is outside both (refused).
fn place_in(f: &Files, guest: &str) -> Option<Place> {
    if let Some(rel) = relative(f, guest) {
        return Some(Place { read: read_path(f, &rel), write: f.sandbox.join(&rel), install: Some(f.install.join(rel)) });
    }
    let r = Path::new(guest).strip_prefix(&f.home).ok()?;
    let mut out = f.home.clone();
    let depth = out.components().count();
    for c in r.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::ParentDir => {
                if out.components().count() == depth {
                    return None;
                }
                out.pop();
            }
            _ => {}
        }
    }
    Some(Place { read: out.clone(), write: out, install: None })
}

/// [`place_in`] for the other service modules (crate::profile).
pub(crate) fn place(guest: &str) -> Option<Place> {
    with_files(|f| place_in(f, guest))
}

fn guest_str_a(m: &dyn Mem, p: u32) -> String {
    (0..).map(|k| m.u8(p.wrapping_add(k))).take_while(|&b| b != 0).map(|b| b as char).collect()
}

fn guest_str_w(m: &dyn Mem, p: u32) -> String {
    let units: Vec<u16> = (0..).map(|k| m.u16(p.wrapping_add(2 * k))).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units)
}

fn new_handle(f: &mut Files, o: Open) -> u32 {
    if f.next == 0 {
        f.next = FIRST_HANDLE;
    }
    let h = f.next;
    f.next += 4;
    f.handles.insert(h, o);
    h
}

/// CreateFileA/W(name, access, share, security, disposition, flags, template).
fn create_file(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let name = if wide { guest_str_w(m, arg(m, cpu, 0)) } else { guest_str_a(m, arg(m, cpu, 0)) };
    let (access, disposition) = (arg(m, cpu, 1), arg(m, cpu, 4));
    let result = with_files(|f| -> Result<u32, u32> {
        let pl = place_in(f, &name).ok_or(ERROR_ACCESS_DENIED)?;
        let write = access & GENERIC_WRITE != 0 || matches!(disposition, 1 | 2 | 5);
        let file = if write {
            // writes go to the overlay; existing install contents are copied first
            let over = pl.write.clone();
            let orig = pl.install.clone().unwrap_or_else(|| over.clone());
            if pl.install.is_some() {
                if let Some(dir) = over.parent() {
                    std::fs::create_dir_all(dir).map_err(|_| ERROR_PATH_NOT_FOUND)?;
                }
            }
            let existed = over.exists() || orig.exists();
            if !over.exists() && orig.is_file() && disposition != 2 && disposition != 5 {
                std::fs::copy(&orig, &over).map_err(|_| ERROR_ACCESS_DENIED)?;
            }
            let mut o = OpenOptions::new();
            o.read(true).write(true);
            match disposition {
                1 if existed => return Err(ERROR_FILE_EXISTS), // CREATE_NEW
                1 | 2 => { o.create(true).truncate(true); }      // CREATE_NEW, CREATE_ALWAYS
                3 if !existed => return Err(ERROR_FILE_NOT_FOUND), // OPEN_EXISTING
                4 => { o.create(true); }                           // OPEN_ALWAYS
                5 if !existed => return Err(ERROR_FILE_NOT_FOUND), // TRUNCATE_EXISTING
                5 => { o.truncate(true); }
                _ => {}
            }
            let file = o.open(&over).map_err(|_| ERROR_ACCESS_DENIED)?;
            if existed && matches!(disposition, 2 | 4) {
                set_error(ERROR_ALREADY_EXISTS); // documented for CREATE_ALWAYS/OPEN_ALWAYS
            }
            file
        } else {
            let path = pl.read;
            if path.is_dir() {
                return Err(ERROR_ACCESS_DENIED);
            }
            File::open(&path).map_err(|_| if path.parent().map(Path::exists) == Some(true) {
                ERROR_FILE_NOT_FOUND
            } else {
                ERROR_PATH_NOT_FOUND
            })?
        };
        Ok(new_handle(f, Open::File(file)))
    });
    cpu.eax = match result {
        Ok(h) => h,
        Err(e) => {
            set_error(e);
            INVALID_HANDLE_VALUE
        }
    };
    ret(m, cpu, 28);
}
pub fn create_file_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    create_file(m, cpu, false)
}
pub fn create_file_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    create_file(m, cpu, true)
}

/// ReadFile(handle, buffer, count, read*, overlapped): synchronous reads
/// only (an OVERLAPPED argument is refused until asynchronous I/O exists).
pub fn read_file(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, buf, count, read_out, overlapped) =
        (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    assert_eq!(overlapped, 0, "ReadFile with OVERLAPPED: asynchronous I/O is not implemented");
    let data = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::File(file)) => {
            let mut v = vec![0u8; count as usize];
            let n = file.read(&mut v).unwrap_or(0);
            v.truncate(n);
            Some(v)
        }
        _ => None,
    });
    cpu.eax = match data {
        Some(v) => {
            for (k, &b) in v.iter().enumerate() {
                m.set_u8(buf.wrapping_add(k as u32), b);
            }
            if read_out != 0 {
                m.set_u32(read_out, v.len() as u32);
            }
            1
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            0
        }
    };
    ret(m, cpu, 20);
}

/// ReadFileEx(handle, buffer, count, OVERLAPPED*, completion routine):
/// the read happens now, at the OVERLAPPED's 64-bit offset (Offset +8,
/// OffsetHigh +12; asynchronous handles have no file pointer), and the
/// completion routine (error, bytes, OVERLAPPED*) is queued to this thread,
/// to run at its next alertable wait (SleepEx), as Windows delivers it. A
/// read starting at or past the end fails with ERROR_HANDLE_EOF; one that
/// reaches the end completes with the bytes there were. The game's reader
/// is MtFile::vf08 (0x00D0D440) with completion 0x00D0D430.
pub fn read_file_ex(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, buf, count, ov, routine) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3), arg(m, cpu, 4));
    let offset = m.u32(ov + 8) as u64 | (m.u32(ov + 12) as u64) << 32;
    let data = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::File(file)) => {
            let size = file.metadata().map(|md| md.len()).unwrap_or(0);
            if offset >= size {
                return Err(ERROR_HANDLE_EOF);
            }
            let mut v = vec![0u8; count.min((size - offset) as u32) as usize];
            file.seek(SeekFrom::Start(offset)).and_then(|_| file.read_exact(&mut v)).map_err(|_| ERROR_HANDLE_EOF)?;
            Ok(v)
        }
        _ => Err(ERROR_INVALID_HANDLE),
    });
    cpu.eax = match data {
        Ok(v) => {
            for (k, &b) in v.iter().enumerate() {
                m.set_u8(buf.wrapping_add(k as u32), b);
            }
            m.set_u32(ov, 0); // Internal: STATUS_SUCCESS
            m.set_u32(ov + 4, v.len() as u32); // InternalHigh: bytes
            crate::threads::queue_apc(crate::threads::current_tid(), routine, [0, v.len() as u32, ov]);
            set_error(0);
            1
        }
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 20);
}

/// WriteFile(handle, buffer, count, written*, overlapped): into the overlay.
pub fn write_file(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, buf, count, written_out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let data: Vec<u8> = (0..count).map(|k| m.u8(buf.wrapping_add(k))).collect();
    let ok = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::File(file)) => file.write_all(&data).is_ok(),
        _ => false,
    });
    if written_out != 0 {
        m.set_u32(written_out, if ok { count } else { 0 });
    }
    cpu.eax = ok as u32;
    if !ok {
        set_error(ERROR_INVALID_HANDLE);
    }
    ret(m, cpu, 20);
}

/// SetFilePointer(handle, low, high*, method): 32-bit offsets (a high part
/// other than 0 or -1 is refused); returns the new low position.
pub fn set_file_pointer(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, low, high_p, method) = (arg(m, cpu, 0), arg(m, cpu, 1) as i32, arg(m, cpu, 2), arg(m, cpu, 3));
    let high = if high_p != 0 { m.u32(high_p) as i32 } else if low < 0 { -1 } else { 0 };
    let dist = ((high as i64) << 32) | (low as u32 as i64);
    let pos = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::File(file)) => file.seek(match method {
            0 => SeekFrom::Start(dist as u64),
            1 => SeekFrom::Current(dist),
            _ => SeekFrom::End(dist),
        }).ok(),
        _ => None,
    });
    cpu.eax = match pos {
        Some(p) => {
            if high_p != 0 {
                m.set_u32(high_p, (p >> 32) as u32);
            }
            p as u32
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            u32::MAX // INVALID_SET_FILE_POINTER
        }
    };
    ret(m, cpu, 16);
}

/// GetFileSize(handle, high*): the low 32 bits; the high part if asked.
pub fn get_file_size(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, high_p) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let size = with_files(|f| match f.handles.get(&h) {
        Some(Open::File(file)) => file.metadata().ok().map(|md| md.len()),
        _ => None,
    });
    cpu.eax = match size {
        Some(s) => {
            if high_p != 0 {
                m.set_u32(high_p, (s >> 32) as u32);
            }
            s as u32
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            u32::MAX
        }
    };
    ret(m, cpu, 8);
}

/// CloseHandle(handle): files and find handles; unknown handles fail.
pub fn close_handle(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    cpu.eax = (crate::threads::close(h) || with_files(|f| f.handles.remove(&h).is_some())) as u32;
    if cpu.eax == 0 {
        set_error(ERROR_INVALID_HANDLE);
    }
    ret(m, cpu, 4);
}

pub fn flush_file_buffers(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    cpu.eax = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::File(file)) => file.flush().is_ok(),
        _ => false,
    }) as u32;
    ret(m, cpu, 4);
}

fn file_attributes(m: &mut dyn Mem, cpu: &mut Cpu, wide: bool) {
    let name = if wide { guest_str_w(m, arg(m, cpu, 0)) } else { guest_str_a(m, arg(m, cpu, 0)) };
    let attrs = with_files(|f| {
        let md = std::fs::metadata(place_in(f, &name)?.read).ok()?;
        Some(if md.is_dir() { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL })
    });
    cpu.eax = attrs.unwrap_or_else(|| {
        set_error(ERROR_FILE_NOT_FOUND);
        INVALID_FILE_ATTRIBUTES
    });
    ret(m, cpu, 4);
}
pub fn get_file_attributes_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    file_attributes(m, cpu, false)
}
pub fn get_file_attributes_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    file_attributes(m, cpu, true)
}

/// CreateDirectoryA(path, security): in the overlay.
pub fn create_directory_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let name = guest_str_a(m, arg(m, cpu, 0));
    let r = with_files(|f| {
        let pl = place_in(f, &name).ok_or(ERROR_ACCESS_DENIED)?;
        if pl.read.exists() {
            return Err(ERROR_ALREADY_EXISTS);
        }
        std::fs::create_dir_all(pl.write).map_err(|_| ERROR_PATH_NOT_FOUND)
    });
    cpu.eax = r.is_ok() as u32;
    if let Err(e) = r {
        set_error(e);
    }
    ret(m, cpu, 8);
}

/// FindFirstFileA(pattern, WIN32_FIND_DATAA*): `dir\*` or `dir\*.ext`
/// patterns over the overlay and the install (overlay entries win).
pub fn find_first_file_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (pattern, data) = (guest_str_a(m, arg(m, cpu, 0)), arg(m, cpu, 1));
    let r = with_files(|f| -> Result<(u32, Found), u32> {
        let pl = place_in(f, &pattern).ok_or(ERROR_ACCESS_DENIED)?;
        let mask = pl.write.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let dirs: Vec<PathBuf> = [Some(&pl.write), pl.install.as_ref()].into_iter().flatten()
            .map(|p| p.parent().unwrap_or(Path::new("")).to_path_buf()).collect();
        let mut found: Vec<Found> = Vec::new();
        for root in dirs {
            for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if wildcard(&mask, &name.to_lowercase()) && !found.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
                    let md = e.metadata().ok();
                    found.push(Found { name, dir: md.as_ref().is_some_and(|m| m.is_dir()),
                                       size: md.map(|m| m.len()).unwrap_or(0) });
                }
            }
        }
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        if found.is_empty() {
            return Err(ERROR_FILE_NOT_FOUND);
        }
        let first = found.remove(0);
        Ok((new_handle(f, Open::Find(found)), first))
    });
    cpu.eax = match r {
        Ok((h, first)) => {
            write_find_data(m, data, &first);
            h
        }
        Err(e) => {
            set_error(e);
            INVALID_HANDLE_VALUE
        }
    };
    ret(m, cpu, 8);
}

pub fn find_next_file_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, data) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let next = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::Find(rest)) if !rest.is_empty() => Ok(Some(rest.remove(0))),
        Some(Open::Find(_)) => Ok(None),
        _ => Err(()),
    });
    cpu.eax = match next {
        Ok(Some(e)) => {
            write_find_data(m, data, &e);
            1
        }
        Ok(None) => {
            set_error(ERROR_NO_MORE_FILES);
            0
        }
        Err(()) => {
            set_error(ERROR_INVALID_HANDLE);
            0
        }
    };
    ret(m, cpu, 8);
}

pub fn find_close(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    cpu.eax = with_files(|f| matches!(f.handles.remove(&h), Some(Open::Find(_)))) as u32;
    ret(m, cpu, 4);
}

/// WIN32_FIND_DATAA: attributes +0, sizes +28/+32, cFileName[260] +44.
fn write_find_data(m: &mut dyn Mem, p: u32, e: &Found) {
    crate::services::zero(m, p, 320);
    m.set_u32(p, if e.dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL });
    m.set_u32(p + 28, (e.size >> 32) as u32);
    m.set_u32(p + 32, e.size as u32);
    for (k, b) in e.name.bytes().take(259).enumerate() {
        m.set_u8(p + 44 + k as u32, b);
    }
}

/// `*` and `?` matching, case already folded.
fn wildcard(mask: &str, name: &str) -> bool {
    fn go(m: &[u8], n: &[u8]) -> bool {
        match (m.first(), n.first()) {
            (None, None) => true,
            (Some(b'*'), _) => go(&m[1..], n) || (!n.is_empty() && go(m, &n[1..])),
            (Some(b'?'), Some(_)) => go(&m[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => go(&m[1..], &n[1..]),
            _ => false,
        }
    }
    let mask = if mask == "*.*" { "*" } else { mask };
    go(mask.as_bytes(), name.as_bytes())
}

/// GetCurrentDirectoryA(size, buffer): the install path plus the guest's
/// relative directory; returns the length (or the needed size).
pub fn get_current_directory_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (size, buf) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let dir = with_files(|f| f.install.join(&f.cwd)).to_string_lossy().to_string();
    let bytes = dir.as_bytes();
    cpu.eax = if (bytes.len() as u32) < size {
        for (k, &b) in bytes.iter().chain([0u8].iter()).enumerate() {
            m.set_u8(buf.wrapping_add(k as u32), b);
        }
        bytes.len() as u32
    } else {
        bytes.len() as u32 + 1
    };
    ret(m, cpu, 8);
}

/// SetCurrentDirectoryA(path): only directories inside the roots.
pub fn set_current_directory_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let name = guest_str_a(m, arg(m, cpu, 0));
    let ok = with_files(|f| match relative(f, &name) {
        Some(rel) if read_path(f, &rel).is_dir() => {
            f.cwd = rel;
            true
        }
        _ => false,
    });
    if !ok {
        set_error(ERROR_PATH_NOT_FOUND);
    }
    cpu.eax = ok as u32;
    ret(m, cpu, 4);
}

/// The file a write-side operation may change: the overlay copy or a home
/// file. A file that exists only in the install is read-only for the
/// guest: ERROR_ACCESS_DENIED, which Windows also answers for a read-only
/// file (Microsoft Learn, DeleteFileA remarks).
fn writable_existing(pl: &Place) -> Result<PathBuf, u32> {
    if pl.write.exists() {
        return Ok(pl.write.clone());
    }
    match &pl.install {
        Some(orig) if orig.exists() => Err(ERROR_ACCESS_DENIED),
        _ => Err(missing_error(&pl.read)),
    }
}

/// ERROR_FILE_NOT_FOUND, or ERROR_PATH_NOT_FOUND when the directory is
/// missing too.
fn missing_error(p: &Path) -> u32 {
    if p.parent().is_some_and(Path::is_dir) { ERROR_FILE_NOT_FOUND } else { ERROR_PATH_NOT_FOUND }
}

/// Whether the directory a new file would go into exists in the guest's
/// view (the overlay's or the install's copy of it, or the home).
fn parent_exists(pl: &Place) -> bool {
    [Some(&pl.write), pl.install.as_ref()].into_iter().flatten().any(|p| p.parent().is_some_and(Path::is_dir))
}

fn bool_result(m: &mut dyn Mem, cpu: &mut Cpu, r: Result<(), u32>, args: u32) {
    cpu.eax = match r {
        Ok(()) => 1,
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, args);
}

/// DeleteFileA(lpFileName) (Microsoft Learn "DeleteFileA function
/// (fileapi.h)"): deletes a file in the overlay or the home. FALSE with
/// ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND (missing), ERROR_ACCESS_DENIED
/// (a directory, a file only the read-only install has, or a path outside
/// the roots). Caller: 0x00D0CDB0 (at 0x00D0CDC5).
pub fn delete_file_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let name = guest_str_a(m, arg(m, cpu, 0));
    let r = with_files(|f| {
        let pl = place_in(f, &name).ok_or(ERROR_ACCESS_DENIED)?;
        let p = writable_existing(&pl)?;
        if p.is_dir() {
            return Err(ERROR_ACCESS_DENIED);
        }
        std::fs::remove_file(p).map_err(|_| ERROR_ACCESS_DENIED)
    });
    bool_result(m, cpu, r, 4);
}

/// MoveFileA(lpExistingFileName, lpNewFileName) (Microsoft Learn
/// "MoveFileA function (winbase.h)"): renames a file or directory of the
/// overlay or the home. FALSE with ERROR_ALREADY_EXISTS (the new name
/// exists in the guest's view), ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND,
/// ERROR_ACCESS_DENIED (the source is only in the read-only install, or a
/// path is outside the roots). Caller: 0x00D0CDE0 (at 0x00D0CE0D).
pub fn move_file_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (from, to) = (guest_str_a(m, arg(m, cpu, 0)), guest_str_a(m, arg(m, cpu, 1)));
    let r = with_files(|f| {
        let src = place_in(f, &from).ok_or(ERROR_ACCESS_DENIED)?;
        let dst = place_in(f, &to).ok_or(ERROR_ACCESS_DENIED)?;
        let p = writable_existing(&src)?;
        if dst.read.exists() {
            return Err(ERROR_ALREADY_EXISTS);
        }
        if !parent_exists(&dst) {
            return Err(ERROR_PATH_NOT_FOUND);
        }
        if let Some(dir) = dst.write.parent() {
            std::fs::create_dir_all(dir).map_err(|_| ERROR_PATH_NOT_FOUND)?;
        }
        std::fs::rename(p, &dst.write).map_err(|_| ERROR_ACCESS_DENIED)
    });
    bool_result(m, cpu, r, 8);
}

/// CopyFileA(lpExistingFileName, lpNewFileName, bFailIfExists) (Microsoft
/// Learn "CopyFileA function (winbase.h)"): copies the guest's view of the
/// source into the overlay or the home. FALSE with ERROR_FILE_EXISTS (the
/// target exists and bFailIfExists), ERROR_FILE_NOT_FOUND /
/// ERROR_PATH_NOT_FOUND, ERROR_ACCESS_DENIED (a directory as source, a path
/// outside the roots). Caller: 0x00D0CFC0 (bFailIfExists FALSE).
pub fn copy_file_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (from, to, fail_if_exists) = (guest_str_a(m, arg(m, cpu, 0)), guest_str_a(m, arg(m, cpu, 1)), arg(m, cpu, 2));
    let r = with_files(|f| {
        let src = place_in(f, &from).ok_or(ERROR_ACCESS_DENIED)?;
        let dst = place_in(f, &to).ok_or(ERROR_ACCESS_DENIED)?;
        if !src.read.exists() {
            return Err(missing_error(&src.read));
        }
        if src.read.is_dir() {
            return Err(ERROR_ACCESS_DENIED);
        }
        if dst.read.exists() && fail_if_exists != 0 {
            return Err(ERROR_FILE_EXISTS);
        }
        if !parent_exists(&dst) {
            return Err(ERROR_PATH_NOT_FOUND);
        }
        if let Some(dir) = dst.write.parent() {
            std::fs::create_dir_all(dir).map_err(|_| ERROR_PATH_NOT_FOUND)?;
        }
        std::fs::copy(&src.read, &dst.write).map(|_| ()).map_err(|_| ERROR_ACCESS_DENIED)
    });
    bool_result(m, cpu, r, 12);
}

/// SetEndOfFile(hFile) (Microsoft Learn "SetEndOfFile function
/// (fileapi.h)"): the file ends at its current position. FALSE with
/// ERROR_ACCESS_DENIED for a handle opened for reading, ERROR_INVALID_HANDLE
/// for anything that is not an open file. Callers: MtFile::vf14 (at
/// 0x00D0D031) and MtFileEx::vf14 (at 0x00D0DE1A) (seek, SetEndOfFile,
/// seek back), the CRT's `__chsize_nolock`.
pub fn set_end_of_file(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    let r = with_files(|f| match f.handles.get_mut(&h) {
        Some(Open::File(file)) => {
            let pos = file.stream_position().map_err(|_| ERROR_INVALID_HANDLE)?;
            file.set_len(pos).map_err(|_| ERROR_ACCESS_DENIED)
        }
        _ => Err(ERROR_INVALID_HANDLE),
    });
    bool_result(m, cpu, r, 4);
}

/// The volume GetDiskFreeSpaceA reports for every path the guest may use:
/// one real NTFS volume, recorded like the D3D9 profile (the maintainer's
/// E: drive holding the install, 2026-10-11): 8 sectors per cluster, 512
/// bytes per sector, 16,192,600 of 122,092,543 clusters free. Fixed, so a
/// session does not depend on the host's disks.
pub const VOLUME: [u32; 4] = [8, 512, 16_192_600, 122_092_543];

/// GetDiskFreeSpaceA(lpRootPathName, lpSectorsPerCluster,
/// lpBytesPerSector, lpNumberOfFreeClusters, lpTotalNumberOfClusters)
/// (Microsoft Learn "GetDiskFreeSpaceA function (fileapi.h)"): fills the
/// non-NULL out-parameters from [`VOLUME`]; TRUE. The root may be NULL
/// (the current directory's volume), a drive ("X:" or "X:\") or any
/// directory on it (Windows 11 accepts all of these, measured); only the
/// install's and the sandbox's drives are modelled, other paths stop. The
/// caller MtFile::vf05 (at 0x00D0D109) opens a file synchronously instead
/// of overlapped when more than 0x800 clusters are free.
pub fn get_disk_free_space_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let root = arg(m, cpu, 0);
    if root != 0 {
        let path = guest_str_a(m, root);
        let drive = |p: &str| p.get(..2).filter(|d| d.ends_with(':')).map(str::to_ascii_uppercase);
        let known = with_files(|f| {
            let wanted = drive(&path);
            wanted.is_some() && [&f.install, &f.home].iter().any(|r| drive(&r.to_string_lossy()) == wanted)
        });
        if !known {
            panic!("GetDiskFreeSpaceA({path:?}) is not modelled: not on the install's or the sandbox's drive (crate::files)");
        }
    }
    for k in 0..4 {
        let p = arg(m, cpu, 1 + k);
        if p != 0 {
            m.set_u32(p, VOLUME[k as usize]);
        }
    }
    cpu.eax = 1;
    ret(m, cpu, 20);
}

/// SHELL32 SHCreateDirectoryExW(hwnd, pszPath, psa) (Microsoft Learn
/// "SHCreateDirectoryExW function (shlobj_core.h)"): creates the directory
/// and any missing parents in the overlay or the home and returns
/// ERROR_SUCCESS, or the error as the result: ERROR_ALREADY_EXISTS (a
/// directory or a file has that name), ERROR_BAD_PATHNAME (a relative or
/// empty path), ERROR_PATH_NOT_FOUND (a parent is a file),
/// ERROR_ACCESS_DENIED (outside the roots); `..` is resolved first (all
/// measured on Windows 11). Caller: MtRegistry's constructor 0x00CFFF00
/// (at 0x00CFFF82), for `<Documents>\CAPCOM\DRAGONS DOGMA DARK ARISEN`.
pub fn sh_create_directory_ex_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let path = guest_str_w(m, arg(m, cpu, 1));
    let absolute = path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\");
    let r = if !absolute {
        Err(ERROR_BAD_PATHNAME)
    } else {
        with_files(|f| {
            let pl = place_in(f, &path).ok_or(ERROR_ACCESS_DENIED)?;
            if pl.read.exists() {
                return Err(ERROR_ALREADY_EXISTS);
            }
            // a parent that is a file (in the guest's view) stops it
            let mut p = pl.read.parent();
            while let Some(dir) = p {
                if dir.is_file() {
                    return Err(ERROR_PATH_NOT_FOUND);
                }
                p = dir.parent();
            }
            std::fs::create_dir_all(&pl.write).map_err(|_| ERROR_PATH_NOT_FOUND)
        })
    };
    cpu.eax = r.err().unwrap_or(0);
    ret(m, cpu, 12);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "CreateFileA", create_file_a),
    ("KERNEL32.dll", "CreateFileW", create_file_w),
    ("KERNEL32.dll", "ReadFile", read_file),
    ("KERNEL32.dll", "ReadFileEx", read_file_ex),
    ("KERNEL32.dll", "WriteFile", write_file),
    ("KERNEL32.dll", "SetFilePointer", set_file_pointer),
    ("KERNEL32.dll", "GetFileSize", get_file_size),
    ("KERNEL32.dll", "CloseHandle", close_handle),
    ("KERNEL32.dll", "FlushFileBuffers", flush_file_buffers),
    ("KERNEL32.dll", "GetFileAttributesA", get_file_attributes_a),
    ("KERNEL32.dll", "GetFileAttributesW", get_file_attributes_w),
    ("KERNEL32.dll", "CreateDirectoryA", create_directory_a),
    ("KERNEL32.dll", "FindFirstFileA", find_first_file_a),
    ("KERNEL32.dll", "FindNextFileA", find_next_file_a),
    ("KERNEL32.dll", "FindClose", find_close),
    ("KERNEL32.dll", "GetCurrentDirectoryA", get_current_directory_a),
    ("KERNEL32.dll", "SetCurrentDirectoryA", set_current_directory_a),
    ("KERNEL32.dll", "DeleteFileA", delete_file_a),
    ("KERNEL32.dll", "MoveFileA", move_file_a),
    ("KERNEL32.dll", "CopyFileA", copy_file_a),
    ("KERNEL32.dll", "SetEndOfFile", set_end_of_file),
    ("KERNEL32.dll", "GetDiskFreeSpaceA", get_disk_free_space_a),
    ("SHELL32.dll", "SHCreateDirectoryExW", sh_create_directory_ex_w),
];
