//! Reader for MT Framework ARC archives as used by Dragon's Dogma: Dark
//! Arisen on PC (`ARC\0`, version 7).
//!
//! Layout (verified against all 8,536 archives of the Steam install):
//!
//! ```text
//! header   : magic "ARC\0", u16 version (7), u16 entry count
//! entry    : char path[64]   backslash path, NUL-padded, no extension
//!            u32 type hash   JAMCRC(resource class name) & 0x7FFF_FFFF
//!            u32 compressed size
//!            u32 size | flags << 29   (low 29 bits: decompressed size)
//!            u32 offset      absolute, from the start of the file
//! payload  : zlib streams
//! ```

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

mod classes;
pub use classes::RESOURCE_CLASSES;

pub const MAGIC: [u8; 4] = *b"ARC\0";
pub const VERSION: u16 = 7;
pub const HEADER_SIZE: usize = 8;
pub const ENTRY_SIZE: usize = 80;
pub const PATH_LEN: usize = 64;
const SIZE_MASK: u32 = (1 << 29) - 1;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    BadMagic([u8; 4]),
    BadVersion(u16),
    Truncated { needed: usize, have: usize },
    OutOfBounds { path: String, end: u64, file_len: u64 },
    Inflate { path: String, detail: String },
    SizeMismatch { path: String, expected: u32, got: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o: {e}"),
            Error::BadMagic(m) => write!(f, "not an ARC archive (magic {m:02x?})"),
            Error::BadVersion(v) => write!(f, "unsupported ARC version {v} (expected {VERSION})"),
            Error::Truncated { needed, have } => {
                write!(f, "truncated index: need {needed} bytes, have {have}")
            }
            Error::OutOfBounds { path, end, file_len } => {
                write!(f, "{path}: payload ends at {end}, file is {file_len} bytes")
            }
            Error::Inflate { path, detail } => write!(f, "{path}: zlib error: {detail}"),
            Error::SizeMismatch { path, expected, got } => {
                write!(f, "{path}: expected {expected} bytes after inflate, got {got}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path inside the archive, backslash-separated, without extension.
    /// Bytes are mapped one-to-one to chars (Latin-1) so the original
    /// bytes can always be recovered.
    pub path: String,
    pub type_hash: u32,
    pub compressed_size: u32,
    pub size: u32,
    /// Top three bits of the size field. Every entry in the PC install has 2.
    pub flags: u8,
    pub offset: u32,
}

impl Entry {
    /// The resource class name for this entry's type hash, if known.
    pub fn type_name(&self) -> Option<&'static str> {
        type_name(self.type_hash)
    }

    fn payload_end(&self) -> u64 {
        u64::from(self.offset) + u64::from(self.compressed_size)
    }
}

#[derive(Debug, Clone)]
pub struct Archive {
    pub version: u16,
    pub entries: Vec<Entry>,
}

impl Archive {
    /// Parses the header and entry table from the start of an archive.
    /// `bytes` needs to hold at least the header and the full table.
    pub fn parse_index(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::Truncated {
                needed: HEADER_SIZE,
                have: bytes.len(),
            });
        }
        let magic: [u8; 4] = bytes[0..4].try_into().unwrap();
        if magic != MAGIC {
            return Err(Error::BadMagic(magic));
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(Error::BadVersion(version));
        }
        let count = usize::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let needed = HEADER_SIZE + count * ENTRY_SIZE;
        if bytes.len() < needed {
            return Err(Error::Truncated {
                needed,
                have: bytes.len(),
            });
        }
        let entries = bytes[HEADER_SIZE..needed]
            .as_chunks::<ENTRY_SIZE>()
            .0
            .iter()
            .map(|e| parse_entry(e))
            .collect();
        Ok(Archive { version, entries })
    }

    /// Reads just the index of an archive file.
    pub fn read_index(file: &mut (impl Read + Seek)) -> Result<Self> {
        file.seek(SeekFrom::Start(0))?;
        let mut head = [0u8; HEADER_SIZE];
        file.read_exact(&mut head)?;
        let count = usize::from(u16::from_le_bytes([head[6], head[7]]));
        let mut buf = vec![0u8; HEADER_SIZE + count * ENTRY_SIZE];
        buf[..HEADER_SIZE].copy_from_slice(&head);
        file.read_exact(&mut buf[HEADER_SIZE..])?;
        Self::parse_index(&buf)
    }

    /// Opens an archive file and reads its index.
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, File)> {
        let mut file = File::open(path)?;
        let archive = Self::read_index(&mut file)?;
        Ok((archive, file))
    }

    /// Checks that every payload lies inside a file of `file_len` bytes.
    pub fn check_bounds(&self, file_len: u64) -> Result<()> {
        for e in &self.entries {
            if e.payload_end() > file_len {
                return Err(Error::OutOfBounds {
                    path: e.path.clone(),
                    end: e.payload_end(),
                    file_len,
                });
            }
        }
        Ok(())
    }
}

