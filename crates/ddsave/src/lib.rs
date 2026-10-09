//! Read-only reader for `DDDA.sav`, the PC (Steam) save of Dragon's Dogma:
//! Dark Arisen. Layout and rules are documented in `docs/PAWNS.md`.
//!
//! A save is a 32-byte header, a zlib stream and zero padding. The stream
//! inflates to UTF-8 XML: the game serialises its save objects through MtDTI
//! property reflection, so every element carries the engine's own class and
//! field names (`sSave::saveDataAllDA`, `cSAVE_DATA_CMC`, `mRomPawnUniqueID`).
//!
//! - [`SaveFile::parse`] checks the header and checksum and inflates.
//! - [`SaveFile::document`] parses the XML into a borrowed tree ([`xml`]).
//! - [`pawn`] reads typed pawn records out of that tree.
//!
//! Nothing here writes a save.

pub mod pawn;
pub mod knowledge;
pub mod pawns;
pub mod xml;

use std::fmt;

/// Header length in bytes; the zlib stream starts right after it.
pub const HEADER_LEN: usize = 32;
/// Marker at header +0x0c, written by `0x0040f1a0`.
pub const MARK_0C: u32 = 0x334d_234d;
/// Marker at header +0x14, written by `0x0040f1a0`.
pub const MARK_14: u32 = 0x334d_4044;
/// Marker at header +0x1c (the last header word), written by `0x0040f1a0`.
pub const MARK_1C: u32 = 0x4056_5235;
/// Header version observed in the Steam save (equals the root's `mCreateNo`).
/// The writer `0x0040f1a0` takes it as an argument; the constant its caller
/// passes is not traced yet, so other values are accepted and reported.
pub const OBSERVED_VERSION: u32 = 21;

