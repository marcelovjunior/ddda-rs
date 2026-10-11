//! File services: the KERNEL32 file API over two roots (docs/RUNTIME.md,
//! "Platform services").
//!
//! - The **install** (the directory of the player's DDDA.exe): read-only.
//!   The runtime never creates, writes, renames or deletes anything there.
//! - The **sandbox** ([`crate::services::sandbox_home`]): an overlay that
//!   mirrors install-relative paths. Every write lands here; reads look here
//!   first, so the guest sees its own writes.
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
const ERROR_FILE_EXISTS: u32 = 80;
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
    sandbox: PathBuf,
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
        let rel = relative(f, &name).ok_or(ERROR_ACCESS_DENIED)?;
        let write = access & GENERIC_WRITE != 0 || matches!(disposition, 1 | 2 | 5);
        let file = if write {
            // writes go to the overlay; existing install contents are copied first
            let over = f.sandbox.join(&rel);
            if let Some(dir) = over.parent() {
                std::fs::create_dir_all(dir).map_err(|_| ERROR_PATH_NOT_FOUND)?;
            }
            let existed = over.exists() || f.install.join(&rel).exists();
            if !over.exists() && f.install.join(&rel).is_file() && disposition != 2 && disposition != 5 {
                std::fs::copy(f.install.join(&rel), &over).map_err(|_| ERROR_ACCESS_DENIED)?;
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
            let path = read_path(f, &rel);
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
        let rel = relative(f, &name)?;
        let md = std::fs::metadata(read_path(f, &rel)).ok()?;
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
        let rel = relative(f, &name).ok_or(ERROR_ACCESS_DENIED)?;
        if read_path(f, &rel).exists() {
            return Err(ERROR_ALREADY_EXISTS);
        }
        std::fs::create_dir_all(f.sandbox.join(rel)).map_err(|_| ERROR_PATH_NOT_FOUND)
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
        let rel = relative(f, &pattern).ok_or(ERROR_ACCESS_DENIED)?;
        let mask = rel.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let dir = rel.parent().unwrap_or(Path::new("")).to_path_buf();
        let mut found: Vec<Found> = Vec::new();
        for root in [f.sandbox.join(&dir), f.install.join(&dir)] {
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

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "CreateFileA", create_file_a),
    ("KERNEL32.dll", "CreateFileW", create_file_w),
    ("KERNEL32.dll", "ReadFile", read_file),
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
];