fn parse_entry(b: &[u8]) -> Entry {
    let name = &b[..PATH_LEN];
    let len = name.iter().position(|&c| c == 0).unwrap_or(PATH_LEN);
    let word = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let size_field = word(72);
    Entry {
        path: name[..len].iter().map(|&c| char::from(c)).collect(),
        type_hash: word(64),
        compressed_size: word(68),
        size: size_field & SIZE_MASK,
        flags: (size_field >> 29) as u8,
        offset: word(76),
    }
}

/// Reads and inflates one entry's payload.
pub fn read_entry(file: &mut (impl Read + Seek), entry: &Entry) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(u64::from(entry.offset)))?;
    let mut raw = vec![0u8; entry.compressed_size as usize];
    file.read_exact(&mut raw)?;
    decode_payload(entry, &raw)
}

/// Inflates a payload already read from the archive. Payloads without a
/// zlib header are returned as stored when their sizes agree.
pub fn decode_payload(entry: &Entry, raw: &[u8]) -> Result<Vec<u8>> {
    let is_zlib =
        raw.len() >= 2 && raw[0] & 0x0F == 8 && u16::from_be_bytes([raw[0], raw[1]]).is_multiple_of(31);
    let out = if is_zlib {
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(raw, entry.size as usize).map_err(|e| {
            Error::Inflate {
                path: entry.path.clone(),
                detail: format!("{:?}", e.status),
            }
        })?
    } else {
        raw.to_vec()
    };
    if out.len() != entry.size as usize {
        return Err(Error::SizeMismatch {
            path: entry.path.clone(),
            expected: entry.size,
            got: out.len(),
        });
    }
    Ok(out)
}

/// JAMCRC (bitwise-inverted CRC-32/IEEE) of `name`, masked to 31 bits: the
/// type hash MT Framework stores for a resource class.
pub fn type_hash(name: &str) -> u32 {
    !crc32(name.as_bytes()) & 0x7FFF_FFFF
}

/// Resource class name for a type hash, from the exe's class registry.
pub fn type_name(hash: u32) -> Option<&'static str> {
    static TABLE: OnceLock<Vec<(u32, &'static str)>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t: Vec<_> = RESOURCE_CLASSES.iter().map(|&n| (type_hash(n), n)).collect();
        t.sort_unstable();
        t
    });
    table
        .binary_search_by_key(&hash, |&(h, _)| h)
        .ok()
        .map(|i| table[i].1)
}

fn crc32(data: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    });
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn build(entries: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let payloads: Vec<Vec<u8>> = entries
            .iter()
            .map(|(_, _, d)| miniz_oxide::deflate::compress_to_vec_zlib(d, 6))
            .collect();
        let mut offset = (HEADER_SIZE + entries.len() * ENTRY_SIZE) as u32;
        for ((path, hash, data), packed) in entries.iter().zip(&payloads) {
            let mut name = [0u8; PATH_LEN];
            name[..path.len()].copy_from_slice(path.as_bytes());
            out.extend_from_slice(&name);
            out.extend_from_slice(&hash.to_le_bytes());
            out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            out.extend_from_slice(&((data.len() as u32) | (2 << 29)).to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += packed.len() as u32;
        }
        for p in payloads {
            out.extend_from_slice(&p);
        }
        out
    }

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn round_trip() {
        let tex = type_hash("rTexture");
        let bytes = build(&[("a\\b\\c", tex, b"hello"), ("d", 7, &[0u8; 300])]);
        let mut cur = Cursor::new(&bytes);
        let arc = Archive::read_index(&mut cur).unwrap();
        arc.check_bounds(bytes.len() as u64).unwrap();
        assert_eq!(arc.entries.len(), 2);
        let e = &arc.entries[0];
        assert_eq!((e.path.as_str(), e.flags, e.size), ("a\\b\\c", 2, 5));
        assert_eq!(e.type_name(), Some("rTexture"));
        assert_eq!(read_entry(&mut cur, e).unwrap(), b"hello");
        assert_eq!(read_entry(&mut cur, &arc.entries[1]).unwrap(), vec![0u8; 300]);
    }

    #[test]
    fn rejects_other_versions() {
        let mut bytes = build(&[]);
        bytes[4] = 8;
        assert!(matches!(Archive::parse_index(&bytes), Err(Error::BadVersion(8))));
    }

    #[test]
    fn detects_out_of_bounds_payload() {
        let bytes = build(&[("x", 1, b"data")]);
        let arc = Archive::parse_index(&bytes).unwrap();
        assert!(arc.check_bounds(bytes.len() as u64 - 1).is_err());
    }
}
