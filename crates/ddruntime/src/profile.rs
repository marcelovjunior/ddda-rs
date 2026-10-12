//! Profile (ini) file services: GetPrivateProfileStringW and
//! WritePrivateProfileStringW over crate::files (the install read-only,
//! writes to the overlay or the home).
//!
//! The game's settings live in `<LocalAppData>\CAPCOM\DRAGONS DOGMA DARK
//! ARISEN\config.ini` (MtRegistry's constructor 0x00CFFF00, which keeps the
//! path at +4). Its reader 0x00D00010 (GetPrivateProfileStringW at
//! 0x00D000CB, default L"", nSize 0x200) and writer 0x00CFFD20
//! (WritePrivateProfileStringW at 0x00CFFE09) convert section, key and
//! value from and to code page 1252.
//!
//! Microsoft Learn ("GetPrivateProfileStringW function (winbase.h)",
//! "WritePrivateProfileStringW function (winbase.h)") gives the contract;
//! the file format details it leaves open were measured on Windows 11
//! 26200 (research/runtime/windows/Probe3.cs, Probe4.cs, Probe5.cs):
//! - A file starting with the UTF-16LE byte order mark is UTF-16 (and
//!   written back as such); any other file is code page 1252 (a UTF-8 BOM
//!   is just three characters of the first line).
//! - Lines end at CR, LF or CR LF. Spaces and tabs around section names,
//!   keys and values are ignored; a section line is `[` up to `]` or the
//!   end of the line; lines starting with `;` are comments; a line
//!   without `=` is no key. A value enclosed in matching quotes (`"` or
//!   `'`) loses them.
//! - Lookups take the first matching section (case-insensitive) and its
//!   first matching key; a later section with the same name is not
//!   searched.
//! - Writes keep the file's other bytes. A changed value replaces the text
//!   from `=` to the value's last non-blank character; a new key goes
//!   after the section's last line that is not blank or a comment; a new
//!   section is appended (after a CR LF if the file does not end in a line
//!   break). New lines end in CR LF; names are trimmed, values are written
//!   as given. Deleting from a missing file creates it empty.
//!
//! A file name without a path (Windows searches the Windows directory) or
//! a UTF-16BE file is not modelled (stop).
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_MORE_DATA: u32 = 234;

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
}

fn guest_opt_w(m: &dyn Mem, p: u32) -> Option<String> {
    (p != 0).then(|| crate::user32::guest_str_w(m, p))
}

const BLANKS: &[char] = &[' ', '\t'];

/// A profile file's text, line by line with each line's own terminator.
struct Text {
    unicode: bool,
    /// The file had bytes (a byte order mark counts).
    nonempty: bool,
    lines: Vec<(String, &'static str)>,
}

enum Line<'a> {
    Section(&'a str),
    Key { key: &'a str, eq: usize },
    Other,
}

fn classify(line: &str) -> Line<'_> {
    let t = line.trim_start_matches(BLANKS);
    if let Some(rest) = t.strip_prefix('[') {
        let name = rest.split(']').next().unwrap_or("");
        return Line::Section(name.trim_matches(BLANKS));
    }
    if t.starts_with(';') {
        return Line::Other;
    }
    match line.find('=') {
        Some(eq) => Line::Key { key: line[..eq].trim_matches(BLANKS), eq },
        None => Line::Other,
    }
}

fn decode(bytes: &[u8]) -> Text {
    let (unicode, body): (bool, String) = if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        (true, String::from_utf16_lossy(&units))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        panic!("a UTF-16BE profile file is not modelled (crate::profile)");
    } else {
        (false, bytes.iter().map(|&b| char::from_u32(crate::text::decode_1252(b) as u32).unwrap_or('\u{FFFD}')).collect())
    };
    let mut lines = Vec::new();
    let mut rest = body.as_str();
    while !rest.is_empty() {
        let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
        let (line, tail) = rest.split_at(end);
        let eol = if tail.starts_with("\r\n") { "\r\n" } else if tail.starts_with('\r') { "\r" } else if tail.starts_with('\n') { "\n" } else { "" };
        lines.push((line.to_string(), eol));
        rest = &tail[eol.len()..];
    }
    Text { unicode, nonempty: !bytes.is_empty(), lines }
}

