//! Read-only access to data tables in the player's installed `DDDA.exe`.
//!
//! Several systems need bulk tables that live in the executable's `.rdata`
//! and `.data` (skill records, name tables, action tables, event tables).
//! The constants policy in `AGENTS.md` forbids copying them into the repo,
//! so they are read at run time from the player's own exe. This crate does
//! that once for everyone:
//!
//! * [`locate`] finds `DDDA.exe` next to `nativePC`;
//! * [`Exe::parse`] checks the build by PE timestamp and refuses any other;
//! * [`Exe::bytes`] maps a virtual address to file bytes, and the typed
//!   readers ([`Exe::u32`], [`Exe::u32s`], [`Exe::cstr`], [`Exe::record`]...)
//!   build on it;
//! * [`Exe::class_names`] names reflected classes from their `MtDTI`
//!   registration code.
//!
//! The Steam exe is wrapped by SteamStub, but every section this crate reads
//! (`.text`, `.rdata`, `.data`) has the same bytes in the installed exe and
//! in the unpacked analysis copy (`METHODOLOGY.md`), so the installed file
//! can be read directly. Addresses are virtual addresses of the supported
//! build, as everywhere in the repo.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// PE timestamp (`IMAGE_FILE_HEADER.TimeDateStamp`) of the only supported
/// build, DDDA.exe 2017-12-13 Steam (`METHODOLOGY.md`).
pub const PE_TIMESTAMP: u32 = 1_513_180_209;
/// Human name of the supported build.
pub const BUILD: &str = "DDDA.exe 2017-12-13 Steam";
/// The `MtDTI` constructor. Every reflected class registers itself with
/// `push name; mov ecx, dti; call 0x00cf5780` (e.g. 0x0136ddf0 for
/// `cPlActWpnDaggerCstmHyakuretsu`).
pub const DTI_CONSTRUCTOR_VA: u32 = 0x00cf_5780;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.0
    }
}

/// `DDDA.exe` in the directory that holds `nativePC` (the Steam layout).
/// The file name is matched without regard to case.
pub fn locate(native_pc: &Path) -> Result<PathBuf, Error> {
    let dir = native_pc
        .parent()
        .ok_or_else(|| Error(format!("{} has no parent directory", native_pc.display())))?;
    let entries = std::fs::read_dir(dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().eq_ignore_ascii_case("DDDA.exe") && e.path().is_file() {
            return Ok(e.path());
        }
    }
    Err(Error(format!("no DDDA.exe in {}", dir.display())))
}

/// A `DDDA.exe` read into memory.
pub struct ExeFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

impl ExeFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let bytes = std::fs::read(&path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        Ok(Self { path, bytes })
    }

    /// Opens the exe next to `nativePC` ([`locate`]).
    pub fn next_to(native_pc: &Path) -> Result<Self, Error> {
        Self::open(locate(native_pc)?)
    }

    /// Parses and checks the build; the error names the file.
    pub fn image(&self) -> Result<Exe<'_>, Error> {
        Exe::parse(&self.bytes).map_err(|e| Error(format!("{}: {e}", self.path.display())))
    }
}

/// One PE section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// Virtual address (image base included).
    pub va: u32,
    pub virtual_size: u32,
    pub raw_size: u32,
    pub raw_offset: u32,
}

impl Section {
    /// Bytes of the section that exist in the file (the rest is zeroed at
    /// load time and cannot be read from disk).
    fn file_len(&self) -> u32 {
        self.virtual_size.min(self.raw_size)
    }
}

/// A parsed image of the supported build.
pub struct Exe<'a> {
    bytes: &'a [u8],
    base: u32,
    timestamp: u32,
    sections: Vec<Section>,
}