/// The 32-byte header.
///
/// Layout (little-endian u32s), from the writer `0x0040f1a0`:
/// `version, inflated_len, compressed_len, MARK_0C, 0, MARK_14, checksum, MARK_1C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub inflated_len: u32,
    pub compressed_len: u32,
    /// The word at +0x10; zero in every save written by `0x0040f1a0`.
    pub reserved: u32,
    pub checksum: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input ends before the named structure does.
    Truncated { what: &'static str, need: usize, have: usize },
    /// A header marker word is not the expected constant.
    Marker { offset: usize, found: u32, expected: u32 },
    /// The stored checksum does not match the compressed bytes.
    Checksum { stored: u32, computed: u32 },
    /// The zlib stream is invalid or inflates past the declared length.
    Inflate(String),
    /// The stream inflated to a different length than the header declares.
    InflatedLen { declared: u32, actual: usize },
    /// The inflated payload is not UTF-8.
    Utf8 { offset: usize },
    /// The XML is outside the subset the game writes.
    Xml { offset: usize, message: &'static str },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { what, need, have } => {
                write!(f, "truncated {what}: need {need} bytes, have {have}")
            }
            Error::Marker { offset, found, expected } => {
                write!(f, "header +{offset:#x}: {found:#010x}, expected {expected:#010x}")
            }
            Error::Checksum { stored, computed } => {
                write!(f, "checksum {stored:#010x} stored, {computed:#010x} computed")
            }
            Error::Inflate(e) => write!(f, "zlib: {e}"),
            Error::InflatedLen { declared, actual } => {
                write!(f, "inflated {actual} bytes, header declares {declared}")
            }
            Error::Utf8 { offset } => write!(f, "payload is not UTF-8 at byte {offset}"),
            Error::Xml { offset, message } => write!(f, "XML at byte {offset}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Save checksum: CRC-32 (reflected polynomial `0xEDB88320`, initial value
/// `0xFFFFFFFF`) **without** the final inversion, i.e. `!crc32(data)`.
///
/// Translated from `0x0040f1a0` (DDDA.exe 2017-12-13 Steam); the verifier
/// `0x0040f400` computes the same value. The table at `0x014075c0` is the
/// standard one (`0x77073096` at index 1). The exe processes aligned data a
/// word at a time but in memory byte order, so this bytewise form is exact.
pub fn checksum(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = (crc >> 8) ^ table[((crc ^ b as u32) & 0xff) as usize];
    }
    crc
}

impl Header {
    /// Reads and validates the header (markers only; lengths are checked by
    /// [`SaveFile::parse`]).
    ///
    /// The exe's reader (`0x0040f2c0`, `0x0040f600`) also accepts a
    /// byte-swapped (console) header and a legacy 12-byte header without
    /// markers; neither occurs in PC saves and both are rejected here.
    pub fn read(bytes: &[u8]) -> Result<Header, Error> {
        if bytes.len() < HEADER_LEN {
            return Err(Error::Truncated { what: "header", need: HEADER_LEN, have: bytes.len() });
        }
        for (offset, expected) in [(0x0c, MARK_0C), (0x14, MARK_14), (0x1c, MARK_1C)] {
            let found = u32_at(bytes, offset);
            if found != expected {
                return Err(Error::Marker { offset, found, expected });
            }
        }
        Ok(Header {
            version: u32_at(bytes, 0),
            inflated_len: u32_at(bytes, 4),
            compressed_len: u32_at(bytes, 8),
            reserved: u32_at(bytes, 0x10),
            checksum: u32_at(bytes, 0x18),
        })
    }
}

/// An inflated save: its header and the XML text.
#[derive(Debug, Clone)]
pub struct SaveFile {
    pub header: Header,
    pub xml: String,
    /// Bytes after the compressed stream (zero padding in PC saves).
    pub trailing_len: usize,
    /// Whether every trailing byte is zero.
    pub trailing_zero: bool,
}

impl SaveFile {
    /// Validates header and checksum, then inflates with the declared length
    /// as a hard limit.
    pub fn parse(bytes: &[u8]) -> Result<SaveFile, Error> {
        let header = Header::read(bytes)?;
        let end = HEADER_LEN + header.compressed_len as usize;
        if bytes.len() < end {
            return Err(Error::Truncated { what: "compressed stream", need: end, have: bytes.len() });
        }
        let stream = &bytes[HEADER_LEN..end];
        let computed = checksum(stream);
        if computed != header.checksum {
            return Err(Error::Checksum { stored: header.checksum, computed });
        }
        let limit = header.inflated_len as usize;
        let data = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stream, limit)
            .map_err(|e| Error::Inflate(format!("{:?}", e.status)))?;
        if data.len() != limit {
            return Err(Error::InflatedLen { declared: header.inflated_len, actual: data.len() });
        }
        let xml = String::from_utf8(data)
            .map_err(|e| Error::Utf8 { offset: e.utf8_error().valid_up_to() })?;
        let trailing = &bytes[end..];
        Ok(SaveFile {
            header,
            xml,
            trailing_len: trailing.len(),
            trailing_zero: trailing.iter().all(|&b| b == 0),
        })
    }

    /// Parses the XML into a tree borrowing from `self.xml`.
    pub fn document(&self) -> Result<xml::Document<'_>, Error> {
        xml::Document::parse(&self.xml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a save around `xml` the way `0x0040f1a0` lays it out.
    pub(crate) fn build(xml: &str) -> Vec<u8> {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(xml.as_bytes(), 6);
        let mut out = Vec::new();
        for w in [
            OBSERVED_VERSION,
            xml.len() as u32,
            z.len() as u32,
            MARK_0C,
            0,
            MARK_14,
            checksum(&z),
            MARK_1C,
        ] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend_from_slice(&z);
        out.resize(out.len() + 16, 0);
        out
    }

    #[test]
    fn checksum_is_crc32_without_final_xor() {
        // CRC-32 check value of "123456789" is 0xCBF43926.
        assert_eq!(checksum(b"123456789"), !0xCBF4_3926);
        assert_eq!(checksum(b""), 0xFFFF_FFFF);
    }

    #[test]
    fn round_trip_synthetic() {
        let xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<class name=\"x\" type=\"t\">\n<u32 name=\"a\" value=\"7\"/>\n</class>\n";
        let bytes = build(xml);
        let save = SaveFile::parse(&bytes).unwrap();
        assert_eq!(save.header.version, OBSERVED_VERSION);
        assert_eq!(save.xml, xml);
        assert_eq!((save.trailing_len, save.trailing_zero), (16, true));
        let doc = save.document().unwrap();
        assert_eq!(doc.root().get::<u32>("a"), Some(7));
    }

    #[test]
    fn corrupt_inputs_are_rejected() {
        let xml = "<class name=\"x\" type=\"t\">\n</class>\n";
        let good = build(xml);
        // Every truncation inside header or stream fails cleanly.
        let stream_end = HEADER_LEN + u32_at(&good, 8) as usize;
        for n in 0..stream_end {
            assert!(SaveFile::parse(&good[..n]).is_err(), "accepted {n} bytes");
        }
        // Wrong markers.
        for o in [0x0c, 0x14, 0x1c] {
            let mut b = good.clone();
            b[o] ^= 1;
            assert!(matches!(SaveFile::parse(&b), Err(Error::Marker { offset, .. }) if offset == o));
        }
        // A flipped stream byte fails the checksum before inflating.
        let mut b = good.clone();
        b[HEADER_LEN + 3] ^= 0x40;
        assert!(matches!(SaveFile::parse(&b), Err(Error::Checksum { .. })));
        // Declared inflated length too small or too large.
        for delta in [-1i64, 1] {
            let mut b = good.clone();
            let n = (xml.len() as i64 + delta) as u32;
            b[4..8].copy_from_slice(&n.to_le_bytes());
            assert!(SaveFile::parse(&b).is_err(), "accepted inflated len {n}");
        }
    }
}