fn encode(t: &Text) -> Vec<u8> {
    let body: String = t.lines.iter().map(|(l, e)| format!("{l}{e}")).collect();
    if t.unicode {
        [0xFF, 0xFE].into_iter().chain(body.encode_utf16().flat_map(u16::to_le_bytes)).collect()
    } else {
        body.encode_utf16().map(|u| crate::text::encode_1252(u, true).unwrap_or(b'?')).collect()
    }
}

impl Text {
    /// The first section with this name: (header line, end of section).
    fn section(&self, name: &str) -> Option<(usize, usize)> {
        let start = self.lines.iter().position(|(l, _)| matches!(classify(l), Line::Section(s) if s.eq_ignore_ascii_case(name)))?;
        let end = (start + 1..self.lines.len()).find(|&i| matches!(classify(&self.lines[i].0), Line::Section(_))).unwrap_or(self.lines.len());
        Some((start, end))
    }

    fn sections(&self) -> Vec<String> {
        self.lines.iter().filter_map(|(l, _)| match classify(l) {
            Line::Section(s) => Some(s.to_string()),
            _ => None,
        }).collect()
    }

    fn keys(&self, (start, end): (usize, usize)) -> Vec<(usize, String)> {
        (start + 1..end).filter_map(|i| match classify(&self.lines[i].0) {
            Line::Key { key, .. } => Some((i, key.to_string())),
            _ => None,
        }).collect()
    }

    fn find_key(&self, sec: (usize, usize), key: &str) -> Option<usize> {
        self.keys(sec).into_iter().find(|(_, k)| k.eq_ignore_ascii_case(key)).map(|(i, _)| i)
    }

    /// Insert a line at `at` (ending in CR LF), terminating the line before
    /// it first if that one had no line break (the end of the file).
    fn insert(&mut self, at: usize, line: String) {
        if at > 0 && self.lines[at - 1].1.is_empty() {
            self.lines[at - 1].1 = "\r\n";
        }
        self.lines.insert(at, (line, "\r\n"));
    }
}