fn rd16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn rd32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// `yyyy-mm-dd` of a Unix timestamp (UTC), for error messages.
fn date(ts: u32) -> String {
    // Days to civil date (H. Hinnant's algorithm).
    let z = i64::from(ts) / 86_400 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl<'a> Exe<'a> {
    /// Parses the PE headers and refuses every build but [`PE_TIMESTAMP`].
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let exe = Self::parse_any(bytes)?;
        if exe.timestamp != PE_TIMESTAMP {
            return Err(Error(format!(
                "unsupported DDDA.exe build: PE timestamp {} ({}); ddda-rs reads only {BUILD} \
                 (timestamp {PE_TIMESTAMP}, {})",
                exe.timestamp,
                date(exe.timestamp),
                date(PE_TIMESTAMP)
            )));
        }
        Ok(exe)
    }

    /// Parses the PE headers without checking the build. Addresses in this
    /// repo are meaningless for other builds; use only for inspection.
    pub fn parse_any(bytes: &'a [u8]) -> Result<Self, Error> {
        let bad = |what: &str| Error(format!("not a PE image ({what})"));
        if bytes.get(..2) != Some(&b"MZ"[..]) {
            return Err(bad("no MZ header"));
        }
        let pe = rd32(bytes, 0x3c).ok_or_else(|| bad("truncated"))? as usize;
        if bytes.get(pe..pe + 4) != Some(&b"PE\0\0"[..]) {
            return Err(bad("no PE signature"));
        }
        let count = rd16(bytes, pe + 6).ok_or_else(|| bad("truncated"))? as usize;
        let timestamp = rd32(bytes, pe + 8).ok_or_else(|| bad("truncated"))?;
        let optional = rd16(bytes, pe + 20).ok_or_else(|| bad("truncated"))? as usize;
        if rd16(bytes, pe + 24) != Some(0x10b) {
            return Err(bad("not a 32-bit image"));
        }
        let base = rd32(bytes, pe + 24 + 28).ok_or_else(|| bad("truncated"))?;
        let mut sections = Vec::with_capacity(count);
        for i in 0..count {
            let s = pe + 24 + optional + 40 * i;
            let h = bytes.get(s..s + 40).ok_or_else(|| bad("truncated section table"))?;
            let name = h[..8].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
            sections.push(Section {
                name,
                virtual_size: rd32(h, 8).unwrap(),
                va: base.wrapping_add(rd32(h, 12).unwrap()),
                raw_size: rd32(h, 16).unwrap(),
                raw_offset: rd32(h, 20).unwrap(),
            });
        }
        Ok(Self { bytes, base, timestamp, sections })
    }

    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }
    pub fn image_base(&self) -> u32 {
        self.base
    }
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Name of the section holding `va` (`.text`, `.rdata`, `.data`...).
    pub fn section_of(&self, va: u32) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| va >= s.va && va - s.va < s.virtual_size)
            .map(|s| s.name.as_str())
    }

    /// `len` bytes at `va`. The whole range must lie in the file-backed part
    /// of one section.
    pub fn bytes(&self, va: u32, len: usize) -> Result<&'a [u8], Error> {
        let s = self
            .sections
            .iter()
            .find(|s| va >= s.va && va - s.va < s.file_len())
            .ok_or_else(|| Error(format!("{va:#010x} is not in the file-backed part of a section")))?;
        let rel = (va - s.va) as usize;
        if rel + len > s.file_len() as usize {
            return Err(Error(format!("{va:#010x}+{len:#x} runs past the end of {}", s.name)));
        }
        let o = s.raw_offset as usize + rel;
        self.bytes
            .get(o..o + len)
            .ok_or_else(|| Error(format!("{va:#010x}+{len:#x} runs past the end of the file")))
    }

    fn array<const N: usize>(&self, va: u32) -> Result<[u8; N], Error> {
        Ok(self.bytes(va, N)?.try_into().unwrap())
    }

    pub fn u8(&self, va: u32) -> Result<u8, Error> {
        Ok(self.bytes(va, 1)?[0])
    }
    pub fn u16(&self, va: u32) -> Result<u16, Error> {
        self.array(va).map(u16::from_le_bytes)
    }
    pub fn i16(&self, va: u32) -> Result<i16, Error> {
        self.array(va).map(i16::from_le_bytes)
    }
    pub fn u32(&self, va: u32) -> Result<u32, Error> {
        self.array(va).map(u32::from_le_bytes)
    }
    pub fn i32(&self, va: u32) -> Result<i32, Error> {
        self.array(va).map(i32::from_le_bytes)
    }
    pub fn f32(&self, va: u32) -> Result<f32, Error> {
        self.array(va).map(f32::from_le_bytes)
    }

    /// `count` consecutive values starting at `va`.
    pub fn u8s(&self, va: u32, count: usize) -> Result<Vec<u8>, Error> {
        Ok(self.bytes(va, count)?.to_vec())
    }
    pub fn u16s(&self, va: u32, count: usize) -> Result<Vec<u16>, Error> {
        let b = self.bytes(va, count * 2)?;
        Ok(b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect())
    }
    pub fn u32s(&self, va: u32, count: usize) -> Result<Vec<u32>, Error> {
        let b = self.bytes(va, count * 4)?;
        Ok(b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect())
    }
    pub fn f32s(&self, va: u32, count: usize) -> Result<Vec<f32>, Error> {
        let b = self.bytes(va, count * 4)?;
        Ok(b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }

    /// A NUL-terminated string of printable ASCII (at most 1024 bytes).
    pub fn cstr(&self, va: u32) -> Result<String, Error> {
        let s = self
            .sections
            .iter()
            .find(|s| va >= s.va && va - s.va < s.file_len())
            .ok_or_else(|| Error(format!("{va:#010x} is not in the file-backed part of a section")))?;
        let rest = self.bytes(va, (s.file_len() - (va - s.va)).min(1025) as usize)?;
        let n = rest
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| Error(format!("unterminated string at {va:#010x}")))?;
        let text = &rest[..n];
        if !text.iter().all(|&c| (0x20..0x7f).contains(&c) || c == b'\t') {
            return Err(Error(format!("non-ASCII string at {va:#010x}")));
        }
        Ok(text.iter().map(|&c| c as char).collect())
    }

    /// One fixed-size record at `va`.
    pub fn record(&self, va: u32, size: usize) -> Result<Record<'a>, Error> {
        Ok(Record { va, bytes: self.bytes(va, size)? })
    }

    /// `count` records of `stride` bytes starting at `va`.
    pub fn records(&self, va: u32, stride: usize, count: usize) -> Result<Vec<Record<'a>>, Error> {
        let all = self.bytes(va, stride * count)?;
        Ok(all
            .chunks_exact(stride.max(1))
            .take(count)
            .enumerate()
            .map(|(i, bytes)| Record { va: va + (i * stride) as u32, bytes })
            .collect())
    }

    /// Class names of the reflected (`MtDTI`) classes, keyed by the address
    /// of each class's `MtDTI` object. Found by scanning `.text` for the
    /// registration calls `68 <name> B9 <dti> E8 <rel32 -> 0x00cf5780>`.
    /// The `MtDTI` objects themselves live in uninitialised `.data`, so this
    /// is the only way to name them from the file.
    pub fn class_names(&self) -> HashMap<u32, String> {
        let mut out = HashMap::new();
        for s in self.sections.iter().filter(|s| s.name == ".text") {
            let Ok(text) = self.bytes(s.va, s.file_len() as usize) else { continue };
            for i in 0..text.len().saturating_sub(14) {
                if text[i] != 0x68 || text[i + 5] != 0xb9 || text[i + 10] != 0xe8 {
                    continue;
                }
                let word = |o: usize| u32::from_le_bytes(text[o..o + 4].try_into().unwrap());
                let next = s.va + i as u32 + 15;
                if next.wrapping_add(word(i + 11)) != DTI_CONSTRUCTOR_VA {
                    continue;
                }
                if let Ok(name) = self.cstr(word(i + 1)) {
                    if !name.is_empty() {
                        out.insert(word(i + 6), name);
                    }
                }
            }
        }
        out
    }
}