/// A key line's value: trimmed, and without enclosing matching quotes.
fn value_of(line: &str, eq: usize) -> String {
    let v = line[eq + 1..].trim_matches(BLANKS);
    let b = v.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// The file as the guest sees it: Ok(None) if it does not exist.
fn read_file(name: &str) -> Result<Option<(crate::files::Place, Text)>, u32> {
    if !name.contains(['\\', '/']) {
        panic!("profile file {name:?} without a path (the Windows directory) is not modelled (crate::profile)");
    }
    let place = crate::files::place(name).ok_or(ERROR_ACCESS_DENIED)?;
    match std::fs::read(&place.read) {
        Ok(bytes) => {
            let text = decode(&bytes);
            Ok(Some((place, text)))
        }
        Err(_) => Ok(None),
    }
}

/// Copy `units` and a NUL into the guest buffer at `buf` (it fits).
fn put(m: &mut dyn Mem, buf: u32, units: &[u16]) {
    for (k, &u) in units.iter().chain([0u16].iter()).enumerate() {
        m.set_u16(buf + 2 * k as u32, u);
    }
}

/// KERNEL32 `GetPrivateProfileStringW(lpAppName, lpKeyName, lpDefault,
/// lpReturnedString, nSize, lpFileName)` (stdcall, 24 bytes). Returns the
/// characters copied, without the NUL.
/// - A value: copied with its NUL, last error ERROR_SUCCESS. If it has
///   nSize - 1 characters or more (measured: an exact fit counts), the
///   first nSize - 1 are copied, the result is nSize - 1 and the last error
///   ERROR_MORE_DATA.
/// - lpAppName NULL: every section name; lpKeyName NULL: every key of the
///   section; each with a NUL and a final NUL (an empty list is one NUL).
///   If the list has nSize - 1 characters or more, its first nSize - 2 are
///   followed by two NULs and the result is nSize - 2 (ERROR_MORE_DATA).
/// - Not found (file, section or key): lpDefault (NULL: empty) without
///   trailing blanks, cut to nSize - 1 characters; last error
///   ERROR_FILE_NOT_FOUND.
/// - nSize 0: nothing is written, the result is 0.
pub fn get_private_profile_string_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (app, key, def) = (guest_opt_w(m, arg(m, cpu, 0)), guest_opt_w(m, arg(m, cpu, 1)), guest_opt_w(m, arg(m, cpu, 2)));
    let (buf, size, file) = (arg(m, cpu, 3), arg(m, cpu, 4), crate::user32::guest_str_w(m, arg(m, cpu, 5)));
    let text = read_file(&file).ok().flatten().map(|(_, t)| t);
    // the answer: a list, a value, or not found
    enum Answer {
        List(Vec<String>),
        Value(String),
        Missing,
    }
    let answer = match (&text, &app, &key) {
        (None, _, _) => Answer::Missing,
        (Some(t), None, _) => Answer::List(t.sections()),
        (Some(t), Some(a), k) => match (t.section(a), k) {
            (None, _) => Answer::Missing,
            (Some(sec), None) => Answer::List(t.keys(sec).into_iter().map(|(_, k)| k).collect()),
            (Some(sec), Some(k)) => match t.find_key(sec, k) {
                Some(i) => {
                    let line = &t.lines[i].0;
                    let Line::Key { eq, .. } = classify(line) else { unreachable!() };
                    Answer::Value(value_of(line, eq))
                }
                None => Answer::Missing,
            },
        },
    };
    let n = size as usize;
    cpu.eax = match answer {
        Answer::Missing => {
            let d: Vec<u16> = def.unwrap_or_default().trim_end_matches(BLANKS).encode_utf16().collect();
            set_error(ERROR_FILE_NOT_FOUND);
            if n == 0 {
                0
            } else {
                let cut = &d[..d.len().min(n - 1)];
                put(m, buf, cut);
                cut.len() as u32
            }
        }
        Answer::Value(v) => {
            let v: Vec<u16> = v.encode_utf16().collect();
            if n > 0 && v.len() < n - 1 {
                put(m, buf, &v);
                set_error(0);
                v.len() as u32
            } else {
                if n > 0 {
                    put(m, buf, &v[..n - 1]);
                }
                set_error(ERROR_MORE_DATA);
                n.saturating_sub(1) as u32
            }
        }
        Answer::List(names) => {
            let joined: Vec<u16> = names.iter().flat_map(|s| s.encode_utf16().chain([0])).collect();
            if names.is_empty() && n > 0 {
                m.set_u16(buf, 0);
                set_error(0);
                0
            } else if n > 0 && joined.len() < n - 1 {
                put(m, buf, &joined);
                set_error(0);
                joined.len() as u32
            } else {
                if n >= 2 {
                    put(m, buf, &joined[..n - 2]);
                    m.set_u16(buf + 2 * (n as u32 - 1), 0);
                } else if n == 1 {
                    m.set_u16(buf, 0);
                }
                set_error(ERROR_MORE_DATA);
                n.saturating_sub(2) as u32
            }
        }
    };
    ret(m, cpu, 24);
}

/// KERNEL32 `WritePrivateProfileStringW(lpAppName, lpKeyName, lpString,
/// lpFileName)` (stdcall, 16 bytes): sets a key (creating the file and
/// the section as needed), deletes a key (lpString NULL) or a whole
/// section (lpKeyName NULL); TRUE, the last error untouched (measured).
/// Deleting something missing succeeds. FALSE with ERROR_FILE_NOT_FOUND
/// for lpAppName NULL (the documented cache flush; measured),
/// ERROR_PATH_NOT_FOUND when the file's directory does not exist,
/// ERROR_ACCESS_DENIED outside the roots.
pub fn write_private_profile_string_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (app, key, value) = (guest_opt_w(m, arg(m, cpu, 0)), guest_opt_w(m, arg(m, cpu, 1)), guest_opt_w(m, arg(m, cpu, 2)));
    let file = crate::user32::guest_str_w(m, arg(m, cpu, 3));
    let r = (|| -> Result<(), u32> {
        let app = app.ok_or(ERROR_FILE_NOT_FOUND)?;
        let (place, mut t) = match read_file(&file)? {
            Some(x) => x,
            None => {
                let place = crate::files::place(&file).ok_or(ERROR_ACCESS_DENIED)?;
                (place, Text { unicode: false, nonempty: false, lines: Vec::new() })
            }
        };
        let app = app.trim_matches(BLANKS);
        match (t.section(app), key, value) {
            (None, None, _) | (None, Some(_), None) => {}
            (Some((start, end)), None, _) => {
                t.lines.drain(start..end);
            }
            (Some(sec), Some(k), None) => {
                if let Some(i) = t.find_key(sec, k.trim_matches(BLANKS)) {
                    t.lines.remove(i);
                }
            }
            (Some(sec), Some(k), Some(v)) => {
                let k = k.trim_matches(BLANKS);
                match t.find_key(sec, k) {
                    Some(i) => {
                        let line = &t.lines[i].0;
                        let Line::Key { eq, .. } = classify(line) else { unreachable!() };
                        let kept_tail = &line[eq + 1..][line[eq + 1..].trim_end_matches(BLANKS).len()..];
                        t.lines[i].0 = format!("{}{v}{kept_tail}", &line[..=eq]);
                    }
                    None => {
                        let (start, end) = sec;
                        let after = (start + 1..end).rev()
                            .find(|&j| {
                                let l = t.lines[j].0.trim_matches(BLANKS);
                                !l.is_empty() && !l.starts_with(';')
                            })
                            .unwrap_or(start);
                        t.insert(after + 1, format!("{k}={v}"));
                    }
                }
            }
            (None, Some(k), Some(v)) => {
                let n = t.lines.len();
                if t.nonempty && t.lines.last().is_none_or(|(_, e)| e.is_empty()) {
                    // the file does not end with a line break
                    match t.lines.last_mut() {
                        Some(last) => last.1 = "\r\n",
                        None => t.lines.push((String::new(), "\r\n")),
                    }
                }
                let at = t.lines.len().max(n);
                t.lines.insert(at, (format!("[{app}]"), "\r\n"));
                t.lines.insert(at + 1, (format!("{}={v}", k.trim_matches(BLANKS)), "\r\n"));
            }
        }
        let out = place.write;
        if let (Some(dir), Some(orig)) = (out.parent(), &place.install) {
            // the overlay mirrors a directory the install (or overlay) has
            if !orig.parent().is_some_and(std::path::Path::is_dir) && !dir.is_dir() {
                return Err(ERROR_PATH_NOT_FOUND);
            }
            std::fs::create_dir_all(dir).map_err(|_| ERROR_PATH_NOT_FOUND)?;
        } else if !out.parent().is_some_and(std::path::Path::is_dir) {
            return Err(ERROR_PATH_NOT_FOUND);
        }
        std::fs::write(&out, encode(&t)).map_err(|_| ERROR_ACCESS_DENIED)
    })();
    cpu.eax = match r {
        Ok(()) => 1,
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 16);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "GetPrivateProfileStringW", get_private_profile_string_w),
    ("KERNEL32.dll", "WritePrivateProfileStringW", write_private_profile_string_w),
];