/// A fixed-size record read from the exe, with typed field access by
/// offset. Out-of-range offsets are errors.
#[derive(Debug, Clone, Copy)]
pub struct Record<'a> {
    pub va: u32,
    pub bytes: &'a [u8],
}

impl Record<'_> {
    fn array<const N: usize>(&self, off: usize) -> Result<[u8; N], Error> {
        self.bytes
            .get(off..off + N)
            .map(|b| b.try_into().unwrap())
            .ok_or_else(|| Error(format!("record {:#010x}: offset {off:#x} out of range", self.va)))
    }
    pub fn u8(&self, off: usize) -> Result<u8, Error> {
        self.array::<1>(off).map(|b| b[0])
    }
    pub fn u16(&self, off: usize) -> Result<u16, Error> {
        self.array(off).map(u16::from_le_bytes)
    }
    pub fn i16(&self, off: usize) -> Result<i16, Error> {
        self.array(off).map(i16::from_le_bytes)
    }
    pub fn u32(&self, off: usize) -> Result<u32, Error> {
        self.array(off).map(u32::from_le_bytes)
    }
    pub fn i32(&self, off: usize) -> Result<i32, Error> {
        self.array(off).map(i32::from_le_bytes)
    }
    pub fn f32(&self, off: usize) -> Result<f32, Error> {
        self.array(off).map(f32::from_le_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built PE32 image: `.text` at VA 0x00401000 (file 0x200,
    /// 0x200 bytes), `.rdata` at 0x00402000 (file 0x400, 0x200 bytes) and
    /// `.data` at 0x00403000 with 0x100 file bytes but 0x1000 in memory.
    pub(crate) fn image(timestamp: u32, text: &[u8], rdata: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 0x700];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x86..0x88].copy_from_slice(&3u16.to_le_bytes());
        b[0x88..0x8c].copy_from_slice(&timestamp.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&0xe0u16.to_le_bytes());
        b[0x98..0x9a].copy_from_slice(&0x10bu16.to_le_bytes());
        b[0x98 + 28..0x98 + 32].copy_from_slice(&0x0040_0000u32.to_le_bytes());
        let secs: [(&[u8; 8], u32, u32, u32, u32); 3] = [
            (b".text\0\0\0", 0x200, 0x1000, 0x200, 0x200),
            (b".rdata\0\0", 0x200, 0x2000, 0x200, 0x400),
            (b".data\0\0\0", 0x1000, 0x3000, 0x100, 0x600),
        ];
        for (i, (name, vsize, va, raw, off)) in secs.iter().enumerate() {
            let s = 0x98 + 0xe0 + 40 * i;
            b[s..s + 8].copy_from_slice(*name);
            b[s + 8..s + 12].copy_from_slice(&vsize.to_le_bytes());
            b[s + 12..s + 16].copy_from_slice(&va.to_le_bytes());
            b[s + 16..s + 20].copy_from_slice(&raw.to_le_bytes());
            b[s + 20..s + 24].copy_from_slice(&off.to_le_bytes());
        }
        b[0x200..0x200 + text.len()].copy_from_slice(text);
        b[0x400..0x400 + rdata.len()].copy_from_slice(rdata);
        b
    }

    #[test]
    fn checks_the_build() {
        assert!(Exe::parse(b"MZ").is_err());
        assert!(Exe::parse(&[0u8; 0x200]).is_err());
        let e = Exe::parse(&image(1_600_000_000, &[], &[])).err().unwrap();
        assert!(e.0.contains("1600000000 (2020-09-13)"), "{e}");
        assert!(e.0.contains("2017-12-13"), "{e}");
        let ok = image(PE_TIMESTAMP, &[], &[]);
        let exe = Exe::parse(&ok).unwrap();
        assert_eq!(exe.timestamp(), PE_TIMESTAMP);
        assert_eq!(exe.image_base(), 0x0040_0000);
        assert_eq!(exe.sections().len(), 3);
        assert!(Exe::parse_any(&image(1, &[], &[])).is_ok());
    }

    #[test]
    fn typed_reads() {
        let mut rdata = b"cFoo\0".to_vec();
        rdata.extend_from_slice(&[0x34, 0x12, 0xff, 0xff]); // 0x402005: u16 0x1234, i16 -1
        rdata.extend_from_slice(&1.5f32.to_le_bytes()); // 0x402009
        rdata.extend_from_slice(&[1, 0, 0, 0, 2, 0, 0, 0]); // 0x40200d: u32s 1, 2
        rdata.extend_from_slice(b"\x01\x02bad"); // 0x402015: non-ASCII string
        let b = image(PE_TIMESTAMP, &[], &rdata);
        let exe = Exe::parse(&b).unwrap();
        assert_eq!(exe.cstr(0x0040_2000).unwrap(), "cFoo");
        assert_eq!(exe.u16(0x0040_2005).unwrap(), 0x1234);
        assert_eq!(exe.i16(0x0040_2007).unwrap(), -1);
        assert_eq!(exe.f32(0x0040_2009).unwrap(), 1.5);
        assert_eq!(exe.u32s(0x0040_200d, 2).unwrap(), [1, 2]);
        assert_eq!(exe.u16s(0x0040_2005, 2).unwrap(), [0x1234, 0xffff]);
        assert_eq!(exe.f32s(0x0040_2009, 1).unwrap(), [1.5]);
        assert_eq!(exe.u8s(0x0040_2000, 4).unwrap(), b"cFoo");
        assert!(exe.cstr(0x0040_2015).is_err());
        let r = exe.record(0x0040_2005, 8).unwrap();
        assert_eq!((r.u16(0).unwrap(), r.i16(2).unwrap(), r.f32(4).unwrap()), (0x1234, -1, 1.5));
        assert!(r.u32(6).is_err());
        let rs = exe.records(0x0040_200d, 4, 2).unwrap();
        assert_eq!((rs[1].va, rs[1].u32(0).unwrap()), (0x0040_2011, 2));
        assert_eq!(exe.section_of(0x0040_2000), Some(".rdata"));
        assert_eq!(exe.section_of(0x0040_3800), Some(".data"));
        // Ranges outside the file-backed bytes are refused.
        assert!(exe.u32(0x0040_31fe).is_err()); // past .data's 0x100 file bytes
        assert!(exe.u32(0x0040_3800).is_err()); // zero-fill part of .data
        assert!(exe.bytes(0x0040_21fe, 4).is_err()); // crosses the end of .rdata
        assert!(exe.u8(0x0050_0000).is_err());
        assert!(exe.u32(0x0040_3000).is_ok());
    }

    #[test]
    fn class_names_from_registration() {
        // push 0x00402000 ("cFoo"); mov ecx, 0x01234568; call DTI_CONSTRUCTOR_VA
        let mut text = vec![0x68, 0x00, 0x20, 0x40, 0x00, 0xb9, 0x68, 0x45, 0x23, 0x01, 0xe8];
        text.extend_from_slice(&DTI_CONSTRUCTOR_VA.wrapping_sub(0x0040_1000 + 15).to_le_bytes());
        // The same shape calling another function is ignored.
        text.extend_from_slice(&[0x68, 0x00, 0x20, 0x40, 0x00, 0xb9, 0, 0, 0, 2, 0xe8, 0, 0, 0, 0]);
        let b = image(PE_TIMESTAMP, &text, b"cFoo\0");
        let names = Exe::parse(&b).unwrap().class_names();
        assert_eq!(names.len(), 1);
        assert_eq!(names[&0x0123_4568], "cFoo");
    }

    #[test]
    fn locates_next_to_native_pc() {
        let dir = std::env::temp_dir().join(format!("ddexe-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nativePC")).unwrap();
        assert!(locate(&dir.join("nativePC")).is_err());
        std::fs::write(dir.join("ddda.EXE"), image(PE_TIMESTAMP, &[], &[])).unwrap();
        let found = locate(&dir.join("nativePC")).unwrap();
        assert_eq!(found.file_name().unwrap(), "ddda.EXE");
        let file = ExeFile::next_to(&dir.join("nativePC")).unwrap();
        assert!(file.image().is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
